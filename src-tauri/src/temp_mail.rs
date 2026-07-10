use crate::error::{AppError, AppResult};
use crate::models::{GenerateTempEmailInput, TempEmailMessage, TempEmailProviderConfig};
use mailparse::{parse_mail, MailHeaderMap, ParsedMail};
use reqwest::blocking::{Client, Response};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::time::Duration;

const DEFAULT_GPTMAIL_URL: &str = "https://mail.chatgpt.org.uk";
const DEFAULT_DUCKMAIL_URL: &str = "https://api.duckmail.sbs";

#[derive(Debug, Clone)]
pub struct CloudflareChannelCredentials {
    pub worker_url: String,
    pub admin_password: String,
    pub domains: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct TempMailboxCredentials {
    pub email: String,
    pub provider: String,
    pub base_url: String,
    pub api_key: String,
    pub token: String,
    pub account_id: String,
    pub cloudflare_channel: Option<CloudflareChannelCredentials>,
}

pub struct CreatedTempMailbox {
    pub email: String,
    pub provider: String,
    pub base_url: String,
    pub api_key: String,
    pub password: String,
    pub token: String,
    pub account_id: String,
}

fn client() -> AppResult<Client> {
    Client::builder().timeout(Duration::from_secs(30)).build()
        .map_err(|err| AppError::Internal(format!("failed to build temporary mail client: {err}")))
}

fn normalized_url(value: Option<String>, fallback: &str) -> String {
    value.unwrap_or_else(|| fallback.to_string()).trim().trim_end_matches('/').to_string()
}

fn response_json(response: Response) -> AppResult<Value> {
    let status = response.status();
    if status.as_u16() == 204 { return Ok(json!({"success": true})); }
    let body = response.text().map_err(network_error)?;
    if !status.is_success() {
        let detail = serde_json::from_str::<Value>(&body).ok().and_then(|value| value.get("message").or_else(|| value.get("error")).and_then(Value::as_str).map(str::to_string)).unwrap_or_else(|| format!("HTTP {status}"));
        return Err(AppError::Internal(format!("temporary mail provider request failed: {detail}")));
    }
    serde_json::from_str(&body).map_err(|err| AppError::Internal(format!("invalid temporary mail response: {err}")))
}

fn network_error(err: reqwest::Error) -> AppError {
    AppError::Internal(format!("temporary mail provider is unavailable: {err}"))
}

pub fn create(input: GenerateTempEmailInput, channel: Option<&CloudflareChannelCredentials>) -> AppResult<CreatedTempMailbox> {
    match input.provider.trim().to_lowercase().as_str() {
        "gptmail" => create_gptmail(input),
        "duckmail" => create_duckmail(input),
        "cloudflare" => create_cloudflare(input, channel),
        _ => Err(AppError::InvalidInput("provider must be gptmail, duckmail, or cloudflare".to_string())),
    }
}

pub fn list_domains(config: TempEmailProviderConfig, channel: Option<&CloudflareChannelCredentials>) -> AppResult<Vec<String>> {
    if config.provider.trim().eq_ignore_ascii_case("cloudflare") {
        return channel.map(|item| item.domains.clone()).ok_or_else(|| AppError::InvalidInput("Cloudflare channel is required".to_string()));
    }
    if !config.provider.trim().eq_ignore_ascii_case("duckmail") { return Ok(Vec::new()); }
    let url = normalized_url(config.base_url, DEFAULT_DUCKMAIL_URL);
    let api_key = config.api_key.unwrap_or_default();
    let c = client()?;
    let mut request = c.get(format!("{url}/domains"));
    if !api_key.trim().is_empty() { request = request.bearer_auth(api_key.trim()); }
    let value = response_json(request.send().map_err(network_error)?)?;
    Ok(value.get("hydra:member").and_then(Value::as_array).cloned().unwrap_or_default().iter().filter(|item| item.get("isVerified").and_then(Value::as_bool).unwrap_or(false)).filter_map(|item| item.get("domain").and_then(Value::as_str).map(str::to_string)).collect())
}

fn create_gptmail(input: GenerateTempEmailInput) -> AppResult<CreatedTempMailbox> {
    let url = normalized_url(input.base_url, DEFAULT_GPTMAIL_URL);
    let api_key = input.api_key.unwrap_or_else(|| "gpt-test".to_string()).trim().to_string();
    let c = client()?;
    let request = if input.prefix.as_deref().unwrap_or("").trim().is_empty() && input.domain.as_deref().unwrap_or("").trim().is_empty() {
        c.get(format!("{url}/api/generate-email")).header("X-API-Key", &api_key)
    } else {
        c.post(format!("{url}/api/generate-email")).header("X-API-Key", &api_key).json(&json!({"prefix": input.prefix, "domain": input.domain}))
    };
    let value = response_json(request.send().map_err(network_error)?)?;
    let email = value.pointer("/data/email").and_then(Value::as_str).ok_or_else(|| AppError::Internal("GPTMail did not return an email address".to_string()))?.to_string();
    Ok(CreatedTempMailbox { email, provider: "gptmail".to_string(), base_url: url, api_key, password: String::new(), token: String::new(), account_id: String::new() })
}

fn create_duckmail(input: GenerateTempEmailInput) -> AppResult<CreatedTempMailbox> {
    let url = normalized_url(input.base_url, DEFAULT_DUCKMAIL_URL);
    let username = input.username.unwrap_or_default().trim().to_string();
    let domain = input.domain.unwrap_or_default().trim().to_string();
    let password = input.password.unwrap_or_default();
    if username.len() < 3 || domain.is_empty() || password.len() < 6 { return Err(AppError::InvalidInput("DuckMail requires a username of at least 3 characters, a domain, and a password of at least 6 characters".to_string())); }
    let email = format!("{username}@{domain}");
    let api_key = input.api_key.unwrap_or_default().trim().to_string();
    let c = client()?;
    let mut account_request = c.post(format!("{url}/accounts")).json(&json!({"address": email, "password": password}));
    if !api_key.is_empty() { account_request = account_request.bearer_auth(&api_key); }
    let account = response_json(account_request.send().map_err(network_error)?)?;
    let account_id = account.get("id").and_then(Value::as_str).ok_or_else(|| AppError::Internal("DuckMail did not return an account id".to_string()))?.to_string();
    let token_value = response_json(c.post(format!("{url}/token")).json(&json!({"address": email, "password": password})).send().map_err(network_error)?)?;
    let token = token_value.get("token").and_then(Value::as_str).ok_or_else(|| AppError::Internal("DuckMail did not return an access token".to_string()))?.to_string();
    Ok(CreatedTempMailbox { email, provider: "duckmail".to_string(), base_url: url, api_key, password, token, account_id })
}

fn create_cloudflare(input: GenerateTempEmailInput, channel: Option<&CloudflareChannelCredentials>) -> AppResult<CreatedTempMailbox> {
    let channel = channel.ok_or_else(|| AppError::InvalidInput("Cloudflare channel is required".to_string()))?;
    let domain = input.domain.unwrap_or_default().trim().trim_start_matches('@').to_lowercase();
    if !channel.domains.iter().any(|item| item.eq_ignore_ascii_case(&domain)) { return Err(AppError::InvalidInput("selected domain does not belong to this Cloudflare channel".to_string())); }
    let username = input.username.or(input.prefix).unwrap_or_else(|| format!("mail{}", uuid::Uuid::new_v4().simple().to_string().chars().take(10).collect::<String>()));
    if username.trim().len() < 3 { return Err(AppError::InvalidInput("Cloudflare username must contain at least 3 characters".to_string())); }
    let value = cloudflare_request(channel, reqwest::Method::POST, "/admin/new_address", None, Some(json!({"enablePrefix": true, "name": username.trim(), "domain": domain})))?;
    let email = value.get("address").and_then(Value::as_str).ok_or_else(|| AppError::Internal("Cloudflare Worker did not return an address".to_string()))?.to_string();
    let token = value.get("jwt").and_then(Value::as_str).unwrap_or_default().to_string();
    let account_id = value.get("id").or_else(|| value.get("address_id")).map(value_id).unwrap_or_default();
    Ok(CreatedTempMailbox { email, provider: "cloudflare".to_string(), base_url: channel.worker_url.clone(), api_key: String::new(), password: String::new(), token, account_id })
}

fn cloudflare_request(channel: &CloudflareChannelCredentials, method: reqwest::Method, endpoint: &str, query: Option<&[(&str, String)]>, body: Option<Value>) -> AppResult<Value> {
    let mut request = client()?.request(method, format!("{}{}", channel.worker_url.trim_end_matches('/'), endpoint)).header("x-admin-auth", &channel.admin_password).header("Content-Type", "application/json");
    if let Some(query) = query { request = request.query(query); }
    if let Some(body) = body { request = request.json(&body); }
    response_json(request.send().map_err(network_error)?)
}

pub fn list_messages(mailbox: &TempMailboxCredentials) -> AppResult<Vec<TempEmailMessage>> {
    let value = match mailbox.provider.as_str() {
        "gptmail" => response_json(client()?.get(format!("{}/api/emails", mailbox.base_url)).header("X-API-Key", &mailbox.api_key).query(&[("email", &mailbox.email)]).send().map_err(network_error)?)?,
        "duckmail" => response_json(client()?.get(format!("{}/messages", mailbox.base_url)).bearer_auth(&mailbox.token).send().map_err(network_error)?)?,
        "cloudflare" => {
            let channel = mailbox.cloudflare_channel.as_ref().ok_or_else(|| AppError::InvalidInput("Cloudflare channel is missing".to_string()))?;
            cloudflare_request(channel, reqwest::Method::GET, "/admin/mails", Some(&[("limit", "50".to_string()), ("offset", "0".to_string()), ("address", mailbox.email.clone())]), None)?
        }
        _ => return Err(AppError::InvalidInput("unsupported temporary mail provider".to_string())),
    };
    let items = if mailbox.provider == "gptmail" { value.pointer("/data/emails").and_then(Value::as_array) } else if mailbox.provider == "cloudflare" { cloudflare_message_items(&value) } else { value.get("hydra:member").and_then(Value::as_array) }.cloned().unwrap_or_default();
    Ok(items.iter().filter_map(|item| if mailbox.provider == "cloudflare" { normalize_cloudflare_message(item, &mailbox.email, false) } else { Some(normalize_message(item, &mailbox.email, false)) }).collect())
}

pub fn get_message(mailbox: &TempMailboxCredentials, message_id: &str) -> AppResult<TempEmailMessage> {
    if mailbox.provider == "cloudflare" {
        let channel = mailbox.cloudflare_channel.as_ref().ok_or_else(|| AppError::InvalidInput("Cloudflare channel is missing".to_string()))?;
        let value = cloudflare_request(channel, reqwest::Method::GET, "/admin/mails", Some(&[("limit", "50".to_string()), ("offset", "0".to_string()), ("address", mailbox.email.clone())]), None)?;
        return cloudflare_message_items(&value).cloned().unwrap_or_default().iter().filter_map(|item| normalize_cloudflare_message(item, &mailbox.email, true)).find(|item| item.id == message_id).ok_or_else(|| AppError::InvalidInput("temporary email message not found".to_string()));
    }
    let value = if mailbox.provider == "gptmail" {
        let result = response_json(client()?.get(format!("{}/api/email/{}", mailbox.base_url, urlencoding::encode(message_id))).header("X-API-Key", &mailbox.api_key).send().map_err(network_error)?)?;
        result.get("data").cloned().unwrap_or(result)
    } else {
        response_json(client()?.get(format!("{}/messages/{}", mailbox.base_url, urlencoding::encode(message_id))).bearer_auth(&mailbox.token).send().map_err(network_error)?)?
    };
    Ok(normalize_message(&value, &mailbox.email, true))
}

fn cloudflare_message_items(value: &Value) -> Option<&Vec<Value>> {
    if let Some(items) = value.as_array() { return Some(items); }
    value.get("results").or_else(|| value.get("mails")).or_else(|| value.get("emails")).and_then(Value::as_array).or_else(|| value.pointer("/data/results").and_then(Value::as_array)).or_else(|| value.get("data").and_then(Value::as_array))
}

fn value_id(value: &Value) -> String {
    value.as_str().map(str::to_string).or_else(|| value.as_i64().map(|id| id.to_string())).unwrap_or_default()
}

fn normalize_cloudflare_message(item: &Value, recipient: &str, include_body: bool) -> Option<TempEmailMessage> {
    let raw = item.get("raw").or_else(|| item.get("raw_content")).or_else(|| item.get("source_raw")).and_then(Value::as_str)?;
    let parsed = parse_mail(raw.as_bytes()).ok()?;
    let id = item.get("id").or_else(|| item.get("mail_id")).map(value_id).filter(|value| !value.is_empty()).or_else(|| parsed.headers.get_first_value("Message-ID")).unwrap_or_else(|| format!("cf-{:x}", Sha256::digest(raw.as_bytes())));
    let (plain, html) = mime_bodies(&parsed);
    let body = if html.is_empty() { plain.clone() } else { html.clone() };
    Some(TempEmailMessage { id, sender: parsed.headers.get_first_value("From").unwrap_or_else(|| "Unknown".to_string()), recipients: parsed.headers.get_first_value("To").unwrap_or_else(|| recipient.to_string()), subject: parsed.headers.get_first_value("Subject").unwrap_or_default(), body_preview: plain.chars().take(200).collect(), body: include_body.then_some(body), body_type: if html.is_empty() { "text" } else { "html" }.to_string(), received_at: parsed.headers.get_first_value("Date").unwrap_or_default() })
}

fn mime_bodies(parsed: &ParsedMail<'_>) -> (String, String) {
    if parsed.subparts.is_empty() {
        let body = parsed.get_body().unwrap_or_default();
        if parsed.ctype.mimetype.eq_ignore_ascii_case("text/html") { return (String::new(), body); }
        if parsed.ctype.mimetype.eq_ignore_ascii_case("text/plain") { return (body, String::new()); }
        return (String::new(), String::new());
    }
    let mut plain = String::new(); let mut html = String::new();
    for part in &parsed.subparts { let (part_plain, part_html) = mime_bodies(part); if plain.is_empty() { plain = part_plain; } if html.is_empty() { html = part_html; } }
    (plain, html)
}

fn text(value: Option<&Value>) -> String { value.and_then(Value::as_str).unwrap_or_default().to_string() }

fn normalize_message(item: &Value, recipient: &str, include_body: bool) -> TempEmailMessage {
    let sender = item.get("from_address").and_then(Value::as_str).or_else(|| item.get("from").and_then(|value| value.get("address")).and_then(Value::as_str)).or_else(|| item.get("from").and_then(Value::as_str)).unwrap_or("Unknown").to_string();
    let html = item.get("html_content").and_then(Value::as_str).map(str::to_string).or_else(|| item.get("html").and_then(|value| value.as_array().and_then(|values| values.first()).and_then(Value::as_str).or_else(|| value.as_str())).map(str::to_string)).unwrap_or_default();
    let plain = text(item.get("content").or_else(|| item.get("text")));
    let body = if html.is_empty() { plain.clone() } else { html.clone() };
    let preview = if plain.is_empty() { text(item.get("intro").or_else(|| item.get("body_preview"))) } else { plain.chars().take(200).collect() };
    TempEmailMessage { id: text(item.get("id").or_else(|| item.get("message_id"))), sender, recipients: recipient.to_string(), subject: text(item.get("subject")), body_preview: preview, body: include_body.then_some(body), body_type: if html.is_empty() { "text" } else { "html" }.to_string(), received_at: text(item.get("createdAt").or_else(|| item.get("created_at")).or_else(|| item.get("date"))) }
}

pub fn delete_remote(mailbox: &TempMailboxCredentials) -> AppResult<()> {
    if mailbox.provider == "duckmail" && !mailbox.account_id.is_empty() {
        let response = client()?.delete(format!("{}/accounts/{}", mailbox.base_url, urlencoding::encode(&mailbox.account_id))).bearer_auth(&mailbox.token).send().map_err(network_error)?;
        if !response.status().is_success() { return Err(AppError::Internal(format!("DuckMail account deletion failed: HTTP {}", response.status()))); }
    }
    if mailbox.provider == "cloudflare" {
        let channel = mailbox.cloudflare_channel.as_ref().ok_or_else(|| AppError::InvalidInput("Cloudflare channel is missing".to_string()))?;
        if mailbox.account_id.is_empty() { return Err(AppError::InvalidInput("Cloudflare address id is missing".to_string())); }
        let _ = cloudflare_request(channel, reqwest::Method::DELETE, &format!("/admin/delete_address/{}", urlencoding::encode(&mailbox.account_id)), None, None)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_cloudflare_raw_mime() {
        let value = json!({"id": 42, "raw": "From: Sender <sender@example.com>\r\nTo: box@example.test\r\nSubject: Cloudflare mail\r\nDate: Thu, 10 Jul 2026 10:00:00 +0000\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nYour code is 654321"});
        let message = normalize_cloudflare_message(&value, "box@example.test", true).expect("message");
        assert_eq!(message.id, "42");
        assert_eq!(message.subject, "Cloudflare mail");
        assert!(message.body.unwrap().contains("654321"));
    }

    #[test]
    fn cloudflare_raw_mime_without_ids_uses_stable_hash() {
        let value = json!({"raw": "From: sender@example.com\r\nTo: box@example.test\r\nSubject: Stable\r\n\r\nBody"});
        let first = normalize_cloudflare_message(&value, "box@example.test", false).expect("first");
        let second = normalize_cloudflare_message(&value, "box@example.test", true).expect("second");
        assert_eq!(first.id, second.id);
        assert!(first.id.starts_with("cf-"));
    }

    #[test]
    fn rejects_incomplete_duckmail_account() {
        let result = create(GenerateTempEmailInput { provider: "duckmail".to_string(), base_url: None, api_key: None, prefix: None, domain: Some(String::new()), username: Some("ab".to_string()), password: Some("123".to_string()), cloudflare_channel_id: None }, None);
        assert!(matches!(result, Err(AppError::InvalidInput(_))));
    }
}
