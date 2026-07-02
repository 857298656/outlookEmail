use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ImportedAccount {
    pub email: String,
    pub password: String,
    pub client_id: String,
    pub refresh_token: String,
    pub remark: String,
}

pub fn parse_accounts(raw: &str) -> Vec<ImportedAccount> {
    raw.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }

            let parts = split_line(trimmed);
            let email = parts.first()?.trim().to_lowercase();
            if !email.contains('@') {
                return None;
            }

            Some(ImportedAccount {
                email,
                password: parts.get(1).unwrap_or(&"").trim().to_string(),
                client_id: parts.get(2).unwrap_or(&"").trim().to_string(),
                refresh_token: parts.get(3).unwrap_or(&"").trim().to_string(),
                remark: parts.get(4).unwrap_or(&"").trim().to_string(),
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

#[cfg(test)]
mod tests {
    use super::parse_accounts;

    #[test]
    fn parses_legacy_outlook_format() {
        let rows = parse_accounts("user@example.com----pass----cid----rt----primary");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].email, "user@example.com");
        assert_eq!(rows[0].refresh_token, "rt");
    }
}
