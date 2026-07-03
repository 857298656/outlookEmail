export type AppStatus = {
  initialized: boolean;
  unlocked: boolean;
  db_path: string;
  account_count: number;
  message_count: number;
};

export type Group = {
  id: number;
  name: string;
  description: string;
  color: string;
  proxy_url: string;
  fallback_proxy_url_1: string;
  fallback_proxy_url_2: string;
  parent_id: number | null;
  level: number;
  sort_order: number;
  account_count: number;
};

export type Tag = {
  id: number;
  name: string;
  color: string;
};

export type Account = {
  id: number;
  email: string;
  group_id: number | null;
  group_name: string | null;
  remark: string;
  status: string;
  provider: string;
  account_type: string;
  forward_enabled: boolean;
  last_refresh_status: string;
  last_refresh_error: string | null;
  last_refresh_at: string | null;
  message_count: number;
  created_at: string;
  updated_at: string;
  tags: Tag[];
  aliases: string[];
  has_password: boolean;
  has_refresh_token: boolean;
  has_imap_password: boolean;
  imap_host: string;
  imap_port: number;
  proxy_url: string;
  fallback_proxy_url_1: string;
  fallback_proxy_url_2: string;
};

export type AccountSecretsPreview = {
  password: string;
  client_id: string;
  refresh_token_preview: string;
  imap_password: string;
};

export type MailMessage = {
  id: number;
  account_id: number;
  folder: string;
  provider_message_id: string;
  subject: string;
  sender: string;
  recipients: string;
  received_at: string;
  is_read: boolean;
  has_attachments: boolean;
  body_preview: string;
  body: string | null;
  body_type: string;
  attachments: AttachmentInfo[];
  remote_sync_failure: RemoteSyncFailure | null;
};

export type RemoteSyncFailure = {
  retry_id: number;
  task_type: string;
  status: string;
  action: string;
  error_message: string;
  attempts: number;
  max_attempts: number;
  next_attempt_at: string | null;
  last_attempt_at: string | null;
  updated_at: string;
};

export type MailMessageQuery = {
  account_id?: number;
  folder?: string;
  search?: string;
  read_state?: "all" | "read" | "unread";
  has_attachments?: boolean;
  sort_by?: "date" | "subject" | "sender" | "read" | "attachments" | "folder";
  sort_order?: "asc" | "desc";
  limit?: number;
  offset?: number;
};

export type AttachmentInfo = {
  id: string;
  name: string;
  content_type: string;
  size: number;
};

export type Settings = {
  graph_client_id: string;
  oauth_redirect_uri: string;
  gptmail_base_url: string;
  gptmail_api_key: string;
  duckmail_base_url: string;
  duckmail_api_key: string;
  webdav_url: string;
  webdav_username: string;
  webdav_password: string;
  backup_enabled: boolean;
  backup_interval_minutes: number;
  scheduler_refresh_enabled: boolean;
  scheduler_refresh_interval_minutes: number;
  scheduler_refresh_top: number;
  forwarding_enabled: boolean;
  forwarding_interval_minutes: number;
  forward_smtp_host: string;
  forward_smtp_port: number;
  forward_smtp_username: string;
  forward_smtp_password: string;
  forward_smtp_from: string;
  forward_smtp_to: string;
  forward_telegram_bot_token: string;
  forward_telegram_chat_id: string;
  forward_wecom_webhook: string;
};

export type ImportAccountsResult = {
  imported: number;
  skipped: number;
};

export type JobResult = {
  success: boolean;
  message: string;
  refreshed: number;
  failed: number;
};

export type ForwardingLog = {
  id: number;
  account_id: number | null;
  account_email: string;
  message_id: string;
  channel: string;
  status: string;
  error_message: string | null;
  created_at: string;
};

export type BackupLog = {
  id: number;
  target: string;
  status: string;
  file_name: string;
  size: number;
  error_message: string | null;
  created_at: string;
};

export type BackupResult = {
  success: boolean;
  message: string;
  path: string;
  remote_url: string;
  size: number;
};

export type RestoreBackupResult = {
  success: boolean;
  message: string;
  restored_file: string;
  safety_backup_path: string;
  replaced_database_path: string;
  size: number;
};

export type ExportResult = {
  path: string;
  file_name: string;
  size: number;
  item_count: number;
};

export type SchedulerStatus = {
  last_refresh_at: string | null;
  last_forwarding_at: string | null;
  last_backup_at: string | null;
};

export type AutomationRun = {
  id: number;
  job_type: string;
  trigger_type: string;
  status: string;
  message: string;
  refreshed: number;
  failed: number;
  duration_ms: number;
  started_at: string;
  finished_at: string;
};

export type RefreshLog = {
  id: number;
  account_id: number | null;
  account_email: string;
  refresh_type: string;
  status: string;
  error_message: string | null;
  created_at: string;
};

export type AutomationRunQuery = {
  job_type?: string;
  trigger_type?: string;
  status?: string;
  search?: string;
  limit?: number;
};

export type RetryQueueItem = {
  id: number;
  task_type: string;
  status: string;
  account_id: number | null;
  account_email: string;
  message_id: string;
  channel: string;
  action: string;
  payload_json: string;
  error_message: string;
  attempts: number;
  max_attempts: number;
  next_attempt_at: string | null;
  last_attempt_at: string | null;
  created_at: string;
  updated_at: string;
};

export type RetryQueueQuery = {
  status?: string;
  task_type?: string;
  limit?: number;
};

export type TempEmail = {
  id: number;
  email: string;
  provider: string;
  status: string;
  channel_id: number | null;
  message_count: number;
  last_refresh_at: string | null;
  last_refresh_status: string;
  last_refresh_error: string | null;
  tags: string[];
  created_at: string;
  updated_at: string;
};

export type TempEmailMessage = {
  id: number;
  message_id: string;
  email_address: string;
  from_address: string;
  subject: string;
  content: string;
  html_content: string;
  has_html: boolean;
  timestamp: number;
  raw_content: string;
  created_at: string;
};

export type CloudflareChannel = {
  id: number;
  name: string;
  worker_domain: string;
  email_domains: string[];
  enabled: boolean;
  is_default: boolean;
  admin_password_configured: boolean;
  reference_count: number;
  created_at: string;
  updated_at: string;
};

export type ProjectStats = {
  total: number;
  to_claim: number;
  claimed: number;
  success: number;
  failed: number;
  removed: number;
};

export type Project = {
  id: number;
  name: string;
  project_key: string;
  description: string;
  scope_mode: string;
  use_alias_email: boolean;
  status: string;
  group_ids: number[];
  tag_ids: number[];
  stats: ProjectStats;
  created_at: string;
  updated_at: string;
};

export type ProjectAccount = {
  id: number;
  project_id: number;
  account_id: number | null;
  email: string;
  normalized_email: string;
  status: string;
  claim_token: string | null;
  claimed_at: string | null;
  lease_expires_at: string | null;
  last_result: string;
  last_result_detail: string;
  claim_count: number;
  created_at: string;
  updated_at: string;
};

export type ProjectAccountEvent = {
  id: number;
  project_id: number;
  account_id: number | null;
  project_account_id: number | null;
  normalized_email: string;
  action: string;
  from_status: string | null;
  to_status: string | null;
  detail: string;
  created_at: string;
};
