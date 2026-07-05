use crate::automation;
use crate::crypto;
use crate::error::{AppError, AppResult};
use crate::import::ImportedAccount;
use crate::models::*;
use crate::providers;
use chrono::{DateTime, Duration as ChronoDuration, NaiveDateTime, Utc};
use directories::ProjectDirs;
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, params_from_iter, Connection, OpenFlags, OptionalExtension};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub struct Database {
    conn: Connection,
    db_path: PathBuf,
    crypto_key: Option<[u8; 32]>,
}

#[derive(Debug, Clone)]
struct MailMessageRef {
    id: i64,
    account_id: i64,
    account_email: String,
    folder: String,
    provider_message_id: String,
}

#[derive(Debug, Clone)]
struct ExportMailMessageRow {
    id: i64,
    account_id: i64,
    account_email: String,
    folder: String,
    provider_message_id: String,
    subject: String,
    sender: String,
    recipients: String,
    cc: String,
    received_at: String,
    is_read: bool,
    body_preview: String,
    body: Option<String>,
    body_type: String,
    attachments: Vec<AttachmentInfo>,
}

struct AutomationRunFilter {
    job_type: String,
    trigger_type: String,
    status: String,
    search: String,
    limit: i64,
}

impl AutomationRunFilter {
    fn from_query(query: AutomationRunQuery) -> AppResult<Self> {
        Ok(Self {
            job_type: normalize_automation_value(query.job_type.as_deref(), &["refresh", "forwarding", "backup", "retry"], "job_type")?,
            trigger_type: normalize_automation_value(query.trigger_type.as_deref(), &["manual", "schedule"], "trigger_type")?,
            status: normalize_automation_value(query.status.as_deref(), &["success", "failed"], "status")?,
            search: query.search.unwrap_or_default().trim().to_string(),
            limit: query.limit.unwrap_or(100).clamp(1, 500),
        })
    }

    fn from_clear_input(input: &ClearAutomationRunsInput) -> AppResult<Self> {
        Ok(Self {
            job_type: normalize_automation_value(input.job_type.as_deref(), &["refresh", "forwarding", "backup", "retry"], "job_type")?,
            trigger_type: normalize_automation_value(input.trigger_type.as_deref(), &["manual", "schedule"], "trigger_type")?,
            status: normalize_automation_value(input.status.as_deref(), &["success", "failed"], "status")?,
            search: input.search.clone().unwrap_or_default().trim().to_string(),
            limit: 500,
        })
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
struct MailRetryPayload {
    account_id: i64,
    account_email: String,
    folder: String,
    provider_message_id: String,
    is_read: Option<bool>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct ForwardRetryPayload {
    account_id: i64,
    message_id: String,
    channel: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct RefreshRetryPayload {
    account_id: i64,
    folder: String,
    top: usize,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct BackupRetryPayload {
    target: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct TempRefreshRetryPayload {
    email: String,
    provider: String,
}

impl Database {
    pub fn open() -> AppResult<Self> {
        let db_path = resolve_db_path();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| AppError::Internal(err.to_string()))?;
        }

        let conn = Connection::open(&db_path)?;
        let mut database = Self {
            conn,
            db_path,
            crypto_key: None,
        };
        database.initialize_schema()?;
        Ok(database)
    }

    pub fn db_path(&self) -> String {
        self.db_path.to_string_lossy().to_string()
    }

    pub fn is_initialized(&self) -> AppResult<bool> {
        Ok(self.get_config("password_hash")?.is_some())
    }

    pub fn is_unlocked(&self) -> bool {
        self.crypto_key.is_some()
    }

    pub fn initialize_app(&mut self, password: &str) -> AppResult<()> {
        validate_password(password)?;
        if self.is_initialized()? {
            return Err(AppError::InvalidInput("app is already initialized".to_string()));
        }

        let hash = crypto::hash_password(password)?;
        let salt = crypto::random_salt();
        self.set_config("password_hash", &hash)?;
        self.set_config("crypto_salt", &salt)?;
        self.crypto_key = Some(crypto::derive_key(password, &salt));
        self.ensure_default_data()?;
        self.audit("app.initialized", "settings", None, "initial setup")?;
        Ok(())
    }

    pub fn unlock(&mut self, password: &str) -> AppResult<()> {
        let hash = self
            .get_config("password_hash")?
            .ok_or_else(|| AppError::InvalidInput("app is not initialized".to_string()))?;
        if !crypto::verify_password(password, &hash)? {
            return Err(AppError::Unauthorized);
        }
        let salt = self
            .get_config("crypto_salt")?
            .ok_or_else(|| AppError::Crypto("missing crypto salt".to_string()))?;
        self.crypto_key = Some(crypto::derive_key(password, &salt));
        self.audit("app.unlocked", "session", None, "local unlock")?;
        Ok(())
    }

    pub fn lock(&mut self) {
        self.crypto_key = None;
    }

    pub fn app_status(&self) -> AppResult<AppStatus> {
        Ok(AppStatus {
            initialized: self.is_initialized()?,
            unlocked: self.is_unlocked(),
            db_path: self.db_path(),
            account_count: self.scalar_count("SELECT COUNT(*) FROM accounts")?,
            message_count: self.scalar_count("SELECT COUNT(*) FROM retained_mail_messages")?,
        })
    }

    pub fn list_groups(&self) -> AppResult<Vec<Group>> {
        self.require_unlocked()?;
        let mut stmt = self.conn.prepare(
            "
            SELECT g.id, g.name, COALESCE(g.description, ''), g.color,
                   COALESCE(g.proxy_url, ''), COALESCE(g.fallback_proxy_url_1, ''),
                   COALESCE(g.fallback_proxy_url_2, ''), g.parent_id, g.level,
                   g.sort_order, COUNT(a.id) AS account_count
            FROM groups g
            LEFT JOIN accounts a ON a.group_id = g.id
            GROUP BY g.id
            ORDER BY g.level ASC, g.sort_order ASC, g.name ASC
            ",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Group {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                color: row.get(3)?,
                proxy_url: row.get(4)?,
                fallback_proxy_url_1: row.get(5)?,
                fallback_proxy_url_2: row.get(6)?,
                parent_id: row.get(7)?,
                level: row.get(8)?,
                sort_order: row.get(9)?,
                account_count: row.get(10)?,
            })
        })?;
        collect_rows(rows)
    }

    pub fn create_group(&self, input: CreateGroupInput) -> AppResult<Group> {
        self.require_unlocked()?;
        let name = input.name.trim();
        if name.is_empty() {
            return Err(AppError::InvalidInput("group name is required".to_string()));
        }
        let parent_level = match input.parent_id {
            Some(parent_id) => self
                .conn
                .query_row("SELECT level FROM groups WHERE id = ?", [parent_id], |row| row.get::<_, i64>(0))
                .optional()?
                .ok_or_else(|| AppError::InvalidInput("parent group not found".to_string()))?,
            None => 0,
        };
        let level = parent_level + 1;
        if level > 3 {
            return Err(AppError::InvalidInput("groups support at most 3 levels".to_string()));
        }

        self.conn.execute(
            "
            INSERT INTO groups
            (name, description, color, proxy_url, fallback_proxy_url_1, fallback_proxy_url_2, parent_id, level, sort_order)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, COALESCE((SELECT MAX(sort_order) + 1 FROM groups), 0))
            ",
            params![
                name,
                input.description.unwrap_or_default(),
                input.color.unwrap_or_else(|| "#2f6f9f".to_string()),
                normalize_proxy_value(input.proxy_url.as_deref())?,
                normalize_proxy_value(input.fallback_proxy_url_1.as_deref())?,
                normalize_proxy_value(input.fallback_proxy_url_2.as_deref())?,
                input.parent_id,
                level
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        self.audit("group.created", "group", Some(id), name)?;
        self.get_group(id)
    }

    pub fn update_group_proxy(&self, input: UpdateGroupProxyInput) -> AppResult<Group> {
        self.require_unlocked()?;
        let existing_id = self
            .conn
            .query_row("SELECT id FROM groups WHERE id = ?", [input.id], |row| row.get::<_, i64>(0))
            .optional()?
            .ok_or_else(|| AppError::InvalidInput("group not found".to_string()))?;
        self.conn.execute(
            "
            UPDATE groups
            SET proxy_url = COALESCE(?, proxy_url),
                fallback_proxy_url_1 = COALESCE(?, fallback_proxy_url_1),
                fallback_proxy_url_2 = COALESCE(?, fallback_proxy_url_2)
            WHERE id = ?
            ",
            params![
                normalize_proxy_option(input.proxy_url.as_deref())?,
                normalize_proxy_option(input.fallback_proxy_url_1.as_deref())?,
                normalize_proxy_option(input.fallback_proxy_url_2.as_deref())?,
                existing_id
            ],
        )?;
        self.audit("group.proxy_updated", "group", Some(existing_id), "")?;
        self.get_group(existing_id)
    }

    pub fn update_group(&self, input: UpdateGroupInput) -> AppResult<Group> {
        self.require_unlocked()?;
        let name = input.name.trim();
        if name.is_empty() {
            return Err(AppError::InvalidInput("group name is required".to_string()));
        }
        let (current_parent_id, current_level): (Option<i64>, i64) = self
            .conn
            .query_row("SELECT parent_id, level FROM groups WHERE id = ?", [input.id], |row| {
                Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, i64>(1)?))
            })
            .optional()?
            .ok_or_else(|| AppError::InvalidInput("group not found".to_string()))?;
        if input.parent_id == Some(input.id) {
            return Err(AppError::InvalidInput("group cannot be its own parent".to_string()));
        }
        if let Some(parent_id) = input.parent_id {
            if self.group_descendant_ids(input.id)?.contains(&parent_id) {
                return Err(AppError::InvalidInput("group cannot move under its descendant".to_string()));
            }
        }
        let parent_level = match input.parent_id {
            Some(parent_id) => self
                .conn
                .query_row("SELECT level FROM groups WHERE id = ?", [parent_id], |row| row.get::<_, i64>(0))
                .optional()?
                .ok_or_else(|| AppError::InvalidInput("parent group not found".to_string()))?,
            None => 0,
        };
        let new_level = parent_level + 1;
        let subtree_depth = self.group_subtree_depth(input.id, current_level)?;
        if new_level + subtree_depth > 3 {
            return Err(AppError::InvalidInput("groups support at most 3 levels".to_string()));
        }

        self.conn.execute(
            "
            UPDATE groups
            SET name = ?,
                description = ?,
                color = ?,
                proxy_url = ?,
                fallback_proxy_url_1 = ?,
                fallback_proxy_url_2 = ?,
                parent_id = ?,
                level = ?,
                sort_order = ?
            WHERE id = ?
            ",
            params![
                name,
                input.description.unwrap_or_default(),
                input.color.unwrap_or_else(|| "#2f6f9f".to_string()),
                normalize_proxy_value(input.proxy_url.as_deref())?,
                normalize_proxy_value(input.fallback_proxy_url_1.as_deref())?,
                normalize_proxy_value(input.fallback_proxy_url_2.as_deref())?,
                input.parent_id,
                new_level,
                input.sort_order.unwrap_or(0).max(0),
                input.id
            ],
        )?;
        if current_parent_id != input.parent_id || current_level != new_level {
            self.shift_group_descendant_levels(input.id, new_level - current_level)?;
        }
        self.audit("group.updated", "group", Some(input.id), name)?;
        self.get_group(input.id)
    }

    pub fn delete_group(&self, group_id: i64) -> AppResult<()> {
        self.require_unlocked()?;
        let (parent_id, level, is_system): (Option<i64>, i64, i64) = self
            .conn
            .query_row(
                "SELECT parent_id, level, is_system FROM groups WHERE id = ?",
                [group_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?
            .ok_or_else(|| AppError::InvalidInput("group not found".to_string()))?;
        if is_system == 1 {
            return Err(AppError::InvalidInput("system group cannot be deleted".to_string()));
        }
        let descendants = self.group_descendant_ids(group_id)?;
        self.conn
            .execute("UPDATE accounts SET group_id = ? WHERE group_id = ?", params![parent_id, group_id])?;
        self.conn.execute(
            "UPDATE groups SET parent_id = ? WHERE parent_id = ?",
            params![parent_id, group_id],
        )?;
        if !descendants.is_empty() {
            self.shift_group_levels(&descendants, -1)?;
        }
        self.conn.execute("DELETE FROM groups WHERE id = ?", [group_id])?;
        self.audit("group.deleted", "group", Some(group_id), &format!("level {level}"))?;
        Ok(())
    }

    pub fn list_tags(&self) -> AppResult<Vec<Tag>> {
        self.require_unlocked()?;
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, color FROM tags ORDER BY name ASC")?;
        let rows = stmt.query_map([], |row| {
            Ok(Tag {
                id: row.get(0)?,
                name: row.get(1)?,
                color: row.get(2)?,
            })
        })?;
        collect_rows(rows)
    }

    pub fn create_tag(&self, input: CreateTagInput) -> AppResult<Tag> {
        self.require_unlocked()?;
        let name = input.name.trim();
        if name.is_empty() {
            return Err(AppError::InvalidInput("tag name is required".to_string()));
        }
        self.conn.execute(
            "INSERT INTO tags (name, color) VALUES (?, ?)",
            params![name, input.color],
        )?;
        let id = self.conn.last_insert_rowid();
        self.audit("tag.created", "tag", Some(id), name)?;
        Ok(Tag {
            id,
            name: name.to_string(),
            color: input.color,
        })
    }

    pub fn delete_tag(&self, tag_id: i64) -> AppResult<()> {
        self.require_unlocked()?;
        self.conn.execute("DELETE FROM tags WHERE id = ?", [tag_id])?;
        self.audit("tag.deleted", "tag", Some(tag_id), "")?;
        Ok(())
    }

    pub fn list_accounts(&self) -> AppResult<Vec<Account>> {
        self.require_unlocked()?;
        let mut stmt = self.conn.prepare(
            "
            SELECT a.id, a.email, a.group_id, g.name, COALESCE(a.remark, ''), a.status,
                   a.provider, a.account_type, a.forward_enabled, a.last_refresh_status,
                   a.last_refresh_error, a.last_refresh_at, COUNT(m.id) AS message_count, a.created_at, a.updated_at,
                   a.password_enc, a.refresh_token_enc, a.imap_password_enc, COALESCE(a.imap_host, ''),
                   a.imap_port, COALESCE(a.proxy_url, ''), COALESCE(a.fallback_proxy_url_1, ''),
                   COALESCE(a.fallback_proxy_url_2, '')
            FROM accounts a
            LEFT JOIN groups g ON g.id = a.group_id
            LEFT JOIN retained_mail_messages m ON m.account_id = a.id
            GROUP BY a.id
            ORDER BY a.sort_order ASC, a.email ASC
            ",
        )?;
        let rows = stmt.query_map([], |row| {
            let account_id: i64 = row.get(0)?;
            Ok(Account {
                id: account_id,
                email: row.get(1)?,
                group_id: row.get(2)?,
                group_name: row.get(3)?,
                remark: row.get(4)?,
                status: row.get(5)?,
                provider: row.get(6)?,
                account_type: row.get(7)?,
                forward_enabled: row.get::<_, i64>(8)? == 1,
                last_refresh_status: row.get(9)?,
                last_refresh_error: row.get(10)?,
                last_refresh_at: row.get(11)?,
                message_count: row.get(12)?,
                created_at: row.get(13)?,
                updated_at: row.get(14)?,
                tags: Vec::new(),
                aliases: Vec::new(),
                has_password: !row.get::<_, String>(15)?.is_empty(),
                has_refresh_token: !row.get::<_, String>(16)?.is_empty(),
                has_imap_password: !row.get::<_, String>(17)?.is_empty(),
                imap_host: row.get(18)?,
                imap_port: row.get(19)?,
                proxy_url: row.get(20)?,
                fallback_proxy_url_1: row.get(21)?,
                fallback_proxy_url_2: row.get(22)?,
            })
        })?;

        let mut accounts = collect_rows(rows)?;
        for account in &mut accounts {
            account.tags = self.tags_for_account(account.id)?;
            account.aliases = self.aliases_for_account(account.id)?;
        }
        Ok(accounts)
    }

    pub fn import_accounts(&self, rows: Vec<ImportedAccount>, group_id: Option<i64>) -> AppResult<ImportAccountsResult> {
        self.require_unlocked()?;
        let key = self.crypto_key.as_ref().ok_or(AppError::Unauthorized)?;
        let mut imported = 0_usize;
        let mut skipped = 0_usize;

        for row in rows {
            let password = crypto::encrypt_text(&row.password, key)?;
            let client_id = crypto::encrypt_text(&row.client_id, key)?;
            let refresh_token = crypto::encrypt_text(&row.refresh_token, key)?;
            let provider = if row.refresh_token.trim().is_empty() {
                "outlook"
            } else {
                "graph"
            };
            let changed = self.conn.execute(
                "
                INSERT INTO accounts
                (email, password_enc, client_id_enc, refresh_token_enc, group_id, remark, provider, account_type)
                VALUES (?, ?, ?, ?, ?, ?, ?, 'outlook')
                ON CONFLICT(email) DO UPDATE SET
                    password_enc = excluded.password_enc,
                    client_id_enc = excluded.client_id_enc,
                    refresh_token_enc = excluded.refresh_token_enc,
                    group_id = COALESCE(excluded.group_id, accounts.group_id),
                    remark = excluded.remark,
                    provider = excluded.provider,
                    updated_at = CURRENT_TIMESTAMP
                ",
                params![row.email, password, client_id, refresh_token, group_id, row.remark, provider],
            )?;
            if changed > 0 {
                imported += 1;
            } else {
                skipped += 1;
            }
        }

        self.audit("account.imported", "account", None, &format!("{} imported", imported))?;
        Ok(ImportAccountsResult { imported, skipped })
    }

    pub fn update_account(&self, input: UpdateAccountInput) -> AppResult<Account> {
        self.require_unlocked()?;
        let email = input.email.trim().to_ascii_lowercase();
        if !email.contains('@') {
            return Err(AppError::InvalidInput("account email is invalid".to_string()));
        }
        let existing_id = self
            .conn
            .query_row("SELECT id FROM accounts WHERE id = ?", [input.id], |row| row.get::<_, i64>(0))
            .optional()?
            .ok_or_else(|| AppError::InvalidInput("account not found".to_string()))?;
        self.ensure_primary_email_is_not_alias(existing_id, &email)?;
        let key = self.crypto_key.as_ref().ok_or(AppError::Unauthorized)?;

        self.conn.execute(
            "
            UPDATE accounts
            SET email = ?,
                group_id = ?,
                remark = COALESCE(?, remark),
                status = COALESCE(?, status),
                provider = COALESCE(?, provider),
                account_type = COALESCE(?, account_type),
                imap_host = COALESCE(?, imap_host),
                imap_port = COALESCE(?, imap_port),
                proxy_url = COALESCE(?, proxy_url),
                fallback_proxy_url_1 = COALESCE(?, fallback_proxy_url_1),
                fallback_proxy_url_2 = COALESCE(?, fallback_proxy_url_2),
                forward_enabled = COALESCE(?, forward_enabled),
                updated_at = CURRENT_TIMESTAMP
            WHERE id = ?
            ",
            params![
                email,
                input.group_id,
                input.remark,
                input.status,
                input.provider,
                input.account_type,
                input.imap_host,
                input.imap_port,
                normalize_proxy_option(input.proxy_url.as_deref())?,
                normalize_proxy_option(input.fallback_proxy_url_1.as_deref())?,
                normalize_proxy_option(input.fallback_proxy_url_2.as_deref())?,
                input.forward_enabled.map(|enabled| if enabled { 1 } else { 0 }),
                existing_id
            ],
        )?;

        if let Some(value) = input.password {
            let encrypted = crypto::encrypt_text(&value, key)?;
            self.conn.execute(
                "UPDATE accounts SET password_enc = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
                params![encrypted, existing_id],
            )?;
        }
        if let Some(value) = input.client_id {
            let encrypted = crypto::encrypt_text(&value, key)?;
            self.conn.execute(
                "UPDATE accounts SET client_id_enc = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
                params![encrypted, existing_id],
            )?;
        }
        if let Some(value) = input.refresh_token {
            let encrypted = crypto::encrypt_text(&value, key)?;
            self.conn.execute(
                "UPDATE accounts SET refresh_token_enc = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
                params![encrypted, existing_id],
            )?;
        }
        if let Some(value) = input.imap_password {
            let encrypted = crypto::encrypt_text(&value, key)?;
            self.conn.execute(
                "UPDATE accounts SET imap_password_enc = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
                params![encrypted, existing_id],
            )?;
        }
        if let Some(tag_ids) = input.tag_ids {
            self.replace_account_tags(existing_id, tag_ids)?;
        }
        if let Some(aliases) = input.aliases {
            self.replace_account_aliases(existing_id, &email, aliases)?;
        } else {
            self.conn.execute(
                "DELETE FROM account_aliases WHERE account_id = ? AND alias_email = ?",
                params![existing_id, email],
            )?;
        }

        self.audit("account.updated", "account", Some(existing_id), "")?;
        self.list_accounts()?
            .into_iter()
            .find(|account| account.id == existing_id)
            .ok_or_else(|| AppError::Internal("updated account not found".to_string()))
    }

    pub fn delete_account(&self, account_id: i64) -> AppResult<()> {
        self.require_unlocked()?;
        self.conn.execute("DELETE FROM accounts WHERE id = ?", [account_id])?;
        self.audit("account.deleted", "account", Some(account_id), "")?;
        Ok(())
    }

    pub fn batch_accounts(&self, input: AccountBatchInput) -> AppResult<JobResult> {
        self.require_unlocked()?;
        let mut requested_ids: Vec<i64> = input.account_ids.into_iter().filter(|id| *id > 0).collect();
        requested_ids.sort_unstable();
        requested_ids.dedup();
        if requested_ids.is_empty() {
            return Err(AppError::InvalidInput("account_ids are required".to_string()));
        }

        let mut account_ids = Vec::new();
        let mut account_stmt = self.conn.prepare("SELECT id FROM accounts WHERE id = ?")?;
        for id in &requested_ids {
            if account_stmt.exists([id])? {
                account_ids.push(*id);
            }
        }
        drop(account_stmt);
        if account_ids.is_empty() {
            return Err(AppError::InvalidInput("no matching accounts".to_string()));
        }

        let missing = requested_ids.len().saturating_sub(account_ids.len());
        let action = input.action.trim();
        let affected = match action {
            "delete" => {
                for account_id in &account_ids {
                    self.conn.execute("DELETE FROM accounts WHERE id = ?", [account_id])?;
                }
                account_ids.len()
            }
            "move_group" => {
                if let Some(group_id) = input.group_id {
                    let exists = self.conn.prepare("SELECT id FROM groups WHERE id = ?")?.exists([group_id])?;
                    if !exists {
                        return Err(AppError::InvalidInput("group not found".to_string()));
                    }
                }
                for account_id in &account_ids {
                    self.conn.execute(
                        "UPDATE accounts SET group_id = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
                        params![input.group_id, account_id],
                    )?;
                }
                account_ids.len()
            }
            "set_forward" => {
                let enabled = input
                    .forward_enabled
                    .ok_or_else(|| AppError::InvalidInput("forward_enabled is required".to_string()))?;
                for account_id in &account_ids {
                    self.conn.execute(
                        "UPDATE accounts SET forward_enabled = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
                        params![if enabled { 1 } else { 0 }, account_id],
                    )?;
                }
                account_ids.len()
            }
            "add_tags" | "remove_tags" => {
                let mut tag_ids: Vec<i64> = input
                    .tag_ids
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|id| *id > 0)
                    .collect();
                tag_ids.sort_unstable();
                tag_ids.dedup();
                if tag_ids.is_empty() {
                    return Err(AppError::InvalidInput("tag_ids are required".to_string()));
                }
                let mut tag_stmt = self.conn.prepare("SELECT id FROM tags WHERE id = ?")?;
                for tag_id in &tag_ids {
                    if !tag_stmt.exists([tag_id])? {
                        return Err(AppError::InvalidInput("tag not found".to_string()));
                    }
                }
                drop(tag_stmt);
                for account_id in &account_ids {
                    for tag_id in &tag_ids {
                        if action == "add_tags" {
                            self.conn.execute(
                                "INSERT OR IGNORE INTO account_tags (account_id, tag_id) VALUES (?, ?)",
                                params![account_id, tag_id],
                            )?;
                        } else {
                            self.conn.execute(
                                "DELETE FROM account_tags WHERE account_id = ? AND tag_id = ?",
                                params![account_id, tag_id],
                            )?;
                        }
                    }
                }
                account_ids.len()
            }
            _ => return Err(AppError::InvalidInput("unsupported batch action".to_string())),
        };

        self.audit(
            "accounts.batch",
            "account",
            None,
            &format!("{action}: {affected} affected, {missing} missing"),
        )?;
        Ok(JobResult {
            success: missing == 0,
            message: format!("Batch {action} processed {affected} account(s)"),
            refreshed: affected,
            failed: missing,
        })
    }

    pub fn reveal_account_secrets(&self, input: RevealAccountSecretsInput) -> AppResult<AccountSecretsPreview> {
        self.require_unlocked()?;
        let hash = self
            .get_config("password_hash")?
            .ok_or_else(|| AppError::InvalidInput("app is not initialized".to_string()))?;
        if !crypto::verify_password(&input.password, &hash)? {
            return Err(AppError::Unauthorized);
        }
        let salt = self
            .get_config("crypto_salt")?
            .ok_or_else(|| AppError::Crypto("missing crypto salt".to_string()))?;
        let key = crypto::derive_key(&input.password, &salt);
        let (password_enc, client_id_enc, refresh_token_enc, imap_password_enc): (String, String, String, String) = self
            .conn
            .query_row(
                "
                SELECT password_enc, client_id_enc, refresh_token_enc, imap_password_enc
                FROM accounts
                WHERE id = ?
                ",
                [input.account_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?
            .ok_or_else(|| AppError::InvalidInput("account not found".to_string()))?;
        let password = crypto::decrypt_text(&password_enc, &key)?;
        let client_id = crypto::decrypt_text(&client_id_enc, &key)?;
        let refresh_token = crypto::decrypt_text(&refresh_token_enc, &key)?;
        let imap_password = crypto::decrypt_text(&imap_password_enc, &key)?;
        self.audit("account.secrets_viewed", "account", Some(input.account_id), "local password verified")?;
        Ok(AccountSecretsPreview {
            password,
            client_id,
            refresh_token_preview: preview_secret(&refresh_token),
            imap_password,
        })
    }

    pub fn list_projects(&self) -> AppResult<Vec<Project>> {
        self.require_unlocked()?;
        let mut stmt = self.conn.prepare(
            "
            SELECT id, name, project_key, COALESCE(description, ''), scope_mode, use_alias_email, status, created_at, updated_at
            FROM projects
            ORDER BY updated_at DESC, id DESC
            ",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)? == 1,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
            ))
        })?;
        let mut projects = Vec::new();
        for row in rows {
            let (id, name, project_key, description, scope_mode, use_alias_email, status, created_at, updated_at) = row?;
            projects.push(Project {
                id,
                name,
                project_key,
                description,
                scope_mode,
                use_alias_email,
                status,
                group_ids: self.project_group_ids(id)?,
                tag_ids: self.project_tag_ids(id)?,
                stats: self.project_stats(id)?,
                created_at,
                updated_at,
            });
        }
        Ok(projects)
    }

    pub fn get_project(&self, project_id: i64) -> AppResult<Project> {
        self.require_unlocked()?;
        let project = self
            .list_projects()?
            .into_iter()
            .find(|project| project.id == project_id)
            .ok_or_else(|| AppError::InvalidInput("project not found".to_string()))?;
        Ok(project)
    }

    pub fn create_project(&self, input: CreateProjectInput) -> AppResult<Project> {
        self.require_unlocked()?;
        let name = input.name.trim();
        if name.is_empty() {
            return Err(AppError::InvalidInput("project name is required".to_string()));
        }
        let scope_mode = input.scope_mode.unwrap_or_else(|| "all".to_string());
        if !matches!(scope_mode.as_str(), "all" | "groups" | "tags") {
            return Err(AppError::InvalidInput("project scope_mode must be all, groups, or tags".to_string()));
        }
        let project_key = input
            .project_key
            .map(|value| normalize_project_key(&value))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| normalize_project_key(name));
        if project_key.is_empty() {
            return Err(AppError::InvalidInput("project key is required".to_string()));
        }

        self.conn.execute(
            "
            INSERT INTO projects (name, project_key, description, scope_mode, use_alias_email, status)
            VALUES (?, ?, ?, ?, ?, 'active')
            ",
            params![
                name,
                project_key,
                input.description.unwrap_or_default(),
                scope_mode,
                input.use_alias_email.unwrap_or(false).then_some(1).unwrap_or(0)
            ],
        )?;
        let project_id = self.conn.last_insert_rowid();
        self.replace_project_group_scope(project_id, input.group_ids.unwrap_or_default())?;
        self.replace_project_tag_scope(project_id, input.tag_ids.unwrap_or_default())?;
        self.sync_project_scope(project_id)?;
        self.audit("project.created", "project", Some(project_id), name)?;
        self.get_project(project_id)
    }

    pub fn sync_project_scope(&self, project_id: i64) -> AppResult<Project> {
        self.require_unlocked()?;
        let (scope_mode, use_alias_email): (String, bool) = self
            .conn
            .query_row("SELECT scope_mode, use_alias_email FROM projects WHERE id = ?", [project_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? == 1))
            })
            .optional()?
            .ok_or_else(|| AppError::InvalidInput("project not found".to_string()))?;
        let group_ids = self.project_group_ids(project_id)?;
        let tag_ids = self.project_tag_ids(project_id)?;
        let accounts = self.accounts_for_project_scope(&scope_mode, use_alias_email, &group_ids, &tag_ids)?;
        for (account_id, email) in &accounts {
            let normalized = email.trim().to_ascii_lowercase();
            self.conn.execute(
                "
                INSERT INTO project_accounts
                (project_id, account_id, normalized_email, email_snapshot, status)
                VALUES (?, ?, ?, ?, 'toClaim')
                ON CONFLICT(project_id, normalized_email) DO UPDATE SET
                    account_id = excluded.account_id,
                    email_snapshot = excluded.email_snapshot,
                    updated_at = CURRENT_TIMESTAMP
                ",
                params![project_id, account_id, normalized, email],
            )?;
        }

        let target_emails = accounts
            .iter()
            .map(|(_, email)| email.trim().to_ascii_lowercase())
            .collect::<Vec<_>>();
        let existing = self.project_account_ids(project_id)?;
        for (project_account_id, account_id, normalized_email, status) in existing {
            if !target_emails.iter().any(|email| email == &normalized_email) && status != "removed" {
                self.transition_project_account(
                    project_account_id,
                    "removed",
                    "scope.sync_removed",
                    "Account is no longer in project scope",
                    account_id,
                )?;
            }
        }

        self.conn.execute(
            "UPDATE projects SET updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            [project_id],
        )?;
        self.audit("project.synced", "project", Some(project_id), "")?;
        self.get_project(project_id)
    }

    pub fn list_project_accounts(&self, project_id: i64) -> AppResult<Vec<ProjectAccount>> {
        self.require_unlocked()?;
        let mut stmt = self.conn.prepare(
            "
            SELECT id, project_id, account_id, normalized_email, email_snapshot, status, claim_token,
                   claimed_at, lease_expires_at, COALESCE(last_result, ''), COALESCE(last_result_detail, ''),
                   claim_count, created_at, updated_at
            FROM project_accounts
            WHERE project_id = ?
            ORDER BY
                CASE status
                    WHEN 'toClaim' THEN 0
                    WHEN 'claimed' THEN 1
                    WHEN 'failed' THEN 2
                    WHEN 'success' THEN 3
                    WHEN 'removed' THEN 4
                    ELSE 5
                END,
                updated_at DESC,
                id DESC
            ",
        )?;
        let rows = stmt.query_map([project_id], project_account_from_row)?;
        collect_rows(rows)
    }

    pub fn claim_project_account(&self, input: ClaimProjectAccountInput) -> AppResult<Option<ProjectAccount>> {
        self.require_unlocked()?;
        let lease_minutes = input.lease_minutes.unwrap_or(30).clamp(1, 1440);
        let candidate = self
            .conn
            .query_row(
                "
                SELECT id
                FROM project_accounts
                WHERE project_id = ?
                  AND (
                    status = 'toClaim'
                    OR (status = 'claimed' AND lease_expires_at IS NOT NULL AND lease_expires_at <= CURRENT_TIMESTAMP)
                  )
                ORDER BY
                    CASE status WHEN 'toClaim' THEN 0 ELSE 1 END,
                    claim_count ASC,
                    id ASC
                LIMIT 1
                ",
                [input.project_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let Some(project_account_id) = candidate else {
            return Ok(None);
        };
        let token = uuid::Uuid::new_v4().to_string();
        let before = self.get_project_account(project_account_id)?;
        self.conn.execute(
            "
            UPDATE project_accounts
            SET status = 'claimed',
                claim_token = ?,
                claimed_at = CURRENT_TIMESTAMP,
                lease_expires_at = datetime('now', ?),
                claim_count = claim_count + 1,
                last_result = '',
                last_result_detail = '',
                updated_at = CURRENT_TIMESTAMP
            WHERE id = ?
            ",
            params![token, format!("+{} minutes", lease_minutes), project_account_id],
        )?;
        self.insert_project_event(&before, "claim", Some(&before.status), Some("claimed"), "")?;
        self.get_project_account(project_account_id).map(Some)
    }

    pub fn complete_project_account_success(&self, input: ProjectAccountActionInput) -> AppResult<ProjectAccount> {
        self.transition_project_account(
            input.project_account_id,
            "success",
            "complete_success",
            input.detail.as_deref().unwrap_or(""),
            None,
        )
    }

    pub fn complete_project_account_failed(&self, input: ProjectAccountActionInput) -> AppResult<ProjectAccount> {
        self.transition_project_account(
            input.project_account_id,
            "failed",
            "complete_failed",
            input.detail.as_deref().unwrap_or(""),
            None,
        )
    }

    pub fn release_project_account(&self, input: ProjectAccountActionInput) -> AppResult<ProjectAccount> {
        self.transition_project_account(
            input.project_account_id,
            "toClaim",
            "release",
            input.detail.as_deref().unwrap_or(""),
            None,
        )
    }

    pub fn remove_project_account(&self, input: ProjectAccountActionInput) -> AppResult<ProjectAccount> {
        self.transition_project_account(
            input.project_account_id,
            "removed",
            "remove",
            input.detail.as_deref().unwrap_or(""),
            None,
        )
    }

    pub fn restore_project_account(&self, input: ProjectAccountActionInput) -> AppResult<ProjectAccount> {
        self.transition_project_account(
            input.project_account_id,
            "toClaim",
            "restore",
            input.detail.as_deref().unwrap_or(""),
            None,
        )
    }

    pub fn list_project_events(&self, project_id: i64) -> AppResult<Vec<ProjectAccountEvent>> {
        self.require_unlocked()?;
        let mut stmt = self.conn.prepare(
            "
            SELECT id, project_id, account_id, project_account_id, normalized_email, action,
                   from_status, to_status, COALESCE(detail, ''), created_at
            FROM project_account_events
            WHERE project_id = ?
            ORDER BY created_at DESC, id DESC
            LIMIT 100
            ",
        )?;
        let rows = stmt.query_map([project_id], |row| {
            Ok(ProjectAccountEvent {
                id: row.get(0)?,
                project_id: row.get(1)?,
                account_id: row.get(2)?,
                project_account_id: row.get(3)?,
                normalized_email: row.get(4)?,
                action: row.get(5)?,
                from_status: row.get(6)?,
                to_status: row.get(7)?,
                detail: row.get(8)?,
                created_at: row.get(9)?,
            })
        })?;
        collect_rows(rows)
    }

    pub fn list_messages(&self, account_id: Option<i64>, folder: Option<String>) -> AppResult<Vec<MailMessage>> {
        self.list_messages_query(MailMessageQuery {
            account_id,
            folder,
            search: None,
            read_state: None,
            has_attachments: None,
            sort_by: None,
            sort_order: None,
            limit: None,
            offset: None,
        })
    }

    pub fn list_messages_query(&self, query: MailMessageQuery) -> AppResult<Vec<MailMessage>> {
        self.require_unlocked()?;
        let search = parse_mail_search(query.search.as_deref().unwrap_or_default());
        let folder = match normalize_mail_folder(query.folder.as_deref().unwrap_or("all")).as_str() {
            "all" => search.folder.unwrap_or_else(|| "all".to_string()),
            value => value.to_string(),
        };
        let read_state = match normalize_read_state(query.read_state.as_deref())?.as_str() {
            "all" => search.read_state.unwrap_or_else(|| "all".to_string()),
            value => value.to_string(),
        };
        let has_attachments = query.has_attachments.or(search.has_attachments);
        let order_clause = mail_sort_clause(query.sort_by.as_deref(), query.sort_order.as_deref())?;
        let limit = query.limit.unwrap_or(200).clamp(1, 500);
        let offset = query.offset.unwrap_or(0).max(0);
        let mut sql = String::from(
            r#"
            SELECT m.id, m.account_id, m.folder, m.provider_message_id, m.subject, m.sender, m.recipients,
                   m.received_at, m.is_read, m.has_attachments, m.body_preview, m.body, m.body_type,
                   m.attachments_json,
                   rq.id, rq.task_type, rq.status, rq.action, rq.error_message,
                   rq.attempts, rq.max_attempts, rq.next_attempt_at, rq.last_attempt_at,
                   rq.updated_at
            FROM retained_mail_messages m
            LEFT JOIN retry_queue rq ON rq.id = (
                SELECT latest.id
                FROM retry_queue latest
                WHERE latest.task_type IN ('mail_mark', 'mail_delete')
                  AND latest.status IN ('pending', 'failed')
                  AND latest.account_id = m.account_id
                  AND latest.message_id = m.provider_message_id
                  AND (latest.channel = '' OR latest.channel = m.folder)
                ORDER BY latest.id DESC
                LIMIT 1
            )
            WHERE 1 = 1
            "#,
        );
        let mut values = Vec::new();
        if let Some(account_id) = query.account_id {
            sql.push_str(" AND m.account_id = ?");
            values.push(SqlValue::Integer(account_id));
        }
        if folder != "all" {
            sql.push_str(" AND m.folder = ?");
            values.push(SqlValue::Text(folder));
        }
        match read_state.as_str() {
            "read" => sql.push_str(" AND m.is_read = 1"),
            "unread" => sql.push_str(" AND m.is_read = 0"),
            _ => {}
        }
        if let Some(has_attachments) = has_attachments {
            sql.push_str(" AND m.has_attachments = ?");
            values.push(SqlValue::Integer(if has_attachments { 1 } else { 0 }));
        }
        append_mail_search_terms(&mut sql, &mut values, &search.terms);
        sql.push_str(" ORDER BY ");
        sql.push_str(&order_clause);
        sql.push_str(" LIMIT ? OFFSET ?");
        values.push(SqlValue::Integer(limit));
        values.push(SqlValue::Integer(offset));

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(values), |row| {
            Ok(MailMessage {
                id: row.get(0)?,
                account_id: row.get(1)?,
                folder: row.get(2)?,
                provider_message_id: row.get(3)?,
                subject: row.get(4)?,
                sender: row.get(5)?,
                recipients: row.get(6)?,
                received_at: row.get(7)?,
                is_read: row.get::<_, i64>(8)? == 1,
                has_attachments: row.get::<_, i64>(9)? == 1,
                body_preview: row.get(10)?,
                body: row.get(11)?,
                body_type: row.get(12)?,
                attachments: parse_attachments_json(row.get::<_, String>(13)?.as_str()),
                remote_sync_failure: remote_sync_failure_from_message_row(row)?,
            })
        })?;
        collect_rows(rows)
    }

    pub fn create_demo_message(&self, account_id: i64) -> AppResult<MailMessage> {
        self.require_unlocked()?;
        let exists = self
            .conn
            .query_row("SELECT id FROM accounts WHERE id = ?", [account_id], |row| row.get::<_, i64>(0))
            .optional()?;
        if exists.is_none() {
            return Err(AppError::InvalidInput("account not found".to_string()));
        }
        let now = chrono::Utc::now();
        let provider_id = format!("local-demo-{}", uuid::Uuid::new_v4());
        self.conn.execute(
            "
            INSERT INTO retained_mail_messages
            (account_id, folder, provider_message_id, subject, sender, recipients, received_at,
             received_at_sort, body_preview, body, body_type, body_cached)
            VALUES (?, 'inbox', ?, 'Local sync placeholder', 'system@local', '', ?, ?, ?, ?, 'text', 1)
            ",
            params![
                account_id,
                provider_id,
                now.to_rfc3339(),
                now.timestamp() as f64,
                "Provider sync is wired as a job boundary. Add Graph or IMAP credentials to enable real mail fetching.",
                "This local message confirms the SQLite workspace is working. The next implementation step is the Graph/IMAP provider adapters."
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        self.audit("mail.demo_created", "message", Some(id), "")?;
        Ok(self
            .list_messages(Some(account_id), Some("inbox".to_string()))?
            .into_iter()
            .find(|message| message.id == id)
            .ok_or_else(|| AppError::Internal("created message not found".to_string()))?)
    }

    pub fn mark_mail_messages(&self, input: MarkMailMessagesInput) -> AppResult<JobResult> {
        self.require_unlocked()?;
        let ids = normalize_message_ids(&input.message_ids)?;
        let targets = self.mail_message_refs(&ids)?;
        if targets.is_empty() {
            return Err(AppError::InvalidInput("no matching messages found".to_string()));
        }

        let sync_remote = input.sync_remote.unwrap_or(true);
        let mut failed = 0_usize;
        let mut errors = Vec::new();
        if sync_remote {
            for target in &targets {
                if let Err(err) = self.sync_remote_mark_message(target, input.is_read) {
                    failed += 1;
                    let error = err.to_string();
                    errors.push(format!("#{} {}: {}", target.id, target.provider_message_id, error));
                    self.enqueue_mail_retry(target, input.is_read, &error)?;
                }
            }
        }

        let mut changed = 0_usize;
        for id in &ids {
            changed += self.conn.execute(
                "
                UPDATE retained_mail_messages
                SET is_read = ?, updated_at = CURRENT_TIMESTAMP
                WHERE id = ?
                ",
                params![if input.is_read { 1 } else { 0 }, id],
            )?;
        }

        let action = if input.is_read { "mail.mark_read" } else { "mail.mark_unread" };
        self.audit(action, "message", None, &format!("{} message(s)", changed))?;
        Ok(JobResult {
            success: failed == 0,
            message: mail_action_message(
                if input.is_read { "Marked read" } else { "Marked unread" },
                changed,
                failed,
                &errors,
            ),
            refreshed: changed,
            failed,
        })
    }

    pub fn delete_mail_messages(&self, input: DeleteMailMessagesInput) -> AppResult<JobResult> {
        self.require_unlocked()?;
        let ids = normalize_message_ids(&input.message_ids)?;
        let targets = self.mail_message_refs(&ids)?;
        if targets.is_empty() {
            return Err(AppError::InvalidInput("no matching messages found".to_string()));
        }

        let sync_remote = input.sync_remote.unwrap_or(true);
        let mut failed = 0_usize;
        let mut errors = Vec::new();
        let mut failed_local_ids = HashSet::new();
        if sync_remote {
            for target in &targets {
                if let Err(err) = self.sync_remote_delete_message(target) {
                    failed += 1;
                    let error = err.to_string();
                    errors.push(format!("#{} {}: {}", target.id, target.provider_message_id, error));
                    self.enqueue_mail_delete_retry(target, &error)?;
                    failed_local_ids.insert(target.id);
                } else {
                    self.clear_mail_delete_retry(target)?;
                }
            }
        }

        let mut changed = 0_usize;
        for id in &ids {
            if failed_local_ids.contains(id) {
                continue;
            }
            let deleted = self
                .conn
                .execute("DELETE FROM retained_mail_messages WHERE id = ?", [id])?;
            changed += deleted;
            if deleted > 0 {
                if let Some(target) = targets.iter().find(|target| target.id == *id) {
                    self.clear_mail_delete_retry(target)?;
                }
            }
        }

        self.audit("mail.deleted", "message", None, &format!("{} message(s)", changed))?;
        Ok(JobResult {
            success: failed == 0,
            message: mail_action_message("Deleted", changed, failed, &errors),
            refreshed: changed,
            failed,
        })
    }

    pub fn get_settings(&self) -> AppResult<Settings> {
        self.require_unlocked()?;
        let mut settings = Settings::default();
        settings.graph_client_id = self
            .get_config("graph_client_id")?
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(settings.graph_client_id);
        settings.oauth_redirect_uri = self
            .get_config("oauth_redirect_uri")?
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(settings.oauth_redirect_uri);
        settings.gptmail_base_url = self
            .get_config("gptmail_base_url")?
            .unwrap_or(settings.gptmail_base_url);
        settings.gptmail_api_key = self.get_config_secret("gptmail_api_key")?;
        settings.duckmail_base_url = self
            .get_config("duckmail_base_url")?
            .unwrap_or(settings.duckmail_base_url);
        settings.duckmail_api_key = self.get_config_secret("duckmail_api_key")?;
        settings.webdav_url = self.get_config("webdav_url")?.unwrap_or_default();
        settings.webdav_username = self.get_config("webdav_username")?.unwrap_or_default();
        settings.webdav_password = self.get_config_secret("webdav_password")?;
        settings.backup_enabled = self.get_config_bool("backup_enabled", settings.backup_enabled)?;
        settings.backup_interval_minutes =
            self.get_config_i64("backup_interval_minutes", settings.backup_interval_minutes)?;
        settings.scheduler_refresh_enabled =
            self.get_config_bool("scheduler_refresh_enabled", settings.scheduler_refresh_enabled)?;
        settings.scheduler_refresh_interval_minutes = self.get_config_i64(
            "scheduler_refresh_interval_minutes",
            settings.scheduler_refresh_interval_minutes,
        )?;
        settings.scheduler_refresh_top = self.get_config_i64("scheduler_refresh_top", settings.scheduler_refresh_top)?;
        settings.forwarding_enabled = self.get_config_bool("forwarding_enabled", settings.forwarding_enabled)?;
        settings.forwarding_interval_minutes =
            self.get_config_i64("forwarding_interval_minutes", settings.forwarding_interval_minutes)?;
        settings.forward_smtp_host = self.get_config("forward_smtp_host")?.unwrap_or_default();
        settings.forward_smtp_port = self.get_config_i64("forward_smtp_port", settings.forward_smtp_port)?;
        settings.forward_smtp_username = self.get_config("forward_smtp_username")?.unwrap_or_default();
        settings.forward_smtp_password = self.get_config_secret("forward_smtp_password")?;
        settings.forward_smtp_from = self.get_config("forward_smtp_from")?.unwrap_or_default();
        settings.forward_smtp_to = self.get_config("forward_smtp_to")?.unwrap_or_default();
        settings.forward_telegram_bot_token = self.get_config_secret("forward_telegram_bot_token")?;
        settings.forward_telegram_chat_id = self.get_config("forward_telegram_chat_id")?.unwrap_or_default();
        settings.forward_wecom_webhook = self.get_config_secret("forward_wecom_webhook")?;
        settings.appearance_theme = normalize_theme_setting(
            self.get_config("appearance_theme")?
                .as_deref()
                .unwrap_or(&settings.appearance_theme),
        );
        settings.accent_color = normalize_accent_color(
            self.get_config("accent_color")?
                .as_deref()
                .unwrap_or(&settings.accent_color),
        );
        Ok(settings)
    }

    pub fn update_settings(&self, settings: Settings) -> AppResult<Settings> {
        self.require_unlocked()?;
        self.set_config("graph_client_id", &settings.graph_client_id)?;
        self.set_config("oauth_redirect_uri", &settings.oauth_redirect_uri)?;
        self.set_config("gptmail_base_url", &settings.gptmail_base_url)?;
        self.set_config_secret("gptmail_api_key", &settings.gptmail_api_key)?;
        self.set_config("duckmail_base_url", &settings.duckmail_base_url)?;
        self.set_config_secret("duckmail_api_key", &settings.duckmail_api_key)?;
        self.set_config("webdav_url", &settings.webdav_url)?;
        self.set_config("webdav_username", &settings.webdav_username)?;
        self.set_config_secret("webdav_password", &settings.webdav_password)?;
        self.set_config_bool("backup_enabled", settings.backup_enabled)?;
        self.set_config_i64("backup_interval_minutes", settings.backup_interval_minutes.max(1))?;
        self.set_config_bool("scheduler_refresh_enabled", settings.scheduler_refresh_enabled)?;
        self.set_config_i64(
            "scheduler_refresh_interval_minutes",
            settings.scheduler_refresh_interval_minutes.max(1),
        )?;
        self.set_config_i64("scheduler_refresh_top", settings.scheduler_refresh_top.clamp(1, 50))?;
        self.set_config_bool("forwarding_enabled", settings.forwarding_enabled)?;
        self.set_config_i64("forwarding_interval_minutes", settings.forwarding_interval_minutes.max(1))?;
        self.set_config("forward_smtp_host", &settings.forward_smtp_host)?;
        self.set_config_i64("forward_smtp_port", settings.forward_smtp_port.clamp(1, 65535))?;
        self.set_config("forward_smtp_username", &settings.forward_smtp_username)?;
        self.set_config_secret("forward_smtp_password", &settings.forward_smtp_password)?;
        self.set_config("forward_smtp_from", &settings.forward_smtp_from)?;
        self.set_config("forward_smtp_to", &settings.forward_smtp_to)?;
        self.set_config_secret("forward_telegram_bot_token", &settings.forward_telegram_bot_token)?;
        self.set_config("forward_telegram_chat_id", &settings.forward_telegram_chat_id)?;
        self.set_config_secret("forward_wecom_webhook", &settings.forward_wecom_webhook)?;
        self.set_config("appearance_theme", &normalize_theme_setting(&settings.appearance_theme))?;
        self.set_config("accent_color", &normalize_accent_color(&settings.accent_color))?;
        self.audit("settings.updated", "settings", None, "")?;
        self.get_settings()
    }

    pub fn exchange_oauth_token(&self, input: OAuthExchangeInput) -> AppResult<OAuthTokenResult> {
        self.require_unlocked()?;
        let provider = match normalize_oauth_provider(input.provider.as_deref())? {
            Some(provider) => provider,
            None => match input.account_id {
                Some(account_id) => {
                    let stored_provider = self
                        .conn
                        .query_row("SELECT provider FROM accounts WHERE id = ?", [account_id], |row| row.get::<_, String>(0))
                        .optional()?;
                    normalize_oauth_provider(stored_provider.as_deref())?.unwrap_or_else(|| "graph".to_string())
                }
                None => "graph".to_string(),
            },
        };
        let token = providers::exchange_microsoft_code(&input.client_id, &input.redirect_uri, &input.code_or_url, Some(&provider))?;
        if let Some(account_id) = input.account_id {
            let key = self.crypto_key.as_ref().ok_or(AppError::Unauthorized)?;
            let refresh_token = crypto::encrypt_text(&token.refresh_token, key)?;
            let client_id = crypto::encrypt_text(&input.client_id, key)?;
            self.conn.execute(
                "
                UPDATE accounts
                SET client_id_enc = ?,
                    refresh_token_enc = ?,
                    provider = ?,
                    account_type = ?,
                    last_refresh_status = 'authorized',
                    last_refresh_error = NULL,
                    refresh_token_updated_at = CURRENT_TIMESTAMP,
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = ?
                ",
                params![client_id, refresh_token, provider, provider, account_id],
            )?;
            self.audit(&format!("oauth.{provider}.exchanged"), "account", Some(account_id), "")?;
        }
        Ok(OAuthTokenResult {
            success: true,
            account_id: input.account_id,
            scope: token.scope,
            expires_in: token.expires_in,
            refresh_token_preview: preview_secret(&token.refresh_token),
            refresh_token: if input.account_id.is_none() {
                Some(token.refresh_token)
            } else {
                None
            },
        })
    }

    pub fn save_oauth_account(&self, input: OAuthSaveAccountInput) -> AppResult<OAuthSaveAccountResult> {
        self.require_unlocked()?;
        let email = input.email.trim().to_ascii_lowercase();
        if !email.contains('@') {
            return Err(AppError::InvalidInput("account email is required".to_string()));
        }
        let client_id = input.client_id.trim();
        if client_id.is_empty() {
            return Err(AppError::InvalidInput("Microsoft client id is required".to_string()));
        }

        let provider = normalize_oauth_provider(input.provider.as_deref())?.unwrap_or_else(|| "graph".to_string());
        let token = if let Some(refresh_token) = input.refresh_token.as_ref().filter(|value| !value.trim().is_empty()) {
            providers::OAuthTokenResponse {
                access_token: String::new(),
                refresh_token: refresh_token.trim().to_string(),
                expires_in: 0,
                scope: String::new(),
            }
        } else {
            let code_or_url = input
                .code_or_url
                .as_ref()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| AppError::InvalidInput("OAuth callback URL is required".to_string()))?;
            providers::exchange_microsoft_code(client_id, &input.redirect_uri, code_or_url, Some(&provider))?
        };
        if token.refresh_token.trim().is_empty() {
            return Err(AppError::InvalidInput("OAuth response did not include a refresh token".to_string()));
        }

        let key = self.crypto_key.as_ref().ok_or(AppError::Unauthorized)?;
        let password = crypto::encrypt_text(input.password.as_deref().unwrap_or_default(), key)?;
        let client_id_enc = crypto::encrypt_text(client_id, key)?;
        let refresh_token_enc = crypto::encrypt_text(&token.refresh_token, key)?;
        let remark = input.remark.unwrap_or_default();
        let forward_enabled = if input.forward_enabled.unwrap_or(false) { 1 } else { 0 };

        self.conn.execute(
            "
            INSERT INTO accounts
            (email, password_enc, client_id_enc, refresh_token_enc, group_id, remark, provider,
             account_type, forward_enabled, last_refresh_status, last_refresh_error, refresh_token_updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'authorized', NULL, CURRENT_TIMESTAMP)
            ON CONFLICT(email) DO UPDATE SET
                password_enc = CASE
                    WHEN excluded.password_enc = '' THEN accounts.password_enc
                    ELSE excluded.password_enc
                END,
                client_id_enc = excluded.client_id_enc,
                refresh_token_enc = excluded.refresh_token_enc,
                group_id = COALESCE(excluded.group_id, accounts.group_id),
                remark = CASE
                    WHEN excluded.remark = '' THEN accounts.remark
                    ELSE excluded.remark
                END,
                provider = excluded.provider,
                account_type = excluded.account_type,
                forward_enabled = excluded.forward_enabled,
                last_refresh_status = 'authorized',
                last_refresh_error = NULL,
                refresh_token_updated_at = CURRENT_TIMESTAMP,
                updated_at = CURRENT_TIMESTAMP
            ",
            params![
                email.as_str(),
                password,
                client_id_enc,
                refresh_token_enc,
                input.group_id,
                remark,
                provider.as_str(),
                provider.as_str(),
                forward_enabled
            ],
        )?;

        let account_id = self.conn.query_row("SELECT id FROM accounts WHERE email = ?", params![email.as_str()], |row| {
            row.get::<_, i64>(0)
        })?;
        self.audit(&format!("oauth.{provider}.account_saved"), "account", Some(account_id), "")?;
        let account = self
            .list_accounts()?
            .into_iter()
            .find(|account| account.id == account_id)
            .ok_or_else(|| AppError::Internal("saved OAuth account was not found".to_string()))?;

        Ok(OAuthSaveAccountResult {
            success: true,
            account,
            scope: token.scope,
            expires_in: token.expires_in,
            refresh_token_preview: preview_secret(&token.refresh_token),
        })
    }

    pub fn refresh_accounts(&self, input: RefreshInput) -> AppResult<JobResult> {
        self.refresh_accounts_with_trigger(input, "manual")
    }

    fn refresh_accounts_with_trigger(&self, input: RefreshInput, trigger_type: &str) -> AppResult<JobResult> {
        let started_at = Utc::now();
        let result = self.refresh_accounts_inner(input);
        let _ = self.record_job_result("refresh", trigger_type, started_at, &result);
        result
    }

    fn refresh_accounts_inner(&self, input: RefreshInput) -> AppResult<JobResult> {
        self.require_unlocked()?;
        let credentials = self.account_credentials(input.account_id)?;
        if credentials.is_empty() {
            return Err(AppError::InvalidInput("no matching accounts to refresh".to_string()));
        }

        let folder = input.folder.unwrap_or_else(|| "all".to_string());
        let top = input.top.unwrap_or(25).clamp(1, 50);
        let mut refreshed = 0_usize;
        let mut failed = 0_usize;
        let mut cached_messages = 0_usize;
        let mut errors = Vec::new();

        for account in credentials {
            match self.refresh_account_credential(&account, &folder, top) {
                Ok(count) => {
                    refreshed += 1;
                    cached_messages += count;
                    self.mark_account_refresh_success(account.id, &account.email, count)?;
                }
                Err(err) => {
                    failed += 1;
                    let message = err.to_string();
                    errors.push(format!("{}: {}", account.email, message));
                    self.mark_account_refresh_failed(account.id, &account.email, &message)?;
                    self.enqueue_refresh_retry(&account, &folder, top, &message)?;
                }
            }
        }

        Ok(JobResult {
            success: failed == 0,
            message: if errors.is_empty() {
                format!("Refreshed {} account(s), cached {} message(s)", refreshed, cached_messages)
            } else {
                format!(
                    "Refreshed {} account(s), cached {} message(s), {} failed: {}",
                    refreshed,
                    cached_messages,
                    failed,
                    errors.join("; ")
                )
            },
            refreshed,
            failed,
        })
    }

    pub fn download_attachment(&self, input: DownloadAttachmentInput) -> AppResult<DownloadAttachmentResult> {
        self.require_unlocked()?;
        let account = self
            .account_credentials(Some(input.account_id))?
            .into_iter()
            .next()
            .ok_or_else(|| AppError::InvalidInput("account not found".to_string()))?;
        let folder = input
            .folder
            .as_deref()
            .map(normalize_mail_folder)
            .filter(|value| value != "all");
        let attachment = self.fetch_attachment_content(&account, &input.message_id, &input.attachment_id, folder.as_deref())?;
        let file_name = safe_file_name(&attachment.name);
        let dir = attachment_dir(&self.db_path)?;
        std::fs::create_dir_all(&dir).map_err(|err| AppError::Internal(err.to_string()))?;
        let path = unique_path(&dir, &file_name);
        std::fs::write(&path, &attachment.bytes).map_err(|err| AppError::Internal(err.to_string()))?;
        self.audit(
            "attachment.downloaded",
            "attachment",
            Some(input.account_id),
            &format!("{} ({})", file_name, attachment.content_type),
        )?;
        Ok(DownloadAttachmentResult {
            path: path.to_string_lossy().to_string(),
            file_name,
            size: attachment.bytes.len() as i64,
        })
    }

    pub fn download_all_attachments(&self, input: DownloadAllAttachmentsInput) -> AppResult<ExportResult> {
        self.require_unlocked()?;
        let account = self
            .account_credentials(Some(input.account_id))?
            .into_iter()
            .next()
            .ok_or_else(|| AppError::InvalidInput("account not found".to_string()))?;
        let folder = input
            .folder
            .as_deref()
            .map(normalize_mail_folder)
            .filter(|value| value != "all");
        let attachment_infos = self.cached_message_attachments(account.id, &input.message_id, folder.as_deref())?;
        if attachment_infos.is_empty() {
            return Err(AppError::InvalidInput("message has no cached attachment metadata".to_string()));
        }

        let mut used_names = HashSet::new();
        let mut files = Vec::new();
        for attachment_info in attachment_infos {
            let downloaded = self
                .fetch_attachment_content(&account, &input.message_id, &attachment_info.id, folder.as_deref())
                .map_err(|err| AppError::Internal(format!("failed to download attachment {}: {}", attachment_info.name, err)))?;
            let display_name = if downloaded.name.trim().is_empty() {
                attachment_info.name.as_str()
            } else {
                downloaded.name.as_str()
            };
            files.push((unique_bundle_file_name(&mut used_names, display_name), downloaded.bytes));
        }

        let dir = attachment_dir(&self.db_path)?;
        std::fs::create_dir_all(&dir).map_err(|err| AppError::Internal(err.to_string()))?;
        let requested_file_name = timestamped_file_name("attachments", "zip");
        let path = unique_path(&dir, &requested_file_name);
        let size = write_zip_bundle(&path, &files)?;
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or(requested_file_name.as_str())
            .to_string();
        self.audit(
            "attachment.bundle_downloaded",
            "attachment",
            Some(input.account_id),
            &format!("{} attachment(s)", files.len()),
        )?;
        Ok(ExportResult {
            path: path.to_string_lossy().to_string(),
            file_name,
            size,
            item_count: files.len(),
        })
    }

    pub fn get_mail_raw_content(&self, message_id: i64) -> AppResult<MailRawContent> {
        self.require_unlocked()?;
        let (provider_message_id, raw_mime): (String, Option<Vec<u8>>) = self
            .conn
            .query_row(
                "
                SELECT provider_message_id, raw_mime
                FROM retained_mail_messages
                WHERE id = ?
                ",
                [message_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .ok_or_else(|| AppError::InvalidInput("message not found".to_string()))?;
        let raw_mime = raw_mime.ok_or_else(|| {
            AppError::InvalidInput(
                "raw MIME is not cached for this message; refresh an IMAP account message before viewing raw source".to_string(),
            )
        })?;
        if raw_mime.is_empty() {
            return Err(AppError::InvalidInput("cached raw MIME is empty".to_string()));
        }
        Ok(MailRawContent {
            message_id,
            file_name: format!("message-{}-raw.eml", safe_file_name(&provider_message_id)),
            content: String::from_utf8_lossy(&raw_mime).to_string(),
            size: raw_mime.len() as i64,
        })
    }

    fn fetch_attachment_content(
        &self,
        account: &AccountCredentials,
        message_id: &str,
        attachment_id: &str,
        folder: Option<&str>,
    ) -> AppResult<DownloadedAttachment> {
        if should_use_graph(account) {
            providers::download_graph_attachment(account, message_id, attachment_id)
        } else {
            let raw_mime = self.cached_imap_raw_mime(account.id, message_id, folder)?;
            providers::download_imap_attachment_from_raw(&raw_mime, attachment_id)
        }
    }

    pub fn export_mail_messages(&self, input: ExportMailMessagesInput) -> AppResult<ExportResult> {
        self.require_unlocked()?;
        let ids = normalize_message_ids(&input.message_ids)?;
        let rows = self.export_mail_message_rows(&ids)?;
        if rows.is_empty() {
            return Err(AppError::InvalidInput("no matching messages found".to_string()));
        }
        let title = input
            .title
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("OutlookEmail message export");
        let content = render_mail_export_html(title, &rows);
        let file_name = timestamped_file_name("mail-export", "html");
        let (path, size) = self.write_export_file("mail", &file_name, content.as_bytes())?;
        self.audit("mail.exported", "message", None, &format!("{} message(s)", rows.len()))?;
        Ok(ExportResult {
            path,
            file_name,
            size,
            item_count: rows.len(),
        })
    }

    pub fn create_mail_share(&self, input: CreateMailShareInput) -> AppResult<MailShareRecord> {
        self.require_unlocked()?;
        let ids = normalize_message_ids(&input.message_ids)?;
        let rows = self.export_mail_message_rows(&ids)?;
        if rows.is_empty() {
            return Err(AppError::InvalidInput("no matching messages found".to_string()));
        }
        let title = input
            .title
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("OutlookEmail local share");
        let expires_in_days = input.expires_in_days.unwrap_or(30).clamp(1, 365);
        let expires_at = (Utc::now() + ChronoDuration::days(expires_in_days)).to_rfc3339();
        let content = render_mail_export_html(title, &rows);
        let file_name = timestamped_file_name("mail-share", "html");
        let (path, size) = self.write_export_file("shares", &file_name, content.as_bytes())?;
        let file_name = Path::new(&path)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or(file_name.as_str())
            .to_string();
        let token = Uuid::new_v4().to_string();
        let token_hash = share_token_hash(&token);
        let token_preview = token.chars().take(8).collect::<String>();
        let message_ids_json = serde_json::to_string(&ids)
            .map_err(|err| AppError::Internal(format!("serialize share message ids failed: {err}")))?;
        let account_id = rows[0].account_id;
        self.conn.execute(
            "
            INSERT INTO email_share_links
            (account_id, token_hash, exported_path, title, file_name, item_count, size, message_ids_json, expires_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            ",
            params![
                account_id,
                token_hash,
                path,
                title,
                file_name,
                rows.len() as i64,
                size,
                message_ids_json,
                expires_at
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        self.audit("mail_share.created", "share", Some(id), &format!("{} message(s)", rows.len()))?;
        let mut record = self.get_mail_share_record(id)?;
        record.token_preview = token_preview;
        Ok(record)
    }

    pub fn list_mail_share_records(&self, limit: Option<i64>) -> AppResult<Vec<MailShareRecord>> {
        self.require_unlocked()?;
        let limit = limit.unwrap_or(100).clamp(1, 500);
        let mut stmt = self.conn.prepare(
            "
            SELECT s.id, s.account_id, a.email, COALESCE(s.title, ''), s.token_hash,
                   s.exported_path, COALESCE(s.file_name, ''), COALESCE(s.item_count, 0),
                   COALESCE(s.size, 0), s.expires_at, s.revoked_at, s.created_at, s.updated_at
            FROM email_share_links s
            JOIN accounts a ON a.id = s.account_id
            ORDER BY s.id DESC
            LIMIT ?
            ",
        )?;
        let rows = stmt.query_map([limit], mail_share_record_from_row)?;
        collect_rows(rows)
    }

    pub fn revoke_mail_share(&self, input: RevokeMailShareInput) -> AppResult<MailShareRecord> {
        self.require_unlocked()?;
        let updated = self.conn.execute(
            "
            UPDATE email_share_links
            SET revoked_at = CURRENT_TIMESTAMP,
                updated_at = CURRENT_TIMESTAMP
            WHERE id = ? AND revoked_at IS NULL
            ",
            [input.share_id],
        )?;
        if updated == 0 {
            let _ = self.get_mail_share_record(input.share_id)?;
        }
        self.audit("mail_share.revoked", "share", Some(input.share_id), "")?;
        self.get_mail_share_record(input.share_id)
    }

    fn get_mail_share_record(&self, share_id: i64) -> AppResult<MailShareRecord> {
        self.conn
            .query_row(
                "
                SELECT s.id, s.account_id, a.email, COALESCE(s.title, ''), s.token_hash,
                       s.exported_path, COALESCE(s.file_name, ''), COALESCE(s.item_count, 0),
                       COALESCE(s.size, 0), s.expires_at, s.revoked_at, s.created_at, s.updated_at
                FROM email_share_links s
                JOIN accounts a ON a.id = s.account_id
                WHERE s.id = ?
                ",
                [share_id],
                mail_share_record_from_row,
            )
            .optional()?
            .ok_or_else(|| AppError::InvalidInput("mail share record not found".to_string()))
    }

    pub fn export_accounts(&self, input: ExportAccountsInput) -> AppResult<ExportResult> {
        self.require_unlocked()?;
        let mut accounts = self.list_accounts()?;
        if let Some(group_id) = input.group_id {
            accounts.retain(|account| account.group_id == Some(group_id));
        }
        if let Some(account_ids) = input.account_ids {
            let selected: HashSet<i64> = account_ids.into_iter().collect();
            accounts.retain(|account| selected.contains(&account.id));
        }
        let mut csv = String::new();
        csv.push_str(&csv_row(&[
            "id",
            "email",
            "aliases",
            "group",
            "remark",
            "status",
            "provider",
            "account_type",
            "forward_enabled",
            "proxy_url",
            "fallback_proxy_url_1",
            "fallback_proxy_url_2",
            "message_count",
            "last_refresh_status",
            "created_at",
            "updated_at",
        ]));
        for account in &accounts {
            csv.push_str(&csv_row(&[
                account.id.to_string(),
                account.email.clone(),
                account.aliases.join("; "),
                account.group_name.clone().unwrap_or_default(),
                account.remark.clone(),
                account.status.clone(),
                account.provider.clone(),
                account.account_type.clone(),
                account.forward_enabled.to_string(),
                account.proxy_url.clone(),
                account.fallback_proxy_url_1.clone(),
                account.fallback_proxy_url_2.clone(),
                account.message_count.to_string(),
                account.last_refresh_status.clone(),
                account.created_at.clone(),
                account.updated_at.clone(),
            ]));
        }
        let file_name = timestamped_file_name("accounts-export", "csv");
        let (path, size) = self.write_export_file("accounts", &file_name, csv.as_bytes())?;
        self.audit("accounts.exported", "account", None, &format!("{} account(s)", accounts.len()))?;
        Ok(ExportResult {
            path,
            file_name,
            size,
            item_count: accounts.len(),
        })
    }

    pub fn export_account_secrets(&self, input: ExportAccountSecretsInput) -> AppResult<ExportResult> {
        self.require_unlocked()?;
        if input.confirm.trim() != "EXPORT ACCOUNT SECRETS" {
            return Err(AppError::InvalidInput(
                "type EXPORT ACCOUNT SECRETS to confirm secret export".to_string(),
            ));
        }
        let mut seen = HashSet::new();
        let account_ids = input
            .account_ids
            .into_iter()
            .filter(|id| *id > 0 && seen.insert(*id))
            .collect::<Vec<_>>();
        if account_ids.is_empty() {
            return Err(AppError::InvalidInput("select at least one account".to_string()));
        }
        let hash = self
            .get_config("password_hash")?
            .ok_or_else(|| AppError::InvalidInput("app is not initialized".to_string()))?;
        if !crypto::verify_password(&input.password, &hash)? {
            return Err(AppError::Unauthorized);
        }
        let salt = self
            .get_config("crypto_salt")?
            .ok_or_else(|| AppError::Crypto("missing crypto salt".to_string()))?;
        let key = crypto::derive_key(&input.password, &salt);
        let placeholders = repeat_placeholders(account_ids.len());
        let sql = format!(
            "
            SELECT id, email, provider, account_type, remark,
                   COALESCE(imap_host, ''), imap_port,
                   password_enc, client_id_enc, refresh_token_enc, imap_password_enc
            FROM accounts
            WHERE id IN ({placeholders})
            ORDER BY email ASC
            "
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(account_ids.iter()), |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, String>(10)?,
            ))
        })?;
        let rows = collect_rows(rows)?;
        if rows.is_empty() {
            return Err(AppError::InvalidInput("no matching accounts found".to_string()));
        }
        let exported_count = rows.len();

        let mut csv = String::new();
        csv.push_str(&csv_row(&[
            "id",
            "email",
            "provider",
            "account_type",
            "remark",
            "imap_host",
            "imap_port",
            "password",
            "client_id",
            "refresh_token",
            "imap_password",
        ]));
        for (
            id,
            email,
            provider,
            account_type,
            remark,
            imap_host,
            imap_port,
            password_enc,
            client_id_enc,
            refresh_token_enc,
            imap_password_enc,
        ) in rows
        {
            csv.push_str(&csv_row(&[
                id.to_string(),
                email,
                provider,
                account_type,
                remark,
                imap_host,
                imap_port.to_string(),
                crypto::decrypt_text(&password_enc, &key)?,
                crypto::decrypt_text(&client_id_enc, &key)?,
                crypto::decrypt_text(&refresh_token_enc, &key)?,
                crypto::decrypt_text(&imap_password_enc, &key)?,
            ]));
        }
        let file_name = timestamped_file_name("account-secrets", "csv");
        let (path, size) = self.write_export_file("account-secrets", &file_name, csv.as_bytes())?;
        self.audit(
            "account_secrets.exported",
            "account",
            None,
            &format!("{} account(s)", exported_count),
        )?;
        Ok(ExportResult {
            path,
            file_name,
            size,
            item_count: exported_count,
        })
    }

    pub fn export_project_accounts(&self, input: ExportProjectAccountsInput) -> AppResult<ExportResult> {
        self.require_unlocked()?;
        let project = self.get_project(input.project_id)?;
        let accounts = self.list_project_accounts(input.project_id)?;
        let mut csv = String::new();
        csv.push_str(&csv_row(&[
            "id",
            "project_id",
            "project_key",
            "email",
            "normalized_email",
            "status",
            "claim_count",
            "claimed_at",
            "lease_expires_at",
            "last_result",
            "last_result_detail",
            "created_at",
            "updated_at",
        ]));
        for account in &accounts {
            csv.push_str(&csv_row(&[
                account.id.to_string(),
                account.project_id.to_string(),
                project.project_key.clone(),
                account.email.clone(),
                account.normalized_email.clone(),
                account.status.clone(),
                account.claim_count.to_string(),
                account.claimed_at.clone().unwrap_or_default(),
                account.lease_expires_at.clone().unwrap_or_default(),
                account.last_result.clone(),
                account.last_result_detail.clone(),
                account.created_at.clone(),
                account.updated_at.clone(),
            ]));
        }
        let prefix = safe_file_name(&format!("project-{}-accounts", project.project_key));
        let file_name = timestamped_file_name(&prefix, "csv");
        let (path, size) = self.write_export_file("projects", &file_name, csv.as_bytes())?;
        self.audit(
            "project_accounts.exported",
            "project",
            Some(project.id),
            &format!("{} account(s)", accounts.len()),
        )?;
        Ok(ExportResult {
            path,
            file_name,
            size,
            item_count: accounts.len(),
        })
    }

    pub fn run_forwarding_job(&self, input: Option<ForwardingInput>) -> AppResult<JobResult> {
        self.run_forwarding_job_with_trigger(input, "manual")
    }

    fn run_forwarding_job_with_trigger(&self, input: Option<ForwardingInput>, trigger_type: &str) -> AppResult<JobResult> {
        let started_at = Utc::now();
        let result = self.run_forwarding_job_inner(input);
        let _ = self.record_job_result("forwarding", trigger_type, started_at, &result);
        result
    }

    fn run_forwarding_job_inner(&self, input: Option<ForwardingInput>) -> AppResult<JobResult> {
        self.require_unlocked()?;
        let settings = self.get_settings()?;
        let channels = automation::configured_forward_channels(&settings);
        if channels.is_empty() {
            return Err(AppError::InvalidInput(
                "configure at least one forwarding channel first".to_string(),
            ));
        }
        let channel_circuits = self.forwarding_channel_circuits(&settings)?;
        let account_id = input.as_ref().and_then(|value| value.account_id);
        let limit = input
            .as_ref()
            .and_then(|value| value.limit)
            .unwrap_or(25)
            .clamp(1, 200);
        let accounts = self.forwarding_accounts(account_id)?;
        if accounts.is_empty() {
            return Err(AppError::InvalidInput(
                "no active accounts have forwarding enabled".to_string(),
            ));
        }

        let mut forwarded = 0_usize;
        let mut failed = 0_usize;
        let mut skipped = 0_usize;
        let mut circuit_skipped = 0_usize;
        let mut errors = Vec::new();

        for (account_id, account_email) in accounts {
            let messages = self.forwarding_candidates(account_id, limit)?;
            let proxy_chain = self.proxy_chain_for_account(account_id)?;
            for message in messages {
                for channel in &channels {
                    if let Some(circuit) = channel_circuits.iter().find(|item| item.channel == *channel && item.status == "open") {
                        circuit_skipped += 1;
                        let error = forwarding_circuit_error(circuit);
                        if !errors.iter().any(|item| item == &error) {
                            errors.push(error);
                        }
                        continue;
                    }
                    if self.forward_success_exists(account_id, &message.message_id, channel)? {
                        skipped += 1;
                        continue;
                    }
                    match automation::forward_message(&settings, channel, &message, &proxy_chain) {
                        Ok(()) => {
                            forwarded += 1;
                            self.insert_forwarding_log(
                                Some(account_id),
                                &account_email,
                                &message.message_id,
                                channel,
                                "success",
                                None,
                            )?;
                        }
                        Err(err) => {
                            failed += 1;
                            let error = err.to_string();
                            errors.push(format!("{} {}: {}", account_email, channel, error));
                            self.insert_forwarding_log(
                                Some(account_id),
                                &account_email,
                                &message.message_id,
                                channel,
                                "failed",
                                Some(&error),
                            )?;
                            self.enqueue_forwarding_retry(account_id, &account_email, &message, channel, &error)?;
                        }
                    }
                }
            }
            self.conn.execute(
                "UPDATE accounts SET forward_last_checked_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
                [account_id],
            )?;
        }

        self.audit(
            "forwarding.job",
            "forwarding",
            None,
            &format!("{forwarded} success, {failed} failed, {skipped} skipped, {circuit_skipped} circuit skipped"),
        )?;
        Ok(JobResult {
            success: failed == 0 && circuit_skipped == 0,
            message: if errors.is_empty() {
                format!("Forwarded {forwarded} message channel(s), skipped {skipped}")
            } else {
                format!(
                    "Forwarded {forwarded} message channel(s), {failed} failed, {circuit_skipped} circuit skipped: {}",
                    errors.join("; ")
                )
            },
            refreshed: forwarded,
            failed: failed + circuit_skipped,
        })
    }

    pub fn run_backup_job(&self) -> AppResult<BackupResult> {
        self.run_backup_job_with_trigger("manual")
    }

    fn run_backup_job_with_trigger(&self, trigger_type: &str) -> AppResult<BackupResult> {
        let started_at = Utc::now();
        let result = self.run_backup_job_inner();
        let _ = self.record_backup_result(trigger_type, started_at, &result);
        if let Err(err) = &result {
            let settings = self.get_settings().unwrap_or_default();
            let _ = self.enqueue_backup_retry(settings.webdav_url.trim(), &err.to_string());
        }
        result
    }

    fn run_backup_job_inner(&self) -> AppResult<BackupResult> {
        self.require_unlocked()?;
        let settings = self.get_settings()?;
        if settings.webdav_url.trim().is_empty() {
            return Err(AppError::InvalidInput("WebDAV URL is required".to_string()));
        }
        let backup_dir = backup_dir(&self.db_path)?;
        std::fs::create_dir_all(&backup_dir).map_err(|err| AppError::Internal(err.to_string()))?;
        let file_name = format!("outlook-email-{}.sqlite", Utc::now().format("%Y%m%d-%H%M%S"));
        let path = unique_path(&backup_dir, &file_name);
        let path_text = path.to_string_lossy().to_string();

        let result = (|| -> AppResult<BackupResult> {
            self.conn.execute("VACUUM INTO ?", [path_text.as_str()])?;
            let bytes = std::fs::read(&path).map_err(|err| AppError::Internal(err.to_string()))?;
            let size = bytes.len() as i64;
            let remote_url = automation::upload_webdav(&settings, &file_name, bytes)?;
            self.insert_backup_log(&remote_url, "success", &file_name, size, None)?;
            self.audit("backup.webdav", "backup", None, &remote_url)?;
            Ok(BackupResult {
                success: true,
                message: format!("Uploaded {file_name}"),
                path: path_text,
                remote_url,
                size,
            })
        })();

        if let Err(err) = &result {
            let _ = self.insert_backup_log(
                settings.webdav_url.trim(),
                "failed",
                &file_name,
                path.metadata().map(|meta| meta.len() as i64).unwrap_or_default(),
                Some(&err.to_string()),
            );
        }
        result
    }

    pub fn list_forwarding_logs(&self, limit: Option<i64>) -> AppResult<Vec<ForwardingLog>> {
        self.require_unlocked()?;
        let limit = limit.unwrap_or(100).clamp(1, 500);
        let mut stmt = self.conn.prepare(
            "
            SELECT id, account_id, account_email, message_id, channel, status, error_message, created_at
            FROM forwarding_logs
            ORDER BY id DESC
            LIMIT ?
            ",
        )?;
        let rows = stmt.query_map([limit], |row| {
            Ok(ForwardingLog {
                id: row.get(0)?,
                account_id: row.get(1)?,
                account_email: row.get(2)?,
                message_id: row.get(3)?,
                channel: row.get(4)?,
                status: row.get(5)?,
                error_message: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?;
        collect_rows(rows)
    }

    pub fn list_backup_logs(&self, limit: Option<i64>) -> AppResult<Vec<BackupLog>> {
        self.require_unlocked()?;
        let limit = limit.unwrap_or(100).clamp(1, 500);
        let mut stmt = self.conn.prepare(
            "
            SELECT id, target, status, file_name, size, error_message, created_at
            FROM backup_logs
            ORDER BY id DESC
            LIMIT ?
            ",
        )?;
        let rows = stmt.query_map([limit], |row| {
            Ok(BackupLog {
                id: row.get(0)?,
                target: row.get(1)?,
                status: row.get(2)?,
                file_name: row.get(3)?,
                size: row.get(4)?,
                error_message: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?;
        collect_rows(rows)
    }

    pub fn restore_backup(&mut self, input: RestoreBackupInput) -> AppResult<RestoreBackupResult> {
        self.require_unlocked()?;
        if !input.confirm {
            return Err(AppError::InvalidInput(
                "restore confirmation is required".to_string(),
            ));
        }

        let log = self.get_backup_log(input.backup_log_id)?;
        if log.status != "success" {
            return Err(AppError::InvalidInput(
                "only successful local backup snapshots can be restored".to_string(),
            ));
        }

        let file_name = validate_local_backup_file_name(&log.file_name)?;
        let backup_dir = backup_dir(&self.db_path)?;
        std::fs::create_dir_all(&backup_dir).map_err(|err| AppError::Internal(err.to_string()))?;
        let backup_dir = std::fs::canonicalize(&backup_dir).map_err(|err| AppError::Internal(err.to_string()))?;
        let source_path = std::fs::canonicalize(backup_dir.join(&file_name))
            .map_err(|err| AppError::InvalidInput(format!("local backup snapshot not found: {err}")))?;
        if !source_path.starts_with(&backup_dir) {
            return Err(AppError::InvalidInput(
                "backup snapshot path is outside the local backup directory".to_string(),
            ));
        }

        validate_sqlite_snapshot(&source_path)?;

        let stamp = Utc::now().format("%Y%m%d-%H%M%S").to_string();
        let safety_name = format!("pre-restore-{stamp}.sqlite");
        let safety_path = unique_path(&backup_dir, &safety_name);
        let safety_path_text = safety_path.to_string_lossy().to_string();
        self.conn.execute("VACUUM INTO ?", [safety_path_text.as_str()])?;

        let db_parent = self
            .db_path
            .parent()
            .ok_or_else(|| AppError::Internal("database path has no parent directory".to_string()))?;
        let replacement_path = unique_path(db_parent, &format!(".restore-{stamp}.sqlite"));
        std::fs::copy(&source_path, &replacement_path).map_err(|err| AppError::Internal(err.to_string()))?;

        let crypto_key = self.crypto_key;
        let db_path = self.db_path.clone();
        let memory_conn = Connection::open_in_memory()?;
        let old_conn = std::mem::replace(&mut self.conn, memory_conn);
        drop(old_conn);

        let install_result = (|| -> AppResult<()> {
            remove_sqlite_file_set(&db_path)?;
            std::fs::rename(&replacement_path, &db_path).map_err(|err| AppError::Internal(err.to_string()))?;
            Ok(())
        })();

        if let Err(err) = install_result {
            let _ = std::fs::copy(&safety_path, &db_path);
            self.conn = Connection::open(&db_path)?;
            self.crypto_key = crypto_key;
            return Err(err);
        }

        self.conn = Connection::open(&db_path)?;
        self.crypto_key = crypto_key;
        self.initialize_schema()?;
        self.audit(
            "backup.restored",
            "backup",
            Some(log.id),
            &format!("{} restored; safety snapshot {}", log.file_name, safety_path_text),
        )?;

        Ok(RestoreBackupResult {
            success: true,
            message: format!("Restored local backup {}", log.file_name),
            restored_file: log.file_name,
            safety_backup_path: safety_path_text,
            replaced_database_path: db_path.to_string_lossy().to_string(),
            size: log.size,
        })
    }

    pub fn local_retention_summary(&self) -> AppResult<LocalRetentionSummary> {
        self.require_unlocked()?;
        let (attachment_file_count, attachments_size) = dir_stats(&attachment_dir(&self.db_path)?)?;
        let (export_file_count, exports_size) = dir_stats(&exports_dir(&self.db_path)?)?;
        let (backup_file_count, backups_size) = dir_stats(&backup_dir(&self.db_path)?)?;
        let (mail_message_count, unread_message_count, raw_mime_count, body_cached_count): (i64, i64, i64, i64) =
            self.conn.query_row(
                "
                SELECT COUNT(*),
                       COALESCE(SUM(CASE WHEN is_read = 0 THEN 1 ELSE 0 END), 0),
                       COALESCE(SUM(CASE WHEN raw_mime IS NOT NULL AND LENGTH(raw_mime) > 0 THEN 1 ELSE 0 END), 0),
                       COALESCE(SUM(CASE WHEN body_cached = 1 THEN 1 ELSE 0 END), 0)
                FROM retained_mail_messages
                ",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )?;
        let latest_mail_received_at = self
            .conn
            .query_row(
                "
                SELECT received_at
                FROM retained_mail_messages
                ORDER BY received_at_sort DESC, id DESC
                LIMIT 1
                ",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let temp_message_count = self.scalar_count("SELECT COUNT(*) FROM temp_email_messages")?;
        let retry_queue_count = self.scalar_count("SELECT COUNT(*) FROM retry_queue WHERE status IN ('pending', 'failed')")?;
        let latest_account_refresh_at = self
            .conn
            .query_row(
                "
                SELECT MAX(last_refresh_at)
                FROM accounts
                WHERE last_refresh_at IS NOT NULL
                ",
                [],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten();
        Ok(LocalRetentionSummary {
            database_path: self.db_path.to_string_lossy().to_string(),
            database_size: file_set_size(&sqlite_file_set(&self.db_path))?,
            attachment_file_count,
            attachments_size,
            export_file_count,
            exports_size,
            backup_file_count,
            backups_size,
            mail_message_count,
            unread_message_count,
            raw_mime_count,
            body_cached_count,
            temp_message_count,
            retry_queue_count,
            latest_mail_received_at,
            latest_account_refresh_at,
        })
    }

    pub fn clear_local_data(&self, input: ClearLocalDataInput) -> AppResult<ClearLocalDataResult> {
        self.require_unlocked()?;
        if input.confirm.trim() != "CLEAR LOCAL DATA" {
            return Err(AppError::InvalidInput("type CLEAR LOCAL DATA to confirm local data cleanup".to_string()));
        }
        let clear_mail_cache = input.clear_mail_cache.unwrap_or(false);
        let clear_temp_mail_cache = input.clear_temp_mail_cache.unwrap_or(false);
        let clear_attachments = input.clear_attachments.unwrap_or(false);
        let clear_exports = input.clear_exports.unwrap_or(false);
        if !clear_mail_cache && !clear_temp_mail_cache && !clear_attachments && !clear_exports {
            return Err(AppError::InvalidInput("select at least one local data category to clear".to_string()));
        }

        let mut deleted_messages = 0_i64;
        let mut deleted_temp_messages = 0_i64;
        let mut deleted_files = 0_usize;
        let mut freed_bytes = 0_i64;

        if clear_mail_cache {
            deleted_messages = self.scalar_count("SELECT COUNT(*) FROM retained_mail_messages")?;
            self.conn.execute("DELETE FROM retained_mail_messages", [])?;
        }
        if clear_temp_mail_cache {
            deleted_temp_messages = self.scalar_count("SELECT COUNT(*) FROM temp_email_messages")?;
            self.conn.execute("DELETE FROM temp_email_messages", [])?;
        }
        if clear_attachments {
            let (files, bytes) = remove_dir_contents(&attachment_dir(&self.db_path)?)?;
            deleted_files += files;
            freed_bytes += bytes;
        }
        if clear_exports {
            let (files, bytes) = remove_dir_contents(&exports_dir(&self.db_path)?)?;
            deleted_files += files;
            freed_bytes += bytes;
            self.conn.execute(
                "
                UPDATE email_share_links
                SET revoked_at = COALESCE(revoked_at, CURRENT_TIMESTAMP),
                    updated_at = CURRENT_TIMESTAMP
                WHERE revoked_at IS NULL
                ",
                [],
            )?;
        }

        self.audit(
            "local_data.cleared",
            "storage",
            None,
            &format!(
                "{} mail, {} temp, {} files, {} bytes",
                deleted_messages, deleted_temp_messages, deleted_files, freed_bytes
            ),
        )?;
        Ok(ClearLocalDataResult {
            success: true,
            message: format!(
                "Cleared local data: {} mail message(s), {} temp message(s), {} file(s)",
                deleted_messages, deleted_temp_messages, deleted_files
            ),
            deleted_messages,
            deleted_temp_messages,
            deleted_files,
            freed_bytes,
        })
    }

    fn get_backup_log(&self, backup_log_id: i64) -> AppResult<BackupLog> {
        self.conn
            .query_row(
                "
                SELECT id, target, status, file_name, size, error_message, created_at
                FROM backup_logs
                WHERE id = ?
                ",
                [backup_log_id],
                backup_log_from_row,
            )
            .optional()?
            .ok_or_else(|| AppError::InvalidInput("backup log not found".to_string()))
    }

    pub fn list_automation_runs(&self, limit: Option<i64>) -> AppResult<Vec<AutomationRun>> {
        self.list_automation_runs_query(AutomationRunQuery {
            limit,
            ..AutomationRunQuery::default()
        })
    }

    pub fn list_automation_runs_query(&self, query: AutomationRunQuery) -> AppResult<Vec<AutomationRun>> {
        self.require_unlocked()?;
        let filter = AutomationRunFilter::from_query(query)?;
        let search_like = format!("%{}%", filter.search);
        let mut stmt = self.conn.prepare(
            "
            SELECT id, job_type, trigger_type, status, message, refreshed, failed,
                   duration_ms, started_at, finished_at
            FROM automation_runs
            WHERE (?1 = '' OR job_type = ?1)
              AND (?2 = '' OR trigger_type = ?2)
              AND (?3 = '' OR status = ?3)
              AND (?4 = '' OR message LIKE ?5 OR job_type LIKE ?5 OR trigger_type LIKE ?5 OR status LIKE ?5)
            ORDER BY id DESC
            LIMIT ?6
            ",
        )?;
        let rows = stmt.query_map(
            params![
                filter.job_type,
                filter.trigger_type,
                filter.status,
                filter.search,
                search_like,
                filter.limit
            ],
            automation_run_from_row,
        )?;
        collect_rows(rows)
    }

    pub fn clear_automation_runs(&self, input: ClearAutomationRunsInput) -> AppResult<JobResult> {
        self.require_unlocked()?;
        let filter = AutomationRunFilter::from_clear_input(&input)?;
        if !input.clear_all.unwrap_or(false)
            && filter.job_type.is_empty()
            && filter.trigger_type.is_empty()
            && filter.status.is_empty()
            && filter.search.is_empty()
            && input.older_than_days.is_none()
        {
            return Err(AppError::InvalidInput(
                "choose a filter or enable clear_all before clearing automation history".to_string(),
            ));
        }
        let search_like = format!("%{}%", filter.search);
        let older_than_days = input.older_than_days.filter(|value| *value > 0);
        let older_interval = older_than_days
            .map(|days| format!("-{days} days"))
            .unwrap_or_default();
        let deleted = self.conn.execute(
            "
            DELETE FROM automation_runs
            WHERE (?1 = '' OR job_type = ?1)
              AND (?2 = '' OR trigger_type = ?2)
              AND (?3 = '' OR status = ?3)
              AND (?4 = '' OR message LIKE ?5 OR job_type LIKE ?5 OR trigger_type LIKE ?5 OR status LIKE ?5)
              AND (?6 IS NULL OR datetime(created_at) <= datetime('now', ?7))
            ",
            params![
                filter.job_type,
                filter.trigger_type,
                filter.status,
                filter.search,
                search_like,
                older_than_days,
                older_interval
            ],
        )?;
        self.audit("automation_runs.cleared", "automation", None, &format!("{} run(s)", deleted))?;
        Ok(JobResult {
            success: true,
            message: format!("Cleared {} automation run(s)", deleted),
            refreshed: deleted,
            failed: 0,
        })
    }

    pub fn list_retry_queue(&self, query: RetryQueueQuery) -> AppResult<Vec<RetryQueueItem>> {
        self.require_unlocked()?;
        let status = normalize_retry_value(query.status.as_deref(), &["pending", "failed"], "status")?;
        let task_type = normalize_retry_value(
            query.task_type.as_deref(),
            &[
                "mail_mark",
                "mail_delete",
                "forward_message",
                "temp_refresh",
                "refresh_account",
                "backup_job",
            ],
            "task_type",
        )?;
        let limit = query.limit.unwrap_or(100).clamp(1, 500);
        let mut stmt = self.conn.prepare(
            "
            SELECT id, task_type, status, account_id, account_email, message_id, channel,
                   action, payload_json, error_message, attempts, max_attempts,
                   next_attempt_at, last_attempt_at, created_at, updated_at
            FROM retry_queue
            WHERE (?1 = '' OR status = ?1)
              AND (?2 = '' OR task_type = ?2)
            ORDER BY id DESC
            LIMIT ?3
            ",
        )?;
        let rows = stmt.query_map(params![status, task_type, limit], retry_queue_item_from_row)?;
        collect_rows(rows)
    }

    pub fn run_retry_queue(&self, input: Option<RetryQueueRunInput>) -> AppResult<JobResult> {
        self.run_retry_queue_with_trigger(input.unwrap_or_default(), "manual", true)
    }

    fn run_retry_queue_with_trigger(
        &self,
        input: RetryQueueRunInput,
        trigger_type: &str,
        record_history: bool,
    ) -> AppResult<JobResult> {
        let started_at = Utc::now();
        let result = self.run_retry_queue_inner(input);
        if record_history {
            let _ = self.record_job_result("retry", trigger_type, started_at, &result);
        }
        result
    }

    fn run_retry_queue_inner(&self, input: RetryQueueRunInput) -> AppResult<JobResult> {
        self.require_unlocked()?;
        let items = self.retry_queue_candidates(input)?;
        let mut completed = 0_usize;
        let mut failed = 0_usize;
        let mut errors = Vec::new();

        for item in items {
            match self.execute_retry_item(&item) {
                Ok(()) => {
                    completed += 1;
                    self.conn.execute("DELETE FROM retry_queue WHERE id = ?", [item.id])?;
                    self.audit(
                        "retry.completed",
                        "retry",
                        Some(item.id),
                        &format!("{} {}", item.task_type, item.message_id),
                    )?;
                }
                Err(err) => {
                    failed += 1;
                    let message = err.to_string();
                    errors.push(format!("#{} {}: {}", item.id, item.task_type, message));
                    self.mark_retry_failed(&item, &message)?;
                }
            }
        }

        Ok(JobResult {
            success: failed == 0,
            message: retry_job_message(completed, failed, &errors),
            refreshed: completed,
            failed,
        })
    }

    pub fn dismiss_retry_item(&self, input: RetryQueueItemInput) -> AppResult<JobResult> {
        self.require_unlocked()?;
        let deleted = self
            .conn
            .execute("DELETE FROM retry_queue WHERE id = ?", [input.retry_id])?;
        self.audit("retry.dismissed", "retry", Some(input.retry_id), "")?;
        Ok(JobResult {
            success: true,
            message: format!("Dismissed {} retry item(s)", deleted),
            refreshed: deleted,
            failed: 0,
        })
    }

    pub fn scheduler_status(&self) -> AppResult<SchedulerStatus> {
        self.require_unlocked()?;
        Ok(SchedulerStatus {
            last_refresh_at: self.get_config("scheduler_last_refresh_at")?,
            last_forwarding_at: self.get_config("scheduler_last_forwarding_at")?,
            last_backup_at: self.get_config("scheduler_last_backup_at")?,
        })
    }

    pub fn get_automation_observability(&self) -> AppResult<AutomationObservability> {
        self.require_unlocked()?;
        let runs = self.list_automation_runs_query(AutomationRunQuery {
            limit: Some(500),
            ..AutomationRunQuery::default()
        })?;
        let retry_items = self.list_retry_queue(RetryQueueQuery {
            limit: Some(500),
            ..RetryQueueQuery::default()
        })?;
        let settings = self.get_settings()?;
        let channel_circuits = self.forwarding_channel_circuits(&settings)?;

        let run_count = runs.len() as i64;
        let successful_run_count = runs.iter().filter(|run| run.status == "success").count() as i64;
        let failed_run_count = runs.iter().filter(|run| run.status == "failed").count() as i64;
        let scheduled_run_count = runs.iter().filter(|run| run.trigger_type == "schedule").count() as i64;
        let manual_run_count = runs.iter().filter(|run| run.trigger_type == "manual").count() as i64;
        let average_duration_ms = average_i64(runs.iter().map(|run| run.duration_ms), run_count);
        let retry_pending_count = retry_items.iter().filter(|item| item.status == "pending").count() as i64;
        let retry_failed_count = retry_items.iter().filter(|item| item.status == "failed").count() as i64;
        let retry_due_count = retry_items
            .iter()
            .filter(|item| item.status == "pending" && item.due_now)
            .count() as i64;
        let retry_exhausted_count = retry_items
            .iter()
            .filter(|item| item.status == "failed" || item.attempts >= item.max_attempts)
            .count() as i64;
        let open_circuit_count = channel_circuits
            .iter()
            .filter(|channel| channel.status == "open")
            .count() as i64;

        let job_summaries = ["refresh", "forwarding", "backup", "retry"]
            .iter()
            .map(|job_type| automation_job_summary(&runs, job_type))
            .collect();
        let retry_summaries = [
            "mail_mark",
            "mail_delete",
            "forward_message",
            "temp_refresh",
            "refresh_account",
            "backup_job",
        ]
        .iter()
        .map(|task_type| retry_task_summary(&retry_items, task_type))
        .collect();
        let mut error_buckets = Vec::new();
        for run in runs.iter().filter(|run| run.status == "failed") {
            push_error_bucket(
                &mut error_buckets,
                &run.error_category,
                &run.message,
                Some(run.finished_at.as_str()),
                run.failed.max(1),
            );
        }
        for item in retry_items.iter() {
            push_error_bucket(
                &mut error_buckets,
                &item.error_category,
                &item.error_message,
                Some(item.updated_at.as_str()),
                1,
            );
        }
        error_buckets.sort_by(|left, right| right.count.cmp(&left.count).then_with(|| left.category.cmp(&right.category)));
        error_buckets.truncate(8);

        Ok(AutomationObservability {
            run_count,
            successful_run_count,
            failed_run_count,
            scheduled_run_count,
            manual_run_count,
            average_duration_ms,
            retry_pending_count,
            retry_failed_count,
            retry_due_count,
            retry_exhausted_count,
            open_circuit_count,
            job_summaries,
            retry_summaries,
            error_buckets,
            channel_circuits,
        })
    }

    pub fn list_refresh_logs(&self, account_id: Option<i64>, limit: Option<i64>) -> AppResult<Vec<RefreshLog>> {
        self.require_unlocked()?;
        let limit = limit.unwrap_or(100).clamp(1, 500);
        let mut stmt = self.conn.prepare(
            "
            SELECT id, account_id, account_email, refresh_type, status, error_message, created_at
            FROM refresh_logs
            WHERE (?1 IS NULL OR account_id = ?1)
            ORDER BY id DESC
            LIMIT ?2
            ",
        )?;
        let rows = stmt.query_map(params![account_id, limit], refresh_log_from_row)?;
        collect_rows(rows)
    }

    pub fn run_due_scheduled_jobs(&self) -> AppResult<()> {
        if !self.is_unlocked() {
            return Ok(());
        }
        let settings = self.get_settings()?;
        let now = Utc::now();

        let retry_started_at = Utc::now();
        let retry_result = self.run_retry_queue_inner(RetryQueueRunInput {
            retry_id: None,
            limit: Some(20),
        });
        match &retry_result {
            Ok(result) if result.refreshed + result.failed > 0 => {
                let _ = self.record_job_result("retry", "schedule", retry_started_at, &retry_result);
                self.audit("scheduler.retry", "scheduler", None, &result.message)?;
            }
            Ok(_) => {}
            Err(err) => self.audit("scheduler.retry_failed", "scheduler", None, &err.to_string())?,
        }

        if settings.scheduler_refresh_enabled
            && self.scheduler_due("scheduler_last_refresh_at", settings.scheduler_refresh_interval_minutes, now)?
        {
            match self.refresh_accounts_with_trigger(RefreshInput {
                account_id: None,
                folder: Some("all".to_string()),
                top: Some(settings.scheduler_refresh_top.clamp(1, 50) as usize),
            }, "schedule") {
                Ok(result) => self.audit("scheduler.refresh", "scheduler", None, &result.message)?,
                Err(err) => self.audit("scheduler.refresh_failed", "scheduler", None, &err.to_string())?,
            }
            self.set_config("scheduler_last_refresh_at", &now.to_rfc3339())?;
        }

        if settings.forwarding_enabled
            && self.scheduler_due("scheduler_last_forwarding_at", settings.forwarding_interval_minutes, now)?
        {
            match self.run_forwarding_job_with_trigger(Some(ForwardingInput {
                account_id: None,
                limit: Some(50),
            }), "schedule") {
                Ok(result) => self.audit("scheduler.forwarding", "scheduler", None, &result.message)?,
                Err(err) => self.audit("scheduler.forwarding_failed", "scheduler", None, &err.to_string())?,
            }
            self.set_config("scheduler_last_forwarding_at", &now.to_rfc3339())?;
        }

        if settings.backup_enabled
            && self.scheduler_due("scheduler_last_backup_at", settings.backup_interval_minutes, now)?
        {
            match self.run_backup_job_with_trigger("schedule") {
                Ok(result) => self.audit("scheduler.backup", "scheduler", None, &result.message)?,
                Err(err) => self.audit("scheduler.backup_failed", "scheduler", None, &err.to_string())?,
            }
            self.set_config("scheduler_last_backup_at", &now.to_rfc3339())?;
        }

        Ok(())
    }

    pub fn list_temp_emails(&self) -> AppResult<Vec<TempEmail>> {
        self.require_unlocked()?;
        let mut stmt = self.conn.prepare(
            "
            SELECT te.id, te.email, te.provider, te.status, te.channel_id,
                   COUNT(tm.id) AS message_count, te.last_refresh_at,
                   COALESCE(te.last_refresh_status, 'never'), te.last_refresh_error,
                   COALESCE(te.tags_json, '[]'), te.created_at, te.updated_at
            FROM temp_emails te
            LEFT JOIN temp_email_messages tm ON tm.email_address = te.email
            GROUP BY te.id
            ORDER BY te.updated_at DESC, te.id DESC
            ",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(TempEmail {
                id: row.get(0)?,
                email: row.get(1)?,
                provider: row.get(2)?,
                status: row.get(3)?,
                channel_id: row.get(4)?,
                message_count: row.get(5)?,
                last_refresh_at: row.get(6)?,
                last_refresh_status: row.get(7)?,
                last_refresh_error: row.get(8)?,
                tags: temp_tags_from_json(&row.get::<_, String>(9)?),
                created_at: row.get(10)?,
                updated_at: row.get(11)?,
            })
        })?;
        collect_rows(rows)
    }

    pub fn list_temp_email_messages(&self, email: String) -> AppResult<Vec<TempEmailMessage>> {
        self.require_unlocked()?;
        let email = normalize_email(&email)?;
        let mut stmt = self.conn.prepare(
            "
            SELECT id, message_id, email_address, from_address, subject, content,
                   html_content, has_html, COALESCE(timestamp, 0), raw_content, created_at
            FROM temp_email_messages
            WHERE email_address = ?
            ORDER BY timestamp DESC, id DESC
            LIMIT 200
            ",
        )?;
        let rows = stmt.query_map([email], temp_message_from_row)?;
        collect_rows(rows)
    }

    pub fn import_temp_emails(&self, input: ImportTempEmailsInput) -> AppResult<ImportAccountsResult> {
        self.require_unlocked()?;
        let provider = normalize_temp_provider(&input.provider)?;
        if provider == "cloudflare" {
            self.ensure_cloudflare_channel_exists(input.channel_id)?;
        }
        let mut imported = 0_usize;
        let mut skipped = 0_usize;
        for line in input.raw.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with('#')) {
            let parts = split_legacy_line(line);
            let Some(raw_email) = parts.first() else {
                skipped += 1;
                continue;
            };
            let Ok(email) = normalize_email(raw_email) else {
                skipped += 1;
                continue;
            };
            let mut credential = TempEmailCredential {
                id: 0,
                email,
                provider: provider.clone(),
                channel_id: if provider == "cloudflare" { input.channel_id } else { None },
                provider_token: String::new(),
                provider_account_id: String::new(),
                provider_password: String::new(),
            };
            if provider == "duckmail" {
                credential.provider_password = parts.get(1).cloned().unwrap_or_default();
            } else if provider == "cloudflare" {
                credential.provider_account_id = parts.get(1).cloned().unwrap_or_default();
            }
            if self.upsert_temp_email_credential(&credential)? {
                imported += 1;
            } else {
                skipped += 1;
            }
        }
        self.audit("temp_email.imported", "temp_email", None, &format!("{imported} imported"))?;
        Ok(ImportAccountsResult { imported, skipped })
    }

    pub fn generate_temp_email(&self, input: GenerateTempEmailInput) -> AppResult<TempEmail> {
        self.require_unlocked()?;
        let provider = normalize_temp_provider(&input.provider)?;
        let mut input = input;
        input.provider = provider.clone();
        let channel = if provider == "cloudflare" {
            Some(self.cloudflare_channel_credential(input.channel_id)?)
        } else {
            None
        };
        let settings = self.get_settings()?;
        let credential = providers::generate_temp_email(&settings, &input, channel.as_ref())?;
        self.upsert_temp_email_credential(&credential)?;
        self.audit("temp_email.generated", "temp_email", None, &credential.email)?;
        self.list_temp_emails()?
            .into_iter()
            .find(|item| item.email == credential.email)
            .ok_or_else(|| AppError::Internal("generated temp email not found".to_string()))
    }

    pub fn generate_cloudflare_batch(&self, input: GenerateCloudflareBatchInput) -> AppResult<ImportAccountsResult> {
        self.require_unlocked()?;
        let count = input.count.clamp(1, 200);
        let tags = normalize_temp_tags(input.tags.unwrap_or_default());
        let tags_json = serde_json::to_string(&tags).map_err(|err| AppError::Internal(err.to_string()))?;
        self.cloudflare_channel_credential(input.channel_id)?;
        let prefix = input
            .prefix
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("cf");
        let mut imported = 0_usize;
        let mut skipped = 0_usize;
        for index in 0..count {
            let username = format!("{}{}", prefix, random_temp_suffix(index));
            let generated = self.generate_temp_email(GenerateTempEmailInput {
                provider: "cloudflare".to_string(),
                prefix: None,
                domain: input.domain.clone(),
                username: Some(username),
                password: None,
                channel_id: input.channel_id,
            });
            match generated {
                Ok(item) => {
                    imported += 1;
                    if !tags.is_empty() {
                        self.conn.execute(
                            "UPDATE temp_emails SET tags_json = ?, updated_at = CURRENT_TIMESTAMP WHERE email = ?",
                            params![tags_json, item.email],
                        )?;
                    }
                }
                Err(_) => skipped += 1,
            }
        }
        self.audit("temp_email.cloudflare_batch_generated", "temp_email", None, &format!("{imported} generated"))?;
        Ok(ImportAccountsResult { imported, skipped })
    }

    pub fn refresh_temp_email_messages(&self, email: String) -> AppResult<JobResult> {
        self.require_unlocked()?;
        let credential = self.temp_email_credential(&email)?;
        match self.refresh_temp_email_credential(&credential) {
            Ok(result) => Ok(result),
            Err(err) => {
                let message = err.to_string();
                self.mark_temp_email_refresh_failed(&credential, &message)?;
                self.enqueue_temp_refresh_retry(&credential, &message)?;
                Err(err)
            }
        }
    }

    pub fn update_temp_email(&self, input: UpdateTempEmailInput) -> AppResult<TempEmail> {
        self.require_unlocked()?;
        let email = normalize_email(&input.email)?;
        let tags = normalize_temp_tags(input.tags);
        let tags_json = serde_json::to_string(&tags).map_err(|err| AppError::Internal(err.to_string()))?;
        let changed = self.conn.execute(
            "
            UPDATE temp_emails
            SET tags_json = ?, updated_at = CURRENT_TIMESTAMP
            WHERE email = ?
            ",
            params![tags_json, email],
        )?;
        if changed == 0 {
            return Err(AppError::InvalidInput("temp email not found".to_string()));
        }
        self.audit("temp_email.updated", "temp_email", None, &email)?;
        self.list_temp_emails()?
            .into_iter()
            .find(|item| item.email == email)
            .ok_or_else(|| AppError::Internal("updated temp email not found".to_string()))
    }

    pub fn delete_temp_email(&self, email: String) -> AppResult<()> {
        self.require_unlocked()?;
        let credential = self.temp_email_credential(&email)?;
        let channel = if credential.provider == "cloudflare" {
            Some(self.cloudflare_channel_credential(credential.channel_id)?)
        } else {
            None
        };
        let _ = providers::delete_temp_remote(&credential, channel.as_ref());
        self.conn.execute("DELETE FROM temp_email_messages WHERE email_address = ?", [credential.email.as_str()])?;
        self.conn.execute("DELETE FROM temp_emails WHERE id = ?", [credential.id])?;
        self.audit("temp_email.deleted", "temp_email", Some(credential.id), &credential.email)?;
        Ok(())
    }

    pub fn list_cloudflare_channels(&self) -> AppResult<Vec<CloudflareChannel>> {
        self.require_unlocked()?;
        let mut stmt = self.conn.prepare(
            "
            SELECT c.id, c.name, c.worker_domain, COALESCE(c.email_domains, ''),
                   c.admin_password_enc, c.enabled, c.is_default, c.created_at, c.updated_at,
                   COUNT(te.id) AS reference_count
            FROM cloudflare_channels c
            LEFT JOIN temp_emails te ON te.provider = 'cloudflare' AND te.channel_id = c.id
            GROUP BY c.id
            ORDER BY c.is_default DESC, c.name ASC, c.id ASC
            ",
        )?;
        let rows = stmt.query_map([], |row| {
            let domains: String = row.get(3)?;
            let admin_password: String = row.get(4)?;
            Ok(CloudflareChannel {
                id: row.get(0)?,
                name: row.get(1)?,
                worker_domain: row.get(2)?,
                email_domains: parse_domain_list(&domains),
                admin_password_configured: !admin_password.is_empty(),
                enabled: row.get::<_, i64>(5)? == 1,
                is_default: row.get::<_, i64>(6)? == 1,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
                reference_count: row.get(9)?,
            })
        })?;
        collect_rows(rows)
    }

    pub fn upsert_cloudflare_channel(&self, input: UpsertCloudflareChannelInput) -> AppResult<CloudflareChannel> {
        self.require_unlocked()?;
        let name = input.name.trim();
        let worker_domain = input.worker_domain.trim().trim_end_matches('/').to_string();
        if name.is_empty() || worker_domain.is_empty() {
            return Err(AppError::InvalidInput("Cloudflare channel name and worker domain are required".to_string()));
        }
        let domains = serialize_domain_list(&input.email_domains);
        let admin_password_enc = match input.admin_password.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
            Some(secret) => self.encrypt_optional_secret(secret)?,
            None => String::new(),
        };

        if input.is_default {
            self.conn.execute("UPDATE cloudflare_channels SET is_default = 0", [])?;
        }

        let channel_id = if let Some(id) = input.id {
            let existing_password: String = self
                .conn
                .query_row("SELECT admin_password_enc FROM cloudflare_channels WHERE id = ?", [id], |row| row.get(0))
                .optional()?
                .ok_or_else(|| AppError::InvalidInput("Cloudflare channel not found".to_string()))?;
            let next_password = if admin_password_enc.is_empty() {
                existing_password
            } else {
                admin_password_enc
            };
            self.conn.execute(
                "
                UPDATE cloudflare_channels
                SET name = ?, worker_domain = ?, email_domains = ?, admin_password_enc = ?,
                    enabled = ?, is_default = ?, updated_at = CURRENT_TIMESTAMP
                WHERE id = ?
                ",
                params![
                    name,
                    worker_domain,
                    domains,
                    next_password,
                    if input.enabled { 1 } else { 0 },
                    if input.is_default { 1 } else { 0 },
                    id
                ],
            )?;
            id
        } else {
            self.conn.execute(
                "
                INSERT INTO cloudflare_channels
                (name, worker_domain, email_domains, admin_password_enc, enabled, is_default)
                VALUES (?, ?, ?, ?, ?, ?)
                ",
                params![
                    name,
                    worker_domain,
                    domains,
                    admin_password_enc,
                    if input.enabled { 1 } else { 0 },
                    if input.is_default { 1 } else { 0 }
                ],
            )?;
            self.conn.last_insert_rowid()
        };
        if input.is_default {
            self.conn.execute(
                "UPDATE cloudflare_channels SET is_default = CASE WHEN id = ? THEN 1 ELSE 0 END",
                [channel_id],
            )?;
        }
        self.audit("cloudflare_channel.saved", "cloudflare_channel", Some(channel_id), name)?;
        self.list_cloudflare_channels()?
            .into_iter()
            .find(|channel| channel.id == channel_id)
            .ok_or_else(|| AppError::Internal("saved Cloudflare channel not found".to_string()))
    }

    pub fn delete_cloudflare_channel(&self, channel_id: i64) -> AppResult<()> {
        self.require_unlocked()?;
        let references: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM temp_emails WHERE provider = 'cloudflare' AND channel_id = ?",
            [channel_id],
            |row| row.get(0),
        )?;
        if references > 0 {
            return Err(AppError::InvalidInput(
                "Cloudflare channel is still referenced by temp emails".to_string(),
            ));
        }
        self.conn.execute("DELETE FROM cloudflare_channels WHERE id = ?", [channel_id])?;
        self.audit("cloudflare_channel.deleted", "cloudflare_channel", Some(channel_id), "")?;
        Ok(())
    }

    pub fn test_cloudflare_channel(&self, channel_id: i64) -> AppResult<JobResult> {
        self.require_unlocked()?;
        let channel = self.cloudflare_channel_credential(Some(channel_id))?;
        let message = providers::test_cloudflare_channel(&channel)?;
        Ok(JobResult {
            success: true,
            message,
            refreshed: 1,
            failed: 0,
        })
    }

    fn initialize_schema(&mut self) -> AppResult<()> {
        self.conn.pragma_update(None, "journal_mode", "WAL")?;
        self.conn.pragma_update(None, "foreign_keys", "ON")?;
        self.conn.busy_timeout(std::time::Duration::from_secs(5))?;
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS app_config (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS groups (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT UNIQUE NOT NULL,
                description TEXT DEFAULT '',
                color TEXT NOT NULL DEFAULT '#2f6f9f',
                proxy_url TEXT DEFAULT '',
                fallback_proxy_url_1 TEXT DEFAULT '',
                fallback_proxy_url_2 TEXT DEFAULT '',
                sort_order INTEGER NOT NULL DEFAULT 0,
                is_system INTEGER NOT NULL DEFAULT 0,
                parent_id INTEGER,
                level INTEGER NOT NULL DEFAULT 1 CHECK(level IN (1, 2, 3)),
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY(parent_id) REFERENCES groups(id) ON DELETE SET NULL
            );

            CREATE TABLE IF NOT EXISTS accounts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                email TEXT UNIQUE NOT NULL,
                password_enc TEXT NOT NULL DEFAULT '',
                client_id_enc TEXT NOT NULL DEFAULT '',
                refresh_token_enc TEXT NOT NULL DEFAULT '',
                group_id INTEGER,
                sort_order INTEGER NOT NULL DEFAULT 0,
                remark TEXT DEFAULT '',
                status TEXT NOT NULL DEFAULT 'active',
                account_type TEXT NOT NULL DEFAULT 'outlook',
                provider TEXT NOT NULL DEFAULT 'outlook',
                imap_host TEXT DEFAULT '',
                imap_port INTEGER NOT NULL DEFAULT 993,
                imap_password_enc TEXT NOT NULL DEFAULT '',
                forward_enabled INTEGER NOT NULL DEFAULT 0,
                forward_last_checked_at TEXT,
                proxy_url TEXT DEFAULT '',
                fallback_proxy_url_1 TEXT DEFAULT '',
                fallback_proxy_url_2 TEXT DEFAULT '',
                last_refresh_at TEXT,
                last_refresh_status TEXT NOT NULL DEFAULT 'never',
                last_refresh_error TEXT,
                refresh_token_updated_at TEXT,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY(group_id) REFERENCES groups(id) ON DELETE SET NULL
            );

            CREATE TABLE IF NOT EXISTS account_aliases (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                account_id INTEGER NOT NULL,
                alias_email TEXT UNIQUE NOT NULL,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY(account_id) REFERENCES accounts(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS tags (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT UNIQUE NOT NULL,
                color TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS account_tags (
                account_id INTEGER NOT NULL,
                tag_id INTEGER NOT NULL,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                PRIMARY KEY(account_id, tag_id),
                FOREIGN KEY(account_id) REFERENCES accounts(id) ON DELETE CASCADE,
                FOREIGN KEY(tag_id) REFERENCES tags(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS temp_emails (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                email TEXT UNIQUE NOT NULL,
                provider TEXT NOT NULL DEFAULT 'gptmail',
                status TEXT NOT NULL DEFAULT 'active',
                channel_id INTEGER,
                provider_token_enc TEXT NOT NULL DEFAULT '',
                provider_account_id TEXT NOT NULL DEFAULT '',
                provider_password_enc TEXT NOT NULL DEFAULT '',
                last_refresh_at TEXT,
                last_refresh_status TEXT NOT NULL DEFAULT 'never',
                last_refresh_error TEXT,
                tags_json TEXT NOT NULL DEFAULT '[]',
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS temp_email_messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                message_id TEXT UNIQUE NOT NULL,
                email_address TEXT NOT NULL,
                from_address TEXT DEFAULT '',
                subject TEXT DEFAULT '',
                content TEXT DEFAULT '',
                html_content TEXT DEFAULT '',
                has_html INTEGER NOT NULL DEFAULT 0,
                timestamp INTEGER,
                raw_content TEXT DEFAULT '',
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS cloudflare_channels (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT UNIQUE NOT NULL,
                worker_domain TEXT NOT NULL,
                email_domains TEXT DEFAULT '',
                admin_password_enc TEXT NOT NULL DEFAULT '',
                enabled INTEGER NOT NULL DEFAULT 1,
                is_default INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS retained_mail_messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                account_id INTEGER NOT NULL,
                folder TEXT NOT NULL DEFAULT 'inbox',
                provider_message_id TEXT NOT NULL,
                subject TEXT NOT NULL DEFAULT '',
                sender TEXT NOT NULL DEFAULT '',
                recipients TEXT NOT NULL DEFAULT '',
                cc TEXT NOT NULL DEFAULT '',
                received_at TEXT NOT NULL DEFAULT '',
                received_at_sort REAL NOT NULL DEFAULT 0,
                is_read INTEGER NOT NULL DEFAULT 0,
                has_attachments INTEGER NOT NULL DEFAULT 0,
                body_preview TEXT NOT NULL DEFAULT '',
                body TEXT,
                body_type TEXT NOT NULL DEFAULT 'text',
                attachments_json TEXT NOT NULL DEFAULT '[]',
                raw_mime BLOB,
                list_cached INTEGER NOT NULL DEFAULT 1,
                body_cached INTEGER NOT NULL DEFAULT 0,
                list_cached_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                body_cached_at TEXT,
                last_synced_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(account_id, folder, provider_message_id),
                FOREIGN KEY(account_id) REFERENCES accounts(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS refresh_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                account_id INTEGER,
                account_email TEXT NOT NULL DEFAULT '',
                refresh_type TEXT NOT NULL DEFAULT 'manual',
                status TEXT NOT NULL,
                error_message TEXT,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY(account_id) REFERENCES accounts(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS forwarding_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                account_id INTEGER,
                account_email TEXT NOT NULL DEFAULT '',
                message_id TEXT NOT NULL DEFAULT '',
                channel TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL,
                error_message TEXT,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY(account_id) REFERENCES accounts(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS backup_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                target TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL,
                file_name TEXT NOT NULL DEFAULT '',
                size INTEGER NOT NULL DEFAULT 0,
                error_message TEXT,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS automation_runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                job_type TEXT NOT NULL,
                trigger_type TEXT NOT NULL DEFAULT 'manual',
                status TEXT NOT NULL,
                message TEXT NOT NULL DEFAULT '',
                refreshed INTEGER NOT NULL DEFAULT 0,
                failed INTEGER NOT NULL DEFAULT 0,
                duration_ms INTEGER NOT NULL DEFAULT 0,
                started_at TEXT NOT NULL,
                finished_at TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS retry_queue (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_type TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                account_id INTEGER,
                account_email TEXT NOT NULL DEFAULT '',
                message_id TEXT NOT NULL DEFAULT '',
                channel TEXT NOT NULL DEFAULT '',
                action TEXT NOT NULL DEFAULT '',
                payload_json TEXT NOT NULL DEFAULT '{}',
                error_message TEXT NOT NULL DEFAULT '',
                attempts INTEGER NOT NULL DEFAULT 0,
                max_attempts INTEGER NOT NULL DEFAULT 5,
                next_attempt_at TEXT,
                last_attempt_at TEXT,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS projects (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                project_key TEXT UNIQUE NOT NULL,
                description TEXT DEFAULT '',
                scope_mode TEXT NOT NULL DEFAULT 'all',
                use_alias_email INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'active',
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS project_group_scopes (
                project_id INTEGER NOT NULL,
                group_id INTEGER NOT NULL,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                PRIMARY KEY(project_id, group_id),
                FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
                FOREIGN KEY(group_id) REFERENCES groups(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS project_tag_scopes (
                project_id INTEGER NOT NULL,
                tag_id INTEGER NOT NULL,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                PRIMARY KEY(project_id, tag_id),
                FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
                FOREIGN KEY(tag_id) REFERENCES tags(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS project_accounts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id INTEGER NOT NULL,
                account_id INTEGER,
                normalized_email TEXT NOT NULL,
                email_snapshot TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'toClaim',
                claim_token TEXT,
                claimed_at TEXT,
                lease_expires_at TEXT,
                last_result TEXT DEFAULT '',
                last_result_detail TEXT DEFAULT '',
                claim_count INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(project_id, normalized_email),
                FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
                FOREIGN KEY(account_id) REFERENCES accounts(id) ON DELETE SET NULL
            );

            CREATE TABLE IF NOT EXISTS project_account_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id INTEGER NOT NULL,
                account_id INTEGER,
                project_account_id INTEGER,
                normalized_email TEXT NOT NULL,
                action TEXT NOT NULL,
                from_status TEXT,
                to_status TEXT,
                detail TEXT DEFAULT '',
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS email_share_links (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                account_id INTEGER NOT NULL,
                token_hash TEXT UNIQUE NOT NULL,
                exported_path TEXT NOT NULL DEFAULT '',
                title TEXT NOT NULL DEFAULT '',
                file_name TEXT NOT NULL DEFAULT '',
                item_count INTEGER NOT NULL DEFAULT 0,
                size INTEGER NOT NULL DEFAULT 0,
                message_ids_json TEXT NOT NULL DEFAULT '[]',
                expires_at TEXT,
                revoked_at TEXT,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY(account_id) REFERENCES accounts(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS audit_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                action TEXT NOT NULL,
                resource_type TEXT NOT NULL,
                resource_id TEXT,
                detail TEXT DEFAULT '',
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE INDEX IF NOT EXISTS idx_accounts_group ON accounts(group_id);
            CREATE INDEX IF NOT EXISTS idx_messages_account_folder ON retained_mail_messages(account_id, folder);
            CREATE INDEX IF NOT EXISTS idx_messages_received ON retained_mail_messages(received_at_sort DESC);
            CREATE INDEX IF NOT EXISTS idx_temp_messages_email ON temp_email_messages(email_address, timestamp DESC);
            CREATE INDEX IF NOT EXISTS idx_temp_emails_provider ON temp_emails(provider, status);
            CREATE INDEX IF NOT EXISTS idx_project_accounts_project_status ON project_accounts(project_id, status);
            CREATE INDEX IF NOT EXISTS idx_project_events_project_created ON project_account_events(project_id, created_at DESC);
            CREATE INDEX IF NOT EXISTS idx_forwarding_logs_message ON forwarding_logs(account_id, message_id, channel, status);
            CREATE INDEX IF NOT EXISTS idx_backup_logs_created ON backup_logs(created_at DESC);
            CREATE INDEX IF NOT EXISTS idx_automation_runs_created ON automation_runs(created_at DESC);
            CREATE INDEX IF NOT EXISTS idx_retry_queue_status_due ON retry_queue(status, next_attempt_at, created_at);
            CREATE INDEX IF NOT EXISTS idx_retry_queue_key ON retry_queue(task_type, account_id, message_id, channel, action, status);
            CREATE INDEX IF NOT EXISTS idx_audit_created ON audit_logs(created_at DESC);
            ",
        )?;
        self.ensure_default_data()?;
        self.ensure_group_columns()?;
        self.ensure_account_columns()?;
        self.ensure_project_columns()?;
        self.ensure_temp_columns()?;
        self.ensure_message_columns()?;
        self.ensure_share_columns()?;
        Ok(())
    }

    fn ensure_account_columns(&self) -> AppResult<()> {
        let columns = table_columns(&self.conn, "accounts")?;
        for (name, ddl) in [
            ("imap_host", "ALTER TABLE accounts ADD COLUMN imap_host TEXT DEFAULT ''"),
            ("imap_port", "ALTER TABLE accounts ADD COLUMN imap_port INTEGER NOT NULL DEFAULT 993"),
            (
                "imap_password_enc",
                "ALTER TABLE accounts ADD COLUMN imap_password_enc TEXT NOT NULL DEFAULT ''",
            ),
            (
                "forward_enabled",
                "ALTER TABLE accounts ADD COLUMN forward_enabled INTEGER NOT NULL DEFAULT 0",
            ),
            (
                "forward_last_checked_at",
                "ALTER TABLE accounts ADD COLUMN forward_last_checked_at TEXT",
            ),
            (
                "proxy_url",
                "ALTER TABLE accounts ADD COLUMN proxy_url TEXT DEFAULT ''",
            ),
            (
                "fallback_proxy_url_1",
                "ALTER TABLE accounts ADD COLUMN fallback_proxy_url_1 TEXT DEFAULT ''",
            ),
            (
                "fallback_proxy_url_2",
                "ALTER TABLE accounts ADD COLUMN fallback_proxy_url_2 TEXT DEFAULT ''",
            ),
            (
                "refresh_token_updated_at",
                "ALTER TABLE accounts ADD COLUMN refresh_token_updated_at TEXT",
            ),
        ] {
            if !columns.iter().any(|column| column == name) {
                self.conn.execute(ddl, [])?;
            }
        }
        Ok(())
    }

    fn ensure_group_columns(&self) -> AppResult<()> {
        let columns = table_columns(&self.conn, "groups")?;
        for (name, ddl) in [
            ("proxy_url", "ALTER TABLE groups ADD COLUMN proxy_url TEXT DEFAULT ''"),
            (
                "fallback_proxy_url_1",
                "ALTER TABLE groups ADD COLUMN fallback_proxy_url_1 TEXT DEFAULT ''",
            ),
            (
                "fallback_proxy_url_2",
                "ALTER TABLE groups ADD COLUMN fallback_proxy_url_2 TEXT DEFAULT ''",
            ),
        ] {
            if !columns.iter().any(|column| column == name) {
                self.conn.execute(ddl, [])?;
            }
        }
        Ok(())
    }

    fn ensure_project_columns(&self) -> AppResult<()> {
        let columns = table_columns(&self.conn, "projects")?;
        for (name, ddl) in [(
            "use_alias_email",
            "ALTER TABLE projects ADD COLUMN use_alias_email INTEGER NOT NULL DEFAULT 0",
        )] {
            if !columns.iter().any(|column| column == name) {
                self.conn.execute(ddl, [])?;
            }
        }
        Ok(())
    }

    fn ensure_temp_columns(&self) -> AppResult<()> {
        let columns = table_columns(&self.conn, "temp_emails")?;
        for (name, ddl) in [
            (
                "provider_account_id",
                "ALTER TABLE temp_emails ADD COLUMN provider_account_id TEXT NOT NULL DEFAULT ''",
            ),
            (
                "provider_password_enc",
                "ALTER TABLE temp_emails ADD COLUMN provider_password_enc TEXT NOT NULL DEFAULT ''",
            ),
            (
                "last_refresh_at",
                "ALTER TABLE temp_emails ADD COLUMN last_refresh_at TEXT",
            ),
            (
                "last_refresh_status",
                "ALTER TABLE temp_emails ADD COLUMN last_refresh_status TEXT NOT NULL DEFAULT 'never'",
            ),
            (
                "last_refresh_error",
                "ALTER TABLE temp_emails ADD COLUMN last_refresh_error TEXT",
            ),
            (
                "tags_json",
                "ALTER TABLE temp_emails ADD COLUMN tags_json TEXT NOT NULL DEFAULT '[]'",
            ),
        ] {
            if !columns.iter().any(|column| column == name) {
                self.conn.execute(ddl, [])?;
            }
        }
        Ok(())
    }

    fn ensure_message_columns(&self) -> AppResult<()> {
        let columns = table_columns(&self.conn, "retained_mail_messages")?;
        for (name, ddl) in [("raw_mime", "ALTER TABLE retained_mail_messages ADD COLUMN raw_mime BLOB")] {
            if !columns.iter().any(|column| column == name) {
                self.conn.execute(ddl, [])?;
            }
        }
        Ok(())
    }

    fn ensure_share_columns(&self) -> AppResult<()> {
        let columns = table_columns(&self.conn, "email_share_links")?;
        for (name, ddl) in [
            ("title", "ALTER TABLE email_share_links ADD COLUMN title TEXT NOT NULL DEFAULT ''"),
            ("file_name", "ALTER TABLE email_share_links ADD COLUMN file_name TEXT NOT NULL DEFAULT ''"),
            ("item_count", "ALTER TABLE email_share_links ADD COLUMN item_count INTEGER NOT NULL DEFAULT 0"),
            ("size", "ALTER TABLE email_share_links ADD COLUMN size INTEGER NOT NULL DEFAULT 0"),
            (
                "message_ids_json",
                "ALTER TABLE email_share_links ADD COLUMN message_ids_json TEXT NOT NULL DEFAULT '[]'",
            ),
        ] {
            if !columns.iter().any(|column| column == name) {
                self.conn.execute(ddl, [])?;
            }
        }
        Ok(())
    }

    fn ensure_default_data(&self) -> AppResult<()> {
        self.conn.execute(
            "
            INSERT OR IGNORE INTO groups (id, name, description, color, sort_order, is_system)
            VALUES (1, 'Default', 'Default mailbox group', '#3b82f6', 0, 1)
            ",
            [],
        )?;
        for (name, color) in [
            ("Core", "#2563eb"),
            ("Warmup", "#16a34a"),
            ("Issue", "#dc2626"),
        ] {
            self.conn.execute(
                "INSERT OR IGNORE INTO tags (name, color) VALUES (?, ?)",
                params![name, color],
            )?;
        }
        Ok(())
    }

    fn require_unlocked(&self) -> AppResult<()> {
        if self.crypto_key.is_none() {
            return Err(AppError::Unauthorized);
        }
        Ok(())
    }

    fn get_config(&self, key: &str) -> AppResult<Option<String>> {
        self.conn
            .query_row("SELECT value FROM app_config WHERE key = ?", [key], |row| row.get(0))
            .optional()
            .map_err(AppError::from)
    }

    fn set_config(&self, key: &str, value: &str) -> AppResult<()> {
        self.conn.execute(
            "
            INSERT INTO app_config (key, value, updated_at)
            VALUES (?, ?, CURRENT_TIMESTAMP)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = CURRENT_TIMESTAMP
            ",
            params![key, value],
        )?;
        Ok(())
    }

    fn get_config_secret(&self, key: &str) -> AppResult<String> {
        let value = self.get_config(key)?.unwrap_or_default();
        if value.is_empty() {
            return Ok(value);
        }
        let crypto_key = self.crypto_key.as_ref().ok_or(AppError::Unauthorized)?;
        match crypto::decrypt_text(&value, crypto_key) {
            Ok(secret) => Ok(secret),
            Err(_) => Ok(value),
        }
    }

    fn set_config_secret(&self, key: &str, value: &str) -> AppResult<()> {
        let encrypted = if value.is_empty() {
            String::new()
        } else {
            let crypto_key = self.crypto_key.as_ref().ok_or(AppError::Unauthorized)?;
            crypto::encrypt_text(value, crypto_key)?
        };
        self.set_config(key, &encrypted)
    }

    fn get_config_bool(&self, key: &str, default: bool) -> AppResult<bool> {
        Ok(self
            .get_config(key)?
            .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
            .unwrap_or(default))
    }

    fn set_config_bool(&self, key: &str, value: bool) -> AppResult<()> {
        self.set_config(key, if value { "1" } else { "0" })
    }

    fn get_config_i64(&self, key: &str, default: i64) -> AppResult<i64> {
        Ok(self
            .get_config(key)?
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(default))
    }

    fn set_config_i64(&self, key: &str, value: i64) -> AppResult<()> {
        self.set_config(key, &value.to_string())
    }

    fn scalar_count(&self, sql: &str) -> AppResult<i64> {
        self.conn
            .query_row(sql, [], |row| row.get(0))
            .map_err(AppError::from)
    }

    fn get_group(&self, id: i64) -> AppResult<Group> {
        self.conn
            .query_row(
                "
                SELECT g.id, g.name, COALESCE(g.description, ''), g.color,
                       COALESCE(g.proxy_url, ''), COALESCE(g.fallback_proxy_url_1, ''),
                       COALESCE(g.fallback_proxy_url_2, ''), g.parent_id, g.level,
                       g.sort_order, COUNT(a.id) AS account_count
                FROM groups g
                LEFT JOIN accounts a ON a.group_id = g.id
                WHERE g.id = ?
                GROUP BY g.id
                ",
                [id],
                |row| {
                    Ok(Group {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        description: row.get(2)?,
                        color: row.get(3)?,
                        proxy_url: row.get(4)?,
                        fallback_proxy_url_1: row.get(5)?,
                        fallback_proxy_url_2: row.get(6)?,
                        parent_id: row.get(7)?,
                        level: row.get(8)?,
                        sort_order: row.get(9)?,
                        account_count: row.get(10)?,
                    })
                },
            )
            .map_err(AppError::from)
    }

    fn group_descendant_ids(&self, group_id: i64) -> AppResult<Vec<i64>> {
        let mut descendants = Vec::new();
        let mut stack = vec![group_id];
        while let Some(parent_id) = stack.pop() {
            let mut stmt = self.conn.prepare("SELECT id FROM groups WHERE parent_id = ?")?;
            let rows = stmt.query_map([parent_id], |row| row.get::<_, i64>(0))?;
            for row in rows {
                let id = row?;
                descendants.push(id);
                stack.push(id);
            }
        }
        Ok(descendants)
    }

    fn group_subtree_depth(&self, group_id: i64, current_level: i64) -> AppResult<i64> {
        let descendants = self.group_descendant_ids(group_id)?;
        let mut max_level = current_level;
        for id in descendants {
            let level = self
                .conn
                .query_row("SELECT level FROM groups WHERE id = ?", [id], |row| row.get::<_, i64>(0))?;
            max_level = max_level.max(level);
        }
        Ok(max_level - current_level)
    }

    fn shift_group_descendant_levels(&self, group_id: i64, delta: i64) -> AppResult<()> {
        let descendants = self.group_descendant_ids(group_id)?;
        self.shift_group_levels(&descendants, delta)
    }

    fn shift_group_levels(&self, group_ids: &[i64], delta: i64) -> AppResult<()> {
        for id in group_ids {
            self.conn.execute(
                "UPDATE groups SET level = level + ? WHERE id = ?",
                params![delta, id],
            )?;
        }
        Ok(())
    }

    fn tags_for_account(&self, account_id: i64) -> AppResult<Vec<Tag>> {
        let mut stmt = self.conn.prepare(
            "
            SELECT t.id, t.name, t.color
            FROM tags t
            JOIN account_tags at ON at.tag_id = t.id
            WHERE at.account_id = ?
            ORDER BY t.name
            ",
        )?;
        let rows = stmt.query_map([account_id], |row| {
            Ok(Tag {
                id: row.get(0)?,
                name: row.get(1)?,
                color: row.get(2)?,
            })
        })?;
        collect_rows(rows)
    }

    fn aliases_for_account(&self, account_id: i64) -> AppResult<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "
            SELECT alias_email
            FROM account_aliases
            WHERE account_id = ?
            ORDER BY created_at ASC, id ASC
            ",
        )?;
        let rows = stmt.query_map([account_id], |row| row.get::<_, String>(0))?;
        collect_rows(rows)
    }

    fn primary_alias_for_account(&self, account_id: i64) -> AppResult<Option<String>> {
        self.conn
            .query_row(
                "
                SELECT alias_email
                FROM account_aliases
                WHERE account_id = ?
                ORDER BY created_at ASC, id ASC
                LIMIT 1
                ",
                [account_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(AppError::from)
    }

    fn ensure_primary_email_is_not_alias(&self, account_id: i64, email: &str) -> AppResult<()> {
        let conflict = self
            .conn
            .query_row(
                "SELECT account_id FROM account_aliases WHERE alias_email = ? AND account_id != ? LIMIT 1",
                params![email, account_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        if conflict.is_some() {
            return Err(AppError::InvalidInput(
                "account email conflicts with another account alias".to_string(),
            ));
        }
        Ok(())
    }

    fn normalize_account_aliases(
        &self,
        account_id: i64,
        primary_email: &str,
        aliases: Vec<String>,
    ) -> AppResult<Vec<String>> {
        let mut normalized_aliases = Vec::new();
        let mut seen = HashSet::new();
        for value in aliases {
            let alias = normalize_email(&value).map_err(|_| {
                AppError::InvalidInput("account alias email is invalid".to_string())
            })?;
            if alias == primary_email {
                return Err(AppError::InvalidInput(
                    "account alias cannot equal the primary email".to_string(),
                ));
            }
            if !seen.insert(alias.clone()) {
                continue;
            }

            let primary_conflict = self
                .conn
                .query_row(
                    "SELECT id FROM accounts WHERE email = ? AND id != ? LIMIT 1",
                    params![alias, account_id],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?;
            if primary_conflict.is_some() {
                return Err(AppError::InvalidInput(
                    "account alias conflicts with another primary email".to_string(),
                ));
            }

            let alias_conflict = self
                .conn
                .query_row(
                    "SELECT account_id FROM account_aliases WHERE alias_email = ? AND account_id != ? LIMIT 1",
                    params![alias, account_id],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?;
            if alias_conflict.is_some() {
                return Err(AppError::InvalidInput(
                    "account alias conflicts with another account alias".to_string(),
                ));
            }
            normalized_aliases.push(alias);
        }
        Ok(normalized_aliases)
    }

    fn replace_account_aliases(
        &self,
        account_id: i64,
        primary_email: &str,
        aliases: Vec<String>,
    ) -> AppResult<()> {
        let aliases = self.normalize_account_aliases(account_id, primary_email, aliases)?;
        self.conn
            .execute("DELETE FROM account_aliases WHERE account_id = ?", [account_id])?;
        for alias in aliases {
            self.conn.execute(
                "INSERT INTO account_aliases (account_id, alias_email) VALUES (?, ?)",
                params![account_id, alias],
            )?;
        }
        Ok(())
    }

    fn replace_account_tags(&self, account_id: i64, tag_ids: Vec<i64>) -> AppResult<()> {
        self.conn
            .execute("DELETE FROM account_tags WHERE account_id = ?", [account_id])?;
        for tag_id in tag_ids {
            self.conn.execute(
                "INSERT OR IGNORE INTO account_tags (account_id, tag_id) VALUES (?, ?)",
                params![account_id, tag_id],
            )?;
        }
        Ok(())
    }

    fn project_group_ids(&self, project_id: i64) -> AppResult<Vec<i64>> {
        let mut stmt = self
            .conn
            .prepare("SELECT group_id FROM project_group_scopes WHERE project_id = ? ORDER BY group_id")?;
        let rows = stmt.query_map([project_id], |row| row.get::<_, i64>(0))?;
        collect_rows(rows)
    }

    fn project_tag_ids(&self, project_id: i64) -> AppResult<Vec<i64>> {
        let mut stmt = self
            .conn
            .prepare("SELECT tag_id FROM project_tag_scopes WHERE project_id = ? ORDER BY tag_id")?;
        let rows = stmt.query_map([project_id], |row| row.get::<_, i64>(0))?;
        collect_rows(rows)
    }

    fn replace_project_group_scope(&self, project_id: i64, group_ids: Vec<i64>) -> AppResult<()> {
        self.conn
            .execute("DELETE FROM project_group_scopes WHERE project_id = ?", [project_id])?;
        for group_id in group_ids {
            self.conn.execute(
                "INSERT OR IGNORE INTO project_group_scopes (project_id, group_id) VALUES (?, ?)",
                params![project_id, group_id],
            )?;
        }
        Ok(())
    }

    fn replace_project_tag_scope(&self, project_id: i64, tag_ids: Vec<i64>) -> AppResult<()> {
        self.conn
            .execute("DELETE FROM project_tag_scopes WHERE project_id = ?", [project_id])?;
        for tag_id in tag_ids {
            self.conn.execute(
                "INSERT OR IGNORE INTO project_tag_scopes (project_id, tag_id) VALUES (?, ?)",
                params![project_id, tag_id],
            )?;
        }
        Ok(())
    }

    fn project_stats(&self, project_id: i64) -> AppResult<ProjectStats> {
        let mut stats = ProjectStats::default();
        let mut stmt = self.conn.prepare(
            "
            SELECT status, COUNT(*)
            FROM project_accounts
            WHERE project_id = ?
            GROUP BY status
            ",
        )?;
        let rows = stmt.query_map([project_id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))?;
        for row in rows {
            let (status, count) = row?;
            stats.total += count;
            match status.as_str() {
                "toClaim" => stats.to_claim = count,
                "claimed" => stats.claimed = count,
                "success" => stats.success = count,
                "failed" => stats.failed = count,
                "removed" => stats.removed = count,
                _ => {}
            }
        }
        Ok(stats)
    }

    fn accounts_for_project_scope(
        &self,
        scope_mode: &str,
        use_alias_email: bool,
        group_ids: &[i64],
        tag_ids: &[i64],
    ) -> AppResult<Vec<(i64, String)>> {
        if scope_mode == "groups" && group_ids.is_empty() {
            return Ok(Vec::new());
        }
        if scope_mode == "tags" && tag_ids.is_empty() {
            return Ok(Vec::new());
        }
        let sql = match scope_mode {
            "groups" => format!(
                "SELECT id, email FROM accounts WHERE status = 'active' AND group_id IN ({}) ORDER BY email",
                std::iter::repeat("?")
                    .take(group_ids.len())
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            "tags" => format!(
                "
                SELECT DISTINCT a.id, a.email
                FROM accounts a
                JOIN account_tags at ON at.account_id = a.id
                WHERE a.status = 'active'
                  AND at.tag_id IN ({})
                ORDER BY a.email
                ",
                std::iter::repeat("?")
                    .take(tag_ids.len())
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            _ => "SELECT id, email FROM accounts WHERE status = 'active' ORDER BY email".to_string(),
        };
        let mut stmt = self.conn.prepare(&sql)?;
        let mut accounts = if scope_mode == "groups" {
            let params = rusqlite::params_from_iter(group_ids.iter());
            let rows = stmt.query_map(params, |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?;
            collect_rows(rows)
        } else if scope_mode == "tags" {
            let params = rusqlite::params_from_iter(tag_ids.iter());
            let rows = stmt.query_map(params, |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?;
            collect_rows(rows)
        } else {
            let rows = stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?;
            collect_rows(rows)
        }?;
        if use_alias_email {
            for (account_id, email) in &mut accounts {
                if let Some(alias) = self.primary_alias_for_account(*account_id)? {
                    *email = alias;
                }
            }
        }
        Ok(accounts)
    }

    fn project_account_ids(&self, project_id: i64) -> AppResult<Vec<(i64, Option<i64>, String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, account_id, normalized_email, status FROM project_accounts WHERE project_id = ?",
        )?;
        let rows = stmt.query_map([project_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<i64>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        collect_rows(rows)
    }

    fn get_project_account(&self, project_account_id: i64) -> AppResult<ProjectAccount> {
        self.conn
            .query_row(
                "
                SELECT id, project_id, account_id, normalized_email, email_snapshot, status, claim_token,
                       claimed_at, lease_expires_at, COALESCE(last_result, ''), COALESCE(last_result_detail, ''),
                       claim_count, created_at, updated_at
                FROM project_accounts
                WHERE id = ?
                ",
                [project_account_id],
                project_account_from_row,
            )
            .optional()?
            .ok_or_else(|| AppError::InvalidInput("project account not found".to_string()))
    }

    fn transition_project_account(
        &self,
        project_account_id: i64,
        next_status: &str,
        action: &str,
        detail: &str,
        account_id_override: Option<i64>,
    ) -> AppResult<ProjectAccount> {
        self.require_unlocked()?;
        validate_project_status(next_status)?;
        let before = self.get_project_account(project_account_id)?;
        let result = match next_status {
            "success" => "success",
            "failed" => "failed",
            _ => "",
        };
        self.conn.execute(
            "
            UPDATE project_accounts
            SET status = ?,
                claim_token = CASE WHEN ? IN ('toClaim', 'success', 'failed', 'removed') THEN NULL ELSE claim_token END,
                lease_expires_at = CASE WHEN ? IN ('toClaim', 'success', 'failed', 'removed') THEN NULL ELSE lease_expires_at END,
                last_result = ?,
                last_result_detail = ?,
                updated_at = CURRENT_TIMESTAMP
            WHERE id = ?
            ",
            params![next_status, next_status, next_status, result, detail, project_account_id],
        )?;
        let mut after = self.get_project_account(project_account_id)?;
        if let Some(account_id) = account_id_override {
            after.account_id = Some(account_id);
        }
        self.insert_project_event(&before, action, Some(&before.status), Some(next_status), detail)?;
        Ok(after)
    }

    fn insert_project_event(
        &self,
        account: &ProjectAccount,
        action: &str,
        from_status: Option<&str>,
        to_status: Option<&str>,
        detail: &str,
    ) -> AppResult<()> {
        self.conn.execute(
            "
            INSERT INTO project_account_events
            (project_id, account_id, project_account_id, normalized_email, action, from_status, to_status, detail)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            ",
            params![
                account.project_id,
                account.account_id,
                account.id,
                account.normalized_email,
                action,
                from_status,
                to_status,
                detail
            ],
        )?;
        Ok(())
    }

    fn account_credentials(&self, account_id: Option<i64>) -> AppResult<Vec<AccountCredentials>> {
        let key = self.crypto_key.as_ref().ok_or(AppError::Unauthorized)?;
        let mut stmt = self.conn.prepare(
            "
            SELECT a.id, a.email, a.provider, a.account_type, a.password_enc, a.client_id_enc,
                   a.refresh_token_enc, COALESCE(a.imap_host, ''), a.imap_port, a.imap_password_enc,
                   COALESCE(a.proxy_url, ''), COALESCE(a.fallback_proxy_url_1, ''),
                   COALESCE(a.fallback_proxy_url_2, ''), COALESCE(g.proxy_url, ''),
                   COALESCE(g.fallback_proxy_url_1, ''), COALESCE(g.fallback_proxy_url_2, '')
            FROM accounts a
            LEFT JOIN groups g ON g.id = a.group_id
            WHERE a.status = 'active' AND (?1 IS NULL OR a.id = ?1)
            ORDER BY a.sort_order ASC, a.email ASC
            ",
        )?;
        let rows = stmt.query_map([account_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, i64>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, String>(10)?,
                row.get::<_, String>(11)?,
                row.get::<_, String>(12)?,
                row.get::<_, String>(13)?,
                row.get::<_, String>(14)?,
                row.get::<_, String>(15)?,
            ))
        })?;

        let mut credentials = Vec::new();
        for row in rows {
            let (
                id,
                email,
                provider,
                account_type,
                password,
                client_id,
                refresh_token,
                imap_host,
                imap_port,
                imap_password,
                account_proxy,
                account_proxy_1,
                account_proxy_2,
                group_proxy,
                group_proxy_1,
                group_proxy_2,
            ) = row?;
            let mut proxy_chain = proxy_chain_from_values(&[&account_proxy, &account_proxy_1, &account_proxy_2])?;
            if proxy_chain.is_empty() {
                proxy_chain = proxy_chain_from_values(&[&group_proxy, &group_proxy_1, &group_proxy_2])?;
            }
            credentials.push(AccountCredentials {
                id,
                email,
                provider,
                account_type,
                password: crypto::decrypt_text(&password, key)?,
                client_id: crypto::decrypt_text(&client_id, key)?,
                refresh_token: crypto::decrypt_text(&refresh_token, key)?,
                imap_host,
                imap_port,
                imap_password: crypto::decrypt_text(&imap_password, key)?,
                proxy_chain,
            });
        }
        Ok(credentials)
    }

    fn proxy_chain_for_account(&self, account_id: i64) -> AppResult<Vec<String>> {
        let row = self
            .conn
            .query_row(
                "
                SELECT COALESCE(a.proxy_url, ''), COALESCE(a.fallback_proxy_url_1, ''),
                       COALESCE(a.fallback_proxy_url_2, ''), COALESCE(g.proxy_url, ''),
                       COALESCE(g.fallback_proxy_url_1, ''), COALESCE(g.fallback_proxy_url_2, '')
                FROM accounts a
                LEFT JOIN groups g ON g.id = a.group_id
                WHERE a.id = ?
                ",
                [account_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| AppError::InvalidInput("account not found".to_string()))?;
        let account_chain = proxy_chain_from_values(&[&row.0, &row.1, &row.2])?;
        if !account_chain.is_empty() {
            return Ok(account_chain);
        }
        proxy_chain_from_values(&[&row.3, &row.4, &row.5])
    }

    fn save_refresh_token(&self, account_id: i64, refresh_token: &str) -> AppResult<()> {
        let key = self.crypto_key.as_ref().ok_or(AppError::Unauthorized)?;
        let encrypted = crypto::encrypt_text(refresh_token, key)?;
        self.conn.execute(
            "
            UPDATE accounts
            SET refresh_token_enc = ?, refresh_token_updated_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP
            WHERE id = ?
            ",
            params![encrypted, account_id],
        )?;
        Ok(())
    }

    fn upsert_provider_messages(&self, account_id: i64, messages: &[ProviderMessage]) -> AppResult<()> {
        for message in messages {
            self.conn.execute(
                "
                INSERT INTO retained_mail_messages
                (account_id, folder, provider_message_id, subject, sender, recipients, cc,
                 received_at, received_at_sort, is_read, has_attachments, body_preview, body,
                 body_type, attachments_json, raw_mime, body_cached, last_synced_at, updated_at)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
                ON CONFLICT(account_id, folder, provider_message_id) DO UPDATE SET
                    subject = excluded.subject,
                    sender = excluded.sender,
                    recipients = excluded.recipients,
                    cc = excluded.cc,
                    received_at = excluded.received_at,
                    received_at_sort = excluded.received_at_sort,
                    is_read = excluded.is_read,
                    has_attachments = excluded.has_attachments,
                    body_preview = excluded.body_preview,
                    body = COALESCE(excluded.body, retained_mail_messages.body),
                    body_type = excluded.body_type,
                    attachments_json = excluded.attachments_json,
                    raw_mime = COALESCE(excluded.raw_mime, retained_mail_messages.raw_mime),
                    body_cached = CASE WHEN excluded.body IS NULL THEN retained_mail_messages.body_cached ELSE 1 END,
                    last_synced_at = CURRENT_TIMESTAMP,
                    updated_at = CURRENT_TIMESTAMP
                ",
                params![
                    account_id,
                    message.folder,
                    message.provider_message_id,
                    message.subject,
                    message.sender,
                    message.recipients,
                    message.cc,
                    message.received_at,
                    message.received_at_sort,
                    if message.is_read { 1 } else { 0 },
                    if message.has_attachments { 1 } else { 0 },
                    message.body_preview,
                    message.body,
                    message.body_type,
                    serde_json::to_string(&message.attachments).unwrap_or_else(|_| "[]".to_string()),
                    message.raw_mime.as_deref(),
                    if message.body.is_some() { 1 } else { 0 },
                ],
            )?;
        }
        Ok(())
    }

    fn cached_imap_raw_mime(&self, account_id: i64, message_id: &str, folder: Option<&str>) -> AppResult<Vec<u8>> {
        let raw_mime: Option<Vec<u8>> = match folder {
            Some(folder) => self
                .conn
                .query_row(
                    "
                    SELECT raw_mime
                    FROM retained_mail_messages
                    WHERE account_id = ? AND provider_message_id = ? AND folder = ?
                    ORDER BY id DESC
                    LIMIT 1
                    ",
                    params![account_id, message_id, folder],
                    |row| row.get::<_, Option<Vec<u8>>>(0),
                )
                .optional()?
                .flatten(),
            None => self
                .conn
                .query_row(
                    "
                    SELECT raw_mime
                    FROM retained_mail_messages
                    WHERE account_id = ? AND provider_message_id = ?
                    ORDER BY id DESC
                    LIMIT 1
                    ",
                    params![account_id, message_id],
                    |row| row.get::<_, Option<Vec<u8>>>(0),
                )
                .optional()?
                .flatten(),
        };
        let raw_mime = raw_mime.ok_or_else(|| AppError::InvalidInput("cached IMAP message not found".to_string()))?;
        if raw_mime.is_empty() {
            return Err(AppError::InvalidInput(
                "cached IMAP raw MIME is missing; refresh the account before downloading this attachment".to_string(),
            ));
        }
        Ok(raw_mime)
    }

    fn cached_message_attachments(&self, account_id: i64, message_id: &str, folder: Option<&str>) -> AppResult<Vec<AttachmentInfo>> {
        let attachments_json: Option<String> = match folder {
            Some(folder) => self
                .conn
                .query_row(
                    "
                    SELECT attachments_json
                    FROM retained_mail_messages
                    WHERE account_id = ? AND provider_message_id = ? AND folder = ?
                    ORDER BY id DESC
                    LIMIT 1
                    ",
                    params![account_id, message_id, folder],
                    |row| row.get::<_, Option<String>>(0),
                )
                .optional()?
                .flatten(),
            None => self
                .conn
                .query_row(
                    "
                    SELECT attachments_json
                    FROM retained_mail_messages
                    WHERE account_id = ? AND provider_message_id = ?
                    ORDER BY id DESC
                    LIMIT 1
                    ",
                    params![account_id, message_id],
                    |row| row.get::<_, Option<String>>(0),
                )
                .optional()?
                .flatten(),
        };
        let attachments_json = attachments_json.ok_or_else(|| AppError::InvalidInput("cached message not found".to_string()))?;
        Ok(parse_attachments_json(&attachments_json))
    }

    fn mark_account_refresh_success(&self, account_id: i64, email: &str, count: usize) -> AppResult<()> {
        self.conn.execute(
            "
            UPDATE accounts
            SET last_refresh_at = CURRENT_TIMESTAMP,
                last_refresh_status = 'success',
                last_refresh_error = NULL,
                updated_at = CURRENT_TIMESTAMP
            WHERE id = ?
            ",
            [account_id],
        )?;
        self.conn.execute(
            "
            INSERT INTO refresh_logs (account_id, account_email, refresh_type, status, error_message)
            VALUES (?, ?, 'manual', 'success', ?)
            ",
            params![account_id, email, format!("{} message(s) cached", count)],
        )?;
        Ok(())
    }

    fn mark_account_refresh_failed(&self, account_id: i64, email: &str, error: &str) -> AppResult<()> {
        self.conn.execute(
            "
            UPDATE accounts
            SET last_refresh_at = CURRENT_TIMESTAMP,
                last_refresh_status = 'failed',
                last_refresh_error = ?,
                updated_at = CURRENT_TIMESTAMP
            WHERE id = ?
            ",
            params![error, account_id],
        )?;
        self.conn.execute(
            "
            INSERT INTO refresh_logs (account_id, account_email, refresh_type, status, error_message)
            VALUES (?, ?, 'manual', 'failed', ?)
            ",
            params![account_id, email, error],
        )?;
        Ok(())
    }

    fn forwarding_accounts(&self, account_id: Option<i64>) -> AppResult<Vec<(i64, String)>> {
        let mut stmt = self.conn.prepare(
            "
            SELECT id, email
            FROM accounts
            WHERE status = 'active'
              AND forward_enabled = 1
              AND (?1 IS NULL OR id = ?1)
            ORDER BY sort_order ASC, email ASC
            ",
        )?;
        let rows = stmt.query_map([account_id], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?;
        collect_rows(rows)
    }

    fn forwarding_candidates(&self, account_id: i64, limit: usize) -> AppResult<Vec<ForwardContent>> {
        let mut stmt = self.conn.prepare(
            "
            SELECT a.email, m.provider_message_id, m.subject, m.sender, m.received_at,
                   m.body_preview, m.body
            FROM retained_mail_messages m
            JOIN accounts a ON a.id = m.account_id
            WHERE m.account_id = ?
            ORDER BY m.received_at_sort DESC, m.id DESC
            LIMIT ?
            ",
        )?;
        let rows = stmt.query_map(params![account_id, limit as i64], |row| {
            Ok(ForwardContent {
                account_email: row.get(0)?,
                message_id: row.get(1)?,
                subject: row.get(2)?,
                sender: row.get(3)?,
                received_at: row.get(4)?,
                body_preview: row.get(5)?,
                body: row.get(6)?,
            })
        })?;
        collect_rows(rows)
    }

    fn forward_success_exists(&self, account_id: i64, message_id: &str, channel: &str) -> AppResult<bool> {
        let exists = self
            .conn
            .query_row(
                "
                SELECT 1
                FROM forwarding_logs
                WHERE account_id = ? AND message_id = ? AND channel = ? AND status = 'success'
                LIMIT 1
                ",
                params![account_id, message_id, channel],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        Ok(exists.is_some())
    }

    fn insert_forwarding_log(
        &self,
        account_id: Option<i64>,
        account_email: &str,
        message_id: &str,
        channel: &str,
        status: &str,
        error: Option<&str>,
    ) -> AppResult<()> {
        self.conn.execute(
            "
            INSERT INTO forwarding_logs (account_id, account_email, message_id, channel, status, error_message)
            VALUES (?, ?, ?, ?, ?, ?)
            ",
            params![account_id, account_email, message_id, channel, status, error],
        )?;
        Ok(())
    }

    fn insert_backup_log(
        &self,
        target: &str,
        status: &str,
        file_name: &str,
        size: i64,
        error: Option<&str>,
    ) -> AppResult<()> {
        self.conn.execute(
            "
            INSERT INTO backup_logs (target, status, file_name, size, error_message)
            VALUES (?, ?, ?, ?, ?)
            ",
            params![target, status, file_name, size, error],
        )?;
        Ok(())
    }

    fn forwarding_channel_circuits(&self, settings: &Settings) -> AppResult<Vec<ForwardingChannelCircuit>> {
        ["smtp", "telegram", "wecom"]
            .iter()
            .map(|channel| self.forwarding_channel_circuit(channel, settings))
            .collect()
    }

    fn forwarding_channel_circuit(&self, channel: &str, settings: &Settings) -> AppResult<ForwardingChannelCircuit> {
        let configured = automation::configured_forward_channels(settings)
            .iter()
            .any(|configured_channel| *configured_channel == channel);
        let since = sqlite_timestamp(Utc::now() - ChronoDuration::minutes(30));
        let recent_log_failures: i64 = self.conn.query_row(
            "
            SELECT COUNT(*)
            FROM forwarding_logs
            WHERE channel = ?
              AND status = 'failed'
              AND datetime(created_at) >= datetime(?)
            ",
            params![channel, since],
            |row| row.get(0),
        )?;
        let recent_retry_failures: i64 = self.conn.query_row(
            "
            SELECT COUNT(*)
            FROM retry_queue
            WHERE task_type = 'forward_message'
              AND channel = ?
              AND status IN ('pending', 'failed')
              AND datetime(updated_at) >= datetime(?)
            ",
            params![channel, since],
            |row| row.get(0),
        )?;
        let pending_retries: i64 = self.conn.query_row(
            "
            SELECT COUNT(*)
            FROM retry_queue
            WHERE task_type = 'forward_message'
              AND channel = ?
              AND status IN ('pending', 'failed')
            ",
            [channel],
            |row| row.get(0),
        )?;
        let last_success_at = self
            .conn
            .query_row(
                "
                SELECT created_at
                FROM forwarding_logs
                WHERE channel = ? AND status = 'success'
                ORDER BY id DESC
                LIMIT 1
                ",
                [channel],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let log_failure = self
            .conn
            .query_row(
                "
                SELECT COALESCE(error_message, ''), created_at
                FROM forwarding_logs
                WHERE channel = ? AND status = 'failed'
                ORDER BY id DESC
                LIMIT 1
                ",
                [channel],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        let retry_failure = self
            .conn
            .query_row(
                "
                SELECT error_message, updated_at
                FROM retry_queue
                WHERE task_type = 'forward_message'
                  AND channel = ?
                  AND status IN ('pending', 'failed')
                ORDER BY updated_at DESC, id DESC
                LIMIT 1
                ",
                [channel],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        let (last_error, last_failure_at) = latest_failure_detail(log_failure, retry_failure);
        let recent_failures = recent_log_failures + recent_retry_failures;
        let open_until = last_failure_at
            .as_deref()
            .and_then(parse_scheduler_timestamp)
            .map(|value| (value + ChronoDuration::minutes(30)).to_rfc3339());
        let is_open = configured
            && recent_failures >= 3
            && open_until
                .as_deref()
                .and_then(parse_scheduler_timestamp)
                .is_some_and(|value| value > Utc::now());
        let status = if !configured {
            "not_configured"
        } else if is_open {
            "open"
        } else if recent_failures > 0 || pending_retries > 0 {
            "degraded"
        } else {
            "healthy"
        };

        Ok(ForwardingChannelCircuit {
            channel: channel.to_string(),
            configured,
            status: status.to_string(),
            recent_failures,
            pending_retries,
            open_until: if is_open { open_until } else { None },
            last_success_at,
            last_failure_at,
            last_error,
        })
    }

    fn retry_queue_candidates(&self, input: RetryQueueRunInput) -> AppResult<Vec<RetryQueueItem>> {
        if let Some(retry_id) = input.retry_id {
            let mut stmt = self.conn.prepare(
                "
                SELECT id, task_type, status, account_id, account_email, message_id, channel,
                       action, payload_json, error_message, attempts, max_attempts,
                       next_attempt_at, last_attempt_at, created_at, updated_at
                FROM retry_queue
                WHERE id = ?
                ",
            )?;
            let rows = stmt.query_map([retry_id], retry_queue_item_from_row)?;
            return collect_rows(rows);
        }

        let limit = input.limit.unwrap_or(20).clamp(1, 100);
        let now = Utc::now().to_rfc3339();
        let mut stmt = self.conn.prepare(
            "
            SELECT id, task_type, status, account_id, account_email, message_id, channel,
                   action, payload_json, error_message, attempts, max_attempts,
                   next_attempt_at, last_attempt_at, created_at, updated_at
            FROM retry_queue
            WHERE status = 'pending'
              AND (next_attempt_at IS NULL OR next_attempt_at <= ?1)
            ORDER BY COALESCE(next_attempt_at, created_at) ASC, id ASC
            LIMIT ?2
            ",
        )?;
        let rows = stmt.query_map(params![now, limit], retry_queue_item_from_row)?;
        collect_rows(rows)
    }

    fn execute_retry_item(&self, item: &RetryQueueItem) -> AppResult<()> {
        match item.task_type.as_str() {
            "mail_mark" => {
                let payload = parse_retry_payload::<MailRetryPayload>(&item.payload_json)?;
                let is_read = payload
                    .is_read
                    .ok_or_else(|| AppError::InvalidInput("mail mark retry is missing read state".to_string()))?;
                self.retry_remote_mark_message(&payload, is_read)
            }
            "mail_delete" => {
                let payload = parse_retry_payload::<MailRetryPayload>(&item.payload_json)?;
                self.retry_remote_delete_message(&payload)
            }
            "forward_message" => {
                let payload = parse_retry_payload::<ForwardRetryPayload>(&item.payload_json)?;
                self.retry_forward_message(&payload)
            }
            "refresh_account" => {
                let payload = parse_retry_payload::<RefreshRetryPayload>(&item.payload_json)?;
                self.retry_refresh_account(&payload)
            }
            "backup_job" => {
                let payload = parse_retry_payload::<BackupRetryPayload>(&item.payload_json)?;
                self.retry_backup_job(&payload)
            }
            "temp_refresh" => {
                let payload = parse_retry_payload::<TempRefreshRetryPayload>(&item.payload_json)?;
                self.retry_temp_refresh(&payload)
            }
            _ => Err(AppError::InvalidInput(format!(
                "unsupported retry task type: {}",
                item.task_type
            ))),
        }
    }

    fn mark_retry_failed(&self, item: &RetryQueueItem, error: &str) -> AppResult<()> {
        let attempts = item.attempts + 1;
        let exhausted = attempts >= item.max_attempts;
        let status = if exhausted { "failed" } else { "pending" };
        let next_attempt_at = if exhausted {
            None
        } else {
            Some((Utc::now() + ChronoDuration::minutes(retry_delay_minutes_for_error(&item.task_type, attempts, error))).to_rfc3339())
        };
        self.conn.execute(
            "
            UPDATE retry_queue
            SET status = ?,
                error_message = ?,
                attempts = ?,
                next_attempt_at = ?,
                last_attempt_at = CURRENT_TIMESTAMP,
                updated_at = CURRENT_TIMESTAMP
            WHERE id = ?
            ",
            params![status, error, attempts, next_attempt_at, item.id],
        )?;
        Ok(())
    }

    fn enqueue_mail_retry(&self, target: &MailMessageRef, is_read: bool, error: &str) -> AppResult<()> {
        let action = if is_read { "mark_read" } else { "mark_unread" };
        self.enqueue_retry_item(
            "mail_mark",
            Some(target.account_id),
            &target.account_email,
            &target.provider_message_id,
            &target.folder,
            action,
            serde_json::json!({
                "account_id": target.account_id,
                "account_email": target.account_email.as_str(),
                "folder": target.folder.as_str(),
                "provider_message_id": target.provider_message_id.as_str(),
                "is_read": is_read
            }),
            error,
        )
    }

    fn enqueue_mail_delete_retry(&self, target: &MailMessageRef, error: &str) -> AppResult<()> {
        self.enqueue_retry_item(
            "mail_delete",
            Some(target.account_id),
            &target.account_email,
            &target.provider_message_id,
            &target.folder,
            "delete",
            serde_json::json!({
                "account_id": target.account_id,
                "account_email": target.account_email.as_str(),
                "folder": target.folder.as_str(),
                "provider_message_id": target.provider_message_id.as_str()
            }),
            error,
        )
    }

    fn enqueue_forwarding_retry(
        &self,
        account_id: i64,
        account_email: &str,
        message: &ForwardContent,
        channel: &str,
        error: &str,
    ) -> AppResult<()> {
        self.enqueue_retry_item(
            "forward_message",
            Some(account_id),
            account_email,
            &message.message_id,
            channel,
            "forward",
            serde_json::json!({
                "account_id": account_id,
                "message_id": message.message_id.as_str(),
                "channel": channel
            }),
            error,
        )
    }

    fn enqueue_refresh_retry(
        &self,
        account: &AccountCredentials,
        folder: &str,
        top: usize,
        error: &str,
    ) -> AppResult<()> {
        self.enqueue_retry_item(
            "refresh_account",
            Some(account.id),
            &account.email,
            folder,
            "mailbox",
            "refresh",
            serde_json::json!({
                "account_id": account.id,
                "account_email": account.email.as_str(),
                "folder": folder,
                "top": top
            }),
            error,
        )
    }

    fn enqueue_backup_retry(&self, target: &str, error: &str) -> AppResult<()> {
        self.enqueue_retry_item(
            "backup_job",
            None,
            "",
            if target.trim().is_empty() { "webdav" } else { target.trim() },
            "backup",
            "backup",
            serde_json::json!({
                "target": target.trim()
            }),
            error,
        )
    }

    fn enqueue_temp_refresh_retry(&self, credential: &TempEmailCredential, error: &str) -> AppResult<()> {
        self.enqueue_retry_item(
            "temp_refresh",
            None,
            "",
            &credential.email,
            &credential.provider,
            "refresh",
            serde_json::json!({
                "email": credential.email.as_str(),
                "provider": credential.provider.as_str()
            }),
            error,
        )
    }

    fn enqueue_retry_item(
        &self,
        task_type: &str,
        account_id: Option<i64>,
        account_email: &str,
        message_id: &str,
        channel: &str,
        action: &str,
        payload: serde_json::Value,
        error: &str,
    ) -> AppResult<()> {
        let payload_json = serde_json::to_string(&payload)
            .map_err(|err| AppError::Internal(format!("serialize retry payload failed: {err}")))?;
        let max_attempts = retry_max_attempts_for_error(task_type, error);
        let next_attempt_at = Some((Utc::now() + ChronoDuration::minutes(retry_delay_minutes_for_error(task_type, 1, error))).to_rfc3339());
        let account_key = account_id.unwrap_or(-1);
        let existing = self
            .conn
            .query_row(
                "
                SELECT id
                FROM retry_queue
                WHERE task_type = ?
                  AND COALESCE(account_id, -1) = ?
                  AND message_id = ?
                  AND channel = ?
                  AND action = ?
                  AND status IN ('pending', 'failed')
                LIMIT 1
                ",
                params![task_type, account_key, message_id, channel, action],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;

        if let Some(id) = existing {
            self.conn.execute(
                "
                UPDATE retry_queue
                SET status = 'pending',
                    account_email = ?,
                    payload_json = ?,
                    error_message = ?,
                    max_attempts = ?,
                    next_attempt_at = COALESCE(next_attempt_at, ?),
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = ?
                ",
                params![account_email, payload_json, error, max_attempts, next_attempt_at, id],
            )?;
        } else {
            self.conn.execute(
                "
                INSERT INTO retry_queue
                (task_type, status, account_id, account_email, message_id, channel,
                 action, payload_json, error_message, max_attempts, next_attempt_at)
                VALUES (?, 'pending', ?, ?, ?, ?, ?, ?, ?, ?, ?)
                ",
                params![
                    task_type,
                    account_id,
                    account_email,
                    message_id,
                    channel,
                    action,
                    payload_json,
                    error,
                    max_attempts,
                    next_attempt_at
                ],
            )?;
        }
        Ok(())
    }

    fn record_job_result(
        &self,
        job_type: &str,
        trigger_type: &str,
        started_at: DateTime<Utc>,
        result: &AppResult<JobResult>,
    ) -> AppResult<()> {
        match result {
            Ok(result) => self.insert_automation_run(
                job_type,
                trigger_type,
                if result.success { "success" } else { "failed" },
                &result.message,
                result.refreshed as i64,
                result.failed as i64,
                started_at,
            ),
            Err(err) => self.insert_automation_run(
                job_type,
                trigger_type,
                "failed",
                &err.to_string(),
                0,
                1,
                started_at,
            ),
        }
    }

    fn record_backup_result(
        &self,
        trigger_type: &str,
        started_at: DateTime<Utc>,
        result: &AppResult<BackupResult>,
    ) -> AppResult<()> {
        match result {
            Ok(result) => self.insert_automation_run(
                "backup",
                trigger_type,
                if result.success { "success" } else { "failed" },
                &result.message,
                1,
                0,
                started_at,
            ),
            Err(err) => self.insert_automation_run(
                "backup",
                trigger_type,
                "failed",
                &err.to_string(),
                0,
                1,
                started_at,
            ),
        }
    }

    fn insert_automation_run(
        &self,
        job_type: &str,
        trigger_type: &str,
        status: &str,
        message: &str,
        refreshed: i64,
        failed: i64,
        started_at: DateTime<Utc>,
    ) -> AppResult<()> {
        let finished_at = Utc::now();
        let duration_ms = finished_at
            .signed_duration_since(started_at)
            .num_milliseconds()
            .max(0);
        self.conn.execute(
            "
            INSERT INTO automation_runs
            (job_type, trigger_type, status, message, refreshed, failed, duration_ms, started_at, finished_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            ",
            params![
                job_type,
                trigger_type,
                status,
                message,
                refreshed.max(0),
                failed.max(0),
                duration_ms,
                started_at.to_rfc3339(),
                finished_at.to_rfc3339()
            ],
        )?;
        Ok(())
    }

    fn scheduler_due(&self, key: &str, interval_minutes: i64, now: DateTime<Utc>) -> AppResult<bool> {
        if interval_minutes <= 0 {
            return Ok(false);
        }
        let Some(value) = self.get_config(key)? else {
            return Ok(true);
        };
        let Some(last_run) = parse_scheduler_timestamp(&value) else {
            return Ok(true);
        };
        Ok(now.signed_duration_since(last_run).num_minutes() >= interval_minutes)
    }

    fn encrypt_optional_secret(&self, value: &str) -> AppResult<String> {
        if value.is_empty() {
            return Ok(String::new());
        }
        let key = self.crypto_key.as_ref().ok_or(AppError::Unauthorized)?;
        crypto::encrypt_text(value, key)
    }

    fn decrypt_optional_secret(&self, value: &str) -> AppResult<String> {
        if value.is_empty() {
            return Ok(String::new());
        }
        let key = self.crypto_key.as_ref().ok_or(AppError::Unauthorized)?;
        crypto::decrypt_text(value, key)
    }

    fn upsert_temp_email_credential(&self, credential: &TempEmailCredential) -> AppResult<bool> {
        let provider = normalize_temp_provider(&credential.provider)?;
        let provider_token = self.encrypt_optional_secret(&credential.provider_token)?;
        let provider_password = self.encrypt_optional_secret(&credential.provider_password)?;
        let changed = self.conn.execute(
            "
            INSERT INTO temp_emails
            (email, provider, status, channel_id, provider_token_enc, provider_account_id, provider_password_enc)
            VALUES (?, ?, 'active', ?, ?, ?, ?)
            ON CONFLICT(email) DO UPDATE SET
                provider = excluded.provider,
                status = 'active',
                channel_id = excluded.channel_id,
                provider_token_enc = CASE
                    WHEN excluded.provider_token_enc = '' THEN temp_emails.provider_token_enc
                    ELSE excluded.provider_token_enc
                END,
                provider_account_id = CASE
                    WHEN excluded.provider_account_id = '' THEN temp_emails.provider_account_id
                    ELSE excluded.provider_account_id
                END,
                provider_password_enc = CASE
                    WHEN excluded.provider_password_enc = '' THEN temp_emails.provider_password_enc
                    ELSE excluded.provider_password_enc
                END,
                updated_at = CURRENT_TIMESTAMP
            ",
            params![
                credential.email,
                provider,
                credential.channel_id,
                provider_token,
                credential.provider_account_id,
                provider_password
            ],
        )?;
        Ok(changed > 0)
    }

    fn temp_email_credential(&self, email: &str) -> AppResult<TempEmailCredential> {
        let email = normalize_email(email)?;
        let row = self
            .conn
            .query_row(
                "
                SELECT id, email, provider, channel_id, provider_token_enc,
                       COALESCE(provider_account_id, ''), COALESCE(provider_password_enc, '')
                FROM temp_emails
                WHERE email = ?
                ",
                [email],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| AppError::InvalidInput("temp email not found".to_string()))?;
        Ok(TempEmailCredential {
            id: row.0,
            email: row.1,
            provider: row.2,
            channel_id: row.3,
            provider_token: self.decrypt_optional_secret(&row.4)?,
            provider_account_id: row.5,
            provider_password: self.decrypt_optional_secret(&row.6)?,
        })
    }

    fn upsert_temp_messages(&self, email: &str, messages: &[TempEmailMessage]) -> AppResult<usize> {
        let mut saved = 0_usize;
        for message in messages {
            let changed = self.conn.execute(
                "
                INSERT INTO temp_email_messages
                (message_id, email_address, from_address, subject, content, html_content,
                 has_html, timestamp, raw_content)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(message_id) DO UPDATE SET
                    email_address = excluded.email_address,
                    from_address = excluded.from_address,
                    subject = excluded.subject,
                    content = excluded.content,
                    html_content = excluded.html_content,
                    has_html = excluded.has_html,
                    timestamp = excluded.timestamp,
                    raw_content = excluded.raw_content
                ",
                params![
                    message.message_id,
                    email,
                    message.from_address,
                    message.subject,
                    message.content,
                    message.html_content,
                    if message.has_html { 1 } else { 0 },
                    message.timestamp,
                    message.raw_content
                ],
            )?;
            if changed > 0 {
                saved += 1;
            }
        }
        Ok(saved)
    }

    fn ensure_cloudflare_channel_exists(&self, channel_id: Option<i64>) -> AppResult<()> {
        if let Some(channel_id) = channel_id {
            let exists = self
                .conn
                .query_row("SELECT 1 FROM cloudflare_channels WHERE id = ?", [channel_id], |row| row.get::<_, i64>(0))
                .optional()?;
            if exists.is_none() {
                return Err(AppError::InvalidInput("Cloudflare channel not found".to_string()));
            }
        }
        Ok(())
    }

    fn cloudflare_channel_credential(&self, channel_id: Option<i64>) -> AppResult<CloudflareChannelCredential> {
        let sql = if channel_id.is_some() {
            "
            SELECT id, name, worker_domain, COALESCE(email_domains, ''),
                   admin_password_enc, enabled, is_default
            FROM cloudflare_channels
            WHERE id = ?
            "
        } else {
            "
            SELECT id, name, worker_domain, COALESCE(email_domains, ''),
                   admin_password_enc, enabled, is_default
            FROM cloudflare_channels
            WHERE is_default = 1
            ORDER BY id
            LIMIT 1
            "
        };
        let row = if let Some(channel_id) = channel_id {
            self.conn
                .query_row(sql, [channel_id], cloudflare_channel_row)
                .optional()?
        } else {
            self.conn.query_row(sql, [], cloudflare_channel_row).optional()?
        }
        .ok_or_else(|| AppError::InvalidInput("Cloudflare channel not found".to_string()))?;
        Ok(CloudflareChannelCredential {
            id: row.0,
            worker_domain: row.2,
            email_domains: parse_domain_list(&row.3),
            admin_password: self.decrypt_optional_secret(&row.4)?,
            enabled: row.5,
        })
    }

    fn mail_message_refs(&self, ids: &[i64]) -> AppResult<Vec<MailMessageRef>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = std::iter::repeat("?")
            .take(ids.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "
            SELECT m.id, m.account_id, a.email, m.folder, m.provider_message_id
            FROM retained_mail_messages m
            JOIN accounts a ON a.id = m.account_id
            WHERE m.id IN ({placeholders})
            "
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(ids.iter()), |row| {
            Ok(MailMessageRef {
                id: row.get(0)?,
                account_id: row.get(1)?,
                account_email: row.get(2)?,
                folder: row.get(3)?,
                provider_message_id: row.get(4)?,
            })
        })?;
        collect_rows(rows)
    }

    fn export_mail_message_rows(&self, ids: &[i64]) -> AppResult<Vec<ExportMailMessageRow>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = std::iter::repeat("?")
            .take(ids.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "
            SELECT m.id, m.account_id, a.email, m.folder, m.provider_message_id, m.subject,
                   m.sender, m.recipients, m.cc, m.received_at, m.is_read,
                   m.body_preview, m.body, m.body_type, m.attachments_json
            FROM retained_mail_messages m
            JOIN accounts a ON a.id = m.account_id
            WHERE m.id IN ({placeholders})
            ORDER BY m.received_at_sort DESC, m.id DESC
            "
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(ids.iter()), |row| {
            Ok(ExportMailMessageRow {
                id: row.get(0)?,
                account_id: row.get(1)?,
                account_email: row.get(2)?,
                folder: row.get(3)?,
                provider_message_id: row.get(4)?,
                subject: row.get(5)?,
                sender: row.get(6)?,
                recipients: row.get(7)?,
                cc: row.get(8)?,
                received_at: row.get(9)?,
                is_read: row.get::<_, i64>(10)? == 1,
                body_preview: row.get(11)?,
                body: row.get(12)?,
                body_type: row.get(13)?,
                attachments: parse_attachments_json(row.get::<_, String>(14)?.as_str()),
            })
        })?;
        collect_rows(rows)
    }

    fn write_export_file(&self, category: &str, file_name: &str, bytes: &[u8]) -> AppResult<(String, i64)> {
        let dir = exports_dir(&self.db_path)?.join(safe_file_name(category));
        std::fs::create_dir_all(&dir).map_err(|err| AppError::Internal(err.to_string()))?;
        let path = unique_path(&dir, &safe_file_name(file_name));
        std::fs::write(&path, bytes).map_err(|err| AppError::Internal(err.to_string()))?;
        Ok((path.to_string_lossy().to_string(), bytes.len() as i64))
    }

    fn sync_remote_mark_message(&self, target: &MailMessageRef, is_read: bool) -> AppResult<()> {
        let account = self
            .account_credentials(Some(target.account_id))?
            .into_iter()
            .next()
            .ok_or_else(|| AppError::InvalidInput("account not found".to_string()))?;
        if target.provider_message_id.starts_with("local-demo-") {
            return Ok(());
        }
        if should_use_graph(&account) {
            providers::mark_graph_message_read(&account, &target.provider_message_id, is_read)
        } else {
            providers::mark_imap_message_read(&account, &target.folder, &target.provider_message_id, is_read)
        }
    }

    fn refresh_account_credential(&self, account: &AccountCredentials, folder: &str, top: usize) -> AppResult<usize> {
        if should_use_graph(account) {
            providers::fetch_graph_messages(account, folder, top).and_then(|(next_refresh_token, messages)| {
                if !next_refresh_token.is_empty() && next_refresh_token != account.refresh_token {
                    self.save_refresh_token(account.id, &next_refresh_token)?;
                }
                self.upsert_provider_messages(account.id, &messages)?;
                Ok(messages.len())
            })
        } else {
            providers::fetch_imap_messages(account, folder, top).and_then(|messages| {
                self.upsert_provider_messages(account.id, &messages)?;
                Ok(messages.len())
            })
        }
    }

    fn retry_remote_mark_message(&self, payload: &MailRetryPayload, is_read: bool) -> AppResult<()> {
        let target = MailMessageRef {
            id: 0,
            account_id: payload.account_id,
            account_email: payload.account_email.clone(),
            folder: payload.folder.clone(),
            provider_message_id: payload.provider_message_id.clone(),
        };
        self.sync_remote_mark_message(&target, is_read)
    }

    fn sync_remote_delete_message(&self, target: &MailMessageRef) -> AppResult<()> {
        let account = self
            .account_credentials(Some(target.account_id))?
            .into_iter()
            .next()
            .ok_or_else(|| AppError::InvalidInput("account not found".to_string()))?;
        if target.provider_message_id.starts_with("local-demo-") {
            return Ok(());
        }
        if should_use_graph(&account) {
            providers::delete_graph_message(&account, &target.provider_message_id)
        } else {
            providers::delete_imap_message(&account, &target.folder, &target.provider_message_id)
        }
    }

    fn retry_remote_delete_message(&self, payload: &MailRetryPayload) -> AppResult<()> {
        let target = MailMessageRef {
            id: 0,
            account_id: payload.account_id,
            account_email: payload.account_email.clone(),
            folder: payload.folder.clone(),
            provider_message_id: payload.provider_message_id.clone(),
        };
        self.sync_remote_delete_message(&target)?;
        self.delete_cached_mail_message(&target)?;
        Ok(())
    }

    fn retry_forward_message(&self, payload: &ForwardRetryPayload) -> AppResult<()> {
        let settings = self.get_settings()?;
        let circuit = self.forwarding_channel_circuit(&payload.channel, &settings)?;
        if circuit.status == "open" {
            return Err(AppError::Internal(forwarding_circuit_error(&circuit)));
        }
        if self.forward_success_exists(payload.account_id, &payload.message_id, &payload.channel)? {
            return Ok(());
        }
        let message = self.forwarding_retry_content(payload.account_id, &payload.message_id)?;
        let proxy_chain = self.proxy_chain_for_account(payload.account_id)?;
        automation::forward_message(&settings, &payload.channel, &message, &proxy_chain)?;
        self.insert_forwarding_log(
            Some(payload.account_id),
            &message.account_email,
            &payload.message_id,
            &payload.channel,
            "success",
            None,
        )?;
        Ok(())
    }

    fn retry_refresh_account(&self, payload: &RefreshRetryPayload) -> AppResult<()> {
        let account = self
            .account_credentials(Some(payload.account_id))?
            .into_iter()
            .next()
            .ok_or_else(|| AppError::InvalidInput("account not found".to_string()))?;
        let folder = normalize_mail_folder(&payload.folder);
        let top = payload.top.clamp(1, 50);
        match self.refresh_account_credential(&account, &folder, top) {
            Ok(count) => {
                self.mark_account_refresh_success(account.id, &account.email, count)?;
                self.clear_refresh_retry(account.id, &folder)?;
                Ok(())
            }
            Err(err) => {
                let message = err.to_string();
                self.mark_account_refresh_failed(account.id, &account.email, &message)?;
                Err(err)
            }
        }
    }

    fn retry_backup_job(&self, payload: &BackupRetryPayload) -> AppResult<()> {
        let result = self.run_backup_job_inner();
        if result.is_ok() {
            self.clear_backup_retry(payload.target.as_str())?;
        }
        result.map(|_| ())
    }

    fn retry_temp_refresh(&self, payload: &TempRefreshRetryPayload) -> AppResult<()> {
        let credential = self.temp_email_credential(&payload.email)?;
        if credential.provider != payload.provider {
            return Err(AppError::InvalidInput(format!(
                "temp email provider changed from {} to {}",
                payload.provider, credential.provider
            )));
        }
        match self.refresh_temp_email_credential(&credential) {
            Ok(_) => Ok(()),
            Err(err) => {
                let message = err.to_string();
                self.mark_temp_email_refresh_failed(&credential, &message)?;
                Err(err)
            }
        }
    }

    fn refresh_temp_email_credential(&self, credential: &TempEmailCredential) -> AppResult<JobResult> {
        let settings = self.get_settings()?;
        let channel = if credential.provider == "cloudflare" {
            Some(self.cloudflare_channel_credential(credential.channel_id)?)
        } else {
            None
        };
        let messages = providers::fetch_temp_messages(&settings, credential, channel.as_ref(), 50)?;
        let saved = self.upsert_temp_messages(&credential.email, &messages)?;
        self.conn.execute(
            "
            UPDATE temp_emails
            SET last_refresh_at = CURRENT_TIMESTAMP,
                last_refresh_status = 'success',
                last_refresh_error = NULL,
                updated_at = CURRENT_TIMESTAMP
            WHERE id = ?
            ",
            [credential.id],
        )?;
        self.clear_temp_refresh_retry(&credential.email)?;
        self.audit("temp_email.refreshed", "temp_email", Some(credential.id), &credential.email)?;
        Ok(JobResult {
            success: true,
            message: format!("Refreshed {} temp message(s)", messages.len()),
            refreshed: saved,
            failed: 0,
        })
    }

    fn mark_temp_email_refresh_failed(&self, credential: &TempEmailCredential, error: &str) -> AppResult<()> {
        self.conn.execute(
            "
            UPDATE temp_emails
            SET last_refresh_at = CURRENT_TIMESTAMP,
                last_refresh_status = 'failed',
                last_refresh_error = ?,
                updated_at = CURRENT_TIMESTAMP
            WHERE id = ?
            ",
            params![error, credential.id],
        )?;
        Ok(())
    }

    fn clear_refresh_retry(&self, account_id: i64, folder: &str) -> AppResult<()> {
        self.conn.execute(
            "
            DELETE FROM retry_queue
            WHERE task_type = 'refresh_account'
              AND account_id = ?
              AND message_id = ?
              AND action = 'refresh'
            ",
            params![account_id, folder],
        )?;
        Ok(())
    }

    fn clear_mail_delete_retry(&self, target: &MailMessageRef) -> AppResult<()> {
        self.conn.execute(
            "
            DELETE FROM retry_queue
            WHERE task_type = 'mail_delete'
              AND account_id = ?
              AND message_id = ?
              AND (channel = '' OR channel = ?)
              AND action = 'delete'
            ",
            params![target.account_id, target.provider_message_id, target.folder],
        )?;
        Ok(())
    }

    fn delete_cached_mail_message(&self, target: &MailMessageRef) -> AppResult<()> {
        self.conn.execute(
            "
            DELETE FROM retained_mail_messages
            WHERE account_id = ?
              AND folder = ?
              AND provider_message_id = ?
            ",
            params![target.account_id, target.folder, target.provider_message_id],
        )?;
        Ok(())
    }

    fn clear_backup_retry(&self, target: &str) -> AppResult<()> {
        let message_id = if target.trim().is_empty() { "webdav" } else { target.trim() };
        self.conn.execute(
            "
            DELETE FROM retry_queue
            WHERE task_type = 'backup_job'
              AND message_id = ?
              AND action = 'backup'
            ",
            [message_id],
        )?;
        Ok(())
    }

    fn clear_temp_refresh_retry(&self, email: &str) -> AppResult<()> {
        self.conn.execute(
            "
            DELETE FROM retry_queue
            WHERE task_type = 'temp_refresh'
              AND message_id = ?
              AND action = 'refresh'
            ",
            [email],
        )?;
        Ok(())
    }

    fn forwarding_retry_content(&self, account_id: i64, message_id: &str) -> AppResult<ForwardContent> {
        self.conn
            .query_row(
                "
                SELECT a.email, m.provider_message_id, m.subject, m.sender, m.received_at,
                       m.body_preview, m.body
                FROM retained_mail_messages m
                JOIN accounts a ON a.id = m.account_id
                WHERE m.account_id = ? AND m.provider_message_id = ?
                LIMIT 1
                ",
                params![account_id, message_id],
                |row| {
                    Ok(ForwardContent {
                        account_email: row.get(0)?,
                        message_id: row.get(1)?,
                        subject: row.get(2)?,
                        sender: row.get(3)?,
                        received_at: row.get(4)?,
                        body_preview: row.get(5)?,
                        body: row.get(6)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| AppError::InvalidInput("forwarding retry message not found".to_string()))
    }

    fn audit(&self, action: &str, resource_type: &str, resource_id: Option<i64>, detail: &str) -> AppResult<()> {
        self.conn.execute(
            "
            INSERT INTO audit_logs (action, resource_type, resource_id, detail)
            VALUES (?, ?, ?, ?)
            ",
            params![action, resource_type, resource_id.map(|id| id.to_string()), detail],
        )?;
        Ok(())
    }
}

fn collect_rows<T, F>(rows: rusqlite::MappedRows<'_, F>) -> AppResult<Vec<T>>
where
    F: FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
{
    let mut values = Vec::new();
    for row in rows {
        values.push(row?);
    }
    Ok(values)
}

fn resolve_db_path() -> PathBuf {
    if let Some(project_dirs) = ProjectDirs::from("com", "outlookemail", "OutlookEmailDesktop") {
        return project_dirs.data_local_dir().join("outlook-email.sqlite");
    }
    PathBuf::from("outlook-email.sqlite")
}

fn validate_password(password: &str) -> AppResult<()> {
    if password.len() < 8 {
        return Err(AppError::InvalidInput(
            "local app password must be at least 8 characters".to_string(),
        ));
    }
    Ok(())
}

fn normalize_message_ids(ids: &[i64]) -> AppResult<Vec<i64>> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for id in ids {
        if *id > 0 && seen.insert(*id) {
            normalized.push(*id);
        }
    }
    if normalized.is_empty() {
        return Err(AppError::InvalidInput("select at least one message".to_string()));
    }
    Ok(normalized)
}

fn normalize_mail_folder(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "inbox" => "inbox".to_string(),
        "junk" | "junkemail" => "junkemail".to_string(),
        "deleted" | "deleteditems" => "deleteditems".to_string(),
        _ => "all".to_string(),
    }
}

fn normalize_oauth_provider(value: Option<&str>) -> AppResult<Option<String>> {
    let provider = value.unwrap_or_default().trim().to_ascii_lowercase();
    if provider.is_empty() {
        return Ok(None);
    }
    match provider.as_str() {
        "graph" | "outlook" | "imap" => Ok(Some(provider)),
        value => Err(AppError::InvalidInput(format!("unsupported OAuth provider: {value}"))),
    }
}

fn normalize_read_state(value: Option<&str>) -> AppResult<String> {
    match value.unwrap_or("all").trim().to_ascii_lowercase().as_str() {
        "" | "all" => Ok("all".to_string()),
        "read" => Ok("read".to_string()),
        "unread" => Ok("unread".to_string()),
        _ => Err(AppError::InvalidInput("read_state must be all, read, or unread".to_string())),
    }
}

fn normalize_automation_value(value: Option<&str>, allowed: &[&str], field: &str) -> AppResult<String> {
    let normalized = value.unwrap_or("all").trim().to_ascii_lowercase();
    if normalized.is_empty() || normalized == "all" {
        return Ok(String::new());
    }
    if allowed.iter().any(|item| *item == normalized) {
        return Ok(normalized);
    }
    Err(AppError::InvalidInput(format!(
        "{field} must be one of: all, {}",
        allowed.join(", ")
    )))
}

fn normalize_retry_value(value: Option<&str>, allowed: &[&str], field: &str) -> AppResult<String> {
    let normalized = value.unwrap_or("all").trim().to_ascii_lowercase();
    if normalized.is_empty() || normalized == "all" {
        return Ok(String::new());
    }
    if allowed.iter().any(|item| *item == normalized) {
        return Ok(normalized);
    }
    Err(AppError::InvalidInput(format!(
        "{field} must be one of: all, {}",
        allowed.join(", ")
    )))
}

fn normalize_theme_setting(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "default" | "graphite" | "ocean" | "forest" | "rose" => value.trim().to_ascii_lowercase(),
        _ => "default".to_string(),
    }
}

fn normalize_accent_color(value: &str) -> String {
    let trimmed = value.trim();
    let Some(hex) = trimmed.strip_prefix('#') else {
        return "#2563eb".to_string();
    };
    if hex.len() == 6 && hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
        format!("#{hex}")
    } else {
        "#2563eb".to_string()
    }
}

fn automation_job_summary(runs: &[AutomationRun], job_type: &str) -> AutomationJobSummary {
    let matching = runs
        .iter()
        .filter(|run| run.job_type == job_type)
        .collect::<Vec<_>>();
    let total = matching.len() as i64;
    let success = matching.iter().filter(|run| run.status == "success").count() as i64;
    let failed = matching.iter().filter(|run| run.status == "failed").count() as i64;
    let scheduled = matching.iter().filter(|run| run.trigger_type == "schedule").count() as i64;
    let manual = matching.iter().filter(|run| run.trigger_type == "manual").count() as i64;
    let average_duration_ms = average_i64(matching.iter().map(|run| run.duration_ms), total);
    let latest = matching.first();
    AutomationJobSummary {
        job_type: job_type.to_string(),
        total,
        success,
        failed,
        scheduled,
        manual,
        average_duration_ms,
        last_finished_at: latest.map(|run| run.finished_at.clone()),
        latest_message: latest.map(|run| run.message.clone()).unwrap_or_default(),
    }
}

fn retry_task_summary(items: &[RetryQueueItem], task_type: &str) -> RetryTaskSummary {
    let matching = items
        .iter()
        .filter(|item| item.task_type == task_type)
        .collect::<Vec<_>>();
    let pending = matching.iter().filter(|item| item.status == "pending").count() as i64;
    let failed = matching.iter().filter(|item| item.status == "failed").count() as i64;
    let due = matching
        .iter()
        .filter(|item| item.status == "pending" && item.due_now)
        .count() as i64;
    let exhausted = matching
        .iter()
        .filter(|item| item.status == "failed" || item.attempts >= item.max_attempts)
        .count() as i64;
    let next_attempt_at = matching
        .iter()
        .filter_map(|item| item.next_attempt_at.as_ref())
        .min_by(|left, right| compare_timestamps(left, right))
        .cloned();
    let last_error = matching
        .iter()
        .find(|item| !item.error_message.trim().is_empty())
        .map(|item| item.error_message.clone())
        .unwrap_or_default();
    RetryTaskSummary {
        task_type: task_type.to_string(),
        pending,
        failed,
        due,
        exhausted,
        next_attempt_at,
        last_error,
    }
}

fn average_i64<I>(values: I, count: i64) -> i64
where
    I: Iterator<Item = i64>,
{
    if count <= 0 {
        return 0;
    }
    values.sum::<i64>() / count
}

fn push_error_bucket(
    buckets: &mut Vec<AutomationErrorBucket>,
    category: &str,
    message: &str,
    at: Option<&str>,
    count: i64,
) {
    if category == "none" || message.trim().is_empty() || count <= 0 {
        return;
    }
    if let Some(bucket) = buckets.iter_mut().find(|bucket| bucket.category == category) {
        bucket.count += count;
        if at.is_some_and(|value| bucket.latest_at.as_deref().is_none_or(|current| timestamp_is_newer(value, current))) {
            bucket.latest_message = message.to_string();
            bucket.latest_at = at.map(str::to_string);
        }
    } else {
        buckets.push(AutomationErrorBucket {
            category: category.to_string(),
            count,
            latest_message: message.to_string(),
            latest_at: at.map(str::to_string),
        });
    }
}

fn latest_failure_detail(
    log_failure: Option<(String, String)>,
    retry_failure: Option<(String, String)>,
) -> (String, Option<String>) {
    match (log_failure, retry_failure) {
        (Some((log_error, log_at)), Some((retry_error, retry_at))) => {
            if timestamp_is_newer(&retry_at, &log_at) {
                (retry_error, Some(retry_at))
            } else {
                (log_error, Some(log_at))
            }
        }
        (Some((error, at)), None) | (None, Some((error, at))) => (error, Some(at)),
        (None, None) => (String::new(), None),
    }
}

fn compare_timestamps(left: &str, right: &str) -> std::cmp::Ordering {
    match (parse_scheduler_timestamp(left), parse_scheduler_timestamp(right)) {
        (Some(left), Some(right)) => left.cmp(&right),
        _ => left.cmp(right),
    }
}

fn timestamp_is_newer(value: &str, current: &str) -> bool {
    compare_timestamps(value, current).is_gt()
}

fn sqlite_timestamp(value: DateTime<Utc>) -> String {
    value.format("%Y-%m-%d %H:%M:%S").to_string()
}

fn classify_error_category(error: &str) -> &'static str {
    let lower = error.to_ascii_lowercase();
    if lower.trim().is_empty() {
        return "none";
    }
    if lower.contains("auth")
        || lower.contains("unauthorized")
        || lower.contains("forbidden")
        || lower.contains("invalid_grant")
        || lower.contains("credential")
        || lower.contains("password")
        || lower.contains("token")
        || lower.contains("login")
    {
        return "auth";
    }
    if lower.contains("429")
        || lower.contains("rate limit")
        || lower.contains("too many requests")
        || lower.contains("throttle")
    {
        return "rate_limit";
    }
    if lower.contains("timeout")
        || lower.contains("timed out")
        || lower.contains("connection")
        || lower.contains("connect")
        || lower.contains("dns")
        || lower.contains("network")
        || lower.contains("proxy")
        || lower.contains("tls")
        || lower.contains("refused")
    {
        return "network";
    }
    if lower.contains("configure")
        || lower.contains("required")
        || lower.contains("missing")
        || lower.contains("unsupported")
    {
        return "config";
    }
    if lower.contains("sqlite")
        || lower.contains("database")
        || lower.contains("disk")
        || lower.contains("permission")
        || lower.contains("backup")
    {
        return "storage";
    }
    if lower.contains("parse")
        || lower.contains("invalid")
        || lower.contains("not found")
        || lower.contains("deserialize")
    {
        return "data";
    }
    if lower.contains("imap")
        || lower.contains("smtp")
        || lower.contains("telegram")
        || lower.contains("wecom")
        || lower.contains("webdav")
        || lower.contains("graph")
        || lower.contains("cloudflare")
        || lower.contains("gptmail")
        || lower.contains("duckmail")
        || lower.contains("http 5")
    {
        return "provider";
    }
    "unknown"
}

fn retry_delay_minutes(attempts: i64) -> i64 {
    match attempts {
        0 | 1 => 5,
        2 => 15,
        3 => 60,
        _ => 360,
    }
}

fn retry_delay_minutes_for_error(task_type: &str, attempts: i64, error: &str) -> i64 {
    let category = classify_error_category(error);
    match (task_type, category, attempts) {
        ("forward_message", "rate_limit", 0 | 1) => 15,
        ("forward_message", "rate_limit", 2) => 30,
        ("forward_message", "rate_limit", 3) => 90,
        ("forward_message", "auth" | "config", 0 | 1) => 60,
        ("forward_message", "auth" | "config", 2) => 240,
        ("forward_message", "network" | "provider", 0 | 1) => 10,
        ("forward_message", "network" | "provider", 2) => 30,
        (_, "rate_limit", 0 | 1) => 15,
        (_, "rate_limit", 2) => 60,
        (_, "auth" | "config", 0 | 1) => 30,
        (_, "auth" | "config", 2) => 180,
        (_, "storage", 0 | 1) => 10,
        _ => retry_delay_minutes(attempts),
    }
}

fn retry_max_attempts_for_error(task_type: &str, error: &str) -> i64 {
    let category = classify_error_category(error);
    match (task_type, category) {
        (_, "auth" | "config") => 3,
        (_, "rate_limit") => 8,
        ("forward_message", "network" | "provider") => 7,
        (_, "network") => 6,
        _ => 5,
    }
}

fn retry_due_now(status: &str, next_attempt_at: Option<&str>) -> bool {
    if status != "pending" {
        return false;
    }
    next_attempt_at
        .and_then(parse_scheduler_timestamp)
        .is_none_or(|value| value <= Utc::now())
}

fn retry_next_delay_minutes(next_attempt_at: Option<&str>) -> i64 {
    next_attempt_at
        .and_then(parse_scheduler_timestamp)
        .map(|value| value.signed_duration_since(Utc::now()).num_minutes().max(0))
        .unwrap_or(0)
}

fn share_token_hash(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn share_record_status(expires_at: Option<&str>, revoked_at: Option<&str>) -> &'static str {
    if revoked_at.is_some() {
        return "revoked";
    }
    if expires_at
        .and_then(parse_scheduler_timestamp)
        .is_some_and(|value| value <= Utc::now())
    {
        return "expired";
    }
    "active"
}

fn forwarding_circuit_error(circuit: &ForwardingChannelCircuit) -> String {
    match circuit.open_until.as_deref() {
        Some(open_until) => format!(
            "forwarding channel circuit open for {} until {} after {} recent failure(s)",
            circuit.channel, open_until, circuit.recent_failures
        ),
        None => format!(
            "forwarding channel circuit open for {} after {} recent failure(s)",
            circuit.channel, circuit.recent_failures
        ),
    }
}

fn parse_retry_payload<T: serde::de::DeserializeOwned>(value: &str) -> AppResult<T> {
    serde_json::from_str(value)
        .map_err(|err| AppError::InvalidInput(format!("invalid retry payload: {err}")))
}

fn retry_job_message(completed: usize, failed: usize, errors: &[String]) -> String {
    if completed == 0 && failed == 0 {
        return "No retry item(s) due".to_string();
    }
    if failed == 0 {
        return format!("Retried {} item(s)", completed);
    }
    let preview = errors.iter().take(3).cloned().collect::<Vec<_>>().join("; ");
    if errors.len() > 3 {
        format!("Retried {completed} item(s), {failed} failed: {preview}; ...")
    } else {
        format!("Retried {completed} item(s), {failed} failed: {preview}")
    }
}

fn mail_action_message(action: &str, changed: usize, failed: usize, errors: &[String]) -> String {
    if failed == 0 {
        return format!("{action} {changed} message(s)");
    }
    let preview = errors.iter().take(3).cloned().collect::<Vec<_>>().join("; ");
    if errors.len() > 3 {
        format!("{action} {changed} local message(s), {failed} remote sync failed: {preview}; ...")
    } else {
        format!("{action} {changed} local message(s), {failed} remote sync failed: {preview}")
    }
}

fn timestamped_file_name(prefix: &str, extension: &str) -> String {
    let timestamp = Utc::now().format("%Y%m%d-%H%M%S");
    let prefix = safe_file_name(prefix);
    let extension = extension.trim_start_matches('.');
    format!("{prefix}-{timestamp}.{extension}")
}

fn csv_row<T: AsRef<str>>(fields: &[T]) -> String {
    let mut row = fields
        .iter()
        .map(|field| csv_escape(field.as_ref()))
        .collect::<Vec<_>>()
        .join(",");
    row.push('\n');
    row
}

fn csv_escape(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn html_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn render_mail_export_html(title: &str, rows: &[ExportMailMessageRow]) -> String {
    let mut html = String::new();
    html.push_str("<!doctype html><html><head><meta charset=\"utf-8\"><title>");
    html.push_str(&html_escape(title));
    html.push_str("</title><style>");
    html.push_str(
        "body{font-family:Segoe UI,Arial,sans-serif;margin:24px;background:#f8fafc;color:#17202a}\
         h1{font-size:24px;margin:0 0 8px}\
         .summary{color:#64748b;margin-bottom:18px}\
         article{background:#fff;border:1px solid #e2e8f0;border-radius:8px;margin:14px 0;padding:18px}\
         h2{font-size:18px;margin:0 0 10px}\
         .meta{display:grid;grid-template-columns:120px 1fr;gap:6px 12px;color:#64748b;font-size:13px;margin-bottom:14px}\
         .meta strong{color:#17202a}\
         pre{white-space:pre-wrap;line-height:1.55;font-family:inherit}\
         ul{margin:8px 0 0 18px;padding:0}",
    );
    html.push_str("</style></head><body><h1>");
    html.push_str(&html_escape(title));
    html.push_str("</h1><div class=\"summary\">Exported ");
    html.push_str(&rows.len().to_string());
    html.push_str(" message(s) at ");
    html.push_str(&html_escape(&Utc::now().to_rfc3339()));
    html.push_str("</div>");
    for row in rows {
        html.push_str("<article><h2>");
        html.push_str(&html_escape(if row.subject.is_empty() { "(no subject)" } else { &row.subject }));
        html.push_str("</h2><div class=\"meta\">");
        for (label, value) in [
            ("Local ID", row.id.to_string()),
            ("Account", row.account_email.clone()),
            ("Folder", row.folder.clone()),
            ("Provider ID", row.provider_message_id.clone()),
            ("From", row.sender.clone()),
            ("To", row.recipients.clone()),
            ("Cc", row.cc.clone()),
            ("Received", row.received_at.clone()),
            ("Status", if row.is_read { "Read" } else { "Unread" }.to_string()),
            ("Body type", row.body_type.clone()),
        ] {
            html.push_str("<span>");
            html.push_str(&html_escape(label));
            html.push_str("</span><strong>");
            html.push_str(&html_escape(&value));
            html.push_str("</strong>");
        }
        html.push_str("</div><pre>");
        html.push_str(&html_escape(row.body.as_deref().unwrap_or(&row.body_preview)));
        html.push_str("</pre>");
        if !row.attachments.is_empty() {
            html.push_str("<h3>Attachments</h3><ul>");
            for attachment in &row.attachments {
                html.push_str("<li>");
                html.push_str(&html_escape(&format!(
                    "{} ({}, {} bytes)",
                    attachment.name, attachment.content_type, attachment.size
                )));
                html.push_str("</li>");
            }
            html.push_str("</ul>");
        }
        html.push_str("</article>");
    }
    html.push_str("</body></html>");
    html
}

fn normalize_email(value: &str) -> AppResult<String> {
    let email = value.trim().to_ascii_lowercase();
    if !email.contains('@') {
        return Err(AppError::InvalidInput("email is invalid".to_string()));
    }
    Ok(email)
}

fn normalize_proxy_option(value: Option<&str>) -> AppResult<Option<String>> {
    value.map(|item| normalize_proxy_value(Some(item))).transpose()
}

fn normalize_proxy_value(value: Option<&str>) -> AppResult<String> {
    let Some(value) = value else {
        return Ok(String::new());
    };
    let proxy = value.trim();
    if proxy.is_empty() {
        return Ok(String::new());
    }
    let lower = proxy.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return Err(AppError::InvalidInput(
            "proxy URL must start with http:// or https://".to_string(),
        ));
    }
    Ok(proxy.to_string())
}

fn proxy_chain_from_values(values: &[&str]) -> AppResult<Vec<String>> {
    let mut chain = Vec::new();
    let mut seen = HashSet::new();
    for value in values {
        let proxy = normalize_proxy_value(Some(value))?;
        if proxy.is_empty() || !seen.insert(proxy.to_ascii_lowercase()) {
            continue;
        }
        chain.push(proxy);
    }
    Ok(chain)
}

fn normalize_temp_provider(value: &str) -> AppResult<String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "gptmail" => Ok("gptmail".to_string()),
        "duckmail" => Ok("duckmail".to_string()),
        "cloudflare" => Ok("cloudflare".to_string()),
        _ => Err(AppError::InvalidInput("temp email provider must be gptmail, duckmail, or cloudflare".to_string())),
    }
}

fn normalize_temp_tags(tags: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::new();
    let mut seen = HashSet::new();
    for tag in tags {
        let tag = tag.trim();
        if tag.is_empty() {
            continue;
        }
        let key = tag.to_ascii_lowercase();
        if seen.insert(key) {
            normalized.push(tag.chars().take(32).collect());
        }
        if normalized.len() >= 20 {
            break;
        }
    }
    normalized
}

fn temp_tags_from_json(value: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(value)
        .map(normalize_temp_tags)
        .unwrap_or_default()
}

fn random_temp_suffix(index: usize) -> String {
    let value = uuid::Uuid::new_v4().simple().to_string();
    format!("{index}{}", &value[..8])
}

fn split_legacy_line(value: &str) -> Vec<String> {
    if value.contains("----") {
        value.split("----").map(|part| part.trim().to_string()).collect()
    } else {
        value.split(',').map(|part| part.trim().to_string()).collect()
    }
}

fn parse_domain_list(value: &str) -> Vec<String> {
    value
        .split([',', '\n', ';'])
        .map(|item| item.trim().trim_start_matches('@').trim_end_matches('.').to_ascii_lowercase())
        .filter(|item| !item.is_empty())
        .fold(Vec::new(), |mut domains, item| {
            if !domains.contains(&item) {
                domains.push(item);
            }
            domains
        })
}

fn serialize_domain_list(values: &[String]) -> String {
    parse_domain_list(&values.join(",")).join(", ")
}

fn temp_message_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TempEmailMessage> {
    Ok(TempEmailMessage {
        id: row.get(0)?,
        message_id: row.get(1)?,
        email_address: row.get(2)?,
        from_address: row.get(3)?,
        subject: row.get(4)?,
        content: row.get(5)?,
        html_content: row.get(6)?,
        has_html: row.get::<_, i64>(7)? == 1,
        timestamp: row.get(8)?,
        raw_content: row.get(9)?,
        created_at: row.get(10)?,
    })
}

fn cloudflare_channel_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<(i64, String, String, String, String, bool, bool)> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get::<_, i64>(5)? == 1,
        row.get::<_, i64>(6)? == 1,
    ))
}

fn parse_attachments_json(value: &str) -> Vec<AttachmentInfo> {
    serde_json::from_str(value).unwrap_or_default()
}

#[derive(Default)]
struct ParsedMailSearch {
    terms: Vec<MailSearchTerm>,
    read_state: Option<String>,
    has_attachments: Option<bool>,
    folder: Option<String>,
}

enum MailSearchTerm {
    Any(String),
    Subject(String),
    Sender(String),
    Recipient(String),
    Body(String),
    ProviderId(String),
}

fn parse_mail_search(value: &str) -> ParsedMailSearch {
    let mut search = ParsedMailSearch::default();
    for token in tokenize_search(value) {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        let Some((key, raw_value)) = token.split_once(':') else {
            search.terms.push(MailSearchTerm::Any(token.to_string()));
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let raw_value = raw_value.trim();
        if key.is_empty() || raw_value.is_empty() {
            search.terms.push(MailSearchTerm::Any(token.to_string()));
            continue;
        }
        match key.as_str() {
            "subject" | "sub" => search.terms.push(MailSearchTerm::Subject(raw_value.to_string())),
            "from" | "sender" => search.terms.push(MailSearchTerm::Sender(raw_value.to_string())),
            "to" | "recipient" | "recipients" | "cc" => search.terms.push(MailSearchTerm::Recipient(raw_value.to_string())),
            "body" | "content" | "text" => search.terms.push(MailSearchTerm::Body(raw_value.to_string())),
            "id" | "message" | "message_id" => search.terms.push(MailSearchTerm::ProviderId(raw_value.to_string())),
            "folder" | "mailbox" => search.folder = Some(normalize_mail_folder(raw_value)),
            "is" | "status" => match raw_value.to_ascii_lowercase().as_str() {
                "read" => search.read_state = Some("read".to_string()),
                "unread" => search.read_state = Some("unread".to_string()),
                _ => search.terms.push(MailSearchTerm::Any(token.to_string())),
            },
            "has" => match raw_value.to_ascii_lowercase().as_str() {
                "attachment" | "attachments" | "file" | "files" => search.has_attachments = Some(true),
                "noattachment" | "noattachments" | "nofile" | "nofiles" => search.has_attachments = Some(false),
                _ => search.terms.push(MailSearchTerm::Any(token.to_string())),
            },
            _ => search.terms.push(MailSearchTerm::Any(token.to_string())),
        }
    }
    search
}

fn tokenize_search(value: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut token = String::new();
    let mut in_quote = false;
    for ch in value.chars() {
        match ch {
            '"' => in_quote = !in_quote,
            ch if ch.is_whitespace() && !in_quote => {
                if !token.trim().is_empty() {
                    tokens.push(token.trim().to_string());
                    token.clear();
                }
            }
            ch => token.push(ch),
        }
    }
    if !token.trim().is_empty() {
        tokens.push(token.trim().to_string());
    }
    tokens
}

fn append_mail_search_terms(sql: &mut String, values: &mut Vec<SqlValue>, terms: &[MailSearchTerm]) {
    for term in terms {
        match term {
            MailSearchTerm::Any(value) => {
                sql.push_str(" AND (m.subject LIKE ? OR m.sender LIKE ? OR m.recipients LIKE ? OR m.cc LIKE ? OR m.body_preview LIKE ? OR COALESCE(m.body, '') LIKE ?)");
                push_like_values(values, value, 6);
            }
            MailSearchTerm::Subject(value) => {
                sql.push_str(" AND m.subject LIKE ?");
                push_like_values(values, value, 1);
            }
            MailSearchTerm::Sender(value) => {
                sql.push_str(" AND m.sender LIKE ?");
                push_like_values(values, value, 1);
            }
            MailSearchTerm::Recipient(value) => {
                sql.push_str(" AND (m.recipients LIKE ? OR m.cc LIKE ?)");
                push_like_values(values, value, 2);
            }
            MailSearchTerm::Body(value) => {
                sql.push_str(" AND (m.body_preview LIKE ? OR COALESCE(m.body, '') LIKE ?)");
                push_like_values(values, value, 2);
            }
            MailSearchTerm::ProviderId(value) => {
                sql.push_str(" AND m.provider_message_id LIKE ?");
                push_like_values(values, value, 1);
            }
        }
    }
}

fn push_like_values(values: &mut Vec<SqlValue>, value: &str, count: usize) {
    let pattern = format!("%{}%", value);
    for _ in 0..count {
        values.push(SqlValue::Text(pattern.clone()));
    }
}

fn repeat_placeholders(count: usize) -> String {
    std::iter::repeat("?")
        .take(count)
        .collect::<Vec<_>>()
        .join(",")
}

fn mail_sort_clause(sort_by: Option<&str>, sort_order: Option<&str>) -> AppResult<String> {
    let order = match sort_order.unwrap_or("desc").trim().to_ascii_lowercase().as_str() {
        "" | "desc" => "DESC",
        "asc" => "ASC",
        value => return Err(AppError::InvalidInput(format!("unsupported mail sort_order: {value}"))),
    };
    let clause = match sort_by.unwrap_or("date").trim().to_ascii_lowercase().as_str() {
        "" | "date" | "received" | "received_at" => format!("m.received_at_sort {order}, m.id {order}"),
        "subject" => format!("LOWER(m.subject) {order}, m.received_at_sort DESC, m.id DESC"),
        "sender" | "from" => format!("LOWER(m.sender) {order}, m.received_at_sort DESC, m.id DESC"),
        "read" | "status" => format!("m.is_read {order}, m.received_at_sort DESC, m.id DESC"),
        "attachments" | "files" => format!("m.has_attachments {order}, m.received_at_sort DESC, m.id DESC"),
        "folder" => format!("m.folder {order}, m.received_at_sort DESC, m.id DESC"),
        value => return Err(AppError::InvalidInput(format!("unsupported mail sort_by: {value}"))),
    };
    Ok(clause)
}

fn preview_secret(value: &str) -> String {
    if value.len() <= 10 {
        return "*".repeat(value.len());
    }
    format!("{}...{}", &value[..4], &value[value.len() - 4..])
}

fn should_use_graph(account: &AccountCredentials) -> bool {
    let provider = account.provider.to_ascii_lowercase();
    let account_type = account.account_type.to_ascii_lowercase();
    if provider == "imap" || account_type == "imap" {
        return false;
    }
    provider == "graph"
        || provider == "outlook"
        || account_type == "outlook"
        || (!account.client_id.is_empty() && !account.refresh_token.is_empty())
}

fn table_columns(conn: &Connection, table: &str) -> AppResult<Vec<String>> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    collect_rows(rows)
}

fn attachment_dir(db_path: &Path) -> AppResult<PathBuf> {
    Ok(db_path
        .parent()
        .ok_or_else(|| AppError::Internal("database path has no parent directory".to_string()))?
        .join("attachments"))
}

fn backup_dir(db_path: &Path) -> AppResult<PathBuf> {
    Ok(db_path
        .parent()
        .ok_or_else(|| AppError::Internal("database path has no parent directory".to_string()))?
        .join("backups"))
}

fn file_set_size(paths: &[PathBuf]) -> AppResult<i64> {
    let mut size = 0_i64;
    for path in paths {
        if path.exists() {
            size += path
                .metadata()
                .map_err(|err| AppError::Internal(err.to_string()))?
                .len() as i64;
        }
    }
    Ok(size)
}

fn dir_stats(dir: &Path) -> AppResult<(usize, i64)> {
    if !dir.exists() {
        return Ok((0, 0));
    }
    let mut files = 0_usize;
    let mut bytes = 0_i64;
    collect_dir_stats(dir, &mut files, &mut bytes)?;
    Ok((files, bytes))
}

fn collect_dir_stats(dir: &Path, files: &mut usize, bytes: &mut i64) -> AppResult<()> {
    for entry in std::fs::read_dir(dir).map_err(|err| AppError::Internal(err.to_string()))? {
        let entry = entry.map_err(|err| AppError::Internal(err.to_string()))?;
        let metadata = std::fs::symlink_metadata(entry.path()).map_err(|err| AppError::Internal(err.to_string()))?;
        if metadata.file_type().is_dir() {
            collect_dir_stats(&entry.path(), files, bytes)?;
        } else {
            *files += 1;
            *bytes += metadata.len() as i64;
        }
    }
    Ok(())
}

fn remove_dir_contents(dir: &Path) -> AppResult<(usize, i64)> {
    if !dir.exists() {
        return Ok((0, 0));
    }
    let root = std::fs::canonicalize(dir).map_err(|err| AppError::Internal(err.to_string()))?;
    let (files, bytes) = dir_stats(&root)?;
    for entry in std::fs::read_dir(&root).map_err(|err| AppError::Internal(err.to_string()))? {
        let entry = entry.map_err(|err| AppError::Internal(err.to_string()))?;
        let path = entry.path();
        if !path.starts_with(&root) {
            return Err(AppError::Internal("refusing to remove path outside local data directory".to_string()));
        }
        let metadata = std::fs::symlink_metadata(&path).map_err(|err| AppError::Internal(err.to_string()))?;
        if metadata.file_type().is_dir() {
            std::fs::remove_dir_all(&path).map_err(|err| AppError::Internal(err.to_string()))?;
        } else {
            std::fs::remove_file(&path).map_err(|err| AppError::Internal(err.to_string()))?;
        }
    }
    Ok((files, bytes))
}

fn validate_local_backup_file_name(value: &str) -> AppResult<String> {
    let file_name = value.trim();
    if file_name.is_empty()
        || file_name == "."
        || file_name == ".."
        || file_name.contains('/')
        || file_name.contains('\\')
        || !file_name.ends_with(".sqlite")
    {
        return Err(AppError::InvalidInput(
            "invalid local backup snapshot file name".to_string(),
        ));
    }
    Ok(file_name.to_string())
}

fn validate_sqlite_snapshot(path: &Path) -> AppResult<()> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let integrity: String = conn.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if integrity != "ok" {
        return Err(AppError::InvalidInput(format!(
            "backup snapshot failed SQLite integrity check: {integrity}"
        )));
    }
    let schema_tables: i64 = conn.query_row(
        "
        SELECT COUNT(*)
        FROM sqlite_master
        WHERE type = 'table'
          AND name IN ('app_config', 'accounts', 'retained_mail_messages')
        ",
        [],
        |row| row.get(0),
    )?;
    if schema_tables < 3 {
        return Err(AppError::InvalidInput(
            "backup snapshot does not look like an OutlookEmail database".to_string(),
        ));
    }
    Ok(())
}

fn remove_sqlite_file_set(db_path: &Path) -> AppResult<()> {
    for path in sqlite_file_set(db_path) {
        if path.exists() {
            std::fs::remove_file(&path).map_err(|err| AppError::Internal(err.to_string()))?;
        }
    }
    Ok(())
}

fn sqlite_file_set(db_path: &Path) -> Vec<PathBuf> {
    let path_text = db_path.to_string_lossy();
    vec![
        db_path.to_path_buf(),
        PathBuf::from(format!("{path_text}-wal")),
        PathBuf::from(format!("{path_text}-shm")),
    ]
}

fn exports_dir(db_path: &Path) -> AppResult<PathBuf> {
    Ok(db_path
        .parent()
        .ok_or_else(|| AppError::Internal("database path has no parent directory".to_string()))?
        .join("exports"))
}

fn parse_scheduler_timestamp(value: &str) -> Option<DateTime<Utc>> {
    if let Ok(parsed) = DateTime::parse_from_rfc3339(value) {
        return Some(parsed.with_timezone(&Utc));
    }
    NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S")
        .ok()
        .map(|value| DateTime::<Utc>::from_naive_utc_and_offset(value, Utc))
}

fn safe_file_name(value: &str) -> String {
    let cleaned = value
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            ch if ch.is_control() => '_',
            ch => ch,
        })
        .collect::<String>()
        .trim()
        .to_string();
    if cleaned.is_empty() {
        "attachment.bin".to_string()
    } else {
        cleaned
    }
}

fn unique_path(dir: &Path, file_name: &str) -> PathBuf {
    let path = dir.join(file_name);
    if !path.exists() {
        return path;
    }
    let stem = Path::new(file_name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("attachment");
    let extension = Path::new(file_name)
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!(".{value}"))
        .unwrap_or_default();
    for counter in 2..1000 {
        let candidate = dir.join(format!("{stem}-{counter}{extension}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    dir.join(format!("{stem}-{}{}", uuid::Uuid::new_v4(), extension))
}

fn unique_bundle_file_name(used_names: &mut HashSet<String>, value: &str) -> String {
    let file_name = safe_file_name(value);
    if used_names.insert(file_name.clone()) {
        return file_name;
    }
    let stem = Path::new(&file_name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("attachment");
    let extension = Path::new(&file_name)
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!(".{value}"))
        .unwrap_or_default();
    for counter in 2..1000 {
        let candidate = format!("{stem}-{counter}{extension}");
        if used_names.insert(candidate.clone()) {
            return candidate;
        }
    }
    let candidate = format!("{stem}-{}{}", uuid::Uuid::new_v4(), extension);
    used_names.insert(candidate.clone());
    candidate
}

struct ZipCentralEntry {
    name: Vec<u8>,
    crc32: u32,
    size: u32,
    local_header_offset: u32,
}

fn write_zip_bundle(path: &Path, files: &[(String, Vec<u8>)]) -> AppResult<i64> {
    if files.is_empty() {
        return Err(AppError::InvalidInput("zip bundle requires at least one file".to_string()));
    }
    let mut data = Vec::new();
    let mut central_entries = Vec::new();
    for (name, bytes) in files {
        let name_bytes = name.as_bytes();
        let name_len = zip_u16(name_bytes.len(), "zip file name is too long")?;
        let size = zip_u32(bytes.len(), "attachment is too large for a standard ZIP bundle")?;
        let offset = zip_u32(data.len(), "zip bundle is too large")?;
        let crc = crc32(bytes);

        push_u32(&mut data, 0x0403_4b50);
        push_u16(&mut data, 20);
        push_u16(&mut data, 0x0800);
        push_u16(&mut data, 0);
        push_u16(&mut data, 0);
        push_u16(&mut data, 33);
        push_u32(&mut data, crc);
        push_u32(&mut data, size);
        push_u32(&mut data, size);
        push_u16(&mut data, name_len);
        push_u16(&mut data, 0);
        data.extend_from_slice(name_bytes);
        data.extend_from_slice(bytes);

        central_entries.push(ZipCentralEntry {
            name: name_bytes.to_vec(),
            crc32: crc,
            size,
            local_header_offset: offset,
        });
    }

    let central_offset_usize = data.len();
    let central_offset = zip_u32(central_offset_usize, "zip central directory offset is too large")?;
    for entry in &central_entries {
        let name_len = zip_u16(entry.name.len(), "zip file name is too long")?;
        push_u32(&mut data, 0x0201_4b50);
        push_u16(&mut data, 20);
        push_u16(&mut data, 20);
        push_u16(&mut data, 0x0800);
        push_u16(&mut data, 0);
        push_u16(&mut data, 0);
        push_u16(&mut data, 33);
        push_u32(&mut data, entry.crc32);
        push_u32(&mut data, entry.size);
        push_u32(&mut data, entry.size);
        push_u16(&mut data, name_len);
        push_u16(&mut data, 0);
        push_u16(&mut data, 0);
        push_u16(&mut data, 0);
        push_u16(&mut data, 0);
        push_u32(&mut data, 0);
        push_u32(&mut data, entry.local_header_offset);
        data.extend_from_slice(&entry.name);
    }

    let central_size = zip_u32(data.len() - central_offset_usize, "zip central directory is too large")?;
    let entry_count = zip_u16(central_entries.len(), "zip bundle has too many files")?;
    push_u32(&mut data, 0x0605_4b50);
    push_u16(&mut data, 0);
    push_u16(&mut data, 0);
    push_u16(&mut data, entry_count);
    push_u16(&mut data, entry_count);
    push_u32(&mut data, central_size);
    push_u32(&mut data, central_offset);
    push_u16(&mut data, 0);

    std::fs::write(path, &data).map_err(|err| AppError::Internal(err.to_string()))?;
    Ok(data.len() as i64)
}

fn push_u16(target: &mut Vec<u8>, value: u16) {
    target.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(target: &mut Vec<u8>, value: u32) {
    target.extend_from_slice(&value.to_le_bytes());
}

fn zip_u16(value: usize, label: &str) -> AppResult<u16> {
    u16::try_from(value).map_err(|_| AppError::InvalidInput(label.to_string()))
}

fn zip_u32(value: usize, label: &str) -> AppResult<u32> {
    u32::try_from(value).map_err(|_| AppError::InvalidInput(label.to_string()))
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in bytes {
        crc ^= *byte as u32;
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn normalize_project_key(value: &str) -> String {
    let mut output = String::new();
    let mut last_dash = false;
    for ch in value.trim().to_ascii_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            output.push(ch);
            last_dash = false;
        } else if !last_dash {
            output.push('-');
            last_dash = true;
        }
    }
    output.trim_matches('-').to_string()
}

fn validate_project_status(value: &str) -> AppResult<()> {
    match value {
        "toClaim" | "claimed" | "success" | "failed" | "removed" => Ok(()),
        _ => Err(AppError::InvalidInput(format!("invalid project account status: {value}"))),
    }
}

fn project_account_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProjectAccount> {
    Ok(ProjectAccount {
        id: row.get(0)?,
        project_id: row.get(1)?,
        account_id: row.get(2)?,
        normalized_email: row.get(3)?,
        email: row.get(4)?,
        status: row.get(5)?,
        claim_token: row.get(6)?,
        claimed_at: row.get(7)?,
        lease_expires_at: row.get(8)?,
        last_result: row.get(9)?,
        last_result_detail: row.get(10)?,
        claim_count: row.get(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
    })
}

fn backup_log_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<BackupLog> {
    Ok(BackupLog {
        id: row.get(0)?,
        target: row.get(1)?,
        status: row.get(2)?,
        file_name: row.get(3)?,
        size: row.get(4)?,
        error_message: row.get(5)?,
        created_at: row.get(6)?,
    })
}

fn refresh_log_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RefreshLog> {
    Ok(RefreshLog {
        id: row.get(0)?,
        account_id: row.get(1)?,
        account_email: row.get(2)?,
        refresh_type: row.get(3)?,
        status: row.get(4)?,
        error_message: row.get(5)?,
        created_at: row.get(6)?,
    })
}

fn automation_run_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AutomationRun> {
    let status = row.get::<_, String>(3)?;
    let message = row.get::<_, String>(4)?;
    Ok(AutomationRun {
        id: row.get(0)?,
        job_type: row.get(1)?,
        trigger_type: row.get(2)?,
        status: status.clone(),
        error_category: if status == "failed" {
            classify_error_category(&message).to_string()
        } else {
            "none".to_string()
        },
        message,
        refreshed: row.get(5)?,
        failed: row.get(6)?,
        duration_ms: row.get(7)?,
        started_at: row.get(8)?,
        finished_at: row.get(9)?,
    })
}

fn retry_queue_item_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RetryQueueItem> {
    let status = row.get::<_, String>(2)?;
    let error_message = row.get::<_, String>(9)?;
    let next_attempt_at = row.get::<_, Option<String>>(12)?;
    Ok(RetryQueueItem {
        id: row.get(0)?,
        task_type: row.get(1)?,
        status: status.clone(),
        account_id: row.get(3)?,
        account_email: row.get(4)?,
        message_id: row.get(5)?,
        channel: row.get(6)?,
        action: row.get(7)?,
        payload_json: row.get(8)?,
        error_message: error_message.clone(),
        error_category: classify_error_category(&error_message).to_string(),
        attempts: row.get(10)?,
        max_attempts: row.get(11)?,
        due_now: retry_due_now(&status, next_attempt_at.as_deref()),
        next_delay_minutes: retry_next_delay_minutes(next_attempt_at.as_deref()),
        next_attempt_at,
        last_attempt_at: row.get(13)?,
        created_at: row.get(14)?,
        updated_at: row.get(15)?,
    })
}

fn mail_share_record_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MailShareRecord> {
    let token_hash = row.get::<_, String>(4)?;
    let expires_at = row.get::<_, Option<String>>(9)?;
    let revoked_at = row.get::<_, Option<String>>(10)?;
    Ok(MailShareRecord {
        id: row.get(0)?,
        account_id: row.get(1)?,
        account_email: row.get(2)?,
        title: row.get(3)?,
        token_preview: token_hash.chars().take(8).collect(),
        exported_path: row.get(5)?,
        file_name: row.get(6)?,
        item_count: row.get(7)?,
        size: row.get(8)?,
        status: share_record_status(expires_at.as_deref(), revoked_at.as_deref()).to_string(),
        expires_at,
        revoked_at,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
    })
}

fn remote_sync_failure_from_message_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Option<RemoteSyncFailure>> {
    let retry_id = row.get::<_, Option<i64>>(14)?;
    Ok(match retry_id {
        Some(retry_id) => Some(RemoteSyncFailure {
            retry_id,
            task_type: row.get(15)?,
            status: row.get(16)?,
            action: row.get(17)?,
            error_message: row.get(18)?,
            attempts: row.get(19)?,
            max_attempts: row.get(20)?,
            next_attempt_at: row.get(21)?,
            last_attempt_at: row.get(22)?,
            updated_at: row.get(23)?,
        }),
        None => None,
    })
}

#[cfg(test)]
mod project_tests {
    use super::{attachment_dir, backup_dir, exports_dir, normalize_project_key, Database, MailMessageRef};
    use crate::error::AppError;
    use crate::import::ImportedAccount;
    use crate::models::{
        AccountBatchInput, AttachmentInfo, AutomationRunQuery, ClaimProjectAccountInput,
        ClearAutomationRunsInput, ClearLocalDataInput, CreateGroupInput, CreateMailShareInput,
        CreateProjectInput, DeleteMailMessagesInput,
        DownloadAllAttachmentsInput, DownloadAttachmentInput, ExportAccountSecretsInput, ExportAccountsInput, ExportMailMessagesInput,
        ImportTempEmailsInput, MailMessageQuery, MarkMailMessagesInput, ProjectAccountActionInput, RefreshInput,
        RestoreBackupInput, RevealAccountSecretsInput, RevokeMailShareInput, RetryQueueItemInput, RetryQueueQuery,
        RetryQueueRunInput, UpdateAccountInput, UpdateGroupInput, UpdateGroupProxyInput,
        UpdateTempEmailInput, Settings,
    };
    use rusqlite::{params, Connection};
    use std::path::PathBuf;

    #[test]
    fn normalizes_project_key() {
        assert_eq!(normalize_project_key(" My Project 01 "), "my-project-01");
        assert_eq!(normalize_project_key("中文项目"), "");
    }

    #[test]
    fn local_desktop_workflow_covers_core_e2e_paths() {
        let root = std::env::temp_dir().join(format!("outlook-email-e2e-workflow-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("create temp dir");
        let db_path = root.join("workflow.sqlite");
        let conn = Connection::open(&db_path).expect("open db");
        let mut db = Database {
            conn,
            db_path,
            crypto_key: Some([7; 32]),
        };
        db.initialize_schema().expect("schema");

        let imported = db
            .import_accounts(
                vec![ImportedAccount {
                    email: "flow@example.com".to_string(),
                    password: "password".to_string(),
                    client_id: String::new(),
                    refresh_token: String::new(),
                    remark: "workflow".to_string(),
                }],
                None,
            )
            .expect("import account");
        assert_eq!(imported.imported, 1);
        let account = db.list_accounts().expect("accounts").remove(0);

        let refresh = db
            .refresh_accounts(RefreshInput {
                account_id: Some(account.id),
                folder: Some("inbox".to_string()),
                top: Some(1),
            })
            .expect("refresh result");
        assert_eq!(refresh.failed, 1);
        assert!(db
            .list_retry_queue(RetryQueueQuery {
                task_type: Some("refresh_account".to_string()),
                ..RetryQueueQuery::default()
            })
            .expect("refresh retry")
            .len()
            >= 1);

        let message = db.create_demo_message(account.id).expect("demo message");
        let marked = db
            .mark_mail_messages(MarkMailMessagesInput {
                message_ids: vec![message.id],
                is_read: true,
                sync_remote: Some(false),
            })
            .expect("mark read");
        assert_eq!(marked.refreshed, 1);
        let target = MailMessageRef {
            id: message.id,
            account_id: account.id,
            account_email: account.email.clone(),
            folder: message.folder.clone(),
            provider_message_id: message.provider_message_id.clone(),
        };
        db.enqueue_mail_delete_retry(&target, "workflow remote delete failed")
            .expect("enqueue delete retry");
        let delete_retry = db
            .list_retry_queue(RetryQueueQuery {
                task_type: Some("mail_delete".to_string()),
                ..RetryQueueQuery::default()
            })
            .expect("delete retry")
            .remove(0);
        let retried = db
            .run_retry_queue(Some(RetryQueueRunInput {
                retry_id: Some(delete_retry.id),
                limit: None,
            }))
            .expect("retry delete");
        assert_eq!(retried.refreshed, 1);
        assert!(db
            .list_messages(Some(account.id), Some("all".to_string()))
            .expect("messages after delete retry")
            .is_empty());

        let temp_imported = db
            .import_temp_emails(ImportTempEmailsInput {
                raw: "temp@example.com".to_string(),
                provider: "gptmail".to_string(),
                channel_id: None,
            })
            .expect("import temp email");
        assert_eq!(temp_imported.imported, 1);
        db.update_temp_email(UpdateTempEmailInput {
            email: "temp@example.com".to_string(),
            tags: vec!["Workflow".to_string()],
        })
        .expect("tag temp email");

        let backup_dir = backup_dir(&db.db_path).expect("backup dir");
        std::fs::create_dir_all(&backup_dir).expect("create backup dir");
        let backup_file = "workflow-backup.sqlite";
        let backup_path = backup_dir.join(backup_file);
        let backup_path_text = backup_path.to_string_lossy().to_string();
        db.conn
            .execute("VACUUM INTO ?", [backup_path_text.as_str()])
            .expect("vacuum backup");
        let size = backup_path.metadata().expect("backup metadata").len() as i64;
        db.insert_backup_log("local-workflow", "success", backup_file, size, None)
            .expect("backup log");

        db.delete_temp_email("temp@example.com".to_string())
            .expect("delete temp before restore");
        assert!(db.list_temp_emails().expect("temp deleted").is_empty());
        let restored = db
            .restore_backup(RestoreBackupInput {
                backup_log_id: 1,
                confirm: true,
            })
            .expect("restore backup");
        assert!(std::path::Path::new(&restored.safety_backup_path).exists());
        let restored_temp = db.list_temp_emails().expect("restored temp").remove(0);
        assert_eq!(restored_temp.email, "temp@example.com");
        assert_eq!(restored_temp.tags, vec!["Workflow".to_string()]);
    }

    #[test]
    fn project_pool_claim_success_flow() {
        let conn = Connection::open_in_memory().expect("open memory db");
        let mut db = Database {
            conn,
            db_path: PathBuf::from("memory.sqlite"),
            crypto_key: Some([7; 32]),
        };
        db.initialize_schema().expect("schema");
        db.conn
            .execute(
                "INSERT INTO accounts (email, status, group_id) VALUES ('one@example.com', 'active', 1)",
                [],
            )
            .expect("insert account");

        let project = db
            .create_project(CreateProjectInput {
                name: "Registration".to_string(),
                project_key: None,
                description: None,
                scope_mode: Some("all".to_string()),
                use_alias_email: None,
                group_ids: None,
                tag_ids: None,
            })
            .expect("create project");
        assert_eq!(project.stats.to_claim, 1);

        let claimed = db
            .claim_project_account(ClaimProjectAccountInput {
                project_id: project.id,
                lease_minutes: Some(30),
            })
            .expect("claim")
            .expect("claimed account");
        assert_eq!(claimed.status, "claimed");
        assert_eq!(claimed.claim_count, 1);

        let completed = db
            .complete_project_account_success(ProjectAccountActionInput {
                project_account_id: claimed.id,
                detail: Some("ok".to_string()),
            })
            .expect("success");
        assert_eq!(completed.status, "success");
        assert_eq!(db.get_project(project.id).expect("project").stats.success, 1);
    }

    #[test]
    fn project_scope_can_sync_accounts_by_tags() {
        let conn = Connection::open_in_memory().expect("open memory db");
        let mut db = Database {
            conn,
            db_path: PathBuf::from("memory.sqlite"),
            crypto_key: Some([7; 32]),
        };
        db.initialize_schema().expect("schema");
        db.conn
            .execute(
                "
                INSERT INTO accounts (id, email, status, group_id)
                VALUES
                    (1, 'core@example.com', 'active', 1),
                    (2, 'warmup@example.com', 'active', 1),
                    (3, 'disabled@example.com', 'disabled', 1)
                ",
                [],
            )
            .expect("insert accounts");
        db.conn
            .execute("INSERT INTO tags (id, name, color) VALUES (10, 'ProjectCore', '#2563eb'), (11, 'ProjectWarmup', '#16a34a')", [])
            .expect("insert tags");

        db.update_account(UpdateAccountInput {
            id: 1,
            email: "core@example.com".to_string(),
            group_id: Some(1),
            remark: None,
            status: Some("active".to_string()),
            provider: None,
            account_type: None,
            imap_host: None,
            imap_port: None,
            proxy_url: None,
            fallback_proxy_url_1: None,
            fallback_proxy_url_2: None,
            forward_enabled: None,
            password: None,
            client_id: None,
            refresh_token: None,
            imap_password: None,
            tag_ids: Some(vec![10]),
            aliases: None,
        })
        .expect("tag core account");
        db.update_account(UpdateAccountInput {
            id: 2,
            email: "warmup@example.com".to_string(),
            group_id: Some(1),
            remark: None,
            status: Some("active".to_string()),
            provider: None,
            account_type: None,
            imap_host: None,
            imap_port: None,
            proxy_url: None,
            fallback_proxy_url_1: None,
            fallback_proxy_url_2: None,
            forward_enabled: None,
            password: None,
            client_id: None,
            refresh_token: None,
            imap_password: None,
            tag_ids: Some(vec![11]),
            aliases: None,
        })
        .expect("tag warmup account");
        db.update_account(UpdateAccountInput {
            id: 3,
            email: "disabled@example.com".to_string(),
            group_id: Some(1),
            remark: None,
            status: Some("disabled".to_string()),
            provider: None,
            account_type: None,
            imap_host: None,
            imap_port: None,
            proxy_url: None,
            fallback_proxy_url_1: None,
            fallback_proxy_url_2: None,
            forward_enabled: None,
            password: None,
            client_id: None,
            refresh_token: None,
            imap_password: None,
            tag_ids: Some(vec![10]),
            aliases: None,
        })
        .expect("tag disabled account");

        let project = db
            .create_project(CreateProjectInput {
                name: "Tagged Project".to_string(),
                project_key: None,
                description: None,
                scope_mode: Some("tags".to_string()),
                use_alias_email: None,
                group_ids: None,
                tag_ids: Some(vec![10]),
            })
            .expect("create tagged project");
        assert_eq!(project.scope_mode, "tags");
        assert_eq!(project.tag_ids, vec![10]);
        assert_eq!(project.stats.total, 1);
        assert_eq!(project.stats.to_claim, 1);

        let accounts = db.list_project_accounts(project.id).expect("project accounts");
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].email, "core@example.com");
        assert_eq!(db.list_accounts().expect("accounts")[0].tags[0].name, "ProjectCore");
    }

    #[test]
    fn account_batch_updates_delete_and_selected_export() {
        let root = std::env::temp_dir().join(format!("outlook-email-account-batch-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("create temp dir");
        let conn = Connection::open_in_memory().expect("open memory db");
        let mut db = Database {
            conn,
            db_path: root.join("test.sqlite"),
            crypto_key: Some([7; 32]),
        };
        db.initialize_schema().expect("schema");
        let batch_group = db
            .create_group(CreateGroupInput {
                name: "Batch".to_string(),
                description: None,
                color: None,
                parent_id: None,
                proxy_url: None,
                fallback_proxy_url_1: None,
                fallback_proxy_url_2: None,
            })
            .expect("create group");
        db.conn
            .execute(
                "
                INSERT INTO accounts (id, email, status, group_id)
                VALUES
                    (1, 'one@example.com', 'active', 1),
                    (2, 'two@example.com', 'active', 1),
                    (3, 'three@example.com', 'active', 1)
                ",
                [],
            )
            .expect("insert accounts");
        db.conn
            .execute(
                "INSERT INTO tags (id, name, color) VALUES (10, 'BatchCore', '#111827'), (11, 'BatchWarmup', '#374151')",
                [],
            )
            .expect("insert tags");

        let moved = db
            .batch_accounts(AccountBatchInput {
                account_ids: vec![1, 2, 2, 999],
                action: "move_group".to_string(),
                group_id: Some(batch_group.id),
                forward_enabled: None,
                tag_ids: None,
            })
            .expect("move accounts");
        assert_eq!(moved.refreshed, 2);
        assert_eq!(moved.failed, 1);
        assert!(!moved.success);
        let moved_count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM accounts WHERE group_id = ?",
                [batch_group.id],
                |row| row.get(0),
            )
            .expect("moved count");
        assert_eq!(moved_count, 2);

        db.batch_accounts(AccountBatchInput {
            account_ids: vec![1, 2],
            action: "set_forward".to_string(),
            group_id: None,
            forward_enabled: Some(true),
            tag_ids: None,
        })
        .expect("set forward");
        let forward_count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM accounts WHERE forward_enabled = 1", [], |row| row.get(0))
            .expect("forward count");
        assert_eq!(forward_count, 2);

        db.batch_accounts(AccountBatchInput {
            account_ids: vec![1, 2],
            action: "add_tags".to_string(),
            group_id: None,
            forward_enabled: None,
            tag_ids: Some(vec![10, 11]),
        })
        .expect("add tags");
        let tag_count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM account_tags", [], |row| row.get(0))
            .expect("tag count");
        assert_eq!(tag_count, 4);

        db.batch_accounts(AccountBatchInput {
            account_ids: vec![1, 2],
            action: "remove_tags".to_string(),
            group_id: None,
            forward_enabled: None,
            tag_ids: Some(vec![11]),
        })
        .expect("remove tags");
        let warmup_count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM account_tags WHERE tag_id = 11", [], |row| row.get(0))
            .expect("warmup tag count");
        assert_eq!(warmup_count, 0);

        let selected_export = db
            .export_accounts(ExportAccountsInput {
                group_id: None,
                account_ids: Some(vec![1]),
            })
            .expect("export selected accounts");
        assert_eq!(selected_export.item_count, 1);
        let csv = std::fs::read_to_string(&selected_export.path).expect("read selected export");
        assert!(csv.contains("one@example.com"));
        assert!(!csv.contains("two@example.com"));

        db.batch_accounts(AccountBatchInput {
            account_ids: vec![1, 2],
            action: "delete".to_string(),
            group_id: None,
            forward_enabled: None,
            tag_ids: None,
        })
        .expect("delete accounts");
        assert_eq!(db.list_accounts().expect("accounts").len(), 1);
    }

    #[test]
    fn account_secrets_require_local_password() {
        let root = std::env::temp_dir().join(format!("outlook-email-secret-export-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("create temp dir");
        let conn = Connection::open_in_memory().expect("open memory db");
        let mut db = Database {
            conn,
            db_path: root.join("test.sqlite"),
            crypto_key: None,
        };
        db.initialize_schema().expect("schema");
        db.initialize_app("local-password").expect("initialize app");
        db.import_accounts(
            vec![ImportedAccount {
                email: "secret@example.com".to_string(),
                password: "account-password".to_string(),
                client_id: "client-id-value".to_string(),
                refresh_token: "refresh-token-value".to_string(),
                remark: "secret".to_string(),
            }],
            Some(1),
        )
        .expect("import account");

        let secrets = db
            .reveal_account_secrets(RevealAccountSecretsInput {
                account_id: 1,
                password: "local-password".to_string(),
            })
            .expect("reveal secrets");
        assert_eq!(secrets.password, "account-password");
        assert_eq!(secrets.client_id, "client-id-value");
        assert_eq!(secrets.refresh_token_preview, "refr...alue");
        assert_eq!(secrets.imap_password, "");

        let denied = db.reveal_account_secrets(RevealAccountSecretsInput {
            account_id: 1,
            password: "wrong-password".to_string(),
        });
        assert!(matches!(denied, Err(AppError::Unauthorized)));

        let bad_confirm = db.export_account_secrets(ExportAccountSecretsInput {
            account_ids: vec![1],
            password: "local-password".to_string(),
            confirm: "EXPORT".to_string(),
        });
        assert!(matches!(bad_confirm, Err(AppError::InvalidInput(_))));

        let bad_password = db.export_account_secrets(ExportAccountSecretsInput {
            account_ids: vec![1],
            password: "wrong-password".to_string(),
            confirm: "EXPORT ACCOUNT SECRETS".to_string(),
        });
        assert!(matches!(bad_password, Err(AppError::Unauthorized)));

        let exported = db
            .export_account_secrets(ExportAccountSecretsInput {
                account_ids: vec![1],
                password: "local-password".to_string(),
                confirm: "EXPORT ACCOUNT SECRETS".to_string(),
            })
            .expect("export account secrets");
        assert_eq!(exported.item_count, 1);
        let csv = std::fs::read_to_string(&exported.path).expect("read secret export");
        assert!(csv.contains("secret@example.com"));
        assert!(csv.contains("account-password"));
        assert!(csv.contains("client-id-value"));
        assert!(csv.contains("refresh-token-value"));
    }

    #[test]
    fn account_aliases_are_listed_and_validated() {
        let conn = Connection::open_in_memory().expect("open memory db");
        let mut db = Database {
            conn,
            db_path: PathBuf::from("memory.sqlite"),
            crypto_key: Some([7; 32]),
        };
        db.initialize_schema().expect("schema");
        db.conn
            .execute(
                "
                INSERT INTO accounts (id, email, status, group_id)
                VALUES
                    (1, 'one@example.com', 'active', 1),
                    (2, 'two@example.com', 'active', 1)
                ",
                [],
            )
            .expect("insert accounts");

        let account_input = |id: i64, email: &str, aliases: Vec<&str>| UpdateAccountInput {
            id,
            email: email.to_string(),
            group_id: Some(1),
            remark: None,
            status: Some("active".to_string()),
            provider: None,
            account_type: None,
            imap_host: None,
            imap_port: None,
            proxy_url: None,
            fallback_proxy_url_1: None,
            fallback_proxy_url_2: None,
            forward_enabled: None,
            password: None,
            client_id: None,
            refresh_token: None,
            imap_password: None,
            tag_ids: None,
            aliases: Some(aliases.into_iter().map(str::to_string).collect()),
        };

        let updated = db
            .update_account(account_input(
                1,
                "one@example.com",
                vec!["Alias@One.com", "alias-one@example.com", "ALIAS@ONE.COM"],
            ))
            .expect("update aliases");
        assert_eq!(updated.aliases, vec!["alias@one.com", "alias-one@example.com"]);

        let listed = db
            .list_accounts()
            .expect("list accounts")
            .into_iter()
            .find(|account| account.id == 1)
            .expect("listed account");
        assert_eq!(listed.aliases, vec!["alias@one.com", "alias-one@example.com"]);

        let primary_conflict = db.update_account(account_input(1, "one@example.com", vec!["two@example.com"]));
        assert!(matches!(primary_conflict, Err(AppError::InvalidInput(_))));

        let alias_conflict = db.update_account(account_input(2, "two@example.com", vec!["alias@one.com"]));
        assert!(matches!(alias_conflict, Err(AppError::InvalidInput(_))));
    }

    #[test]
    fn account_proxy_chain_inherits_group_and_allows_override() {
        let conn = Connection::open_in_memory().expect("open memory db");
        let mut db = Database {
            conn,
            db_path: PathBuf::from("memory.sqlite"),
            crypto_key: Some([7; 32]),
        };
        db.initialize_schema().expect("schema");
        db.update_group_proxy(UpdateGroupProxyInput {
            id: 1,
            proxy_url: Some("http://group-proxy:8080".to_string()),
            fallback_proxy_url_1: Some("https://group-backup:8443".to_string()),
            fallback_proxy_url_2: None,
        })
        .expect("set group proxy");
        db.conn
            .execute(
                "INSERT INTO accounts (id, email, status, group_id) VALUES (1, 'proxy@example.com', 'active', 1)",
                [],
            )
            .expect("insert account");

        let inherited = db.account_credentials(Some(1)).expect("credentials");
        assert_eq!(
            inherited[0].proxy_chain,
            vec!["http://group-proxy:8080", "https://group-backup:8443"]
        );

        db.update_account(UpdateAccountInput {
            id: 1,
            email: "proxy@example.com".to_string(),
            group_id: Some(1),
            remark: None,
            status: Some("active".to_string()),
            provider: None,
            account_type: None,
            imap_host: None,
            imap_port: None,
            proxy_url: Some("http://account-proxy:8080".to_string()),
            fallback_proxy_url_1: Some("http://account-proxy:8080".to_string()),
            fallback_proxy_url_2: Some("https://account-backup:8443".to_string()),
            forward_enabled: None,
            password: None,
            client_id: None,
            refresh_token: None,
            imap_password: None,
            tag_ids: None,
            aliases: None,
        })
        .expect("set account proxy");

        let overridden = db.account_credentials(Some(1)).expect("credentials");
        assert_eq!(
            overridden[0].proxy_chain,
            vec!["http://account-proxy:8080", "https://account-backup:8443"]
        );

        let invalid = db.update_group_proxy(UpdateGroupProxyInput {
            id: 1,
            proxy_url: Some("socks5://127.0.0.1:1080".to_string()),
            fallback_proxy_url_1: None,
            fallback_proxy_url_2: None,
        });
        assert!(matches!(invalid, Err(AppError::InvalidInput(_))));
    }

    #[test]
    fn group_update_and_delete_maintain_tree_and_accounts() {
        let conn = Connection::open_in_memory().expect("open memory db");
        let mut db = Database {
            conn,
            db_path: PathBuf::from("memory.sqlite"),
            crypto_key: Some([7; 32]),
        };
        db.initialize_schema().expect("schema");
        let child = db
            .create_group(CreateGroupInput {
                name: "Child".to_string(),
                description: None,
                color: None,
                parent_id: Some(1),
                proxy_url: None,
                fallback_proxy_url_1: None,
                fallback_proxy_url_2: None,
            })
            .expect("create child");
        let grandchild = db
            .create_group(CreateGroupInput {
                name: "Grandchild".to_string(),
                description: None,
                color: None,
                parent_id: Some(child.id),
                proxy_url: None,
                fallback_proxy_url_1: None,
                fallback_proxy_url_2: None,
            })
            .expect("create grandchild");
        db.conn
            .execute(
                "INSERT INTO accounts (id, email, status, group_id) VALUES (1, 'group@example.com', 'active', ?)",
                [child.id],
            )
            .expect("insert account");

        let moved = db
            .update_group(UpdateGroupInput {
                id: child.id,
                name: "Child Root".to_string(),
                description: Some("moved".to_string()),
                color: Some("#111827".to_string()),
                parent_id: None,
                sort_order: Some(2),
                proxy_url: None,
                fallback_proxy_url_1: None,
                fallback_proxy_url_2: None,
            })
            .expect("move child");
        assert_eq!(moved.level, 1);
        assert_eq!(db.get_group(grandchild.id).expect("grandchild").level, 2);

        let cycle = db.update_group(UpdateGroupInput {
            id: child.id,
            name: "Cycle".to_string(),
            description: None,
            color: None,
            parent_id: Some(grandchild.id),
            sort_order: None,
            proxy_url: None,
            fallback_proxy_url_1: None,
            fallback_proxy_url_2: None,
        });
        assert!(matches!(cycle, Err(AppError::InvalidInput(_))));

        db.delete_group(child.id).expect("delete child");
        let account_group = db
            .conn
            .query_row("SELECT group_id FROM accounts WHERE id = 1", [], |row| row.get::<_, Option<i64>>(0))
            .expect("account group");
        assert_eq!(account_group, None);
        let promoted = db.get_group(grandchild.id).expect("promoted grandchild");
        assert_eq!(promoted.parent_id, None);
        assert_eq!(promoted.level, 1);
    }

    #[test]
    fn project_scope_can_use_account_alias_email() {
        let conn = Connection::open_in_memory().expect("open memory db");
        let mut db = Database {
            conn,
            db_path: PathBuf::from("memory.sqlite"),
            crypto_key: Some([7; 32]),
        };
        db.initialize_schema().expect("schema");
        db.conn
            .execute(
                "INSERT INTO accounts (id, email, status, group_id) VALUES (1, 'primary@example.com', 'active', 1)",
                [],
            )
            .expect("insert account");
        db.update_account(UpdateAccountInput {
            id: 1,
            email: "primary@example.com".to_string(),
            group_id: Some(1),
            remark: None,
            status: Some("active".to_string()),
            provider: None,
            account_type: None,
            imap_host: None,
            imap_port: None,
            proxy_url: None,
            fallback_proxy_url_1: None,
            fallback_proxy_url_2: None,
            forward_enabled: None,
            password: None,
            client_id: None,
            refresh_token: None,
            imap_password: None,
            tag_ids: None,
            aliases: Some(vec!["alias@example.com".to_string(), "second@example.com".to_string()]),
        })
        .expect("set alias");

        let project = db
            .create_project(CreateProjectInput {
                name: "Alias Project".to_string(),
                project_key: None,
                description: None,
                scope_mode: Some("all".to_string()),
                use_alias_email: Some(true),
                group_ids: None,
                tag_ids: None,
            })
            .expect("create project");
        assert!(project.use_alias_email);

        let accounts = db.list_project_accounts(project.id).expect("project accounts");
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].email, "alias@example.com");
        assert_eq!(accounts[0].normalized_email, "alias@example.com");
    }

    #[test]
    fn mail_message_mark_and_delete_updates_local_cache() {
        let conn = Connection::open_in_memory().expect("open memory db");
        let mut db = Database {
            conn,
            db_path: PathBuf::from("memory.sqlite"),
            crypto_key: Some([7; 32]),
        };
        db.initialize_schema().expect("schema");
        db.conn
            .execute(
                "INSERT INTO accounts (id, email, status, group_id) VALUES (1, 'one@example.com', 'active', 1)",
                [],
            )
            .expect("insert account");
        db.conn
            .execute(
                "
                INSERT INTO retained_mail_messages
                (id, account_id, folder, provider_message_id, subject, received_at, received_at_sort, is_read)
                VALUES (10, 1, 'inbox', 'local-demo-test', 'Hello', '2026-01-01T00:00:00Z', 1, 0)
                ",
                [],
            )
            .expect("insert message");

        let marked = db
            .mark_mail_messages(MarkMailMessagesInput {
                message_ids: vec![10],
                is_read: true,
                sync_remote: Some(false),
            })
            .expect("mark");
        assert_eq!(marked.refreshed, 1);
        assert!(db
            .list_messages(Some(1), Some("all".to_string()))
            .expect("messages")[0]
            .is_read);

        let deleted = db
            .delete_mail_messages(DeleteMailMessagesInput {
                message_ids: vec![10],
                sync_remote: Some(false),
            })
            .expect("delete");
        assert_eq!(deleted.refreshed, 1);
        assert!(db
            .list_messages(Some(1), Some("all".to_string()))
            .expect("messages")
            .is_empty());
    }

    #[test]
    fn searches_messages_with_field_tokens_and_sorting() {
        let conn = Connection::open_in_memory().expect("open memory db");
        let mut db = Database {
            conn,
            db_path: PathBuf::from("memory.sqlite"),
            crypto_key: Some([7; 32]),
        };
        db.initialize_schema().expect("schema");
        db.conn
            .execute(
                "INSERT INTO accounts (id, email, status, group_id) VALUES (1, 'one@example.com', 'active', 1)",
                [],
            )
            .expect("insert account");
        db.conn
            .execute(
                "
                INSERT INTO retained_mail_messages
                (id, account_id, folder, provider_message_id, subject, sender, recipients, cc,
                 received_at, received_at_sort, is_read, has_attachments, body_preview, body)
                VALUES
                (10, 1, 'inbox', 'a', 'Alpha notice', 'noreply@example.com', 'one@example.com', '',
                 '2026-01-01T00:00:00Z', 1, 1, 0, 'alpha preview', 'plain body'),
                (11, 1, 'junkemail', 'b', 'Beta invoice', 'billing@example.com', 'one@example.com', '',
                 '2026-01-02T00:00:00Z', 2, 0, 0, 'invoice preview', 'invoice body'),
                (12, 1, 'inbox', 'c', 'Reset Password', 'alice@example.com', 'one@example.com', '',
                 '2026-01-03T00:00:00Z', 3, 0, 1, 'security preview', 'reset body')
                ",
                [],
            )
            .expect("insert messages");

        let filtered = db
            .list_messages_query(MailMessageQuery {
                account_id: Some(1),
                folder: Some("all".to_string()),
                search: Some("from:alice subject:\"Reset Password\" is:unread has:attachment".to_string()),
                read_state: None,
                has_attachments: None,
                sort_by: Some("date".to_string()),
                sort_order: Some("desc".to_string()),
                limit: Some(10),
                offset: Some(0),
            })
            .expect("advanced search");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].provider_message_id, "c");

        let junk = db
            .list_messages_query(MailMessageQuery {
                account_id: Some(1),
                folder: Some("all".to_string()),
                search: Some("folder:junk body:invoice".to_string()),
                read_state: None,
                has_attachments: None,
                sort_by: None,
                sort_order: None,
                limit: Some(10),
                offset: Some(0),
            })
            .expect("folder search");
        assert_eq!(junk.len(), 1);
        assert_eq!(junk[0].folder, "junkemail");

        let sorted = db
            .list_messages_query(MailMessageQuery {
                account_id: Some(1),
                folder: Some("all".to_string()),
                search: None,
                read_state: None,
                has_attachments: None,
                sort_by: Some("subject".to_string()),
                sort_order: Some("asc".to_string()),
                limit: Some(10),
                offset: Some(0),
            })
            .expect("sort messages");
        assert_eq!(
            sorted.into_iter().map(|message| message.subject).collect::<Vec<_>>(),
            vec!["Alpha notice", "Beta invoice", "Reset Password"]
        );
    }

    #[test]
    fn exports_mail_html_and_accounts_csv() {
        let root = std::env::temp_dir().join(format!("outlook-email-export-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("create temp dir");
        let conn = Connection::open_in_memory().expect("open memory db");
        let mut db = Database {
            conn,
            db_path: root.join("test.sqlite"),
            crypto_key: Some([7; 32]),
        };
        db.initialize_schema().expect("schema");
        db.conn
            .execute(
                "INSERT INTO accounts (id, email, status, group_id, remark) VALUES (1, 'one@example.com', 'active', 1, 'main')",
                [],
            )
            .expect("insert account");
        db.conn
            .execute(
                "
                INSERT INTO retained_mail_messages
                (id, account_id, folder, provider_message_id, subject, sender, recipients, received_at, received_at_sort, body_preview, body)
                VALUES (10, 1, 'inbox', 'local-demo-test', 'Hello <b>', 'sender@example.com', 'one@example.com', '2026-01-01T00:00:00Z', 1, 'preview', '<b>body</b>')
                ",
                [],
            )
            .expect("insert message");

        let mail_export = db
            .export_mail_messages(ExportMailMessagesInput {
                message_ids: vec![10],
                title: Some("Mail export".to_string()),
            })
            .expect("export mail");
        assert_eq!(mail_export.item_count, 1);
        let html = std::fs::read_to_string(&mail_export.path).expect("read html export");
        assert!(html.contains("&lt;b&gt;body&lt;/b&gt;"));

        let accounts_export = db
            .export_accounts(ExportAccountsInput {
                group_id: None,
                account_ids: None,
            })
            .expect("export accounts");
        assert_eq!(accounts_export.item_count, 1);
        let csv = std::fs::read_to_string(&accounts_export.path).expect("read csv export");
        assert!(csv.contains("one@example.com"));
    }

    #[test]
    fn creates_lists_and_revokes_local_mail_shares() {
        let root = std::env::temp_dir().join(format!("outlook-email-share-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("create temp dir");
        let conn = Connection::open_in_memory().expect("open memory db");
        let mut db = Database {
            conn,
            db_path: root.join("test.sqlite"),
            crypto_key: Some([7; 32]),
        };
        db.initialize_schema().expect("schema");
        db.conn
            .execute(
                "INSERT INTO accounts (id, email, status, group_id) VALUES (1, 'one@example.com', 'active', 1)",
                [],
            )
            .expect("insert account");
        db.conn
            .execute(
                "
                INSERT INTO retained_mail_messages
                (id, account_id, folder, provider_message_id, subject, sender, recipients,
                 received_at, received_at_sort, body_preview, body)
                VALUES (10, 1, 'inbox', 'share-message', 'Share me', 'sender@example.com',
                        'one@example.com', '2026-01-01T00:00:00Z', 1, 'preview', 'share body')
                ",
                [],
            )
            .expect("insert message");

        let share = db
            .create_mail_share(CreateMailShareInput {
                message_ids: vec![10],
                title: Some("Local share".to_string()),
                expires_in_days: Some(7),
            })
            .expect("create share");
        assert_eq!(share.account_email, "one@example.com");
        assert_eq!(share.item_count, 1);
        assert_eq!(share.status, "active");
        assert!(share.token_preview.len() >= 8);
        assert!(share.exported_path.ends_with(".html"));
        assert!(std::path::Path::new(&share.exported_path).exists());
        let html = std::fs::read_to_string(&share.exported_path).expect("read share html");
        assert!(html.contains("share body"));

        let shares = db.list_mail_share_records(Some(10)).expect("shares");
        assert_eq!(shares.len(), 1);
        assert_eq!(shares[0].id, share.id);
        assert_eq!(shares[0].status, "active");

        let revoked = db
            .revoke_mail_share(RevokeMailShareInput { share_id: share.id })
            .expect("revoke share");
        assert_eq!(revoked.status, "revoked");
        assert!(revoked.revoked_at.is_some());

        let expired = db
            .create_mail_share(CreateMailShareInput {
                message_ids: vec![10],
                title: Some("Expired share".to_string()),
                expires_in_days: Some(1),
            })
            .expect("create second share");
        db.conn
            .execute(
                "UPDATE email_share_links SET expires_at = datetime('now', '-1 day') WHERE id = ?",
                [expired.id],
            )
            .expect("expire share");
        let shares = db.list_mail_share_records(Some(10)).expect("shares after expire");
        let expired = shares.iter().find(|item| item.id == expired.id).expect("expired share");
        assert_eq!(expired.status, "expired");
    }

    #[test]
    fn reports_and_clears_local_retention_data() {
        let root = std::env::temp_dir().join(format!("outlook-email-retention-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("create temp dir");
        let conn = Connection::open_in_memory().expect("open memory db");
        let mut db = Database {
            conn,
            db_path: root.join("test.sqlite"),
            crypto_key: Some([7; 32]),
        };
        db.initialize_schema().expect("schema");
        db.conn
            .execute(
                "
                INSERT INTO accounts (id, email, status, group_id, provider, account_type)
                VALUES (1, 'local@example.com', 'active', 1, 'imap', 'imap')
                ",
                [],
            )
            .expect("insert account");
        db.conn
            .execute(
                "
                INSERT INTO retained_mail_messages
                (id, account_id, folder, provider_message_id, subject, received_at, received_at_sort,
                 is_read, body_cached, raw_mime)
                VALUES (10, 1, 'inbox', 'm1', 'Hello', '2026-01-01T00:00:00Z', 1, 0, 1, ?)
                ",
                params![b"raw message"],
            )
            .expect("insert retained message");
        db.conn
            .execute(
                "
                INSERT INTO temp_email_messages
                (message_id, email_address, from_address, subject, content, html_content, has_html, timestamp, raw_content)
                VALUES ('t1', 'temp@example.com', 'sender@example.com', 'Hi', 'body', '', 0, 1, 'raw')
                ",
                [],
            )
            .expect("insert temp message");

        let attachments = attachment_dir(&db.db_path).expect("attachment dir");
        std::fs::create_dir_all(&attachments).expect("create attachments");
        std::fs::write(attachments.join("one.txt"), b"attachment").expect("write attachment");
        let exports = exports_dir(&db.db_path).expect("exports dir").join("mail");
        std::fs::create_dir_all(&exports).expect("create exports");
        std::fs::write(exports.join("one.html"), b"export").expect("write export");

        let summary = db.local_retention_summary().expect("summary");
        assert_eq!(summary.mail_message_count, 1);
        assert_eq!(summary.unread_message_count, 1);
        assert_eq!(summary.raw_mime_count, 1);
        assert_eq!(summary.temp_message_count, 1);
        assert_eq!(summary.attachment_file_count, 1);
        assert_eq!(summary.export_file_count, 1);

        assert!(db
            .clear_local_data(ClearLocalDataInput {
                clear_mail_cache: Some(true),
                clear_temp_mail_cache: None,
                clear_attachments: None,
                clear_exports: None,
                confirm: "wrong".to_string(),
            })
            .is_err());

        let result = db
            .clear_local_data(ClearLocalDataInput {
                clear_mail_cache: Some(true),
                clear_temp_mail_cache: Some(true),
                clear_attachments: Some(true),
                clear_exports: Some(true),
                confirm: "CLEAR LOCAL DATA".to_string(),
            })
            .expect("clear local data");
        assert_eq!(result.deleted_messages, 1);
        assert_eq!(result.deleted_temp_messages, 1);
        assert_eq!(result.deleted_files, 2);
        assert!(result.freed_bytes > 0);

        let summary = db.local_retention_summary().expect("summary after clear");
        assert_eq!(summary.mail_message_count, 0);
        assert_eq!(summary.temp_message_count, 0);
        assert_eq!(summary.attachment_file_count, 0);
        assert_eq!(summary.export_file_count, 0);
    }

    #[test]
    fn downloads_imap_attachment_from_cached_raw_mime() {
        let root = std::env::temp_dir().join(format!("outlook-email-imap-attachment-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("create temp dir");
        let conn = Connection::open_in_memory().expect("open memory db");
        let mut db = Database {
            conn,
            db_path: root.join("test.sqlite"),
            crypto_key: Some([7; 32]),
        };
        db.initialize_schema().expect("schema");
        db.conn
            .execute(
                "
                INSERT INTO accounts (id, email, status, group_id, provider, account_type)
                VALUES (1, 'imap@example.com', 'active', 1, 'imap', 'imap')
                ",
                [],
            )
            .expect("insert account");
        let raw_mime = concat!(
            "From: sender@example.com\r\n",
            "To: imap@example.com\r\n",
            "Subject: Attachment test\r\n",
            "MIME-Version: 1.0\r\n",
            "Content-Type: multipart/mixed; boundary=\"mix\"\r\n",
            "\r\n",
            "--mix\r\n",
            "Content-Type: text/plain; charset=utf-8\r\n",
            "\r\n",
            "Body\r\n",
            "--mix\r\n",
            "Content-Type: text/plain; name=\"note.txt\"\r\n",
            "Content-Disposition: attachment; filename=\"note.txt\"\r\n",
            "Content-Transfer-Encoding: base64\r\n",
            "\r\n",
            "SGVsbG8gSU1BUCBhdHRhY2htZW50\r\n",
            "--mix--\r\n"
        );
        let attachments = serde_json::to_string(&vec![AttachmentInfo {
            id: "note.txt".to_string(),
            name: "note.txt".to_string(),
            content_type: "text/plain".to_string(),
            size: 21,
        }])
        .expect("attachments json");
        db.conn
            .execute(
                "
                INSERT INTO retained_mail_messages
                (id, account_id, folder, provider_message_id, subject, received_at, received_at_sort,
                 has_attachments, attachments_json, raw_mime)
                VALUES (10, 1, 'inbox', '42', 'Attachment test', '2026-01-01T00:00:00Z', 1,
                        1, ?, ?)
                ",
                params![attachments, raw_mime.as_bytes()],
            )
            .expect("insert message");

        let result = db
            .download_attachment(DownloadAttachmentInput {
                account_id: 1,
                message_id: "42".to_string(),
                attachment_id: "note.txt".to_string(),
                folder: Some("inbox".to_string()),
            })
            .expect("download attachment");

        assert_eq!(result.file_name, "note.txt");
        assert_eq!(std::fs::read(&result.path).expect("read attachment"), b"Hello IMAP attachment");

        let raw = db.get_mail_raw_content(10).expect("read raw content");
        assert_eq!(raw.message_id, 10);
        assert!(raw.file_name.ends_with(".eml"));
        assert!(raw.content.contains("Subject: Attachment test"));

        let bundle = db
            .download_all_attachments(DownloadAllAttachmentsInput {
                account_id: 1,
                message_id: "42".to_string(),
                folder: Some("inbox".to_string()),
            })
            .expect("download all attachments");
        assert_eq!(bundle.item_count, 1);
        assert!(bundle.file_name.ends_with(".zip"));
        let zip = std::fs::read(&bundle.path).expect("read zip bundle");
        assert!(zip.starts_with(b"PK\x03\x04"));
        assert!(zip.windows(b"note.txt".len()).any(|window| window == b"note.txt"));
        assert!(zip
            .windows(b"Hello IMAP attachment".len())
            .any(|window| window == b"Hello IMAP attachment"));
    }

    #[test]
    fn records_automation_run_for_failed_refresh() {
        let conn = Connection::open_in_memory().expect("open memory db");
        let mut db = Database {
            conn,
            db_path: PathBuf::from("memory.sqlite"),
            crypto_key: Some([7; 32]),
        };
        db.initialize_schema().expect("schema");

        let result = db.refresh_accounts(RefreshInput {
            account_id: None,
            folder: Some("all".to_string()),
            top: Some(10),
        });
        assert!(result.is_err());

        let runs = db.list_automation_runs(Some(10)).expect("runs");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].job_type, "refresh");
        assert_eq!(runs[0].trigger_type, "manual");
        assert_eq!(runs[0].status, "failed");
        assert_eq!(runs[0].failed, 1);
    }

    #[test]
    fn filters_and_clears_automation_runs() {
        let conn = Connection::open_in_memory().expect("open memory db");
        let mut db = Database {
            conn,
            db_path: PathBuf::from("memory.sqlite"),
            crypto_key: Some([7; 32]),
        };
        db.initialize_schema().expect("schema");
        db.conn
            .execute(
                "
                INSERT INTO automation_runs
                (job_type, trigger_type, status, message, refreshed, failed, duration_ms, started_at, finished_at)
                VALUES
                ('refresh', 'manual', 'failed', 'token expired', 0, 1, 10, '2026-01-01T00:00:00Z', '2026-01-01T00:00:01Z'),
                ('backup', 'schedule', 'success', 'uploaded', 1, 0, 20, '2026-01-02T00:00:00Z', '2026-01-02T00:00:01Z')
                ",
                [],
            )
            .expect("insert runs");

        let failed = db
            .list_automation_runs_query(AutomationRunQuery {
                status: Some("failed".to_string()),
                limit: Some(10),
                ..AutomationRunQuery::default()
            })
            .expect("filter failed");
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].job_type, "refresh");

        let cleared = db
            .clear_automation_runs(ClearAutomationRunsInput {
                job_type: None,
                trigger_type: None,
                status: Some("failed".to_string()),
                search: None,
                older_than_days: None,
                clear_all: None,
            })
            .expect("clear failed");
        assert_eq!(cleared.refreshed, 1);
        assert_eq!(db.list_automation_runs(Some(10)).expect("remaining").len(), 1);
    }

    #[test]
    fn settings_persist_theme_values_with_fallbacks() {
        let conn = Connection::open_in_memory().expect("open memory db");
        let mut db = Database {
            conn,
            db_path: PathBuf::from("memory.sqlite"),
            crypto_key: Some([7; 32]),
        };
        db.initialize_schema().expect("schema");

        let saved = db
            .update_settings(Settings {
                appearance_theme: "forest".to_string(),
                accent_color: "#0f766e".to_string(),
                ..Settings::default()
            })
            .expect("save theme");
        assert_eq!(saved.appearance_theme, "forest");
        assert_eq!(saved.accent_color, "#0f766e");

        let saved = db
            .update_settings(Settings {
                appearance_theme: "unexpected".to_string(),
                accent_color: "red".to_string(),
                ..Settings::default()
            })
            .expect("save fallback theme");
        assert_eq!(saved.appearance_theme, "default");
        assert_eq!(saved.accent_color, "#2563eb");
    }

    #[test]
    fn reports_automation_observability_and_forwarding_circuit() {
        let conn = Connection::open_in_memory().expect("open memory db");
        let mut db = Database {
            conn,
            db_path: PathBuf::from("memory.sqlite"),
            crypto_key: Some([7; 32]),
        };
        db.initialize_schema().expect("schema");
        db.update_settings(Settings {
            forwarding_enabled: true,
            forward_smtp_host: "smtp.example.com".to_string(),
            forward_smtp_to: "ops@example.com".to_string(),
            ..Settings::default()
        })
        .expect("settings");
        db.conn
            .execute(
                "INSERT INTO accounts (id, email, status, group_id) VALUES (1, 'one@example.com', 'active', 1)",
                [],
            )
            .expect("insert account");
        db.conn
            .execute(
                "
                INSERT INTO automation_runs
                (job_type, trigger_type, status, message, refreshed, failed, duration_ms, started_at, finished_at)
                VALUES
                ('refresh', 'manual', 'failed', 'invalid token', 0, 1, 10, '2026-01-01T00:00:00Z', '2026-01-01T00:00:01Z'),
                ('forwarding', 'schedule', 'failed', 'SMTP send failed: connection refused', 0, 1, 20, '2026-01-01T00:00:02Z', '2026-01-01T00:00:03Z'),
                ('retry', 'manual', 'success', 'Retried 1 item(s)', 1, 0, 30, '2026-01-01T00:00:04Z', '2026-01-01T00:00:05Z')
                ",
                [],
            )
            .expect("insert runs");
        for index in 0..3 {
            db.conn
                .execute(
                    "
                    INSERT INTO forwarding_logs
                    (account_id, account_email, message_id, channel, status, error_message)
                    VALUES (1, 'one@example.com', ?, 'smtp', 'failed', 'SMTP send failed: connection refused')
                    ",
                    [format!("message-{index}")],
                )
                .expect("insert forwarding log");
        }
        db.conn
            .execute(
                "
                INSERT INTO retry_queue
                (task_type, status, account_id, account_email, message_id, channel, action,
                 payload_json, error_message, attempts, max_attempts, next_attempt_at)
                VALUES
                ('forward_message', 'pending', 1, 'one@example.com', 'message-4', 'smtp', 'forward',
                 '{}', 'SMTP send failed: connection refused', 2, 7, datetime('now', '-1 minutes'))
                ",
                [],
            )
            .expect("insert retry");

        let observability = db.get_automation_observability().expect("observability");
        assert_eq!(observability.run_count, 3);
        assert_eq!(observability.failed_run_count, 2);
        assert_eq!(observability.retry_due_count, 1);
        assert!(observability
            .error_buckets
            .iter()
            .any(|bucket| bucket.category == "auth" && bucket.count >= 1));
        assert!(observability
            .error_buckets
            .iter()
            .any(|bucket| bucket.category == "network" && bucket.count >= 1));
        let smtp = observability
            .channel_circuits
            .iter()
            .find(|channel| channel.channel == "smtp")
            .expect("smtp channel");
        assert_eq!(smtp.status, "open");
        assert!(smtp.open_until.is_some());

        let retry = db
            .list_retry_queue(RetryQueueQuery::default())
            .expect("retry queue")
            .into_iter()
            .next()
            .expect("retry item");
        assert_eq!(retry.error_category, "network");
        assert!(retry.due_now);
    }

    #[test]
    fn queues_and_retries_failed_remote_mail_action() {
        let conn = Connection::open_in_memory().expect("open memory db");
        let mut db = Database {
            conn,
            db_path: PathBuf::from("memory.sqlite"),
            crypto_key: Some([7; 32]),
        };
        db.initialize_schema().expect("schema");
        db.conn
            .execute(
                "
                INSERT INTO accounts (id, email, status, group_id, provider, account_type)
                VALUES (1, 'one@example.com', 'active', 1, 'imap', 'imap')
                ",
                [],
            )
            .expect("insert account");
        db.conn
            .execute(
                "
                INSERT INTO retained_mail_messages
                (id, account_id, folder, provider_message_id, subject, received_at, received_at_sort, is_read)
                VALUES (10, 1, 'inbox', '42', 'Hello', '2026-01-01T00:00:00Z', 1, 0)
                ",
                [],
            )
            .expect("insert message");

        let marked = db
            .mark_mail_messages(MarkMailMessagesInput {
                message_ids: vec![10],
                is_read: true,
                sync_remote: Some(true),
            })
            .expect("mark with remote failure");
        assert!(!marked.success);
        assert_eq!(marked.failed, 1);

        let queued = db
            .list_retry_queue(RetryQueueQuery::default())
            .expect("retry queue");
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].task_type, "mail_mark");
        assert_eq!(queued[0].action, "mark_read");

        let messages = db
            .list_messages_query(MailMessageQuery {
                account_id: Some(1),
                folder: Some("inbox".to_string()),
                ..MailMessageQuery::default()
            })
            .expect("messages with remote failure");
        let failure = messages[0]
            .remote_sync_failure
            .as_ref()
            .expect("remote failure");
        assert_eq!(failure.retry_id, queued[0].id);
        assert_eq!(failure.action, "mark_read");
        assert_eq!(failure.status, "pending");

        let retried = db
            .run_retry_queue(Some(RetryQueueRunInput {
                retry_id: Some(queued[0].id),
                limit: None,
            }))
            .expect("retry item");
        assert_eq!(retried.failed, 1);

        let queued = db
            .list_retry_queue(RetryQueueQuery::default())
            .expect("retry queue after retry");
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].attempts, 1);
        assert!(queued[0].next_attempt_at.is_some());

        let dismissed = db
            .dismiss_retry_item(RetryQueueItemInput {
                retry_id: queued[0].id,
            })
            .expect("dismiss");
        assert_eq!(dismissed.refreshed, 1);
        assert!(db
            .list_retry_queue(RetryQueueQuery::default())
            .expect("empty queue")
            .is_empty());
        assert!(db
            .list_messages_query(MailMessageQuery {
                account_id: Some(1),
                folder: Some("inbox".to_string()),
                ..MailMessageQuery::default()
            })
            .expect("messages after dismiss")[0]
            .remote_sync_failure
            .is_none());
    }

    #[test]
    fn failed_remote_delete_keeps_message_visible_until_retry_success() {
        let conn = Connection::open_in_memory().expect("open memory db");
        let mut db = Database {
            conn,
            db_path: PathBuf::from("memory.sqlite"),
            crypto_key: Some([7; 32]),
        };
        db.initialize_schema().expect("schema");
        db.conn
            .execute(
                "
                INSERT INTO accounts (id, email, status, group_id, provider, account_type)
                VALUES (1, 'one@example.com', 'active', 1, 'imap', 'imap')
                ",
                [],
            )
            .expect("insert account");
        db.conn
            .execute(
                "
                INSERT INTO retained_mail_messages
                (id, account_id, folder, provider_message_id, subject, received_at, received_at_sort, is_read)
                VALUES (10, 1, 'inbox', '42', 'Hello', '2026-01-01T00:00:00Z', 1, 0)
                ",
                [],
            )
            .expect("insert message");

        let deleted = db
            .delete_mail_messages(DeleteMailMessagesInput {
                message_ids: vec![10],
                sync_remote: Some(true),
            })
            .expect("delete with remote failure");
        assert!(!deleted.success);
        assert_eq!(deleted.refreshed, 0);
        assert_eq!(deleted.failed, 1);

        let queued = db
            .list_retry_queue(RetryQueueQuery::default())
            .expect("retry queue");
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].task_type, "mail_delete");
        assert_eq!(queued[0].action, "delete");
        assert_eq!(queued[0].channel, "inbox");

        let messages = db
            .list_messages_query(MailMessageQuery {
                account_id: Some(1),
                folder: Some("inbox".to_string()),
                ..MailMessageQuery::default()
            })
            .expect("messages after failed delete");
        assert_eq!(messages.len(), 1);
        let failure = messages[0]
            .remote_sync_failure
            .as_ref()
            .expect("delete failure");
        assert_eq!(failure.retry_id, queued[0].id);
        assert_eq!(failure.task_type, "mail_delete");
        assert_eq!(failure.action, "delete");

        db.dismiss_retry_item(RetryQueueItemInput {
            retry_id: queued[0].id,
        })
        .expect("dismiss");
        assert!(db
            .list_messages_query(MailMessageQuery {
                account_id: Some(1),
                folder: Some("inbox".to_string()),
                ..MailMessageQuery::default()
            })
            .expect("messages after dismiss")[0]
            .remote_sync_failure
            .is_none());
    }

    #[test]
    fn successful_delete_retry_removes_cached_message() {
        let conn = Connection::open_in_memory().expect("open memory db");
        let mut db = Database {
            conn,
            db_path: PathBuf::from("memory.sqlite"),
            crypto_key: Some([7; 32]),
        };
        db.initialize_schema().expect("schema");
        db.conn
            .execute(
                "INSERT INTO accounts (id, email, status, group_id) VALUES (1, 'one@example.com', 'active', 1)",
                [],
            )
            .expect("insert account");
        db.conn
            .execute(
                "
                INSERT INTO retained_mail_messages
                (id, account_id, folder, provider_message_id, subject, received_at, received_at_sort, is_read)
                VALUES (10, 1, 'inbox', 'local-demo-delete', 'Hello', '2026-01-01T00:00:00Z', 1, 0)
                ",
                [],
            )
            .expect("insert message");
        let target = MailMessageRef {
            id: 10,
            account_id: 1,
            account_email: "one@example.com".to_string(),
            folder: "inbox".to_string(),
            provider_message_id: "local-demo-delete".to_string(),
        };
        db.enqueue_mail_delete_retry(&target, "previous remote failure")
            .expect("enqueue retry");
        let queued = db
            .list_retry_queue(RetryQueueQuery::default())
            .expect("retry queue");
        assert_eq!(queued.len(), 1);

        let retried = db
            .run_retry_queue(Some(RetryQueueRunInput {
                retry_id: Some(queued[0].id),
                limit: None,
            }))
            .expect("retry delete");
        assert!(retried.success);
        assert_eq!(retried.refreshed, 1);
        assert!(db
            .list_messages_query(MailMessageQuery {
                account_id: Some(1),
                folder: Some("inbox".to_string()),
                ..MailMessageQuery::default()
            })
            .expect("messages after retry")
            .is_empty());
    }

    #[test]
    fn queues_failed_temp_mail_refresh() {
        let conn = Connection::open_in_memory().expect("open memory db");
        let mut db = Database {
            conn,
            db_path: PathBuf::from("memory.sqlite"),
            crypto_key: Some([7; 32]),
        };
        db.initialize_schema().expect("schema");
        db.conn
            .execute(
                "
                INSERT INTO temp_emails
                (email, provider, status, channel_id, provider_token_enc, provider_account_id, provider_password_enc)
                VALUES ('temp@example.com', 'cloudflare', 'active', NULL, '', '', '')
                ",
                [],
            )
            .expect("insert temp email");

        let result = db.refresh_temp_email_messages("temp@example.com".to_string());
        assert!(result.is_err());

        let temp_email = db.list_temp_emails().expect("temp emails").remove(0);
        assert_eq!(temp_email.last_refresh_status, "failed");
        assert!(temp_email.last_refresh_error.is_some());

        let queued = db
            .list_retry_queue(RetryQueueQuery::default())
            .expect("retry queue");
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].task_type, "temp_refresh");
        assert_eq!(queued[0].message_id, "temp@example.com");
        assert_eq!(queued[0].channel, "cloudflare");

        let retried = db
            .run_retry_queue(Some(RetryQueueRunInput {
                retry_id: Some(queued[0].id),
                limit: None,
            }))
            .expect("retry temp refresh");
        assert_eq!(retried.failed, 1);
        let queued = db
            .list_retry_queue(RetryQueueQuery::default())
            .expect("retry queue after retry");
        assert_eq!(queued[0].attempts, 1);
    }

    #[test]
    fn temp_email_tags_are_saved_and_normalized() {
        let conn = Connection::open_in_memory().expect("open memory db");
        let mut db = Database {
            conn,
            db_path: PathBuf::from("memory.sqlite"),
            crypto_key: Some([7; 32]),
        };
        db.initialize_schema().expect("schema");
        db.conn
            .execute(
                "
                INSERT INTO temp_emails
                (email, provider, status, channel_id, provider_token_enc, provider_account_id, provider_password_enc)
                VALUES ('tagged@example.com', 'gptmail', 'active', NULL, '', '', '')
                ",
                [],
            )
            .expect("insert temp email");

        let updated = db
            .update_temp_email(UpdateTempEmailInput {
                email: "Tagged@Example.com".to_string(),
                tags: vec![
                    "Warmup".to_string(),
                    "warmup".to_string(),
                    "  Client A  ".to_string(),
                    "".to_string(),
                ],
            })
            .expect("update temp email");
        assert_eq!(updated.email, "tagged@example.com");
        assert_eq!(updated.tags, vec!["Warmup".to_string(), "Client A".to_string()]);
        let listed = db.list_temp_emails().expect("list temp emails");
        assert_eq!(listed[0].tags, updated.tags);
    }

    #[test]
    fn queues_failed_account_refresh_retry() {
        let conn = Connection::open_in_memory().expect("open memory db");
        let mut db = Database {
            conn,
            db_path: PathBuf::from("memory.sqlite"),
            crypto_key: Some([7; 32]),
        };
        db.initialize_schema().expect("schema");
        db.conn
            .execute(
                "
                INSERT INTO accounts (id, email, status, group_id, provider, account_type)
                VALUES (1, 'refresh@example.com', 'active', 1, 'imap', 'imap')
                ",
                [],
            )
            .expect("insert account");

        let result = db
            .refresh_accounts(RefreshInput {
                account_id: Some(1),
                folder: Some("inbox".to_string()),
                top: Some(10),
            })
            .expect("refresh result");
        assert!(!result.success);
        assert_eq!(result.failed, 1);

        let queued = db
            .list_retry_queue(RetryQueueQuery::default())
            .expect("retry queue");
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].task_type, "refresh_account");
        assert_eq!(queued[0].account_id, Some(1));
        assert_eq!(queued[0].message_id, "inbox");
        let refresh_logs = db
            .list_refresh_logs(Some(1), Some(10))
            .expect("refresh logs");
        assert_eq!(refresh_logs.len(), 1);
        assert_eq!(refresh_logs[0].account_email, "refresh@example.com");
        assert_eq!(refresh_logs[0].status, "failed");

        let retried = db
            .run_retry_queue(Some(RetryQueueRunInput {
                retry_id: Some(queued[0].id),
                limit: None,
            }))
            .expect("retry refresh");
        assert_eq!(retried.failed, 1);
        let queued = db
            .list_retry_queue(RetryQueueQuery::default())
            .expect("retry queue after retry");
        assert_eq!(queued[0].attempts, 1);
    }

    #[test]
    fn queues_failed_backup_retry() {
        let conn = Connection::open_in_memory().expect("open memory db");
        let mut db = Database {
            conn,
            db_path: PathBuf::from("memory.sqlite"),
            crypto_key: Some([7; 32]),
        };
        db.initialize_schema().expect("schema");

        let result = db.run_backup_job();
        assert!(result.is_err());

        let queued = db
            .list_retry_queue(RetryQueueQuery::default())
            .expect("retry queue");
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].task_type, "backup_job");
        assert_eq!(queued[0].message_id, "webdav");

        let retried = db
            .run_retry_queue(Some(RetryQueueRunInput {
                retry_id: Some(queued[0].id),
                limit: None,
            }))
            .expect("retry backup");
        assert_eq!(retried.failed, 1);
        let queued = db
            .list_retry_queue(RetryQueueQuery::default())
            .expect("retry queue after retry");
        assert_eq!(queued[0].attempts, 1);
    }

    #[test]
    fn restores_database_from_local_backup_snapshot() {
        let root = std::env::temp_dir().join(format!("outlook-email-restore-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("create temp dir");
        let db_path = root.join("test.sqlite");
        let conn = Connection::open(&db_path).expect("open file db");
        let mut db = Database {
            conn,
            db_path,
            crypto_key: Some([7; 32]),
        };
        db.initialize_schema().expect("schema");
        db.conn
            .execute(
                "INSERT INTO accounts (id, email, status, group_id) VALUES (1, 'before@example.com', 'active', 1)",
                [],
            )
            .expect("insert first account");

        let backup_dir = backup_dir(&db.db_path).expect("backup dir");
        std::fs::create_dir_all(&backup_dir).expect("create backup dir");
        let file_name = "outlook-email-test-restore.sqlite";
        let backup_path = backup_dir.join(file_name);
        let backup_path_text = backup_path.to_string_lossy().to_string();
        db.conn
            .execute("VACUUM INTO ?", [backup_path_text.as_str()])
            .expect("vacuum backup");
        let size = backup_path.metadata().expect("backup metadata").len() as i64;
        db.insert_backup_log("local-test", "success", file_name, size, None)
            .expect("backup log");

        db.conn
            .execute(
                "INSERT INTO accounts (id, email, status, group_id) VALUES (2, 'after@example.com', 'active', 1)",
                [],
            )
            .expect("insert second account");
        assert_eq!(db.list_accounts().expect("accounts before restore").len(), 2);

        let result = db
            .restore_backup(RestoreBackupInput {
                backup_log_id: 1,
                confirm: true,
            })
            .expect("restore backup");
        assert!(result.success);
        assert!(std::path::Path::new(&result.safety_backup_path).exists());
        let accounts = db.list_accounts().expect("accounts after restore");
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].email, "before@example.com");
    }
}
