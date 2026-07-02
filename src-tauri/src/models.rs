use serde::{Deserialize, Serialize};

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
    pub color: String,
    pub parent_id: Option<i64>,
    pub level: i64,
    pub sort_order: i64,
    pub account_count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Tag {
    pub id: i64,
    pub name: String,
    pub color: String,
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
    pub forward_enabled: bool,
    pub last_refresh_status: String,
    pub last_refresh_error: Option<String>,
    pub message_count: i64,
    pub created_at: String,
    pub updated_at: String,
    pub tags: Vec<Tag>,
    pub has_password: bool,
    pub has_refresh_token: bool,
    pub has_imap_password: bool,
    pub imap_host: String,
    pub imap_port: i64,
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

#[derive(Debug, Clone, Deserialize)]
pub struct MailMessageQuery {
    pub account_id: Option<i64>,
    pub folder: Option<String>,
    pub search: Option<String>,
    pub read_state: Option<String>,
    pub has_attachments: Option<bool>,
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
    pub gptmail_base_url: String,
    pub gptmail_api_key: String,
    pub duckmail_base_url: String,
    pub duckmail_api_key: String,
    pub webdav_url: String,
    pub webdav_username: String,
    pub webdav_password: String,
    pub backup_enabled: bool,
    pub backup_interval_minutes: i64,
    pub scheduler_refresh_enabled: bool,
    pub scheduler_refresh_interval_minutes: i64,
    pub scheduler_refresh_top: i64,
    pub forwarding_enabled: bool,
    pub forwarding_interval_minutes: i64,
    pub forward_smtp_host: String,
    pub forward_smtp_port: i64,
    pub forward_smtp_username: String,
    pub forward_smtp_password: String,
    pub forward_smtp_from: String,
    pub forward_smtp_to: String,
    pub forward_telegram_bot_token: String,
    pub forward_telegram_chat_id: String,
    pub forward_wecom_webhook: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            graph_client_id: String::new(),
            oauth_redirect_uri: "http://localhost:8080".to_string(),
            gptmail_base_url: "https://mail.chatgpt.org.uk".to_string(),
            gptmail_api_key: String::new(),
            duckmail_base_url: "https://api.duckmail.sbs".to_string(),
            duckmail_api_key: String::new(),
            webdav_url: String::new(),
            webdav_username: String::new(),
            webdav_password: String::new(),
            backup_enabled: false,
            backup_interval_minutes: 1440,
            scheduler_refresh_enabled: false,
            scheduler_refresh_interval_minutes: 15,
            scheduler_refresh_top: 25,
            forwarding_enabled: false,
            forwarding_interval_minutes: 10,
            forward_smtp_host: String::new(),
            forward_smtp_port: 587,
            forward_smtp_username: String::new(),
            forward_smtp_password: String::new(),
            forward_smtp_from: String::new(),
            forward_smtp_to: String::new(),
            forward_telegram_bot_token: String::new(),
            forward_telegram_chat_id: String::new(),
            forward_wecom_webhook: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ForwardingLog {
    pub id: i64,
    pub account_id: Option<i64>,
    pub account_email: String,
    pub message_id: String,
    pub channel: String,
    pub status: String,
    pub error_message: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BackupLog {
    pub id: i64,
    pub target: String,
    pub status: String,
    pub file_name: String,
    pub size: i64,
    pub error_message: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BackupResult {
    pub success: bool,
    pub message: String,
    pub path: String,
    pub remote_url: String,
    pub size: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchedulerStatus {
    pub last_refresh_at: Option<String>,
    pub last_forwarding_at: Option<String>,
    pub last_backup_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ForwardingInput {
    pub account_id: Option<i64>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct ForwardContent {
    pub account_email: String,
    pub message_id: String,
    pub subject: String,
    pub sender: String,
    pub received_at: String,
    pub body_preview: String,
    pub body: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TempEmail {
    pub id: i64,
    pub email: String,
    pub provider: String,
    pub status: String,
    pub channel_id: Option<i64>,
    pub message_count: i64,
    pub last_refresh_at: Option<String>,
    pub last_refresh_status: String,
    pub last_refresh_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TempEmailMessage {
    pub id: i64,
    pub message_id: String,
    pub email_address: String,
    pub from_address: String,
    pub subject: String,
    pub content: String,
    pub html_content: String,
    pub has_html: bool,
    pub timestamp: i64,
    pub raw_content: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CloudflareChannel {
    pub id: i64,
    pub name: String,
    pub worker_domain: String,
    pub email_domains: Vec<String>,
    pub enabled: bool,
    pub is_default: bool,
    pub admin_password_configured: bool,
    pub reference_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GenerateTempEmailInput {
    pub provider: String,
    pub prefix: Option<String>,
    pub domain: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub channel_id: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ImportTempEmailsInput {
    pub raw: String,
    pub provider: String,
    pub channel_id: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TempEmailAddressInput {
    pub email: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpsertCloudflareChannelInput {
    pub id: Option<i64>,
    pub name: String,
    pub worker_domain: String,
    pub email_domains: Vec<String>,
    pub admin_password: Option<String>,
    pub enabled: bool,
    pub is_default: bool,
}

#[derive(Debug, Clone)]
pub struct TempEmailCredential {
    pub id: i64,
    pub email: String,
    pub provider: String,
    pub channel_id: Option<i64>,
    pub provider_token: String,
    pub provider_account_id: String,
    pub provider_password: String,
}

#[derive(Debug, Clone)]
pub struct CloudflareChannelCredential {
    pub id: i64,
    pub worker_domain: String,
    pub email_domains: Vec<String>,
    pub admin_password: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateGroupInput {
    pub name: String,
    pub description: Option<String>,
    pub color: Option<String>,
    pub parent_id: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateTagInput {
    pub name: String,
    pub color: String,
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
}

#[derive(Debug, Clone, Serialize)]
pub struct JobResult {
    pub success: bool,
    pub message: String,
    pub refreshed: usize,
    pub failed: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub project_key: String,
    pub description: String,
    pub scope_mode: String,
    pub status: String,
    pub group_ids: Vec<i64>,
    pub stats: ProjectStats,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ProjectStats {
    pub total: i64,
    pub to_claim: i64,
    pub claimed: i64,
    pub success: i64,
    pub failed: i64,
    pub removed: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectAccount {
    pub id: i64,
    pub project_id: i64,
    pub account_id: Option<i64>,
    pub email: String,
    pub normalized_email: String,
    pub status: String,
    pub claim_token: Option<String>,
    pub claimed_at: Option<String>,
    pub lease_expires_at: Option<String>,
    pub last_result: String,
    pub last_result_detail: String,
    pub claim_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectAccountEvent {
    pub id: i64,
    pub project_id: i64,
    pub account_id: Option<i64>,
    pub project_account_id: Option<i64>,
    pub normalized_email: String,
    pub action: String,
    pub from_status: Option<String>,
    pub to_status: Option<String>,
    pub detail: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateProjectInput {
    pub name: String,
    pub project_key: Option<String>,
    pub description: Option<String>,
    pub scope_mode: Option<String>,
    pub group_ids: Option<Vec<i64>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProjectAccountActionInput {
    pub project_account_id: i64,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClaimProjectAccountInput {
    pub project_id: i64,
    pub lease_minutes: Option<i64>,
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
    pub forward_enabled: Option<bool>,
    pub password: Option<String>,
    pub client_id: Option<String>,
    pub refresh_token: Option<String>,
    pub imap_password: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OAuthAuthUrlInput {
    pub client_id: String,
    pub redirect_uri: String,
    pub login_hint: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OAuthExchangeInput {
    pub account_id: Option<i64>,
    pub client_id: String,
    pub redirect_uri: String,
    pub code_or_url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OAuthTokenResult {
    pub success: bool,
    pub account_id: Option<i64>,
    pub scope: String,
    pub expires_in: i64,
    pub refresh_token_preview: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DownloadAttachmentInput {
    pub account_id: i64,
    pub message_id: String,
    pub attachment_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadAttachmentResult {
    pub path: String,
    pub file_name: String,
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
pub struct ExportAccountsInput {
    pub group_id: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExportProjectAccountsInput {
    pub project_id: i64,
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
    pub password: String,
    pub client_id: String,
    pub refresh_token: String,
    pub imap_host: String,
    pub imap_port: i64,
    pub imap_password: String,
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
}
