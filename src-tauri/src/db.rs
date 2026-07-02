use crate::automation;
use crate::crypto;
use crate::error::{AppError, AppResult};
use crate::import::ImportedAccount;
use crate::models::*;
use crate::providers;
use chrono::{DateTime, NaiveDateTime, Utc};
use directories::ProjectDirs;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};

pub struct Database {
    conn: Connection,
    db_path: PathBuf,
    crypto_key: Option<[u8; 32]>,
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
            SELECT g.id, g.name, COALESCE(g.description, ''), g.color, g.parent_id, g.level,
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
                parent_id: row.get(4)?,
                level: row.get(5)?,
                sort_order: row.get(6)?,
                account_count: row.get(7)?,
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
            INSERT INTO groups (name, description, color, parent_id, level, sort_order)
            VALUES (?, ?, ?, ?, ?, COALESCE((SELECT MAX(sort_order) + 1 FROM groups), 0))
            ",
            params![
                name,
                input.description.unwrap_or_default(),
                input.color.unwrap_or_else(|| "#2f6f9f".to_string()),
                input.parent_id,
                level
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        self.audit("group.created", "group", Some(id), name)?;
        self.get_group(id)
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
                   a.last_refresh_error, COUNT(m.id) AS message_count, a.created_at, a.updated_at,
                   a.password_enc, a.refresh_token_enc, a.imap_password_enc, COALESCE(a.imap_host, ''),
                   a.imap_port
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
                message_count: row.get(11)?,
                created_at: row.get(12)?,
                updated_at: row.get(13)?,
                tags: Vec::new(),
                has_password: !row.get::<_, String>(14)?.is_empty(),
                has_refresh_token: !row.get::<_, String>(15)?.is_empty(),
                has_imap_password: !row.get::<_, String>(16)?.is_empty(),
                imap_host: row.get(17)?,
                imap_port: row.get(18)?,
            })
        })?;

        let mut accounts = collect_rows(rows)?;
        for account in &mut accounts {
            account.tags = self.tags_for_account(account.id)?;
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

    pub fn list_projects(&self) -> AppResult<Vec<Project>> {
        self.require_unlocked()?;
        let mut stmt = self.conn.prepare(
            "
            SELECT id, name, project_key, COALESCE(description, ''), scope_mode, status, created_at, updated_at
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
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
            ))
        })?;
        let mut projects = Vec::new();
        for row in rows {
            let (id, name, project_key, description, scope_mode, status, created_at, updated_at) = row?;
            projects.push(Project {
                id,
                name,
                project_key,
                description,
                scope_mode,
                status,
                group_ids: self.project_group_ids(id)?,
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
        if scope_mode != "all" && scope_mode != "groups" {
            return Err(AppError::InvalidInput("project scope_mode must be all or groups".to_string()));
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
            INSERT INTO projects (name, project_key, description, scope_mode, status)
            VALUES (?, ?, ?, ?, 'active')
            ",
            params![name, project_key, input.description.unwrap_or_default(), scope_mode],
        )?;
        let project_id = self.conn.last_insert_rowid();
        self.replace_project_group_scope(project_id, input.group_ids.unwrap_or_default())?;
        self.sync_project_scope(project_id)?;
        self.audit("project.created", "project", Some(project_id), name)?;
        self.get_project(project_id)
    }

    pub fn sync_project_scope(&self, project_id: i64) -> AppResult<Project> {
        self.require_unlocked()?;
        let scope_mode: String = self
            .conn
            .query_row("SELECT scope_mode FROM projects WHERE id = ?", [project_id], |row| row.get(0))
            .optional()?
            .ok_or_else(|| AppError::InvalidInput("project not found".to_string()))?;
        let group_ids = self.project_group_ids(project_id)?;
        let accounts = self.accounts_for_project_scope(&scope_mode, &group_ids)?;
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
        self.require_unlocked()?;
        let folder = folder.unwrap_or_else(|| "all".to_string());
        let sql = String::from(
            "
            SELECT id, account_id, folder, provider_message_id, subject, sender, recipients,
                   received_at, is_read, has_attachments, body_preview, body, body_type,
                   attachments_json
            FROM retained_mail_messages
            WHERE (?1 IS NULL OR account_id = ?1)
              AND (?2 = 'all' OR folder = ?2)
            ORDER BY received_at_sort DESC, id DESC
            LIMIT 200
            ",
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(
            params![account_id, folder],
            |row| {
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
                })
            },
        )?;
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

    pub fn get_settings(&self) -> AppResult<Settings> {
        self.require_unlocked()?;
        let mut settings = Settings::default();
        settings.graph_client_id = self.get_config("graph_client_id")?.unwrap_or_default();
        settings.oauth_redirect_uri = self
            .get_config("oauth_redirect_uri")?
            .unwrap_or(settings.oauth_redirect_uri);
        settings.gptmail_base_url = self
            .get_config("gptmail_base_url")?
            .unwrap_or(settings.gptmail_base_url);
        settings.duckmail_base_url = self
            .get_config("duckmail_base_url")?
            .unwrap_or(settings.duckmail_base_url);
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
        Ok(settings)
    }

    pub fn update_settings(&self, settings: Settings) -> AppResult<Settings> {
        self.require_unlocked()?;
        self.set_config("graph_client_id", &settings.graph_client_id)?;
        self.set_config("oauth_redirect_uri", &settings.oauth_redirect_uri)?;
        self.set_config("gptmail_base_url", &settings.gptmail_base_url)?;
        self.set_config("duckmail_base_url", &settings.duckmail_base_url)?;
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
        self.audit("settings.updated", "settings", None, "")?;
        self.get_settings()
    }

    pub fn exchange_oauth_token(&self, input: OAuthExchangeInput) -> AppResult<OAuthTokenResult> {
        self.require_unlocked()?;
        let token = providers::exchange_graph_code(&input.client_id, &input.redirect_uri, &input.code_or_url)?;
        if let Some(account_id) = input.account_id {
            let key = self.crypto_key.as_ref().ok_or(AppError::Unauthorized)?;
            let refresh_token = crypto::encrypt_text(&token.refresh_token, key)?;
            let client_id = crypto::encrypt_text(&input.client_id, key)?;
            self.conn.execute(
                "
                UPDATE accounts
                SET client_id_enc = ?,
                    refresh_token_enc = ?,
                    provider = 'graph',
                    last_refresh_status = 'authorized',
                    last_refresh_error = NULL,
                    refresh_token_updated_at = CURRENT_TIMESTAMP,
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = ?
                ",
                params![client_id, refresh_token, account_id],
            )?;
            self.audit("oauth.graph.exchanged", "account", Some(account_id), "")?;
        }
        Ok(OAuthTokenResult {
            success: true,
            account_id: input.account_id,
            scope: token.scope,
            expires_in: token.expires_in,
            refresh_token_preview: preview_secret(&token.refresh_token),
        })
    }

    pub fn refresh_accounts(&self, input: RefreshInput) -> AppResult<JobResult> {
        self.require_unlocked()?;
        let credentials = self.account_credentials(input.account_id)?;
        if credentials.is_empty() {
            return Err(AppError::InvalidInput("no matching accounts to refresh".to_string()));
        }

        let folder = input.folder.unwrap_or_else(|| "all".to_string());
        let top = input.top.unwrap_or(25).clamp(1, 50);
        let mut refreshed = 0_usize;
        let mut failed = 0_usize;
        let mut errors = Vec::new();

        for account in credentials {
            let result = if should_use_graph(&account) {
                providers::fetch_graph_messages(&account, &folder, top).and_then(|(next_refresh_token, messages)| {
                    if !next_refresh_token.is_empty() && next_refresh_token != account.refresh_token {
                        self.save_refresh_token(account.id, &next_refresh_token)?;
                    }
                    self.upsert_provider_messages(account.id, &messages)?;
                    Ok(messages.len())
                })
            } else {
                providers::fetch_imap_messages(&account, &folder, top).and_then(|messages| {
                    self.upsert_provider_messages(account.id, &messages)?;
                    Ok(messages.len())
                })
            };

            match result {
                Ok(count) => {
                    refreshed += 1;
                    self.mark_account_refresh_success(account.id, &account.email, count)?;
                }
                Err(err) => {
                    failed += 1;
                    let message = err.to_string();
                    errors.push(format!("{}: {}", account.email, message));
                    self.mark_account_refresh_failed(account.id, &account.email, &message)?;
                }
            }
        }

        Ok(JobResult {
            success: failed == 0,
            message: if errors.is_empty() {
                format!("Refreshed {} account(s)", refreshed)
            } else {
                format!("Refreshed {} account(s), {} failed: {}", refreshed, failed, errors.join("; "))
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
        if !should_use_graph(&account) {
            return Err(AppError::InvalidInput(
                "attachment download is currently implemented for Graph accounts".to_string(),
            ));
        }
        let attachment = providers::download_graph_attachment(&account, &input.message_id, &input.attachment_id)?;
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

    pub fn run_forwarding_job(&self, input: Option<ForwardingInput>) -> AppResult<JobResult> {
        self.require_unlocked()?;
        let settings = self.get_settings()?;
        let channels = automation::configured_forward_channels(&settings);
        if channels.is_empty() {
            return Err(AppError::InvalidInput(
                "configure at least one forwarding channel first".to_string(),
            ));
        }
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
        let mut errors = Vec::new();

        for (account_id, account_email) in accounts {
            let messages = self.forwarding_candidates(account_id, limit)?;
            for message in messages {
                for channel in &channels {
                    if self.forward_success_exists(account_id, &message.message_id, channel)? {
                        skipped += 1;
                        continue;
                    }
                    match automation::forward_message(&settings, channel, &message) {
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
            &format!("{forwarded} success, {failed} failed, {skipped} skipped"),
        )?;
        Ok(JobResult {
            success: failed == 0,
            message: if errors.is_empty() {
                format!("Forwarded {forwarded} message channel(s), skipped {skipped}")
            } else {
                format!(
                    "Forwarded {forwarded} message channel(s), {failed} failed: {}",
                    errors.join("; ")
                )
            },
            refreshed: forwarded,
            failed,
        })
    }

    pub fn run_backup_job(&self) -> AppResult<BackupResult> {
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

    pub fn scheduler_status(&self) -> AppResult<SchedulerStatus> {
        self.require_unlocked()?;
        Ok(SchedulerStatus {
            last_refresh_at: self.get_config("scheduler_last_refresh_at")?,
            last_forwarding_at: self.get_config("scheduler_last_forwarding_at")?,
            last_backup_at: self.get_config("scheduler_last_backup_at")?,
        })
    }

    pub fn run_due_scheduled_jobs(&self) -> AppResult<()> {
        if !self.is_unlocked() {
            return Ok(());
        }
        let settings = self.get_settings()?;
        let now = Utc::now();

        if settings.scheduler_refresh_enabled
            && self.scheduler_due("scheduler_last_refresh_at", settings.scheduler_refresh_interval_minutes, now)?
        {
            match self.refresh_accounts(RefreshInput {
                account_id: None,
                folder: Some("all".to_string()),
                top: Some(settings.scheduler_refresh_top.clamp(1, 50) as usize),
            }) {
                Ok(result) => self.audit("scheduler.refresh", "scheduler", None, &result.message)?,
                Err(err) => self.audit("scheduler.refresh_failed", "scheduler", None, &err.to_string())?,
            }
            self.set_config("scheduler_last_refresh_at", &now.to_rfc3339())?;
        }

        if settings.forwarding_enabled
            && self.scheduler_due("scheduler_last_forwarding_at", settings.forwarding_interval_minutes, now)?
        {
            match self.run_forwarding_job(Some(ForwardingInput {
                account_id: None,
                limit: Some(50),
            })) {
                Ok(result) => self.audit("scheduler.forwarding", "scheduler", None, &result.message)?,
                Err(err) => self.audit("scheduler.forwarding_failed", "scheduler", None, &err.to_string())?,
            }
            self.set_config("scheduler_last_forwarding_at", &now.to_rfc3339())?;
        }

        if settings.backup_enabled
            && self.scheduler_due("scheduler_last_backup_at", settings.backup_interval_minutes, now)?
        {
            match self.run_backup_job() {
                Ok(result) => self.audit("scheduler.backup", "scheduler", None, &result.message)?,
                Err(err) => self.audit("scheduler.backup_failed", "scheduler", None, &err.to_string())?,
            }
            self.set_config("scheduler_last_backup_at", &now.to_rfc3339())?;
        }

        Ok(())
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
            CREATE INDEX IF NOT EXISTS idx_project_accounts_project_status ON project_accounts(project_id, status);
            CREATE INDEX IF NOT EXISTS idx_project_events_project_created ON project_account_events(project_id, created_at DESC);
            CREATE INDEX IF NOT EXISTS idx_forwarding_logs_message ON forwarding_logs(account_id, message_id, channel, status);
            CREATE INDEX IF NOT EXISTS idx_backup_logs_created ON backup_logs(created_at DESC);
            CREATE INDEX IF NOT EXISTS idx_audit_created ON audit_logs(created_at DESC);
            ",
        )?;
        self.ensure_default_data()?;
        self.ensure_account_columns()?;
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
                SELECT g.id, g.name, COALESCE(g.description, ''), g.color, g.parent_id, g.level,
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
                        parent_id: row.get(4)?,
                        level: row.get(5)?,
                        sort_order: row.get(6)?,
                        account_count: row.get(7)?,
                    })
                },
            )
            .map_err(AppError::from)
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

    fn project_group_ids(&self, project_id: i64) -> AppResult<Vec<i64>> {
        let mut stmt = self
            .conn
            .prepare("SELECT group_id FROM project_group_scopes WHERE project_id = ? ORDER BY group_id")?;
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

    fn accounts_for_project_scope(&self, scope_mode: &str, group_ids: &[i64]) -> AppResult<Vec<(i64, String)>> {
        if scope_mode == "groups" && group_ids.is_empty() {
            return Ok(Vec::new());
        }
        let sql = if scope_mode == "groups" {
            format!(
                "SELECT id, email FROM accounts WHERE status = 'active' AND group_id IN ({}) ORDER BY email",
                std::iter::repeat("?")
                    .take(group_ids.len())
                    .collect::<Vec<_>>()
                    .join(",")
            )
        } else {
            "SELECT id, email FROM accounts WHERE status = 'active' ORDER BY email".to_string()
        };
        let mut stmt = self.conn.prepare(&sql)?;
        if scope_mode == "groups" {
            let params = rusqlite::params_from_iter(group_ids.iter());
            let rows = stmt.query_map(params, |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?;
            collect_rows(rows)
        } else {
            let rows = stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?;
            collect_rows(rows)
        }
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
            SELECT id, email, provider, account_type, password_enc, client_id_enc,
                   refresh_token_enc, COALESCE(imap_host, ''), imap_port, imap_password_enc
            FROM accounts
            WHERE status = 'active' AND (?1 IS NULL OR id = ?1)
            ORDER BY sort_order ASC, email ASC
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
            ) = row?;
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
            });
        }
        Ok(credentials)
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
                 body_type, attachments_json, body_cached, last_synced_at, updated_at)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
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
                    if message.body.is_some() { 1 } else { 0 },
                ],
            )?;
        }
        Ok(())
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

fn parse_attachments_json(value: &str) -> Vec<AttachmentInfo> {
    serde_json::from_str(value).unwrap_or_default()
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

#[cfg(test)]
mod project_tests {
    use super::{normalize_project_key, Database};
    use crate::models::{ClaimProjectAccountInput, CreateProjectInput, ProjectAccountActionInput};
    use rusqlite::Connection;
    use std::path::PathBuf;

    #[test]
    fn normalizes_project_key() {
        assert_eq!(normalize_project_key(" My Project 01 "), "my-project-01");
        assert_eq!(normalize_project_key("中文项目"), "");
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
                group_ids: None,
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
}
