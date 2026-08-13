use crate::error::{AppError, AppResult};
use crate::models::{
    AccountCredentials, AttachmentInfo, DownloadedAttachment, ImapProviderFetchResult,
    OAuthAuthUrlInput, ProviderMessage, ProviderMessageState,
};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chrono::{DateTime, Utc};
use imap::types::{Flag, NameAttribute};
use mailparse::MailHeaderMap;
use reqwest::{blocking::Client, Proxy};
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

const GRAPH_SCOPE: &str =
    "offline_access https://graph.microsoft.com/Mail.Read https://graph.microsoft.com/Mail.ReadWrite https://graph.microsoft.com/User.Read";
const IMAP_OAUTH_SCOPE: &str = "offline_access https://outlook.office.com/IMAP.AccessAsUser.All";
const IMAP_MESSAGE_FETCH_QUERY: &str = "(FLAGS INTERNALDATE BODY.PEEK[])";
const IMAP_SEQUENCE_FETCH_QUERY: &str = "(UID FLAGS INTERNALDATE BODY.PEEK[])";
pub const MAIL_REFRESH_MAX_TOP: usize = 2000;
const GRAPH_MESSAGE_PAGE_MAX_TOP: usize = 1000;

#[derive(Debug, Clone, Default)]
struct ImapFetchMeta {
    is_read: bool,
    internal_date: Option<chrono::DateTime<chrono::FixedOffset>>,
}

#[derive(Clone, Copy)]
pub struct MailProviderDefinition {
    pub id: &'static str,
    pub credential_kind: &'static str,
    pub account_type: &'static str,
    pub default_imap_host: &'static str,
    pub default_imap_port: i64,
    pub capabilities: &'static [&'static str],
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
        capabilities: &[
            "read_mail",
            "download_attachments",
            "mark_read",
            "remote_delete",
        ],
        aliases: &["outlook", "microsoft", "msgraph"],
        domains: &["outlook.com", "hotmail.com", "live.com", "msn.com"],
    },
    MailProviderDefinition {
        id: "gmail",
        credential_kind: "imap_app_password",
        account_type: "imap",
        default_imap_host: "imap.gmail.com",
        default_imap_port: 993,
        capabilities: &[
            "read_mail",
            "download_attachments",
            "mark_read",
            "remote_delete",
            "imap_folders",
        ],
        aliases: &["google", "googlemail"],
        domains: &["gmail.com", "googlemail.com"],
    },
    MailProviderDefinition {
        id: "qq",
        credential_kind: "imap_auth_code",
        account_type: "imap",
        default_imap_host: "imap.qq.com",
        default_imap_port: 993,
        capabilities: &[
            "read_mail",
            "download_attachments",
            "mark_read",
            "remote_delete",
            "imap_folders",
        ],
        aliases: &["qqmail"],
        domains: &["qq.com", "foxmail.com"],
    },
    MailProviderDefinition {
        id: "imap",
        credential_kind: "imap_password",
        account_type: "imap",
        default_imap_host: "",
        default_imap_port: 993,
        capabilities: &[
            "read_mail",
            "download_attachments",
            "mark_read",
            "remote_delete",
            "imap_folders",
        ],
        aliases: &["outlook_imap"],
        domains: &[],
    },
    MailProviderDefinition {
        id: "netease_163",
        credential_kind: "imap_auth_code",
        account_type: "imap",
        default_imap_host: "imap.163.com",
        default_imap_port: 993,
        capabilities: &[
            "read_mail",
            "download_attachments",
            "mark_read",
            "remote_delete",
            "imap_folders",
        ],
        aliases: &["163", "netease", "163mail"],
        domains: &["163.com"],
    },
    MailProviderDefinition {
        id: "imap_custom",
        credential_kind: "imap_password",
        account_type: "imap",
        default_imap_host: "",
        default_imap_port: 993,
        capabilities: &[
            "read_mail",
            "download_attachments",
            "mark_read",
            "remote_delete",
            "imap_folders",
        ],
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
    MAIL_PROVIDER_REGISTRY
        .iter()
        .find(|item| item.id == provider_id)
}

pub fn mail_provider_capabilities(value: &str) -> Option<&'static [&'static str]> {
    mail_provider_definition(value).map(|provider| provider.capabilities)
}

pub fn mail_provider_supports_capability(value: &str, capability: &str) -> bool {
    mail_provider_capabilities(value).is_some_and(|capabilities| capabilities.contains(&capability))
}

pub fn detect_mail_provider(
    email: &str,
    explicit_provider: Option<&str>,
    has_refresh_token: bool,
) -> AppResult<&'static MailProviderDefinition> {
    if let Some(provider) = explicit_provider.and_then(normalize_mail_provider_id) {
        return mail_provider_definition(provider).ok_or_else(|| {
            AppError::InvalidInput(format!("unsupported mail provider: {provider}"))
        });
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
    match provider
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "imap" => IMAP_OAUTH_SCOPE,
        _ => GRAPH_SCOPE,
    }
}

pub fn build_graph_auth_url(input: &OAuthAuthUrlInput) -> AppResult<String> {
    let provider = normalize_oauth_provider(input.provider.as_deref())?;

    let client_id = input.client_id.trim();
    let redirect_uri = input.redirect_uri.trim();
    if client_id.is_empty() {
        return Err(AppError::InvalidInput(
            "Microsoft client id is required".to_string(),
        ));
    }
    if redirect_uri.is_empty() {
        return Err(AppError::InvalidInput(
            "OAuth redirect URI is required".to_string(),
        ));
    }

    let scope = urlencoding::encode(microsoft_oauth_scope(Some(provider))).replace("%20", "+");
    let mut url = format!(
        "https://login.microsoftonline.com/common/oauth2/v2.0/authorize?client_id={}&response_type=code&redirect_uri={}&response_mode=query&scope={}&state=12345",
        urlencoding::encode(client_id),
        urlencoding::encode(redirect_uri),
        scope
    );
    if let Some(login_hint) = input
        .login_hint
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
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

fn normalize_oauth_provider(provider: Option<&str>) -> AppResult<&'static str> {
    let value = provider.unwrap_or_default().trim();
    if value.is_empty() {
        return Ok("graph");
    }
    match normalize_mail_provider_id(value) {
        Some("graph") => Ok("graph"),
        Some("imap") => Ok("imap"),
        Some("gmail") => Err(AppError::InvalidInput(
            "Gmail OAuth is disabled; use IMAP app password".to_string(),
        )),
        _ => Err(AppError::InvalidInput(format!(
            "unsupported OAuth provider: {value}"
        ))),
    }
}

fn refresh_graph_access_token_with_client(
    account: &AccountCredentials,
    client: &Client,
) -> AppResult<OAuthTokenResponse> {
    refresh_microsoft_access_token_with_scope(account, client, GRAPH_SCOPE, "Graph")
}

fn refresh_imap_oauth_access_token(account: &AccountCredentials) -> AppResult<String> {
    with_account_http_client(account, |client| {
        refresh_microsoft_access_token_with_scope(account, client, IMAP_OAUTH_SCOPE, "IMAP")
            .map(|token| token.access_token)
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

pub fn fetch_graph_messages(
    account: &AccountCredentials,
    folder: &str,
    top: usize,
    cached_message_ids: &HashMap<String, HashSet<String>>,
) -> AppResult<(String, Vec<ProviderMessage>, Vec<ProviderMessageState>)> {
    with_account_http_client(account, |client| {
        let token = refresh_graph_access_token_with_client(account, client)?;
        let folders = folders_for(folder);
        let mut all = Vec::new();
        let mut states = Vec::new();
        for folder_name in folders {
            let cached_ids = cached_message_ids.get(folder_name);
            let has_cached_messages = cached_ids.is_some_and(|ids| !ids.is_empty());
            let select = if has_cached_messages {
                "id,receivedDateTime,isRead"
            } else {
                "id,subject,from,toRecipients,ccRecipients,receivedDateTime,isRead,hasAttachments,bodyPreview,body"
            };
            let target_count = top.clamp(1, MAIL_REFRESH_MAX_TOP);
            let mut remaining = target_count;
            let mut next_url = Some(format!(
                "https://graph.microsoft.com/v1.0/me/mailFolders/{}/messages?$top={}&$orderby=receivedDateTime%20desc&$select={select}",
                folder_name,
                graph_message_page_size(remaining)
            ));
            while let Some(url) = next_url.take() {
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
                let page_items = page.value.into_iter().take(remaining).collect::<Vec<_>>();
                let page_count = page_items.len();
                for item in page_items {
                    if cached_ids.is_some_and(|ids| ids.contains(&item.id)) {
                        states.push(ProviderMessageState {
                            folder: folder_name.to_string(),
                            provider_message_id: item.id,
                            is_read: item.is_read.unwrap_or(false),
                        });
                        continue;
                    }
                    let item = if has_cached_messages {
                        fetch_graph_message(client, &token.access_token, folder_name, &item.id)?
                    } else {
                        item
                    };
                    let mut message = item.into_provider_message(folder_name);
                    if message.has_attachments {
                        message.attachments = fetch_graph_attachments_metadata(
                            client,
                            &token.access_token,
                            &message.provider_message_id,
                        )?;
                    }
                    all.push(message);
                }
                remaining = remaining.saturating_sub(page_count);
                if remaining == 0 || page_count == 0 {
                    break;
                }
                next_url = page.next_link;
            }
        }
        all.sort_by(|a, b| b.received_at_sort.total_cmp(&a.received_at_sort));
        Ok((token.refresh_token, all, states))
    })
}

fn graph_message_page_size(remaining: usize) -> usize {
    remaining.clamp(1, GRAPH_MESSAGE_PAGE_MAX_TOP)
}

fn fetch_graph_message(
    client: &Client,
    access_token: &str,
    folder: &str,
    message_id: &str,
) -> AppResult<GraphMessage> {
    let url = format!(
        "https://graph.microsoft.com/v1.0/me/messages/{}?$select=id,subject,from,toRecipients,ccRecipients,receivedDateTime,isRead,hasAttachments,bodyPreview,body",
        urlencoding::encode(message_id)
    );
    let response = client
        .get(url)
        .bearer_auth(access_token)
        .send()
        .map_err(network_error)?;
    if !response.status().is_success() {
        return Err(AppError::Internal(format!(
            "Graph fetch message failed for {folder}/{message_id}: HTTP {} {}",
            response.status(),
            response.text().unwrap_or_default()
        )));
    }
    response.json().map_err(network_error)
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
        let content = attachment.content_bytes.ok_or_else(|| {
            AppError::InvalidInput("Graph attachment has no inline content bytes".to_string())
        })?;
        let bytes = STANDARD
            .decode(content)
            .map_err(|err| AppError::Internal(format!("attachment base64 decode failed: {err}")))?;
        Ok(DownloadedAttachment {
            name: attachment
                .name
                .unwrap_or_else(|| "attachment.bin".to_string()),
            content_type: attachment.content_type.unwrap_or_default(),
            bytes,
        })
    })
}

pub fn download_imap_attachment_from_raw(
    raw_mime: &[u8],
    attachment_id: &str,
) -> AppResult<DownloadedAttachment> {
    let requested_id = attachment_id.trim();
    if requested_id.is_empty() {
        return Err(AppError::InvalidInput(
            "attachment id is required".to_string(),
        ));
    }
    let parsed = mailparse::parse_mail(raw_mime).map_err(|err| {
        AppError::InvalidInput(format!("cached IMAP MIME could not be parsed: {err}"))
    })?;
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
        .ok_or_else(|| {
            AppError::InvalidInput("attachment was not found in cached IMAP MIME".to_string())
        })
}

pub fn mark_graph_message_read(
    account: &AccountCredentials,
    message_id: &str,
    is_read: bool,
) -> AppResult<()> {
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

pub fn fetch_imap_messages(
    account: &AccountCredentials,
    folder: &str,
    top: usize,
    cached_message_ids: &HashMap<String, HashSet<String>>,
    saved_uid_validity: &HashMap<String, u32>,
) -> AppResult<ImapProviderFetchResult> {
    with_imap_session(account, |session| {
        let mut result = ImapProviderFetchResult {
            uid_validity: saved_uid_validity.clone(),
            ..ImapProviderFetchResult::default()
        };
        let mut selected_remote_messages = 0_u32;
        for target in imap_mailbox_targets(session, folder) {
            let Some(mailbox) = select_imap_target(session, &target)? else {
                continue;
            };
            selected_remote_messages = selected_remote_messages.saturating_add(mailbox.exists);
            let uid_validity_changed = imap_uid_validity_changed(
                saved_uid_validity.get(target.app_folder).copied(),
                mailbox.uid_validity,
            );
            if let Some(uid_validity) = mailbox.uid_validity {
                result
                    .uid_validity
                    .insert(target.app_folder.to_string(), uid_validity);
            }
            let selected_uids = match search_recent_imap_uids(session, top) {
                Ok(uids) if !uids.is_empty() || mailbox.exists == 0 => uids,
                Ok(_) => {
                    let mut fetched = fetch_recent_imap_messages_by_sequence(
                        session,
                        &target,
                        top,
                        "UID SEARCH returned no UIDs for a non-empty mailbox",
                    )?;
                    if fetched.is_empty() {
                        return Err(AppError::Internal(format!(
                            "IMAP mailbox {} reports {} messages, but both UID and sequence searches returned no messages; existing local cache was preserved",
                            target.mailbox, mailbox.exists
                        )));
                    }
                    if uid_validity_changed {
                        result.reset_folders.push(target.app_folder.to_string());
                    }
                    result.messages.append(&mut fetched);
                    continue;
                }
                Err(uid_error) => {
                    let mut fetched = fetch_recent_imap_messages_by_sequence(
                        session,
                        &target,
                        top,
                        &uid_error.to_string(),
                    )?;
                    if mailbox.exists > 0 && fetched.is_empty() {
                        return Err(AppError::Internal(format!(
                            "IMAP mailbox {} reports {} messages, but UID and sequence fallbacks returned no messages; existing local cache was preserved",
                            target.mailbox, mailbox.exists
                        )));
                    }
                    if uid_validity_changed && !fetched.is_empty() {
                        result.reset_folders.push(target.app_folder.to_string());
                    }
                    result.messages.append(&mut fetched);
                    continue;
                }
            };
            if selected_uids.is_empty() {
                continue;
            }
            let cached_ids = cached_message_ids.get(target.app_folder);
            let (cached_uids, new_uids): (Vec<u32>, Vec<u32>) = selected_uids
                .into_iter()
                .partition(|uid| {
                    !uid_validity_changed
                        && cached_ids.is_some_and(|ids| ids.contains(&uid.to_string()))
                });
            let (mut fetched, incomplete_reason) = match fetch_imap_uids(session, &target, &new_uids)
            {
                Ok(fetched) if fetched_imap_uids_cover(&fetched, &new_uids) => (fetched, None),
                Ok(fetched) => {
                    let reason = format!(
                        "UID FETCH returned {} of {} requested message bodies",
                        fetched.len(),
                        new_uids.len()
                    );
                    (fetched, Some(reason))
                }
                Err(batch_error) => match fetch_imap_uids_individually(
                    account,
                    &target,
                    &new_uids,
                    &batch_error,
                ) {
                    Ok(fetched) if fetched_imap_uids_cover(&fetched, &new_uids) => (fetched, None),
                    Ok(fetched) => {
                        let reason = format!(
                            "{batch_error}; individual UID FETCH returned {} of {} requested message bodies",
                            fetched.len(),
                            new_uids.len()
                        );
                        (fetched, Some(reason))
                    }
                    Err(err) => (Vec::new(), Some(err.to_string())),
                },
            };
            if let Some(reason) = incomplete_reason {
                let mut fallback = fetch_recent_imap_messages_by_sequence(
                    session,
                    &target,
                    top,
                    &reason,
                )?;
                if mailbox.exists > 0 && fallback.is_empty() {
                    return Err(AppError::Internal(format!(
                        "IMAP mailbox {} reports {} messages, but UID and sequence fetches returned no message bodies; existing local cache was preserved",
                        target.mailbox, mailbox.exists
                    )));
                }
                if uid_validity_changed {
                    result.reset_folders.push(target.app_folder.to_string());
                }
                result.messages.append(&mut fallback);
                continue;
            }
            if uid_validity_changed && !new_uids.is_empty() {
                result.reset_folders.push(target.app_folder.to_string());
            }
            result.messages.append(&mut fetched);
            let mut states = fetch_imap_message_states(session, &target, &cached_uids)
                .map_err(AppError::Internal)?;
            result.states.append(&mut states);
        }
        let has_cached_target_messages = cached_message_ids.values().any(|ids| !ids.is_empty());
        if selected_remote_messages == 0
            && !has_cached_target_messages
            && result.messages.is_empty()
        {
            let nonempty_mailboxes = list_nonempty_imap_mailboxes(session);
            if !nonempty_mailboxes.is_empty() {
                return Err(AppError::Internal(format!(
                    "IMAP inbox/junk folders are empty, but the server reports messages in other folders: {}. Existing local cache was preserved",
                    nonempty_mailboxes.join(", ")
                )));
            }
        }
        result
            .messages
            .sort_by(|a, b| b.received_at_sort.total_cmp(&a.received_at_sort));
        Ok(result)
    })
}

fn imap_uid_validity_changed(saved: Option<u32>, current: Option<u32>) -> bool {
    match (saved, current) {
        (Some(saved), Some(current)) => saved != current,
        _ => false,
    }
}

fn build_imap_select_variants(folder_name: &str) -> Vec<String> {
    let raw_name = folder_name.trim();
    if raw_name.is_empty() {
        return Vec::new();
    }

    let unquoted = if raw_name.starts_with('"') && raw_name.ends_with('"') && raw_name.len() >= 2 {
        raw_name[1..raw_name.len() - 1].to_string()
    } else {
        raw_name.to_string()
    };

    let mut variants = Vec::new();
    for candidate in [
        raw_name.to_string(),
        unquoted.clone(),
        format!("\"{unquoted}\""),
    ] {
        if !candidate.is_empty() && !variants.contains(&candidate) {
            variants.push(candidate);
        }
    }
    variants
}

fn try_select_imap_mailbox(
    session: &mut imap::Session<native_tls::TlsStream<TcpStream>>,
    folder_name: &str,
) -> Result<imap::types::Mailbox, imap::Error> {
    let mut last_error = None;
    for candidate in build_imap_select_variants(folder_name) {
        match session.select(&candidate) {
            Ok(mailbox) => return Ok(mailbox),
            Err(err) => last_error = Some(err),
        }
    }
    Err(last_error.unwrap_or_else(|| imap::Error::Bad("empty mailbox name".into())))
}

fn select_imap_target(
    session: &mut imap::Session<native_tls::TlsStream<TcpStream>>,
    target: &ImapMailboxTarget,
) -> AppResult<Option<imap::types::Mailbox>> {
    let mailbox = target.mailbox.as_str();
    let mut last_error = None;
    for candidate in build_imap_select_variants(mailbox) {
        match session.select(&candidate) {
            Ok(mailbox) => return Ok(Some(mailbox)),
            Err(err) => last_error = Some(err),
        }
    }
    if target.app_folder != "inbox" {
        return Ok(None);
    }
    Err(format_imap_select_error(
        mailbox,
        &last_error.unwrap_or_else(|| imap::Error::Bad("empty mailbox name".into())),
    ))
}

fn format_imap_select_error(mailbox: &str, err: &imap::Error) -> AppError {
    let raw = err.to_string();
    let lower = raw.to_ascii_lowercase();
    if lower.contains("unsafe login") {
        return AppError::Internal(format!(
            "IMAP select {mailbox} failed: 163/网易邮箱拒绝第三方客户端登录（Unsafe Login）。请确认导入时填写的是客户端授权密码，不是网页登录密码，并已在 设置 > POP3/SMTP/IMAP 中开启 IMAP/SMTP；若仍失败，需要按网易提示联系 kefu@188.com 开通第三方客户端访问。原始错误：{raw}"
        ));
    }
    AppError::Internal(format!("IMAP select {mailbox} failed: {raw}"))
}

fn search_recent_imap_uids(
    session: &mut imap::Session<native_tls::TlsStream<TcpStream>>,
    top: usize,
) -> AppResult<Vec<u32>> {
    let uids = session
        .uid_search("ALL")
        .map_err(|err| AppError::Internal(format!("IMAP search failed: {err}")))?;
    let mut selected_uids: Vec<u32> = uids.into_iter().collect();
    selected_uids.sort_unstable();
    selected_uids.reverse();
    selected_uids.truncate(top.clamp(1, MAIL_REFRESH_MAX_TOP));
    Ok(selected_uids)
}

fn fetch_recent_imap_messages_by_sequence(
    session: &mut imap::Session<native_tls::TlsStream<TcpStream>>,
    target: &ImapMailboxTarget,
    top: usize,
    uid_error: &str,
) -> AppResult<Vec<ProviderMessage>> {
    let sequences = session.search("ALL").map_err(|err| {
        AppError::Internal(format!(
            "IMAP UID search failed ({uid_error}); sequence search fallback failed: {err}"
        ))
    })?;
    let mut selected_sequences = sequences.into_iter().collect::<Vec<_>>();
    selected_sequences.sort_unstable();
    selected_sequences.reverse();
    selected_sequences.truncate(top.clamp(1, MAIL_REFRESH_MAX_TOP));
    if selected_sequences.is_empty() {
        return Ok(Vec::new());
    }

    let sequence_set = selected_sequences
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let fetches = session
        .fetch(sequence_set, IMAP_SEQUENCE_FETCH_QUERY)
        .map_err(|err| {
            AppError::Internal(format!(
                "IMAP UID search failed ({uid_error}); sequence fetch fallback failed: {err}"
            ))
        })?;
    let mut messages = Vec::new();
    for fetch in fetches.iter() {
        let (Some(uid), Some(body)) = (fetch.uid, fetch.body()) else {
            continue;
        };
        messages.push(parse_imap_message(
            target.app_folder,
            uid,
            body,
            extract_imap_fetch_meta(fetch),
        ));
    }
    if messages.len() != selected_sequences.len() {
        return Err(AppError::Internal(format!(
            "IMAP sequence fallback returned {} of {} messages for {}; existing local cache was preserved",
            messages.len(),
            selected_sequences.len(),
            target.mailbox
        )));
    }
    messages.sort_by(|a, b| b.received_at_sort.total_cmp(&a.received_at_sort));
    Ok(messages)
}

fn fetch_imap_uids(
    session: &mut imap::Session<native_tls::TlsStream<TcpStream>>,
    target: &ImapMailboxTarget,
    selected_uids: &[u32],
) -> Result<Vec<ProviderMessage>, String> {
    if selected_uids.is_empty() {
        return Ok(Vec::new());
    }
    let sequence = selected_uids
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let fetches = session
        .uid_fetch(sequence, IMAP_MESSAGE_FETCH_QUERY)
        .map_err(|err| format!("IMAP fetch failed: {err}"))?;
    let mut messages = Vec::new();
    for fetch in fetches.iter() {
        if let Some(body) = fetch.body() {
            let meta = extract_imap_fetch_meta(fetch);
            messages.push(parse_imap_message(
                target.app_folder,
                fetch.uid.unwrap_or(fetch.message),
                body,
                meta,
            ));
        }
    }
    Ok(messages)
}

fn fetched_imap_uids_cover(messages: &[ProviderMessage], requested_uids: &[u32]) -> bool {
    requested_uids.iter().all(|uid| {
        let uid = uid.to_string();
        messages
            .iter()
            .any(|message| message.provider_message_id == uid)
    })
}

fn fetch_imap_message_states(
    session: &mut imap::Session<native_tls::TlsStream<TcpStream>>,
    target: &ImapMailboxTarget,
    selected_uids: &[u32],
) -> Result<Vec<ProviderMessageState>, String> {
    if selected_uids.is_empty() {
        return Ok(Vec::new());
    }
    let sequence = selected_uids
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let fetches = session
        .uid_fetch(sequence, "(FLAGS)")
        .map_err(|err| format!("IMAP flags fetch failed: {err}"))?;
    Ok(fetches
        .iter()
        .map(|fetch| ProviderMessageState {
            folder: target.app_folder.to_string(),
            provider_message_id: fetch.uid.unwrap_or(fetch.message).to_string(),
            is_read: imap_flags_include_seen(fetch.flags()),
        })
        .collect())
}

fn fetch_imap_uids_individually(
    account: &AccountCredentials,
    target: &ImapMailboxTarget,
    selected_uids: &[u32],
    batch_error: &str,
) -> AppResult<Vec<ProviderMessage>> {
    let mut messages = Vec::new();
    let mut failures = Vec::new();
    for uid in selected_uids {
        match fetch_single_imap_uid(account, target, *uid) {
            Ok(mut fetched) => messages.append(&mut fetched),
            Err(err) => failures.push(format!("{uid}: {err}")),
        }
    }
    if messages.is_empty() && !failures.is_empty() {
        return Err(AppError::Internal(format!(
            "{batch_error}; IMAP individual fetch fallback failed: {}",
            failures.join("; ")
        )));
    }
    messages.sort_by(|a, b| b.received_at_sort.total_cmp(&a.received_at_sort));
    Ok(messages)
}

fn fetch_single_imap_uid(
    account: &AccountCredentials,
    target: &ImapMailboxTarget,
    uid: u32,
) -> Result<Vec<ProviderMessage>, String> {
    with_imap_session(account, |session| {
        try_select_imap_mailbox(session, &target.mailbox)
            .map_err(|err| format_imap_select_error(&target.mailbox, &err))?;
        fetch_imap_uids(session, target, &[uid]).map_err(AppError::Internal)
    })
    .map_err(|err| err.to_string())
}

pub fn mark_imap_message_read(
    account: &AccountCredentials,
    folder: &str,
    message_id: &str,
    is_read: bool,
) -> AppResult<()> {
    mutate_imap_message(
        account,
        folder,
        message_id,
        if is_read {
            "+FLAGS (\\Seen)"
        } else {
            "-FLAGS (\\Seen)"
        },
        false,
    )
}

pub fn delete_imap_message(
    account: &AccountCredentials,
    folder: &str,
    message_id: &str,
) -> AppResult<()> {
    mutate_imap_message(account, folder, message_id, "+FLAGS (\\Deleted)", true)
}

fn http_client() -> AppResult<Client> {
    http_client_for_proxy(None)
}

fn http_client_for_proxy(proxy_url: Option<&str>) -> AppResult<Client> {
    let mut builder = Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("OutlookEmailDesktop/0.1");
    if let Some(proxy_url) = proxy_url.filter(|value| !value.trim().is_empty()) {
        let proxy = Proxy::all(proxy_url.trim()).map_err(|err| {
            AppError::InvalidInput(format!("invalid proxy URL {proxy_url}: {err}"))
        })?;
        builder = builder.proxy(proxy);
    }
    builder.build().map_err(network_error)
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
        if last_error.is_empty() {
            "unknown network error"
        } else {
            last_error.as_str()
        }
    )))
}

fn network_error(err: reqwest::Error) -> AppError {
    AppError::Internal(format!("network request failed: {err}"))
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
            "OAuth 授权码已过期或已被使用，请重新点击“打开”完成授权，然后粘贴新的回调 URL。"
                .to_string(),
        );
    }

    if lower.contains("invalid_grant") {
        return AppError::InvalidInput(
            "OAuth 授权码无效，请重新打开授权页面并复制新的完整回调 URL。".to_string(),
        );
    }

    AppError::Internal(format!(
        "OAuth token request failed: HTTP {status} {description}"
    ))
}

fn fetch_graph_attachments_metadata(
    client: &Client,
    access_token: &str,
    message_id: &str,
) -> AppResult<Vec<AttachmentInfo>> {
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
    let compact = code_or_url.split_whitespace().collect::<String>();
    let value = compact.trim();
    if value.is_empty() {
        return Err(AppError::InvalidInput(
            "OAuth code or callback URL is required".to_string(),
        ));
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
        "inbox_junk" | "all" => vec!["inbox", "junkemail"],
        _ => vec!["inbox", "junkemail"],
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
            "deleted" | "deleteditems" => self
                .deleteditems
                .clone()
                .unwrap_or_else(|| "Deleted".to_string()),
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

fn imap_mailbox_map(
    session: &mut imap::Session<native_tls::TlsStream<TcpStream>>,
) -> ImapMailboxMap {
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
    if attributes.iter().any(|value| value == "inbox") {
        return Some("inbox");
    }
    if attributes
        .iter()
        .any(|value| value == "junk" || value == "spam")
    {
        return Some("junkemail");
    }
    if attributes
        .iter()
        .any(|value| value == "trash" || value == "deleted")
    {
        return Some("deleteditems");
    }
    let normalized = normalize_imap_mailbox_name(mailbox_name);
    if normalized == "inbox" || normalized == "收件箱" || normalized == "收件匣" {
        return Some("inbox");
    }
    let leaf = normalized.rsplit('/').next().unwrap_or(normalized.as_str());
    match leaf {
        "inbox" | "收件箱" | "收件匣" => Some("inbox"),
        "junk" | "junkemail" | "spam" | "bulkmail" | "垃圾邮件" | "垃圾郵件" | "垃圾信" => {
            Some("junkemail")
        }
        "deleted" | "deleteditems" | "deletedmessages" | "trash" | "bin" | "已删除" | "已刪除"
        | "已删除邮件" | "已刪除郵件" | "垃圾箱" | "回收站" => Some("deleteditems"),
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
        if last_error.is_empty() {
            "unknown network error"
        } else {
            last_error.as_str()
        }
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
        format!(
            "user={}\x01auth=Bearer {}\x01\x01",
            self.user, self.access_token
        )
    }
}

fn imap_auth_secret(account: &AccountCredentials) -> AppResult<ImapAuthSecret> {
    if account.provider.eq_ignore_ascii_case("imap") {
        if !account.client_id.trim().is_empty() && !account.refresh_token.trim().is_empty() {
            return refresh_imap_oauth_access_token(account).map(ImapAuthSecret::OAuth2);
        }
        return Err(AppError::InvalidInput(
            "IMAP OAuth requires client ID and refresh token".to_string(),
        ));
    }

    if let Some(def) = mail_provider_definition(&account.provider) {
        if def.credential_kind.starts_with("imap") {
            if !account.refresh_token.trim().is_empty() {
                return Ok(ImapAuthSecret::Password(
                    account.refresh_token.trim().to_string(),
                ));
            }
            return Err(AppError::InvalidInput(format!(
                "{} requires an app password or auth code",
                def.id
            )));
        }
    }

    if !account.client_id.trim().is_empty() && !account.refresh_token.trim().is_empty() {
        return refresh_imap_oauth_access_token(account).map(ImapAuthSecret::OAuth2);
    }

    Err(AppError::InvalidInput(
        "IMAP credentials are missing".to_string(),
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
    let mut session = match auth {
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
    }?;
    send_imap_client_id_if_needed(account, &mut session);
    Ok(session)
}

fn should_send_imap_client_id(account: &AccountCredentials) -> bool {
    if is_netease_imap_account(account) {
        return true;
    }
    let provider = account.provider.trim().to_ascii_lowercase();
    matches!(
        provider.as_str(),
        "qq" | "netease_163" | "imap_custom" | "gmail"
    )
}

fn send_imap_client_id_if_needed(
    account: &AccountCredentials,
    session: &mut imap::Session<native_tls::TlsStream<TcpStream>>,
) {
    if !should_send_imap_client_id(account) {
        return;
    }
    let _ = session.run_command_and_check_ok(
        r#"ID ("name" "OutlookEmail Desktop" "version" "0.1.0" "vendor" "OutlookEmail")"#,
    );
}

fn is_netease_imap_account(account: &AccountCredentials) -> bool {
    let host = account.imap_host.trim().to_ascii_lowercase();
    account.provider.eq_ignore_ascii_case("netease_163")
        || host == "imap.163.com"
        || host == "imap.126.com"
        || host == "imap.yeah.net"
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
        try_select_imap_mailbox(session, &mailbox)
            .map_err(|err| format_imap_select_error(&mailbox, &err))?;
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

fn imap_flags_include_seen(flags: &[Flag<'_>]) -> bool {
    flags.iter().any(|flag| matches!(flag, Flag::Seen))
}

fn extract_imap_fetch_meta(fetch: &imap::types::Fetch) -> ImapFetchMeta {
    ImapFetchMeta {
        is_read: imap_flags_include_seen(fetch.flags()),
        internal_date: fetch.internal_date(),
    }
}

fn resolve_imap_received_time(
    internal_date: Option<chrono::DateTime<chrono::FixedOffset>>,
    mime_date_header: &str,
) -> (String, f64) {
    if let Some(internal_date) = internal_date {
        let received_at = internal_date.with_timezone(&Utc);
        return (
            received_at.to_rfc3339(),
            received_at.timestamp() as f64,
        );
    }
    if let Ok(timestamp) = mailparse::dateparse(mime_date_header) {
        let received_at = DateTime::<Utc>::from_timestamp(timestamp, 0).unwrap_or_else(Utc::now);
        return (received_at.to_rfc3339(), timestamp as f64);
    }
    let received_at = Utc::now();
    (
        received_at.to_rfc3339(),
        received_at.timestamp() as f64,
    )
}

fn parse_imap_message(folder: &str, uid: u32, body: &[u8], meta: ImapFetchMeta) -> ProviderMessage {
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
    let (received, timestamp) =
        resolve_imap_received_time(meta.internal_date, &date_header);
    let (body_text, body_type, attachments) = parsed
        .as_ref()
        .map(extract_body_and_attachments)
        .unwrap_or_else(|| (String::new(), "text".to_string(), Vec::new()));
    let preview_source = if body_type.eq_ignore_ascii_case("html") {
        html_to_plain_text(&body_text)
    } else {
        body_text.clone()
    };
    let preview = text_preview(&preview_source, 30);

    ProviderMessage {
        folder: folder.to_string(),
        provider_message_id: uid.to_string(),
        subject,
        sender,
        recipients,
        cc,
        received_at: received,
        received_at_sort: timestamp,
        is_read: meta.is_read,
        has_attachments: !attachments.is_empty(),
        body_preview: preview,
        body: Some(body_text),
        body_type,
        attachments,
        raw_mime: Some(body.to_vec()),
    }
}

fn text_preview(value: &str, words: usize) -> String {
    value
        .split_whitespace()
        .take(words)
        .collect::<Vec<_>>()
        .join(" ")
}

fn html_to_plain_text(value: &str) -> String {
    let without_script = remove_html_block(value, "script");
    let without_style = remove_html_block(&without_script, "style");
    let mut output = String::new();
    let mut tag = String::new();
    let mut in_tag = false;

    for ch in without_style.chars() {
        if in_tag {
            if ch == '>' {
                append_html_tag_boundary(&tag, &mut output);
                tag.clear();
                in_tag = false;
            } else {
                tag.push(ch);
            }
            continue;
        }
        if ch == '<' {
            in_tag = true;
            tag.clear();
            continue;
        }
        output.push(ch);
    }

    text_preview(
        &strip_css_fragments(&decode_html_entities(&output)),
        usize::MAX,
    )
}

fn strip_css_fragments(value: &str) -> String {
    let mut text = remove_css_comments(value);
    for _ in 0..16 {
        let Some((start, end)) = find_css_fragment(&text) else {
            break;
        };
        text.replace_range(start..end, " ");
    }
    text
}

fn remove_css_comments(value: &str) -> String {
    let mut output = String::new();
    let mut cursor = 0_usize;
    while let Some(start_offset) = value[cursor..].find("/*") {
        let start = cursor + start_offset;
        output.push_str(&value[cursor..start]);
        if let Some(end_offset) = value[start + 2..].find("*/") {
            cursor = start + 2 + end_offset + 2;
            output.push(' ');
        } else {
            cursor = value.len();
            break;
        }
    }
    output.push_str(&value[cursor..]);
    output
}

fn find_css_fragment(value: &str) -> Option<(usize, usize)> {
    let mut cursor = 0_usize;
    while let Some(open_offset) = value[cursor..].find('{') {
        let open = cursor + open_offset;
        let close = value[open + 1..].find('}').map(|offset| open + 1 + offset);
        let block_end = close.unwrap_or(value.len());
        let block = &value[open + 1..block_end];
        if looks_like_css_declaration_block(block) {
            if let Some(start) = css_selector_start(value, open) {
                return Some((start, close.map_or(value.len(), |index| index + 1)));
            }
        }
        let Some(close) = close else {
            return None;
        };
        cursor = close + 1;
    }
    None
}

fn looks_like_css_declaration_block(value: &str) -> bool {
    if value.contains("!important") || value.to_ascii_lowercase().contains("url(") {
        return true;
    }
    value
        .char_indices()
        .filter_map(|(index, ch)| (ch == ':').then_some(index))
        .any(|index| {
            let before = value[..index].trim_end();
            let start = before
                .char_indices()
                .rev()
                .find(|(_, ch)| !(ch.is_ascii_alphabetic() || *ch == '-'))
                .map(|(position, ch)| position + ch.len_utf8())
                .unwrap_or(0);
            is_css_property_name(&before[start..])
        })
}

fn is_css_property_name(value: &str) -> bool {
    let name = value.trim().trim_start_matches('-');
    (3..=48).contains(&name.len())
        && name
            .bytes()
            .all(|byte| matches!(byte, b'a'..=b'z' | b'A'..=b'Z' | b'-'))
}

fn css_selector_start(value: &str, open: usize) -> Option<usize> {
    let prefix_start = value[..open]
        .char_indices()
        .rev()
        .nth(240)
        .map(|(index, _)| index)
        .unwrap_or(0);
    let prefix = &value[prefix_start..open];
    let trimmed = prefix.trim_end();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    if let Some(position) = ["@", ".", "#", "*", "["]
        .iter()
        .filter_map(|marker| lower.rfind(marker))
        .max()
    {
        let mut start = prefix_start + position;
        if lower.as_bytes().get(position) == Some(&b'[') {
            start = selector_token_start(value, start);
        }
        return Some(start);
    }

    let tail_start = trimmed
        .char_indices()
        .rev()
        .find(|(_, ch)| ch.is_whitespace())
        .map(|(index, ch)| index + ch.len_utf8())
        .unwrap_or(0);
    let tail = trimmed[tail_start..].to_ascii_lowercase();
    if is_css_selector_tail(&tail) {
        return Some(prefix_start + tail_start);
    }
    None
}

fn selector_token_start(value: &str, index: usize) -> usize {
    value[..index]
        .char_indices()
        .rev()
        .find(|(_, ch)| ch.is_whitespace())
        .map(|(position, ch)| position + ch.len_utf8())
        .unwrap_or(0)
}

fn is_css_selector_tail(value: &str) -> bool {
    let tail = value.trim_start();
    matches!(
        tail.split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'))
            .next()
            .unwrap_or_default(),
        "body"
            | "html"
            | "a"
            | "p"
            | "div"
            | "span"
            | "table"
            | "td"
            | "th"
            | "img"
            | "font"
            | "strong"
            | "em"
            | "u"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
    )
}

fn remove_html_block(value: &str, tag: &str) -> String {
    let lower = value.to_ascii_lowercase();
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut output = String::new();
    let mut cursor = 0_usize;

    while let Some(start_offset) = lower[cursor..].find(&open) {
        let start = cursor + start_offset;
        output.push_str(&value[cursor..start]);
        if let Some(end_offset) = lower[start..].find(&close) {
            cursor = start + end_offset + close.len();
            output.push(' ');
        } else {
            cursor = value.len();
            break;
        }
    }
    output.push_str(&value[cursor..]);
    output
}

fn append_html_tag_boundary(tag: &str, output: &mut String) {
    let name = tag
        .trim()
        .trim_start_matches('/')
        .trim_start_matches('!')
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_end_matches('/')
        .to_ascii_lowercase();
    if matches!(
        name.as_str(),
        "br" | "p"
            | "div"
            | "li"
            | "tr"
            | "td"
            | "th"
            | "table"
            | "ul"
            | "ol"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
    ) {
        output.push(' ');
    }
}

fn decode_html_entities(value: &str) -> String {
    let mut output = String::new();
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '&' {
            output.push(ch);
            continue;
        }

        let mut entity = String::new();
        let mut terminated = false;
        while let Some(&next) = chars.peek() {
            chars.next();
            if next == ';' {
                terminated = true;
                break;
            }
            entity.push(next);
            if entity.len() > 16 {
                break;
            }
        }

        match terminated.then(|| decode_html_entity(&entity)).flatten() {
            Some(decoded) => output.push(decoded),
            None => {
                output.push('&');
                output.push_str(&entity);
                if terminated {
                    output.push(';');
                }
            }
        }
    }
    output
}

fn decode_html_entity(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some(' '),
        _ if entity.starts_with("#x") || entity.starts_with("#X") => {
            u32::from_str_radix(&entity[2..], 16)
                .ok()
                .and_then(char::from_u32)
        }
        _ if entity.starts_with('#') => entity[1..].parse::<u32>().ok().and_then(char::from_u32),
        _ => None,
    }
}

fn extract_body_and_attachments(
    mail: &mailparse::ParsedMail<'_>,
) -> (String, String, Vec<AttachmentInfo>) {
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
            size: mail
                .get_body_raw()
                .map(|bytes| bytes.len() as i64)
                .unwrap_or_default(),
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

fn collect_downloadable_parts(
    mail: &mailparse::ParsedMail<'_>,
    attachments: &mut Vec<RawAttachment>,
) {
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

fn is_downloadable_part(
    mail: &mailparse::ParsedMail<'_>,
    disposition: &mailparse::ParsedContentDisposition,
) -> bool {
    disposition.disposition == mailparse::DispositionType::Attachment
        || disposition.params.contains_key("filename")
        || mail.ctype.params.contains_key("name")
}

fn attachment_part_name(
    mail: &mailparse::ParsedMail<'_>,
    disposition: &mailparse::ParsedContentDisposition,
) -> String {
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
    #[serde(rename = "@odata.nextLink")]
    next_link: Option<String>,
}

fn list_nonempty_imap_mailboxes(
    session: &mut imap::Session<native_tls::TlsStream<TcpStream>>,
) -> Vec<String> {
    let Ok(names) = session.list(Some(""), Some("*")) else {
        return Vec::new();
    };
    let candidates = names
        .iter()
        .filter_map(|name| {
            let attributes = name
                .attributes()
                .iter()
                .map(imap_attribute_name)
                .collect::<Vec<_>>();
            if attributes.iter().any(|attribute| attribute == "noselect") {
                None
            } else {
                Some(name.name().to_string())
            }
        })
        .collect::<Vec<_>>();
    let mut nonempty = Vec::new();
    for mailbox_name in candidates {
        let Ok(mailbox) = try_select_imap_mailbox(session, &mailbox_name) else {
            continue;
        };
        if mailbox.exists > 0 {
            nonempty.push(format!("{mailbox_name} ({})", mailbox.exists));
        }
    }
    nonempty
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
        let received_at = self
            .received_date_time
            .unwrap_or_else(|| Utc::now().to_rfc3339());
        let received_at_sort = DateTime::parse_from_rfc3339(&received_at)
            .map(|value| value.timestamp() as f64)
            .unwrap_or_else(|_| Utc::now().timestamp() as f64);
        let body = self.body.map(|body| body);
        let body_type = body
            .as_ref()
            .map(|body| body.content_type.to_ascii_lowercase())
            .unwrap_or_else(|| "text".to_string());
        let body_content = body
            .map(|body| body.content)
            .filter(|value| !value.is_empty());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_for_default_skips_deleted_items() {
        assert_eq!(
            folders_for("inbox_junk"),
            vec!["inbox", "junkemail"]
        );
        assert_eq!(folders_for("all"), vec!["inbox", "junkemail"]);
        assert_eq!(folders_for("unknown"), vec!["inbox", "junkemail"]);
    }

    #[test]
    fn graph_message_pages_respect_the_api_limit_while_allowing_two_thousand_total() {
        assert_eq!(MAIL_REFRESH_MAX_TOP, 2000);
        assert_eq!(graph_message_page_size(2000), 1000);
        assert_eq!(graph_message_page_size(1000), 1000);
        assert_eq!(graph_message_page_size(725), 725);
    }

    #[test]
    fn graph_message_page_reads_the_next_link() {
        let page: GraphMessagePage = serde_json::from_value(json!({
            "value": [],
            "@odata.nextLink": "https://graph.microsoft.com/v1.0/me/messages?$skip=1000"
        }))
        .expect("Graph message page should deserialize");

        assert_eq!(
            page.next_link.as_deref(),
            Some("https://graph.microsoft.com/v1.0/me/messages?$skip=1000")
        );
    }

    #[test]
    fn imap_uid_validity_initialization_preserves_cache_and_known_changes_rebuild() {
        assert!(!imap_uid_validity_changed(None, Some(7)));
        assert!(!imap_uid_validity_changed(Some(7), Some(7)));
        assert!(imap_uid_validity_changed(Some(7), Some(8)));
        assert!(!imap_uid_validity_changed(Some(7), None));
    }

    #[test]
    fn imap_uid_fetch_must_return_every_requested_message_body() {
        let message = ProviderMessage {
            folder: "inbox".to_string(),
            provider_message_id: "5".to_string(),
            subject: String::new(),
            sender: String::new(),
            recipients: String::new(),
            cc: String::new(),
            received_at: String::new(),
            received_at_sort: 0.0,
            is_read: false,
            has_attachments: false,
            body_preview: String::new(),
            body: Some(String::new()),
            body_type: "text".to_string(),
            attachments: Vec::new(),
            raw_mime: Some(Vec::new()),
        };

        assert!(fetched_imap_uids_cover(&[message.clone()], &[5]));
        assert!(!fetched_imap_uids_cover(&[message], &[5, 6]));
        assert!(!fetched_imap_uids_cover(&[], &[5]));
    }

    #[test]
    fn classifies_imap_special_use_and_common_mailbox_names() {
        assert_eq!(
            classify_imap_mailbox(&["junk".to_string()], "Mailbox"),
            Some("junkemail")
        );
        assert_eq!(
            classify_imap_mailbox(&["trash".to_string()], "Mailbox"),
            Some("deleteditems")
        );
        assert_eq!(classify_imap_mailbox(&[], "INBOX"), Some("inbox"));
        assert_eq!(
            classify_imap_mailbox(&["inbox".to_string()], "Mailbox"),
            Some("inbox")
        );
        assert_eq!(
            classify_imap_mailbox(&[], "其他文件夹/收件箱"),
            Some("inbox")
        );
        assert_eq!(
            classify_imap_mailbox(&[], "[Gmail]/Spam"),
            Some("junkemail")
        );
        assert_eq!(
            classify_imap_mailbox(&[], "Deleted Items"),
            Some("deleteditems")
        );
        assert_eq!(classify_imap_mailbox(&[], "垃圾邮件"), Some("junkemail"));
        assert_eq!(
            classify_imap_mailbox(&[], "已删除邮件"),
            Some("deleteditems")
        );
        assert_eq!(classify_imap_mailbox(&[], "回收站"), Some("deleteditems"));
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
    fn build_imap_select_variants_includes_quoted_and_unquoted_names() {
        assert_eq!(
            build_imap_select_variants("INBOX"),
            vec!["INBOX".to_string(), "\"INBOX\"".to_string()]
        );
        assert_eq!(
            build_imap_select_variants("\"垃圾邮件\""),
            vec![
                "\"垃圾邮件\"".to_string(),
                "垃圾邮件".to_string(),
            ]
        );
    }

    #[test]
    fn imap_flags_include_seen_detects_seen_flag() {
        assert!(imap_flags_include_seen(&[Flag::Seen]));
        assert!(!imap_flags_include_seen(&[Flag::Answered, Flag::Flagged]));
    }

    #[test]
    fn resolve_imap_received_time_prefers_internal_date() {
        let internal_date = chrono::DateTime::parse_from_rfc3339("2026-07-01T08:00:00+00:00")
            .expect("internal date");
        let (received_at, received_at_sort) = resolve_imap_received_time(
            Some(internal_date),
            "Tue, 07 Jul 2026 12:00:00 +0000",
        );
        assert_eq!(received_at, "2026-07-01T08:00:00+00:00");
        assert_eq!(received_at_sort, internal_date.timestamp() as f64);
    }

    #[test]
    fn resolve_imap_received_time_falls_back_to_mime_date() {
        let mime_date = "Tue, 07 Jul 2026 12:00:00 +0000";
        let expected_timestamp = mailparse::dateparse(mime_date).expect("mime date");
        let (received_at, received_at_sort) = resolve_imap_received_time(None, mime_date);
        assert_eq!(received_at, "2026-07-07T12:00:00+00:00");
        assert_eq!(received_at_sort, expected_timestamp as f64);
    }

    #[test]
    fn parse_imap_message_uses_seen_flag_from_fetch_meta() {
        let raw = concat!(
            "Subject: Seen flag\r\n",
            "From: sender@example.com\r\n",
            "To: user@example.com\r\n",
            "Date: Tue, 07 Jul 2026 12:00:00 +0000\r\n",
            "Content-Type: text/plain; charset=utf-8\r\n",
            "\r\n",
            "Hello"
        );
        let message = parse_imap_message(
            "inbox",
            7,
            raw.as_bytes(),
            ImapFetchMeta {
                is_read: true,
                internal_date: None,
            },
        );
        assert!(message.is_read);
        assert_eq!(message.subject, "Seen flag");
    }

    #[test]
    fn should_send_imap_client_id_for_qq_gmail_and_custom_providers() {
        fn account_with_provider(provider: &str) -> AccountCredentials {
            AccountCredentials {
                id: 1,
                email: "user@example.com".to_string(),
                provider: provider.to_string(),
                account_type: "imap".to_string(),
                client_id: String::new(),
                refresh_token: String::new(),
                imap_host: String::new(),
                imap_port: 993,
                proxy_chain: Vec::new(),
            }
        }

        assert!(should_send_imap_client_id(&account_with_provider("qq")));
        assert!(should_send_imap_client_id(&account_with_provider("gmail")));
        assert!(should_send_imap_client_id(&account_with_provider("imap_custom")));
        assert!(should_send_imap_client_id(&account_with_provider("netease_163")));
        assert!(!should_send_imap_client_id(&account_with_provider("graph")));
    }

    #[test]
    fn imap_html_preview_strips_tags_and_keeps_html_body() {
        let raw = concat!(
            "Subject: HTML preview\r\n",
            "From: sender@example.com\r\n",
            "To: user@example.com\r\n",
            "Date: Tue, 07 Jul 2026 12:00:00 +0000\r\n",
            "Content-Type: text/html; charset=utf-8\r\n",
            "\r\n",
            "<html><head><style>.x{display:none}</style></head>",
            "<body>.aw a {color: #FFFFFF; text-decoration: none;} ",
            "@font-face {font-family: Roboto; src: url(https://example.test/font.woff2);} ",
            "*{box-sizing:border-box}body{margin:0;padding:0}",
            "a[x-apple-data-detectors]{color:inherit!important;text-decoration:none}",
            "<p>Hello <strong>Gmail</strong><br>Code &amp; value</p></body></html>"
        );
        let message = parse_imap_message("inbox", 42, raw.as_bytes(), ImapFetchMeta::default());

        assert_eq!(message.body_type, "html");
        assert_eq!(message.body_preview, "Hello Gmail Code & value");
        assert!(message
            .body
            .unwrap_or_default()
            .contains("<strong>Gmail</strong>"));
    }

    #[test]
    fn detects_mail_provider_registry_defaults() {
        let gmail = detect_mail_provider("person@gmail.com", None, false).expect("gmail");
        assert_eq!(gmail.id, "gmail");
        assert_eq!(gmail.account_type, "imap");
        assert_eq!(gmail.default_imap_host, "imap.gmail.com");
        assert!(gmail.capabilities.contains(&"imap_folders"));
        assert!(gmail.capabilities.contains(&"remote_delete"));

        let qq = detect_mail_provider("user@foxmail.com", None, false).expect("qq");
        assert_eq!(qq.id, "qq");
        assert_eq!(qq.default_imap_host, "imap.qq.com");
        assert!(qq.capabilities.contains(&"imap_folders"));
        assert!(qq.capabilities.contains(&"remote_delete"));

        let netease = detect_mail_provider("manual@example.com", Some("163"), false).expect("163");
        assert_eq!(netease.id, "netease_163");
        assert_eq!(netease.default_imap_host, "imap.163.com");
        assert!(netease.capabilities.contains(&"imap_folders"));

        let graph = detect_mail_provider("user@example.com", None, true).expect("graph");
        assert_eq!(graph.id, "graph");
        assert!(graph.capabilities.contains(&"remote_delete"));
    }

    #[test]
    fn rejects_gmail_oauth_auth_url() {
        let error = build_graph_auth_url(&OAuthAuthUrlInput {
            client_id: "google-client".to_string(),
            redirect_uri: "http://127.0.0.1:53682/callback".to_string(),
            login_hint: Some("person@gmail.com".to_string()),
            provider: Some("gmail".to_string()),
        })
        .expect_err("gmail oauth disabled");

        assert!(error.to_string().contains("IMAP app password"));
    }
}
