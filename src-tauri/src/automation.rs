use crate::error::{AppError, AppResult};
use crate::models::{ForwardContent, Settings};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use lettre::message::Mailbox;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use native_tls::TlsConnector;
use reqwest::{blocking::Client, Proxy};
use serde_json::json;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

pub fn configured_forward_channels(settings: &Settings) -> Vec<&'static str> {
    let mut channels = Vec::new();
    if !settings.forward_smtp_host.trim().is_empty() && !settings.forward_smtp_to.trim().is_empty()
    {
        channels.push("smtp");
    }
    if !settings.forward_telegram_bot_token.trim().is_empty()
        && !settings.forward_telegram_chat_id.trim().is_empty()
    {
        channels.push("telegram");
    }
    if !settings.forward_wecom_webhook.trim().is_empty() {
        channels.push("wecom");
    }
    channels
}

pub fn forward_message(
    settings: &Settings,
    channel: &str,
    content: &ForwardContent,
    proxy_chain: &[String],
) -> AppResult<()> {
    match channel {
        "smtp" => send_smtp(settings, content, proxy_chain),
        "telegram" => send_telegram(settings, content, proxy_chain),
        "wecom" => send_wecom(settings, content, proxy_chain),
        value => Err(AppError::InvalidInput(format!(
            "unsupported forwarding channel: {value}"
        ))),
    }
}

pub fn upload_webdav(settings: &Settings, file_name: &str, bytes: Vec<u8>) -> AppResult<String> {
    let base_url = settings.webdav_url.trim();
    if base_url.is_empty() {
        return Err(AppError::InvalidInput("WebDAV URL is required".to_string()));
    }
    let target = format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        urlencoding::encode(file_name)
    );
    let client = http_client()?;
    let mut request = client.put(&target).body(bytes);
    if !settings.webdav_username.trim().is_empty() {
        request = request.basic_auth(
            settings.webdav_username.trim().to_string(),
            Some(settings.webdav_password.clone()),
        );
    }
    let response = request.send().map_err(network_error)?;
    if !response.status().is_success() {
        return Err(AppError::Internal(format!(
            "WebDAV upload failed: HTTP {} {}",
            response.status(),
            response.text().unwrap_or_default()
        )));
    }
    Ok(target)
}

fn send_smtp(settings: &Settings, content: &ForwardContent, proxy_chain: &[String]) -> AppResult<()> {
    let host = settings.forward_smtp_host.trim();
    let to = split_recipients(&settings.forward_smtp_to);
    if host.is_empty() || to.is_empty() {
        return Err(AppError::InvalidInput(
            "SMTP host and recipient are required".to_string(),
        ));
    }
    let from = first_non_empty(&[
        settings.forward_smtp_from.as_str(),
        settings.forward_smtp_username.as_str(),
        content.account_email.as_str(),
    ])
    .ok_or_else(|| AppError::InvalidInput("SMTP sender is required".to_string()))?;

    let mut builder = Message::builder()
        .from(parse_mailbox(from)?)
        .subject(format!("Forwarded: {}", fallback_subject(&content.subject)));
    for recipient in &to {
        builder = builder.to(parse_mailbox(recipient)?);
    }
    let email = builder
        .body(render_forward_text(content))
        .map_err(|err| AppError::Internal(format!("build SMTP message failed: {err}")))?;

    if !proxy_chain.is_empty() {
        return send_smtp_via_proxy_chain(settings, host, from, &to, &email, proxy_chain);
    }

    let mut transport = SmtpTransport::relay(host)
        .map_err(|err| AppError::Internal(format!("SMTP relay setup failed: {err}")))?
        .port(settings.forward_smtp_port.clamp(1, 65535) as u16);
    if !settings.forward_smtp_username.trim().is_empty()
        || !settings.forward_smtp_password.is_empty()
    {
        transport = transport.credentials(Credentials::new(
            settings.forward_smtp_username.trim().to_string(),
            settings.forward_smtp_password.clone(),
        ));
    }
    let mailer = transport.build();
    mailer
        .send(&email)
        .map_err(|err| AppError::Internal(format!("SMTP send failed: {err}")))?;
    Ok(())
}

fn send_smtp_via_proxy_chain(
    settings: &Settings,
    host: &str,
    from: &str,
    recipients: &[&str],
    email: &Message,
    proxy_chain: &[String],
) -> AppResult<()> {
    let mut last_error = String::new();
    for proxy_url in proxy_chain {
        match send_smtp_via_proxy(settings, host, from, recipients, email, proxy_url) {
            Ok(()) => return Ok(()),
            Err(err) => last_error = format!("{err}"),
        }
    }
    Err(AppError::Internal(format!(
        "all proxy attempts failed for SMTP forward: {}",
        if last_error.is_empty() { "unknown SMTP proxy error" } else { last_error.as_str() }
    )))
}

fn send_smtp_via_proxy(
    settings: &Settings,
    host: &str,
    from: &str,
    recipients: &[&str],
    email: &Message,
    proxy_url: &str,
) -> AppResult<()> {
    let port = settings.forward_smtp_port.clamp(1, 65535) as u16;
    let tcp = connect_http_proxy_tunnel(proxy_url, host, port)?;
    let tls = TlsConnector::builder()
        .build()
        .map_err(|err| AppError::Internal(format!("SMTP TLS setup failed: {err}")))?;
    let mut stream = if port == 465 {
        let mut stream = tls
            .connect(host, tcp)
            .map_err(|err| AppError::Internal(format!("SMTP TLS handshake failed: {err}")))?;
        expect_smtp(&mut stream, &["220"])?;
        stream
    } else {
        let mut tcp = tcp;
        expect_smtp(&mut tcp, &["220"])?;
        smtp_command(&mut tcp, "EHLO outlook-email.local\r\n", &["250"])?;
        smtp_command(&mut tcp, "STARTTLS\r\n", &["220"])?;
        let mut stream = tls
            .connect(host, tcp)
            .map_err(|err| AppError::Internal(format!("SMTP STARTTLS handshake failed: {err}")))?;
        smtp_command(&mut stream, "EHLO outlook-email.local\r\n", &["250"])?;
        stream
    };

    if !settings.forward_smtp_username.trim().is_empty() || !settings.forward_smtp_password.is_empty() {
        let token = STANDARD.encode(format!(
            "\0{}\0{}",
            settings.forward_smtp_username.trim(),
            settings.forward_smtp_password
        ));
        smtp_command(&mut stream, &format!("AUTH PLAIN {token}\r\n"), &["235"])?;
    }
    smtp_command(&mut stream, &format!("MAIL FROM:<{}>\r\n", smtp_path(from)), &["250"])?;
    for recipient in recipients {
        smtp_command(&mut stream, &format!("RCPT TO:<{}>\r\n", smtp_path(recipient)), &["250", "251"])?;
    }
    smtp_command(&mut stream, "DATA\r\n", &["354"])?;
    write_smtp_data(&mut stream, &email.formatted())?;
    expect_smtp(&mut stream, &["250"])?;
    let _ = smtp_command(&mut stream, "QUIT\r\n", &["221"]);
    Ok(())
}

fn send_telegram(settings: &Settings, content: &ForwardContent, proxy_chain: &[String]) -> AppResult<()> {
    let token = settings.forward_telegram_bot_token.trim();
    let chat_id = settings.forward_telegram_chat_id.trim();
    if token.is_empty() || chat_id.is_empty() {
        return Err(AppError::InvalidInput(
            "Telegram bot token and chat id are required".to_string(),
        ));
    }
    let url = format!("https://api.telegram.org/bot{token}/sendMessage");
    with_proxy_chain(proxy_chain, |client| {
        let response = client
            .post(&url)
            .json(&json!({
                "chat_id": chat_id,
                "text": trim_message(&render_forward_text(content), 3900),
                "disable_web_page_preview": true
            }))
            .send()
            .map_err(network_error)?;
        if !response.status().is_success() {
            return Err(AppError::Internal(format!(
                "Telegram forward failed: HTTP {} {}",
                response.status(),
                response.text().unwrap_or_default()
            )));
        }
        Ok(())
    })
}

fn send_wecom(settings: &Settings, content: &ForwardContent, proxy_chain: &[String]) -> AppResult<()> {
    let webhook = settings.forward_wecom_webhook.trim();
    if webhook.is_empty() {
        return Err(AppError::InvalidInput(
            "WeCom webhook is required".to_string(),
        ));
    }
    with_proxy_chain(proxy_chain, |client| {
        let response = client
            .post(webhook)
            .json(&json!({
                "msgtype": "markdown",
                "markdown": {
                    "content": trim_message(&render_forward_markdown(content), 3900)
                }
            }))
            .send()
            .map_err(network_error)?;
        if !response.status().is_success() {
            return Err(AppError::Internal(format!(
                "WeCom forward failed: HTTP {} {}",
                response.status(),
                response.text().unwrap_or_default()
            )));
        }
        Ok(())
    })
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

fn with_proxy_chain<T>(proxy_chain: &[String], operation: impl Fn(&Client) -> AppResult<T>) -> AppResult<T> {
    if proxy_chain.is_empty() {
        let client = http_client()?;
        return operation(&client);
    }

    let mut last_error = String::new();
    for proxy_url in proxy_chain {
        let client = http_client_for_proxy(Some(proxy_url))?;
        match operation(&client) {
            Ok(value) => return Ok(value),
            Err(err) => last_error = format!("{err}"),
        }
    }
    Err(AppError::Internal(format!(
        "all proxy attempts failed for forwarding request: {}",
        if last_error.is_empty() { "unknown network error" } else { last_error.as_str() }
    )))
}

fn connect_http_proxy_tunnel(proxy_url: &str, host: &str, port: u16) -> AppResult<TcpStream> {
    let url = reqwest::Url::parse(proxy_url)
        .map_err(|err| AppError::InvalidInput(format!("invalid proxy URL {proxy_url}: {err}")))?;
    if url.scheme() != "http" {
        return Err(AppError::InvalidInput(
            "SMTP proxy tunnel currently supports http:// proxies only".to_string(),
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
        return Err(AppError::Internal(format!("proxy CONNECT failed: {status_line}")));
    }
    Ok(stream)
}

fn smtp_command<S: Read + Write>(stream: &mut S, command: &str, expected: &[&str]) -> AppResult<String> {
    stream
        .write_all(command.as_bytes())
        .map_err(|err| AppError::Internal(format!("SMTP command write failed: {err}")))?;
    stream
        .flush()
        .map_err(|err| AppError::Internal(format!("SMTP command flush failed: {err}")))?;
    expect_smtp(stream, expected)
}

fn expect_smtp<S: Read>(stream: &mut S, expected: &[&str]) -> AppResult<String> {
    let response = read_smtp_response(stream)?;
    if expected.iter().any(|code| response.starts_with(code)) {
        return Ok(response);
    }
    Err(AppError::Internal(format!("SMTP unexpected response: {}", response.trim())))
}

fn read_smtp_response<S: Read>(stream: &mut S) -> AppResult<String> {
    let mut response = String::new();
    loop {
        let line = read_smtp_line(stream)?;
        let is_final = line.as_bytes().get(3) == Some(&b' ');
        response.push_str(&line);
        if is_final {
            break;
        }
    }
    Ok(response)
}

fn read_smtp_line<S: Read>(stream: &mut S) -> AppResult<String> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 1];
    loop {
        let read = stream
            .read(&mut buffer)
            .map_err(|err| AppError::Internal(format!("SMTP response read failed: {err}")))?;
        if read == 0 {
            return Err(AppError::Internal("SMTP connection closed".to_string()));
        }
        bytes.push(buffer[0]);
        if buffer[0] == b'\n' {
            break;
        }
        if bytes.len() > 8192 {
            return Err(AppError::Internal("SMTP response line is too long".to_string()));
        }
    }
    String::from_utf8(bytes).map_err(|err| AppError::Internal(format!("SMTP response is not UTF-8: {err}")))
}

fn write_smtp_data<S: Write>(stream: &mut S, bytes: &[u8]) -> AppResult<()> {
    let normalized = String::from_utf8_lossy(bytes).replace("\r\n", "\n").replace('\n', "\r\n");
    for line in normalized.split("\r\n") {
        if line.starts_with('.') {
            stream
                .write_all(b".")
                .map_err(|err| AppError::Internal(format!("SMTP DATA dot-stuff failed: {err}")))?;
        }
        stream
            .write_all(line.as_bytes())
            .map_err(|err| AppError::Internal(format!("SMTP DATA write failed: {err}")))?;
        stream
            .write_all(b"\r\n")
            .map_err(|err| AppError::Internal(format!("SMTP DATA newline write failed: {err}")))?;
    }
    stream
        .write_all(b".\r\n")
        .map_err(|err| AppError::Internal(format!("SMTP DATA terminator write failed: {err}")))?;
    stream
        .flush()
        .map_err(|err| AppError::Internal(format!("SMTP DATA flush failed: {err}")))
}

fn smtp_path(value: &str) -> String {
    let value = value.trim();
    if let (Some(start), Some(end)) = (value.find('<'), value.rfind('>')) {
        if end > start {
            return value[(start + 1)..end].trim().to_string();
        }
    }
    value.to_string()
}

fn network_error(err: reqwest::Error) -> AppError {
    AppError::Internal(format!("network request failed: {err}"))
}

fn parse_mailbox(value: &str) -> AppResult<Mailbox> {
    value
        .parse::<Mailbox>()
        .map_err(|err| AppError::InvalidInput(format!("invalid email address {value}: {err}")))
}

fn split_recipients(value: &str) -> Vec<&str> {
    value
        .split([',', ';', '\n'])
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .collect()
}

fn first_non_empty<'a>(values: &[&'a str]) -> Option<&'a str> {
    values
        .iter()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
}

fn fallback_subject(subject: &str) -> &str {
    if subject.trim().is_empty() {
        "(no subject)"
    } else {
        subject.trim()
    }
}

fn render_forward_text(content: &ForwardContent) -> String {
    let body = content
        .body
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(&content.body_preview);
    format!(
        "Account: {}\nMessage ID: {}\nFrom: {}\nReceived: {}\nSubject: {}\n\n{}",
        content.account_email,
        content.message_id,
        content.sender,
        content.received_at,
        fallback_subject(&content.subject),
        body
    )
}

fn render_forward_markdown(content: &ForwardContent) -> String {
    let body = content
        .body
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(&content.body_preview);
    format!(
        "**{}**\n> Account: {}\n> Message ID: {}\n> From: {}\n> Received: {}\n\n{}",
        fallback_subject(&content.subject),
        content.account_email,
        content.message_id,
        content.sender,
        content.received_at,
        body
    )
}

fn trim_message(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut output = value
        .chars()
        .take(max_chars.saturating_sub(16))
        .collect::<String>();
    output.push_str("\n...[trimmed]");
    output
}
