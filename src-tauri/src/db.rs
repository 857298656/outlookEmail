use crate::crypto;
use crate::error::{AppError, AppResult};
use crate::import::ImportedAccount;
use crate::models::*;
use crate::providers;
use crate::temp_mail::{self, CloudflareChannelCredentials, TempMailboxCredentials};
use chrono::{DateTime, Duration as ChronoDuration, NaiveDateTime, Utc};
use directories::ProjectDirs;
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub struct Database {
    conn: Connection,
    db_path: PathBuf,
    crypto_key: Option<[u8; 32]>,
}

const CONFIG_SECRET_KEYS: &[&str] = &[];

const WORKSPACE_KEY_CONFIG: &str = "workspace_key_enc";

#[derive(Debug, Clone)]
struct MailMessageRef {
    id: i64,
    account_id: i64,
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

    pub fn initialize_app(&mut self, _password: &str) -> AppResult<()> {
        validate_password(DEFAULT_LOGIN_PASSWORD)?;
        if self.is_initialized()? {
            return Err(AppError::InvalidInput(
                "app is already initialized".to_string(),
            ));
        }

        let workspace_key = crypto::random_workspace_key();
        let runtime_key = crypto::derive_workspace_key(&workspace_key)?;
        let hash = crypto::hash_password(DEFAULT_LOGIN_PASSWORD)?;
        let salt = crypto::random_salt();
        let password_key = crypto::derive_key(DEFAULT_LOGIN_PASSWORD, &salt);
        let workspace_key_enc = crypto::encrypt_text(&workspace_key, &password_key)?;
        self.set_config("password_hash", &hash)?;
        self.set_config("crypto_salt", &salt)?;
        self.set_config(WORKSPACE_KEY_CONFIG, &workspace_key_enc)?;
        self.crypto_key = Some(runtime_key);
        self.ensure_default_data()?;
        self.audit("app.initialized", "settings", None, "initial setup")?;
        Ok(())
    }

    pub fn login(&mut self, input: LoginInput) -> AppResult<()> {
        validate_login_username(&input.username)?;
        if self.is_initialized()? {
            return self.unlock(&input.password);
        }
        if input.password != DEFAULT_LOGIN_PASSWORD {
            return Err(AppError::Unauthorized);
        }
        self.initialize_app(DEFAULT_LOGIN_PASSWORD)
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
        let password_key = crypto::derive_key(password, &salt);
        if let Some(workspace_key_enc) = self.get_config(WORKSPACE_KEY_CONFIG)? {
            let workspace_key = crypto::decrypt_text(&workspace_key_enc, &password_key)?;
            self.crypto_key = Some(crypto::derive_workspace_key(&workspace_key)?);
        } else {
            self.migrate_legacy_password_key(&password_key)?;
        }
        self.migrate_legacy_account_secrets()?;
        self.drop_legacy_account_secret_columns()?;
        self.audit("app.unlocked", "session", None, "local unlock")?;
        Ok(())
    }

    pub fn update_login_password(&mut self, input: UpdateLoginPasswordInput) -> AppResult<()> {
        self.require_unlocked()?;
        validate_password(&input.new_password)?;
        let hash = self
            .get_config("password_hash")?
            .ok_or_else(|| AppError::InvalidInput("app is not initialized".to_string()))?;
        if !crypto::verify_password(&input.current_password, &hash)? {
            return Err(AppError::Unauthorized);
        }
        let current_salt = self
            .get_config("crypto_salt")?
            .ok_or_else(|| AppError::Crypto("missing crypto salt".to_string()))?;
        let migrated_legacy_key = if self.get_config(WORKSPACE_KEY_CONFIG)?.is_none() {
            let legacy_key = crypto::derive_key(&input.current_password, &current_salt);
            self.migrate_legacy_password_key(&legacy_key)?;
            true
        } else {
            false
        };
        let current_salt = self
            .get_config("crypto_salt")?
            .ok_or_else(|| AppError::Crypto("missing crypto salt".to_string()))?;
        let workspace_key_enc = self
            .get_config(WORKSPACE_KEY_CONFIG)?
            .ok_or_else(|| AppError::Crypto("missing workspace key".to_string()))?;
        let current_password = if migrated_legacy_key {
            DEFAULT_LOGIN_PASSWORD
        } else {
            input.current_password.as_str()
        };
        let current_password_key = crypto::derive_key(current_password, &current_salt);
        let workspace_key = crypto::decrypt_text(&workspace_key_enc, &current_password_key)?;
        let new_hash = crypto::hash_password(&input.new_password)?;
        let new_salt = crypto::random_salt();
        let new_password_key = crypto::derive_key(&input.new_password, &new_salt);
        let new_workspace_key_enc = crypto::encrypt_text(&workspace_key, &new_password_key)?;

        let tx = self.conn.transaction()?;
        tx.execute(
            "
            INSERT INTO app_config (key, value, updated_at)
            VALUES ('password_hash', ?, CURRENT_TIMESTAMP)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = CURRENT_TIMESTAMP
            ",
            [new_hash.as_str()],
        )?;
        tx.execute(
            "
            INSERT INTO app_config (key, value, updated_at)
            VALUES ('crypto_salt', ?, CURRENT_TIMESTAMP)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = CURRENT_TIMESTAMP
            ",
            [new_salt.as_str()],
        )?;
        tx.execute(
            "
            INSERT INTO app_config (key, value, updated_at)
            VALUES (?, ?, CURRENT_TIMESTAMP)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = CURRENT_TIMESTAMP
            ",
            params![WORKSPACE_KEY_CONFIG, new_workspace_key_enc],
        )?;
        tx.commit()?;

        self.audit(
            "app.password_updated",
            "settings",
            None,
            "login password changed",
        )?;
        Ok(())
    }

    pub fn lock(&mut self) {
        self.crypto_key = None;
    }

    pub fn list_workspace_key_records(&self) -> AppResult<Vec<WorkspaceKeyRecord>> {
        self.require_unlocked()?;
        let mut stmt = self.conn.prepare(
            "
            SELECT id, purpose, key_fingerprint, created_at
            FROM workspace_key_records
            ORDER BY created_at DESC, id DESC
            ",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(WorkspaceKeyRecord {
                id: row.get(0)?,
                purpose: row.get(1)?,
                key_fingerprint: row.get(2)?,
                created_at: row.get(3)?,
            })
        })?;
        collect_rows(rows)
    }

    pub fn generate_workspace_key(
        &self,
        input: GenerateWorkspaceKeyInput,
    ) -> AppResult<GenerateWorkspaceKeyResult> {
        self.require_unlocked()?;
        let purpose = input.purpose.trim();
        let purpose = if purpose.is_empty() {
            self.next_default_workspace_key_purpose()?
        } else {
            purpose.to_string()
        };
        let workspace_key = crypto::random_workspace_key();
        let key_fingerprint = crypto::workspace_key_fingerprint(&workspace_key);
        self.conn.execute(
            "
            INSERT INTO workspace_key_records (purpose, key_fingerprint)
            VALUES (?, ?)
            ",
            params![purpose, key_fingerprint],
        )?;
        let id = self.conn.last_insert_rowid();
        let record = self
            .conn
            .query_row(
                "
                SELECT id, purpose, key_fingerprint, created_at
                FROM workspace_key_records
                WHERE id = ?
                ",
                [id],
                |row| {
                    Ok(WorkspaceKeyRecord {
                        id: row.get(0)?,
                        purpose: row.get(1)?,
                        key_fingerprint: row.get(2)?,
                        created_at: row.get(3)?,
                    })
                },
            )
            .map_err(|_| {
                AppError::Internal("failed to load generated workspace key record".to_string())
            })?;
        self.audit(
            "workspace_key.generated",
            "workspace_key",
            Some(id),
            &purpose,
        )?;
        Ok(GenerateWorkspaceKeyResult {
            record,
            workspace_key,
        })
    }

    fn next_default_workspace_key_purpose(&self) -> AppResult<String> {
        let mut stmt = self
            .conn
            .prepare("SELECT purpose FROM workspace_key_records")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut max_index = 0_u64;
        for purpose in collect_rows(rows)? {
            if let Some(suffix) = purpose.strip_prefix("密钥_") {
                if let Ok(index) = suffix.parse::<u64>() {
                    max_index = max_index.max(index);
                }
            }
        }
        Ok(format!("密钥_{}", max_index + 1))
    }

    pub fn update_workspace_key_record(
        &self,
        input: UpdateWorkspaceKeyRecordInput,
    ) -> AppResult<WorkspaceKeyRecord> {
        self.require_unlocked()?;
        let existing_purpose: String = self
            .conn
            .query_row(
                "SELECT purpose FROM workspace_key_records WHERE id = ?",
                [input.id],
                |row| row.get(0),
            )
            .map_err(|_| AppError::InvalidInput("workspace key record not found".to_string()))?;
        let trimmed = input.purpose.trim();
        let purpose = if trimmed.is_empty() {
            existing_purpose
        } else {
            trimmed.to_string()
        };
        let changed = self.conn.execute(
            "UPDATE workspace_key_records SET purpose = ? WHERE id = ?",
            params![purpose, input.id],
        )?;
        if changed == 0 {
            return Err(AppError::InvalidInput(
                "workspace key record not found".to_string(),
            ));
        }
        let record = self
            .conn
            .query_row(
                "
                SELECT id, purpose, key_fingerprint, created_at
                FROM workspace_key_records
                WHERE id = ?
                ",
                [input.id],
                |row| {
                    Ok(WorkspaceKeyRecord {
                        id: row.get(0)?,
                        purpose: row.get(1)?,
                        key_fingerprint: row.get(2)?,
                        created_at: row.get(3)?,
                    })
                },
            )
            .map_err(|_| AppError::Internal("failed to load workspace key record".to_string()))?;
        self.audit(
            "workspace_key.updated",
            "workspace_key",
            Some(input.id),
            &record.purpose,
        )?;
        Ok(record)
    }

    pub fn delete_workspace_key_record(&self, record_id: i64) -> AppResult<()> {
        self.require_unlocked()?;
        let changed = self.conn.execute(
            "DELETE FROM workspace_key_records WHERE id = ?",
            [record_id],
        )?;
        if changed == 0 {
            return Err(AppError::InvalidInput(
                "workspace key record not found".to_string(),
            ));
        }
        self.audit(
            "workspace_key.deleted",
            "workspace_key",
            Some(record_id),
            "record deleted",
        )?;
        Ok(())
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
            SELECT g.id, g.name, COALESCE(g.description, ''), g.parent_id, g.level,
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
                parent_id: row.get(3)?,
                level: row.get(4)?,
                sort_order: row.get(5)?,
                account_count: row.get(6)?,
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
                .query_row(
                    "SELECT level FROM groups WHERE id = ?",
                    [parent_id],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?
                .ok_or_else(|| AppError::InvalidInput("parent group not found".to_string()))?,
            None => 0,
        };
        let level = parent_level + 1;
        if level > 3 {
            return Err(AppError::InvalidInput(
                "groups support at most 3 levels".to_string(),
            ));
        }

        self.conn.execute(
            "
            INSERT INTO groups
            (name, description, parent_id, level, sort_order)
            VALUES (?, ?, ?, ?, COALESCE((SELECT MAX(sort_order) + 1 FROM groups), 0))
            ",
            params![
                name,
                input.description.unwrap_or_default(),
                input.parent_id,
                level
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        self.audit("group.created", "group", Some(id), name)?;
        self.get_group(id)
    }

    pub fn update_group(&self, input: UpdateGroupInput) -> AppResult<Group> {
        self.require_unlocked()?;
        let name = input.name.trim();
        if name.is_empty() {
            return Err(AppError::InvalidInput("group name is required".to_string()));
        }
        let (current_parent_id, current_level): (Option<i64>, i64) = self
            .conn
            .query_row(
                "SELECT parent_id, level FROM groups WHERE id = ?",
                [input.id],
                |row| Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?
            .ok_or_else(|| AppError::InvalidInput("group not found".to_string()))?;
        if input.parent_id == Some(input.id) {
            return Err(AppError::InvalidInput(
                "group cannot be its own parent".to_string(),
            ));
        }
        if let Some(parent_id) = input.parent_id {
            if self.group_descendant_ids(input.id)?.contains(&parent_id) {
                return Err(AppError::InvalidInput(
                    "group cannot move under its descendant".to_string(),
                ));
            }
        }
        let parent_level = match input.parent_id {
            Some(parent_id) => self
                .conn
                .query_row(
                    "SELECT level FROM groups WHERE id = ?",
                    [parent_id],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?
                .ok_or_else(|| AppError::InvalidInput("parent group not found".to_string()))?,
            None => 0,
        };
        let new_level = parent_level + 1;
        let subtree_depth = self.group_subtree_depth(input.id, current_level)?;
        if new_level + subtree_depth > 3 {
            return Err(AppError::InvalidInput(
                "groups support at most 3 levels".to_string(),
            ));
        }

        self.conn.execute(
            "
            UPDATE groups
            SET name = ?,
                description = ?,
                parent_id = ?,
                level = ?,
                sort_order = ?
            WHERE id = ?
            ",
            params![
                name,
                input.description.unwrap_or_default(),
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
            return Err(AppError::InvalidInput(
                "system group cannot be deleted".to_string(),
            ));
        }
        let descendants = self.group_descendant_ids(group_id)?;
        self.conn.execute(
            "UPDATE accounts SET group_id = ? WHERE group_id = ?",
            params![parent_id, group_id],
        )?;
        self.conn.execute(
            "UPDATE groups SET parent_id = ? WHERE parent_id = ?",
            params![parent_id, group_id],
        )?;
        if !descendants.is_empty() {
            self.shift_group_levels(&descendants, -1)?;
        }
        self.conn
            .execute("DELETE FROM groups WHERE id = ?", [group_id])?;
        self.audit(
            "group.deleted",
            "group",
            Some(group_id),
            &format!("level {level}"),
        )?;
        Ok(())
    }

    pub fn list_markdown_categories(&self) -> AppResult<Vec<MarkdownCategory>> {
        self.require_unlocked()?;
        let mut stmt = self.conn.prepare(
            "
            SELECT c.id, c.name, c.parent_id, c.sort_order, COUNT(d.id), c.created_at, c.updated_at
            FROM markdown_categories c
            LEFT JOIN markdown_documents d ON d.category_id = c.id
            GROUP BY c.id
            ORDER BY c.sort_order ASC, c.name COLLATE NOCASE ASC
            ",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(MarkdownCategory {
                id: row.get(0)?,
                name: row.get(1)?,
                parent_id: row.get(2)?,
                sort_order: row.get(3)?,
                document_count: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })?;
        collect_rows(rows)
    }

    pub fn create_markdown_category(
        &self,
        input: CreateMarkdownCategoryInput,
    ) -> AppResult<MarkdownCategory> {
        self.require_unlocked()?;
        let name = validate_markdown_category_name(&input.name)?;
        validate_markdown_parent(&self.conn, None, input.parent_id)?;
        self.conn.execute(
            "
            INSERT INTO markdown_categories (name, parent_id, sort_order)
            VALUES (?, ?, COALESCE((SELECT MAX(sort_order) + 1 FROM markdown_categories WHERE parent_id IS ?), 0))
            ",
            params![name, input.parent_id, input.parent_id],
        )?;
        let id = self.conn.last_insert_rowid();
        self.audit(
            "markdown.category_created",
            "markdown_category",
            Some(id),
            name,
        )?;
        self.get_markdown_category(id)
    }

    pub fn update_markdown_category(
        &self,
        input: UpdateMarkdownCategoryInput,
    ) -> AppResult<MarkdownCategory> {
        self.require_unlocked()?;
        let name = validate_markdown_category_name(&input.name)?;
        validate_markdown_parent(&self.conn, Some(input.id), input.parent_id)?;
        let changed = self.conn.execute(
            "
            UPDATE markdown_categories
            SET name = ?, parent_id = ?, sort_order = COALESCE(?, sort_order), updated_at = CURRENT_TIMESTAMP
            WHERE id = ?
            ",
            params![name, input.parent_id, input.sort_order.map(|value| value.max(0)), input.id],
        )?;
        if changed == 0 {
            return Err(AppError::InvalidInput(
                "markdown category not found".to_string(),
            ));
        }
        self.audit(
            "markdown.category_updated",
            "markdown_category",
            Some(input.id),
            name,
        )?;
        self.get_markdown_category(input.id)
    }

    pub fn delete_markdown_category(&self, category_id: i64) -> AppResult<()> {
        self.require_unlocked()?;
        let exists = self
            .conn
            .query_row(
                "SELECT 1 FROM markdown_categories WHERE id = ?",
                [category_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .is_some();
        if !exists {
            return Err(AppError::InvalidInput(
                "markdown category not found".to_string(),
            ));
        }
        let has_children = self.conn.query_row(
            "
            SELECT
                EXISTS(SELECT 1 FROM markdown_categories WHERE parent_id = ?1)
                OR EXISTS(SELECT 1 FROM markdown_documents WHERE category_id = ?1)
            ",
            [category_id],
            |row| Ok(row.get::<_, i64>(0)? != 0),
        )?;
        if has_children {
            return Err(AppError::InvalidInput(
                "文件夹中有子文件或子文件夹，请先删除子文件和子文件夹后再删除父文件夹".to_string(),
            ));
        }
        self.conn.execute(
            "DELETE FROM markdown_categories WHERE id = ?",
            [category_id],
        )?;
        self.audit(
            "markdown.category_deleted",
            "markdown_category",
            Some(category_id),
            "",
        )?;
        Ok(())
    }

    pub fn list_markdown_documents(
        &self,
        category_id: Option<i64>,
        search: Option<String>,
    ) -> AppResult<Vec<MarkdownDocument>> {
        self.require_unlocked()?;
        let search = search.unwrap_or_default().trim().to_string();
        let mut stmt = self.conn.prepare(
            "
            SELECT d.id, d.title, d.content, d.category_id, c.name, d.source_path,
                   d.created_at, d.updated_at
            FROM markdown_documents d
            LEFT JOIN markdown_categories c ON c.id = d.category_id
            WHERE (?1 IS NULL OR d.category_id = ?1)
              AND (?2 = '' OR d.title LIKE '%' || ?2 || '%' COLLATE NOCASE
                           OR d.content LIKE '%' || ?2 || '%' COLLATE NOCASE)
            ORDER BY d.updated_at DESC, d.id DESC
            ",
        )?;
        let rows = stmt.query_map(params![category_id, search], map_markdown_document_row)?;
        collect_rows(rows)
    }

    pub fn get_markdown_document(&self, document_id: i64) -> AppResult<MarkdownDocument> {
        self.require_unlocked()?;
        self.conn
            .query_row(
                "
                SELECT d.id, d.title, d.content, d.category_id, c.name, d.source_path,
                       d.created_at, d.updated_at
                FROM markdown_documents d
                LEFT JOIN markdown_categories c ON c.id = d.category_id
                WHERE d.id = ?
                ",
                [document_id],
                map_markdown_document_row,
            )
            .optional()?
            .ok_or_else(|| AppError::InvalidInput("markdown document not found".to_string()))
    }

    pub fn create_markdown_document(
        &self,
        input: CreateMarkdownDocumentInput,
    ) -> AppResult<MarkdownDocument> {
        self.require_unlocked()?;
        validate_markdown_category_reference(&self.conn, input.category_id)?;
        let title = validate_markdown_title(input.title.as_deref().unwrap_or("未命名文档"))?;
        let content = validate_markdown_content(input.content.unwrap_or_default())?;
        let source_path = normalize_markdown_source_path(input.source_path);
        self.conn.execute(
            "
            INSERT INTO markdown_documents (title, content, category_id, source_path)
            VALUES (?, ?, ?, ?)
            ",
            params![title, content, input.category_id, source_path],
        )?;
        let id = self.conn.last_insert_rowid();
        self.audit(
            "markdown.document_created",
            "markdown_document",
            Some(id),
            title,
        )?;
        self.get_markdown_document(id)
    }

    pub fn update_markdown_document(
        &self,
        input: UpdateMarkdownDocumentInput,
    ) -> AppResult<MarkdownDocument> {
        self.require_unlocked()?;
        validate_markdown_category_reference(&self.conn, input.category_id)?;
        let title = validate_markdown_title(&input.title)?;
        let content = validate_markdown_content(input.content)?;
        let source_path = normalize_markdown_source_path(input.source_path);
        let changed = self.conn.execute(
            "
            UPDATE markdown_documents
            SET title = ?, content = ?, category_id = ?, source_path = ?,
                updated_at = CURRENT_TIMESTAMP
            WHERE id = ?
            ",
            params![title, content, input.category_id, source_path, input.id],
        )?;
        if changed == 0 {
            return Err(AppError::InvalidInput(
                "markdown document not found".to_string(),
            ));
        }
        self.audit(
            "markdown.document_updated",
            "markdown_document",
            Some(input.id),
            title,
        )?;
        self.get_markdown_document(input.id)
    }

    pub fn delete_markdown_document(&self, document_id: i64) -> AppResult<()> {
        self.require_unlocked()?;
        let changed = self
            .conn
            .execute("DELETE FROM markdown_documents WHERE id = ?", [document_id])?;
        if changed == 0 {
            return Err(AppError::InvalidInput(
                "markdown document not found".to_string(),
            ));
        }
        self.audit(
            "markdown.document_deleted",
            "markdown_document",
            Some(document_id),
            "",
        )?;
        Ok(())
    }

    fn get_markdown_category(&self, category_id: i64) -> AppResult<MarkdownCategory> {
        self.conn
            .query_row(
                "
                SELECT c.id, c.name, c.parent_id, c.sort_order, COUNT(d.id), c.created_at, c.updated_at
                FROM markdown_categories c
                LEFT JOIN markdown_documents d ON d.category_id = c.id
                WHERE c.id = ?
                GROUP BY c.id
                ",
                [category_id],
                |row| {
                    Ok(MarkdownCategory {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        parent_id: row.get(2)?,
                        sort_order: row.get(3)?,
                        document_count: row.get(4)?,
                        created_at: row.get(5)?,
                        updated_at: row.get(6)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| AppError::InvalidInput("markdown category not found".to_string()))
    }

    pub fn list_accounts(&self) -> AppResult<Vec<Account>> {
        self.require_unlocked()?;
        let mut stmt = self.conn.prepare(
            "
            SELECT a.id, a.email, a.group_id, g.name, COALESCE(a.remark, ''), a.status,
                   a.provider, a.account_type, a.last_refresh_status,
                   a.last_refresh_error, a.last_refresh_at, COUNT(m.id) AS message_count, a.created_at, a.updated_at,
                   a.client_id_enc, a.refresh_token_enc, COALESCE(a.imap_host, ''),
                   a.imap_port, COALESCE(a.proxy_url, ''), COALESCE(a.fallback_proxy_url_1, ''),
                   COALESCE(a.fallback_proxy_url_2, ''), COALESCE(a.mail_retention_days, 30)
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
                last_refresh_status: row.get(8)?,
                last_refresh_error: row.get(9)?,
                last_refresh_at: row.get(10)?,
                message_count: row.get(11)?,
                created_at: row.get(12)?,
                updated_at: row.get(13)?,
                aliases: Vec::new(),
                has_client_id: !row.get::<_, String>(14)?.is_empty(),
                has_refresh_token: !row.get::<_, String>(15)?.is_empty(),
                imap_host: row.get(16)?,
                imap_port: row.get(17)?,
                proxy_url: row.get(18)?,
                fallback_proxy_url_1: row.get(19)?,
                fallback_proxy_url_2: row.get(20)?,
                mail_retention_days: row.get(21)?,
            })
        })?;

        let mut accounts = collect_rows(rows)?;
        for account in &mut accounts {
            account.aliases = self.aliases_for_account(account.id)?;
        }
        Ok(accounts)
    }

    pub fn import_accounts(
        &self,
        rows: Vec<ImportedAccount>,
        group_id: Option<i64>,
    ) -> AppResult<ImportAccountsResult> {
        self.require_unlocked()?;
        let key = self.crypto_key.as_ref().ok_or(AppError::Unauthorized)?;
        let mut imported = 0_usize;
        let mut skipped = 0_usize;
        let mut imported_emails = HashSet::new();

        for row in rows {
            let provider = providers::detect_mail_provider(
                &row.email,
                row.provider.as_deref(),
                !row.refresh_token.trim().is_empty(),
            )?;
            let is_imap_provider = provider.credential_kind.starts_with("imap");
            let auth_secret = if is_imap_provider {
                row.password.as_str()
            } else if !row.refresh_token.trim().is_empty() {
                row.refresh_token.as_str()
            } else {
                row.password.as_str()
            };
            let client_id = crypto::encrypt_text(&row.client_id, key)?;
            let refresh_token = crypto::encrypt_text(auth_secret, key)?;
            let changed = self.conn.execute(
                "
                INSERT INTO accounts
                (email, client_id_enc, refresh_token_enc, group_id, remark, provider, account_type,
                 imap_host, imap_port)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(email) DO UPDATE SET
                    client_id_enc = excluded.client_id_enc,
                    refresh_token_enc = CASE
                        WHEN excluded.refresh_token_enc = '' THEN accounts.refresh_token_enc
                        ELSE excluded.refresh_token_enc
                    END,
                    group_id = COALESCE(excluded.group_id, accounts.group_id),
                    remark = excluded.remark,
                    provider = excluded.provider,
                    account_type = excluded.account_type,
                    imap_host = CASE
                        WHEN excluded.imap_host = '' THEN accounts.imap_host
                        ELSE excluded.imap_host
                    END,
                    imap_port = excluded.imap_port,
                    updated_at = CURRENT_TIMESTAMP
                ",
                params![
                    row.email,
                    client_id,
                    refresh_token,
                    group_id,
                    row.remark,
                    provider.id,
                    provider.account_type,
                    provider.default_imap_host,
                    provider.default_imap_port,
                ],
            )?;
            if changed > 0 {
                imported += 1;
                imported_emails.insert(row.email.clone());
            } else {
                skipped += 1;
            }
        }

        self.audit(
            "account.imported",
            "account",
            None,
            &format!("{} imported", imported),
        )?;
        let accounts = self
            .list_accounts()?
            .into_iter()
            .filter(|account| imported_emails.contains(&account.email))
            .collect();
        Ok(ImportAccountsResult {
            imported,
            skipped,
            accounts,
        })
    }

    pub fn update_account(&self, input: UpdateAccountInput) -> AppResult<Account> {
        self.require_unlocked()?;
        let email = input.email.trim().to_ascii_lowercase();
        if !email.contains('@') {
            return Err(AppError::InvalidInput(
                "account email is invalid".to_string(),
            ));
        }
        let existing_id = self
            .conn
            .query_row("SELECT id FROM accounts WHERE id = ?", [input.id], |row| {
                row.get::<_, i64>(0)
            })
            .optional()?
            .ok_or_else(|| AppError::InvalidInput("account not found".to_string()))?;
        self.ensure_primary_email_is_not_alias(existing_id, &email)?;
        let key = self.crypto_key.as_ref().ok_or(AppError::Unauthorized)?;
        let mail_retention_days = input.mail_retention_days.map(|days| days.clamp(1, 3650));
        let provider_id = match input
            .provider
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            Some(value) => Some(
                providers::normalize_mail_provider_id(value)
                    .ok_or_else(|| {
                        AppError::InvalidInput(format!("unsupported mail provider: {value}"))
                    })?
                    .to_string(),
            ),
            None => None,
        };
        let provider_definition = provider_id
            .as_deref()
            .and_then(providers::mail_provider_definition);
        let account_type = input
            .account_type
            .clone()
            .or_else(|| provider_definition.map(|provider| provider.account_type.to_string()));
        let imap_host = match (input.imap_host.clone(), provider_definition) {
            (Some(value), _) if !value.trim().is_empty() => Some(value),
            (_, Some(provider)) if !provider.default_imap_host.is_empty() => {
                Some(provider.default_imap_host.to_string())
            }
            (Some(value), _) => Some(value),
            (None, _) => None,
        };
        let imap_port = input
            .imap_port
            .or_else(|| provider_definition.map(|provider| provider.default_imap_port));

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
                mail_retention_days = COALESCE(?, mail_retention_days),
                updated_at = CURRENT_TIMESTAMP
            WHERE id = ?
            ",
            params![
                email,
                input.group_id,
                input.remark,
                input.status,
                provider_id,
                account_type,
                imap_host,
                imap_port,
                normalize_proxy_option(input.proxy_url.as_deref())?,
                normalize_proxy_option(input.fallback_proxy_url_1.as_deref())?,
                normalize_proxy_option(input.fallback_proxy_url_2.as_deref())?,
                mail_retention_days,
                existing_id
            ],
        )?;

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
        self.conn
            .execute("DELETE FROM accounts WHERE id = ?", [account_id])?;
        self.audit("account.deleted", "account", Some(account_id), "")?;
        Ok(())
    }

    pub fn batch_accounts(&self, input: AccountBatchInput) -> AppResult<JobResult> {
        self.require_unlocked()?;
        let mut requested_ids: Vec<i64> =
            input.account_ids.into_iter().filter(|id| *id > 0).collect();
        requested_ids.sort_unstable();
        requested_ids.dedup();
        if requested_ids.is_empty() {
            return Err(AppError::InvalidInput(
                "account_ids are required".to_string(),
            ));
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
                    self.conn
                        .execute("DELETE FROM accounts WHERE id = ?", [account_id])?;
                }
                account_ids.len()
            }
            "move_group" => {
                if let Some(group_id) = input.group_id {
                    let exists = self
                        .conn
                        .prepare("SELECT id FROM groups WHERE id = ?")?
                        .exists([group_id])?;
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
            _ => {
                return Err(AppError::InvalidInput(
                    "unsupported batch action".to_string(),
                ))
            }
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

    pub fn reveal_account_secrets(
        &self,
        input: RevealAccountSecretsInput,
    ) -> AppResult<AccountSecretsPreview> {
        self.require_unlocked()?;
        let hash = self
            .get_config("password_hash")?
            .ok_or_else(|| AppError::InvalidInput("app is not initialized".to_string()))?;
        if !crypto::verify_password(&input.password, &hash)? {
            return Err(AppError::Unauthorized);
        }
        let key = self.crypto_key.as_ref().ok_or(AppError::Unauthorized)?;
        let (client_id_enc, refresh_token_enc): (String, String) = self
            .conn
            .query_row(
                "
                SELECT client_id_enc, refresh_token_enc
                FROM accounts
                WHERE id = ?
                ",
                [input.account_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .ok_or_else(|| AppError::InvalidInput("account not found".to_string()))?;
        let client_id = crypto::decrypt_text(&client_id_enc, &key)?;
        let refresh_token = crypto::decrypt_text(&refresh_token_enc, &key)?;
        self.audit(
            "account.secrets_viewed",
            "account",
            Some(input.account_id),
            "local password verified",
        )?;
        Ok(AccountSecretsPreview {
            client_id,
            refresh_token_preview: preview_secret(&refresh_token),
        })
    }

    pub fn list_messages(
        &self,
        account_id: Option<i64>,
        folder: Option<String>,
    ) -> AppResult<Vec<MailMessage>> {
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
        let folder = match normalize_mail_folder(query.folder.as_deref().unwrap_or("all")).as_str()
        {
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
                   m.attachments_json
            FROM retained_mail_messages m
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
            })
        })?;
        collect_rows(rows)
    }

    pub fn count_messages_query(&self, query: MailMessageQuery) -> AppResult<i64> {
        self.require_unlocked()?;
        let search = parse_mail_search(query.search.as_deref().unwrap_or_default());
        let folder = match normalize_mail_folder(query.folder.as_deref().unwrap_or("all")).as_str()
        {
            "all" => search.folder.unwrap_or_else(|| "all".to_string()),
            value => value.to_string(),
        };
        let read_state = match normalize_read_state(query.read_state.as_deref())?.as_str() {
            "all" => search.read_state.unwrap_or_else(|| "all".to_string()),
            value => value.to_string(),
        };
        let has_attachments = query.has_attachments.or(search.has_attachments);
        let mut sql = String::from(
            r#"
            SELECT COUNT(*)
            FROM retained_mail_messages m
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
        self.conn
            .query_row(&sql, params_from_iter(values), |row| row.get::<_, i64>(0))
            .map_err(AppError::from)
    }

    pub fn create_demo_message(&self, account_id: i64) -> AppResult<MailMessage> {
        self.require_unlocked()?;
        let exists = self
            .conn
            .query_row(
                "SELECT id FROM accounts WHERE id = ?",
                [account_id],
                |row| row.get::<_, i64>(0),
            )
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
            return Err(AppError::InvalidInput(
                "no matching messages found".to_string(),
            ));
        }

        let sync_remote = input.sync_remote.unwrap_or(true);
        let mut failed = 0_usize;
        let mut errors = Vec::new();
        if sync_remote {
            for target in &targets {
                if let Err(err) = self.sync_remote_mark_message(target, input.is_read) {
                    failed += 1;
                    let error = err.to_string();
                    errors.push(format!(
                        "#{} {}: {}",
                        target.id, target.provider_message_id, error
                    ));
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

        let action = if input.is_read {
            "mail.mark_read"
        } else {
            "mail.mark_unread"
        };
        self.audit(action, "message", None, &format!("{} message(s)", changed))?;
        Ok(JobResult {
            success: failed == 0,
            message: mail_action_message(
                if input.is_read {
                    "Marked read"
                } else {
                    "Marked unread"
                },
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
            return Err(AppError::InvalidInput(
                "no matching messages found".to_string(),
            ));
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
                    errors.push(format!(
                        "#{} {}: {}",
                        target.id, target.provider_message_id, error
                    ));
                    failed_local_ids.insert(target.id);
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
        }

        self.audit(
            "mail.deleted",
            "message",
            None,
            &format!("{} message(s)", changed),
        )?;
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
        settings.scheduler_refresh_enabled = self.get_config_bool(
            "scheduler_refresh_enabled",
            settings.scheduler_refresh_enabled,
        )?;
        settings.scheduler_refresh_interval_minutes = self.get_config_i64(
            "scheduler_refresh_interval_minutes",
            settings.scheduler_refresh_interval_minutes,
        )?;
        settings.scheduler_refresh_top =
            self.get_config_i64("scheduler_refresh_top", settings.scheduler_refresh_top)?;
        Ok(settings)
    }

    pub fn update_settings(&self, settings: Settings) -> AppResult<Settings> {
        self.require_unlocked()?;
        self.set_config("graph_client_id", &settings.graph_client_id)?;
        self.set_config("oauth_redirect_uri", &settings.oauth_redirect_uri)?;
        self.set_config_bool(
            "scheduler_refresh_enabled",
            settings.scheduler_refresh_enabled,
        )?;
        self.set_config_i64(
            "scheduler_refresh_interval_minutes",
            settings.scheduler_refresh_interval_minutes.max(1),
        )?;
        self.set_config_i64(
            "scheduler_refresh_top",
            settings
                .scheduler_refresh_top
                .clamp(1, providers::MAIL_REFRESH_MAX_TOP as i64),
        )?;
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
                        .query_row(
                            "SELECT provider FROM accounts WHERE id = ?",
                            [account_id],
                            |row| row.get::<_, String>(0),
                        )
                        .optional()?;
                    normalize_oauth_provider(stored_provider.as_deref())?
                        .unwrap_or_else(|| "graph".to_string())
                }
                None => "graph".to_string(),
            },
        };
        let token = exchange_oauth_code_for_provider(
            &provider,
            &input.client_id,
            &input.redirect_uri,
            &input.code_or_url,
            input.code_verifier.as_deref(),
        )?;
        if let Some(account_id) = input.account_id {
            let key = self.crypto_key.as_ref().ok_or(AppError::Unauthorized)?;
            let refresh_token = crypto::encrypt_text(&token.refresh_token, key)?;
            let client_id = crypto::encrypt_text(&input.client_id, key)?;
            let account_type = oauth_account_type(&provider);
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
                params![
                    client_id,
                    refresh_token,
                    provider.as_str(),
                    account_type.as_str(),
                    account_id
                ],
            )?;
            self.audit(
                &format!("oauth.{provider}.exchanged"),
                "account",
                Some(account_id),
                "",
            )?;
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

    pub fn save_oauth_account(
        &self,
        input: OAuthSaveAccountInput,
    ) -> AppResult<OAuthSaveAccountResult> {
        self.require_unlocked()?;
        let email = input.email.trim().to_ascii_lowercase();
        if !email.contains('@') {
            return Err(AppError::InvalidInput(
                "account email is required".to_string(),
            ));
        }
        let client_id = input.client_id.trim();
        if client_id.is_empty() {
            return Err(AppError::InvalidInput(
                "OAuth client id is required".to_string(),
            ));
        }

        let provider = normalize_oauth_provider(input.provider.as_deref())?
            .unwrap_or_else(|| "graph".to_string());
        let token = if let Some(refresh_token) = input
            .refresh_token
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
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
                .ok_or_else(|| {
                    AppError::InvalidInput("OAuth callback URL is required".to_string())
                })?;
            exchange_oauth_code_for_provider(
                &provider,
                client_id,
                &input.redirect_uri,
                code_or_url,
                input.code_verifier.as_deref(),
            )?
        };
        if token.refresh_token.trim().is_empty() {
            return Err(AppError::InvalidInput(
                "OAuth response did not include a refresh token".to_string(),
            ));
        }

        let key = self.crypto_key.as_ref().ok_or(AppError::Unauthorized)?;
        let client_id_enc = crypto::encrypt_text(client_id, key)?;
        let refresh_token_enc = crypto::encrypt_text(&token.refresh_token, key)?;
        let remark = input.remark.unwrap_or_default();
        let account_type = oauth_account_type(&provider);

        self.conn.execute(
            "
            INSERT INTO accounts
            (email, client_id_enc, refresh_token_enc, group_id, remark, provider,
             account_type, last_refresh_status, last_refresh_error, refresh_token_updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, 'authorized', NULL, CURRENT_TIMESTAMP)
            ON CONFLICT(email) DO UPDATE SET
                client_id_enc = excluded.client_id_enc,
                refresh_token_enc = excluded.refresh_token_enc,
                group_id = COALESCE(excluded.group_id, accounts.group_id),
                remark = CASE
                    WHEN excluded.remark = '' THEN accounts.remark
                    ELSE excluded.remark
                END,
                provider = excluded.provider,
                account_type = excluded.account_type,
                last_refresh_status = 'authorized',
                last_refresh_error = NULL,
                refresh_token_updated_at = CURRENT_TIMESTAMP,
                updated_at = CURRENT_TIMESTAMP
            ",
            params![
                email.as_str(),
                client_id_enc,
                refresh_token_enc,
                input.group_id,
                remark,
                provider.as_str(),
                account_type.as_str(),
            ],
        )?;

        let account_id = self.conn.query_row(
            "SELECT id FROM accounts WHERE email = ?",
            params![email.as_str()],
            |row| row.get::<_, i64>(0),
        )?;
        self.audit(
            &format!("oauth.{provider}.account_saved"),
            "account",
            Some(account_id),
            "",
        )?;
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

    fn refresh_accounts_with_trigger(
        &self,
        input: RefreshInput,
        _trigger_type: &str,
    ) -> AppResult<JobResult> {
        let result = self.refresh_accounts_inner(input);
        result
    }

    fn refresh_accounts_inner(&self, input: RefreshInput) -> AppResult<JobResult> {
        self.require_unlocked()?;
        let credentials = self.account_credentials(input.account_id)?;
        if credentials.is_empty() {
            return Err(AppError::InvalidInput(
                "no matching accounts to refresh".to_string(),
            ));
        }

        let folder = input.folder.unwrap_or_else(|| "inbox_junk".to_string());
        let top = self.refresh_top_for_input(input.top)?;
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
                }
            }
        }

        Ok(JobResult {
            success: failed == 0,
            message: if errors.is_empty() {
                format!(
                    "Refreshed {} account(s), cached {} message(s)",
                    refreshed, cached_messages
                )
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

    fn refresh_top_for_input(&self, top: Option<usize>) -> AppResult<usize> {
        match top {
            Some(0) => Ok(providers::MAIL_REFRESH_MAX_TOP),
            Some(value) => Ok(value.clamp(1, providers::MAIL_REFRESH_MAX_TOP)),
            None => Ok(self
                .get_settings()?
                .scheduler_refresh_top
                .clamp(1, providers::MAIL_REFRESH_MAX_TOP as i64) as usize),
        }
    }

    pub fn download_attachment(
        &self,
        input: DownloadAttachmentInput,
    ) -> AppResult<DownloadAttachmentResult> {
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
        let attachment = self.fetch_attachment_content(
            &account,
            &input.message_id,
            &input.attachment_id,
            folder.as_deref(),
        )?;
        let file_name = safe_file_name(&attachment.name);
        let dir = attachment_dir(&self.db_path)?;
        std::fs::create_dir_all(&dir).map_err(|err| AppError::Internal(err.to_string()))?;
        let path = unique_path(&dir, &file_name);
        std::fs::write(&path, &attachment.bytes)
            .map_err(|err| AppError::Internal(err.to_string()))?;
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

    pub fn download_all_attachments(
        &self,
        input: DownloadAllAttachmentsInput,
    ) -> AppResult<ExportResult> {
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
        let attachment_infos =
            self.cached_message_attachments(account.id, &input.message_id, folder.as_deref())?;
        if attachment_infos.is_empty() {
            return Err(AppError::InvalidInput(
                "message has no cached attachment metadata".to_string(),
            ));
        }

        let mut used_names = HashSet::new();
        let mut files = Vec::new();
        for attachment_info in attachment_infos {
            let downloaded = self
                .fetch_attachment_content(
                    &account,
                    &input.message_id,
                    &attachment_info.id,
                    folder.as_deref(),
                )
                .map_err(|err| {
                    AppError::Internal(format!(
                        "failed to download attachment {}: {}",
                        attachment_info.name, err
                    ))
                })?;
            let display_name = if downloaded.name.trim().is_empty() {
                attachment_info.name.as_str()
            } else {
                downloaded.name.as_str()
            };
            files.push((
                unique_bundle_file_name(&mut used_names, display_name),
                downloaded.bytes,
            ));
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
            return Err(AppError::InvalidInput(
                "cached raw MIME is empty".to_string(),
            ));
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
        require_provider_capability(account, "download_attachments", "attachment download")?;
        match mail_provider_adapter(account)? {
            MailProviderAdapter::Graph => {
                providers::download_graph_attachment(account, message_id, attachment_id)
            }
            MailProviderAdapter::Imap => {
                let raw_mime = self.cached_imap_raw_mime(account.id, message_id, folder)?;
                providers::download_imap_attachment_from_raw(&raw_mime, attachment_id)
            }
        }
    }

    pub fn export_mail_messages(&self, input: ExportMailMessagesInput) -> AppResult<ExportResult> {
        self.require_unlocked()?;
        let ids = normalize_message_ids(&input.message_ids)?;
        let rows = self.export_mail_message_rows(&ids)?;
        if rows.is_empty() {
            return Err(AppError::InvalidInput(
                "no matching messages found".to_string(),
            ));
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
        self.audit(
            "mail.exported",
            "message",
            None,
            &format!("{} message(s)", rows.len()),
        )?;
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
            return Err(AppError::InvalidInput(
                "no matching messages found".to_string(),
            ));
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
        let message_ids_json = serde_json::to_string(&ids).map_err(|err| {
            AppError::Internal(format!("serialize share message ids failed: {err}"))
        })?;
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
        self.audit(
            "mail_share.created",
            "share",
            Some(id),
            &format!("{} message(s)", rows.len()),
        )?;
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
        self.audit(
            "accounts.exported",
            "account",
            None,
            &format!("{} account(s)", accounts.len()),
        )?;
        Ok(ExportResult {
            path,
            file_name,
            size,
            item_count: accounts.len(),
        })
    }

    pub fn export_account_secrets(
        &self,
        input: ExportAccountSecretsInput,
    ) -> AppResult<ExportResult> {
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
            return Err(AppError::InvalidInput(
                "select at least one account".to_string(),
            ));
        }
        let hash = self
            .get_config("password_hash")?
            .ok_or_else(|| AppError::InvalidInput("app is not initialized".to_string()))?;
        if !crypto::verify_password(&input.password, &hash)? {
            return Err(AppError::Unauthorized);
        }
        let key = self.crypto_key.as_ref().ok_or(AppError::Unauthorized)?;
        let placeholders = repeat_placeholders(account_ids.len());
        let sql = format!(
            "
            SELECT id, email, provider, account_type, remark,
                   COALESCE(imap_host, ''), imap_port,
                   client_id_enc, refresh_token_enc
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
            ))
        })?;
        let rows = collect_rows(rows)?;
        if rows.is_empty() {
            return Err(AppError::InvalidInput(
                "no matching accounts found".to_string(),
            ));
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
            "client_id",
            "refresh_token",
        ]));
        for (
            id,
            email,
            provider,
            account_type,
            remark,
            imap_host,
            imap_port,
            client_id_enc,
            refresh_token_enc,
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
                crypto::decrypt_text(&client_id_enc, &key)?,
                crypto::decrypt_text(&refresh_token_enc, &key)?,
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

    pub fn local_retention_summary(&self) -> AppResult<LocalRetentionSummary> {
        self.require_unlocked()?;
        let (attachment_file_count, attachments_size) = dir_stats(&attachment_dir(&self.db_path)?)?;
        let (export_file_count, exports_size) = dir_stats(&exports_dir(&self.db_path)?)?;
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
            mail_message_count,
            unread_message_count,
            raw_mime_count,
            body_cached_count,
            latest_mail_received_at,
            latest_account_refresh_at,
        })
    }

    pub fn clear_local_data(&self, input: ClearLocalDataInput) -> AppResult<ClearLocalDataResult> {
        self.require_unlocked()?;
        if input.confirm.trim() != "CLEAR LOCAL DATA" {
            return Err(AppError::InvalidInput(
                "type CLEAR LOCAL DATA to confirm local data cleanup".to_string(),
            ));
        }
        let clear_mail_cache = input.clear_mail_cache.unwrap_or(false);
        let clear_attachments = input.clear_attachments.unwrap_or(false);
        let clear_exports = input.clear_exports.unwrap_or(false);
        if !clear_mail_cache && !clear_attachments && !clear_exports {
            return Err(AppError::InvalidInput(
                "select at least one local data category to clear".to_string(),
            ));
        }

        let mut deleted_messages = 0_i64;
        let mut deleted_files = 0_usize;
        let mut freed_bytes = 0_i64;

        if clear_mail_cache {
            deleted_messages = self.scalar_count("SELECT COUNT(*) FROM retained_mail_messages")?
                + self.scalar_count("SELECT COUNT(*) FROM temp_email_messages")?;
            self.conn
                .execute("DELETE FROM retained_mail_messages", [])?;
            self.conn.execute("DELETE FROM temp_email_messages", [])?;
            self.conn.execute(
                "UPDATE temp_emails SET message_count = 0, last_checked_at = NULL, updated_at = CURRENT_TIMESTAMP",
                [],
            )?;
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
                "{} mail, {} files, {} bytes",
                deleted_messages, deleted_files, freed_bytes
            ),
        )?;
        Ok(ClearLocalDataResult {
            success: true,
            message: format!(
                "Cleared local data: {} mail message(s), {} file(s)",
                deleted_messages, deleted_files
            ),
            deleted_messages,
            deleted_files,
            freed_bytes,
        })
    }

    pub fn scheduler_status(&self) -> AppResult<SchedulerStatus> {
        self.require_unlocked()?;
        Ok(SchedulerStatus {
            last_refresh_at: self.get_config("scheduler_last_refresh_at")?,
        })
    }

    pub fn run_due_scheduled_jobs(&self) -> AppResult<()> {
        if !self.is_unlocked() {
            return Ok(());
        }
        let settings = self.get_settings()?;
        let now = Utc::now();

        if settings.scheduler_refresh_enabled
            && self.scheduler_due(
                "scheduler_last_refresh_at",
                settings.scheduler_refresh_interval_minutes,
                now,
            )?
        {
            match self.refresh_accounts_with_trigger(
                RefreshInput {
                    account_id: None,
                    folder: Some("inbox_junk".to_string()),
                    top: Some(
                        settings
                            .scheduler_refresh_top
                            .clamp(1, providers::MAIL_REFRESH_MAX_TOP as i64)
                            as usize,
                    ),
                },
                "schedule",
            ) {
                Ok(result) => {
                    self.audit("scheduler.refresh", "scheduler", None, &result.message)?
                }
                Err(err) => self.audit(
                    "scheduler.refresh_failed",
                    "scheduler",
                    None,
                    &err.to_string(),
                )?,
            }
            self.set_config("scheduler_last_refresh_at", &now.to_rfc3339())?;
        }

        Ok(())
    }

    pub fn list_temp_emails(&self) -> AppResult<Vec<TempEmail>> {
        self.require_unlocked()?;
        let mut statement = self.conn.prepare(
            "SELECT te.id, te.email, te.provider, te.provider_base_url, te.cloudflare_channel_id, cc.name, te.message_count, te.last_checked_at, te.created_at FROM temp_emails te LEFT JOIN cloudflare_channels cc ON cc.id = te.cloudflare_channel_id ORDER BY te.id DESC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(TempEmail {
                id: row.get(0)?,
                email: row.get(1)?,
                provider: row.get(2)?,
                provider_base_url: row.get(3)?,
                cloudflare_channel_id: row.get(4)?,
                cloudflare_channel_name: row.get(5)?,
                message_count: row.get(6)?,
                last_checked_at: row.get(7)?,
                created_at: row.get(8)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    pub fn generate_temp_email(&self, input: GenerateTempEmailInput) -> AppResult<TempEmail> {
        let key = self.crypto_key.as_ref().ok_or(AppError::Unauthorized)?;
        let channel_id = input.cloudflare_channel_id;
        let channel = match channel_id {
            Some(id) => Some(self.cloudflare_channel_credentials(id)?),
            None => None,
        };
        let created = temp_mail::create(input, channel.as_ref())?;
        self.conn.execute(
            "INSERT INTO temp_emails (email, provider, provider_base_url, api_key_enc, password_enc, token_enc, provider_account_id, cloudflare_channel_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            params![created.email, created.provider, created.base_url, crypto::encrypt_text(&created.api_key, key)?, crypto::encrypt_text(&created.password, key)?, crypto::encrypt_text(&created.token, key)?, created.account_id, channel_id],
        ).map_err(|err| match err {
            rusqlite::Error::SqliteFailure(ref code, _) if code.code == rusqlite::ErrorCode::ConstraintViolation => AppError::InvalidInput("temporary email already exists".to_string()),
            other => AppError::Database(other),
        })?;
        let id = self.conn.last_insert_rowid();
        self.get_temp_email(id)
    }

    pub fn import_temp_emails(
        &self,
        input: ImportTempEmailsInput,
    ) -> AppResult<ImportTempEmailsResult> {
        let key = self.crypto_key.as_ref().ok_or(AppError::Unauthorized)?;
        let provider = input.provider.trim().to_lowercase();
        let (base_url, api_key) =
            temp_mail::imported_provider_config(&provider, input.base_url, input.api_key)?;
        if input.raw.trim().is_empty() {
            return Err(AppError::InvalidInput(
                "temporary email import text is required".to_string(),
            ));
        }

        let mut imported = 0;
        let mut updated = 0;
        let mut skipped = 0;
        let mut token_failures = Vec::new();
        let mut errors = Vec::new();
        let mut imported_ids = Vec::new();
        let mut current_channel_id = input.cloudflare_channel_id;

        for (line_index, raw_line) in input.raw.lines().enumerate() {
            let line = raw_line.trim();
            if line.is_empty() {
                continue;
            }

            if provider == "cloudflare" {
                if let Some(channel_name) = cloudflare_import_header(line) {
                    match channel_name {
                        Some(name) => match self.cloudflare_channel_id_by_name(&name) {
                            Ok(id) => current_channel_id = Some(id),
                            Err(err) => {
                                current_channel_id = None;
                                errors.push(format!("line {}: {}", line_index + 1, err));
                            }
                        },
                        None => current_channel_id = input.cloudflare_channel_id,
                    }
                    continue;
                }
            }

            let parsed = match provider.as_str() {
                "gptmail" => Some((
                    line.to_string(),
                    String::new(),
                    String::new(),
                    String::new(),
                    None,
                    base_url.clone(),
                    api_key.clone(),
                )),
                "duckmail" => {
                    let mut parts = line.splitn(3, "----");
                    let email = parts.next().unwrap_or_default().trim().to_string();
                    let password = parts.next().unwrap_or_default().trim().to_string();
                    if password.is_empty() {
                        None
                    } else {
                        let token =
                            match temp_mail::authenticate_duckmail(&base_url, &email, &password) {
                                Ok(token) => token,
                                Err(_) => {
                                    token_failures.push(email.clone());
                                    String::new()
                                }
                            };
                        Some((
                            email,
                            password,
                            token,
                            String::new(),
                            None,
                            base_url.clone(),
                            api_key.clone(),
                        ))
                    }
                }
                "cloudflare" => {
                    let email = line
                        .split("----")
                        .next()
                        .unwrap_or_default()
                        .trim()
                        .to_string();
                    match current_channel_id {
                        Some(channel_id) => match self.cloudflare_channel_credentials(channel_id) {
                            Ok(channel) => Some((
                                email,
                                String::new(),
                                String::new(),
                                String::new(),
                                Some(channel_id),
                                channel.worker_url,
                                String::new(),
                            )),
                            Err(err) => {
                                errors.push(format!("line {}: {}", line_index + 1, err));
                                None
                            }
                        },
                        None => {
                            errors.push(format!(
                                "line {}: Cloudflare channel is required",
                                line_index + 1
                            ));
                            None
                        }
                    }
                }
                _ => unreachable!(),
            };

            let Some((email, password, token, account_id, channel_id, item_base_url, item_api_key)) =
                parsed
            else {
                skipped += 1;
                continue;
            };
            if !is_valid_email_address(&email) {
                skipped += 1;
                errors.push(format!("line {}: invalid email address", line_index + 1));
                continue;
            }

            let existing_id = self
                .conn
                .query_row(
                    "SELECT id FROM temp_emails WHERE email = ? COLLATE NOCASE",
                    [&email],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?;
            self.conn.execute(
                "INSERT INTO temp_emails (email, provider, provider_base_url, api_key_enc, password_enc, token_enc, provider_account_id, cloudflare_channel_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(email) DO UPDATE SET provider = excluded.provider, provider_base_url = excluded.provider_base_url, api_key_enc = excluded.api_key_enc, password_enc = excluded.password_enc, token_enc = excluded.token_enc, provider_account_id = excluded.provider_account_id, cloudflare_channel_id = excluded.cloudflare_channel_id, updated_at = CURRENT_TIMESTAMP",
                params![email, provider, item_base_url, crypto::encrypt_text(&item_api_key, key)?, crypto::encrypt_text(&password, key)?, crypto::encrypt_text(&token, key)?, account_id, channel_id],
            )?;
            let id = existing_id.unwrap_or_else(|| self.conn.last_insert_rowid());
            if existing_id.is_some() {
                updated += 1;
            } else {
                imported += 1;
            }
            imported_ids.push(id);
        }

        if imported + updated == 0 {
            return Err(AppError::InvalidInput(
                errors
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "no valid temporary emails were found".to_string()),
            ));
        }
        let emails = imported_ids
            .into_iter()
            .filter_map(|id| self.get_temp_email(id).ok())
            .collect();
        Ok(ImportTempEmailsResult {
            imported,
            updated,
            skipped,
            token_failures,
            errors,
            emails,
        })
    }

    pub fn generate_temp_emails_batch(
        &self,
        input: GenerateTempEmailsBatchInput,
    ) -> AppResult<GenerateTempEmailsBatchResult> {
        if !input.provider.trim().eq_ignore_ascii_case("cloudflare") {
            return Err(AppError::InvalidInput(
                "batch generation currently supports Cloudflare only".to_string(),
            ));
        }
        if !(1..=50).contains(&input.count) {
            return Err(AppError::InvalidInput(
                "count must be between 1 and 50".to_string(),
            ));
        }
        let channel_id = input
            .cloudflare_channel_id
            .ok_or_else(|| AppError::InvalidInput("Cloudflare channel is required".to_string()))?;
        let channel = self.cloudflare_channel_credentials(channel_id)?;
        let domain = input
            .domain
            .unwrap_or_else(|| channel.domains.first().cloned().unwrap_or_default())
            .trim()
            .trim_start_matches('@')
            .to_lowercase();
        if !channel
            .domains
            .iter()
            .any(|item| item.eq_ignore_ascii_case(&domain))
        {
            return Err(AppError::InvalidInput(
                "selected domain does not belong to this Cloudflare channel".to_string(),
            ));
        }

        let usernames = normalize_batch_usernames(input.usernames, input.count)?;
        let mut emails = Vec::new();
        let mut failures = Vec::new();
        for (index, username) in usernames.into_iter().enumerate() {
            let create_input = GenerateTempEmailInput {
                provider: "cloudflare".to_string(),
                base_url: None,
                api_key: None,
                prefix: None,
                domain: Some(domain.clone()),
                username: Some(username.clone()),
                password: None,
                cloudflare_channel_id: Some(channel_id),
            };
            match self.generate_temp_email(create_input) {
                Ok(email) => emails.push(email),
                Err(err) => failures.push(TempEmailBatchFailure {
                    index: index + 1,
                    username,
                    error: err.to_string(),
                }),
            }
        }
        Ok(GenerateTempEmailsBatchResult {
            created_count: emails.len(),
            failed_count: failures.len(),
            emails,
            failures,
        })
    }

    pub fn list_temp_email_messages(&self, temp_email_id: i64) -> AppResult<Vec<TempEmailMessage>> {
        self.require_unlocked()?;
        self.get_temp_email(temp_email_id)?;
        self.cached_temp_email_messages(temp_email_id)
    }

    pub fn refresh_temp_email_messages(
        &self,
        temp_email_id: i64,
    ) -> AppResult<Vec<TempEmailMessage>> {
        let mailbox = self.temp_mailbox_credentials(temp_email_id)?;
        let messages = temp_mail::list_messages(&mailbox)?;
        self.cache_temp_email_messages(temp_email_id, &messages)?;
        self.cached_temp_email_messages(temp_email_id)
    }

    pub fn get_temp_email_message(
        &self,
        temp_email_id: i64,
        message_id: &str,
    ) -> AppResult<TempEmailMessage> {
        if message_id.trim().is_empty() {
            return Err(AppError::InvalidInput("message id is required".to_string()));
        }
        if let Some(message) = self.cached_temp_email_message(temp_email_id, message_id)? {
            if message.body.as_deref().is_some_and(|body| !body.is_empty()) {
                return Ok(message);
            }
        }
        let message =
            temp_mail::get_message(&self.temp_mailbox_credentials(temp_email_id)?, message_id)?;
        self.cache_temp_email_messages(temp_email_id, std::slice::from_ref(&message))?;
        Ok(message)
    }

    pub fn delete_temp_email(&self, temp_email_id: i64) -> AppResult<()> {
        let mailbox = self.temp_mailbox_credentials(temp_email_id)?;
        temp_mail::delete_remote(&mailbox)?;
        let changed = self
            .conn
            .execute("DELETE FROM temp_emails WHERE id = ?", [temp_email_id])?;
        if changed == 0 {
            return Err(AppError::InvalidInput(
                "temporary email not found".to_string(),
            ));
        }
        Ok(())
    }

    fn get_temp_email(&self, id: i64) -> AppResult<TempEmail> {
        self.conn.query_row(
            "SELECT te.id, te.email, te.provider, te.provider_base_url, te.cloudflare_channel_id, cc.name, te.message_count, te.last_checked_at, te.created_at FROM temp_emails te LEFT JOIN cloudflare_channels cc ON cc.id = te.cloudflare_channel_id WHERE te.id = ?",
            [id],
            |row| Ok(TempEmail { id: row.get(0)?, email: row.get(1)?, provider: row.get(2)?, provider_base_url: row.get(3)?, cloudflare_channel_id: row.get(4)?, cloudflare_channel_name: row.get(5)?, message_count: row.get(6)?, last_checked_at: row.get(7)?, created_at: row.get(8)? }),
        ).optional()?.ok_or_else(|| AppError::InvalidInput("temporary email not found".to_string()))
    }

    fn temp_mailbox_credentials(&self, id: i64) -> AppResult<TempMailboxCredentials> {
        let key = self.crypto_key.as_ref().ok_or(AppError::Unauthorized)?;
        let row = self.conn.query_row(
            "SELECT email, provider, provider_base_url, api_key_enc, password_enc, token_enc, provider_account_id, cloudflare_channel_id FROM temp_emails WHERE id = ?",
            [id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, String>(4)?, row.get::<_, String>(5)?, row.get::<_, String>(6)?, row.get::<_, Option<i64>>(7)?)),
        ).optional()?.ok_or_else(|| AppError::InvalidInput("temporary email not found".to_string()))?;
        let password = crypto::decrypt_text(&row.4, key)?;
        let cloudflare_channel = row
            .7
            .map(|channel_id| self.cloudflare_channel_credentials(channel_id))
            .transpose()?;
        Ok(TempMailboxCredentials {
            email: row.0,
            provider: row.1,
            base_url: row.2,
            api_key: crypto::decrypt_text(&row.3, key)?,
            password,
            token: crypto::decrypt_text(&row.5, key)?,
            account_id: row.6,
            cloudflare_channel,
        })
    }

    fn cache_temp_email_messages(
        &self,
        temp_email_id: i64,
        messages: &[TempEmailMessage],
    ) -> AppResult<()> {
        self.get_temp_email(temp_email_id)?;
        for message in messages {
            self.conn.execute(
                "
                INSERT INTO temp_email_messages (
                    temp_email_id, provider_message_id, sender, recipients, subject,
                    body_preview, body, body_type, received_at
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(temp_email_id, provider_message_id) DO UPDATE SET
                    sender = excluded.sender,
                    recipients = excluded.recipients,
                    subject = excluded.subject,
                    body_preview = excluded.body_preview,
                    body = CASE
                        WHEN excluded.body IS NOT NULL AND excluded.body != '' THEN excluded.body
                        ELSE temp_email_messages.body
                    END,
                    body_type = excluded.body_type,
                    received_at = excluded.received_at,
                    updated_at = CURRENT_TIMESTAMP
                ",
                params![
                    temp_email_id,
                    message.id,
                    message.sender,
                    message.recipients,
                    message.subject,
                    message.body_preview,
                    message.body,
                    message.body_type,
                    message.received_at,
                ],
            )?;
        }
        let cached_count = self.conn.query_row(
            "SELECT COUNT(*) FROM temp_email_messages WHERE temp_email_id = ?",
            [temp_email_id],
            |row| row.get::<_, i64>(0),
        )?;
        self.conn.execute(
            "UPDATE temp_emails SET message_count = ?, last_checked_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            params![cached_count, temp_email_id],
        )?;
        Ok(())
    }

    fn cached_temp_email_messages(&self, temp_email_id: i64) -> AppResult<Vec<TempEmailMessage>> {
        let mut statement = self.conn.prepare(
            "
            SELECT provider_message_id, sender, recipients, subject, body_preview,
                   body, body_type, received_at
            FROM temp_email_messages
            WHERE temp_email_id = ?
            ORDER BY received_at DESC, id DESC
            ",
        )?;
        let rows = statement.query_map([temp_email_id], temp_email_message_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    fn cached_temp_email_message(
        &self,
        temp_email_id: i64,
        message_id: &str,
    ) -> AppResult<Option<TempEmailMessage>> {
        self.conn
            .query_row(
                "
            SELECT provider_message_id, sender, recipients, subject, body_preview,
                   body, body_type, received_at
            FROM temp_email_messages
            WHERE temp_email_id = ? AND provider_message_id = ?
            ",
                params![temp_email_id, message_id],
                temp_email_message_from_row,
            )
            .optional()
            .map_err(AppError::from)
    }

    pub fn list_temp_email_domains(
        &self,
        config: TempEmailProviderConfig,
    ) -> AppResult<Vec<String>> {
        self.require_unlocked()?;
        let channel = config
            .cloudflare_channel_id
            .map(|id| self.cloudflare_channel_credentials(id))
            .transpose()?;
        temp_mail::list_domains(config, channel.as_ref())
    }

    pub fn list_cloudflare_channels(&self) -> AppResult<Vec<CloudflareChannel>> {
        self.require_unlocked()?;
        let mut statement = self.conn.prepare("SELECT id, name, worker_url, email_domains, enabled, admin_password_enc, created_at, updated_at FROM cloudflare_channels ORDER BY name COLLATE NOCASE")?;
        let rows = statement.query_map([], |row| {
            let raw: String = row.get(3)?;
            let secret: String = row.get(5)?;
            Ok(CloudflareChannel {
                id: row.get(0)?,
                name: row.get(1)?,
                worker_url: row.get(2)?,
                email_domains: split_domains(&raw),
                enabled: row.get::<_, i64>(4)? != 0,
                has_admin_password: !secret.is_empty(),
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    pub fn save_cloudflare_channel(
        &self,
        input: SaveCloudflareChannelInput,
    ) -> AppResult<CloudflareChannel> {
        let key = self.crypto_key.as_ref().ok_or(AppError::Unauthorized)?;
        let name = input.name.trim();
        let worker_url = normalize_worker_url(&input.worker_url)?;
        if name.is_empty() {
            return Err(AppError::InvalidInput(
                "channel name is required".to_string(),
            ));
        }
        let domains = input
            .email_domains
            .iter()
            .map(|item| {
                item.trim()
                    .trim_start_matches('@')
                    .trim_end_matches('.')
                    .to_lowercase()
            })
            .filter(|item| !item.is_empty())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        if domains.is_empty() {
            return Err(AppError::InvalidInput(
                "at least one Cloudflare email domain is required".to_string(),
            ));
        }
        let enabled = input.enabled.unwrap_or(true);
        let id = if let Some(id) = input.id {
            let existing_secret: String = self
                .conn
                .query_row(
                    "SELECT admin_password_enc FROM cloudflare_channels WHERE id = ?",
                    [id],
                    |row| row.get(0),
                )
                .optional()?
                .ok_or_else(|| {
                    AppError::InvalidInput("Cloudflare channel not found".to_string())
                })?;
            let secret = match input.admin_password.filter(|value| !value.is_empty()) {
                Some(value) => crypto::encrypt_text(&value, key)?,
                None => existing_secret,
            };
            self.conn.execute("UPDATE cloudflare_channels SET name = ?, worker_url = ?, admin_password_enc = ?, email_domains = ?, enabled = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?", params![name, worker_url, secret, domains.join(","), enabled, id])?;
            id
        } else {
            let password = input
                .admin_password
                .filter(|value| !value.is_empty())
                .ok_or_else(|| AppError::InvalidInput("admin password is required".to_string()))?;
            self.conn.execute("INSERT INTO cloudflare_channels (name, worker_url, admin_password_enc, email_domains, enabled) VALUES (?, ?, ?, ?, ?)", params![name, worker_url, crypto::encrypt_text(&password, key)?, domains.join(","), enabled])?;
            self.conn.last_insert_rowid()
        };
        self.list_cloudflare_channels()?
            .into_iter()
            .find(|item| item.id == id)
            .ok_or_else(|| AppError::Internal("saved Cloudflare channel not found".to_string()))
    }

    pub fn delete_cloudflare_channel(&self, id: i64) -> AppResult<()> {
        self.require_unlocked()?;
        let references: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM temp_emails WHERE cloudflare_channel_id = ?",
            [id],
            |row| row.get(0),
        )?;
        if references > 0 {
            return Err(AppError::InvalidInput(
                "Cloudflare channel is still used by temporary emails".to_string(),
            ));
        }
        if self
            .conn
            .execute("DELETE FROM cloudflare_channels WHERE id = ?", [id])?
            == 0
        {
            return Err(AppError::InvalidInput(
                "Cloudflare channel not found".to_string(),
            ));
        }
        Ok(())
    }

    fn cloudflare_channel_credentials(&self, id: i64) -> AppResult<CloudflareChannelCredentials> {
        let key = self.crypto_key.as_ref().ok_or(AppError::Unauthorized)?;
        let row = self.conn.query_row("SELECT worker_url, admin_password_enc, email_domains, enabled FROM cloudflare_channels WHERE id = ?", [id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, i64>(3)?))).optional()?.ok_or_else(|| AppError::InvalidInput("Cloudflare channel not found".to_string()))?;
        if row.3 == 0 {
            return Err(AppError::InvalidInput(
                "Cloudflare channel is disabled".to_string(),
            ));
        }
        Ok(CloudflareChannelCredentials {
            worker_url: row.0,
            admin_password: crypto::decrypt_text(&row.1, key)?,
            domains: split_domains(&row.2),
        })
    }

    fn cloudflare_channel_id_by_name(&self, name: &str) -> AppResult<i64> {
        let id = self
            .conn
            .query_row(
                "SELECT id FROM cloudflare_channels WHERE name = ? COLLATE NOCASE AND enabled = 1",
                [name.trim()],
                |row| row.get(0),
            )
            .optional()?;
        id.ok_or_else(|| {
            AppError::InvalidInput(format!("Cloudflare channel not found or disabled: {name}"))
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
                proxy_url TEXT DEFAULT '',
                fallback_proxy_url_1 TEXT DEFAULT '',
                fallback_proxy_url_2 TEXT DEFAULT '',
                mail_retention_days INTEGER NOT NULL DEFAULT 30,
                provider_sync_state TEXT NOT NULL DEFAULT '',
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

            CREATE TABLE IF NOT EXISTS workspace_key_records (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                purpose TEXT NOT NULL DEFAULT '',
                key_fingerprint TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS markdown_categories (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE COLLATE NOCASE,
                parent_id INTEGER,
                sort_order INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY(parent_id) REFERENCES markdown_categories(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS markdown_documents (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL DEFAULT '',
                content TEXT NOT NULL DEFAULT '',
                category_id INTEGER,
                source_path TEXT,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY(category_id) REFERENCES markdown_categories(id) ON DELETE SET NULL
            );

            CREATE TABLE IF NOT EXISTS temp_emails (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                email TEXT NOT NULL UNIQUE COLLATE NOCASE,
                provider TEXT NOT NULL,
                provider_base_url TEXT NOT NULL,
                api_key_enc TEXT NOT NULL DEFAULT '',
                password_enc TEXT NOT NULL DEFAULT '',
                token_enc TEXT NOT NULL DEFAULT '',
                provider_account_id TEXT NOT NULL DEFAULT '',
                message_count INTEGER NOT NULL DEFAULT 0,
                last_checked_at TEXT,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS temp_email_messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                temp_email_id INTEGER NOT NULL,
                provider_message_id TEXT NOT NULL,
                sender TEXT NOT NULL DEFAULT '',
                recipients TEXT NOT NULL DEFAULT '',
                subject TEXT NOT NULL DEFAULT '',
                body_preview TEXT NOT NULL DEFAULT '',
                body TEXT,
                body_type TEXT NOT NULL DEFAULT 'text',
                received_at TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(temp_email_id, provider_message_id),
                FOREIGN KEY(temp_email_id) REFERENCES temp_emails(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS cloudflare_channels (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE COLLATE NOCASE,
                worker_url TEXT NOT NULL,
                admin_password_enc TEXT NOT NULL DEFAULT '',
                email_domains TEXT NOT NULL DEFAULT '',
                enabled INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE INDEX IF NOT EXISTS idx_accounts_group ON accounts(group_id);
            CREATE INDEX IF NOT EXISTS idx_messages_account_folder ON retained_mail_messages(account_id, folder);
            CREATE INDEX IF NOT EXISTS idx_messages_received ON retained_mail_messages(received_at_sort DESC);
            CREATE INDEX IF NOT EXISTS idx_audit_created ON audit_logs(created_at DESC);
            CREATE INDEX IF NOT EXISTS idx_workspace_key_records_created ON workspace_key_records(created_at DESC);
            CREATE INDEX IF NOT EXISTS idx_markdown_documents_category_updated
                ON markdown_documents(category_id, updated_at DESC);
            CREATE INDEX IF NOT EXISTS idx_markdown_documents_updated
                ON markdown_documents(updated_at DESC);
            CREATE INDEX IF NOT EXISTS idx_temp_email_messages_mailbox_received
                ON temp_email_messages(temp_email_id, received_at DESC);
            ",
        )?;
        self.ensure_default_data()?;
        self.ensure_account_columns()?;
        self.ensure_message_columns()?;
        self.ensure_share_columns()?;
        self.ensure_temp_mail_columns()?;
        self.ensure_markdown_columns()?;
        self.prune_legacy_schema()?;
        Ok(())
    }

    fn prune_legacy_schema(&self) -> AppResult<()> {
        const LEGACY_INDEXES: &[&str] = &[
            "idx_temp_messages_email",
            "idx_project_accounts_project_status",
            "idx_project_events_project_created",
            "idx_forwarding_logs_message",
            "idx_backup_logs_created",
            "idx_automation_runs_created",
            "idx_retry_queue_status_due",
            "idx_retry_queue_key",
        ];
        const LEGACY_TABLES: &[&str] = &[
            "account_tags",
            "tags",
            "project_account_events",
            "project_accounts",
            "project_group_scopes",
            "project_tag_scopes",
            "projects",
            "retry_queue",
            "automation_runs",
            "forwarding_logs",
            "backup_logs",
            "refresh_logs",
        ];
        const LEGACY_ACCOUNT_COLUMNS: &[&str] = &["forward_enabled", "forward_last_checked_at"];
        const LEGACY_GROUP_COLUMNS: &[&str] = &[
            "color",
            "proxy_url",
            "fallback_proxy_url_1",
            "fallback_proxy_url_2",
        ];
        const LEGACY_CONFIG_KEYS: &[&str] = &[
            "webdav_url",
            "webdav_username",
            "webdav_password",
            "forward_smtp_host",
            "forward_smtp_port",
            "forward_smtp_username",
            "forward_smtp_password",
            "forward_smtp_from",
            "forward_smtp_to",
            "forward_telegram_bot_token",
            "forward_telegram_chat_id",
            "forward_wecom_webhook",
            "appearance_theme",
            "accent_color",
            "scheduler_last_forwarding_at",
            "scheduler_last_backup_at",
        ];

        for index in LEGACY_INDEXES {
            let sql = format!("DROP INDEX IF EXISTS {index}");
            self.conn.execute(&sql, [])?;
        }
        for table in LEGACY_TABLES {
            let sql = format!("DROP TABLE IF EXISTS {table}");
            self.conn.execute(&sql, [])?;
        }

        let account_columns = table_columns(&self.conn, "accounts")?;
        for column in LEGACY_ACCOUNT_COLUMNS {
            if account_columns.iter().any(|name| name == column) {
                let sql = format!("ALTER TABLE accounts DROP COLUMN {column}");
                self.conn.execute(&sql, [])?;
            }
        }

        let group_columns = table_columns(&self.conn, "groups")?;
        for column in LEGACY_GROUP_COLUMNS {
            if group_columns.iter().any(|name| name == column) {
                let sql = format!("ALTER TABLE groups DROP COLUMN {column}");
                self.conn.execute(&sql, [])?;
            }
        }

        for key in LEGACY_CONFIG_KEYS {
            self.conn
                .execute("DELETE FROM app_config WHERE key = ?", [key])?;
        }

        Ok(())
    }

    fn ensure_account_columns(&self) -> AppResult<()> {
        let columns = table_columns(&self.conn, "accounts")?;
        for (name, ddl) in [
            (
                "imap_host",
                "ALTER TABLE accounts ADD COLUMN imap_host TEXT DEFAULT ''",
            ),
            (
                "imap_port",
                "ALTER TABLE accounts ADD COLUMN imap_port INTEGER NOT NULL DEFAULT 993",
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
            (
                "mail_retention_days",
                "ALTER TABLE accounts ADD COLUMN mail_retention_days INTEGER NOT NULL DEFAULT 30",
            ),
            (
                "provider_sync_state",
                "ALTER TABLE accounts ADD COLUMN provider_sync_state TEXT NOT NULL DEFAULT ''",
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
        for (name, ddl) in [(
            "raw_mime",
            "ALTER TABLE retained_mail_messages ADD COLUMN raw_mime BLOB",
        )] {
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

    fn ensure_temp_mail_columns(&self) -> AppResult<()> {
        let columns = table_columns(&self.conn, "temp_emails")?;
        if !columns
            .iter()
            .any(|column| column == "cloudflare_channel_id")
        {
            self.conn.execute(
                "ALTER TABLE temp_emails ADD COLUMN cloudflare_channel_id INTEGER",
                [],
            )?;
        }
        Ok(())
    }

    fn ensure_markdown_columns(&self) -> AppResult<()> {
        let columns = table_columns(&self.conn, "markdown_categories")?;
        if !columns.iter().any(|column| column == "parent_id") {
            self.conn.execute(
                "ALTER TABLE markdown_categories ADD COLUMN parent_id INTEGER",
                [],
            )?;
        }
        self.conn.execute(
            "
            CREATE INDEX IF NOT EXISTS idx_markdown_categories_parent
                ON markdown_categories(parent_id, sort_order)
            ",
            [],
        )?;
        Ok(())
    }

    fn ensure_default_data(&self) -> AppResult<()> {
        self.conn.execute(
            "
            INSERT OR IGNORE INTO groups (id, name, description, sort_order, is_system)
            VALUES (1, 'Default', 'Default mailbox group', 0, 1)
            ",
            [],
        )?;
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
            .query_row("SELECT value FROM app_config WHERE key = ?", [key], |row| {
                row.get(0)
            })
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

    fn migrate_legacy_account_secrets(&self) -> AppResult<()> {
        if self.get_config("migrated_account_secrets_v1")?.is_some() {
            return Ok(());
        }
        let columns = table_columns(&self.conn, "accounts")?;
        let has_password = columns.iter().any(|name| name == "password_enc");
        let has_imap_password = columns.iter().any(|name| name == "imap_password_enc");
        if !has_password && !has_imap_password {
            self.set_config("migrated_account_secrets_v1", "1")?;
            return Ok(());
        }
        let key = match self.crypto_key.as_ref() {
            Some(key) => *key,
            None => return Ok(()),
        };
        let mut stmt = self.conn.prepare(
            "
            SELECT id, password_enc, client_id_enc, refresh_token_enc, imap_password_enc
            FROM accounts
            ",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;
        for row in rows {
            let (id, password_enc, _client_id_enc, refresh_token_enc, imap_password_enc) = row?;
            let refresh_token = crypto::decrypt_text(&refresh_token_enc, &key)?;
            let imap_password = crypto::decrypt_text(&imap_password_enc, &key)?;
            let password = crypto::decrypt_text(&password_enc, &key)?;
            let merged = if !refresh_token.trim().is_empty() {
                refresh_token
            } else if !imap_password.trim().is_empty() {
                imap_password
            } else {
                password
            };
            if merged.trim().is_empty() {
                continue;
            }
            let encrypted = crypto::encrypt_text(&merged, &key)?;
            self.conn.execute(
                "
                UPDATE accounts
                SET refresh_token_enc = ?, updated_at = CURRENT_TIMESTAMP
                WHERE id = ?
                ",
                params![encrypted, id],
            )?;
        }
        self.set_config("migrated_account_secrets_v1", "1")?;
        Ok(())
    }

    fn drop_legacy_account_secret_columns(&self) -> AppResult<()> {
        let columns = table_columns(&self.conn, "accounts")?;
        for column in ["password_enc", "imap_password_enc"] {
            if columns.iter().any(|name| name == column) {
                let sql = format!("ALTER TABLE accounts DROP COLUMN {column}");
                self.conn.execute(&sql, [])?;
            }
        }
        Ok(())
    }

    fn migrate_legacy_password_key(&mut self, old_key: &[u8; 32]) -> AppResult<()> {
        let workspace_key = crypto::random_workspace_key();
        let runtime_key = crypto::derive_workspace_key(&workspace_key)?;
        let accounts = self.reencrypt_account_secrets(old_key, &runtime_key)?;
        let config_secrets = self.reencrypt_config_secrets(old_key, &runtime_key)?;

        let hash = crypto::hash_password(DEFAULT_LOGIN_PASSWORD)?;
        let salt = crypto::random_salt();
        let password_key = crypto::derive_key(DEFAULT_LOGIN_PASSWORD, &salt);
        let workspace_key_enc = crypto::encrypt_text(&workspace_key, &password_key)?;

        let columns = table_columns(&self.conn, "accounts")?;
        let has_legacy_secrets = columns.iter().any(|name| name == "password_enc")
            || columns.iter().any(|name| name == "imap_password_enc");
        let tx = self.conn.transaction()?;
        if has_legacy_secrets {
            for (id, password, client_id, refresh_token, imap_password) in accounts {
                tx.execute(
                    "
                    UPDATE accounts
                    SET password_enc = ?, client_id_enc = ?, refresh_token_enc = ?, imap_password_enc = ?,
                        updated_at = CURRENT_TIMESTAMP
                    WHERE id = ?
                    ",
                    params![password, client_id, refresh_token, imap_password, id],
                )?;
            }
        } else {
            for (id, _password, client_id, refresh_token, _imap_password) in accounts {
                tx.execute(
                    "
                    UPDATE accounts
                    SET client_id_enc = ?, refresh_token_enc = ?, updated_at = CURRENT_TIMESTAMP
                    WHERE id = ?
                    ",
                    params![client_id, refresh_token, id],
                )?;
            }
        }
        for (key, value) in config_secrets {
            tx.execute(
                "
                INSERT INTO app_config (key, value, updated_at)
                VALUES (?, ?, CURRENT_TIMESTAMP)
                ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = CURRENT_TIMESTAMP
                ",
                params![key, value],
            )?;
        }
        tx.execute(
            "
            INSERT INTO app_config (key, value, updated_at)
            VALUES ('password_hash', ?, CURRENT_TIMESTAMP)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = CURRENT_TIMESTAMP
            ",
            [hash.as_str()],
        )?;
        tx.execute(
            "
            INSERT INTO app_config (key, value, updated_at)
            VALUES ('crypto_salt', ?, CURRENT_TIMESTAMP)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = CURRENT_TIMESTAMP
            ",
            [salt.as_str()],
        )?;
        tx.execute(
            "
            INSERT INTO app_config (key, value, updated_at)
            VALUES (?, ?, CURRENT_TIMESTAMP)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = CURRENT_TIMESTAMP
            ",
            params![WORKSPACE_KEY_CONFIG, workspace_key_enc],
        )?;
        tx.commit()?;

        self.crypto_key = Some(runtime_key);
        self.audit(
            "app.workspace_key_migrated",
            "settings",
            None,
            "legacy local password replaced by workspace key",
        )?;
        Ok(())
    }

    fn reencrypt_config_secrets(
        &self,
        old_key: &[u8; 32],
        new_key: &[u8; 32],
    ) -> AppResult<Vec<(String, String)>> {
        let mut values = Vec::new();
        for key in CONFIG_SECRET_KEYS {
            if let Some(value) = self.get_config(key)? {
                values.push((
                    (*key).to_string(),
                    reencrypt_secret_value(&value, old_key, new_key)?,
                ));
            }
        }
        Ok(values)
    }

    fn reencrypt_account_secrets(
        &self,
        old_key: &[u8; 32],
        new_key: &[u8; 32],
    ) -> AppResult<Vec<(i64, String, String, String, String)>> {
        let columns = table_columns(&self.conn, "accounts")?;
        let has_legacy_secrets = columns.iter().any(|name| name == "password_enc")
            || columns.iter().any(|name| name == "imap_password_enc");
        if has_legacy_secrets {
            let mut stmt = self.conn.prepare(
                "
                SELECT id, password_enc, client_id_enc, refresh_token_enc, imap_password_enc
                FROM accounts
                ",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?;
            let mut values = Vec::new();
            for row in rows {
                let (id, password, client_id, refresh_token, imap_password) = row?;
                values.push((
                    id,
                    reencrypt_secret_value(&password, old_key, new_key)?,
                    reencrypt_secret_value(&client_id, old_key, new_key)?,
                    reencrypt_secret_value(&refresh_token, old_key, new_key)?,
                    reencrypt_secret_value(&imap_password, old_key, new_key)?,
                ));
            }
            return Ok(values);
        }

        let mut stmt = self.conn.prepare(
            "
            SELECT id, client_id_enc, refresh_token_enc
            FROM accounts
            ",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut values = Vec::new();
        for row in rows {
            let (id, client_id, refresh_token) = row?;
            values.push((
                id,
                String::new(),
                reencrypt_secret_value(&client_id, old_key, new_key)?,
                reencrypt_secret_value(&refresh_token, old_key, new_key)?,
                String::new(),
            ));
        }
        Ok(values)
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
                SELECT g.id, g.name, COALESCE(g.description, ''), g.parent_id, g.level,
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
                        parent_id: row.get(3)?,
                        level: row.get(4)?,
                        sort_order: row.get(5)?,
                        account_count: row.get(6)?,
                    })
                },
            )
            .map_err(AppError::from)
    }

    fn group_descendant_ids(&self, group_id: i64) -> AppResult<Vec<i64>> {
        let mut descendants = Vec::new();
        let mut stack = vec![group_id];
        while let Some(parent_id) = stack.pop() {
            let mut stmt = self
                .conn
                .prepare("SELECT id FROM groups WHERE parent_id = ?")?;
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
            let level =
                self.conn
                    .query_row("SELECT level FROM groups WHERE id = ?", [id], |row| {
                        row.get::<_, i64>(0)
                    })?;
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
        self.conn.execute(
            "DELETE FROM account_aliases WHERE account_id = ?",
            [account_id],
        )?;
        for alias in aliases {
            self.conn.execute(
                "INSERT INTO account_aliases (account_id, alias_email) VALUES (?, ?)",
                params![account_id, alias],
            )?;
        }
        Ok(())
    }

    fn account_credentials(&self, account_id: Option<i64>) -> AppResult<Vec<AccountCredentials>> {
        let key = self.crypto_key.as_ref().ok_or(AppError::Unauthorized)?;
        let mut stmt = self.conn.prepare(
            "
            SELECT a.id, a.email, a.provider, a.account_type, a.client_id_enc,
                   a.refresh_token_enc, COALESCE(a.imap_host, ''), a.imap_port,
                   COALESCE(a.proxy_url, ''), COALESCE(a.fallback_proxy_url_1, ''),
                   COALESCE(a.fallback_proxy_url_2, '')
            FROM accounts a
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
                row.get::<_, i64>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, String>(10)?,
            ))
        })?;

        let mut credentials = Vec::new();
        for row in rows {
            let (
                id,
                email,
                provider,
                account_type,
                client_id,
                refresh_token,
                imap_host,
                imap_port,
                account_proxy,
                account_proxy_1,
                account_proxy_2,
            ) = row?;
            let proxy_chain =
                proxy_chain_from_values(&[&account_proxy, &account_proxy_1, &account_proxy_2])?;
            credentials.push(AccountCredentials {
                id,
                email,
                provider,
                account_type,
                client_id: crypto::decrypt_text(&client_id, key)?,
                refresh_token: crypto::decrypt_text(&refresh_token, key)?,
                imap_host,
                imap_port,
                proxy_chain,
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

    fn upsert_provider_messages(
        &self,
        account_id: i64,
        messages: &[ProviderMessage],
    ) -> AppResult<()> {
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

    fn cached_imap_raw_mime(
        &self,
        account_id: i64,
        message_id: &str,
        folder: Option<&str>,
    ) -> AppResult<Vec<u8>> {
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
        let raw_mime = raw_mime
            .ok_or_else(|| AppError::InvalidInput("cached IMAP message not found".to_string()))?;
        if raw_mime.is_empty() {
            return Err(AppError::InvalidInput(
                "cached IMAP raw MIME is missing; refresh the account before downloading this attachment".to_string(),
            ));
        }
        Ok(raw_mime)
    }

    fn cached_message_attachments(
        &self,
        account_id: i64,
        message_id: &str,
        folder: Option<&str>,
    ) -> AppResult<Vec<AttachmentInfo>> {
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
        let attachments_json = attachments_json
            .ok_or_else(|| AppError::InvalidInput("cached message not found".to_string()))?;
        Ok(parse_attachments_json(&attachments_json))
    }

    fn mark_account_refresh_success(
        &self,
        account_id: i64,
        _email: &str,
        _count: usize,
    ) -> AppResult<()> {
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
        Ok(())
    }

    fn mark_account_refresh_failed(
        &self,
        account_id: i64,
        _email: &str,
        error: &str,
    ) -> AppResult<()> {
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
        Ok(())
    }

    fn scheduler_due(
        &self,
        key: &str,
        interval_minutes: i64,
        now: DateTime<Utc>,
    ) -> AppResult<bool> {
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
            SELECT m.id, m.account_id, m.folder, m.provider_message_id
            FROM retained_mail_messages m
            WHERE m.id IN ({placeholders})
            "
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(ids.iter()), |row| {
            Ok(MailMessageRef {
                id: row.get(0)?,
                account_id: row.get(1)?,
                folder: row.get(2)?,
                provider_message_id: row.get(3)?,
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

    fn write_export_file(
        &self,
        category: &str,
        file_name: &str,
        bytes: &[u8],
    ) -> AppResult<(String, i64)> {
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
        require_provider_capability(&account, "mark_read", "mark read/unread")?;
        match mail_provider_adapter(&account)? {
            MailProviderAdapter::Graph => {
                providers::mark_graph_message_read(&account, &target.provider_message_id, is_read)
            }
            MailProviderAdapter::Imap => providers::mark_imap_message_read(
                &account,
                &target.folder,
                &target.provider_message_id,
                is_read,
            ),
        }
    }

    fn refresh_account_credential(
        &self,
        account: &AccountCredentials,
        folder: &str,
        top: usize,
    ) -> AppResult<usize> {
        require_provider_capability(account, "read_mail", "mail refresh")?;
        match mail_provider_adapter(account)? {
            MailProviderAdapter::Graph => providers::fetch_graph_messages(account, folder, top)
                .and_then(|(next_refresh_token, messages)| {
                    if !next_refresh_token.is_empty() && next_refresh_token != account.refresh_token
                    {
                        self.save_refresh_token(account.id, &next_refresh_token)?;
                    }
                    self.upsert_provider_messages(account.id, &messages)?;
                    Ok(messages.len())
                }),
            MailProviderAdapter::Imap => providers::fetch_imap_messages(account, folder, top)
                .and_then(|messages| {
                    self.upsert_provider_messages(account.id, &messages)?;
                    Ok(messages.len())
                }),
        }
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
        require_provider_delete_capability(&account)?;
        match mail_provider_adapter(&account)? {
            MailProviderAdapter::Graph => {
                providers::delete_graph_message(&account, &target.provider_message_id)
            }
            MailProviderAdapter::Imap => providers::delete_imap_message(
                &account,
                &target.folder,
                &target.provider_message_id,
            ),
        }
    }

    fn audit(
        &self,
        action: &str,
        resource_type: &str,
        resource_id: Option<i64>,
        detail: &str,
    ) -> AppResult<()> {
        self.conn.execute(
            "
            INSERT INTO audit_logs (action, resource_type, resource_id, detail)
            VALUES (?, ?, ?, ?)
            ",
            params![
                action,
                resource_type,
                resource_id.map(|id| id.to_string()),
                detail
            ],
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
            "login password must be at least 8 characters".to_string(),
        ));
    }
    Ok(())
}

fn validate_login_username(username: &str) -> AppResult<()> {
    if username.trim().eq_ignore_ascii_case(DEFAULT_LOGIN_USERNAME) {
        return Ok(());
    }
    Err(AppError::Unauthorized)
}

fn reencrypt_secret_value(
    value: &str,
    old_key: &[u8; 32],
    new_key: &[u8; 32],
) -> AppResult<String> {
    if value.is_empty() {
        return Ok(String::new());
    }
    let plaintext = crypto::decrypt_text(value, old_key)?;
    crypto::encrypt_text(&plaintext, new_key)
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
        return Err(AppError::InvalidInput(
            "select at least one message".to_string(),
        ));
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
    match providers::normalize_mail_provider_id(&provider) {
        Some("graph") => Ok(Some("graph".to_string())),
        Some("imap") => Ok(Some("imap".to_string())),
        Some("gmail") => Err(AppError::InvalidInput(
            "Gmail OAuth is disabled; use IMAP app password".to_string(),
        )),
        _ => Err(AppError::InvalidInput(format!(
            "unsupported OAuth provider: {provider}"
        ))),
    }
}

fn oauth_account_type(provider: &str) -> String {
    providers::mail_provider_definition(provider)
        .map(|definition| definition.account_type.to_string())
        .unwrap_or_else(|| provider.to_string())
}

fn exchange_oauth_code_for_provider(
    provider: &str,
    client_id: &str,
    redirect_uri: &str,
    code_or_url: &str,
    _code_verifier: Option<&str>,
) -> AppResult<providers::OAuthTokenResponse> {
    match provider {
        "gmail" => Err(AppError::InvalidInput(
            "Gmail OAuth is disabled; use IMAP app password".to_string(),
        )),
        "graph" | "imap" => {
            providers::exchange_microsoft_code(client_id, redirect_uri, code_or_url, Some(provider))
        }
        value => Err(AppError::InvalidInput(format!(
            "unsupported OAuth provider: {value}"
        ))),
    }
}

fn normalize_read_state(value: Option<&str>) -> AppResult<String> {
    match value.unwrap_or("all").trim().to_ascii_lowercase().as_str() {
        "" | "all" => Ok("all".to_string()),
        "read" => Ok("read".to_string()),
        "unread" => Ok("unread".to_string()),
        _ => Err(AppError::InvalidInput(
            "read_state must be all, read, or unread".to_string(),
        )),
    }
}

#[cfg(test)]
fn classify_error_category(error: &str) -> &'static str {
    let lower = error.to_ascii_lowercase();
    if lower.trim().is_empty() {
        return "none";
    }
    if lower.contains("auth")
        || lower.contains("unauthorized")
        || lower.contains("forbidden")
        || lower.contains("http 401")
        || lower.contains("http 403")
        || lower.contains("status 401")
        || lower.contains("status 403")
        || lower.contains("invalid_grant")
        || lower.contains("insufficientpermissions")
        || lower.contains("insufficient permissions")
        || lower.contains("insufficient privileges")
        || lower.contains("scope")
        || lower.contains("credential")
        || lower.contains("password")
        || lower.contains("token")
        || lower.contains("login")
        || lower.contains("授权码")
        || lower.contains("授权密码")
        || lower.contains("客户端授权")
        || lower.contains("登录密码")
        || lower.contains("未授权")
        || lower.contains("权限不足")
        || lower.contains("认证")
        || lower.contains("鉴权")
        || lower.contains("令牌")
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
        || lower.contains("graph")
        || lower.contains("http 5")
    {
        return "provider";
    }
    "unknown"
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

fn mail_action_message(action: &str, changed: usize, failed: usize, errors: &[String]) -> String {
    if failed == 0 {
        return format!("{action} {changed} message(s)");
    }
    let preview = errors
        .iter()
        .take(3)
        .cloned()
        .collect::<Vec<_>>()
        .join("; ");
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
        html.push_str(&html_escape(if row.subject.is_empty() {
            "(no subject)"
        } else {
            &row.subject
        }));
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
            (
                "Status",
                if row.is_read { "Read" } else { "Unread" }.to_string(),
            ),
            ("Body type", row.body_type.clone()),
        ] {
            html.push_str("<span>");
            html.push_str(&html_escape(label));
            html.push_str("</span><strong>");
            html.push_str(&html_escape(&value));
            html.push_str("</strong>");
        }
        html.push_str("</div><pre>");
        html.push_str(&html_escape(
            row.body.as_deref().unwrap_or(&row.body_preview),
        ));
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
    value
        .map(|item| normalize_proxy_value(Some(item)))
        .transpose()
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
            "subject" | "sub" => search
                .terms
                .push(MailSearchTerm::Subject(raw_value.to_string())),
            "from" | "sender" => search
                .terms
                .push(MailSearchTerm::Sender(raw_value.to_string())),
            "to" | "recipient" | "recipients" | "cc" => search
                .terms
                .push(MailSearchTerm::Recipient(raw_value.to_string())),
            "body" | "content" | "text" => search
                .terms
                .push(MailSearchTerm::Body(raw_value.to_string())),
            "id" | "message" | "message_id" => search
                .terms
                .push(MailSearchTerm::ProviderId(raw_value.to_string())),
            "folder" | "mailbox" => search.folder = Some(normalize_mail_folder(raw_value)),
            "is" | "status" => match raw_value.to_ascii_lowercase().as_str() {
                "read" => search.read_state = Some("read".to_string()),
                "unread" => search.read_state = Some("unread".to_string()),
                _ => search.terms.push(MailSearchTerm::Any(token.to_string())),
            },
            "has" => match raw_value.to_ascii_lowercase().as_str() {
                "attachment" | "attachments" | "file" | "files" => {
                    search.has_attachments = Some(true)
                }
                "noattachment" | "noattachments" | "nofile" | "nofiles" => {
                    search.has_attachments = Some(false)
                }
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

fn append_mail_search_terms(
    sql: &mut String,
    values: &mut Vec<SqlValue>,
    terms: &[MailSearchTerm],
) {
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
    let order = match sort_order
        .unwrap_or("desc")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "" | "desc" => "DESC",
        "asc" => "ASC",
        value => {
            return Err(AppError::InvalidInput(format!(
                "unsupported mail sort_order: {value}"
            )))
        }
    };
    let clause = match sort_by
        .unwrap_or("date")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "" | "date" | "received" | "received_at" => {
            format!("m.received_at_sort {order}, m.id {order}")
        }
        "subject" => format!("LOWER(m.subject) {order}, m.received_at_sort DESC, m.id DESC"),
        "sender" | "from" => format!("LOWER(m.sender) {order}, m.received_at_sort DESC, m.id DESC"),
        "read" | "status" => format!("m.is_read {order}, m.received_at_sort DESC, m.id DESC"),
        "attachments" | "files" => {
            format!("m.has_attachments {order}, m.received_at_sort DESC, m.id DESC")
        }
        "folder" => format!("m.folder {order}, m.received_at_sort DESC, m.id DESC"),
        value => {
            return Err(AppError::InvalidInput(format!(
                "unsupported mail sort_by: {value}"
            )))
        }
    };
    Ok(clause)
}

fn preview_secret(value: &str) -> String {
    if value.len() <= 10 {
        return "*".repeat(value.len());
    }
    format!("{}...{}", &value[..4], &value[value.len() - 4..])
}

enum MailProviderAdapter {
    Graph,
    Imap,
}

fn mail_provider_adapter(account: &AccountCredentials) -> AppResult<MailProviderAdapter> {
    let provider = account.provider.to_ascii_lowercase();
    let account_type = account.account_type.to_ascii_lowercase();
    match provider.as_str() {
        "graph" | "outlook" => return Ok(MailProviderAdapter::Graph),
        "gmail" => return Ok(MailProviderAdapter::Imap),
        "imap" | "imap_custom" | "qq" | "netease_163" => return Ok(MailProviderAdapter::Imap),
        _ => {}
    }
    match account_type.as_str() {
        "outlook" | "graph" => Ok(MailProviderAdapter::Graph),
        "gmail" => Ok(MailProviderAdapter::Imap),
        "imap" => Ok(MailProviderAdapter::Imap),
        _ if !account.client_id.is_empty() && !account.refresh_token.is_empty() => {
            Ok(MailProviderAdapter::Graph)
        }
        _ => Ok(MailProviderAdapter::Imap),
    }
}

fn require_provider_capability(
    account: &AccountCredentials,
    capability: &str,
    action: &str,
) -> AppResult<()> {
    if account_provider_supports_capability(account, capability) {
        return Ok(());
    }
    Err(AppError::InvalidInput(format!(
        "mail provider {} does not support {action}",
        account.provider
    )))
}

fn require_provider_delete_capability(account: &AccountCredentials) -> AppResult<()> {
    if account_provider_supports_capability(account, "trash")
        || account_provider_supports_capability(account, "remote_delete")
    {
        return Ok(());
    }
    Err(AppError::InvalidInput(format!(
        "mail provider {} does not support remote delete",
        account.provider
    )))
}

fn account_provider_supports_capability(account: &AccountCredentials, capability: &str) -> bool {
    providers::mail_provider_supports_capability(&account.provider, capability)
        || providers::mail_provider_supports_capability(&account.account_type, capability)
}

fn split_domains(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}

fn normalize_worker_url(value: &str) -> AppResult<String> {
    let raw = value.trim();
    let candidate = if raw.starts_with("http://") || raw.starts_with("https://") {
        raw.to_string()
    } else {
        format!("https://{raw}")
    };
    let url = reqwest::Url::parse(&candidate)
        .map_err(|_| AppError::InvalidInput("Cloudflare Worker URL is invalid".to_string()))?;
    if url.scheme() != "https" && url.scheme() != "http" {
        return Err(AppError::InvalidInput(
            "Cloudflare Worker URL must use HTTP or HTTPS".to_string(),
        ));
    }
    Ok(candidate.trim_end_matches('/').to_string())
}

fn is_valid_email_address(value: &str) -> bool {
    let value = value.trim();
    let mut parts = value.split('@');
    matches!((parts.next(), parts.next(), parts.next()), (Some(local), Some(domain), None) if !local.is_empty() && domain.contains('.') && !value.chars().any(char::is_whitespace))
}

fn cloudflare_import_header(value: &str) -> Option<Option<String>> {
    let trimmed = value.trim();
    if !trimmed.starts_with('[') || !trimmed.ends_with(']') {
        return None;
    }
    let inner = &trimmed[1..trimmed.len() - 1];
    if inner.eq_ignore_ascii_case("cloudflare") {
        return Some(None);
    }
    let (prefix, name) = inner.split_once(':')?;
    if !prefix.eq_ignore_ascii_case("cloudflare") {
        return None;
    }
    Some(Some(name.trim().to_string()))
}

fn normalize_batch_usernames(values: Option<Vec<String>>, count: usize) -> AppResult<Vec<String>> {
    let Some(values) = values.filter(|items| !items.is_empty()) else {
        return Ok((0..count)
            .map(|_| {
                format!(
                    "mail{}",
                    Uuid::new_v4()
                        .simple()
                        .to_string()
                        .chars()
                        .take(10)
                        .collect::<String>()
                )
            })
            .collect());
    };
    let mut usernames = Vec::new();
    let mut seen = HashSet::new();
    for value in values {
        let local = value
            .trim()
            .split('@')
            .next()
            .unwrap_or_default()
            .to_lowercase();
        let username = local
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric())
            .take(32)
            .collect::<String>();
        if username.len() < 3 {
            return Err(AppError::InvalidInput(format!(
                "invalid batch username: {value}"
            )));
        }
        if !seen.insert(username.clone()) {
            return Err(AppError::InvalidInput(format!(
                "duplicate batch username: {username}"
            )));
        }
        usernames.push(username);
    }
    if usernames.len() != count {
        return Err(AppError::InvalidInput(format!(
            "username count must equal requested count ({count})"
        )));
    }
    Ok(usernames)
}

fn table_columns(conn: &Connection, table: &str) -> AppResult<Vec<String>> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    collect_rows(rows)
}

#[cfg(test)]
fn table_exists(conn: &Connection, table: &str) -> AppResult<bool> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ? LIMIT 1",
            [table],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

fn attachment_dir(db_path: &Path) -> AppResult<PathBuf> {
    Ok(db_path
        .parent()
        .ok_or_else(|| AppError::Internal("database path has no parent directory".to_string()))?
        .join("attachments"))
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
        let metadata = std::fs::symlink_metadata(entry.path())
            .map_err(|err| AppError::Internal(err.to_string()))?;
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
            return Err(AppError::Internal(
                "refusing to remove path outside local data directory".to_string(),
            ));
        }
        let metadata =
            std::fs::symlink_metadata(&path).map_err(|err| AppError::Internal(err.to_string()))?;
        if metadata.file_type().is_dir() {
            std::fs::remove_dir_all(&path).map_err(|err| AppError::Internal(err.to_string()))?;
        } else {
            std::fs::remove_file(&path).map_err(|err| AppError::Internal(err.to_string()))?;
        }
    }
    Ok((files, bytes))
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
        return Err(AppError::InvalidInput(
            "zip bundle requires at least one file".to_string(),
        ));
    }
    let mut data = Vec::new();
    let mut central_entries = Vec::new();
    for (name, bytes) in files {
        let name_bytes = name.as_bytes();
        let name_len = zip_u16(name_bytes.len(), "zip file name is too long")?;
        let size = zip_u32(
            bytes.len(),
            "attachment is too large for a standard ZIP bundle",
        )?;
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
    let central_offset = zip_u32(
        central_offset_usize,
        "zip central directory offset is too large",
    )?;
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

    let central_size = zip_u32(
        data.len() - central_offset_usize,
        "zip central directory is too large",
    )?;
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

fn temp_email_message_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TempEmailMessage> {
    Ok(TempEmailMessage {
        id: row.get(0)?,
        sender: row.get(1)?,
        recipients: row.get(2)?,
        subject: row.get(3)?,
        body_preview: row.get(4)?,
        body: row.get(5)?,
        body_type: row.get(6)?,
        received_at: row.get(7)?,
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

fn map_markdown_document_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MarkdownDocument> {
    Ok(MarkdownDocument {
        id: row.get(0)?,
        title: row.get(1)?,
        content: row.get(2)?,
        category_id: row.get(3)?,
        category_name: row.get(4)?,
        source_path: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

fn validate_markdown_category_name(value: &str) -> AppResult<&str> {
    let value = value.trim();
    if value.is_empty() {
        return Err(AppError::InvalidInput(
            "markdown category name is required".to_string(),
        ));
    }
    if value.chars().count() > 80 {
        return Err(AppError::InvalidInput(
            "markdown category name is too long".to_string(),
        ));
    }
    Ok(value)
}

fn validate_markdown_parent(
    conn: &Connection,
    current_id: Option<i64>,
    parent_id: Option<i64>,
) -> AppResult<()> {
    let Some(mut parent_id) = parent_id else {
        return Ok(());
    };
    if current_id == Some(parent_id) {
        return Err(AppError::InvalidInput(
            "markdown folder cannot contain itself".to_string(),
        ));
    }
    let mut depth = 1;
    loop {
        let next_parent = conn
            .query_row(
                "SELECT parent_id FROM markdown_categories WHERE id = ?",
                [parent_id],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()?
            .ok_or_else(|| {
                AppError::InvalidInput("markdown parent folder not found".to_string())
            })?;
        if current_id == Some(parent_id) {
            return Err(AppError::InvalidInput(
                "markdown folder cannot move under its descendant".to_string(),
            ));
        }
        let Some(next_parent) = next_parent else {
            break;
        };
        parent_id = next_parent;
        depth += 1;
        if depth >= 5 {
            return Err(AppError::InvalidInput(
                "markdown folders support at most 5 levels".to_string(),
            ));
        }
    }
    Ok(())
}

fn validate_markdown_title(value: &str) -> AppResult<&str> {
    let value = value.trim();
    if value.is_empty() {
        return Err(AppError::InvalidInput(
            "markdown document title is required".to_string(),
        ));
    }
    if value.chars().count() > 200 {
        return Err(AppError::InvalidInput(
            "markdown document title is too long".to_string(),
        ));
    }
    Ok(value)
}

fn validate_markdown_content(value: String) -> AppResult<String> {
    const MAX_MARKDOWN_BYTES: usize = 25 * 1024 * 1024;
    if value.len() > MAX_MARKDOWN_BYTES {
        return Err(AppError::InvalidInput(
            "markdown document exceeds the 25 MB limit".to_string(),
        ));
    }
    Ok(value)
}

fn validate_markdown_category_reference(
    conn: &Connection,
    category_id: Option<i64>,
) -> AppResult<()> {
    let Some(category_id) = category_id else {
        return Ok(());
    };
    let exists = conn
        .query_row(
            "SELECT 1 FROM markdown_categories WHERE id = ?",
            [category_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .is_some();
    if !exists {
        return Err(AppError::InvalidInput(
            "markdown category not found".to_string(),
        ));
    }
    Ok(())
}

fn normalize_markdown_source_path(value: Option<String>) -> Option<String> {
    value.and_then(|path| {
        let path = path.trim();
        if path.is_empty() {
            None
        } else {
            Some(path.to_string())
        }
    })
}

#[cfg(test)]
mod project_tests {
    use super::{
        attachment_dir, classify_error_category, cloudflare_import_header, exports_dir,
        normalize_batch_usernames, normalize_oauth_provider, table_columns, table_exists, Database,
    };
    use crate::crypto;
    use crate::error::AppError;
    use crate::import::ImportedAccount;
    use crate::models::{
        AccountBatchInput, AttachmentInfo, ClearLocalDataInput, CreateGroupInput,
        CreateMailShareInput, CreateMarkdownCategoryInput, CreateMarkdownDocumentInput,
        DeleteMailMessagesInput, DownloadAllAttachmentsInput, DownloadAttachmentInput,
        ExportAccountSecretsInput, ExportAccountsInput, ExportMailMessagesInput,
        GenerateWorkspaceKeyInput, ImportTempEmailsInput, LoginInput, MailMessageQuery,
        MarkMailMessagesInput, RefreshInput, RevealAccountSecretsInput, RevokeMailShareInput,
        SaveCloudflareChannelInput, TempEmailMessage, UpdateAccountInput, UpdateGroupInput,
        UpdateMarkdownCategoryInput, UpdateMarkdownDocumentInput, UpdateWorkspaceKeyRecordInput,
    };
    use rusqlite::{params, Connection};
    use std::path::PathBuf;

    #[test]
    fn local_desktop_workflow_covers_core_e2e_paths() {
        let root = std::env::temp_dir().join(format!(
            "outlook-email-e2e-workflow-test-{}",
            uuid::Uuid::new_v4()
        ));
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
                    provider: None,
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
        assert!(!refresh.success);
        assert_eq!(refresh.failed, 1);

        let message = db.create_demo_message(account.id).expect("demo message");
        let marked = db
            .mark_mail_messages(MarkMailMessagesInput {
                message_ids: vec![message.id],
                is_read: true,
                sync_remote: Some(false),
            })
            .expect("mark read");
        assert_eq!(marked.refreshed, 1);

        let deleted = db
            .delete_mail_messages(DeleteMailMessagesInput {
                message_ids: vec![message.id],
                sync_remote: Some(false),
            })
            .expect("delete local");
        assert_eq!(deleted.refreshed, 1);
        assert!(db
            .list_messages(Some(account.id), Some("all".to_string()))
            .expect("messages after delete")
            .is_empty());
    }

    #[test]
    fn import_accounts_detects_mail_provider_presets() {
        let root = std::env::temp_dir().join(format!(
            "outlook-email-provider-import-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).expect("create temp dir");
        let db_path = root.join("providers.sqlite");
        let conn = Connection::open(&db_path).expect("open db");
        let mut db = Database {
            conn,
            db_path,
            crypto_key: Some([8; 32]),
        };
        db.initialize_schema().expect("schema");

        db.import_accounts(
            vec![
                ImportedAccount {
                    email: "user@qq.com".to_string(),
                    password: "qq-auth-code".to_string(),
                    client_id: String::new(),
                    refresh_token: String::new(),
                    remark: String::new(),
                    provider: None,
                },
                ImportedAccount {
                    email: "manual@example.com".to_string(),
                    password: "app-password".to_string(),
                    client_id: String::new(),
                    refresh_token: String::new(),
                    remark: String::new(),
                    provider: Some("163".to_string()),
                },
                ImportedAccount {
                    email: "person@gmail.com".to_string(),
                    password: "gmail-app-password".to_string(),
                    client_id: String::new(),
                    refresh_token: String::new(),
                    remark: String::new(),
                    provider: Some("gmail".to_string()),
                },
            ],
            None,
        )
        .expect("import providers");

        let accounts = db.list_accounts().expect("list accounts");
        let qq = accounts
            .iter()
            .find(|account| account.email == "user@qq.com")
            .expect("qq account");
        assert_eq!(qq.provider, "qq");
        assert_eq!(qq.account_type, "imap");
        assert_eq!(qq.imap_host, "imap.qq.com");
        assert!(qq.has_refresh_token);
        assert!(!qq.has_client_id);

        let netease = accounts
            .iter()
            .find(|account| account.email == "manual@example.com")
            .expect("163 account");
        assert_eq!(netease.provider, "netease_163");
        assert_eq!(netease.account_type, "imap");
        assert_eq!(netease.imap_host, "imap.163.com");
        assert!(netease.has_refresh_token);

        let gmail = accounts
            .iter()
            .find(|account| account.email == "person@gmail.com")
            .expect("gmail account");
        assert_eq!(gmail.provider, "gmail");
        assert_eq!(gmail.account_type, "imap");
        assert_eq!(gmail.imap_host, "imap.gmail.com");
        assert!(gmail.has_refresh_token);
        assert!(!gmail.has_client_id);
    }

    #[test]
    fn normalize_oauth_provider_rejects_google_aliases() {
        assert!(normalize_oauth_provider(Some("google")).is_err());
        assert!(normalize_oauth_provider(Some("gmail")).is_err());
        assert_eq!(
            normalize_oauth_provider(Some("outlook")).expect("outlook"),
            Some("graph".to_string())
        );
        assert_eq!(
            normalize_oauth_provider(Some("imap")).expect("imap"),
            Some("imap".to_string())
        );
        assert!(normalize_oauth_provider(Some("qq")).is_err());
    }

    #[test]
    fn account_batch_updates_delete_and_selected_export() {
        let root = std::env::temp_dir().join(format!(
            "outlook-email-account-batch-test-{}",
            uuid::Uuid::new_v4()
        ));
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
                parent_id: None,
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

        let moved = db
            .batch_accounts(AccountBatchInput {
                account_ids: vec![1, 2, 2, 999],
                action: "move_group".to_string(),
                group_id: Some(batch_group.id),
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
        })
        .expect("delete accounts");
        assert_eq!(db.list_accounts().expect("accounts").len(), 1);
    }

    #[test]
    fn account_secrets_require_local_password() {
        let root = std::env::temp_dir().join(format!(
            "outlook-email-secret-export-test-{}",
            uuid::Uuid::new_v4()
        ));
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
                provider: None,
            }],
            Some(1),
        )
        .expect("import account");

        let secrets = db
            .reveal_account_secrets(RevealAccountSecretsInput {
                account_id: 1,
                password: "admin123".to_string(),
            })
            .expect("reveal secrets");
        assert_eq!(secrets.client_id, "client-id-value");
        assert_eq!(secrets.refresh_token_preview, "refr...alue");

        let denied = db.reveal_account_secrets(RevealAccountSecretsInput {
            account_id: 1,
            password: "wrong-password".to_string(),
        });
        assert!(matches!(denied, Err(AppError::Unauthorized)));

        let bad_confirm = db.export_account_secrets(ExportAccountSecretsInput {
            account_ids: vec![1],
            password: "admin123".to_string(),
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
                password: "admin123".to_string(),
                confirm: "EXPORT ACCOUNT SECRETS".to_string(),
            })
            .expect("export account secrets");
        assert_eq!(exported.item_count, 1);
        let csv = std::fs::read_to_string(&exported.path).expect("read secret export");
        assert!(csv.contains("secret@example.com"));
        assert!(csv.contains("client-id-value"));
        assert!(csv.contains("refresh-token-value"));
    }

    #[test]
    fn legacy_password_key_migrates_to_default_login_and_workspace_key() {
        let conn = Connection::open_in_memory().expect("open memory db");
        let mut db = Database {
            conn,
            db_path: PathBuf::from("memory.sqlite"),
            crypto_key: None,
        };
        db.initialize_schema().expect("schema");
        db.ensure_default_data().expect("defaults");

        let legacy_password = "old-local-password";
        let legacy_salt = crypto::random_salt();
        let legacy_hash = crypto::hash_password(legacy_password).expect("hash");
        let legacy_key = crypto::derive_key(legacy_password, &legacy_salt);
        db.set_config("password_hash", &legacy_hash)
            .expect("hash config");
        db.set_config("crypto_salt", &legacy_salt)
            .expect("salt config");
        db.crypto_key = Some(legacy_key);
        db.import_accounts(
            vec![ImportedAccount {
                email: "legacy@example.com".to_string(),
                password: "legacy-password".to_string(),
                client_id: "legacy-client".to_string(),
                refresh_token: "legacy-refresh".to_string(),
                remark: String::new(),
                provider: None,
            }],
            Some(1),
        )
        .expect("legacy import");
        db.lock();

        db.login(LoginInput {
            username: "admin".to_string(),
            password: legacy_password.to_string(),
        })
        .expect("legacy login migrates");
        assert!(db
            .get_config("workspace_key_enc")
            .expect("workspace key")
            .is_some());
        assert!(crypto::verify_password(
            "admin123",
            &db.get_config("password_hash")
                .expect("password hash")
                .unwrap()
        )
        .expect("verify default password"));

        let secrets = db
            .reveal_account_secrets(RevealAccountSecretsInput {
                account_id: 1,
                password: "admin123".to_string(),
            })
            .expect("reveal migrated secrets");
        assert_eq!(secrets.client_id, "legacy-client");
        assert_eq!(secrets.refresh_token_preview, "lega...resh");

        db.lock();
        assert!(matches!(
            db.login(LoginInput {
                username: "admin".to_string(),
                password: legacy_password.to_string(),
            }),
            Err(AppError::Unauthorized)
        ));
        db.login(LoginInput {
            username: "admin".to_string(),
            password: "admin123".to_string(),
        })
        .expect("default login after migration");
    }

    #[test]
    fn workspace_key_records_generate_once_and_list_metadata_only() {
        let conn = Connection::open_in_memory().expect("open memory db");
        let mut db = Database {
            conn,
            db_path: PathBuf::from("memory.sqlite"),
            crypto_key: Some([9; 32]),
        };
        db.initialize_schema().expect("schema");

        let generated = db
            .generate_workspace_key(GenerateWorkspaceKeyInput {
                purpose: "测试用途".to_string(),
            })
            .expect("generate workspace key");
        assert_eq!(generated.record.purpose, "测试用途");
        assert!(!generated.workspace_key.is_empty());
        assert_eq!(
            generated.record.key_fingerprint,
            crypto::workspace_key_fingerprint(&generated.workspace_key)
        );

        let listed = db
            .list_workspace_key_records()
            .expect("list workspace keys");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].purpose, "测试用途");
        assert_eq!(listed[0].key_fingerprint, generated.record.key_fingerprint);

        db.delete_workspace_key_record(listed[0].id)
            .expect("delete workspace key record");
        assert!(db.list_workspace_key_records().expect("list").is_empty());

        let first = db
            .generate_workspace_key(GenerateWorkspaceKeyInput {
                purpose: String::new(),
            })
            .expect("default purpose 1");
        assert_eq!(first.record.purpose, "密钥_1");

        let second = db
            .generate_workspace_key(GenerateWorkspaceKeyInput {
                purpose: "   ".to_string(),
            })
            .expect("default purpose 2");
        assert_eq!(second.record.purpose, "密钥_2");

        let custom = db
            .generate_workspace_key(GenerateWorkspaceKeyInput {
                purpose: "自定义用途".to_string(),
            })
            .expect("custom purpose");
        assert_eq!(custom.record.purpose, "自定义用途");

        let third = db
            .generate_workspace_key(GenerateWorkspaceKeyInput {
                purpose: String::new(),
            })
            .expect("default purpose 3");
        assert_eq!(third.record.purpose, "密钥_3");

        let updated = db
            .update_workspace_key_record(UpdateWorkspaceKeyRecordInput {
                id: third.record.id,
                purpose: "更新后的用途".to_string(),
            })
            .expect("update workspace key purpose");
        assert_eq!(updated.purpose, "更新后的用途");
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
            mail_retention_days: None,
            client_id: None,
            refresh_token: None,
            aliases: Some(aliases.into_iter().map(str::to_string).collect()),
        };

        let updated = db
            .update_account(account_input(
                1,
                "one@example.com",
                vec!["Alias@One.com", "alias-one@example.com", "ALIAS@ONE.COM"],
            ))
            .expect("update aliases");
        assert_eq!(
            updated.aliases,
            vec!["alias@one.com", "alias-one@example.com"]
        );

        let listed = db
            .list_accounts()
            .expect("list accounts")
            .into_iter()
            .find(|account| account.id == 1)
            .expect("listed account");
        assert_eq!(
            listed.aliases,
            vec!["alias@one.com", "alias-one@example.com"]
        );

        let primary_conflict =
            db.update_account(account_input(1, "one@example.com", vec!["two@example.com"]));
        assert!(matches!(primary_conflict, Err(AppError::InvalidInput(_))));

        let alias_conflict =
            db.update_account(account_input(2, "two@example.com", vec!["alias@one.com"]));
        assert!(matches!(alias_conflict, Err(AppError::InvalidInput(_))));
    }

    #[test]
    fn account_proxy_chain_uses_account_proxies() {
        let conn = Connection::open_in_memory().expect("open memory db");
        let mut db = Database {
            conn,
            db_path: PathBuf::from("memory.sqlite"),
            crypto_key: Some([7; 32]),
        };
        db.initialize_schema().expect("schema");
        db.conn
            .execute(
                "INSERT INTO accounts (id, email, status, group_id) VALUES (1, 'proxy@example.com', 'active', 1)",
                [],
            )
            .expect("insert account");

        let empty = db.account_credentials(Some(1)).expect("credentials");
        assert!(empty[0].proxy_chain.is_empty());

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
            mail_retention_days: Some(90),
            client_id: None,
            refresh_token: None,
            aliases: None,
        })
        .expect("set account proxy");
        assert_eq!(
            db.list_accounts().expect("accounts")[0].mail_retention_days,
            90
        );

        let overridden = db.account_credentials(Some(1)).expect("credentials");
        assert_eq!(
            overridden[0].proxy_chain,
            vec!["http://account-proxy:8080", "https://account-backup:8443"]
        );
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
                parent_id: Some(1),
            })
            .expect("create child");
        let grandchild = db
            .create_group(CreateGroupInput {
                name: "Grandchild".to_string(),
                description: None,
                parent_id: Some(child.id),
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
                parent_id: None,
                sort_order: Some(2),
            })
            .expect("move child");
        assert_eq!(moved.level, 1);
        assert_eq!(db.get_group(grandchild.id).expect("grandchild").level, 2);

        let cycle = db.update_group(UpdateGroupInput {
            id: child.id,
            name: "Cycle".to_string(),
            description: None,
            parent_id: Some(grandchild.id),
            sort_order: None,
        });
        assert!(matches!(cycle, Err(AppError::InvalidInput(_))));

        db.delete_group(child.id).expect("delete child");
        let account_group = db
            .conn
            .query_row("SELECT group_id FROM accounts WHERE id = 1", [], |row| {
                row.get::<_, Option<i64>>(0)
            })
            .expect("account group");
        assert_eq!(account_group, None);
        let promoted = db.get_group(grandchild.id).expect("promoted grandchild");
        assert_eq!(promoted.parent_id, None);
        assert_eq!(promoted.level, 1);
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
        assert!(
            db.list_messages(Some(1), Some("all".to_string()))
                .expect("messages")[0]
                .is_read
        );

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
                search: Some(
                    "from:alice subject:\"Reset Password\" is:unread has:attachment".to_string(),
                ),
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
            sorted
                .into_iter()
                .map(|message| message.subject)
                .collect::<Vec<_>>(),
            vec!["Alpha notice", "Beta invoice", "Reset Password"]
        );
    }

    #[test]
    fn exports_mail_html_and_accounts_csv() {
        let root = std::env::temp_dir().join(format!(
            "outlook-email-export-test-{}",
            uuid::Uuid::new_v4()
        ));
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
        let root =
            std::env::temp_dir().join(format!("outlook-email-share-test-{}", uuid::Uuid::new_v4()));
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
        let shares = db
            .list_mail_share_records(Some(10))
            .expect("shares after expire");
        let expired = shares
            .iter()
            .find(|item| item.id == expired.id)
            .expect("expired share");
        assert_eq!(expired.status, "expired");
    }

    #[test]
    fn reports_and_clears_local_retention_data() {
        let root = std::env::temp_dir().join(format!(
            "outlook-email-retention-test-{}",
            uuid::Uuid::new_v4()
        ));
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
        assert_eq!(summary.attachment_file_count, 1);
        assert_eq!(summary.export_file_count, 1);

        assert!(db
            .clear_local_data(ClearLocalDataInput {
                clear_mail_cache: Some(true),
                clear_attachments: None,
                clear_exports: None,
                confirm: "wrong".to_string(),
            })
            .is_err());

        let result = db
            .clear_local_data(ClearLocalDataInput {
                clear_mail_cache: Some(true),
                clear_attachments: Some(true),
                clear_exports: Some(true),
                confirm: "CLEAR LOCAL DATA".to_string(),
            })
            .expect("clear local data");
        assert_eq!(result.deleted_messages, 1);
        assert_eq!(result.deleted_files, 2);
        assert!(result.freed_bytes > 0);

        let summary = db.local_retention_summary().expect("summary after clear");
        assert_eq!(summary.mail_message_count, 0);
        assert_eq!(summary.attachment_file_count, 0);
        assert_eq!(summary.export_file_count, 0);
    }

    #[test]
    fn downloads_imap_attachment_from_cached_raw_mime() {
        let root = std::env::temp_dir().join(format!(
            "outlook-email-imap-attachment-test-{}",
            uuid::Uuid::new_v4()
        ));
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
        assert_eq!(
            std::fs::read(&result.path).expect("read attachment"),
            b"Hello IMAP attachment"
        );

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
        assert!(zip
            .windows(b"note.txt".len())
            .any(|window| window == b"note.txt"));
        assert!(zip
            .windows(b"Hello IMAP attachment".len())
            .any(|window| window == b"Hello IMAP attachment"));
    }

    #[test]
    fn classifies_provider_specific_credential_errors() {
        assert_eq!(
            classify_error_category(
                "IMAP login failed: NO [AUTHENTICATIONFAILED] invalid Gmail app password"
            ),
            "auth"
        );
        assert_eq!(
            classify_error_category(
                "Gmail IMAP requires an app password; enable IMAP and save it as the IMAP password"
            ),
            "auth"
        );
        assert_eq!(
            classify_error_category(
                "IMAP login failed: NO [AUTHENTICATIONFAILED] invalid QQ 授权码"
            ),
            "auth"
        );
        assert_eq!(
            classify_error_category("163 客户端授权密码错误，请不要使用网页登录密码"),
            "auth"
        );
        assert_eq!(
            classify_error_category("Gmail list messages failed: HTTP 429 rate limit"),
            "rate_limit"
        );
    }

    #[test]
    fn remote_mail_action_failure_reports_job_result() {
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
    }

    #[test]
    fn failed_account_refresh_reports_job_result() {
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
    }

    #[test]
    fn prunes_legacy_schema_artifacts() {
        let conn = Connection::open_in_memory().expect("open memory db");
        let mut db = Database {
            conn,
            db_path: PathBuf::from("memory.sqlite"),
            crypto_key: Some([7; 32]),
        };
        db.initialize_schema().expect("schema");

        db.conn
            .execute_batch(
                "
                CREATE TABLE retry_queue (id INTEGER PRIMARY KEY);
                CREATE TABLE automation_runs (
                    id INTEGER PRIMARY KEY,
                    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );
                CREATE INDEX idx_automation_runs_created ON automation_runs(created_at DESC);
                ALTER TABLE accounts ADD COLUMN forward_enabled INTEGER NOT NULL DEFAULT 0;
                ALTER TABLE accounts ADD COLUMN forward_last_checked_at TEXT;
                ",
            )
            .expect("seed legacy schema");
        db.conn
            .execute(
                "
                INSERT INTO app_config (key, value) VALUES
                    ('webdav_url', 'https://example.com/webdav'),
                    ('appearance_theme', 'forest')
                ",
                [],
            )
            .expect("seed legacy config");

        db.prune_legacy_schema().expect("prune legacy schema");

        assert!(!table_exists(&db.conn, "retry_queue").expect("retry table check"));
        assert!(!table_exists(&db.conn, "automation_runs").expect("automation table check"));
        assert!(table_exists(&db.conn, "temp_emails").expect("temp email table check"));
        assert!(
            table_exists(&db.conn, "temp_email_messages").expect("temp message cache table check")
        );
        let account_columns = table_columns(&db.conn, "accounts").expect("account columns");
        assert!(!account_columns.iter().any(|name| name == "forward_enabled"));
        assert!(!account_columns
            .iter()
            .any(|name| name == "forward_last_checked_at"));
        let legacy_config_count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM app_config WHERE key IN ('webdav_url', 'appearance_theme')",
                [],
                |row| row.get(0),
            )
            .expect("legacy config count");
        assert_eq!(legacy_config_count, 0);

        db.prune_legacy_schema()
            .expect("second prune should stay idempotent");
    }

    #[test]
    fn cloudflare_channels_encrypt_secrets_and_protect_references() {
        let conn = Connection::open_in_memory().expect("open memory db");
        let mut db = Database {
            conn,
            db_path: PathBuf::from("memory.sqlite"),
            crypto_key: Some([7; 32]),
        };
        db.initialize_schema().expect("schema");
        let channel = db
            .save_cloudflare_channel(SaveCloudflareChannelInput {
                id: None,
                name: "Primary".to_string(),
                worker_url: "worker.example.com".to_string(),
                admin_password: Some("secret-admin-password".to_string()),
                email_domains: vec!["mail.example.com".to_string()],
                enabled: Some(true),
            })
            .expect("save channel");
        assert_eq!(channel.worker_url, "https://worker.example.com");
        assert!(channel.has_admin_password);
        let encrypted: String = db
            .conn
            .query_row(
                "SELECT admin_password_enc FROM cloudflare_channels WHERE id = ?",
                [channel.id],
                |row| row.get(0),
            )
            .expect("encrypted password");
        assert!(!encrypted.contains("secret-admin-password"));
        db.conn.execute("INSERT INTO temp_emails (email, provider, provider_base_url, cloudflare_channel_id) VALUES ('box@mail.example.com', 'cloudflare', 'https://worker.example.com', ?)", [channel.id]).expect("insert address");
        assert!(matches!(
            db.delete_cloudflare_channel(channel.id),
            Err(AppError::InvalidInput(_))
        ));
    }

    #[test]
    fn imports_gptmail_addresses_and_updates_duplicates() {
        let conn = Connection::open_in_memory().expect("open memory db");
        let mut db = Database {
            conn,
            db_path: PathBuf::from("memory.sqlite"),
            crypto_key: Some([7; 32]),
        };
        db.initialize_schema().expect("schema");
        let first = db
            .import_temp_emails(ImportTempEmailsInput {
                raw: "alpha@example.com\ninvalid\nbeta@example.com".to_string(),
                provider: "gptmail".to_string(),
                base_url: Some("https://mail.example.test/".to_string()),
                api_key: Some("secret-key".to_string()),
                cloudflare_channel_id: None,
            })
            .expect("first import");
        assert_eq!((first.imported, first.updated, first.skipped), (2, 0, 1));
        let encrypted: String = db
            .conn
            .query_row(
                "SELECT api_key_enc FROM temp_emails WHERE email = 'alpha@example.com'",
                [],
                |row| row.get(0),
            )
            .expect("encrypted API key");
        assert!(!encrypted.contains("secret-key"));

        let second = db
            .import_temp_emails(ImportTempEmailsInput {
                raw: "ALPHA@example.com".to_string(),
                provider: "gptmail".to_string(),
                base_url: None,
                api_key: None,
                cloudflare_channel_id: None,
            })
            .expect("duplicate import");
        assert_eq!((second.imported, second.updated), (0, 1));
        assert_eq!(db.list_temp_emails().expect("list").len(), 2);
    }

    #[test]
    fn validates_cloudflare_batch_usernames_and_import_headers() {
        let usernames = normalize_batch_usernames(
            Some(vec![
                "Alpha".to_string(),
                "sales.ops@example.com".to_string(),
            ]),
            2,
        )
        .expect("usernames");
        assert_eq!(usernames, vec!["alpha", "salesops"]);
        assert!(normalize_batch_usernames(Some(vec!["only-one".to_string()]), 2).is_err());
        assert_eq!(
            cloudflare_import_header("[cloudflare:Primary]"),
            Some(Some("Primary".to_string()))
        );
        assert_eq!(cloudflare_import_header("box@example.com"), None);
    }

    #[test]
    fn caches_temporary_messages_in_sqlite() {
        let conn = Connection::open_in_memory().expect("open memory db");
        let mut db = Database {
            conn,
            db_path: PathBuf::from("memory.sqlite"),
            crypto_key: Some([7; 32]),
        };
        db.initialize_schema().expect("schema");
        db.conn.execute(
            "INSERT INTO temp_emails (email, provider, provider_base_url) VALUES ('cache@example.com', 'gptmail', 'https://mail.example.test')",
            [],
        ).expect("insert temporary mailbox");
        let temp_email_id = db.conn.last_insert_rowid();
        let message = TempEmailMessage {
            id: "provider-message-1".to_string(),
            sender: "sender@example.com".to_string(),
            recipients: "cache@example.com".to_string(),
            subject: "Cached message".to_string(),
            body_preview: "Preview".to_string(),
            body: Some("Full cached body".to_string()),
            body_type: "text".to_string(),
            received_at: "2026-07-17T10:00:00Z".to_string(),
        };

        db.cache_temp_email_messages(temp_email_id, std::slice::from_ref(&message))
            .expect("cache message");
        let cached = db
            .list_temp_email_messages(temp_email_id)
            .expect("list cached messages");
        assert_eq!(cached.len(), 1);
        assert_eq!(cached[0].body.as_deref(), Some("Full cached body"));
        let mailbox = db.get_temp_email(temp_email_id).expect("mailbox");
        assert_eq!(mailbox.message_count, 1);
        assert!(mailbox.last_checked_at.is_some());

        db.conn
            .execute("DELETE FROM temp_emails WHERE id = ?", [temp_email_id])
            .expect("delete mailbox");
        let remaining: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM temp_email_messages WHERE temp_email_id = ?",
                [temp_email_id],
                |row| row.get(0),
            )
            .expect("cached message count");
        assert_eq!(remaining, 0);
    }

    #[test]
    fn migrates_existing_markdown_category_schema_before_creating_parent_index() {
        let conn = Connection::open_in_memory().expect("open memory db");
        conn.execute_batch(
            "
            CREATE TABLE markdown_categories (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE COLLATE NOCASE,
                sort_order INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );
            INSERT INTO markdown_categories (name, sort_order) VALUES ('旧笔记', 0);
            ",
        )
        .expect("legacy markdown schema");
        let mut db = Database {
            conn,
            db_path: PathBuf::from("memory.sqlite"),
            crypto_key: Some([7; 32]),
        };

        db.initialize_schema().expect("migrate schema");

        let columns = table_columns(&db.conn, "markdown_categories").expect("category columns");
        assert!(columns.iter().any(|column| column == "parent_id"));
        let index_count: i64 = db
            .conn
            .query_row(
                "
                SELECT COUNT(*)
                FROM sqlite_master
                WHERE type = 'index' AND name = 'idx_markdown_categories_parent'
                ",
                [],
                |row| row.get(0),
            )
            .expect("parent index");
        assert_eq!(index_count, 1);
        let category = db
            .list_markdown_categories()
            .expect("legacy category retained");
        assert_eq!(category.len(), 1);
        assert_eq!(category[0].name, "旧笔记");
        assert!(category[0].parent_id.is_none());
    }

    #[test]
    fn manages_markdown_library_categories_documents_and_search() {
        let conn = Connection::open_in_memory().expect("open memory db");
        let mut db = Database {
            conn,
            db_path: PathBuf::from("memory.sqlite"),
            crypto_key: Some([7; 32]),
        };
        db.initialize_schema().expect("schema");

        let category = db
            .create_markdown_category(CreateMarkdownCategoryInput {
                name: "工作笔记".to_string(),
                parent_id: None,
            })
            .expect("create category");
        let child_category = db
            .create_markdown_category(CreateMarkdownCategoryInput {
                name: "发布资料".to_string(),
                parent_id: Some(category.id),
            })
            .expect("create nested category");
        assert_eq!(child_category.parent_id, Some(category.id));
        let document = db
            .create_markdown_document(CreateMarkdownDocumentInput {
                title: Some("发布清单".to_string()),
                content: Some("# 发布\n\n- [ ] 构建安装包".to_string()),
                category_id: Some(child_category.id),
                source_path: Some("C:\\notes\\release.md".to_string()),
            })
            .expect("create document");
        assert_eq!(document.category_name.as_deref(), Some("发布资料"));
        assert_eq!(
            document.source_path.as_deref(),
            Some("C:\\notes\\release.md")
        );

        let search_results = db
            .list_markdown_documents(None, Some("安装包".to_string()))
            .expect("search documents");
        assert_eq!(search_results.len(), 1);
        assert_eq!(search_results[0].id, document.id);

        let updated = db
            .update_markdown_document(UpdateMarkdownDocumentInput {
                id: document.id,
                title: "发布清单 v2".to_string(),
                content: "# 发布\n\n- [x] 构建安装包".to_string(),
                category_id: Some(child_category.id),
                source_path: None,
            })
            .expect("update document");
        assert!(updated.content.contains("[x]"));
        assert!(updated.source_path.is_none());

        let renamed = db
            .update_markdown_category(UpdateMarkdownCategoryInput {
                id: category.id,
                name: "项目文档".to_string(),
                parent_id: None,
                sort_order: Some(2),
            })
            .expect("rename category");
        assert_eq!(renamed.name, "项目文档");
        assert_eq!(renamed.document_count, 0);

        let delete_parent_error = db
            .delete_markdown_category(category.id)
            .expect_err("reject non-empty category deletion");
        assert!(delete_parent_error
            .to_string()
            .contains("文件夹中有子文件或子文件夹"));
        let retained = db
            .get_markdown_document(document.id)
            .expect("document retained");
        assert_eq!(retained.category_id, Some(child_category.id));

        db.delete_markdown_document(document.id)
            .expect("delete document");
        assert!(db.get_markdown_document(document.id).is_err());
        db.delete_markdown_category(child_category.id)
            .expect("delete empty child category");
        db.delete_markdown_category(category.id)
            .expect("delete empty parent category");
    }
}
