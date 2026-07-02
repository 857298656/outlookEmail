import { invoke } from "@tauri-apps/api/core";
import type {
  Account,
  AppStatus,
  AutomationRun,
  AutomationRunQuery,
  BackupLog,
  BackupResult,
  CloudflareChannel,
  ExportResult,
  ForwardingLog,
  Group,
  ImportAccountsResult,
  JobResult,
  MailMessage,
  MailMessageQuery,
  Project,
  ProjectAccount,
  ProjectAccountEvent,
  RestoreBackupResult,
  SchedulerStatus,
  Settings,
  Tag,
  TempEmail,
  TempEmailMessage
} from "./types";

const defaultSettings: Settings = {
  graph_client_id: "",
  oauth_redirect_uri: "http://localhost:8080",
  gptmail_base_url: "https://mail.chatgpt.org.uk",
  gptmail_api_key: "",
  duckmail_base_url: "https://api.duckmail.sbs",
  duckmail_api_key: "",
  webdav_url: "",
  webdav_username: "",
  webdav_password: "",
  backup_enabled: false,
  backup_interval_minutes: 1440,
  scheduler_refresh_enabled: false,
  scheduler_refresh_interval_minutes: 15,
  scheduler_refresh_top: 25,
  forwarding_enabled: false,
  forwarding_interval_minutes: 10,
  forward_smtp_host: "",
  forward_smtp_port: 587,
  forward_smtp_username: "",
  forward_smtp_password: "",
  forward_smtp_from: "",
  forward_smtp_to: "",
  forward_telegram_bot_token: "",
  forward_telegram_chat_id: "",
  forward_wecom_webhook: ""
};

let mockInitialized = false;
let mockUnlocked = false;
let mockGroups: Group[] = [
  {
    id: 1,
    name: "Default",
    description: "Default mailbox group",
    color: "#3b82f6",
    parent_id: null,
    level: 1,
    sort_order: 0,
    account_count: 0
  }
];
let mockTags: Tag[] = [
  { id: 1, name: "Core", color: "#2563eb" },
  { id: 2, name: "Warmup", color: "#16a34a" },
  { id: 3, name: "Issue", color: "#dc2626" }
];
let mockAccounts: Account[] = [];
let mockMessages: MailMessage[] = [];
let mockSettings = defaultSettings;
let mockProjects: Project[] = [];
let mockProjectAccounts: ProjectAccount[] = [];
let mockProjectEvents: ProjectAccountEvent[] = [];
let mockForwardingLogs: ForwardingLog[] = [];
let mockBackupLogs: BackupLog[] = [];
let mockTempEmails: TempEmail[] = [];
let mockTempMessages: TempEmailMessage[] = [];
let mockCloudflareChannels: CloudflareChannel[] = [];
let mockSchedulerStatus: SchedulerStatus = {
  last_refresh_at: null,
  last_forwarding_at: null,
  last_backup_at: null
};
let mockAutomationRuns: AutomationRun[] = [];

function isTauriRuntime() {
  return "__TAURI_INTERNALS__" in window;
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauriRuntime()) {
    return invoke<T>(command, args);
  }
  return mockCall<T>(command, args);
}

async function mockCall<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  await new Promise((resolve) => window.setTimeout(resolve, 80));
  switch (command) {
    case "app_status":
      return status() as T;
    case "initialize_app":
      mockInitialized = true;
      mockUnlocked = true;
      return status() as T;
    case "unlock_app":
      mockUnlocked = true;
      return status() as T;
    case "lock_app":
      mockUnlocked = false;
      return status() as T;
    case "list_groups":
      return mockGroups as T;
    case "create_group": {
      const input = args?.input as { name: string; color?: string; description?: string };
      const group: Group = {
        id: Date.now(),
        name: input.name,
        description: input.description ?? "",
        color: input.color ?? "#2f6f9f",
        parent_id: null,
        level: 1,
        sort_order: mockGroups.length,
        account_count: 0
      };
      mockGroups = [...mockGroups, group];
      return group as T;
    }
    case "list_tags":
      return mockTags as T;
    case "create_tag": {
      const input = args?.input as { name: string; color: string };
      const tag: Tag = { id: Date.now(), name: input.name, color: input.color };
      mockTags = [...mockTags, tag];
      return tag as T;
    }
    case "delete_tag":
      mockTags = mockTags.filter((tag) => tag.id !== args?.tagId);
      return undefined as T;
    case "update_account": {
      const input = args?.input as Partial<Account> & { id: number };
      mockAccounts = mockAccounts.map((account) => (account.id === input.id ? { ...account, ...input } : account));
      return mockAccounts.find((account) => account.id === input.id) as T;
    }
    case "list_accounts":
      return mockAccounts as T;
    case "import_accounts": {
      const input = args?.input as { raw: string; group_id?: number };
      const rows = input.raw
        .split(/\r?\n/)
        .map((line) => line.trim())
        .filter((line) => line.includes("@"));
      const imported = rows.length;
      const nextAccounts = rows.map((line, index) => {
        const parts = line.includes("----") ? line.split("----") : line.split(",");
        return {
          id: Date.now() + index,
          email: parts[0].toLowerCase(),
          group_id: input.group_id ?? 1,
          group_name: "Default",
          remark: parts[4] ?? "",
          status: "active",
          provider: "outlook",
          account_type: "outlook",
          forward_enabled: false,
          last_refresh_status: "never",
          last_refresh_error: null,
          message_count: 0,
          created_at: new Date().toISOString(),
          updated_at: new Date().toISOString(),
          tags: [],
          has_password: Boolean(parts[1]),
          has_refresh_token: Boolean(parts[3]),
          has_imap_password: false,
          imap_host: "",
          imap_port: 993
        } satisfies Account;
      });
      mockAccounts = [...mockAccounts, ...nextAccounts];
      mockGroups = mockGroups.map((group) => ({
        ...group,
        account_count: mockAccounts.filter((account) => account.group_id === group.id).length
      }));
      return { imported, skipped: 0 } as T;
    }
    case "delete_account":
      mockAccounts = mockAccounts.filter((account) => account.id !== args?.accountId);
      mockMessages = mockMessages.filter((message) => message.account_id !== args?.accountId);
      return undefined as T;
    case "list_messages":
      return filterMockMessages(args) as T;
    case "create_demo_message": {
      const accountId = args?.accountId as number;
      const message: MailMessage = {
        id: Date.now(),
        account_id: accountId,
        folder: "inbox",
        provider_message_id: crypto.randomUUID(),
        subject: "Local sync placeholder",
        sender: "system@local",
        recipients: "",
        received_at: new Date().toISOString(),
        is_read: false,
        has_attachments: false,
        body_preview: "Provider sync is wired as a job boundary.",
        body: "This local message confirms the desktop workspace is working.",
        body_type: "text",
        attachments: []
      };
      mockMessages = [message, ...mockMessages];
      return message as T;
    }
    case "mark_mail_messages": {
      const input = args?.input as { message_ids: number[]; is_read: boolean };
      const ids = new Set(input.message_ids);
      let changed = 0;
      mockMessages = mockMessages.map((message) => {
        if (!ids.has(message.id)) return message;
        changed += 1;
        return { ...message, is_read: input.is_read };
      });
      return { success: true, message: `${input.is_read ? "Marked read" : "Marked unread"} ${changed} message(s)`, refreshed: changed, failed: 0 } as T;
    }
    case "delete_mail_messages": {
      const input = args?.input as { message_ids: number[] };
      const ids = new Set(input.message_ids);
      const before = mockMessages.length;
      mockMessages = mockMessages.filter((message) => !ids.has(message.id));
      const changed = before - mockMessages.length;
      return { success: true, message: `Deleted ${changed} message(s)`, refreshed: changed, failed: 0 } as T;
    }
    case "get_settings":
      return mockSettings as T;
    case "update_settings":
      mockSettings = args?.settings as Settings;
      return mockSettings as T;
    case "run_refresh_job":
      mockSchedulerStatus.last_refresh_at = new Date().toISOString();
      return recordMockAutomationRun("refresh", "manual", {
        success: true,
        message: "Refresh job accepted. Provider adapters are available in the Tauri runtime.",
        refreshed: 1,
        failed: 0
      }) as T;
    case "run_forwarding_job": {
      const now = new Date().toISOString();
      mockSchedulerStatus.last_forwarding_at = now;
      const enabledAccounts = mockAccounts.filter((account) => account.forward_enabled);
      const rows = enabledAccounts.slice(0, 3).map((account, index) => ({
        id: Date.now() + index,
        account_id: account.id,
        account_email: account.email,
        message_id: `mock-${index}`,
        channel: mockSettings.forward_smtp_host ? "smtp" : "preview",
        status: "success",
        error_message: null,
        created_at: now
      } satisfies ForwardingLog));
      mockForwardingLogs = [...rows, ...mockForwardingLogs];
      return recordMockAutomationRun("forwarding", "manual", {
        success: true,
        message: `Forwarded ${rows.length} preview item(s)`,
        refreshed: rows.length,
        failed: 0
      }) as T;
    }
    case "run_backup_job": {
      const now = new Date().toISOString();
      mockSchedulerStatus.last_backup_at = now;
      const log: BackupLog = {
        id: Date.now(),
        target: mockSettings.webdav_url || "browser-preview",
        status: "success",
        file_name: "outlook-email-preview.sqlite",
        size: 1024,
        error_message: null,
        created_at: now
      };
      mockBackupLogs = [log, ...mockBackupLogs];
      const result = { success: true, message: "Backup preview completed", path: "browser-preview.sqlite", remote_url: log.target, size: log.size };
      recordMockAutomationRun("backup", "manual", {
        success: result.success,
        message: result.message,
        refreshed: 1,
        failed: 0
      });
      return result as T;
    }
    case "restore_backup": {
      const input = args?.input as { backup_log_id: number; confirm?: boolean };
      if (!input?.confirm) throw new Error("restore confirmation is required");
      const log = mockBackupLogs.find((item) => item.id === input.backup_log_id);
      if (!log || log.status !== "success") throw new Error("successful backup log not found");
      return ({
        success: true,
        message: `Restored local backup ${log.file_name}`,
        restored_file: log.file_name,
        safety_backup_path: "browser-pre-restore.sqlite",
        replaced_database_path: "browser-preview.sqlite",
        size: log.size
      } satisfies RestoreBackupResult) as T;
    }
    case "list_forwarding_logs":
      return mockForwardingLogs.slice(0, (args?.limit as number | undefined) ?? 100) as T;
    case "list_backup_logs":
      return mockBackupLogs.slice(0, (args?.limit as number | undefined) ?? 100) as T;
    case "scheduler_status":
      return mockSchedulerStatus as T;
    case "list_automation_runs":
      return filterMockAutomationRuns(args) as T;
    case "clear_automation_runs": {
      const input = args?.input as AutomationRunQuery & { clear_all?: boolean };
      if (!input.clear_all && !input.job_type && !input.trigger_type && !input.status && !input.search) {
        throw new Error("choose a filter or enable clear_all before clearing automation history");
      }
      const before = mockAutomationRuns.length;
      mockAutomationRuns = mockAutomationRuns.filter((run) => !matchesAutomationRun(run, input));
      const deleted = before - mockAutomationRuns.length;
      return { success: true, message: `Cleared ${deleted} automation run(s)`, refreshed: deleted, failed: 0 } as T;
    }
    case "list_temp_emails":
      return mockTempEmails as T;
    case "import_temp_emails": {
      const input = args?.input as { raw: string; provider: string; channel_id?: number | null };
      let imported = 0;
      let skipped = 0;
      for (const line of input.raw.split(/\r?\n/).map((item) => item.trim()).filter(Boolean)) {
        const email = (line.includes("----") ? line.split("----")[0] : line.split(",")[0]).trim().toLowerCase();
        if (!email.includes("@") || mockTempEmails.some((item) => item.email === email)) {
          skipped += 1;
          continue;
        }
        mockTempEmails = [mockTempEmail(email, input.provider, input.channel_id ?? null), ...mockTempEmails];
        imported += 1;
      }
      return { imported, skipped } as T;
    }
    case "generate_temp_email": {
      const input = args?.input as { provider: string; prefix?: string; domain?: string; username?: string; channel_id?: number | null };
      const provider = input.provider || "gptmail";
      const domain = input.domain || (provider === "cloudflare" ? mockCloudflareChannels[0]?.email_domains[0] : "example.test") || "example.test";
      const name = input.username || input.prefix || `oe${Math.floor(Math.random() * 100000)}`;
      const email = `${name}@${domain}`.toLowerCase();
      const item = mockTempEmail(email, provider, input.channel_id ?? mockCloudflareChannels[0]?.id ?? null);
      mockTempEmails = [item, ...mockTempEmails.filter((existing) => existing.email !== email)];
      return item as T;
    }
    case "delete_temp_email": {
      const input = args?.input as { email: string };
      mockTempEmails = mockTempEmails.filter((item) => item.email !== input.email);
      mockTempMessages = mockTempMessages.filter((item) => item.email_address !== input.email);
      return undefined as T;
    }
    case "refresh_temp_email_messages": {
      const input = args?.input as { email: string };
      const message = mockTempMessage(input.email);
      mockTempMessages = [message, ...mockTempMessages.filter((item) => item.message_id !== message.message_id)];
      mockTempEmails = mockTempEmails.map((item) =>
        item.email === input.email
          ? { ...item, message_count: mockTempMessages.filter((msg) => msg.email_address === input.email).length, last_refresh_at: new Date().toISOString(), last_refresh_status: "success", updated_at: new Date().toISOString() }
          : item
      );
      return { success: true, message: "Temp mailbox refreshed", refreshed: 1, failed: 0 } as T;
    }
    case "list_temp_email_messages":
      return mockTempMessages.filter((item) => item.email_address === args?.email) as T;
    case "list_cloudflare_channels":
      return mockCloudflareChannels as T;
    case "upsert_cloudflare_channel": {
      const input = args?.input as { id?: number; name: string; worker_domain: string; email_domains: string[]; enabled: boolean; is_default: boolean };
      const channel: CloudflareChannel = {
        id: input.id ?? Date.now(),
        name: input.name,
        worker_domain: input.worker_domain,
        email_domains: input.email_domains,
        enabled: input.enabled,
        is_default: input.is_default,
        admin_password_configured: true,
        reference_count: 0,
        created_at: new Date().toISOString(),
        updated_at: new Date().toISOString()
      };
      mockCloudflareChannels = [channel, ...mockCloudflareChannels.filter((item) => item.id !== channel.id)].map((item) =>
        channel.is_default && item.id !== channel.id ? { ...item, is_default: false } : item
      );
      return channel as T;
    }
    case "delete_cloudflare_channel":
      mockCloudflareChannels = mockCloudflareChannels.filter((item) => item.id !== args?.channelId);
      return undefined as T;
    case "test_cloudflare_channel":
      return { success: true, message: "Cloudflare channel connected", refreshed: 1, failed: 0 } as T;
    case "generate_oauth_auth_url": {
      const input = args?.input as { client_id: string; redirect_uri: string; login_hint?: string };
      return `https://login.microsoftonline.com/common/oauth2/v2.0/authorize?client_id=${encodeURIComponent(input.client_id)}&response_type=code&redirect_uri=${encodeURIComponent(input.redirect_uri)}&scope=${encodeURIComponent("offline_access Mail.ReadWrite User.Read")}&response_mode=query&prompt=select_account${input.login_hint ? `&login_hint=${encodeURIComponent(input.login_hint)}` : ""}` as T;
    }
    case "exchange_oauth_token":
      return { success: true, account_id: (args?.input as { account_id?: number }).account_id, scope: "offline_access Mail.ReadWrite User.Read", expires_in: 3600, refresh_token_preview: "mock...oken" } as T;
    case "download_attachment": {
      const input = args?.input as { attachment_id: string };
      return { path: `browser-preview/${input.attachment_id}`, file_name: input.attachment_id, size: 0 } as T;
    }
    case "export_mail_messages": {
      const input = args?.input as { message_ids: number[] };
      const count = mockMessages.filter((message) => input.message_ids.includes(message.id)).length;
      return mockExport("mail-export.html", count) as T;
    }
    case "export_accounts": {
      const input = args?.input as { group_id?: number | null } | undefined;
      const count = mockAccounts.filter((account) => !input?.group_id || account.group_id === input.group_id).length;
      return mockExport("accounts-export.csv", count) as T;
    }
    case "export_project_accounts": {
      const input = args?.input as { project_id: number };
      const count = mockProjectAccounts.filter((account) => account.project_id === input.project_id).length;
      return mockExport("project-accounts-export.csv", count) as T;
    }
    case "list_projects":
      return mockProjects as T;
    case "create_project": {
      const input = args?.input as { name: string; project_key?: string; description?: string; scope_mode?: string; group_ids?: number[] };
      const project: Project = {
        id: Date.now(),
        name: input.name,
        project_key: input.project_key || input.name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, ""),
        description: input.description ?? "",
        scope_mode: input.scope_mode ?? "all",
        status: "active",
        group_ids: input.group_ids ?? [],
        stats: { total: 0, to_claim: 0, claimed: 0, success: 0, failed: 0, removed: 0 },
        created_at: new Date().toISOString(),
        updated_at: new Date().toISOString()
      };
      mockProjects = [project, ...mockProjects];
      syncMockProject(project.id);
      return mockProjects.find((item) => item.id === project.id) as T;
    }
    case "get_project":
      return mockProjects.find((project) => project.id === args?.projectId) as T;
    case "sync_project_scope": {
      const projectId = args?.projectId as number;
      syncMockProject(projectId);
      return mockProjects.find((project) => project.id === projectId) as T;
    }
    case "list_project_accounts":
      return mockProjectAccounts.filter((item) => item.project_id === args?.projectId) as T;
    case "claim_project_account": {
      const input = args?.input as { project_id: number };
      const item = mockProjectAccounts.find((account) => account.project_id === input.project_id && account.status === "toClaim");
      if (!item) return null as T;
      item.status = "claimed";
      item.claim_token = crypto.randomUUID();
      item.claimed_at = new Date().toISOString();
      item.lease_expires_at = new Date(Date.now() + 30 * 60_000).toISOString();
      item.claim_count += 1;
      item.updated_at = new Date().toISOString();
      updateMockProjectStats(input.project_id);
      return item as T;
    }
    case "complete_project_account_success":
      return mutateMockProjectAccount((args?.input as { project_account_id: number }).project_account_id, "success") as T;
    case "complete_project_account_failed":
      return mutateMockProjectAccount((args?.input as { project_account_id: number }).project_account_id, "failed") as T;
    case "release_project_account":
      return mutateMockProjectAccount((args?.input as { project_account_id: number }).project_account_id, "toClaim") as T;
    case "remove_project_account":
      return mutateMockProjectAccount((args?.input as { project_account_id: number }).project_account_id, "removed") as T;
    case "restore_project_account":
      return mutateMockProjectAccount((args?.input as { project_account_id: number }).project_account_id, "toClaim") as T;
    case "list_project_events":
      return mockProjectEvents.filter((event) => event.project_id === args?.projectId) as T;
    default:
      throw new Error(`Unknown command: ${command}`);
  }
}

function syncMockProject(projectId: number) {
  const project = mockProjects.find((item) => item.id === projectId);
  if (!project) return;
  const scopedAccounts = mockAccounts.filter((account) => {
    if (project.scope_mode !== "groups") return account.status === "active";
    return account.status === "active" && project.group_ids.includes(account.group_id ?? -1);
  });
  for (const account of scopedAccounts) {
    if (!mockProjectAccounts.some((item) => item.project_id === project.id && item.normalized_email === account.email)) {
      mockProjectAccounts.push({
        id: Date.now() + mockProjectAccounts.length,
        project_id: project.id,
        account_id: account.id,
        email: account.email,
        normalized_email: account.email,
        status: "toClaim",
        claim_token: null,
        claimed_at: null,
        lease_expires_at: null,
        last_result: "",
        last_result_detail: "",
        claim_count: 0,
        created_at: new Date().toISOString(),
        updated_at: new Date().toISOString()
      });
    }
  }
  updateMockProjectStats(projectId);
}

function mutateMockProjectAccount(projectAccountId: number, status: string) {
  const item = mockProjectAccounts.find((account) => account.id === projectAccountId);
  if (!item) throw new Error("Project account not found");
  item.status = status;
  item.claim_token = status === "claimed" ? item.claim_token : null;
  item.lease_expires_at = status === "claimed" ? item.lease_expires_at : null;
  item.last_result = status === "success" || status === "failed" ? status : "";
  item.updated_at = new Date().toISOString();
  updateMockProjectStats(item.project_id);
  return item;
}

function updateMockProjectStats(projectId: number) {
  const rows = mockProjectAccounts.filter((item) => item.project_id === projectId);
  const stats = {
    total: rows.length,
    to_claim: rows.filter((item) => item.status === "toClaim").length,
    claimed: rows.filter((item) => item.status === "claimed").length,
    success: rows.filter((item) => item.status === "success").length,
    failed: rows.filter((item) => item.status === "failed").length,
    removed: rows.filter((item) => item.status === "removed").length
  };
  mockProjects = mockProjects.map((project) => (project.id === projectId ? { ...project, stats, updated_at: new Date().toISOString() } : project));
}

function status(): AppStatus {
  return {
    initialized: mockInitialized,
    unlocked: mockUnlocked,
    db_path: "browser-preview.sqlite",
    account_count: mockAccounts.length,
    message_count: mockMessages.length
  };
}

function mockExport(fileName: string, itemCount: number): ExportResult {
  return {
    path: `browser-preview/exports/${fileName}`,
    file_name: fileName,
    size: Math.max(128, itemCount * 96),
    item_count: itemCount
  };
}

function recordMockAutomationRun(jobType: string, triggerType: string, result: JobResult): JobResult {
  const finished = new Date().toISOString();
  mockAutomationRuns = [
    {
      id: Date.now() + Math.floor(Math.random() * 1000),
      job_type: jobType,
      trigger_type: triggerType,
      status: result.success ? "success" : "failed",
      message: result.message,
      refreshed: result.refreshed,
      failed: result.failed,
      duration_ms: Math.floor(Math.random() * 900) + 80,
      started_at: finished,
      finished_at: finished
    },
    ...mockAutomationRuns
  ];
  return result;
}

function filterMockAutomationRuns(args?: Record<string, unknown>) {
  const query = ((args?.query ?? {}) as AutomationRunQuery) || {};
  const limit = query.limit ?? (args?.limit as number | undefined) ?? 100;
  return mockAutomationRuns.filter((run) => matchesAutomationRun(run, query)).slice(0, limit);
}

function matchesAutomationRun(run: AutomationRun, query: AutomationRunQuery) {
  const search = query.search?.trim().toLowerCase() ?? "";
  const text = [run.job_type, run.trigger_type, run.status, run.message].join(" ").toLowerCase();
  return (
    (!query.job_type || query.job_type === "all" || run.job_type === query.job_type) &&
    (!query.trigger_type || query.trigger_type === "all" || run.trigger_type === query.trigger_type) &&
    (!query.status || query.status === "all" || run.status === query.status) &&
    (!search || text.includes(search))
  );
}

function filterMockMessages(args?: Record<string, unknown>) {
  const query = (args?.query ?? {}) as MailMessageQuery;
  const accountId = query.account_id ?? (args?.accountId as number | undefined);
  const folder = query.folder ?? (args?.folder as string | undefined);
  const search = query.search?.trim().toLowerCase() ?? "";
  const readState = query.read_state ?? "all";
  const limit = query.limit ?? 200;
  const offset = query.offset ?? 0;
  return mockMessages
    .filter((message) => {
      const haystack = [
        message.subject,
        message.sender,
        message.recipients,
        message.body_preview,
        message.body ?? ""
      ].join(" ").toLowerCase();
      return (
        (!accountId || message.account_id === accountId) &&
        (!folder || folder === "all" || message.folder === folder) &&
        (!search || haystack.includes(search)) &&
        (readState === "all" || (readState === "read" ? message.is_read : !message.is_read)) &&
        (query.has_attachments === undefined || message.has_attachments === query.has_attachments)
      );
    })
    .slice(offset, offset + limit);
}

function mockTempEmail(email: string, provider: string, channelId: number | null): TempEmail {
  return {
    id: Date.now() + Math.floor(Math.random() * 1000),
    email,
    provider,
    status: "active",
    channel_id: channelId,
    message_count: 0,
    last_refresh_at: null,
    last_refresh_status: "never",
    last_refresh_error: null,
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString()
  };
}

function mockTempMessage(email: string): TempEmailMessage {
  return {
    id: Date.now(),
    message_id: crypto.randomUUID(),
    email_address: email,
    from_address: "sender@example.test",
    subject: "Temporary mailbox preview",
    content: "This preview message confirms the temp mailbox workspace is wired.",
    html_content: "",
    has_html: false,
    timestamp: Math.floor(Date.now() / 1000),
    raw_content: "",
    created_at: new Date().toISOString()
  };
}

export const api = {
  status: () => call<AppStatus>("app_status"),
  initialize: (password: string) => call<AppStatus>("initialize_app", { password }),
  unlock: (password: string) => call<AppStatus>("unlock_app", { password }),
  lock: () => call<AppStatus>("lock_app"),
  listGroups: () => call<Group[]>("list_groups"),
  createGroup: (input: { name: string; description?: string; color?: string; parent_id?: number | null }) =>
    call<Group>("create_group", { input }),
  listTags: () => call<Tag[]>("list_tags"),
  createTag: (input: { name: string; color: string }) => call<Tag>("create_tag", { input }),
  deleteTag: (tagId: number) => call<void>("delete_tag", { tagId }),
  listAccounts: () => call<Account[]>("list_accounts"),
  updateAccount: (input: {
    id: number;
    email: string;
    group_id?: number | null;
    remark?: string;
    status?: string;
    provider?: string;
    account_type?: string;
    imap_host?: string;
    imap_port?: number;
    forward_enabled?: boolean;
    password?: string;
    client_id?: string;
    refresh_token?: string;
    imap_password?: string;
  }) => call<Account>("update_account", { input }),
  importAccounts: (input: { raw: string; group_id?: number | null }) =>
    call<ImportAccountsResult>("import_accounts", { input }),
  deleteAccount: (accountId: number) => call<void>("delete_account", { accountId }),
  listMessages: (accountId?: number, folder = "all", query: MailMessageQuery = {}) =>
    call<MailMessage[]>("list_messages", {
      accountId,
      folder,
      query: {
        ...query,
        account_id: query.account_id ?? accountId,
        folder: query.folder ?? folder
      }
    }),
  createDemoMessage: (accountId: number) => call<MailMessage>("create_demo_message", { accountId }),
  markMessagesRead: (messageIds: number[], isRead: boolean, syncRemote = true) =>
    call<JobResult>("mark_mail_messages", { input: { message_ids: messageIds, is_read: isRead, sync_remote: syncRemote } }),
  deleteMessages: (messageIds: number[], syncRemote = true) =>
    call<JobResult>("delete_mail_messages", { input: { message_ids: messageIds, sync_remote: syncRemote } }),
  exportMailMessages: (messageIds: number[], title?: string) =>
    call<ExportResult>("export_mail_messages", { input: { message_ids: messageIds, title } }),
  exportAccounts: (groupId?: number | null) =>
    call<ExportResult>("export_accounts", { input: { group_id: groupId ?? null } }),
  exportProjectAccounts: (projectId: number) =>
    call<ExportResult>("export_project_accounts", { input: { project_id: projectId } }),
  getSettings: () => call<Settings>("get_settings"),
  updateSettings: (settings: Settings) => call<Settings>("update_settings", { settings }),
  runForwardingJob: (input?: { account_id?: number; limit?: number }) =>
    call<JobResult>("run_forwarding_job", { input }),
  runBackupJob: () => call<BackupResult>("run_backup_job"),
  restoreBackup: (backupLogId: number) =>
    call<RestoreBackupResult>("restore_backup", { input: { backup_log_id: backupLogId, confirm: true } }),
  listForwardingLogs: (limit = 100) => call<ForwardingLog[]>("list_forwarding_logs", { limit }),
  listBackupLogs: (limit = 100) => call<BackupLog[]>("list_backup_logs", { limit }),
  schedulerStatus: () => call<SchedulerStatus>("scheduler_status"),
  listAutomationRuns: (query: AutomationRunQuery = {}, limit = 100) =>
    call<AutomationRun[]>("list_automation_runs", { limit, query: { ...query, limit: query.limit ?? limit } }),
  clearAutomationRuns: (input: AutomationRunQuery & { older_than_days?: number; clear_all?: boolean }) =>
    call<JobResult>("clear_automation_runs", { input }),
  listTempEmails: () => call<TempEmail[]>("list_temp_emails"),
  importTempEmails: (input: { raw: string; provider: string; channel_id?: number | null }) =>
    call<ImportAccountsResult>("import_temp_emails", { input }),
  generateTempEmail: (input: { provider: string; prefix?: string; domain?: string; username?: string; password?: string; channel_id?: number | null }) =>
    call<TempEmail>("generate_temp_email", { input }),
  deleteTempEmail: (email: string) => call<void>("delete_temp_email", { input: { email } }),
  refreshTempEmailMessages: (email: string) => call<JobResult>("refresh_temp_email_messages", { input: { email } }),
  listTempEmailMessages: (email: string) => call<TempEmailMessage[]>("list_temp_email_messages", { email }),
  listCloudflareChannels: () => call<CloudflareChannel[]>("list_cloudflare_channels"),
  upsertCloudflareChannel: (input: {
    id?: number;
    name: string;
    worker_domain: string;
    email_domains: string[];
    admin_password?: string;
    enabled: boolean;
    is_default: boolean;
  }) => call<CloudflareChannel>("upsert_cloudflare_channel", { input }),
  deleteCloudflareChannel: (channelId: number) => call<void>("delete_cloudflare_channel", { channelId }),
  testCloudflareChannel: (channelId: number) => call<JobResult>("test_cloudflare_channel", { channelId }),
  generateOAuthAuthUrl: (input: { client_id: string; redirect_uri: string; login_hint?: string }) =>
    call<string>("generate_oauth_auth_url", { input }),
  exchangeOAuthToken: (input: { account_id?: number; client_id: string; redirect_uri: string; code_or_url: string }) =>
    call<{ success: boolean; account_id?: number; scope: string; expires_in: number; refresh_token_preview: string }>("exchange_oauth_token", { input }),
  downloadAttachment: (input: { account_id: number; message_id: string; attachment_id: string }) =>
    call<{ path: string; file_name: string; size: number }>("download_attachment", { input }),
  listProjects: () => call<Project[]>("list_projects"),
  createProject: (input: { name: string; project_key?: string; description?: string; scope_mode?: string; group_ids?: number[] }) =>
    call<Project>("create_project", { input }),
  getProject: (projectId: number) => call<Project>("get_project", { projectId }),
  syncProjectScope: (projectId: number) => call<Project>("sync_project_scope", { projectId }),
  listProjectAccounts: (projectId: number) => call<ProjectAccount[]>("list_project_accounts", { projectId }),
  claimProjectAccount: (input: { project_id: number; lease_minutes?: number }) =>
    call<ProjectAccount | null>("claim_project_account", { input }),
  completeProjectAccountSuccess: (projectAccountId: number, detail = "") =>
    call<ProjectAccount>("complete_project_account_success", { input: { project_account_id: projectAccountId, detail } }),
  completeProjectAccountFailed: (projectAccountId: number, detail = "") =>
    call<ProjectAccount>("complete_project_account_failed", { input: { project_account_id: projectAccountId, detail } }),
  releaseProjectAccount: (projectAccountId: number, detail = "") =>
    call<ProjectAccount>("release_project_account", { input: { project_account_id: projectAccountId, detail } }),
  removeProjectAccount: (projectAccountId: number, detail = "") =>
    call<ProjectAccount>("remove_project_account", { input: { project_account_id: projectAccountId, detail } }),
  restoreProjectAccount: (projectAccountId: number, detail = "") =>
    call<ProjectAccount>("restore_project_account", { input: { project_account_id: projectAccountId, detail } }),
  listProjectEvents: (projectId: number) => call<ProjectAccountEvent[]>("list_project_events", { projectId }),
  runRefreshJob: (accountId?: number, folder = "all", top = 25) =>
    call<JobResult>("run_refresh_job", { input: { account_id: accountId, folder, top }, accountId })
};
