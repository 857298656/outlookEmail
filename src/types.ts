export type AppStatus = {
  initialized: boolean;
  unlocked: boolean;
  db_path: string;
  account_count: number;
  message_count: number;
};

export type LoginInput = {
  username: string;
  password: string;
};

export type UpdateLoginPasswordInput = {
  current_password: string;
  new_password: string;
};

export type WorkspaceKeyRecord = {
  id: number;
  purpose: string;
  key_fingerprint: string;
  created_at: string;
};

export type GenerateWorkspaceKeyResult = {
  record: WorkspaceKeyRecord;
  workspace_key: string;
};

export type Group = {
  id: number;
  name: string;
  description: string;
  parent_id: number | null;
  level: number;
  sort_order: number;
  account_count: number;
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
  last_refresh_status: string;
  last_refresh_error: string | null;
  last_refresh_at: string | null;
  message_count: number;
  created_at: string;
  updated_at: string;
  aliases: string[];
  has_client_id: boolean;
  has_refresh_token: boolean;
  imap_host: string;
  imap_port: number;
  proxy_url: string;
  fallback_proxy_url_1: string;
  fallback_proxy_url_2: string;
  mail_retention_days: number;
};

export type AccountSecretsPreview = {
  client_id: string;
  refresh_token_preview: string;
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
  group_id?: number | null;
  remark?: string;
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
  manual_refresh_top: number;
};

export type ImportAccountsResult = {
  imported: number;
  skipped: number;
  accounts?: Account[];
};

export type JobResult = {
  success: boolean;
  message: string;
  refreshed: number;
  failed: number;
};

export type MarkdownCategory = {
  id: number;
  name: string;
  parent_id: number | null;
  sort_order: number;
  document_count: number;
  created_at: string;
  updated_at: string;
};

export type MarkdownDocument = {
  id: number;
  title: string;
  content: string;
  category_id: number | null;
  category_name: string | null;
  source_path: string | null;
  created_at: string;
  updated_at: string;
};

export type MarkdownFileContent = {
  path: string;
  file_name: string;
  content: string;
  size: number;
};

export type MarkdownFileWriteResult = {
  path: string;
  file_name: string;
  size: number;
};

export type LocalRetentionSummary = {
  database_path: string;
  database_size: number;
  attachment_file_count: number;
  attachments_size: number;
  export_file_count: number;
  exports_size: number;
  mail_message_count: number;
  unread_message_count: number;
  raw_mime_count: number;
  body_cached_count: number;
  latest_mail_received_at: string | null;
  latest_account_refresh_at: string | null;
};

export type ClearLocalDataInput = {
  clear_mail_cache?: boolean;
  clear_attachments?: boolean;
  clear_exports?: boolean;
  confirm: string;
};

export type ClearLocalDataResult = {
  success: boolean;
  message: string;
  deleted_messages: number;
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


export type TempEmail = {
  id: number;
  email: string;
  provider: "gptmail" | "duckmail" | "cloudflare";
  provider_base_url: string;
  cloudflare_channel_id: number | null;
  cloudflare_channel_name: string | null;
  message_count: number;
  last_checked_at: string | null;
  created_at: string;
};

export type TempEmailMessage = {
  id: string;
  sender: string;
  recipients: string;
  subject: string;
  body_preview: string;
  body: string | null;
  body_type: "text" | "html";
  received_at: string;
};

export type GenerateTempEmailInput = {
  provider: "gptmail" | "duckmail" | "cloudflare";
  base_url?: string;
  api_key?: string;
  prefix?: string;
  domain?: string;
  username?: string;
  password?: string;
  cloudflare_channel_id?: number;
};

export type ImportTempEmailsInput = {
  raw: string;
  provider: "gptmail" | "duckmail" | "cloudflare";
  base_url?: string;
  api_key?: string;
  cloudflare_channel_id?: number;
};

export type ImportTempEmailsResult = {
  imported: number;
  updated: number;
  skipped: number;
  token_failures: string[];
  errors: string[];
  emails: TempEmail[];
};

export type GenerateTempEmailsBatchInput = {
  provider: "cloudflare";
  count: number;
  domain?: string;
  usernames?: string[];
  cloudflare_channel_id?: number;
};

export type TempEmailBatchFailure = {
  index: number;
  username: string;
  error: string;
};

export type GenerateTempEmailsBatchResult = {
  created_count: number;
  failed_count: number;
  emails: TempEmail[];
  failures: TempEmailBatchFailure[];
};

export type CloudflareChannel = {
  id: number;
  name: string;
  worker_url: string;
  email_domains: string[];
  enabled: boolean;
  has_admin_password: boolean;
  created_at: string;
  updated_at: string;
};

export type SaveCloudflareChannelInput = {
  id?: number;
  name: string;
  worker_url: string;
  admin_password?: string;
  email_domains: string[];
  enabled?: boolean;
};
