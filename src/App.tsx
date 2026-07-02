import {
  Archive,
  CheckCircle2,
  Cloud,
  Download,
  FolderKanban,
  Inbox,
  KeyRound,
  Loader2,
  Lock,
  Mail,
  Plus,
  RefreshCw,
  Search,
  Settings as SettingsIcon,
  Tags,
  Trash2,
  Upload,
  Users,
  XCircle
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import type { ReactNode } from "react";
import { api } from "./api";
import { parseAccountRows } from "./lib/importParser";
import type {
  Account,
  AppStatus,
  AutomationRun,
  BackupLog,
  ExportResult,
  ForwardingLog,
  Group,
  MailMessage,
  MailMessageQuery,
  Project,
  ProjectAccount,
  SchedulerStatus,
  Settings,
  Tag,
  TempEmail,
  TempEmailMessage,
  CloudflareChannel
} from "./types";

type View = "mail" | "accounts" | "temp" | "projects" | "settings";
type MailFilters = {
  search: string;
  readState: "all" | "read" | "unread";
  attachmentFilter: "all" | "attachments" | "plain";
};

const colors = ["#2563eb", "#16a34a", "#dc2626", "#7c3aed", "#0f766e", "#b45309"];
const mailPageSize = 100;

function App() {
  const [status, setStatus] = useState<AppStatus | null>(null);
  const [groups, setGroups] = useState<Group[]>([]);
  const [tags, setTags] = useState<Tag[]>([]);
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [messages, setMessages] = useState<MailMessage[]>([]);
  const [projects, setProjects] = useState<Project[]>([]);
  const [projectAccounts, setProjectAccounts] = useState<ProjectAccount[]>([]);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [tempEmails, setTempEmails] = useState<TempEmail[]>([]);
  const [tempMessages, setTempMessages] = useState<TempEmailMessage[]>([]);
  const [cloudflareChannels, setCloudflareChannels] = useState<CloudflareChannel[]>([]);
  const [forwardingLogs, setForwardingLogs] = useState<ForwardingLog[]>([]);
  const [backupLogs, setBackupLogs] = useState<BackupLog[]>([]);
  const [automationRuns, setAutomationRuns] = useState<AutomationRun[]>([]);
  const [schedulerStatus, setSchedulerStatus] = useState<SchedulerStatus | null>(null);
  const [selectedGroupId, setSelectedGroupId] = useState<number | "all">("all");
  const [selectedAccountId, setSelectedAccountId] = useState<number | undefined>();
  const [selectedMessageId, setSelectedMessageId] = useState<number | undefined>();
  const [selectedTempEmail, setSelectedTempEmail] = useState<string | undefined>();
  const [selectedTempMessageId, setSelectedTempMessageId] = useState<string | undefined>();
  const [folder, setFolder] = useState("all");
  const [mailFilters, setMailFilters] = useState<MailFilters>({ search: "", readState: "all", attachmentFilter: "all" });
  const [mailPage, setMailPage] = useState(0);
  const [selectedMessageIds, setSelectedMessageIds] = useState<number[]>([]);
  const [view, setView] = useState<View>("mail");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const selectedAccount = accounts.find((account) => account.id === selectedAccountId);
  const selectedMessage = messages.find((message) => message.id === selectedMessageId);
  const selectedTempMessage = tempMessages.find((message) => message.message_id === selectedTempMessageId);
  const filteredAccounts = useMemo(() => {
    if (selectedGroupId === "all") return accounts;
    return accounts.filter((account) => account.group_id === selectedGroupId);
  }, [accounts, selectedGroupId]);

  function buildMailQuery(accountId = selectedAccountId, nextFolder = folder, filters = mailFilters, page = mailPage): MailMessageQuery {
    return {
      account_id: accountId,
      folder: nextFolder,
      search: filters.search.trim() || undefined,
      read_state: filters.readState,
      has_attachments:
        filters.attachmentFilter === "attachments"
          ? true
          : filters.attachmentFilter === "plain"
            ? false
            : undefined,
      limit: mailPageSize,
      offset: page * mailPageSize
    };
  }

  async function loadMailboxMessages(accountId = selectedAccountId, nextFolder = folder, filters = mailFilters, page = mailPage) {
    const nextMessages = await api.listMessages(accountId, nextFolder, buildMailQuery(accountId, nextFolder, filters, page));
    setMessages(nextMessages);
    setSelectedMessageId((current) => (nextMessages.some((message) => message.id === current) ? current : nextMessages[0]?.id));
    setSelectedMessageIds([]);
  }

  async function loadStatus() {
    setStatus(await api.status());
  }

  async function loadWorkspace(accountId = selectedAccountId, nextFolder = folder, filters = mailFilters, page = mailPage) {
    const [nextGroups, nextTags, nextAccounts] = await Promise.all([
      api.listGroups(),
      api.listTags(),
      api.listAccounts()
    ]);
    setGroups(nextGroups);
    setTags(nextTags);
    setAccounts(nextAccounts);
    const firstAccountId = accountId ?? nextAccounts[0]?.id;
    setSelectedAccountId(firstAccountId);
    const nextMessages = await api.listMessages(firstAccountId, nextFolder, buildMailQuery(firstAccountId, nextFolder, filters, page));
    setMessages(nextMessages);
    setSelectedMessageId(nextMessages[0]?.id);
    setSelectedMessageIds([]);
  }

  async function loadProjects(projectId?: number) {
    const nextProjects = await api.listProjects();
    setProjects(nextProjects);
    const selectedProject = nextProjects.find((project) => project.id === projectId) ?? nextProjects[0];
    setProjectAccounts(selectedProject ? await api.listProjectAccounts(selectedProject.id) : []);
  }

  async function loadAutomation() {
    const [nextSettings, nextForwardingLogs, nextBackupLogs, nextAutomationRuns, nextSchedulerStatus] = await Promise.all([
      api.getSettings(),
      api.listForwardingLogs(80),
      api.listBackupLogs(40),
      api.listAutomationRuns(80),
      api.schedulerStatus()
    ]);
    setSettings(nextSettings);
    setForwardingLogs(nextForwardingLogs);
    setBackupLogs(nextBackupLogs);
    setAutomationRuns(nextAutomationRuns);
    setSchedulerStatus(nextSchedulerStatus);
  }

  async function loadTempWorkspace(email = selectedTempEmail) {
    const [nextTempEmails, nextChannels] = await Promise.all([api.listTempEmails(), api.listCloudflareChannels()]);
    setTempEmails(nextTempEmails);
    setCloudflareChannels(nextChannels);
    const nextEmail = email ?? nextTempEmails[0]?.email;
    setSelectedTempEmail(nextEmail);
    const nextMessages = nextEmail ? await api.listTempEmailMessages(nextEmail) : [];
    setTempMessages(nextMessages);
    setSelectedTempMessageId(nextMessages[0]?.message_id);
  }

  useEffect(() => {
    loadStatus().catch((err) => setError(readError(err)));
  }, []);

  useEffect(() => {
    if (!status?.unlocked) return;
    loadWorkspace().catch((err) => setError(readError(err)));
    loadProjects().catch((err) => setError(readError(err)));
    loadAutomation().catch((err) => setError(readError(err)));
    loadTempWorkspace().catch((err) => setError(readError(err)));
  }, [status?.unlocked]);

  async function runAction(action: () => Promise<void>, success?: string) {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      await action();
      if (success) setNotice(success);
    } catch (err) {
      setError(readError(err));
    } finally {
      setBusy(false);
    }
  }

  if (!status) {
    return (
      <div className="centerScreen">
        <Loader2 className="spin" size={26} />
      </div>
    );
  }

  if (!status.initialized || !status.unlocked) {
    return (
      <LockScreen
        initialized={status.initialized}
        busy={busy}
        error={error}
        onSubmit={(password) =>
          runAction(async () => {
            setStatus(status.initialized ? await api.unlock(password) : await api.initialize(password));
          })
        }
      />
    );
  }

  return (
    <div className="appShell">
      <aside className="rail">
        <div className="brandMark">OE</div>
        <IconButton active={view === "mail"} title="Mailbox" onClick={() => setView("mail")}>
          <Inbox size={20} />
        </IconButton>
        <IconButton active={view === "accounts"} title="Accounts" onClick={() => setView("accounts")}>
          <Users size={20} />
        </IconButton>
        <IconButton active={view === "temp"} title="Temp Mail" onClick={() => setView("temp")}>
          <Cloud size={20} />
        </IconButton>
        <IconButton active={view === "projects"} title="Projects" onClick={() => setView("projects")}>
          <FolderKanban size={20} />
        </IconButton>
        <IconButton active={view === "settings"} title="Settings" onClick={() => setView("settings")}>
          <SettingsIcon size={20} />
        </IconButton>
        <div className="railSpacer" />
        <IconButton
          title="Lock"
          onClick={() =>
            runAction(async () => {
              setStatus(await api.lock());
            })
          }
        >
          <Lock size={20} />
        </IconButton>
      </aside>

      <main className="mainSurface">
        <header className="topBar">
          <div>
            <h1>OutlookEmail Desktop</h1>
            <p>{status.account_count} accounts · {status.message_count} cached messages</p>
          </div>
          <div className="topActions">
            {notice && <span className="notice">{notice}</span>}
            {error && <span className="errorText">{error}</span>}
            <button
              className="button secondary"
              onClick={() =>
                runAction(async () => {
                  const result = await api.runRefreshJob(selectedAccountId);
                  setNotice(result.message);
                  await loadWorkspace(selectedAccountId, folder, mailFilters, mailPage);
                  await loadStatus();
                })
              }
              disabled={busy}
            >
              {busy ? <Loader2 className="spin" size={16} /> : <RefreshCw size={16} />}
              Refresh
            </button>
          </div>
        </header>

        {view === "mail" && (
          <MailWorkspace
            groups={groups}
            accounts={filteredAccounts}
            messages={messages}
            selectedGroupId={selectedGroupId}
            selectedAccountId={selectedAccountId}
            selectedMessage={selectedMessage}
            selectedMessageIds={selectedMessageIds}
            folder={folder}
            filters={mailFilters}
            page={mailPage}
            hasNextPage={messages.length === mailPageSize}
            onGroupChange={(groupId) => {
              setSelectedGroupId(groupId);
              const nextAccount = groupId === "all" ? accounts[0] : accounts.find((account) => account.group_id === groupId);
              setSelectedAccountId(nextAccount?.id);
              setMailPage(0);
              void runAction(async () => loadMailboxMessages(nextAccount?.id, folder, mailFilters, 0));
            }}
            onAccountSelect={(accountId) =>
              runAction(async () => {
                setSelectedAccountId(accountId);
                setMailPage(0);
                await loadMailboxMessages(accountId, folder, mailFilters, 0);
              })
            }
            onFolderChange={(nextFolder) =>
              runAction(async () => {
                setFolder(nextFolder);
                setMailPage(0);
                await loadMailboxMessages(selectedAccountId, nextFolder, mailFilters, 0);
              })
            }
            onMessageSelect={setSelectedMessageId}
            onToggleMessageSelect={(messageId) =>
              setSelectedMessageIds((current) =>
                current.includes(messageId) ? current.filter((id) => id !== messageId) : [...current, messageId]
              )
            }
            onSelectVisibleMessages={() => setSelectedMessageIds(messages.map((message) => message.id))}
            onClearSelection={() => setSelectedMessageIds([])}
            onFilterApply={(filters) =>
              runAction(async () => {
                setMailFilters(filters);
                setMailPage(0);
                await loadMailboxMessages(selectedAccountId, folder, filters, 0);
              })
            }
            onPageChange={(nextPage) =>
              runAction(async () => {
                const page = Math.max(0, nextPage);
                setMailPage(page);
                await loadMailboxMessages(selectedAccountId, folder, mailFilters, page);
              })
            }
            onMarkMessages={(messageIds, isRead) =>
              runAction(async () => {
                const result = await api.markMessagesRead(messageIds, isRead);
                setNotice(result.message);
                await loadMailboxMessages(selectedAccountId, folder, mailFilters, mailPage);
              })
            }
            onDeleteMessages={(messageIds) =>
              runAction(async () => {
                const result = await api.deleteMessages(messageIds);
                setNotice(result.message);
                await loadMailboxMessages(selectedAccountId, folder, mailFilters, mailPage);
                await loadStatus();
              })
            }
            onExportMessages={(messageIds) =>
              runAction(async () => {
                const result = await api.exportMailMessages(messageIds, "OutlookEmail message export");
                setNotice(exportNotice(result));
              })
            }
            onCreateDemo={() =>
              selectedAccountId
                ? runAction(async () => {
                    await api.createDemoMessage(selectedAccountId);
                    setMailPage(0);
                    await loadWorkspace(selectedAccountId, folder, mailFilters, 0);
                  }, "Local message created")
                : undefined
            }
            onDownloadAttachment={(message, attachmentId) =>
              runAction(async () => {
                const result = await api.downloadAttachment({
                  account_id: message.account_id,
                  message_id: message.provider_message_id,
                  attachment_id: attachmentId
                });
                setNotice(`Downloaded ${result.file_name}`);
              })
            }
          />
        )}

        {view === "accounts" && (
          <AccountsView
            groups={groups}
            tags={tags}
            accounts={accounts}
            settings={settings}
            busy={busy}
            onImport={(raw, groupId) =>
              runAction(async () => {
                await api.importAccounts({ raw, group_id: groupId });
                await loadWorkspace(selectedAccountId, folder);
                await loadStatus();
              }, "Accounts imported")
            }
            onCreateGroup={(name, color) =>
              runAction(async () => {
                await api.createGroup({ name, color });
                await loadWorkspace(selectedAccountId, folder);
              }, "Group created")
            }
            onCreateTag={(name, color) =>
              runAction(async () => {
                await api.createTag({ name, color });
                await loadWorkspace(selectedAccountId, folder);
              }, "Tag created")
            }
            onDeleteAccount={(accountId) =>
              runAction(async () => {
                await api.deleteAccount(accountId);
                await loadWorkspace(undefined, folder);
                await loadStatus();
              }, "Account deleted")
            }
            onExportAccounts={(groupId) =>
              runAction(async () => {
                const result = await api.exportAccounts(groupId);
                setNotice(exportNotice(result));
              })
            }
            onUpdateAccount={(input) =>
              runAction(async () => {
                await api.updateAccount(input);
                await loadWorkspace(input.id, folder);
              }, "Account saved")
            }
            onGenerateOAuthUrl={(input) => api.generateOAuthAuthUrl(input)}
            onExchangeOAuthToken={(input) =>
              runAction(async () => {
                const result = await api.exchangeOAuthToken(input);
                setNotice(`OAuth saved: ${result.refresh_token_preview}`);
                await loadWorkspace(input.account_id, folder);
              })
            }
          />
        )}

        {view === "temp" && (
          <TempEmailsView
            tempEmails={tempEmails}
            messages={tempMessages}
            channels={cloudflareChannels}
            selectedEmail={selectedTempEmail}
            selectedMessage={selectedTempMessage}
            busy={busy}
            onSelect={(email) =>
              runAction(async () => {
                setSelectedTempEmail(email);
                const nextMessages = await api.listTempEmailMessages(email);
                setTempMessages(nextMessages);
                setSelectedTempMessageId(nextMessages[0]?.message_id);
              })
            }
            onMessageSelect={setSelectedTempMessageId}
            onGenerate={(input) =>
              runAction(async () => {
                const created = await api.generateTempEmail(input);
                await loadTempWorkspace(created.email);
              }, "Temp email generated")
            }
            onImport={(input) =>
              runAction(async () => {
                const result = await api.importTempEmails(input);
                await loadTempWorkspace();
                setNotice(`Imported ${result.imported}, skipped ${result.skipped}`);
              })
            }
            onRefresh={(email) =>
              runAction(async () => {
                const result = await api.refreshTempEmailMessages(email);
                setNotice(result.message);
                await loadTempWorkspace(email);
              })
            }
            onDelete={(email) =>
              runAction(async () => {
                await api.deleteTempEmail(email);
                await loadTempWorkspace(undefined);
              }, "Temp email deleted")
            }
            onSaveChannel={(input) =>
              runAction(async () => {
                await api.upsertCloudflareChannel(input);
                await loadTempWorkspace(selectedTempEmail);
              }, "Cloudflare channel saved")
            }
            onDeleteChannel={(channelId) =>
              runAction(async () => {
                await api.deleteCloudflareChannel(channelId);
                await loadTempWorkspace(selectedTempEmail);
              }, "Cloudflare channel deleted")
            }
            onTestChannel={(channelId) =>
              runAction(async () => {
                const result = await api.testCloudflareChannel(channelId);
                setNotice(result.message);
              })
            }
          />
        )}

        {view === "projects" && (
          <ProjectsView
            projects={projects}
            accounts={projectAccounts}
            groups={groups}
            busy={busy}
            onCreate={(input) =>
              runAction(async () => {
                const project = await api.createProject(input);
                await loadProjects(project.id);
              }, "Project created")
            }
            onSelect={(projectId) =>
              runAction(async () => {
                setProjectAccounts(await api.listProjectAccounts(projectId));
              })
            }
            onSync={(projectId) =>
              runAction(async () => {
                await api.syncProjectScope(projectId);
                await loadProjects(projectId);
              }, "Project synced")
            }
            onClaim={(projectId) =>
              runAction(async () => {
                const claimed = await api.claimProjectAccount({ project_id: projectId, lease_minutes: 30 });
                await loadProjects(projectId);
                setNotice(claimed ? `Claimed ${claimed.email}` : "No claimable accounts");
              })
            }
            onExport={(projectId) =>
              runAction(async () => {
                const result = await api.exportProjectAccounts(projectId);
                setNotice(exportNotice(result));
              })
            }
            onAction={(projectId, action, projectAccountId) =>
              runAction(async () => {
                if (action === "success") await api.completeProjectAccountSuccess(projectAccountId);
                if (action === "failed") await api.completeProjectAccountFailed(projectAccountId);
                if (action === "release") await api.releaseProjectAccount(projectAccountId);
                if (action === "remove") await api.removeProjectAccount(projectAccountId);
                if (action === "restore") await api.restoreProjectAccount(projectAccountId);
                await loadProjects(projectId);
              }, "Project account updated")
            }
          />
        )}

        {view === "settings" && settings && (
          <SettingsView
            status={status}
            settings={settings}
            forwardingLogs={forwardingLogs}
            backupLogs={backupLogs}
            automationRuns={automationRuns}
            schedulerStatus={schedulerStatus}
            busy={busy}
            onSave={(nextSettings) =>
              runAction(async () => {
                setSettings(await api.updateSettings(nextSettings));
                await loadAutomation();
              }, "Settings saved")
            }
            onRunForwarding={() =>
              runAction(async () => {
                const result = await api.runForwardingJob({ limit: 50 });
                setNotice(result.message);
                await loadAutomation();
              })
            }
            onRunBackup={() =>
              runAction(async () => {
                const result = await api.runBackupJob();
                setNotice(result.message);
                await loadAutomation();
              })
            }
          />
        )}
      </main>
    </div>
  );
}

function LockScreen({
  initialized,
  busy,
  error,
  onSubmit
}: {
  initialized: boolean;
  busy: boolean;
  error: string | null;
  onSubmit: (password: string) => void;
}) {
  const [password, setPassword] = useState("");
  return (
    <div className="lockScreen">
      <form
        className="lockPanel"
        onSubmit={(event) => {
          event.preventDefault();
          onSubmit(password);
        }}
      >
        <div className="lockIcon">
          <KeyRound size={28} />
        </div>
        <h1>{initialized ? "Unlock workspace" : "Create local workspace"}</h1>
        <p>
          {initialized
            ? "Enter your local app password to decrypt secrets and open the mailbox workspace."
            : "Set an 8+ character local password. It protects encrypted mailbox credentials in SQLite."}
        </p>
        <input
          className="input"
          type="password"
          minLength={8}
          value={password}
          placeholder="Local app password"
          onChange={(event) => setPassword(event.target.value)}
        />
        {error && <div className="formError">{error}</div>}
        <button className="button primary fullWidth" disabled={busy || password.length < 8}>
          {busy ? <Loader2 className="spin" size={16} /> : <Lock size={16} />}
          {initialized ? "Unlock" : "Create workspace"}
        </button>
      </form>
    </div>
  );
}

function MailWorkspace({
  groups,
  accounts,
  messages,
  selectedGroupId,
  selectedAccountId,
  selectedMessage,
  selectedMessageIds,
  folder,
  filters,
  page,
  hasNextPage,
  onGroupChange,
  onAccountSelect,
  onFolderChange,
  onMessageSelect,
  onToggleMessageSelect,
  onSelectVisibleMessages,
  onClearSelection,
  onFilterApply,
  onPageChange,
  onMarkMessages,
  onDeleteMessages,
  onExportMessages,
  onCreateDemo,
  onDownloadAttachment
}: {
  groups: Group[];
  accounts: Account[];
  messages: MailMessage[];
  selectedGroupId: number | "all";
  selectedAccountId?: number;
  selectedMessage?: MailMessage;
  selectedMessageIds: number[];
  folder: string;
  filters: MailFilters;
  page: number;
  hasNextPage: boolean;
  onGroupChange: (groupId: number | "all") => void;
  onAccountSelect: (accountId: number) => void;
  onFolderChange: (folder: string) => void;
  onMessageSelect: (messageId: number) => void;
  onToggleMessageSelect: (messageId: number) => void;
  onSelectVisibleMessages: () => void;
  onClearSelection: () => void;
  onFilterApply: (filters: MailFilters) => void;
  onPageChange: (page: number) => void;
  onMarkMessages: (messageIds: number[], isRead: boolean) => void;
  onDeleteMessages: (messageIds: number[]) => void;
  onExportMessages: (messageIds: number[]) => void;
  onCreateDemo: () => void;
  onDownloadAttachment: (message: MailMessage, attachmentId: string) => void;
}) {
  const [draftFilters, setDraftFilters] = useState(filters);
  const selectedCount = selectedMessageIds.length;

  useEffect(() => {
    setDraftFilters(filters);
  }, [filters]);

  return (
    <section className="workspaceGrid">
      <aside className="pane groupPane">
        <div className="paneHeader">
          <h2>Groups</h2>
        </div>
        <button className={selectedGroupId === "all" ? "listRow active" : "listRow"} onClick={() => onGroupChange("all")}>
          <Archive size={16} />
          <span>All accounts</span>
          <b>{accounts.length}</b>
        </button>
        {groups.map((group) => (
          <button
            key={group.id}
            className={selectedGroupId === group.id ? "listRow active" : "listRow"}
            onClick={() => onGroupChange(group.id)}
          >
            <span className="dot" style={{ backgroundColor: group.color }} />
            <span>{group.name}</span>
            <b>{group.account_count}</b>
          </button>
        ))}
      </aside>

      <aside className="pane accountPane">
        <div className="paneHeader">
          <h2>Accounts</h2>
          <button className="iconMini" title="Create local test message" onClick={onCreateDemo} disabled={!selectedAccountId}>
            <Plus size={16} />
          </button>
        </div>
        <div className="searchBox">
          <Search size={15} />
          <span>Search coming soon</span>
        </div>
        {accounts.map((account) => (
          <button
            key={account.id}
            className={selectedAccountId === account.id ? "accountRow active" : "accountRow"}
            onClick={() => onAccountSelect(account.id)}
          >
            <span className="mailAvatar">{account.email.slice(0, 2).toUpperCase()}</span>
            <span className="accountText">
              <strong>{account.email}</strong>
              <small>{account.last_refresh_status} · {account.message_count} messages</small>
            </span>
          </button>
        ))}
        {accounts.length === 0 && <EmptyState icon={<Mail size={24} />} text="Import accounts to start." />}
      </aside>

      <section className="pane messagePane">
        <div className="paneHeader">
          <h2>Messages</h2>
          <select className="select" value={folder} onChange={(event) => onFolderChange(event.target.value)}>
            <option value="all">All</option>
            <option value="inbox">Inbox</option>
            <option value="junkemail">Junk</option>
            <option value="deleteditems">Deleted</option>
          </select>
        </div>
        <div className="messageTools">
          <label className="searchBox messageSearch">
            <Search size={15} />
            <input
              value={draftFilters.search}
              placeholder="Search sender, subject, body"
              onChange={(event) => setDraftFilters({ ...draftFilters, search: event.target.value })}
              onKeyDown={(event) => {
                if (event.key === "Enter") onFilterApply(draftFilters);
              }}
            />
          </label>
          <div className="filterRow">
            <select
              className="select"
              value={draftFilters.readState}
              onChange={(event) => setDraftFilters({ ...draftFilters, readState: event.target.value as MailFilters["readState"] })}
            >
              <option value="all">All mail</option>
              <option value="unread">Unread</option>
              <option value="read">Read</option>
            </select>
            <select
              className="select"
              value={draftFilters.attachmentFilter}
              onChange={(event) => setDraftFilters({ ...draftFilters, attachmentFilter: event.target.value as MailFilters["attachmentFilter"] })}
            >
              <option value="all">Any files</option>
              <option value="attachments">Has files</option>
              <option value="plain">No files</option>
            </select>
            <button className="button compact secondary" onClick={() => onFilterApply(draftFilters)}>
              <Search size={14} />
              Apply
            </button>
          </div>
        </div>
        {selectedCount > 0 && (
          <div className="bulkBar">
            <span>{selectedCount} selected</span>
            <button className="button compact secondary" onClick={() => onMarkMessages(selectedMessageIds, true)}>
              <CheckCircle2 size={14} />
              Read
            </button>
            <button className="button compact secondary" onClick={() => onMarkMessages(selectedMessageIds, false)}>
              <Mail size={14} />
              Unread
            </button>
            <button className="button compact danger" onClick={() => onDeleteMessages(selectedMessageIds)}>
              <Trash2 size={14} />
              Delete
            </button>
            <button className="button compact secondary" onClick={() => onExportMessages(selectedMessageIds)}>
              <Download size={14} />
              Export
            </button>
            <button className="button compact ghost" onClick={onClearSelection}>
              Clear
            </button>
          </div>
        )}
        {messages.map((message) => (
          <div key={message.id} className={selectedMessage?.id === message.id ? "messageRow active" : "messageRow"}>
            <input
              type="checkbox"
              aria-label={`Select ${message.subject || "message"}`}
              checked={selectedMessageIds.includes(message.id)}
              onChange={() => onToggleMessageSelect(message.id)}
            />
            <button className={message.is_read ? "messageOpen" : "messageOpen unread"} onClick={() => onMessageSelect(message.id)}>
              <span className="messageTop">
                <strong>{message.subject || "(no subject)"}</strong>
                <small>{formatDate(message.received_at)}</small>
              </span>
              <span className="sender">{message.sender}</span>
              <span className="preview">{message.body_preview}</span>
            </button>
          </div>
        ))}
        {messages.length === 0 && <EmptyState icon={<Inbox size={24} />} text="No cached messages yet." />}
        {messages.length > 0 && (
          <div className="pagerBar">
            <button className="button compact secondary" onClick={onSelectVisibleMessages}>
              Select page
            </button>
            <span>Page {page + 1}</span>
            <button className="button compact secondary" disabled={page === 0} onClick={() => onPageChange(page - 1)}>
              Previous
            </button>
            <button className="button compact secondary" disabled={!hasNextPage} onClick={() => onPageChange(page + 1)}>
              Next
            </button>
          </div>
        )}
      </section>

      <article className="pane detailPane">
        {selectedMessage ? (
          <>
            <div className="detailHeader">
              <div>
                <h2>{selectedMessage.subject || "(no subject)"}</h2>
                <p>{selectedMessage.sender}</p>
              </div>
              <div className="detailActions">
                <button className="button compact secondary" onClick={() => onMarkMessages([selectedMessage.id], !selectedMessage.is_read)}>
                  {selectedMessage.is_read ? <Mail size={14} /> : <CheckCircle2 size={14} />}
                  {selectedMessage.is_read ? "Unread" : "Read"}
                </button>
                <button className="button compact danger" onClick={() => onDeleteMessages([selectedMessage.id])}>
                  <Trash2 size={14} />
                  Delete
                </button>
                <button className="button compact secondary" onClick={() => onExportMessages([selectedMessage.id])}>
                  <Download size={14} />
                  Export
                </button>
              </div>
            </div>
            <div className="metaGrid">
              <span>Folder</span>
              <strong>{selectedMessage.folder}</strong>
              <span>Status</span>
              <strong>{selectedMessage.is_read ? "Read" : "Unread"}</strong>
              <span>Received</span>
              <strong>{formatDate(selectedMessage.received_at)}</strong>
            </div>
            <div className="messageBody">{selectedMessage.body || selectedMessage.body_preview}</div>
            {selectedMessage.attachments.length > 0 && (
              <div className="attachmentList">
                <h3>Attachments</h3>
                {selectedMessage.attachments.map((attachment) => (
                  <button
                    className="attachmentButton"
                    key={attachment.id}
                    onClick={() => onDownloadAttachment(selectedMessage, attachment.id)}
                  >
                    <Download size={15} />
                    <span>{attachment.name}</span>
                    <small>{formatBytes(attachment.size)}</small>
                  </button>
                ))}
              </div>
            )}
          </>
        ) : (
          <EmptyState icon={<Mail size={24} />} text="Select a message." />
        )}
      </article>
    </section>
  );
}

function AccountsView({
  groups,
  tags,
  accounts,
  settings,
  busy,
  onImport,
  onCreateGroup,
  onCreateTag,
  onDeleteAccount,
  onExportAccounts,
  onUpdateAccount,
  onGenerateOAuthUrl,
  onExchangeOAuthToken
}: {
  groups: Group[];
  tags: Tag[];
  accounts: Account[];
  settings: Settings | null;
  busy: boolean;
  onImport: (raw: string, groupId: number | null) => void;
  onCreateGroup: (name: string, color: string) => void;
  onCreateTag: (name: string, color: string) => void;
  onDeleteAccount: (accountId: number) => void;
  onExportAccounts: (groupId?: number | null) => void;
  onUpdateAccount: (input: Parameters<typeof api.updateAccount>[0]) => void;
  onGenerateOAuthUrl: (input: { client_id: string; redirect_uri: string; login_hint?: string }) => Promise<string>;
  onExchangeOAuthToken: (input: { account_id?: number; client_id: string; redirect_uri: string; code_or_url: string }) => void;
}) {
  const [raw, setRaw] = useState("");
  const [groupId, setGroupId] = useState<number | null>(groups[0]?.id ?? null);
  const [groupName, setGroupName] = useState("");
  const [tagName, setTagName] = useState("");
  const [colorIndex, setColorIndex] = useState(0);
  const [selectedAccountId, setSelectedAccountId] = useState<number | undefined>(accounts[0]?.id);
  const [oauthUrl, setOauthUrl] = useState("");
  const [oauthCallback, setOauthCallback] = useState("");
  const selectedAccount = accounts.find((account) => account.id === selectedAccountId) ?? accounts[0];
  const parsedRows = useMemo(() => parseAccountRows(raw), [raw]);

  return (
    <section className="managementGrid">
      <div className="panel">
        <div className="panelHeader">
          <h2>Import accounts</h2>
          <Upload size={18} />
        </div>
        <textarea
          className="textarea"
          value={raw}
          onChange={(event) => setRaw(event.target.value)}
          placeholder="email----password----client_id----refresh_token----remark"
        />
        <div className="formLine">
          <select className="select grow" value={groupId ?? ""} onChange={(event) => setGroupId(Number(event.target.value))}>
            {groups.map((group) => (
              <option value={group.id} key={group.id}>
                {group.name}
              </option>
            ))}
          </select>
          <button className="button primary" disabled={busy || parsedRows.length === 0} onClick={() => onImport(raw, groupId)}>
            <Download size={16} />
            Import {parsedRows.length || ""}
          </button>
        </div>
      </div>

      <div className="panel">
        <div className="panelHeader">
          <h2>Groups and tags</h2>
          <Tags size={18} />
        </div>
        <div className="formLine">
          <input className="input grow" value={groupName} placeholder="New group" onChange={(event) => setGroupName(event.target.value)} />
          <button
            className="button secondary"
            disabled={!groupName.trim()}
            onClick={() => {
              onCreateGroup(groupName, colors[colorIndex]);
              setGroupName("");
              setColorIndex((colorIndex + 1) % colors.length);
            }}
          >
            <Plus size={16} />
            Group
          </button>
        </div>
        <div className="chipCloud">
          {groups.map((group) => (
            <span className="chip" key={group.id}>
              <span className="dot" style={{ backgroundColor: group.color }} />
              {group.name}
            </span>
          ))}
        </div>
        <div className="formLine">
          <input className="input grow" value={tagName} placeholder="New tag" onChange={(event) => setTagName(event.target.value)} />
          <button
            className="button secondary"
            disabled={!tagName.trim()}
            onClick={() => {
              onCreateTag(tagName, colors[colorIndex]);
              setTagName("");
              setColorIndex((colorIndex + 1) % colors.length);
            }}
          >
            <Plus size={16} />
            Tag
          </button>
        </div>
        <div className="chipCloud">
          {tags.map((tag) => (
            <span className="chip" key={tag.id}>
              <span className="dot" style={{ backgroundColor: tag.color }} />
              {tag.name}
            </span>
          ))}
        </div>
      </div>

      <AccountEditor
        account={selectedAccount}
        groups={groups}
        settings={settings}
        busy={busy}
        oauthUrl={oauthUrl}
        oauthCallback={oauthCallback}
        onOauthCallbackChange={setOauthCallback}
        onSave={(input) => onUpdateAccount(input)}
        onGenerateOAuthUrl={async (input) => {
          const url = await onGenerateOAuthUrl(input);
          setOauthUrl(url);
        }}
        onExchangeOAuthToken={(input) => onExchangeOAuthToken(input)}
      />

      <div className="panel widePanel">
        <div className="panelHeader">
          <h2>Mailbox inventory</h2>
          <div className="rowActions">
            <span>{accounts.length} accounts</span>
            <button className="iconMini" title="Export accounts" disabled={accounts.length === 0 || busy} onClick={() => onExportAccounts()}>
              <Download size={15} />
            </button>
          </div>
        </div>
        <div className="table">
          <div className="tableHeader">
            <span>Email</span>
            <span>Group</span>
            <span>Status</span>
            <span>Secrets</span>
            <span />
          </div>
          {accounts.map((account) => (
            <div
              className={selectedAccount?.id === account.id ? "tableRow active" : "tableRow"}
              key={account.id}
              onClick={() => setSelectedAccountId(account.id)}
            >
              <span>{account.email}</span>
              <span>{account.group_name ?? "None"}</span>
              <span>{account.last_refresh_status}</span>
              <span>{account.has_refresh_token ? "Graph" : account.has_imap_password || account.has_password ? "IMAP" : "None"}</span>
              <button
                className="iconMini danger"
                title="Delete account"
                onClick={(event) => {
                  event.stopPropagation();
                  onDeleteAccount(account.id);
                }}
              >
                <Trash2 size={15} />
              </button>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}

function TempEmailsView({
  tempEmails,
  messages,
  channels,
  selectedEmail,
  selectedMessage,
  busy,
  onSelect,
  onMessageSelect,
  onGenerate,
  onImport,
  onRefresh,
  onDelete,
  onSaveChannel,
  onDeleteChannel,
  onTestChannel
}: {
  tempEmails: TempEmail[];
  messages: TempEmailMessage[];
  channels: CloudflareChannel[];
  selectedEmail?: string;
  selectedMessage?: TempEmailMessage;
  busy: boolean;
  onSelect: (email: string) => void;
  onMessageSelect: (messageId: string) => void;
  onGenerate: (input: { provider: string; prefix?: string; domain?: string; username?: string; password?: string; channel_id?: number | null }) => void;
  onImport: (input: { raw: string; provider: string; channel_id?: number | null }) => void;
  onRefresh: (email: string) => void;
  onDelete: (email: string) => void;
  onSaveChannel: (input: {
    id?: number;
    name: string;
    worker_domain: string;
    email_domains: string[];
    admin_password?: string;
    enabled: boolean;
    is_default: boolean;
  }) => void;
  onDeleteChannel: (channelId: number) => void;
  onTestChannel: (channelId: number) => void;
}) {
  const [provider, setProvider] = useState("gptmail");
  const [prefix, setPrefix] = useState("");
  const [domain, setDomain] = useState("");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [channelId, setChannelId] = useState<number | null>(channels[0]?.id ?? null);
  const [importRaw, setImportRaw] = useState("");
  const [channelDraft, setChannelDraft] = useState({
    id: undefined as number | undefined,
    name: "",
    worker_domain: "",
    email_domains: "",
    admin_password: "",
    enabled: true,
    is_default: false
  });

  useEffect(() => {
    if (!channelId && channels[0]) setChannelId(channels[0].id);
  }, [channels.length]);

  const selectedTemp = tempEmails.find((item) => item.email === selectedEmail);

  return (
    <section className="tempGrid">
      <aside className="panel tempControlPanel">
        <div className="panelHeader">
          <h2>Temp mail</h2>
          <Cloud size={18} />
        </div>
        <div className="formLine">
          <select className="select grow" value={provider} onChange={(event) => setProvider(event.target.value)}>
            <option value="gptmail">GPTMail</option>
            <option value="duckmail">DuckMail</option>
            <option value="cloudflare">Cloudflare</option>
          </select>
          {provider === "cloudflare" && (
            <select className="select grow" value={channelId ?? ""} onChange={(event) => setChannelId(Number(event.target.value) || null)}>
              <option value="">Channel</option>
              {channels.map((channel) => (
                <option key={channel.id} value={channel.id}>
                  {channel.name}
                </option>
              ))}
            </select>
          )}
        </div>
        <div className="formLine">
          <input
            className="input grow"
            value={provider === "gptmail" ? prefix : username}
            placeholder={provider === "gptmail" ? "Prefix" : "Username"}
            onChange={(event) => (provider === "gptmail" ? setPrefix(event.target.value) : setUsername(event.target.value))}
          />
          <input className="input grow" value={domain} placeholder="Domain" onChange={(event) => setDomain(event.target.value)} />
        </div>
        {provider === "duckmail" && (
          <input
            className="input fullWidth tempPassword"
            type="password"
            value={password}
            placeholder="DuckMail password"
            onChange={(event) => setPassword(event.target.value)}
          />
        )}
        <button
          className="button primary fullWidth"
          disabled={busy || (provider === "duckmail" && (!username.trim() || !domain.trim() || password.length < 6)) || (provider === "cloudflare" && !channelId)}
          onClick={() =>
            onGenerate({
              provider,
              prefix: prefix || undefined,
              domain: domain || undefined,
              username: username || undefined,
              password: password || undefined,
              channel_id: provider === "cloudflare" ? channelId : undefined
            })
          }
        >
          {busy ? <Loader2 className="spin" size={16} /> : <Plus size={16} />}
          Generate
        </button>

        <textarea
          className="textarea compact tempImportBox"
          value={importRaw}
          onChange={(event) => setImportRaw(event.target.value)}
          placeholder={provider === "duckmail" ? "email----password" : "email@example.com"}
        />
        <button
          className="button secondary fullWidth"
          disabled={busy || !importRaw.trim() || (provider === "cloudflare" && !channelId)}
          onClick={() => onImport({ raw: importRaw, provider, channel_id: provider === "cloudflare" ? channelId : undefined })}
        >
          <Upload size={16} />
          Import
        </button>
      </aside>

      <aside className="panel tempListPanel">
        <div className="panelHeader">
          <h2>Addresses</h2>
          <span>{tempEmails.length}</span>
        </div>
        <div className="tempRows">
          {tempEmails.map((item) => (
            <button key={item.id} className={selectedEmail === item.email ? "tempEmailRow active" : "tempEmailRow"} onClick={() => onSelect(item.email)}>
              <strong>{item.email}</strong>
              <small>
                {item.provider} 路 {item.message_count} messages 路 {item.last_refresh_status}
              </small>
            </button>
          ))}
        </div>
        {tempEmails.length === 0 && <EmptyState icon={<Cloud size={24} />} text="No temp emails yet." />}
      </aside>

      <section className="panel tempMessagePanel">
        <div className="panelHeader">
          <h2>{selectedTemp?.email ?? "Messages"}</h2>
          <div className="rowActions">
            <button className="iconMini" title="Refresh" disabled={!selectedEmail || busy} onClick={() => selectedEmail && onRefresh(selectedEmail)}>
              <RefreshCw size={15} />
            </button>
            <button className="iconMini danger" title="Delete" disabled={!selectedEmail || busy} onClick={() => selectedEmail && onDelete(selectedEmail)}>
              <Trash2 size={15} />
            </button>
          </div>
        </div>
        <div className="tempMessageRows">
          {messages.map((message) => (
            <button
              key={message.message_id}
              className={selectedMessage?.message_id === message.message_id ? "messageRow active" : "messageRow"}
              onClick={() => onMessageSelect(message.message_id)}
            >
              <span className="messageTop">
                <strong>{message.subject || "(no subject)"}</strong>
                <small>{message.timestamp ? formatUnixDate(message.timestamp) : formatDate(message.created_at)}</small>
              </span>
              <span className="sender">{message.from_address}</span>
              <span className="preview">{message.content || message.html_content}</span>
            </button>
          ))}
        </div>
        {messages.length === 0 && <EmptyState icon={<Mail size={24} />} text="No cached temp messages." />}
      </section>

      <article className="panel tempDetailPanel">
        {selectedMessage ? (
          <>
            <div className="detailHeader">
              <h2>{selectedMessage.subject || "(no subject)"}</h2>
              <p>{selectedMessage.from_address}</p>
            </div>
            <div className="metaGrid">
              <span>Mailbox</span>
              <strong>{selectedMessage.email_address}</strong>
              <span>Received</span>
              <strong>{selectedMessage.timestamp ? formatUnixDate(selectedMessage.timestamp) : formatDate(selectedMessage.created_at)}</strong>
            </div>
            <div className="messageBody">{selectedMessage.has_html ? selectedMessage.html_content : selectedMessage.content}</div>
          </>
        ) : (
          <EmptyState icon={<Mail size={24} />} text="Select a temp message." />
        )}
      </article>

      <section className="panel widePanel">
        <div className="panelHeader">
          <h2>Cloudflare channels</h2>
          <Cloud size={18} />
        </div>
        <div className="channelEditor">
          <input className="input" value={channelDraft.name} placeholder="Name" onChange={(event) => setChannelDraft({ ...channelDraft, name: event.target.value })} />
          <input
            className="input"
            value={channelDraft.worker_domain}
            placeholder="Worker domain"
            onChange={(event) => setChannelDraft({ ...channelDraft, worker_domain: event.target.value })}
          />
          <input
            className="input"
            value={channelDraft.email_domains}
            placeholder="Domains, comma separated"
            onChange={(event) => setChannelDraft({ ...channelDraft, email_domains: event.target.value })}
          />
          <input
            className="input"
            type="password"
            value={channelDraft.admin_password}
            placeholder="Admin password"
            onChange={(event) => setChannelDraft({ ...channelDraft, admin_password: event.target.value })}
          />
          <label className="checkLine">
            <input type="checkbox" checked={channelDraft.enabled} onChange={(event) => setChannelDraft({ ...channelDraft, enabled: event.target.checked })} />
            <span>Enabled</span>
          </label>
          <label className="checkLine">
            <input type="checkbox" checked={channelDraft.is_default} onChange={(event) => setChannelDraft({ ...channelDraft, is_default: event.target.checked })} />
            <span>Default</span>
          </label>
          <button
            className="button primary"
            disabled={busy || !channelDraft.name.trim() || !channelDraft.worker_domain.trim()}
            onClick={() => {
              onSaveChannel({
                id: channelDraft.id,
                name: channelDraft.name,
                worker_domain: channelDraft.worker_domain,
                email_domains: channelDraft.email_domains.split(/[,\n;]/).map((item) => item.trim()).filter(Boolean),
                admin_password: channelDraft.admin_password || undefined,
                enabled: channelDraft.enabled,
                is_default: channelDraft.is_default
              });
              setChannelDraft({ id: undefined, name: "", worker_domain: "", email_domains: "", admin_password: "", enabled: true, is_default: false });
            }}
          >
            <SettingsIcon size={16} />
            Save
          </button>
        </div>
        <div className="cloudflareChannelRows">
          {channels.map((channel) => (
            <div className="cloudflareChannelRow" key={channel.id}>
              <span>
                <strong>{channel.name}</strong>
                <small>{channel.worker_domain}</small>
              </span>
              <span>{channel.email_domains.join(", ")}</span>
              <StatusPill status={channel.enabled ? "success" : "removed"} />
              <span className="rowActions">
                <button className="iconMini" title="Edit" onClick={() => setChannelDraft({ id: channel.id, name: channel.name, worker_domain: channel.worker_domain, email_domains: channel.email_domains.join(", "), admin_password: "", enabled: channel.enabled, is_default: channel.is_default })}>
                  <SettingsIcon size={15} />
                </button>
                <button className="iconMini" title="Test" onClick={() => onTestChannel(channel.id)}>
                  <RefreshCw size={15} />
                </button>
                <button className="iconMini danger" title="Delete" disabled={channel.reference_count > 0} onClick={() => onDeleteChannel(channel.id)}>
                  <Trash2 size={15} />
                </button>
              </span>
            </div>
          ))}
        </div>
      </section>
    </section>
  );
}

function ProjectsView({
  projects,
  accounts,
  groups,
  busy,
  onCreate,
  onSelect,
  onSync,
  onClaim,
  onExport,
  onAction
}: {
  projects: Project[];
  accounts: ProjectAccount[];
  groups: Group[];
  busy: boolean;
  onCreate: (input: { name: string; project_key?: string; description?: string; scope_mode?: string; group_ids?: number[] }) => void;
  onSelect: (projectId: number) => void;
  onSync: (projectId: number) => void;
  onClaim: (projectId: number) => void;
  onExport: (projectId: number) => void;
  onAction: (projectId: number, action: "success" | "failed" | "release" | "remove" | "restore", projectAccountId: number) => void;
}) {
  const [selectedProjectId, setSelectedProjectId] = useState<number | undefined>(projects[0]?.id);
  const [name, setName] = useState("");
  const [projectKey, setProjectKey] = useState("");
  const [description, setDescription] = useState("");
  const [scopeMode, setScopeMode] = useState("all");
  const [selectedGroupIds, setSelectedGroupIds] = useState<number[]>([]);

  const selectedProject = projects.find((project) => project.id === selectedProjectId) ?? projects[0];

  useEffect(() => {
    if (!selectedProjectId && projects[0]) {
      setSelectedProjectId(projects[0].id);
      onSelect(projects[0].id);
    }
  }, [projects.length]);

  return (
    <section className="projectsGrid">
      <aside className="panel projectListPanel">
        <div className="panelHeader">
          <h2>Projects</h2>
          <FolderKanban size={18} />
        </div>
        <div className="projectCreate">
          <input className="input" value={name} placeholder="Project name" onChange={(event) => setName(event.target.value)} />
          <input className="input" value={projectKey} placeholder="Project key, optional" onChange={(event) => setProjectKey(event.target.value)} />
          <textarea
            className="textarea compact"
            value={description}
            placeholder="Description"
            onChange={(event) => setDescription(event.target.value)}
          />
          <select className="select" value={scopeMode} onChange={(event) => setScopeMode(event.target.value)}>
            <option value="all">All active accounts</option>
            <option value="groups">Selected groups</option>
          </select>
          {scopeMode === "groups" && (
            <div className="groupPicker">
              {groups.map((group) => (
                <label className="checkLine" key={group.id}>
                  <input
                    type="checkbox"
                    checked={selectedGroupIds.includes(group.id)}
                    onChange={(event) => {
                      setSelectedGroupIds((current) =>
                        event.target.checked ? [...current, group.id] : current.filter((id) => id !== group.id)
                      );
                    }}
                  />
                  <span>{group.name}</span>
                </label>
              ))}
            </div>
          )}
          <button
            className="button primary fullWidth"
            disabled={busy || !name.trim() || (scopeMode === "groups" && selectedGroupIds.length === 0)}
            onClick={() => {
              onCreate({
                name,
                project_key: projectKey || undefined,
                description,
                scope_mode: scopeMode,
                group_ids: scopeMode === "groups" ? selectedGroupIds : []
              });
              setName("");
              setProjectKey("");
              setDescription("");
            }}
          >
            <Plus size={16} />
            Create project
          </button>
        </div>

        <div className="projectRows">
          {projects.map((project) => (
            <button
              key={project.id}
              className={selectedProject?.id === project.id ? "projectRow active" : "projectRow"}
              onClick={() => {
                setSelectedProjectId(project.id);
                onSelect(project.id);
              }}
            >
              <strong>{project.name}</strong>
              <small>{project.project_key}</small>
              <span>{project.stats.to_claim} claimable · {project.stats.success} done</span>
            </button>
          ))}
        </div>
      </aside>

      <section className="panel projectDetailPanel">
        {selectedProject ? (
          <>
            <div className="projectHero">
              <div>
                <h2>{selectedProject.name}</h2>
                <p>{selectedProject.description || selectedProject.project_key}</p>
              </div>
              <div className="topActions">
                <button className="button secondary" disabled={busy} onClick={() => onSync(selectedProject.id)}>
                  <RefreshCw size={16} />
                  Sync
                </button>
                <button className="button secondary" disabled={busy || accounts.length === 0} onClick={() => onExport(selectedProject.id)}>
                  <Download size={16} />
                  Export
                </button>
                <button className="button primary" disabled={busy} onClick={() => onClaim(selectedProject.id)}>
                  <Archive size={16} />
                  Claim
                </button>
              </div>
            </div>

            <div className="statStrip">
              <Stat label="Total" value={selectedProject.stats.total} />
              <Stat label="Claimable" value={selectedProject.stats.to_claim} />
              <Stat label="Claimed" value={selectedProject.stats.claimed} />
              <Stat label="Success" value={selectedProject.stats.success} />
              <Stat label="Failed" value={selectedProject.stats.failed} />
              <Stat label="Removed" value={selectedProject.stats.removed} />
            </div>

            <div className="projectAccountTable">
              <div className="projectTableHeader">
                <span>Email</span>
                <span>Status</span>
                <span>Claims</span>
                <span>Lease</span>
                <span />
              </div>
              {accounts.map((account) => (
                <div className="projectTableRow" key={account.id}>
                  <span>
                    <strong>{account.email}</strong>
                    <small>{account.last_result_detail || account.normalized_email}</small>
                  </span>
                  <StatusPill status={account.status} />
                  <span>{account.claim_count}</span>
                  <span>{account.lease_expires_at ? formatDate(account.lease_expires_at) : ""}</span>
                  <span className="rowActions">
                    {account.status === "claimed" && (
                      <>
                        <button className="iconMini" title="Success" onClick={() => onAction(selectedProject.id, "success", account.id)}>
                          <CheckCircle2 size={15} />
                        </button>
                        <button className="iconMini danger" title="Failed" onClick={() => onAction(selectedProject.id, "failed", account.id)}>
                          <XCircle size={15} />
                        </button>
                        <button className="iconMini" title="Release" onClick={() => onAction(selectedProject.id, "release", account.id)}>
                          <RefreshCw size={15} />
                        </button>
                      </>
                    )}
                    {account.status !== "removed" ? (
                      <button className="iconMini danger" title="Remove" onClick={() => onAction(selectedProject.id, "remove", account.id)}>
                        <Trash2 size={15} />
                      </button>
                    ) : (
                      <button className="iconMini" title="Restore" onClick={() => onAction(selectedProject.id, "restore", account.id)}>
                        <RefreshCw size={15} />
                      </button>
                    )}
                  </span>
                </div>
              ))}
            </div>
            {accounts.length === 0 && <EmptyState icon={<FolderKanban size={24} />} text="Sync project scope to populate accounts." />}
          </>
        ) : (
          <EmptyState icon={<FolderKanban size={24} />} text="Create a project to start assigning accounts." />
        )}
      </section>
    </section>
  );
}

function Stat({ label, value }: { label: string; value: number }) {
  return (
    <div className="statBox">
      <strong>{value}</strong>
      <span>{label}</span>
    </div>
  );
}

function StatusPill({ status }: { status: string }) {
  return <span className={`statusPill status-${status}`}>{status}</span>;
}

function AccountEditor({
  account,
  groups,
  settings,
  busy,
  oauthUrl,
  oauthCallback,
  onOauthCallbackChange,
  onSave,
  onGenerateOAuthUrl,
  onExchangeOAuthToken
}: {
  account?: Account;
  groups: Group[];
  settings: Settings | null;
  busy: boolean;
  oauthUrl: string;
  oauthCallback: string;
  onOauthCallbackChange: (value: string) => void;
  onSave: (input: Parameters<typeof api.updateAccount>[0]) => void;
  onGenerateOAuthUrl: (input: { client_id: string; redirect_uri: string; login_hint?: string }) => void;
  onExchangeOAuthToken: (input: { account_id?: number; client_id: string; redirect_uri: string; code_or_url: string }) => void;
}) {
  const [draft, setDraft] = useState({
    email: "",
    group_id: 1 as number | null,
    provider: "graph",
    account_type: "outlook",
    remark: "",
    forward_enabled: false,
    imap_host: "",
    imap_port: 993,
    password: "",
    client_id: "",
    refresh_token: "",
    imap_password: ""
  });

  useEffect(() => {
    if (!account) return;
    setDraft({
      email: account.email,
      group_id: account.group_id,
      provider: account.provider || "graph",
      account_type: account.account_type || "outlook",
      remark: account.remark,
      forward_enabled: account.forward_enabled,
      imap_host: account.imap_host,
      imap_port: account.imap_port || 993,
      password: "",
      client_id: settings?.graph_client_id ?? "",
      refresh_token: "",
      imap_password: ""
    });
    onOauthCallbackChange("");
  }, [account?.id, settings?.graph_client_id]);

  if (!account) {
    return (
      <div className="panel">
        <EmptyState icon={<KeyRound size={24} />} text="Select an account to configure authorization." />
      </div>
    );
  }

  const redirectUri = settings?.oauth_redirect_uri || "http://localhost:8080";

  return (
    <div className="panel">
      <div className="panelHeader">
        <h2>Authorization</h2>
        <KeyRound size={18} />
      </div>
      <div className="formLine">
        <input className="input grow" value={draft.email} onChange={(event) => setDraft({ ...draft, email: event.target.value })} />
        <select className="select" value={draft.provider} onChange={(event) => setDraft({ ...draft, provider: event.target.value })}>
          <option value="graph">Graph</option>
          <option value="imap">IMAP</option>
          <option value="outlook">Outlook</option>
        </select>
      </div>
      <div className="formLine">
        <select
          className="select grow"
          value={draft.group_id ?? ""}
          onChange={(event) => setDraft({ ...draft, group_id: Number(event.target.value) })}
        >
          {groups.map((group) => (
            <option value={group.id} key={group.id}>
              {group.name}
            </option>
          ))}
        </select>
        <input
          className="input grow"
          value={draft.remark}
          placeholder="Remark"
          onChange={(event) => setDraft({ ...draft, remark: event.target.value })}
        />
      </div>
      <label className="checkLine toggleLine">
        <input
          type="checkbox"
          checked={draft.forward_enabled}
          onChange={(event) => setDraft({ ...draft, forward_enabled: event.target.checked })}
        />
        <span>Forward cached messages for this account</span>
      </label>
      <div className="formLine">
        <input
          className="input grow"
          value={draft.client_id}
          placeholder="Microsoft client id"
          onChange={(event) => setDraft({ ...draft, client_id: event.target.value })}
        />
        <button
          className="button secondary"
          disabled={!draft.client_id.trim()}
          onClick={() => onGenerateOAuthUrl({ client_id: draft.client_id, redirect_uri: redirectUri, login_hint: draft.email })}
        >
          <KeyRound size={16} />
          OAuth URL
        </button>
      </div>
      {oauthUrl && <textarea className="textarea compact" readOnly value={oauthUrl} />}
      <div className="formLine">
        <input
          className="input grow"
          value={oauthCallback}
          placeholder="Paste callback URL or authorization code"
          onChange={(event) => onOauthCallbackChange(event.target.value)}
        />
        <button
          className="button secondary"
          disabled={!draft.client_id.trim() || !oauthCallback.trim()}
          onClick={() =>
            onExchangeOAuthToken({
              account_id: account.id,
              client_id: draft.client_id,
              redirect_uri: redirectUri,
              code_or_url: oauthCallback
            })
          }
        >
          Save OAuth
        </button>
      </div>
      <div className="formLine">
        <input
          className="input grow"
          value={draft.imap_host}
          placeholder="IMAP host"
          onChange={(event) => setDraft({ ...draft, imap_host: event.target.value })}
        />
        <input
          className="input smallInput"
          type="number"
          value={draft.imap_port}
          onChange={(event) => setDraft({ ...draft, imap_port: Number(event.target.value) || 993 })}
        />
      </div>
      <div className="formLine">
        <input
          className="input grow"
          type="password"
          value={draft.password}
          placeholder="Account password, optional"
          onChange={(event) => setDraft({ ...draft, password: event.target.value })}
        />
        <input
          className="input grow"
          type="password"
          value={draft.imap_password}
          placeholder="IMAP password, optional"
          onChange={(event) => setDraft({ ...draft, imap_password: event.target.value })}
        />
      </div>
      <button
        className="button primary fullWidth"
        disabled={busy}
        onClick={() =>
          onSave({
            id: account.id,
            email: draft.email,
            group_id: draft.group_id,
            provider: draft.provider,
            account_type: draft.account_type,
            remark: draft.remark,
            forward_enabled: draft.forward_enabled,
            imap_host: draft.imap_host,
            imap_port: draft.imap_port,
            client_id: draft.client_id || undefined,
            password: draft.password || undefined,
            imap_password: draft.imap_password || undefined,
            refresh_token: draft.refresh_token || undefined
          })
        }
      >
        {busy ? <Loader2 className="spin" size={16} /> : <SettingsIcon size={16} />}
        Save account
      </button>
    </div>
  );
}

function SettingsView({
  status,
  settings,
  forwardingLogs,
  backupLogs,
  automationRuns,
  schedulerStatus,
  busy,
  onSave,
  onRunForwarding,
  onRunBackup
}: {
  status: AppStatus;
  settings: Settings;
  forwardingLogs: ForwardingLog[];
  backupLogs: BackupLog[];
  automationRuns: AutomationRun[];
  schedulerStatus: SchedulerStatus | null;
  busy: boolean;
  onSave: (settings: Settings) => void;
  onRunForwarding: () => void;
  onRunBackup: () => void;
}) {
  const [draft, setDraft] = useState(settings);
  useEffect(() => setDraft(settings), [settings]);

  function setField<K extends keyof Settings>(key: K, value: Settings[K]) {
    setDraft((current) => ({ ...current, [key]: value }));
  }

  return (
    <section className="settingsGrid">
      <div className="panel">
        <div className="panelHeader">
          <h2>Provider settings</h2>
          <SettingsIcon size={18} />
        </div>
        <Field label="Microsoft Graph client ID" value={draft.graph_client_id} onChange={(value) => setField("graph_client_id", value)} />
        <Field label="OAuth redirect URI" value={draft.oauth_redirect_uri} onChange={(value) => setField("oauth_redirect_uri", value)} />
        <Field label="GPTMail base URL" value={draft.gptmail_base_url} onChange={(value) => setField("gptmail_base_url", value)} />
        <SecretField label="GPTMail API key" value={draft.gptmail_api_key} onChange={(value) => setField("gptmail_api_key", value)} />
        <Field label="DuckMail base URL" value={draft.duckmail_base_url} onChange={(value) => setField("duckmail_base_url", value)} />
        <SecretField label="DuckMail API key" value={draft.duckmail_api_key} onChange={(value) => setField("duckmail_api_key", value)} />
      </div>

      <div className="panel">
        <div className="panelHeader">
          <h2>Scheduler</h2>
          <RefreshCw size={18} />
        </div>
        <label className="checkLine toggleLine">
          <input
            type="checkbox"
            checked={draft.scheduler_refresh_enabled}
            onChange={(event) => setField("scheduler_refresh_enabled", event.target.checked)}
          />
          <span>Scheduled mailbox refresh</span>
        </label>
        <div className="formLine">
          <NumberField
            label="Refresh interval"
            value={draft.scheduler_refresh_interval_minutes}
            min={1}
            onChange={(value) => setField("scheduler_refresh_interval_minutes", value)}
          />
          <NumberField
            label="Messages per account"
            value={draft.scheduler_refresh_top}
            min={1}
            max={50}
            onChange={(value) => setField("scheduler_refresh_top", value)}
          />
        </div>
        <label className="checkLine toggleLine">
          <input
            type="checkbox"
            checked={draft.forwarding_enabled}
            onChange={(event) => setField("forwarding_enabled", event.target.checked)}
          />
          <span>Scheduled forwarding</span>
        </label>
        <NumberField
          label="Forwarding interval"
          value={draft.forwarding_interval_minutes}
          min={1}
          onChange={(value) => setField("forwarding_interval_minutes", value)}
        />
        <label className="checkLine toggleLine">
          <input
            type="checkbox"
            checked={draft.backup_enabled}
            onChange={(event) => setField("backup_enabled", event.target.checked)}
          />
          <span>Scheduled WebDAV backup</span>
        </label>
        <NumberField
          label="Backup interval"
          value={draft.backup_interval_minutes}
          min={1}
          onChange={(value) => setField("backup_interval_minutes", value)}
        />
      </div>

      <div className="panel">
        <div className="panelHeader">
          <h2>Forwarding channels</h2>
          <Mail size={18} />
        </div>
        <div className="formLine">
          <Field label="SMTP host" value={draft.forward_smtp_host} onChange={(value) => setField("forward_smtp_host", value)} />
          <NumberField
            label="Port"
            value={draft.forward_smtp_port}
            min={1}
            max={65535}
            onChange={(value) => setField("forward_smtp_port", value)}
          />
        </div>
        <Field label="SMTP username" value={draft.forward_smtp_username} onChange={(value) => setField("forward_smtp_username", value)} />
        <SecretField
          label="SMTP password"
          value={draft.forward_smtp_password}
          onChange={(value) => setField("forward_smtp_password", value)}
        />
        <Field label="SMTP from" value={draft.forward_smtp_from} onChange={(value) => setField("forward_smtp_from", value)} />
        <Field label="SMTP recipients" value={draft.forward_smtp_to} onChange={(value) => setField("forward_smtp_to", value)} />
        <SecretField
          label="Telegram bot token"
          value={draft.forward_telegram_bot_token}
          onChange={(value) => setField("forward_telegram_bot_token", value)}
        />
        <Field
          label="Telegram chat ID"
          value={draft.forward_telegram_chat_id}
          onChange={(value) => setField("forward_telegram_chat_id", value)}
        />
        <SecretField
          label="WeCom webhook"
          value={draft.forward_wecom_webhook}
          onChange={(value) => setField("forward_wecom_webhook", value)}
        />
      </div>

      <div className="panel">
        <div className="panelHeader">
          <h2>WebDAV backup</h2>
          <Archive size={18} />
        </div>
        <Field label="WebDAV URL" value={draft.webdav_url} onChange={(value) => setField("webdav_url", value)} />
        <Field label="WebDAV username" value={draft.webdav_username} onChange={(value) => setField("webdav_username", value)} />
        <SecretField label="WebDAV password" value={draft.webdav_password} onChange={(value) => setField("webdav_password", value)} />
        <div className="actionGrid">
          <button className="button secondary" disabled={busy} onClick={onRunForwarding}>
            {busy ? <Loader2 className="spin" size={16} /> : <Mail size={16} />}
            Run forwarding
          </button>
          <button className="button secondary" disabled={busy} onClick={onRunBackup}>
            {busy ? <Loader2 className="spin" size={16} /> : <Archive size={16} />}
            Run backup
          </button>
        </div>
      </div>

      <div className="panel widePanel">
        <div className="panelHeader">
          <h2>Local storage and runs</h2>
          <Lock size={18} />
        </div>
        <div className="storageLine">
          <span>SQLite database</span>
          <code>{status.db_path}</code>
        </div>
        <div className="runStatusGrid">
          <RunStatus label="Refresh" value={schedulerStatus?.last_refresh_at} />
          <RunStatus label="Forwarding" value={schedulerStatus?.last_forwarding_at} />
          <RunStatus label="Backup" value={schedulerStatus?.last_backup_at} />
        </div>
        <button className="button primary" disabled={busy} onClick={() => onSave(draft)}>
          {busy ? <Loader2 className="spin" size={16} /> : <SettingsIcon size={16} />}
          Save settings
        </button>
      </div>

      <div className="panel widePanel">
        <div className="panelHeader">
          <h2>Automation history</h2>
          <RefreshCw size={18} />
        </div>
        <div className="logTable automationLogTable">
          <div className="logHeader">
            <span>Time</span>
            <span>Job</span>
            <span>Trigger</span>
            <span>Status</span>
            <span>Counts</span>
            <span>Duration</span>
            <span>Detail</span>
          </div>
          {automationRuns.map((run) => (
            <div className="logRow" key={run.id}>
              <span>{formatDate(run.finished_at)}</span>
              <span>{run.job_type}</span>
              <span>{run.trigger_type}</span>
              <StatusPill status={run.status} />
              <span>{run.refreshed} ok / {run.failed} failed</span>
              <span>{formatDuration(run.duration_ms)}</span>
              <span>{run.message}</span>
            </div>
          ))}
        </div>
        {automationRuns.length === 0 && <EmptyState icon={<RefreshCw size={24} />} text="No automation runs yet." />}
      </div>

      <div className="panel widePanel">
        <div className="panelHeader">
          <h2>Forwarding logs</h2>
          <Mail size={18} />
        </div>
        <div className="logTable forwardingLogTable">
          <div className="logHeader">
            <span>Time</span>
            <span>Account</span>
            <span>Channel</span>
            <span>Status</span>
            <span>Detail</span>
          </div>
          {forwardingLogs.map((log) => (
            <div className="logRow" key={log.id}>
              <span>{formatDate(log.created_at)}</span>
              <span>{log.account_email}</span>
              <span>{log.channel}</span>
              <StatusPill status={log.status} />
              <span>{log.error_message || log.message_id}</span>
            </div>
          ))}
        </div>
        {forwardingLogs.length === 0 && <EmptyState icon={<Mail size={24} />} text="No forwarding runs yet." />}
      </div>

      <div className="panel widePanel">
        <div className="panelHeader">
          <h2>Backup logs</h2>
          <Archive size={18} />
        </div>
        <div className="logTable backupLogTable">
          <div className="logHeader">
            <span>Time</span>
            <span>File</span>
            <span>Status</span>
            <span>Size</span>
            <span>Target</span>
          </div>
          {backupLogs.map((log) => (
            <div className="logRow" key={log.id}>
              <span>{formatDate(log.created_at)}</span>
              <span>{log.file_name}</span>
              <StatusPill status={log.status} />
              <span>{formatBytes(log.size)}</span>
              <span>{log.error_message || log.target}</span>
            </div>
          ))}
        </div>
        {backupLogs.length === 0 && <EmptyState icon={<Archive size={24} />} text="No backups yet." />}
      </div>
    </section>
  );
}

function Field({ label, value, onChange }: { label: string; value: string; onChange: (value: string) => void }) {
  return (
    <label className="field">
      <span>{label}</span>
      <input className="input" value={value} onChange={(event) => onChange(event.target.value)} />
    </label>
  );
}

function SecretField({ label, value, onChange }: { label: string; value: string; onChange: (value: string) => void }) {
  return (
    <label className="field">
      <span>{label}</span>
      <input className="input" type="password" value={value} onChange={(event) => onChange(event.target.value)} />
    </label>
  );
}

function NumberField({
  label,
  value,
  min,
  max,
  onChange
}: {
  label: string;
  value: number;
  min?: number;
  max?: number;
  onChange: (value: number) => void;
}) {
  return (
    <label className="field grow">
      <span>{label}</span>
      <input
        className="input"
        type="number"
        min={min}
        max={max}
        value={value}
        onChange={(event) => onChange(Number(event.target.value) || min || 1)}
      />
    </label>
  );
}

function RunStatus({ label, value }: { label: string; value?: string | null }) {
  return (
    <div className="runStatus">
      <span>{label}</span>
      <strong>{value ? formatDate(value) : "Never"}</strong>
    </div>
  );
}

function IconButton({
  children,
  active,
  title,
  onClick
}: {
  children: ReactNode;
  active?: boolean;
  title: string;
  onClick: () => void;
}) {
  return (
    <button className={active ? "railButton active" : "railButton"} title={title} aria-label={title} onClick={onClick}>
      {children}
    </button>
  );
}

function EmptyState({ icon, text }: { icon: ReactNode; text: string }) {
  return (
    <div className="emptyState">
      {icon}
      <span>{text}</span>
    </div>
  );
}

function formatDate(value: string) {
  if (!value) return "";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit"
  }).format(date);
}

function formatUnixDate(value: number) {
  if (!value) return "";
  return formatDate(new Date(value * 1000).toISOString());
}

function formatBytes(value: number) {
  if (!value) return "";
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`;
  return `${(value / 1024 / 1024).toFixed(1)} MB`;
}

function formatDuration(value: number) {
  if (!value) return "0 ms";
  if (value < 1000) return `${value} ms`;
  return `${(value / 1000).toFixed(1)} s`;
}

function exportNotice(result: ExportResult) {
  const size = formatBytes(result.size);
  return `Exported ${result.item_count} item(s) to ${result.path}${size ? ` (${size})` : ""}`;
}

function readError(err: unknown) {
  if (err instanceof Error) return err.message;
  if (typeof err === "object" && err && "message" in err) return String((err as { message: unknown }).message);
  return String(err);
}

export default App;
