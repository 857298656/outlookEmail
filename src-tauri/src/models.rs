use serde::{Deserialize, Serialize};

pub const DEFAULT_GRAPH_CLIENT_ID: &str = "6daa9f56-5e67-4cb6-ae52-ef89ef912d36";
pub const DEFAULT_OAUTH_REDIRECT_URI: &str = "http://localhost:8080";
pub const DEFAULT_LOGIN_USERNAME: &str = "admin";
pub const DEFAULT_LOGIN_PASSWORD: &str = "admin123";

#[derive(Debug, Clone, Deserialize)]
pub struct LoginInput {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateLoginPasswordInput {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceKeyRecord {
    pub id: i64,
    pub purpose: String,
    pub key_fingerprint: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GenerateWorkspaceKeyInput {
    pub purpose: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateWorkspaceKeyRecordInput {
    pub id: i64,
    pub purpose: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GenerateWorkspaceKeyResult {
    pub record: WorkspaceKeyRecord,
    pub workspace_key: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppStatus {
    pub initialized: bool,
    pub unlocked: bool,
    pub db_path: String,
    pub account_count: i64,
    pub message_count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Group {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub parent_id: Option<i64>,
    pub level: i64,
    pub sort_order: i64,
    pub account_count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Account {
    pub id: i64,
    pub email: String,
    pub group_id: Option<i64>,
    pub group_name: Option<String>,
    pub remark: String,
    pub status: String,
    pub provider: String,
    pub account_type: String,
    pub last_refresh_status: String,
    pub last_refresh_error: Option<String>,
    pub last_refresh_at: Option<String>,
    pub message_count: i64,
    pub created_at: String,
    pub updated_at: String,
    pub aliases: Vec<String>,
    pub has_client_id: bool,
    pub has_refresh_token: bool,
    pub imap_host: String,
    pub imap_port: i64,
    pub proxy_url: String,
    pub fallback_proxy_url_1: String,
    pub fallback_proxy_url_2: String,
    pub mail_retention_days: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MailMessage {
    pub id: i64,
    pub account_id: i64,
    pub folder: String,
    pub provider_message_id: String,
    pub subject: String,
    pub sender: String,
    pub recipients: String,
    pub received_at: String,
    pub is_read: bool,
    pub has_attachments: bool,
    pub body_preview: String,
    pub body: Option<String>,
    pub body_type: String,
    pub attachments: Vec<AttachmentInfo>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct MailMessageQuery {
    pub account_id: Option<i64>,
    pub folder: Option<String>,
    pub search: Option<String>,
    pub read_state: Option<String>,
    pub has_attachments: Option<bool>,
    pub sort_by: Option<String>,
    pub sort_order: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MarkMailMessagesInput {
    pub message_ids: Vec<i64>,
    pub is_read: bool,
    pub sync_remote: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeleteMailMessagesInput {
    pub message_ids: Vec<i64>,
    pub sync_remote: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentInfo {
    pub id: String,
    pub name: String,
    pub content_type: String,
    pub size: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub graph_client_id: String,
    pub oauth_redirect_uri: String,
    pub scheduler_refresh_enabled: bool,
    pub scheduler_refresh_interval_minutes: i64,
    pub scheduler_refresh_top: i64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            graph_client_id: DEFAULT_GRAPH_CLIENT_ID.to_string(),
            oauth_redirect_uri: DEFAULT_OAUTH_REDIRECT_URI.to_string(),
            scheduler_refresh_enabled: false,
            scheduler_refresh_interval_minutes: 15,
            scheduler_refresh_top: 25,
        }
    }
}






#[derive(Debug, Clone, Serialize)]
pub struct LocalRetentionSummary {
    pub database_path: String,
    pub database_size: i64,
    pub attachment_file_count: usize,
    pub attachments_size: i64,
    pub export_file_count: usize,
    pub exports_size: i64,
    pub mail_message_count: i64,
    pub unread_message_count: i64,
    pub raw_mime_count: i64,
    pub body_cached_count: i64,
    pub latest_mail_received_at: Option<String>,
    pub latest_account_refresh_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClearLocalDataInput {
    pub clear_mail_cache: Option<bool>,
    pub clear_attachments: Option<bool>,
    pub clear_exports: Option<bool>,
    pub confirm: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClearLocalDataResult {
    pub success: bool,
    pub message: String,
    pub deleted_messages: i64,
    pub deleted_files: usize,
    pub freed_bytes: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchedulerStatus {
    pub last_refresh_at: Option<String>,
}


#[derive(Debug, Clone, Deserialize)]
pub struct CreateGroupInput {
    pub name: String,
    pub description: Option<String>,
    pub parent_id: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateGroupInput {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub parent_id: Option<i64>,
    pub sort_order: Option<i64>,
}


#[derive(Debug, Clone, Deserialize)]
pub struct ImportAccountsInput {
    pub raw: String,
    pub group_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportAccountsResult {
    pub imported: usize,
    pub skipped: usize,
    pub accounts: Vec<Account>,
}

#[derive(Debug, Clone, Serialize)]
pub struct JobResult {
    pub success: bool,
    pub message: String,
    pub refreshed: usize,
    pub failed: usize,
}








#[derive(Debug, Clone, Deserialize)]
pub struct UpdateAccountInput {
    pub id: i64,
    pub email: String,
    pub group_id: Option<i64>,
    pub remark: Option<String>,
    pub status: Option<String>,
    pub provider: Option<String>,
    pub account_type: Option<String>,
    pub imap_host: Option<String>,
    pub imap_port: Option<i64>,
    pub proxy_url: Option<String>,
    pub fallback_proxy_url_1: Option<String>,
    pub fallback_proxy_url_2: Option<String>,
    pub mail_retention_days: Option<i64>,
    pub client_id: Option<String>,
    pub refresh_token: Option<String>,
    pub aliases: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AccountBatchInput {
    pub account_ids: Vec<i64>,
    pub action: String,
    pub group_id: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RevealAccountSecretsInput {
    pub account_id: i64,
    pub password: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AccountSecretsPreview {
    pub client_id: String,
    pub refresh_token_preview: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OAuthAuthUrlInput {
    pub client_id: String,
    pub redirect_uri: String,
    pub login_hint: Option<String>,
    pub provider: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OAuthExchangeInput {
    pub account_id: Option<i64>,
    pub client_id: String,
    pub redirect_uri: String,
    pub code_or_url: String,
    pub provider: Option<String>,
    pub code_verifier: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OAuthTokenResult {
    pub success: bool,
    pub account_id: Option<i64>,
    pub scope: String,
    pub expires_in: i64,
    pub refresh_token_preview: String,
    pub refresh_token: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OAuthSaveAccountInput {
    pub email: String,
    pub group_id: Option<i64>,
    pub remark: Option<String>,
    pub client_id: String,
    pub redirect_uri: String,
    pub code_or_url: Option<String>,
    pub refresh_token: Option<String>,
    pub provider: Option<String>,
    pub code_verifier: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OAuthSaveAccountResult {
    pub success: bool,
    pub account: Account,
    pub scope: String,
    pub expires_in: i64,
    pub refresh_token_preview: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DownloadAttachmentInput {
    pub account_id: i64,
    pub message_id: String,
    pub attachment_id: String,
    pub folder: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DownloadAllAttachmentsInput {
    pub account_id: i64,
    pub message_id: String,
    pub folder: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadAttachmentResult {
    pub path: String,
    pub file_name: String,
    pub size: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MailRawContent {
    pub message_id: i64,
    pub file_name: String,
    pub content: String,
    pub size: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExportResult {
    pub path: String,
    pub file_name: String,
    pub size: i64,
    pub item_count: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExportMailMessagesInput {
    pub message_ids: Vec<i64>,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateMailShareInput {
    pub message_ids: Vec<i64>,
    pub title: Option<String>,
    pub expires_in_days: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RevokeMailShareInput {
    pub share_id: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MailShareRecord {
    pub id: i64,
    pub account_id: i64,
    pub account_email: String,
    pub title: String,
    pub token_preview: String,
    pub exported_path: String,
    pub file_name: String,
    pub item_count: i64,
    pub size: i64,
    pub status: String,
    pub expires_at: Option<String>,
    pub revoked_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExportAccountsInput {
    pub group_id: Option<i64>,
    pub account_ids: Option<Vec<i64>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExportAccountSecretsInput {
    pub account_ids: Vec<i64>,
    pub password: String,
    pub confirm: String,
}


#[derive(Debug, Clone, Deserialize)]
pub struct RefreshInput {
    pub account_id: Option<i64>,
    pub folder: Option<String>,
    pub top: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct DownloadedAttachment {
    pub name: String,
    pub content_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct AccountCredentials {
    pub id: i64,
    pub email: String,
    pub provider: String,
    pub account_type: String,
    pub client_id: String,
    pub refresh_token: String,
    pub imap_host: String,
    pub imap_port: i64,
    pub proxy_chain: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ProviderMessage {
    pub folder: String,
    pub provider_message_id: String,
    pub subject: String,
    pub sender: String,
    pub recipients: String,
    pub cc: String,
    pub received_at: String,
    pub received_at_sort: f64,
    pub is_read: bool,
    pub has_attachments: bool,
    pub body_preview: String,
    pub body: Option<String>,
    pub body_type: String,
    pub attachments: Vec<AttachmentInfo>,
    pub raw_mime: Option<Vec<u8>>,
}
