use crate::providers;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ImportedAccount {
    pub email: String,
    pub password: String,
    pub client_id: String,
    pub refresh_token: String,
    pub remark: String,
    pub provider: Option<String>,
}

pub fn parse_accounts(raw: &str) -> Vec<ImportedAccount> {
    raw.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }

            let parts = split_line(trimmed)
                .into_iter()
                .map(|part| part.trim().to_string())
                .collect::<Vec<_>>();
            let explicit_provider = find_explicit_provider(&parts);
            let mut positional_parts = parts
                .into_iter()
                .filter(|part| !is_provider_assignment(part))
                .collect::<Vec<_>>();
            let mut provider = explicit_provider;
            if provider.is_none()
                && positional_parts.len() >= 2
                && is_provider_token(&positional_parts[0])
                && positional_parts[1].contains('@')
            {
                provider =
                    providers::normalize_mail_provider_id(&positional_parts[0]).map(str::to_string);
                positional_parts.remove(0);
            }

            let email_index = positional_parts
                .iter()
                .position(|part| part.contains('@'))?;
            let email = positional_parts.get(email_index)?.trim().to_lowercase();

            Some(ImportedAccount {
                email,
                password: positional_parts
                    .get(email_index + 1)
                    .cloned()
                    .unwrap_or_default(),
                client_id: positional_parts
                    .get(email_index + 2)
                    .cloned()
                    .unwrap_or_default(),
                refresh_token: positional_parts
                    .get(email_index + 3)
                    .cloned()
                    .unwrap_or_default(),
                remark: positional_parts
                    .get(email_index + 4)
                    .cloned()
                    .unwrap_or_default(),
                provider,
            })
        })
        .collect()
}

fn split_line(line: &str) -> Vec<&str> {
    for delimiter in ["----", "|||", "\t", ","] {
        if line.contains(delimiter) {
            return line.split(delimiter).collect();
        }
    }
    vec![line]
}

fn find_explicit_provider(parts: &[String]) -> Option<String> {
    parts.iter().find_map(|part| {
        if !is_provider_assignment(part) {
            return None;
        }
        let (_, value) = part.split_once('=').or_else(|| part.split_once(':'))?;
        providers::normalize_mail_provider_id(value).map(str::to_string)
    })
}

fn is_provider_assignment(value: &str) -> bool {
    let lower = value.trim().to_ascii_lowercase();
    lower.starts_with("provider=") || lower.starts_with("provider:")
}

fn is_provider_token(value: &str) -> bool {
    providers::normalize_mail_provider_id(value).is_some()
}

#[cfg(test)]
mod tests {
    use super::parse_accounts;

    #[test]
    fn parses_legacy_outlook_format() {
        let rows = parse_accounts("user@example.com----pass----cid----rt----primary");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].email, "user@example.com");
        assert_eq!(rows[0].refresh_token, "rt");
        assert_eq!(rows[0].provider, None);
    }

    #[test]
    fn parses_explicit_provider_and_domain_detection_inputs() {
        let rows = parse_accounts(
            "provider=qq----user@example.com----auth\nnetease_163----mail@custom.test----secret\nperson@gmail.com",
        );
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].provider.as_deref(), Some("qq"));
        assert_eq!(rows[1].provider.as_deref(), Some("netease_163"));
        assert_eq!(rows[2].email, "person@gmail.com");
    }

    #[test]
    fn explicit_provider_takes_priority_over_positional_provider_token() {
        let rows = parse_accounts("provider=gmail----qq----user@example.com----secret");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].email, "user@example.com");
        assert_eq!(rows[0].password, "secret");
        assert_eq!(rows[0].provider.as_deref(), Some("gmail"));
    }
}
