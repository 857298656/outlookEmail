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
  has_client_id: boolean;
  has_refresh_token: boolean;
  has_imap_password: boolean;
  imap_host: string;
  imap_port: number;
  proxy_url: string;
  fallback_proxy_url_1: string;
  fallback_proxy_url_2: string;
  mail_retention_days: number;
};

export type AccountSecretsPreview = {
  password: string;
  client_id: string;
  refresh_token_preview: string;
  imap_password: string;
};

export type OAuthTokenResult = {
  success: boolean;
  account_id?: number | null;
  scope: string;
  expires_in: number;
  refresh_token_preview: string;
  refresh_token?: string | null;
};

export type OAuthSaveAccountInput = {
  email: string;
  password?: string;
  group_id?: number | null;
  remark?: string;
  forward_enabled?: boolean;
  client_id: string;
  redirect_uri: string;
  code_or_url?: string;
  refresh_token?: string;
  provider?: string;
  code_verifier?: string;
};

export type OAuthSaveAccountResult = {
  success: boolean;
  account: Account;
  scope: string;
  expires_in: number;
  refresh_token_preview: string;
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

export type MailRawContent = {
  message_id: number;
  file_name: string;
  content: string;
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
  appearance_theme: string;
  accent_color: string;
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

export type LocalRetentionSummary = {
  database_path: string;
  database_size: number;
  attachment_file_count: number;
  attachments_size: number;
  export_file_count: number;
  exports_size: number;
  backup_file_count: number;
  backups_size: number;
  mail_message_count: number;
  unread_message_count: number;
  raw_mime_count: number;
  body_cached_count: number;
  temp_message_count: number;
  retry_queue_count: number;
  latest_mail_received_at: string | null;
  latest_account_refresh_at: string | null;
};

export type ClearLocalDataInput = {
  clear_mail_cache?: boolean;
  clear_temp_mail_cache?: boolean;
  clear_attachments?: boolean;
  clear_exports?: boolean;
  confirm: string;
};

export type ClearLocalDataResult = {
  success: boolean;
  message: string;
  deleted_messages: number;
  deleted_temp_messages: number;
  deleted_files: number;
  freed_bytes: number;
};

export type ExportResult = {
  path: string;
  file_name: string;
  size: number;
  item_count: number;
};

export type MailShareRecord = {
  id: number;
  account_id: number;
  account_email: string;
  title: string;
  token_preview: string;
  exported_path: string;
  file_name: string;
  item_count: number;
  size: number;
  status: string;
  expires_at: string | null;
  revoked_at: string | null;
  created_at: string;
  updated_at: string;
};

export type SchedulerStatus = {
  last_refresh_at: string | null;
  last_forwarding_at: string | null;
  last_backup_at: string | null;
};

export type AutomationObservability = {
  run_count: number;
  successful_run_count: number;
  failed_run_count: number;
  scheduled_run_count: number;
  manual_run_count: number;
  average_duration_ms: number;
  retry_pending_count: number;
  retry_failed_count: number;
  retry_due_count: number;
  retry_exhausted_count: number;
  open_circuit_count: number;
  job_summaries: AutomationJobSummary[];
  retry_summaries: RetryTaskSummary[];
  error_buckets: AutomationErrorBucket[];
  channel_circuits: ForwardingChannelCircuit[];
};

export type AutomationJobSummary = {
  job_type: string;
  total: number;
  success: number;
  failed: number;
  scheduled: number;
  manual: number;
  average_duration_ms: number;
  last_finished_at: string | null;
  latest_message: string;
};

export type RetryTaskSummary = {
  task_type: string;
  pending: number;
  failed: number;
  due: number;
  exhausted: number;
  next_attempt_at: string | null;
  last_error: string;
};

export type AutomationErrorBucket = {
  category: string;
  count: number;
  latest_message: string;
  latest_at: string | null;
};

export type ForwardingChannelCircuit = {
  channel: string;
  configured: boolean;
  status: string;
  recent_failures: number;
  pending_retries: number;
  open_until: string | null;
  last_success_at: string | null;
  last_failure_at: string | null;
  last_error: string;
};

export type AutomationRun = {
  id: number;
  job_type: string;
  trigger_type: string;
  status: string;
  error_category: string;
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
  error_category: string;
  attempts: number;
  max_attempts: number;
  due_now: boolean;
  next_delay_minutes: number;
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
