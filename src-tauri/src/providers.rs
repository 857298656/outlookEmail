use crate::error::{AppError, AppResult};
use crate::models::{AccountCredentials, AttachmentInfo, DownloadedAttachment, OAuthAuthUrlInput, ProviderMessage};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chrono::{DateTime, Utc};
use mailparse::MailHeaderMap;
use reqwest::blocking::Client;
use serde::Deserialize;
use std::time::Duration;

const GRAPH_SCOPE: &str = "offline_access Mail.ReadWrite User.Read";

pub struct OAuthTokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
    pub scope: String,
}

pub fn build_graph_auth_url(input: &OAuthAuthUrlInput) -> AppResult<String> {
    let client_id = input.client_id.trim();
    let redirect_uri = input.redirect_uri.trim();
    if client_id.is_empty() {
        return Err(AppError::InvalidInput("Microsoft client id is required".to_string()));
    }
    if redirect_uri.is_empty() {
        return Err(AppError::InvalidInput("OAuth redirect URI is required".to_string()));
    }

    let mut url = format!(
        "https://login.microsoftonline.com/common/oauth2/v2.0/authorize?client_id={}&response_type=code&redirect_uri={}&response_mode=query&scope={}&prompt=select_account",
        urlencoding::encode(client_id),
        urlencoding::encode(redirect_uri),
        urlencoding::encode(GRAPH_SCOPE)
    );
    if let Some(login_hint) = input.login_hint.as_ref().filter(|value| !value.trim().is_empty()) {
        url.push_str("&login_hint=");
        url.push_str(&urlencoding::encode(login_hint.trim()));
    }
    Ok(url)
}

pub fn exchange_graph_code(client_id: &str, redirect_uri: &str, code_or_url: &str) -> AppResult<OAuthTokenResponse> {
    let code = extract_code(code_or_url)?;
    let client = http_client()?;
    let response = client
        .post("https://login.microsoftonline.com/common/oauth2/v2.0/token")
        .form(&[
            ("client_id", client_id),
            ("redirect_uri", redirect_uri),
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("scope", GRAPH_SCOPE),
        ])
        .send()
        .map_err(network_error)?;
    parse_token_response(response)
}

pub fn refresh_graph_access_token(account: &AccountCredentials) -> AppResult<OAuthTokenResponse> {
    if account.client_id.trim().is_empty() || account.refresh_token.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "Graph account is missing client id or refresh token".to_string(),
        ));
    }
    let client = http_client()?;
    let response = client
        .post("https://login.microsoftonline.com/common/oauth2/v2.0/token")
        .form(&[
            ("client_id", account.client_id.as_str()),
            ("grant_type", "refresh_token"),
            ("refresh_token", account.refresh_token.as_str()),
            ("scope", GRAPH_SCOPE),
        ])
        .send()
        .map_err(network_error)?;
    parse_token_response(response)
}

pub fn fetch_graph_messages(account: &AccountCredentials, folder: &str, top: usize) -> AppResult<(String, Vec<ProviderMessage>)> {
    let token = refresh_graph_access_token(account)?;
    let folders = folders_for(folder);
    let client = http_client()?;
    let mut all = Vec::new();
    for folder_name in folders {
        let url = format!(
            "https://graph.microsoft.com/v1.0/me/mailFolders/{}/messages?$top={}&$orderby=receivedDateTime%20desc&$select=id,subject,from,toRecipients,ccRecipients,receivedDateTime,isRead,hasAttachments,bodyPreview,body",
            folder_name,
            top.clamp(1, 50)
        );
        let response = client
            .get(url)
            .bearer_auth(&token.access_token)
            .send()
            .map_err(network_error)?;
        if !response.status().is_success() {
            return Err(AppError::Internal(format!(
                "Graph list messages failed: HTTP {} {}",
                response.status(),
                response.text().unwrap_or_default()
            )));
        }
        let page: GraphMessagePage = response.json().map_err(network_error)?;
        for item in page.value {
            let mut message = item.into_provider_message(folder_name);
            if message.has_attachments {
                message.attachments = fetch_graph_attachments_metadata(&client, &token.access_token, &message.provider_message_id)?;
            }
            all.push(message);
        }
    }
    all.sort_by(|a, b| b.received_at_sort.total_cmp(&a.received_at_sort));
    Ok((token.refresh_token, all))
}

pub fn download_graph_attachment(
    account: &AccountCredentials,
    message_id: &str,
    attachment_id: &str,
) -> AppResult<DownloadedAttachment> {
    let token = refresh_graph_access_token(account)?;
    let client = http_client()?;
    let url = format!(
        "https://graph.microsoft.com/v1.0/me/messages/{}/attachments/{}",
        urlencoding::encode(message_id),
        urlencoding::encode(attachment_id)
    );
    let response = client
        .get(url)
        .bearer_auth(token.access_token)
        .send()
        .map_err(network_error)?;
    if !response.status().is_success() {
        return Err(AppError::Internal(format!(
            "Graph attachment download failed: HTTP {} {}",
            response.status(),
            response.text().unwrap_or_default()
        )));
    }
    let attachment: GraphAttachment = response.json().map_err(network_error)?;
    let content = attachment
        .content_bytes
        .ok_or_else(|| AppError::InvalidInput("Graph attachment has no inline content bytes".to_string()))?;
    let bytes = STANDARD
        .decode(content)
        .map_err(|err| AppError::Internal(format!("attachment base64 decode failed: {err}")))?;
    Ok(DownloadedAttachment {
        name: attachment.name.unwrap_or_else(|| "attachment.bin".to_string()),
        content_type: attachment.content_type.unwrap_or_default(),
        bytes,
    })
}

pub fn fetch_imap_messages(account: &AccountCredentials, folder: &str, top: usize) -> AppResult<Vec<ProviderMessage>> {
    let host = account.imap_host.trim();
    if host.is_empty() {
        return Err(AppError::InvalidInput("IMAP host is required".to_string()));
    }
    let password = if account.imap_password.trim().is_empty() {
        account.password.as_str()
    } else {
        account.imap_password.as_str()
    };
    if password.trim().is_empty() {
        return Err(AppError::InvalidInput("IMAP password is required".to_string()));
    }

    let tls = native_tls::TlsConnector::builder()
        .build()
        .map_err(|err| AppError::Internal(format!("TLS setup failed: {err}")))?;
    let client = imap::connect(
        (host, u16::try_from(account.imap_port).unwrap_or(993)),
        host,
        &tls,
    )
    .map_err(|err| AppError::Internal(format!("IMAP connect failed: {err}")))?;
    let mut session = client
        .login(account.email.as_str(), password)
        .map_err(|err| AppError::Internal(format!("IMAP login failed: {}", err.0)))?;

    let mut messages = Vec::new();
    for app_folder in folders_for(folder) {
        let mailbox = imap_mailbox_name(app_folder);
        let selected = match session.select(mailbox) {
            Ok(_) => true,
            Err(_) if app_folder != "inbox" => false,
            Err(err) => return Err(AppError::Internal(format!("IMAP select {mailbox} failed: {err}"))),
        };
        if !selected {
            continue;
        }
        let uids = session
            .uid_search("ALL")
            .map_err(|err| AppError::Internal(format!("IMAP search failed: {err}")))?;
        let mut selected_uids: Vec<u32> = uids.into_iter().collect();
        selected_uids.sort_unstable();
        selected_uids.reverse();
        selected_uids.truncate(top.clamp(1, 50));
        if selected_uids.is_empty() {
            continue;
        }
        let sequence = selected_uids
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let fetches = session
            .uid_fetch(sequence, "(RFC822 FLAGS INTERNALDATE)")
            .map_err(|err| AppError::Internal(format!("IMAP fetch failed: {err}")))?;
        for fetch in fetches.iter() {
            if let Some(body) = fetch.body() {
                messages.push(parse_imap_message(app_folder, fetch.uid.unwrap_or(fetch.message), body));
            }
        }
    }
    let _ = session.logout();
    messages.sort_by(|a, b| b.received_at_sort.total_cmp(&a.received_at_sort));
    Ok(messages)
}

fn http_client() -> AppResult<Client> {
    Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("OutlookEmailDesktop/0.1")
        .build()
        .map_err(network_error)
}

fn network_error(err: reqwest::Error) -> AppError {
    AppError::Internal(format!("network request failed: {err}"))
}

fn parse_token_response(response: reqwest::blocking::Response) -> AppResult<OAuthTokenResponse> {
    if !response.status().is_success() {
        return Err(AppError::Internal(format!(
            "OAuth token request failed: HTTP {} {}",
            response.status(),
            response.text().unwrap_or_default()
        )));
    }
    let token: GraphTokenWire = response.json().map_err(network_error)?;
    Ok(OAuthTokenResponse {
        access_token: token.access_token,
        refresh_token: token.refresh_token.unwrap_or_default(),
        expires_in: token.expires_in.unwrap_or_default(),
        scope: token.scope.unwrap_or_default(),
    })
}

fn fetch_graph_attachments_metadata(client: &Client, access_token: &str, message_id: &str) -> AppResult<Vec<AttachmentInfo>> {
    let url = format!(
        "https://graph.microsoft.com/v1.0/me/messages/{}/attachments?$select=id,name,contentType,size",
        urlencoding::encode(message_id)
    );
    let response = client
        .get(url)
        .bearer_auth(access_token)
        .send()
        .map_err(network_error)?;
    if !response.status().is_success() {
        return Ok(Vec::new());
    }
    let page: GraphAttachmentPage = response.json().map_err(network_error)?;
    Ok(page
        .value
        .into_iter()
        .map(|attachment| AttachmentInfo {
            id: attachment.id,
            name: attachment.name.unwrap_or_else(|| "attachment".to_string()),
            content_type: attachment.content_type.unwrap_or_default(),
            size: attachment.size.unwrap_or_default(),
        })
        .collect())
}

fn extract_code(code_or_url: &str) -> AppResult<String> {
    let value = code_or_url.trim();
    if value.is_empty() {
        return Err(AppError::InvalidInput("OAuth code or callback URL is required".to_string()));
    }
    if !value.starts_with("http://") && !value.starts_with("https://") {
        return Ok(value.to_string());
    }
    let marker = value
        .find("code=")
        .ok_or_else(|| AppError::InvalidInput("callback URL does not contain code".to_string()))?;
    let code = &value[(marker + 5)..];
    let code = code.split('&').next().unwrap_or(code);
    Ok(urlencoding::decode(code)
        .map_err(|err| AppError::InvalidInput(format!("invalid OAuth code encoding: {err}")))?
        .to_string())
}

fn folders_for(folder: &str) -> Vec<&'static str> {
    match folder.to_ascii_lowercase().as_str() {
        "inbox" => vec!["inbox"],
        "junk" | "junkemail" => vec!["junkemail"],
        "deleted" | "deleteditems" => vec!["deleteditems"],
        _ => vec!["inbox", "junkemail", "deleteditems"],
    }
}

fn imap_mailbox_name(folder: &str) -> &'static str {
    match folder {
        "junkemail" => "Junk",
        "deleteditems" => "Deleted",
        _ => "INBOX",
    }
}

fn parse_imap_message(folder: &str, uid: u32, body: &[u8]) -> ProviderMessage {
    let parsed = mailparse::parse_mail(body).ok();
    let subject = parsed
        .as_ref()
        .and_then(|mail| mail.headers.get_first_value("Subject"))
        .unwrap_or_else(|| "(no subject)".to_string());
    let sender = parsed
        .as_ref()
        .and_then(|mail| mail.headers.get_first_value("From"))
        .unwrap_or_default();
    let recipients = parsed
        .as_ref()
        .and_then(|mail| mail.headers.get_first_value("To"))
        .unwrap_or_default();
    let cc = parsed
        .as_ref()
        .and_then(|mail| mail.headers.get_first_value("Cc"))
        .unwrap_or_default();
    let date_header = parsed
        .as_ref()
        .and_then(|mail| mail.headers.get_first_value("Date"))
        .unwrap_or_default();
    let timestamp = mailparse::dateparse(&date_header).unwrap_or_else(|_| Utc::now().timestamp());
    let received = DateTime::<Utc>::from_timestamp(timestamp, 0)
        .unwrap_or_else(Utc::now)
        .to_rfc3339();
    let (body_text, body_type, attachments) = parsed
        .as_ref()
        .map(extract_body_and_attachments)
        .unwrap_or_else(|| (String::new(), "text".to_string(), Vec::new()));
    let preview = body_text
        .split_whitespace()
        .take(30)
        .collect::<Vec<_>>()
        .join(" ");

    ProviderMessage {
        folder: folder.to_string(),
        provider_message_id: uid.to_string(),
        subject,
        sender,
        recipients,
        cc,
        received_at: received,
        received_at_sort: timestamp as f64,
        is_read: false,
        has_attachments: !attachments.is_empty(),
        body_preview: preview,
        body: Some(body_text),
        body_type,
        attachments,
    }
}

fn extract_body_and_attachments(mail: &mailparse::ParsedMail<'_>) -> (String, String, Vec<AttachmentInfo>) {
    let mut text_body = String::new();
    let mut html_body = String::new();
    let mut attachments = Vec::new();
    collect_parts(mail, &mut text_body, &mut html_body, &mut attachments);
    if !html_body.is_empty() {
        (html_body, "html".to_string(), attachments)
    } else {
        (text_body, "text".to_string(), attachments)
    }
}

fn collect_parts(
    mail: &mailparse::ParsedMail<'_>,
    text_body: &mut String,
    html_body: &mut String,
    attachments: &mut Vec<AttachmentInfo>,
) {
    let disposition = mail.get_content_disposition();
    if disposition.disposition == mailparse::DispositionType::Attachment {
        let name = disposition
            .params
            .get("filename")
            .cloned()
            .unwrap_or_else(|| "attachment".to_string());
        attachments.push(AttachmentInfo {
            id: name.clone(),
            name,
            content_type: mail.ctype.mimetype.clone(),
            size: mail.get_body_raw().map(|bytes| bytes.len() as i64).unwrap_or_default(),
        });
        return;
    }
    if mail.subparts.is_empty() {
        if mail.ctype.mimetype.eq_ignore_ascii_case("text/html") && html_body.is_empty() {
            *html_body = mail.get_body().unwrap_or_default();
        } else if mail.ctype.mimetype.eq_ignore_ascii_case("text/plain") && text_body.is_empty() {
            *text_body = mail.get_body().unwrap_or_default();
        }
        return;
    }
    for part in &mail.subparts {
        collect_parts(part, text_body, html_body, attachments);
    }
}

#[derive(Debug, Deserialize)]
struct GraphTokenWire {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
    scope: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GraphMessagePage {
    value: Vec<GraphMessage>,
}

#[derive(Debug, Deserialize)]
struct GraphAttachmentPage {
    value: Vec<GraphAttachment>,
}

#[derive(Debug, Deserialize)]
struct GraphAttachment {
    id: String,
    name: Option<String>,
    #[serde(rename = "contentType")]
    content_type: Option<String>,
    size: Option<i64>,
    #[serde(rename = "contentBytes")]
    content_bytes: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GraphMessage {
    id: String,
    subject: Option<String>,
    from: Option<GraphRecipient>,
    #[serde(rename = "toRecipients")]
    to_recipients: Option<Vec<GraphRecipient>>,
    #[serde(rename = "ccRecipients")]
    cc_recipients: Option<Vec<GraphRecipient>>,
    #[serde(rename = "receivedDateTime")]
    received_date_time: Option<String>,
    #[serde(rename = "isRead")]
    is_read: Option<bool>,
    #[serde(rename = "hasAttachments")]
    has_attachments: Option<bool>,
    #[serde(rename = "bodyPreview")]
    body_preview: Option<String>,
    body: Option<GraphBody>,
}

impl GraphMessage {
    fn into_provider_message(self, folder: &str) -> ProviderMessage {
        let received_at = self.received_date_time.unwrap_or_else(|| Utc::now().to_rfc3339());
        let received_at_sort = DateTime::parse_from_rfc3339(&received_at)
            .map(|value| value.timestamp() as f64)
            .unwrap_or_else(|_| Utc::now().timestamp() as f64);
        let body = self.body.map(|body| body);
        let body_type = body
            .as_ref()
            .map(|body| body.content_type.to_ascii_lowercase())
            .unwrap_or_else(|| "text".to_string());
        let body_content = body.map(|body| body.content).filter(|value| !value.is_empty());
        ProviderMessage {
            folder: folder.to_string(),
            provider_message_id: self.id,
            subject: self.subject.unwrap_or_else(|| "(no subject)".to_string()),
            sender: self
                .from
                .and_then(|item| item.email_address)
                .map(|email| email.address)
                .unwrap_or_default(),
            recipients: recipients_to_string(self.to_recipients),
            cc: recipients_to_string(self.cc_recipients),
            received_at,
            received_at_sort,
            is_read: self.is_read.unwrap_or(false),
            has_attachments: self.has_attachments.unwrap_or(false),
            body_preview: self.body_preview.unwrap_or_default(),
            body: body_content,
            body_type,
            attachments: Vec::new(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct GraphBody {
    #[serde(rename = "contentType")]
    content_type: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct GraphRecipient {
    #[serde(rename = "emailAddress")]
    email_address: Option<GraphEmailAddress>,
}

#[derive(Debug, Deserialize)]
struct GraphEmailAddress {
    address: String,
}

fn recipients_to_string(value: Option<Vec<GraphRecipient>>) -> String {
    value
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| item.email_address.map(|email| email.address))
        .collect::<Vec<_>>()
        .join(", ")
}
