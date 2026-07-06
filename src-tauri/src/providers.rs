use crate::error::{AppError, AppResult};
use crate::models::{
    AccountCredentials, AttachmentInfo, CloudflareChannelCredential, DownloadedAttachment,
    GenerateTempEmailInput, OAuthAuthUrlInput, ProviderMessage, Settings, TempEmailCredential,
    TempEmailMessage,
};
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use chrono::{DateTime, Utc};
use imap::types::NameAttribute;
use mailparse::MailHeaderMap;
use reqwest::{blocking::Client, Proxy};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

const GRAPH_SCOPE: &str =
    "offline_access https://graph.microsoft.com/Mail.Read https://graph.microsoft.com/Mail.ReadWrite https://graph.microsoft.com/User.Read";
const IMAP_OAUTH_SCOPE: &str = "offline_access https://outlook.office.com/IMAP.AccessAsUser.All";
const GMAIL_MODIFY_SCOPE: &str = "https://www.googleapis.com/auth/gmail.modify";

#[derive(Clone, Copy)]
pub struct MailProviderDefinition {
    pub id: &'static str,
    pub credential_kind: &'static str,
    pub account_type: &'static str,
    pub default_imap_host: &'static str,
    pub default_imap_port: i64,
    aliases: &'static [&'static str],
    domains: &'static [&'static str],
}

pub const MAIL_PROVIDER_REGISTRY: &[MailProviderDefinition] = &[
    MailProviderDefinition {
        id: "graph",
        credential_kind: "oauth",
        account_type: "outlook",
        default_imap_host: "",
        default_imap_port: 993,
        aliases: &["outlook", "microsoft", "msgraph"],
        domains: &["outlook.com", "hotmail.com", "live.com", "msn.com"],
    },
    MailProviderDefinition {
        id: "gmail",
        credential_kind: "oauth",
        account_type: "gmail",
        default_imap_host: "",
        default_imap_port: 993,
        aliases: &["google", "googlemail"],
        domains: &["gmail.com", "googlemail.com"],
    },
    MailProviderDefinition {
        id: "qq",
        credential_kind: "imap_auth_code",
        account_type: "imap",
        default_imap_host: "imap.qq.com",
        default_imap_port: 993,
        aliases: &["qqmail"],
        domains: &["qq.com", "foxmail.com"],
    },
    MailProviderDefinition {
        id: "imap",
        credential_kind: "imap_password",
        account_type: "imap",
        default_imap_host: "",
        default_imap_port: 993,
        aliases: &["outlook_imap"],
        domains: &[],
    },
    MailProviderDefinition {
        id: "netease_163",
        credential_kind: "imap_auth_code",
        account_type: "imap",
        default_imap_host: "imap.163.com",
        default_imap_port: 993,
        aliases: &["163", "netease", "163mail"],
        domains: &["163.com"],
    },
    MailProviderDefinition {
        id: "imap_custom",
        credential_kind: "imap_password",
        account_type: "imap",
        default_imap_host: "",
        default_imap_port: 993,
        aliases: &["custom_imap", "custom"],
        domains: &[],
    },
];

pub struct OAuthTokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
    pub scope: String,
}

pub fn normalize_mail_provider_id(value: &str) -> Option<&'static str> {
    let provider = value.trim().to_ascii_lowercase();
    if provider.is_empty() {
        return None;
    }
    MAIL_PROVIDER_REGISTRY
        .iter()
        .find(|item| item.id == provider || item.aliases.iter().any(|alias| *alias == provider))
        .map(|item| item.id)
}

pub fn mail_provider_definition(value: &str) -> Option<&'static MailProviderDefinition> {
    let provider_id = normalize_mail_provider_id(value)?;
    MAIL_PROVIDER_REGISTRY.iter().find(|item| item.id == provider_id)
}

pub fn detect_mail_provider(email: &str, explicit_provider: Option<&str>, has_refresh_token: bool) -> AppResult<&'static MailProviderDefinition> {
    if let Some(provider) = explicit_provider.and_then(normalize_mail_provider_id) {
        return mail_provider_definition(provider)
            .ok_or_else(|| AppError::InvalidInput(format!("unsupported mail provider: {provider}")));
    }
    if has_refresh_token {
        return mail_provider_definition("graph")
            .ok_or_else(|| AppError::Internal("Graph provider definition is missing".to_string()));
    }

    let domain = email
        .trim()
        .to_ascii_lowercase()
        .rsplit_once('@')
        .map(|(_, domain)| domain.to_string())
        .unwrap_or_default();
    MAIL_PROVIDER_REGISTRY
        .iter()
        .find(|item| item.domains.iter().any(|known| *known == domain))
        .or_else(|| mail_provider_definition("imap_custom"))
        .ok_or_else(|| AppError::Internal("Custom IMAP provider definition is missing".to_string()))
}

fn microsoft_oauth_scope(provider: Option<&str>) -> &'static str {
    match provider.unwrap_or_default().trim().to_ascii_lowercase().as_str() {
        "imap" => IMAP_OAUTH_SCOPE,
        _ => GRAPH_SCOPE,
    }
}

pub fn build_graph_auth_url(input: &OAuthAuthUrlInput) -> AppResult<String> {
    let provider = normalize_oauth_provider(input.provider.as_deref())?;
    if provider == "gmail" {
        return build_google_auth_url(input);
    }

    let client_id = input.client_id.trim();
    let redirect_uri = input.redirect_uri.trim();
    if client_id.is_empty() {
        return Err(AppError::InvalidInput("Microsoft client id is required".to_string()));
    }
    if redirect_uri.is_empty() {
        return Err(AppError::InvalidInput("OAuth redirect URI is required".to_string()));
    }

    let scope = urlencoding::encode(microsoft_oauth_scope(Some(provider))).replace("%20", "+");
    let mut url = format!(
        "https://login.microsoftonline.com/common/oauth2/v2.0/authorize?client_id={}&response_type=code&redirect_uri={}&response_mode=query&scope={}&state=12345",
        urlencoding::encode(client_id),
        urlencoding::encode(redirect_uri),
        scope
    );
    if let Some(login_hint) = input.login_hint.as_ref().filter(|value| !value.trim().is_empty()) {
        url.push_str("&login_hint=");
        url.push_str(&urlencoding::encode(login_hint.trim()));
    }
    Ok(url)
}

fn build_google_auth_url(input: &OAuthAuthUrlInput) -> AppResult<String> {
    let client_id = input.client_id.trim();
    let redirect_uri = input.redirect_uri.trim();
    if client_id.is_empty() {
        return Err(AppError::InvalidInput("Google client id is required".to_string()));
    }
    if redirect_uri.is_empty() {
        return Err(AppError::InvalidInput("OAuth redirect URI is required".to_string()));
    }

    let code_verifier = input.code_verifier.as_deref().unwrap_or_default();
    let code_challenge = pkce_s256_challenge(code_verifier)?;
    let mut url = format!(
        "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&response_type=code&redirect_uri={}&scope={}&state=12345&access_type=offline&prompt=consent&code_challenge={}&code_challenge_method=S256",
        urlencoding::encode(client_id),
        urlencoding::encode(redirect_uri),
        urlencoding::encode(GMAIL_MODIFY_SCOPE),
        urlencoding::encode(&code_challenge)
    );
    if let Some(login_hint) = input.login_hint.as_ref().filter(|value| !value.trim().is_empty()) {
        url.push_str("&login_hint=");
        url.push_str(&urlencoding::encode(login_hint.trim()));
    }
    Ok(url)
}

pub fn exchange_microsoft_code(
    client_id: &str,
    redirect_uri: &str,
    code_or_url: &str,
    provider: Option<&str>,
) -> AppResult<OAuthTokenResponse> {
    let code = extract_code(code_or_url)?;
    let client = http_client()?;
    let response = client
        .post("https://login.microsoftonline.com/common/oauth2/v2.0/token")
        .form(&[
            ("client_id", client_id),
            ("redirect_uri", redirect_uri),
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("scope", microsoft_oauth_scope(provider)),
        ])
        .send()
        .map_err(network_error)?;
    parse_token_response(response)
}

pub fn exchange_google_code(
    client_id: &str,
    redirect_uri: &str,
    code_or_url: &str,
    code_verifier: Option<&str>,
) -> AppResult<OAuthTokenResponse> {
    let client_id = client_id.trim();
    let redirect_uri = redirect_uri.trim();
    if client_id.is_empty() {
        return Err(AppError::InvalidInput("Google client id is required".to_string()));
    }
    if redirect_uri.is_empty() {
        return Err(AppError::InvalidInput("OAuth redirect URI is required".to_string()));
    }
    let code = extract_code(code_or_url)?;
    let code_verifier = validate_pkce_verifier(code_verifier.unwrap_or_default())?;
    let client = http_client()?;
    let response = client
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("client_id", client_id),
            ("redirect_uri", redirect_uri),
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("code_verifier", code_verifier),
        ])
        .send()
        .map_err(network_error)?;
    parse_token_response(response)
}

fn normalize_oauth_provider(provider: Option<&str>) -> AppResult<&'static str> {
    let value = provider.unwrap_or_default().trim();
    if value.is_empty() {
        return Ok("graph");
    }
    match normalize_mail_provider_id(value) {
        Some("graph") => Ok("graph"),
        Some("imap") => Ok("imap"),
        Some("gmail") => Ok("gmail"),
        _ => Err(AppError::InvalidInput(format!("unsupported OAuth provider: {value}"))),
    }
}

fn validate_pkce_verifier(code_verifier: &str) -> AppResult<&str> {
    let value = code_verifier.trim();
    if value.is_empty() {
        return Err(AppError::InvalidInput(
            "Google OAuth PKCE code verifier is required".to_string(),
        ));
    }
    if !(43..=128).contains(&value.len()) {
        return Err(AppError::InvalidInput(
            "Google OAuth PKCE code verifier must be 43-128 characters".to_string(),
        ));
    }
    if !value
        .bytes()
        .all(|byte| matches!(byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~'))
    {
        return Err(AppError::InvalidInput(
            "Google OAuth PKCE code verifier contains invalid characters".to_string(),
        ));
    }
    Ok(value)
}

fn pkce_s256_challenge(code_verifier: &str) -> AppResult<String> {
    let value = validate_pkce_verifier(code_verifier)?;
    let digest = Sha256::digest(value.as_bytes());
    Ok(URL_SAFE_NO_PAD.encode(&digest[..]))
}

fn refresh_graph_access_token_with_client(account: &AccountCredentials, client: &Client) -> AppResult<OAuthTokenResponse> {
    refresh_microsoft_access_token_with_scope(account, client, GRAPH_SCOPE, "Graph")
}

fn refresh_gmail_access_token_with_client(account: &AccountCredentials, client: &Client) -> AppResult<OAuthTokenResponse> {
    if account.client_id.trim().is_empty() || account.refresh_token.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "Gmail account is missing client id or refresh token".to_string(),
        ));
    }
    let response = client
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("client_id", account.client_id.as_str()),
            ("grant_type", "refresh_token"),
            ("refresh_token", account.refresh_token.as_str()),
        ])
        .send()
        .map_err(network_error)?;
    parse_token_response(response)
}

fn refresh_imap_oauth_access_token(account: &AccountCredentials) -> AppResult<String> {
    with_account_http_client(account, |client| {
        refresh_microsoft_access_token_with_scope(account, client, IMAP_OAUTH_SCOPE, "IMAP").map(|token| token.access_token)
    })
}

fn refresh_microsoft_access_token_with_scope(
    account: &AccountCredentials,
    client: &Client,
    scope: &str,
    label: &str,
) -> AppResult<OAuthTokenResponse> {
    if account.client_id.trim().is_empty() || account.refresh_token.trim().is_empty() {
        return Err(AppError::InvalidInput(format!(
            "{label} account is missing client id or refresh token"
        )));
    }
    let response = client
        .post("https://login.microsoftonline.com/common/oauth2/v2.0/token")
        .form(&[
            ("client_id", account.client_id.as_str()),
            ("grant_type", "refresh_token"),
            ("refresh_token", account.refresh_token.as_str()),
            ("scope", scope),
        ])
        .send()
        .map_err(network_error)?;
    parse_token_response(response)
}

pub fn fetch_graph_messages(account: &AccountCredentials, folder: &str, top: usize) -> AppResult<(String, Vec<ProviderMessage>)> {
    with_account_http_client(account, |client| {
        let token = refresh_graph_access_token_with_client(account, client)?;
        let folders = folders_for(folder);
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
                    message.attachments = fetch_graph_attachments_metadata(client, &token.access_token, &message.provider_message_id)?;
                }
                all.push(message);
            }
        }
        all.sort_by(|a, b| b.received_at_sort.total_cmp(&a.received_at_sort));
        Ok((token.refresh_token, all))
    })
}

pub fn download_graph_attachment(
    account: &AccountCredentials,
    message_id: &str,
    attachment_id: &str,
) -> AppResult<DownloadedAttachment> {
    with_account_http_client(account, |client| {
        let token = refresh_graph_access_token_with_client(account, client)?;
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
    })
}

pub fn fetch_gmail_messages(account: &AccountCredentials, folder: &str, top: usize) -> AppResult<(String, Vec<ProviderMessage>)> {
    with_account_http_client(account, |client| {
        let token = refresh_gmail_access_token_with_client(account, client)?;
        let targets = gmail_folder_targets(folder);
        let mut all = Vec::new();
        for target in targets {
            let page = list_gmail_messages(client, &token.access_token, target.label_id, top.clamp(1, 50))?;
            for item in page.messages.unwrap_or_default() {
                let detail = fetch_gmail_message_detail(client, &token.access_token, &item.id)?;
                all.push(detail.into_provider_message(target.app_folder)?);
            }
        }
        all.sort_by(|a, b| b.received_at_sort.total_cmp(&a.received_at_sort));
        Ok((token.refresh_token, all))
    })
}

pub fn download_gmail_attachment(
    account: &AccountCredentials,
    message_id: &str,
    attachment_id: &str,
) -> AppResult<DownloadedAttachment> {
    let message_id = message_id.trim();
    let attachment_id = attachment_id.trim();
    if message_id.is_empty() || attachment_id.is_empty() {
        return Err(AppError::InvalidInput(
            "Gmail message id and attachment id are required".to_string(),
        ));
    }
    with_account_http_client(account, |client| {
        let token = refresh_gmail_access_token_with_client(account, client)?;
        let url = format!(
            "https://gmail.googleapis.com/gmail/v1/users/me/messages/{}/attachments/{}",
            urlencoding::encode(message_id),
            urlencoding::encode(attachment_id)
        );
        let response = client
            .get(url)
            .bearer_auth(&token.access_token)
            .send()
            .map_err(network_error)?;
        if !response.status().is_success() {
            return Err(AppError::Internal(format!(
                "Gmail attachment download failed: HTTP {} {}",
                response.status(),
                response.text().unwrap_or_default()
            )));
        }
        let body: GmailMessagePartBody = response.json().map_err(network_error)?;
        let bytes = decode_gmail_base64(body.data.as_deref().unwrap_or_default())?;
        let detail = fetch_gmail_message_detail(client, &token.access_token, message_id)?;
        let (name, content_type) = detail
            .payload
            .as_ref()
            .and_then(|payload| find_gmail_attachment_metadata(payload, attachment_id))
            .unwrap_or_else(|| ("attachment.bin".to_string(), String::new()));
        Ok(DownloadedAttachment {
            name,
            content_type,
            bytes,
        })
    })
}

pub fn mark_gmail_message_read(account: &AccountCredentials, message_id: &str, is_read: bool) -> AppResult<()> {
    let message_id = message_id.trim();
    if message_id.is_empty() {
        return Err(AppError::InvalidInput("Gmail message id is required".to_string()));
    }
    with_account_http_client(account, |client| {
        let token = refresh_gmail_access_token_with_client(account, client)?;
        let url = format!(
            "https://gmail.googleapis.com/gmail/v1/users/me/messages/{}/modify",
            urlencoding::encode(message_id)
        );
        let response = client
            .post(url)
            .bearer_auth(&token.access_token)
            .json(&gmail_read_state_payload(is_read))
            .send()
            .map_err(network_error)?;
        if !response.status().is_success() {
            return Err(AppError::Internal(format!(
                "Gmail mark message failed: HTTP {} {}",
                response.status(),
                response.text().unwrap_or_default()
            )));
        }
        Ok(())
    })
}

pub fn delete_gmail_message(account: &AccountCredentials, message_id: &str) -> AppResult<()> {
    let message_id = message_id.trim();
    if message_id.is_empty() {
        return Err(AppError::InvalidInput("Gmail message id is required".to_string()));
    }
    with_account_http_client(account, |client| {
        let token = refresh_gmail_access_token_with_client(account, client)?;
        let url = format!(
            "https://gmail.googleapis.com/gmail/v1/users/me/messages/{}/trash",
            urlencoding::encode(message_id)
        );
        let response = client
            .post(url)
            .bearer_auth(&token.access_token)
            .send()
            .map_err(network_error)?;
        if !response.status().is_success() {
            return Err(AppError::Internal(format!(
                "Gmail trash message failed: HTTP {} {}",
                response.status(),
                response.text().unwrap_or_default()
            )));
        }
        Ok(())
    })
}

pub fn download_imap_attachment_from_raw(raw_mime: &[u8], attachment_id: &str) -> AppResult<DownloadedAttachment> {
    let requested_id = attachment_id.trim();
    if requested_id.is_empty() {
        return Err(AppError::InvalidInput("attachment id is required".to_string()));
    }
    let parsed = mailparse::parse_mail(raw_mime)
        .map_err(|err| AppError::InvalidInput(format!("cached IMAP MIME could not be parsed: {err}")))?;
    let mut attachments = Vec::new();
    collect_downloadable_parts(&parsed, &mut attachments);
    attachments
        .into_iter()
        .find(|attachment| attachment.id == requested_id || attachment.name == requested_id)
        .map(|attachment| DownloadedAttachment {
            name: attachment.name,
            content_type: attachment.content_type,
            bytes: attachment.bytes,
        })
        .ok_or_else(|| AppError::InvalidInput("attachment was not found in cached IMAP MIME".to_string()))
}

pub fn mark_graph_message_read(account: &AccountCredentials, message_id: &str, is_read: bool) -> AppResult<()> {
    with_account_http_client(account, |client| {
        let token = refresh_graph_access_token_with_client(account, client)?;
        let url = format!(
            "https://graph.microsoft.com/v1.0/me/messages/{}",
            urlencoding::encode(message_id)
        );
        let response = client
            .patch(url)
            .bearer_auth(token.access_token)
            .json(&json!({ "isRead": is_read }))
            .send()
            .map_err(network_error)?;
        if !response.status().is_success() {
            return Err(AppError::Internal(format!(
                "Graph mark message failed: HTTP {} {}",
                response.status(),
                response.text().unwrap_or_default()
            )));
        }
        Ok(())
    })
}

pub fn delete_graph_message(account: &AccountCredentials, message_id: &str) -> AppResult<()> {
    with_account_http_client(account, |client| {
        let token = refresh_graph_access_token_with_client(account, client)?;
        let url = format!(
            "https://graph.microsoft.com/v1.0/me/messages/{}",
            urlencoding::encode(message_id)
        );
        let response = client
            .delete(url)
            .bearer_auth(token.access_token)
            .send()
            .map_err(network_error)?;
        if !response.status().is_success() {
            return Err(AppError::Internal(format!(
                "Graph delete message failed: HTTP {} {}",
                response.status(),
                response.text().unwrap_or_default()
            )));
        }
        Ok(())
    })
}

pub fn fetch_imap_messages(account: &AccountCredentials, folder: &str, top: usize) -> AppResult<Vec<ProviderMessage>> {
    with_imap_session(account, |session| {
        let mut messages = Vec::new();
        for target in imap_mailbox_targets(session, folder) {
            let mailbox = target.mailbox.as_str();
            let selected = match session.select(mailbox) {
                Ok(_) => true,
                Err(_) if target.app_folder != "inbox" => false,
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
                    messages.push(parse_imap_message(target.app_folder, fetch.uid.unwrap_or(fetch.message), body));
                }
            }
        }
        messages.sort_by(|a, b| b.received_at_sort.total_cmp(&a.received_at_sort));
        Ok(messages)
    })
}

pub fn mark_imap_message_read(account: &AccountCredentials, folder: &str, message_id: &str, is_read: bool) -> AppResult<()> {
    mutate_imap_message(account, folder, message_id, if is_read { "+FLAGS (\\Seen)" } else { "-FLAGS (\\Seen)" }, false)
}

pub fn delete_imap_message(account: &AccountCredentials, folder: &str, message_id: &str) -> AppResult<()> {
    mutate_imap_message(account, folder, message_id, "+FLAGS (\\Deleted)", true)
}

pub fn generate_temp_email(settings: &Settings, input: &GenerateTempEmailInput, channel: Option<&CloudflareChannelCredential>) -> AppResult<TempEmailCredential> {
    match input.provider.to_ascii_lowercase().as_str() {
        "gptmail" => generate_gptmail_address(settings, input),
        "duckmail" => generate_duckmail_address(settings, input),
        "cloudflare" => generate_cloudflare_address(input, channel),
        value => Err(AppError::InvalidInput(format!("unsupported temp email provider: {value}"))),
    }
}

pub fn fetch_temp_messages(
    settings: &Settings,
    temp_email: &TempEmailCredential,
    channel: Option<&CloudflareChannelCredential>,
    limit: usize,
) -> AppResult<Vec<TempEmailMessage>> {
    match temp_email.provider.to_ascii_lowercase().as_str() {
        "gptmail" => fetch_gptmail_messages(settings, &temp_email.email),
        "duckmail" => fetch_duckmail_messages(settings, temp_email),
        "cloudflare" => fetch_cloudflare_messages(temp_email, channel, limit),
        value => Err(AppError::InvalidInput(format!("unsupported temp email provider: {value}"))),
    }
}

pub fn delete_temp_remote(temp_email: &TempEmailCredential, channel: Option<&CloudflareChannelCredential>) -> AppResult<bool> {
    match temp_email.provider.to_ascii_lowercase().as_str() {
        "cloudflare" => {
            let Some(channel) = channel else {
                return Ok(false);
            };
            let address_id = temp_email.provider_account_id.trim();
            if address_id.is_empty() {
                return Ok(false);
            }
            let endpoint = format!("/admin/delete_address/{}", urlencoding::encode(address_id));
            cloudflare_request("DELETE", channel, &endpoint, None, None).map(|_| true)
        }
        _ => Ok(false),
    }
}

pub fn test_cloudflare_channel(channel: &CloudflareChannelCredential) -> AppResult<String> {
    let value = cloudflare_request("GET", channel, "/admin/address", Some(&[("limit", "1"), ("offset", "0")]), None)?;
    let count = extract_array(&value, &["results", "addresses", "data"])
        .map(|items| items.len())
        .unwrap_or_default();
    Ok(format!("Cloudflare channel connected, sample addresses: {count}"))
}

fn http_client() -> AppResult<Client> {
    http_client_for_proxy(None)
}

fn http_client_for_proxy(proxy_url: Option<&str>) -> AppResult<Client> {
    let mut builder = Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("OutlookEmailDesktop/0.1");
    if let Some(proxy_url) = proxy_url.filter(|value| !value.trim().is_empty()) {
        let proxy = Proxy::all(proxy_url.trim())
            .map_err(|err| AppError::InvalidInput(format!("invalid proxy URL {proxy_url}: {err}")))?;
        builder = builder.proxy(proxy);
    }
    builder
        .build()
        .map_err(network_error)
}

fn with_account_http_client<T>(
    account: &AccountCredentials,
    operation: impl Fn(&Client) -> AppResult<T>,
) -> AppResult<T> {
    if account.proxy_chain.is_empty() {
        let client = http_client()?;
        return operation(&client);
    }

    let mut last_error = String::new();
    for proxy_url in &account.proxy_chain {
        let client = http_client_for_proxy(Some(proxy_url))?;
        match operation(&client) {
            Ok(value) => return Ok(value),
            Err(err) => {
                last_error = format!("{err}");
            }
        }
    }
    Err(AppError::Internal(format!(
        "all proxy attempts failed for {}: {}",
        account.email,
        if last_error.is_empty() { "unknown network error" } else { last_error.as_str() }
    )))
}

fn network_error(err: reqwest::Error) -> AppError {
    AppError::Internal(format!("network request failed: {err}"))
}

fn generate_gptmail_address(settings: &Settings, input: &GenerateTempEmailInput) -> AppResult<TempEmailCredential> {
    let base_url = settings.gptmail_base_url.trim().trim_end_matches('/');
    if base_url.is_empty() {
        return Err(AppError::InvalidInput("GPTMail base URL is required".to_string()));
    }
    let client = http_client()?;
    let mut request = if input.prefix.as_deref().unwrap_or_default().trim().is_empty()
        && input.domain.as_deref().unwrap_or_default().trim().is_empty()
    {
        client.get(format!("{base_url}/api/generate-email"))
    } else {
        client.post(format!("{base_url}/api/generate-email")).json(&json!({
            "prefix": input.prefix.as_deref().unwrap_or_default().trim(),
            "domain": input.domain.as_deref().unwrap_or_default().trim()
        }))
    };
    if !settings.gptmail_api_key.trim().is_empty() {
        request = request.header("X-API-Key", settings.gptmail_api_key.trim());
    }
    let value = send_json(request, "GPTMail generate email")?;
    let email = string_path(&value, &["data", "email"])
        .or_else(|| string_path(&value, &["email"]))
        .ok_or_else(|| AppError::Internal("GPTMail response did not include an email".to_string()))?;
    Ok(TempEmailCredential {
        id: 0,
        email,
        provider: "gptmail".to_string(),
        channel_id: None,
        provider_token: String::new(),
        provider_account_id: String::new(),
        provider_password: String::new(),
    })
}

fn generate_duckmail_address(settings: &Settings, input: &GenerateTempEmailInput) -> AppResult<TempEmailCredential> {
    let base_url = settings.duckmail_base_url.trim().trim_end_matches('/');
    let username = input.username.as_deref().unwrap_or_default().trim();
    let domain = input.domain.as_deref().unwrap_or_default().trim().trim_start_matches('@');
    let password = input.password.as_deref().unwrap_or_default().trim();
    if base_url.is_empty() || username.is_empty() || domain.is_empty() || password.len() < 6 {
        return Err(AppError::InvalidInput(
            "DuckMail requires base URL, username, domain, and a 6+ character password".to_string(),
        ));
    }
    let email = format!("{username}@{domain}").to_ascii_lowercase();
    let client = http_client()?;
    let mut create = client
        .post(format!("{base_url}/accounts"))
        .json(&json!({ "address": email, "password": password }));
    if !settings.duckmail_api_key.trim().is_empty() {
        create = create.bearer_auth(settings.duckmail_api_key.trim());
    }
    let account = send_json(create, "DuckMail create account")?;
    let account_id = string_path(&account, &["id"]).unwrap_or_default();
    let token = duckmail_token(settings, &email, password)?;
    Ok(TempEmailCredential {
        id: 0,
        email,
        provider: "duckmail".to_string(),
        channel_id: None,
        provider_token: token,
        provider_account_id: account_id,
        provider_password: password.to_string(),
    })
}

fn generate_cloudflare_address(input: &GenerateTempEmailInput, channel: Option<&CloudflareChannelCredential>) -> AppResult<TempEmailCredential> {
    let channel = channel.ok_or_else(|| AppError::InvalidInput("Cloudflare channel is required".to_string()))?;
    if !channel.enabled {
        return Err(AppError::InvalidInput("Cloudflare channel is disabled".to_string()));
    }
    let username = input
        .username
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(random_temp_username);
    let domain = input
        .domain
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.trim_start_matches('@').to_ascii_lowercase())
        .or_else(|| channel.email_domains.first().cloned())
        .ok_or_else(|| AppError::InvalidInput("Cloudflare channel has no email domain".to_string()))?;
    let value = cloudflare_request(
        "POST",
        channel,
        "/admin/new_address",
        None,
        Some(json!({
            "enablePrefix": true,
            "name": username,
            "domain": domain
        })),
    )?;
    let email = string_path(&value, &["address"])
        .or_else(|| string_path(&value, &["email"]))
        .ok_or_else(|| AppError::Internal("Cloudflare response did not include an address".to_string()))?;
    let address_id = string_path(&value, &["id"])
        .or_else(|| string_path(&value, &["address_id"]))
        .unwrap_or_default();
    Ok(TempEmailCredential {
        id: 0,
        email: email.to_ascii_lowercase(),
        provider: "cloudflare".to_string(),
        channel_id: Some(channel.id),
        provider_token: String::new(),
        provider_account_id: address_id,
        provider_password: String::new(),
    })
}

fn fetch_gptmail_messages(settings: &Settings, email: &str) -> AppResult<Vec<TempEmailMessage>> {
    let base_url = settings.gptmail_base_url.trim().trim_end_matches('/');
    if base_url.is_empty() {
        return Err(AppError::InvalidInput("GPTMail base URL is required".to_string()));
    }
    let client = http_client()?;
    let mut request = client.get(format!("{base_url}/api/emails")).query(&[("email", email)]);
    if !settings.gptmail_api_key.trim().is_empty() {
        request = request.header("X-API-Key", settings.gptmail_api_key.trim());
    }
    let value = send_json(request, "GPTMail list messages")?;
    let items = extract_array(&value, &["data.emails", "emails", "data", "results"]).unwrap_or_default();
    Ok(items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| temp_message_from_json(email, item, index))
        .collect())
}

fn fetch_duckmail_messages(settings: &Settings, temp_email: &TempEmailCredential) -> AppResult<Vec<TempEmailMessage>> {
    let token = if temp_email.provider_token.trim().is_empty() {
        duckmail_token(settings, &temp_email.email, &temp_email.provider_password)?
    } else {
        temp_email.provider_token.clone()
    };
    let base_url = settings.duckmail_base_url.trim().trim_end_matches('/');
    let value = send_json(
        http_client()?
            .get(format!("{base_url}/messages"))
            .bearer_auth(token)
            .query(&[("page", "1")]),
        "DuckMail list messages",
    )?;
    let items = extract_array(&value, &["hydra:member", "messages", "data", "results"]).unwrap_or_default();
    Ok(items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| temp_message_from_json(&temp_email.email, item, index))
        .collect())
}

fn fetch_cloudflare_messages(
    temp_email: &TempEmailCredential,
    channel: Option<&CloudflareChannelCredential>,
    limit: usize,
) -> AppResult<Vec<TempEmailMessage>> {
    let channel = channel.ok_or_else(|| AppError::InvalidInput("Cloudflare channel is required".to_string()))?;
    let limit_text = limit.clamp(1, 100).to_string();
    let value = cloudflare_request(
        "GET",
        channel,
        "/admin/mails",
        Some(&[
            ("limit", limit_text.as_str()),
            ("offset", "0"),
            ("address", temp_email.email.as_str()),
        ]),
        None,
    )?;
    let items = extract_array(&value, &["results", "mails", "emails", "data.results", "data"]).unwrap_or_default();
    Ok(items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| cloudflare_message_from_json(&temp_email.email, item, index))
        .collect())
}

fn duckmail_token(settings: &Settings, email: &str, password: &str) -> AppResult<String> {
    if password.trim().is_empty() {
        return Err(AppError::InvalidInput("DuckMail password is required".to_string()));
    }
    let base_url = settings.duckmail_base_url.trim().trim_end_matches('/');
    let value = send_json(
        http_client()?
            .post(format!("{base_url}/token"))
            .json(&json!({ "address": email, "password": password })),
        "DuckMail token",
    )?;
    string_path(&value, &["token"]).ok_or_else(|| AppError::Internal("DuckMail token response is missing token".to_string()))
}

fn cloudflare_request(
    method: &str,
    channel: &CloudflareChannelCredential,
    endpoint: &str,
    query: Option<&[(&str, &str)]>,
    body: Option<Value>,
) -> AppResult<Value> {
    let worker = channel.worker_domain.trim().trim_end_matches('/');
    if worker.is_empty() || channel.admin_password.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "Cloudflare worker domain and admin password are required".to_string(),
        ));
    }
    let base = if worker.starts_with("http://") || worker.starts_with("https://") {
        worker.to_string()
    } else {
        format!("https://{worker}")
    };
    let url = format!("{base}{endpoint}");
    let client = http_client()?;
    let mut request = match method {
        "GET" => client.get(url),
        "POST" => client.post(url),
        "DELETE" => client.delete(url),
        _ => return Err(AppError::InvalidInput(format!("unsupported Cloudflare method: {method}"))),
    }
    .header("x-admin-auth", channel.admin_password.trim());
    if let Some(query) = query {
        request = request.query(query);
    }
    if let Some(body) = body {
        request = request.json(&body);
    }
    send_json(request, "Cloudflare temp email")
}

fn send_json(request: reqwest::blocking::RequestBuilder, label: &str) -> AppResult<Value> {
    let response = request.send().map_err(network_error)?;
    let status = response.status();
    let text = response.text().map_err(network_error)?;
    if !status.is_success() {
        return Err(AppError::Internal(format!("{label} failed: HTTP {status} {text}")));
    }
    if text.trim().is_empty() {
        return Ok(json!({ "success": true }));
    }
    let value: Value = serde_json::from_str(&text)
        .map_err(|err| AppError::Internal(format!("{label} response is not valid JSON: {err}")))?;
    if value.get("success").and_then(Value::as_bool) == Some(false) {
        let error = string_path(&value, &["error"])
            .or_else(|| string_path(&value, &["message"]))
            .unwrap_or_else(|| "request failed".to_string());
        return Err(AppError::Internal(format!("{label} failed: {error}")));
    }
    Ok(value)
}

fn temp_message_from_json(email: &str, item: &Value, index: usize) -> Option<TempEmailMessage> {
    let message_id = string_path(item, &["id"])
        .or_else(|| string_path(item, &["message_id"]))
        .unwrap_or_else(|| format!("{email}-{index}"));
    let from_address = string_path(item, &["from.address"])
        .or_else(|| string_path(item, &["from"]))
        .or_else(|| string_path(item, &["from_address"]))
        .unwrap_or_default();
    let subject = string_path(item, &["subject"]).unwrap_or_else(|| "(no subject)".to_string());
    let html_content = string_path(item, &["html.0"])
        .or_else(|| string_path(item, &["html_content"]))
        .or_else(|| string_path(item, &["html"]))
        .unwrap_or_default();
    let content = string_path(item, &["text"])
        .or_else(|| string_path(item, &["content"]))
        .or_else(|| string_path(item, &["body"]))
        .unwrap_or_default();
    Some(TempEmailMessage {
        id: 0,
        message_id,
        email_address: email.to_string(),
        from_address,
        subject,
        content,
        html_content: html_content.clone(),
        has_html: !html_content.trim().is_empty(),
        timestamp: timestamp_from_value(item).unwrap_or_else(|| Utc::now().timestamp()),
        raw_content: string_path(item, &["raw"]).or_else(|| string_path(item, &["raw_content"])).unwrap_or_default(),
        created_at: String::new(),
    })
}

fn cloudflare_message_from_json(email: &str, item: &Value, index: usize) -> Option<TempEmailMessage> {
    if let Some(raw) = string_path(item, &["raw"]).or_else(|| string_path(item, &["raw_content"])).or_else(|| string_path(item, &["source_raw"])) {
        return Some(parse_raw_temp_message(
            email,
            &raw,
            string_path(item, &["id"]).unwrap_or_else(|| format!("{email}-cf-{index}")),
            timestamp_from_value(item).unwrap_or_else(|| Utc::now().timestamp()),
        ));
    }
    temp_message_from_json(email, item, index)
}

fn parse_raw_temp_message(email: &str, raw: &str, fallback_id: String, timestamp: i64) -> TempEmailMessage {
    let parsed = mailparse::parse_mail(raw.as_bytes()).ok();
    let subject = parsed
        .as_ref()
        .and_then(|mail| mail.headers.get_first_value("Subject"))
        .unwrap_or_else(|| "(no subject)".to_string());
    let from_address = parsed
        .as_ref()
        .and_then(|mail| mail.headers.get_first_value("From"))
        .unwrap_or_default();
    let (body, body_type, _) = parsed
        .as_ref()
        .map(extract_body_and_attachments)
        .unwrap_or_else(|| (String::new(), "text".to_string(), Vec::new()));
    let has_html = body_type == "html";
    TempEmailMessage {
        id: 0,
        message_id: fallback_id,
        email_address: email.to_string(),
        from_address,
        subject,
        content: if has_html { String::new() } else { body.clone() },
        html_content: if has_html { body } else { String::new() },
        has_html,
        timestamp,
        raw_content: raw.to_string(),
        created_at: String::new(),
    }
}

fn extract_array(value: &Value, paths: &[&str]) -> Option<Vec<Value>> {
    for path in paths {
        if let Some(array) = value_at_path(value, path).and_then(Value::as_array) {
            return Some(array.clone());
        }
    }
    None
}

fn string_path(value: &Value, paths: &[&str]) -> Option<String> {
    for path in paths {
        let Some(value) = value_at_path(value, path) else {
            continue;
        };
        if let Some(text) = value.as_str() {
            if !text.trim().is_empty() {
                return Some(text.to_string());
            }
        } else if value.is_number() || value.is_boolean() {
            return Some(value.to_string());
        }
    }
    None
}

fn value_at_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = value;
    for part in path.split('.') {
        if let Ok(index) = part.parse::<usize>() {
            current = current.as_array()?.get(index)?;
        } else {
            current = current.get(part)?;
        }
    }
    Some(current)
}

fn timestamp_from_value(value: &Value) -> Option<i64> {
    for path in ["timestamp", "created_at", "createdAt", "date"] {
        let value = value_at_path(value, path)?;
        if let Some(number) = value.as_i64() {
            return Some(number);
        }
        let text = value.as_str()?;
        if let Ok(number) = text.parse::<i64>() {
            return Some(number);
        }
        if let Ok(parsed) = DateTime::parse_from_rfc3339(text) {
            return Some(parsed.timestamp());
        }
    }
    None
}

fn random_temp_username() -> String {
    format!("oe{}", uuid::Uuid::new_v4().simple().to_string()[..12].to_string())
}

fn parse_token_response(response: reqwest::blocking::Response) -> AppResult<OAuthTokenResponse> {
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        return Err(oauth_token_error(status, &body));
    }
    let token: GraphTokenWire = response.json().map_err(network_error)?;
    Ok(OAuthTokenResponse {
        access_token: token.access_token,
        refresh_token: token.refresh_token.unwrap_or_default(),
        expires_in: token.expires_in.unwrap_or_default(),
        scope: token.scope.unwrap_or_default(),
    })
}

fn oauth_token_error(status: reqwest::StatusCode, body: &str) -> AppError {
    let parsed = serde_json::from_str::<Value>(body).ok();
    let error = parsed
        .as_ref()
        .and_then(|value| value.get("error"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let description = parsed
        .as_ref()
        .and_then(|value| value.get("error_description"))
        .and_then(Value::as_str)
        .unwrap_or(body);
    let lower = format!("{error} {description}").to_ascii_lowercase();

    if lower.contains("aadsts70000")
        || lower.contains("code has expired")
        || lower.contains("provided value for the 'code' parameter is not valid")
    {
        return AppError::InvalidInput(
            "OAuth 授权码已过期或已被使用，请重新点击“打开”完成授权，然后粘贴新的回调 URL。".to_string(),
        );
    }

    if lower.contains("invalid_grant") {
        return AppError::InvalidInput(
            "OAuth 授权码无效，请重新打开授权页面并复制新的完整回调 URL。".to_string(),
        );
    }

    AppError::Internal(format!("OAuth token request failed: HTTP {status} {description}"))
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

fn list_gmail_messages(
    client: &Client,
    access_token: &str,
    label_id: &str,
    top: usize,
) -> AppResult<GmailMessageList> {
    let response = client
        .get("https://gmail.googleapis.com/gmail/v1/users/me/messages")
        .bearer_auth(access_token)
        .query(&[
            ("maxResults", top.to_string()),
            ("labelIds", label_id.to_string()),
            ("includeSpamTrash", "true".to_string()),
        ])
        .send()
        .map_err(network_error)?;
    if !response.status().is_success() {
        return Err(AppError::Internal(format!(
            "Gmail list messages failed: HTTP {} {}",
            response.status(),
            response.text().unwrap_or_default()
        )));
    }
    response.json().map_err(network_error)
}

fn fetch_gmail_message_detail(client: &Client, access_token: &str, message_id: &str) -> AppResult<GmailMessage> {
    let url = format!(
        "https://gmail.googleapis.com/gmail/v1/users/me/messages/{}",
        urlencoding::encode(message_id)
    );
    let response = client
        .get(url)
        .bearer_auth(access_token)
        .query(&[("format", "full")])
        .send()
        .map_err(network_error)?;
    if !response.status().is_success() {
        return Err(AppError::Internal(format!(
            "Gmail get message failed: HTTP {} {}",
            response.status(),
            response.text().unwrap_or_default()
        )));
    }
    response.json().map_err(network_error)
}

fn gmail_read_state_payload(is_read: bool) -> Value {
    if is_read {
        json!({ "removeLabelIds": ["UNREAD"] })
    } else {
        json!({ "addLabelIds": ["UNREAD"] })
    }
}

fn extract_code(code_or_url: &str) -> AppResult<String> {
    let compact = code_or_url.split_whitespace().collect::<String>();
    let value = compact.trim();
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

struct GmailFolderTarget {
    app_folder: &'static str,
    label_id: &'static str,
}

fn gmail_folder_targets(folder: &str) -> Vec<GmailFolderTarget> {
    match folder.to_ascii_lowercase().as_str() {
        "inbox" => vec![GmailFolderTarget {
            app_folder: "inbox",
            label_id: "INBOX",
        }],
        "junk" | "junkemail" => vec![GmailFolderTarget {
            app_folder: "junkemail",
            label_id: "SPAM",
        }],
        "deleted" | "deleteditems" => vec![GmailFolderTarget {
            app_folder: "deleteditems",
            label_id: "TRASH",
        }],
        _ => vec![
            GmailFolderTarget {
                app_folder: "inbox",
                label_id: "INBOX",
            },
            GmailFolderTarget {
                app_folder: "junkemail",
                label_id: "SPAM",
            },
            GmailFolderTarget {
                app_folder: "deleteditems",
                label_id: "TRASH",
            },
        ],
    }
}

#[derive(Debug, Clone)]
struct ImapMailboxTarget {
    app_folder: &'static str,
    mailbox: String,
}

#[derive(Debug, Default, Clone)]
struct ImapMailboxMap {
    inbox: Option<String>,
    junkemail: Option<String>,
    deleteditems: Option<String>,
}

impl ImapMailboxMap {
    fn set_if_missing(&mut self, app_folder: &str, mailbox: &str) {
        let target = match app_folder {
            "junkemail" => &mut self.junkemail,
            "deleteditems" => &mut self.deleteditems,
            _ => &mut self.inbox,
        };
        if target.is_none() {
            *target = Some(mailbox.to_string());
        }
    }

    fn mailbox_for(&self, app_folder: &str) -> String {
        match app_folder {
            "junk" | "junkemail" => self.junkemail.clone().unwrap_or_else(|| "Junk".to_string()),
            "deleted" | "deleteditems" => self.deleteditems.clone().unwrap_or_else(|| "Deleted".to_string()),
            _ => self.inbox.clone().unwrap_or_else(|| "INBOX".to_string()),
        }
    }
}

fn imap_mailbox_targets(
    session: &mut imap::Session<native_tls::TlsStream<TcpStream>>,
    folder: &str,
) -> Vec<ImapMailboxTarget> {
    let map = imap_mailbox_map(session);
    folders_for(folder)
        .into_iter()
        .map(|app_folder| ImapMailboxTarget {
            app_folder,
            mailbox: map.mailbox_for(app_folder),
        })
        .collect()
}

fn imap_mailbox_map(session: &mut imap::Session<native_tls::TlsStream<TcpStream>>) -> ImapMailboxMap {
    let mut map = ImapMailboxMap::default();
    let Ok(names) = session.list(Some(""), Some("*")) else {
        return map;
    };
    for name in names.iter() {
        let attributes = name
            .attributes()
            .iter()
            .map(imap_attribute_name)
            .collect::<Vec<_>>();
        if attributes.iter().any(|attribute| attribute == "noselect") {
            continue;
        }
        if let Some(app_folder) = classify_imap_mailbox(&attributes, name.name()) {
            map.set_if_missing(app_folder, name.name());
        }
    }
    map
}

fn imap_attribute_name(attribute: &NameAttribute<'_>) -> String {
    match attribute {
        NameAttribute::NoInferiors => "noinferiors".to_string(),
        NameAttribute::NoSelect => "noselect".to_string(),
        NameAttribute::Marked => "marked".to_string(),
        NameAttribute::Unmarked => "unmarked".to_string(),
        NameAttribute::Custom(value) => value.trim_start_matches('\\').to_ascii_lowercase(),
    }
}

fn classify_imap_mailbox(attributes: &[String], mailbox_name: &str) -> Option<&'static str> {
    if attributes.iter().any(|value| value == "junk" || value == "spam") {
        return Some("junkemail");
    }
    if attributes.iter().any(|value| value == "trash" || value == "deleted") {
        return Some("deleteditems");
    }
    let normalized = normalize_imap_mailbox_name(mailbox_name);
    if normalized == "inbox" {
        return Some("inbox");
    }
    let leaf = normalized.rsplit('/').next().unwrap_or(normalized.as_str());
    match leaf {
        "junk" | "junkemail" | "spam" | "bulkmail" => Some("junkemail"),
        "deleted" | "deleteditems" | "deletedmessages" | "trash" | "bin" => Some("deleteditems"),
        _ => None,
    }
}

fn normalize_imap_mailbox_name(value: &str) -> String {
    value
        .chars()
        .filter_map(|ch| {
            if ch.is_ascii_whitespace() || ch == '_' || ch == '-' {
                None
            } else if ch == '\\' {
                Some('/')
            } else {
                Some(ch.to_ascii_lowercase())
            }
        })
        .collect()
}

fn with_imap_session<T>(
    account: &AccountCredentials,
    operation: impl Fn(&mut imap::Session<native_tls::TlsStream<TcpStream>>) -> AppResult<T>,
) -> AppResult<T> {
    let host = account.imap_host.trim();
    if host.is_empty() {
        return Err(AppError::InvalidInput("IMAP host is required".to_string()));
    }
    let auth = imap_auth_secret(account)?;

    if account.proxy_chain.is_empty() {
        let mut session = connect_imap_session(account, &auth, None)?;
        let result = operation(&mut session);
        let _ = session.logout();
        return result;
    }

    let mut last_error = String::new();
    for proxy_url in &account.proxy_chain {
        match connect_imap_session(account, &auth, Some(proxy_url)).and_then(|mut session| {
            let result = operation(&mut session);
            let _ = session.logout();
            result
        }) {
            Ok(value) => return Ok(value),
            Err(err) => last_error = format!("{err}"),
        }
    }
    Err(AppError::Internal(format!(
        "all proxy attempts failed for IMAP account {}: {}",
        account.email,
        if last_error.is_empty() { "unknown network error" } else { last_error.as_str() }
    )))
}

enum ImapAuthSecret {
    Password(String),
    OAuth2(String),
}

struct XOAuth2Authenticator {
    user: String,
    access_token: String,
}

impl imap::Authenticator for XOAuth2Authenticator {
    type Response = String;

    fn process(&self, _challenge: &[u8]) -> Self::Response {
        format!("user={}\x01auth=Bearer {}\x01\x01", self.user, self.access_token)
    }
}

fn imap_auth_secret(account: &AccountCredentials) -> AppResult<ImapAuthSecret> {
    let password = if account.imap_password.trim().is_empty() {
        account.password.trim()
    } else {
        account.imap_password.trim()
    };
    if !password.is_empty() {
        return Ok(ImapAuthSecret::Password(password.to_string()));
    }
    if !account.client_id.trim().is_empty() && !account.refresh_token.trim().is_empty() {
        return refresh_imap_oauth_access_token(account).map(ImapAuthSecret::OAuth2);
    }
    Err(AppError::InvalidInput(
        "IMAP password or OAuth refresh token is required".to_string(),
    ))
}

fn connect_imap_session(
    account: &AccountCredentials,
    auth: &ImapAuthSecret,
    proxy_url: Option<&str>,
) -> AppResult<imap::Session<native_tls::TlsStream<TcpStream>>> {
    let host = account.imap_host.trim();
    let port = u16::try_from(account.imap_port).unwrap_or(993);
    let tls = native_tls::TlsConnector::builder()
        .build()
        .map_err(|err| AppError::Internal(format!("TLS setup failed: {err}")))?;
    let client = if let Some(proxy_url) = proxy_url {
        let stream = connect_http_proxy_tunnel(proxy_url, host, port)?;
        let tls_stream = native_tls::TlsConnector::connect(&tls, host, stream)
            .map_err(|err| AppError::Internal(format!("IMAP TLS handshake failed: {err}")))?;
        let mut client = imap::Client::new(tls_stream);
        client
            .read_greeting()
            .map_err(|err| AppError::Internal(format!("IMAP greeting failed: {err}")))?;
        client
    } else {
        imap::connect((host, port), host, &tls)
            .map_err(|err| AppError::Internal(format!("IMAP connect failed: {err}")))?
    };
    match auth {
        ImapAuthSecret::Password(password) => client
            .login(account.email.as_str(), password)
            .map_err(|err| AppError::Internal(format!("IMAP login failed: {}", err.0))),
        ImapAuthSecret::OAuth2(access_token) => {
            let auth = XOAuth2Authenticator {
                user: account.email.clone(),
                access_token: access_token.clone(),
            };
            client
                .authenticate("XOAUTH2", &auth)
                .map_err(|err| AppError::Internal(format!("IMAP XOAUTH2 login failed: {}", err.0)))
        }
    }
}

fn connect_http_proxy_tunnel(proxy_url: &str, host: &str, port: u16) -> AppResult<TcpStream> {
    let url = reqwest::Url::parse(proxy_url)
        .map_err(|err| AppError::InvalidInput(format!("invalid proxy URL {proxy_url}: {err}")))?;
    if url.scheme() != "http" {
        return Err(AppError::InvalidInput(
            "IMAP proxy tunnel currently supports http:// proxies only".to_string(),
        ));
    }
    let proxy_host = url
        .host_str()
        .ok_or_else(|| AppError::InvalidInput("proxy URL must include a host".to_string()))?;
    let proxy_port = url.port_or_known_default().unwrap_or(80);
    let mut stream = TcpStream::connect((proxy_host, proxy_port))
        .map_err(|err| AppError::Internal(format!("proxy connect failed: {err}")))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .map_err(|err| AppError::Internal(format!("proxy read timeout setup failed: {err}")))?;
    stream
        .set_write_timeout(Some(Duration::from_secs(30)))
        .map_err(|err| AppError::Internal(format!("proxy write timeout setup failed: {err}")))?;

    let authority = format!("{host}:{port}");
    let mut request = format!(
        "CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\nProxy-Connection: Keep-Alive\r\n"
    );
    if !url.username().is_empty() {
        let password = url.password().unwrap_or_default();
        let token = STANDARD.encode(format!("{}:{password}", url.username()));
        request.push_str(&format!("Proxy-Authorization: Basic {token}\r\n"));
    }
    request.push_str("\r\n");
    stream
        .write_all(request.as_bytes())
        .map_err(|err| AppError::Internal(format!("proxy CONNECT request failed: {err}")))?;

    let mut response = Vec::new();
    let mut buffer = [0_u8; 1024];
    loop {
        let read = stream
            .read(&mut buffer)
            .map_err(|err| AppError::Internal(format!("proxy CONNECT response failed: {err}")))?;
        if read == 0 {
            break;
        }
        response.extend_from_slice(&buffer[..read]);
        if response.windows(4).any(|window| window == b"\r\n\r\n") || response.len() > 8192 {
            break;
        }
    }
    let response_text = String::from_utf8_lossy(&response);
    let status_line = response_text.lines().next().unwrap_or_default();
    if !status_line.contains(" 200 ") {
        return Err(AppError::Internal(format!(
            "proxy CONNECT failed: {}",
            status_line
        )));
    }
    Ok(stream)
}

fn mutate_imap_message(
    account: &AccountCredentials,
    folder: &str,
    message_id: &str,
    operation: &str,
    expunge: bool,
) -> AppResult<()> {
    let uid = message_id
        .trim()
        .parse::<u32>()
        .map_err(|_| AppError::InvalidInput("IMAP message id is not a UID".to_string()))?;

    with_imap_session(account, |session| {
        let mailbox = imap_mailbox_map(session).mailbox_for(folder);
        session
            .select(&mailbox)
            .map_err(|err| AppError::Internal(format!("IMAP select {mailbox} failed: {err}")))?;
        session
            .uid_store(uid.to_string(), operation)
            .map_err(|err| AppError::Internal(format!("IMAP update flags failed: {err}")))?;
        if expunge {
            session
                .expunge()
                .map_err(|err| AppError::Internal(format!("IMAP expunge failed: {err}")))?;
        }
        Ok(())
    })
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
        raw_mime: Some(body.to_vec()),
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
    if is_downloadable_part(mail, &disposition) {
        let name = attachment_part_name(mail, &disposition);
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

struct RawAttachment {
    id: String,
    name: String,
    content_type: String,
    bytes: Vec<u8>,
}

fn collect_downloadable_parts(mail: &mailparse::ParsedMail<'_>, attachments: &mut Vec<RawAttachment>) {
    let disposition = mail.get_content_disposition();
    if is_downloadable_part(mail, &disposition) {
        let name = attachment_part_name(mail, &disposition);
        let bytes = mail.get_body_raw().unwrap_or_default();
        attachments.push(RawAttachment {
            id: name.clone(),
            name,
            content_type: mail.ctype.mimetype.clone(),
            bytes,
        });
        return;
    }
    for part in &mail.subparts {
        collect_downloadable_parts(part, attachments);
    }
}

fn is_downloadable_part(mail: &mailparse::ParsedMail<'_>, disposition: &mailparse::ParsedContentDisposition) -> bool {
    disposition.disposition == mailparse::DispositionType::Attachment
        || disposition.params.contains_key("filename")
        || mail.ctype.params.contains_key("name")
}

fn attachment_part_name(mail: &mailparse::ParsedMail<'_>, disposition: &mailparse::ParsedContentDisposition) -> String {
    disposition
        .params
        .get("filename")
        .or_else(|| mail.ctype.params.get("name"))
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or("attachment")
        .to_string()
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
                .map(format_graph_email_address)
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
            raw_mime: None,
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
    name: Option<String>,
    address: String,
}

fn format_graph_email_address(email: GraphEmailAddress) -> String {
    let address = email.address.trim();
    let name = email.name.unwrap_or_default();
    let name = name.trim();
    if !name.is_empty() && !address.is_empty() {
        format!("{name} <{address}>")
    } else if !name.is_empty() {
        name.to_string()
    } else {
        address.to_string()
    }
}

fn recipients_to_string(value: Option<Vec<GraphRecipient>>) -> String {
    value
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| item.email_address.map(|email| email.address))
        .collect::<Vec<_>>()
        .join(", ")
}

#[derive(Debug, Deserialize)]
struct GmailMessageList {
    messages: Option<Vec<GmailMessageRef>>,
}

#[derive(Debug, Deserialize)]
struct GmailMessageRef {
    id: String,
}

#[derive(Debug, Deserialize)]
struct GmailMessage {
    id: String,
    #[serde(default, rename = "labelIds")]
    label_ids: Vec<String>,
    #[serde(default, rename = "internalDate")]
    internal_date: Option<String>,
    #[serde(default)]
    snippet: String,
    #[serde(default)]
    payload: Option<GmailMessagePart>,
}

impl GmailMessage {
    fn into_provider_message(self, folder: &str) -> AppResult<ProviderMessage> {
        let subject = self
            .payload
            .as_ref()
            .and_then(|payload| gmail_header(payload, "Subject"))
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "(no subject)".to_string());
        let sender = self
            .payload
            .as_ref()
            .and_then(|payload| gmail_header(payload, "From"))
            .unwrap_or_default();
        let recipients = self
            .payload
            .as_ref()
            .and_then(|payload| gmail_header(payload, "To"))
            .unwrap_or_default();
        let cc = self
            .payload
            .as_ref()
            .and_then(|payload| gmail_header(payload, "Cc"))
            .unwrap_or_default();
        let (received_at, received_at_sort) = gmail_received_at(self.internal_date.as_deref(), self.payload.as_ref());
        let mut content = GmailParsedContent::default();
        if let Some(payload) = self.payload.as_ref() {
            collect_gmail_part(payload, &mut content)?;
        }
        let has_html_body = !content.html_body.is_empty();
        let body = if has_html_body {
            Some(content.html_body)
        } else if !content.text_body.is_empty() {
            Some(content.text_body)
        } else {
            None
        };
        let body_type = if has_html_body {
            "html".to_string()
        } else {
            "text".to_string()
        };
        let body_preview = if self.snippet.trim().is_empty() {
            body.as_deref()
                .unwrap_or_default()
                .split_whitespace()
                .take(30)
                .collect::<Vec<_>>()
                .join(" ")
        } else {
            self.snippet
        };
        Ok(ProviderMessage {
            folder: folder.to_string(),
            provider_message_id: self.id,
            subject,
            sender,
            recipients,
            cc,
            received_at,
            received_at_sort,
            is_read: !self.label_ids.iter().any(|label| label.eq_ignore_ascii_case("UNREAD")),
            has_attachments: !content.attachments.is_empty(),
            body_preview,
            body,
            body_type,
            attachments: content.attachments,
            raw_mime: None,
        })
    }
}

#[derive(Debug, Default, Deserialize)]
struct GmailMessagePart {
    #[serde(default, rename = "mimeType")]
    mime_type: String,
    #[serde(default)]
    filename: String,
    #[serde(default)]
    headers: Vec<GmailHeader>,
    #[serde(default)]
    body: Option<GmailMessagePartBody>,
    #[serde(default)]
    parts: Vec<GmailMessagePart>,
}

#[derive(Debug, Default, Deserialize)]
struct GmailHeader {
    name: String,
    value: String,
}

#[derive(Debug, Default, Deserialize)]
struct GmailMessagePartBody {
    #[serde(default, rename = "attachmentId")]
    attachment_id: Option<String>,
    #[serde(default)]
    size: Option<i64>,
    #[serde(default)]
    data: Option<String>,
}

#[derive(Default)]
struct GmailParsedContent {
    text_body: String,
    html_body: String,
    attachments: Vec<AttachmentInfo>,
}

fn collect_gmail_part(part: &GmailMessagePart, content: &mut GmailParsedContent) -> AppResult<()> {
    let mime_type = part.mime_type.to_ascii_lowercase();
    let filename = part.filename.trim();
    if !filename.is_empty() || part.body.as_ref().and_then(|body| body.attachment_id.as_ref()).is_some() {
        if let Some(attachment_id) = part.body.as_ref().and_then(|body| body.attachment_id.as_ref()) {
            content.attachments.push(AttachmentInfo {
                id: attachment_id.clone(),
                name: if filename.is_empty() {
                    "attachment".to_string()
                } else {
                    filename.to_string()
                },
                content_type: part.mime_type.clone(),
                size: part.body.as_ref().and_then(|body| body.size).unwrap_or_default(),
            });
            return Ok(());
        }
    }
    if let Some(data) = part.body.as_ref().and_then(|body| body.data.as_deref()) {
        if mime_type == "text/html" && content.html_body.is_empty() {
            content.html_body = decode_gmail_text(data)?;
        } else if mime_type == "text/plain" && content.text_body.is_empty() {
            content.text_body = decode_gmail_text(data)?;
        }
    }
    for child in &part.parts {
        collect_gmail_part(child, content)?;
    }
    Ok(())
}

fn find_gmail_attachment_metadata(part: &GmailMessagePart, attachment_id: &str) -> Option<(String, String)> {
    if part
        .body
        .as_ref()
        .and_then(|body| body.attachment_id.as_deref())
        .is_some_and(|value| value == attachment_id)
    {
        let name = if part.filename.trim().is_empty() {
            "attachment.bin".to_string()
        } else {
            part.filename.trim().to_string()
        };
        return Some((name, part.mime_type.clone()));
    }
    part.parts
        .iter()
        .find_map(|child| find_gmail_attachment_metadata(child, attachment_id))
}

fn gmail_header(part: &GmailMessagePart, name: &str) -> Option<String> {
    part.headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case(name))
        .map(|header| header.value.clone())
}

fn gmail_received_at(internal_date: Option<&str>, payload: Option<&GmailMessagePart>) -> (String, f64) {
    if let Some(ms) = internal_date.and_then(|value| value.parse::<i64>().ok()) {
        let secs = ms / 1000;
        let nanos = ((ms % 1000).max(0) as u32) * 1_000_000;
        if let Some(value) = DateTime::<Utc>::from_timestamp(secs, nanos) {
            return (value.to_rfc3339(), ms as f64 / 1000.0);
        }
    }
    if let Some(date_header) = payload.and_then(|part| gmail_header(part, "Date")) {
        if let Ok(timestamp) = mailparse::dateparse(&date_header) {
            let value = DateTime::<Utc>::from_timestamp(timestamp, 0).unwrap_or_else(Utc::now);
            return (value.to_rfc3339(), timestamp as f64);
        }
    }
    let now = Utc::now();
    (now.to_rfc3339(), now.timestamp() as f64)
}

fn decode_gmail_text(data: &str) -> AppResult<String> {
    let bytes = decode_gmail_base64(data)?;
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

fn decode_gmail_base64(data: &str) -> AppResult<Vec<u8>> {
    let compact = data
        .split_whitespace()
        .collect::<String>()
        .trim_end_matches('=')
        .to_string();
    URL_SAFE_NO_PAD
        .decode(compact)
        .map_err(|err| AppError::Internal(format!("Gmail base64 decode failed: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_imap_special_use_and_common_mailbox_names() {
        assert_eq!(classify_imap_mailbox(&["junk".to_string()], "Mailbox"), Some("junkemail"));
        assert_eq!(classify_imap_mailbox(&["trash".to_string()], "Mailbox"), Some("deleteditems"));
        assert_eq!(classify_imap_mailbox(&[], "INBOX"), Some("inbox"));
        assert_eq!(classify_imap_mailbox(&[], "[Gmail]/Spam"), Some("junkemail"));
        assert_eq!(classify_imap_mailbox(&[], "Deleted Items"), Some("deleteditems"));
        assert_eq!(classify_imap_mailbox(&[], "Archive"), None);
    }

    #[test]
    fn formats_xoauth2_sasl_response() {
        let auth = XOAuth2Authenticator {
            user: "user@example.com".to_string(),
            access_token: "token-value".to_string(),
        };
        assert_eq!(
            <XOAuth2Authenticator as imap::Authenticator>::process(&auth, b""),
            "user=user@example.com\x01auth=Bearer token-value\x01\x01"
        );
    }

    #[test]
    fn formats_graph_sender_with_display_name() {
        let sender = format_graph_email_address(GraphEmailAddress {
            name: Some("OpenAI".to_string()),
            address: "noreply@tm.openai.com".to_string(),
        });
        assert_eq!(sender, "OpenAI <noreply@tm.openai.com>");
    }

    #[test]
    fn detects_mail_provider_registry_defaults() {
        let gmail = detect_mail_provider("person@gmail.com", None, false).expect("gmail");
        assert_eq!(gmail.id, "gmail");
        assert_eq!(gmail.account_type, "gmail");

        let qq = detect_mail_provider("user@foxmail.com", None, false).expect("qq");
        assert_eq!(qq.id, "qq");
        assert_eq!(qq.default_imap_host, "imap.qq.com");

        let netease = detect_mail_provider("manual@example.com", Some("163"), false).expect("163");
        assert_eq!(netease.id, "netease_163");
        assert_eq!(netease.default_imap_host, "imap.163.com");

        let graph = detect_mail_provider("user@example.com", None, true).expect("graph");
        assert_eq!(graph.id, "graph");
    }

    #[test]
    fn derives_pkce_s256_challenge() {
        let challenge = pkce_s256_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk").expect("challenge");
        assert_eq!(challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn builds_google_auth_url_with_pkce() {
        let verifier = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-._~".to_string();
        let url = build_graph_auth_url(&OAuthAuthUrlInput {
            client_id: "google-client".to_string(),
            redirect_uri: "http://127.0.0.1:53682/callback".to_string(),
            login_hint: Some("person@gmail.com".to_string()),
            provider: Some("gmail".to_string()),
            code_verifier: Some(verifier.clone()),
        })
        .expect("google auth url");

        assert!(url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"));
        assert!(url.contains("client_id=google-client"));
        assert!(url.contains("scope=https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fgmail.modify"));
        assert!(url.contains("code_challenge="));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("access_type=offline"));
        assert!(url.contains("login_hint=person%40gmail.com"));
        assert!(!url.contains(&verifier));
    }

    #[test]
    fn maps_gmail_folder_targets_to_app_folders() {
        let targets = gmail_folder_targets("all");
        assert_eq!(targets.len(), 3);
        assert_eq!(targets[0].app_folder, "inbox");
        assert_eq!(targets[0].label_id, "INBOX");
        assert_eq!(gmail_folder_targets("junkemail")[0].label_id, "SPAM");
        assert_eq!(gmail_folder_targets("deleteditems")[0].label_id, "TRASH");
    }

    #[test]
    fn builds_gmail_read_state_payloads() {
        assert_eq!(gmail_read_state_payload(true), json!({ "removeLabelIds": ["UNREAD"] }));
        assert_eq!(gmail_read_state_payload(false), json!({ "addLabelIds": ["UNREAD"] }));
    }

    #[test]
    fn normalizes_gmail_message_payload() {
        let message = GmailMessage {
            id: "gmail-1".to_string(),
            label_ids: vec!["INBOX".to_string(), "UNREAD".to_string()],
            internal_date: Some("1700000000000".to_string()),
            snippet: "Preview from Gmail".to_string(),
            payload: Some(GmailMessagePart {
                mime_type: "multipart/mixed".to_string(),
                headers: vec![
                    GmailHeader {
                        name: "Subject".to_string(),
                        value: "Gmail subject".to_string(),
                    },
                    GmailHeader {
                        name: "From".to_string(),
                        value: "Sender <sender@gmail.com>".to_string(),
                    },
                    GmailHeader {
                        name: "To".to_string(),
                        value: "Receiver <to@example.com>".to_string(),
                    },
                ],
                parts: vec![
                    GmailMessagePart {
                        mime_type: "text/html".to_string(),
                        body: Some(GmailMessagePartBody {
                            data: Some("PGI-SGVsbG88L2I-".to_string()),
                            ..GmailMessagePartBody::default()
                        }),
                        ..GmailMessagePart::default()
                    },
                    GmailMessagePart {
                        mime_type: "application/pdf".to_string(),
                        filename: "invoice.pdf".to_string(),
                        body: Some(GmailMessagePartBody {
                            attachment_id: Some("att-1".to_string()),
                            size: Some(42),
                            data: None,
                        }),
                        ..GmailMessagePart::default()
                    },
                ],
                ..GmailMessagePart::default()
            }),
        };

        let normalized = message.into_provider_message("inbox").expect("normalize gmail");
        assert_eq!(normalized.folder, "inbox");
        assert_eq!(normalized.provider_message_id, "gmail-1");
        assert_eq!(normalized.subject, "Gmail subject");
        assert_eq!(normalized.sender, "Sender <sender@gmail.com>");
        assert_eq!(normalized.recipients, "Receiver <to@example.com>");
        assert!(!normalized.is_read);
        assert_eq!(normalized.body_type, "html");
        assert_eq!(normalized.body.as_deref(), Some("<b>Hello</b>"));
        assert_eq!(normalized.body_preview, "Preview from Gmail");
        assert_eq!(normalized.attachments.len(), 1);
        assert_eq!(normalized.attachments[0].id, "att-1");
        assert_eq!(normalized.attachments[0].name, "invoice.pdf");
        assert_eq!(normalized.attachments[0].content_type, "application/pdf");
        assert_eq!(normalized.attachments[0].size, 42);
    }
}
