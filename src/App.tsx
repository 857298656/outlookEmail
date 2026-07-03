import {
  Archive,
  CheckCircle2,
  ChevronDown,
  ChevronUp,
  Cloud,
  Download,
  FileText,
  FolderKanban,
  Inbox,
  KeyRound,
  Loader2,
  Lock,
  Mail,
  PanelLeftClose,
  PanelLeftOpen,
  Plus,
  RefreshCw,
  RotateCcw,
  Search,
  Settings as SettingsIcon,
  Tags,
  Trash2,
  Upload,
  Users,
  WandSparkles,
  XCircle
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";
import { api } from "./api";
import { buildSandboxedEmailHtml } from "./lib/emailHtml";
import { parseAccountRows } from "./lib/importParser";
import type {
  Account,
  AppStatus,
  AutomationRun,
  AutomationRunQuery,
  BackupLog,
  ClearLocalDataInput,
  ExportResult,
  ForwardingLog,
  Group,
  ImportAccountsResult,
  LocalRetentionSummary,
  MailMessage,
  MailMessageQuery,
  MailRawContent,
  Project,
  ProjectAccount,
  RefreshLog,
  RetryQueueItem,
  RemoteSyncFailure,
  SchedulerStatus,
  Settings,
  Tag,
  TempEmail,
  TempEmailMessage,
  CloudflareChannel
} from "./types";

type View = "mail" | "accounts" | "refresh" | "temp" | "projects" | "settings";
type MailFilters = {
  search: string;
  readState: "all" | "read" | "unread";
  attachmentFilter: "all" | "attachments" | "plain";
  sortBy: "date" | "subject" | "sender" | "read" | "attachments" | "folder";
  sortOrder: "asc" | "desc";
};

const colors = ["#111827", "#374151", "#4b5563", "#64748b", "#0f172a", "#52525b"];
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
  const [retryQueue, setRetryQueue] = useState<RetryQueueItem[]>([]);
  const [refreshLogs, setRefreshLogs] = useState<RefreshLog[]>([]);
  const [localRetention, setLocalRetention] = useState<LocalRetentionSummary | null>(null);
  const [schedulerStatus, setSchedulerStatus] = useState<SchedulerStatus | null>(null);
  const [selectedGroupId, setSelectedGroupId] = useState<number | "all">("all");
  const [selectedAccountId, setSelectedAccountId] = useState<number | undefined>();
  const [selectedMessageId, setSelectedMessageId] = useState<number | undefined>();
  const [selectedTempEmail, setSelectedTempEmail] = useState<string | undefined>();
  const [selectedTempMessageId, setSelectedTempMessageId] = useState<string | undefined>();
  const [folder, setFolder] = useState("all");
  const [mailFilters, setMailFilters] = useState<MailFilters>({
    search: "",
    readState: "all",
    attachmentFilter: "all",
    sortBy: "date",
    sortOrder: "desc"
  });
  const [mailPage, setMailPage] = useState(0);
  const [selectedMessageIds, setSelectedMessageIds] = useState<number[]>([]);
  const [view, setView] = useState<View>("mail");
  const [railExpanded, setRailExpanded] = useState(false);
  const [railMenuOpen, setRailMenuOpen] = useState(false);
  const railMenuRef = useRef<HTMLDivElement | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const selectedAccount = accounts.find((account) => account.id === selectedAccountId);
  const selectedMessage = messages.find((message) => message.id === selectedMessageId);
  const selectedTempMessage = tempMessages.find((message) => message.message_id === selectedTempMessageId);
  const railIdentity = selectedAccount?.email ?? accounts[0]?.email ?? "管理员";
  const railInitial = railIdentity === "管理员" ? "管" : railIdentity.slice(0, 1).toUpperCase();
  const filteredAccounts = useMemo(() => {
    if (selectedGroupId === "all") return accounts;
    return accounts.filter((account) => account.group_id === selectedGroupId);
  }, [accounts, selectedGroupId]);

  useEffect(() => {
    if (!railMenuOpen) return;

    function handlePointerDown(event: PointerEvent) {
      if (railMenuRef.current?.contains(event.target as Node)) return;
      setRailMenuOpen(false);
    }

    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") setRailMenuOpen(false);
    }

    document.addEventListener("pointerdown", handlePointerDown);
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("pointerdown", handlePointerDown);
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [railMenuOpen]);

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
      sort_by: filters.sortBy,
      sort_order: filters.sortOrder,
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

  async function loadWorkspace(
    accountId: number | undefined | null = selectedAccountId,
    nextFolder = folder,
    filters = mailFilters,
    page = mailPage
  ) {
    const [nextGroups, nextTags, nextAccounts] = await Promise.all([
      api.listGroups(),
      api.listTags(),
      api.listAccounts()
    ]);
    setGroups(nextGroups);
    setTags(nextTags);
    setAccounts(nextAccounts);
    const firstAccountId = accountId === null ? nextAccounts[0]?.id : accountId ?? nextAccounts[0]?.id;
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
    const [
      nextSettings,
      nextForwardingLogs,
      nextBackupLogs,
      nextAutomationRuns,
      nextRetryQueue,
      nextRefreshLogs,
      nextSchedulerStatus,
      nextLocalRetention
    ] = await Promise.all([
      api.getSettings(),
      api.listForwardingLogs(80),
      api.listBackupLogs(40),
      api.listAutomationRuns({}, 80),
      api.listRetryQueue({}, 80),
      api.listRefreshLogs(null, 100),
      api.schedulerStatus(),
      api.getLocalRetentionSummary()
    ]);
    setSettings(nextSettings);
    setForwardingLogs(nextForwardingLogs);
    setBackupLogs(nextBackupLogs);
    setAutomationRuns(nextAutomationRuns);
    setRetryQueue(nextRetryQueue);
    setRefreshLogs(nextRefreshLogs);
    setSchedulerStatus(nextSchedulerStatus);
    setLocalRetention(nextLocalRetention);
  }

  async function loadTempWorkspace(email: string | undefined | null = selectedTempEmail) {
    const [nextTempEmails, nextChannels] = await Promise.all([api.listTempEmails(), api.listCloudflareChannels()]);
    setTempEmails(nextTempEmails);
    setCloudflareChannels(nextChannels);
    const nextEmail = email === null ? nextTempEmails[0]?.email : email ?? nextTempEmails[0]?.email;
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

  useEffect(() => {
    function handleBeforeUnload(event: BeforeUnloadEvent) {
      if (!busy) return;
      event.preventDefault();
      event.returnValue = "";
    }

    window.addEventListener("beforeunload", handleBeforeUnload);
    return () => window.removeEventListener("beforeunload", handleBeforeUnload);
  }, [busy]);

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
    <div className={railExpanded ? "appShell railExpanded" : "appShell"}>
      <aside className={railExpanded ? "rail expanded" : "rail"}>
        <div className="railHeader">
          <span className="brandName">OutlookEmail</span>
          <button
            className="railHeaderToggle"
            title={railExpanded ? "收起侧边栏" : "展开侧边栏"}
            aria-label={railExpanded ? "收起侧边栏" : "展开侧边栏"}
            onClick={() => setRailExpanded((current) => !current)}
          >
            {railExpanded ? <PanelLeftClose size={19} /> : <PanelLeftOpen size={19} />}
          </button>
        </div>
        <IconButton
          active={view === "mail"}
          title="邮箱"
          onClick={() => {
            setView("mail");
            setRailMenuOpen(false);
          }}
        >
          <Inbox size={20} />
        </IconButton>
        <IconButton
          active={view === "accounts"}
          title="账号"
          onClick={() => {
            setView("accounts");
            setRailMenuOpen(false);
          }}
        >
          <Users size={20} />
        </IconButton>
        <IconButton
          active={view === "refresh"}
          title="刷新"
          onClick={() => {
            setView("refresh");
            setRailMenuOpen(false);
          }}
        >
          <RefreshCw size={20} />
        </IconButton>
        <IconButton
          active={view === "temp"}
          title="临时邮箱"
          onClick={() => {
            setView("temp");
            setRailMenuOpen(false);
          }}
        >
          <Cloud size={20} />
        </IconButton>
        <IconButton
          active={view === "projects"}
          title="项目"
          onClick={() => {
            setView("projects");
            setRailMenuOpen(false);
          }}
        >
          <FolderKanban size={20} />
        </IconButton>
        <div className="railSpacer" />
        <div className="railAccountArea" ref={railMenuRef}>
          {railMenuOpen && (
            <div className="railAccountMenu">
              <div className="railMenuHeader">{railIdentity}</div>
              <button
                className="railMenuItem"
                onClick={() => {
                  setView("settings");
                  setRailMenuOpen(false);
                }}
              >
                <SettingsIcon size={18} />
                <span>设置</span>
              </button>
              <div className="railMenuDivider" />
              <button
                className="railMenuItem"
                onClick={() =>
                  runAction(async () => {
                    setRailMenuOpen(false);
                    setStatus(await api.lock());
                  })
                }
              >
                <Lock size={18} />
                <span>锁定工作区</span>
              </button>
            </div>
          )}
          <button
            className={railMenuOpen ? "railAccountButton active" : "railAccountButton"}
            title="工作区菜单"
            aria-label="工作区菜单"
            onClick={() => setRailMenuOpen((current) => !current)}
          >
            <span className="railAvatar">{railInitial}</span>
            <span className="railAccountText">
              <strong>{railIdentity}</strong>
            </span>
            {railMenuOpen ? <ChevronDown className="railAccountChevron" size={16} /> : <ChevronUp className="railAccountChevron" size={16} />}
          </button>
        </div>
      </aside>

      <main className="mainSurface">
        <header className="topBar">
          <div>
            <h1>OutlookEmail 桌面版</h1>
            <p>{status.account_count} 个账号 · {status.message_count} 封缓存邮件</p>
          </div>
          <div className="topActions">
            {notice && <span className="notice">{notice}</span>}
            {error && <span className="errorText">{error}</span>}
            <button
              className="button secondary"
              onClick={() =>
                runAction(async () => {
                  const result = await api.runRefreshJob(selectedAccountId);
                  setNotice(formatResultMessage(result.message));
                  await loadWorkspace(selectedAccountId, folder, mailFilters, mailPage);
                  await loadStatus();
                })
              }
              disabled={busy}
            >
              {busy ? <Loader2 className="spin" size={16} /> : <RefreshCw size={16} />}
              刷新
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
            busy={busy}
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
                setNotice(formatResultMessage(result.message));
                await loadMailboxMessages(selectedAccountId, folder, mailFilters, mailPage);
                await loadAutomation();
              })
            }
            onDeleteMessages={(messageIds) =>
              runAction(async () => {
                const result = await api.deleteMessages(messageIds);
                setNotice(formatResultMessage(result.message));
                await loadMailboxMessages(selectedAccountId, folder, mailFilters, mailPage);
                await loadStatus();
                await loadAutomation();
              })
            }
            onExportMessages={(messageIds) =>
              runAction(async () => {
                const result = await api.exportMailMessages(messageIds, "OutlookEmail 邮件导出");
                setNotice(exportNotice(result));
              })
            }
            onCreateDemo={() =>
              selectedAccountId
                ? runAction(async () => {
                    await api.createDemoMessage(selectedAccountId);
                    setMailPage(0);
                    await loadWorkspace(selectedAccountId, folder, mailFilters, 0);
                  }, "已创建本地邮件")
                : undefined
            }
            onDownloadAttachment={async (message, attachmentId) => {
              setBusy(true);
              setError(null);
              setNotice(null);
              try {
                const result = await api.downloadAttachment({
                  account_id: message.account_id,
                  message_id: message.provider_message_id,
                  attachment_id: attachmentId,
                  folder: message.folder
                });
                setNotice(`已下载 ${result.file_name}`);
              } catch (err) {
                setError(readError(err));
                throw err;
              } finally {
                setBusy(false);
              }
            }}
            onDownloadAllAttachments={async (message) => {
              setBusy(true);
              setError(null);
              setNotice(null);
              try {
                const result = await api.downloadAllAttachments({
                  account_id: message.account_id,
                  message_id: message.provider_message_id,
                  folder: message.folder
                });
                setNotice(exportNotice(result));
              } catch (err) {
                setError(readError(err));
                throw err;
              } finally {
                setBusy(false);
              }
            }}
            onViewRawMessage={async (message) => {
              setBusy(true);
              setError(null);
              setNotice(null);
              try {
                return await api.getMailRawContent(message.id);
              } catch (err) {
                setError(readError(err));
                throw err;
              } finally {
                setBusy(false);
              }
            }}
            onRetryRemoteFailure={(retryId) =>
              runAction(async () => {
                const result = await api.retryQueueItem(retryId);
                setNotice(formatResultMessage(result.message));
                await loadMailboxMessages(selectedAccountId, folder, mailFilters, mailPage);
                await loadAutomation();
              })
            }
            onDismissRemoteFailure={(retryId) =>
              runAction(async () => {
                const result = await api.dismissRetryItem(retryId);
                setNotice(formatResultMessage(result.message));
                await loadMailboxMessages(selectedAccountId, folder, mailFilters, mailPage);
                await loadAutomation();
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
              }, "账号已导入")
            }
            onCreateGroup={(name, color) =>
              runAction(async () => {
                await api.createGroup({ name, color });
                await loadWorkspace(selectedAccountId, folder);
              }, "分组已创建")
            }
            onUpdateGroup={(input) =>
              runAction(async () => {
                await api.updateGroup(input);
                await loadWorkspace(selectedAccountId, folder);
              }, "分组已保存")
            }
            onDeleteGroup={(groupId) =>
              runAction(async () => {
                await api.deleteGroup(groupId);
                await loadWorkspace(selectedAccountId, folder);
                await loadStatus();
              }, "分组已删除")
            }
            onCreateTag={(name, color) =>
              runAction(async () => {
                await api.createTag({ name, color });
                await loadWorkspace(selectedAccountId, folder);
              }, "标签已创建")
            }
            onDeleteAccount={(accountId) =>
              runAction(async () => {
                await api.deleteAccount(accountId);
                await loadWorkspace(undefined, folder);
                await loadStatus();
              }, "账号已删除")
            }
            onBatchAccounts={(input) =>
              runAction(async () => {
                const result = await api.batchAccounts(input);
                setNotice(formatResultMessage(result.message));
                const nextSelectedAccountId =
                  input.action === "delete" && selectedAccountId && input.account_ids.includes(selectedAccountId)
                    ? undefined
                    : selectedAccountId;
                await loadWorkspace(nextSelectedAccountId, folder);
                await loadStatus();
              })
            }
            onExportAccounts={(groupId, accountIds) =>
              runAction(async () => {
                const result = await api.exportAccounts(groupId, accountIds);
                setNotice(exportNotice(result));
              })
            }
            onUpdateAccount={(input) =>
              runAction(async () => {
                await api.updateAccount(input);
                await loadWorkspace(input.id, folder);
              }, "账号已保存")
            }
            onRevealAccountSecrets={(input) => api.revealAccountSecrets(input)}
            onGenerateOAuthUrl={(input) => api.generateOAuthAuthUrl(input)}
            onExchangeOAuthToken={(input) =>
              runAction(async () => {
                const result = await api.exchangeOAuthToken(input);
                setNotice(`OAuth 已保存：${result.refresh_token_preview}`);
                await loadWorkspace(input.account_id, folder);
              })
            }
          />
        )}

        {view === "refresh" && (
          <RefreshManagementView
            accounts={accounts}
            retryQueue={retryQueue}
            refreshLogs={refreshLogs}
            automationRuns={automationRuns}
            schedulerStatus={schedulerStatus}
            busy={busy}
            onRefreshAccount={(accountId) =>
              runAction(async () => {
                const result = await api.runRefreshJob(accountId);
                setNotice(formatResultMessage(result.message));
                await loadWorkspace(accountId, folder, mailFilters, mailPage);
                await loadAutomation();
                await loadStatus();
              })
            }
            onRefreshAll={() =>
              runAction(async () => {
                const result = await api.runRefreshJob(undefined);
                setNotice(formatResultMessage(result.message));
                await loadWorkspace(selectedAccountId, folder, mailFilters, mailPage);
                await loadAutomation();
                await loadStatus();
              })
            }
            onRunRetryQueue={() =>
              runAction(async () => {
                const result = await api.runRetryQueue(20);
                setNotice(formatResultMessage(result.message));
                await loadWorkspace(selectedAccountId, folder, mailFilters, mailPage);
                await loadAutomation();
              })
            }
            onRetryQueueItem={(retryId) =>
              runAction(async () => {
                const result = await api.retryQueueItem(retryId);
                setNotice(formatResultMessage(result.message));
                await loadWorkspace(selectedAccountId, folder, mailFilters, mailPage);
                await loadAutomation();
              })
            }
            onDismissRetryItem={(retryId) =>
              runAction(async () => {
                const result = await api.dismissRetryItem(retryId);
                setNotice(formatResultMessage(result.message));
                await loadAutomation();
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
              }, "临时邮箱已生成")
            }
            onGenerateCloudflareBatch={(input) =>
              runAction(async () => {
                const result = await api.generateCloudflareTempEmails(input);
                await loadTempWorkspace();
                setNotice(`已生成 ${result.imported} 个 Cloudflare 地址，跳过 ${result.skipped} 个`);
              })
            }
            onImport={async (input) => {
              setBusy(true);
              setError(null);
              setNotice(null);
              try {
                const result = await api.importTempEmails(input);
                await loadTempWorkspace();
                setNotice(`已导入 ${result.imported} 个，跳过 ${result.skipped} 个`);
                return result;
              } catch (err) {
                setError(readError(err));
                throw err;
              } finally {
                setBusy(false);
              }
            }}
            onRefresh={(email) =>
              runAction(async () => {
                const result = await api.refreshTempEmailMessages(email);
                setNotice(formatResultMessage(result.message));
                await loadTempWorkspace(email);
              })
            }
            onUpdate={(input) =>
              runAction(async () => {
                await api.updateTempEmail(input);
                await loadTempWorkspace(input.email);
              }, "临时邮箱已保存")
            }
            onDelete={(email) =>
              runAction(async () => {
                await api.deleteTempEmail(email);
                await loadTempWorkspace(undefined);
              }, "临时邮箱已删除")
            }
            onSaveChannel={(input) =>
              runAction(async () => {
                await api.upsertCloudflareChannel(input);
                await loadTempWorkspace(selectedTempEmail);
              }, "Cloudflare 通道已保存")
            }
            onDeleteChannel={(channelId) =>
              runAction(async () => {
                await api.deleteCloudflareChannel(channelId);
                await loadTempWorkspace(selectedTempEmail);
              }, "Cloudflare 通道已删除")
            }
            onTestChannel={(channelId) =>
              runAction(async () => {
                const result = await api.testCloudflareChannel(channelId);
                setNotice(formatResultMessage(result.message));
              })
            }
          />
        )}

        {view === "projects" && (
          <ProjectsView
            projects={projects}
            accounts={projectAccounts}
            groups={groups}
            tags={tags}
            busy={busy}
            onCreate={(input) =>
              runAction(async () => {
                const project = await api.createProject(input);
                await loadProjects(project.id);
              }, "项目已创建")
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
              }, "项目已同步")
            }
            onClaim={(projectId) =>
              runAction(async () => {
                const claimed = await api.claimProjectAccount({ project_id: projectId, lease_minutes: 30 });
                await loadProjects(projectId);
                setNotice(claimed ? `已领取 ${claimed.email}` : "没有可领取账号");
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
              }, "项目账号已更新")
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
            retryQueue={retryQueue}
            localRetention={localRetention}
            schedulerStatus={schedulerStatus}
            busy={busy}
            onSave={(nextSettings) =>
              runAction(async () => {
                setSettings(await api.updateSettings(nextSettings));
                await loadAutomation();
              }, "设置已保存")
            }
            onRunForwarding={() =>
              runAction(async () => {
                const result = await api.runForwardingJob({ limit: 50 });
                setNotice(formatResultMessage(result.message));
                await loadAutomation();
              })
            }
            onRunBackup={() =>
              runAction(async () => {
                const result = await api.runBackupJob();
                setNotice(formatResultMessage(result.message));
                await loadAutomation();
              })
            }
            onRestoreBackup={(backupLogId) =>
              runAction(async () => {
                const result = await api.restoreBackup(backupLogId);
                const restoredFilters: MailFilters = {
                  search: "",
                  readState: "all",
                  attachmentFilter: "all",
                  sortBy: "date",
                  sortOrder: "desc"
                };
                setFolder("all");
                setMailFilters(restoredFilters);
                setMailPage(0);
                setNotice(`${formatResultMessage(result.message)}。安全快照：${result.safety_backup_path}`);
                await loadStatus();
                await loadWorkspace(null, "all", restoredFilters, 0);
                await loadProjects();
                await loadAutomation();
                await loadTempWorkspace(null);
              })
            }
            onFilterAutomationRuns={(query) =>
              runAction(async () => {
                setAutomationRuns(await api.listAutomationRuns(query, 80));
              })
            }
            onClearAutomationRuns={(query) =>
              runAction(async () => {
                const result = await api.clearAutomationRuns(query);
                setNotice(formatResultMessage(result.message));
                setAutomationRuns(await api.listAutomationRuns(query, 80));
              })
            }
            onClearLocalData={(input) =>
              runAction(async () => {
                const result = await api.clearLocalData(input);
                setNotice(formatResultMessage(result.message));
                await loadStatus();
                await loadWorkspace(selectedAccountId, folder, mailFilters, mailPage);
                await loadTempWorkspace(selectedTempEmail);
                await loadAutomation();
              })
            }
            onRunRetryQueue={() =>
              runAction(async () => {
                const result = await api.runRetryQueue(20);
                setNotice(formatResultMessage(result.message));
                await loadAutomation();
              })
            }
            onRetryQueueItem={(retryId) =>
              runAction(async () => {
                const result = await api.retryQueueItem(retryId);
                setNotice(formatResultMessage(result.message));
                await loadAutomation();
              })
            }
            onDismissRetryItem={(retryId) =>
              runAction(async () => {
                const result = await api.dismissRetryItem(retryId);
                setNotice(formatResultMessage(result.message));
                setRetryQueue(await api.listRetryQueue({}, 80));
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
        <h1>{initialized ? "解锁工作区" : "创建本地工作区"}</h1>
        <p>
          {initialized
            ? "输入本地应用密码，用于解密敏感设置并打开邮箱工作区。"
            : "设置至少 8 位本地密码，用于保护 SQLite 中加密保存的邮箱凭据。"}
        </p>
        <input
          className="input"
          type="password"
          minLength={8}
          value={password}
          placeholder="本地应用密码"
          onChange={(event) => setPassword(event.target.value)}
        />
        {error && <div className="formError">{error}</div>}
        <button className="button primary fullWidth" disabled={busy || password.length < 8}>
          {busy ? <Loader2 className="spin" size={16} /> : <Lock size={16} />}
          {initialized ? "解锁" : "创建工作区"}
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
  busy,
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
  onDownloadAttachment,
  onDownloadAllAttachments,
  onViewRawMessage,
  onRetryRemoteFailure,
  onDismissRemoteFailure
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
  busy: boolean;
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
  onDownloadAttachment: (message: MailMessage, attachmentId: string) => void | Promise<void>;
  onDownloadAllAttachments: (message: MailMessage) => Promise<void>;
  onViewRawMessage: (message: MailMessage) => Promise<MailRawContent>;
  onRetryRemoteFailure: (retryId: number) => void;
  onDismissRemoteFailure: (retryId: number) => void;
}) {
  const [draftFilters, setDraftFilters] = useState(filters);
  const [downloadingAttachmentId, setDownloadingAttachmentId] = useState<string | null>(null);
  const [downloadingAllAttachments, setDownloadingAllAttachments] = useState(false);
  const [attachmentError, setAttachmentError] = useState<string | null>(null);
  const [rawContent, setRawContent] = useState<MailRawContent | null>(null);
  const [rawBusy, setRawBusy] = useState(false);
  const [rawError, setRawError] = useState<string | null>(null);
  const selectedCount = selectedMessageIds.length;

  useEffect(() => {
    setDraftFilters(filters);
  }, [filters]);

  useEffect(() => {
    setAttachmentError(null);
    setRawContent(null);
    setRawError(null);
    setDownloadingAttachmentId(null);
    setDownloadingAllAttachments(false);
  }, [selectedMessage?.id]);

  async function handleDownloadAttachment(message: MailMessage, attachmentId: string) {
    setAttachmentError(null);
    setDownloadingAttachmentId(attachmentId);
    try {
      await onDownloadAttachment(message, attachmentId);
    } catch (err) {
      setAttachmentError(readError(err));
    } finally {
      setDownloadingAttachmentId(null);
    }
  }

  async function handleDownloadAllAttachments(message: MailMessage) {
    setAttachmentError(null);
    setDownloadingAllAttachments(true);
    try {
      await onDownloadAllAttachments(message);
    } catch (err) {
      setAttachmentError(readError(err));
    } finally {
      setDownloadingAllAttachments(false);
    }
  }

  async function handleViewRawMessage(message: MailMessage) {
    setRawError(null);
    setRawBusy(true);
    try {
      setRawContent(await onViewRawMessage(message));
    } catch (err) {
      setRawContent(null);
      setRawError(readError(err));
    } finally {
      setRawBusy(false);
    }
  }

  return (
    <section className="workspaceGrid">
      <aside className="pane groupPane">
        <div className="paneHeader">
          <h2>分组</h2>
        </div>
        <button className={selectedGroupId === "all" ? "listRow active" : "listRow"} onClick={() => onGroupChange("all")}>
          <Archive size={16} />
          <span>全部账号</span>
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
          <h2>账号</h2>
          <button className="iconMini" title="创建本地测试邮件" onClick={onCreateDemo} disabled={!selectedAccountId}>
            <Plus size={16} />
          </button>
        </div>
        <div className="searchBox">
          <Search size={15} />
          <span>账号搜索即将支持</span>
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
              <small>{formatStatus(account.last_refresh_status)} · {account.message_count} 封邮件</small>
            </span>
          </button>
        ))}
        {accounts.length === 0 && <EmptyState icon={<Mail size={24} />} text="导入账号后开始使用。" />}
      </aside>

      <section className="pane messagePane">
        <div className="paneHeader">
          <h2>邮件</h2>
          <select className="select" value={folder} onChange={(event) => onFolderChange(event.target.value)}>
            <option value="all">全部</option>
            <option value="inbox">收件箱</option>
            <option value="junkemail">垃圾邮件</option>
            <option value="deleteditems">已删除</option>
          </select>
        </div>
        <div className="messageTools">
          <label className="searchBox messageSearch">
            <Search size={15} />
            <input
              value={draftFilters.search}
              placeholder="搜索发件人、主题、正文"
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
              <option value="all">全部邮件</option>
              <option value="unread">未读</option>
              <option value="read">已读</option>
            </select>
            <select
              className="select"
              value={draftFilters.attachmentFilter}
              onChange={(event) => setDraftFilters({ ...draftFilters, attachmentFilter: event.target.value as MailFilters["attachmentFilter"] })}
            >
              <option value="all">全部附件状态</option>
              <option value="attachments">有附件</option>
              <option value="plain">无附件</option>
            </select>
            <select
              className="select"
              value={draftFilters.sortBy}
              onChange={(event) => setDraftFilters({ ...draftFilters, sortBy: event.target.value as MailFilters["sortBy"] })}
            >
              <option value="date">日期</option>
              <option value="subject">主题</option>
              <option value="sender">发件人</option>
              <option value="read">状态</option>
              <option value="attachments">附件</option>
              <option value="folder">文件夹</option>
            </select>
            <select
              className="select"
              value={draftFilters.sortOrder}
              onChange={(event) => setDraftFilters({ ...draftFilters, sortOrder: event.target.value as MailFilters["sortOrder"] })}
            >
              <option value="desc">降序</option>
              <option value="asc">升序</option>
            </select>
            <button className="button compact secondary" onClick={() => onFilterApply(draftFilters)}>
              <Search size={14} />
              应用
            </button>
          </div>
        </div>
        {selectedCount > 0 && (
          <div className="bulkBar">
            <span>已选择 {selectedCount} 封</span>
            <button className="button compact secondary" onClick={() => onMarkMessages(selectedMessageIds, true)}>
              <CheckCircle2 size={14} />
              标为已读
            </button>
            <button className="button compact secondary" onClick={() => onMarkMessages(selectedMessageIds, false)}>
              <Mail size={14} />
              标为未读
            </button>
            <button className="button compact danger" onClick={() => onDeleteMessages(selectedMessageIds)}>
              <Trash2 size={14} />
              删除
            </button>
            <button className="button compact secondary" onClick={() => onExportMessages(selectedMessageIds)}>
              <Download size={14} />
              导出
            </button>
            <button className="button compact ghost" onClick={onClearSelection}>
              清除
            </button>
          </div>
        )}
        {messages.map((message) => (
          <div key={message.id} className={selectedMessage?.id === message.id ? "messageRow active" : "messageRow"}>
            <input
              type="checkbox"
              aria-label={`选择 ${message.subject || "邮件"}`}
              checked={selectedMessageIds.includes(message.id)}
              onChange={() => onToggleMessageSelect(message.id)}
            />
            <button className={message.is_read ? "messageOpen" : "messageOpen unread"} onClick={() => onMessageSelect(message.id)}>
              <span className="messageTop">
                <strong>{message.subject || "（无主题）"}</strong>
                <small>{formatDate(message.received_at)}</small>
              </span>
              <span className="sender">{message.sender}</span>
              {message.remote_sync_failure && (
                <span className="remoteFailureInline">
                  <XCircle size={12} />
                  {formatRemoteFailureAction(message.remote_sync_failure.action)} 远端同步失败
                </span>
              )}
              <span className="preview">{message.body_preview}</span>
            </button>
          </div>
        ))}
        {messages.length === 0 && <EmptyState icon={<Inbox size={24} />} text="暂无缓存邮件。" />}
        {messages.length > 0 && (
          <div className="pagerBar">
            <button className="button compact secondary" onClick={onSelectVisibleMessages}>
              选择本页
            </button>
            <span>第 {page + 1} 页</span>
            <button className="button compact secondary" disabled={page === 0} onClick={() => onPageChange(page - 1)}>
              上一页
            </button>
            <button className="button compact secondary" disabled={!hasNextPage} onClick={() => onPageChange(page + 1)}>
              下一页
            </button>
          </div>
        )}
      </section>

      <article className="pane detailPane">
        {selectedMessage ? (
          <>
            <div className="detailHeader">
              <div>
                <h2>{selectedMessage.subject || "（无主题）"}</h2>
                <p>{selectedMessage.sender}</p>
              </div>
              <div className="detailActions">
                <button className="button compact secondary" onClick={() => onMarkMessages([selectedMessage.id], !selectedMessage.is_read)}>
                  {selectedMessage.is_read ? <Mail size={14} /> : <CheckCircle2 size={14} />}
                  {selectedMessage.is_read ? "标为未读" : "标为已读"}
                </button>
                <button className="button compact danger" onClick={() => onDeleteMessages([selectedMessage.id])}>
                  <Trash2 size={14} />
                  删除
                </button>
                <button className="button compact secondary" disabled={rawBusy} onClick={() => handleViewRawMessage(selectedMessage)}>
                  {rawBusy ? <Loader2 className="spin" size={14} /> : <FileText size={14} />}
                  Raw
                </button>
                <button className="button compact secondary" onClick={() => onExportMessages([selectedMessage.id])}>
                  <Download size={14} />
                  导出
                </button>
              </div>
            </div>
            <div className="metaGrid">
              <span>文件夹</span>
              <strong>{selectedMessage.folder}</strong>
              <span>状态</span>
              <strong>{selectedMessage.is_read ? "已读" : "未读"}</strong>
              <span>接收时间</span>
              <strong>{formatDate(selectedMessage.received_at)}</strong>
            </div>
            {selectedMessage.remote_sync_failure && (
              <RemoteFailurePanel
                failure={selectedMessage.remote_sync_failure}
                busy={busy}
                onRetry={onRetryRemoteFailure}
                onDismiss={onDismissRemoteFailure}
              />
            )}
            <MessageBody body={selectedMessage.body || selectedMessage.body_preview} bodyType={selectedMessage.body_type} />
            {rawError && <div className="inlineError">{rawError}</div>}
            {rawContent && (
              <div className="rawSourcePanel">
                <div className="rawSourceHeader">
                  <div>
                    <strong>{rawContent.file_name}</strong>
                    <small>{formatBytes(rawContent.size)}</small>
                  </div>
                  <button className="iconMini" title="Close raw source" onClick={() => setRawContent(null)}>
                    <XCircle size={15} />
                  </button>
                </div>
                <pre>{rawContent.content}</pre>
              </div>
            )}
            {selectedMessage.attachments.length > 0 && (
              <div className="attachmentList">
                <h3>附件</h3>
                <button
                  className="button compact secondary attachmentDownloadAll"
                  disabled={downloadingAllAttachments || downloadingAttachmentId !== null}
                  onClick={() => handleDownloadAllAttachments(selectedMessage)}
                >
                  {downloadingAllAttachments ? <Loader2 className="spin" size={14} /> : <Download size={14} />}
                  All
                </button>
                {attachmentError && <div className="inlineError">{attachmentError}</div>}
                {selectedMessage.attachments.map((attachment) => (
                  <button
                    className="attachmentButton"
                    key={attachment.id}
                    disabled={downloadingAllAttachments || downloadingAttachmentId !== null}
                    onClick={() => handleDownloadAttachment(selectedMessage, attachment.id)}
                  >
                    {downloadingAttachmentId === attachment.id ? <Loader2 className="spin" size={15} /> : <Download size={15} />}
                    <span>{attachment.name}</span>
                    <small>{downloadingAttachmentId === attachment.id ? "Downloading" : formatBytes(attachment.size)}</small>
                  </button>
                ))}
              </div>
            )}
          </>
        ) : (
          <EmptyState icon={<Mail size={24} />} text="请选择一封邮件。" />
        )}
      </article>
    </section>
  );
}

type GroupDraft = {
  name: string;
  description: string;
  color: string;
  parent_id: number | "";
  sort_order: string;
  proxy_url: string;
  fallback_proxy_url_1: string;
  fallback_proxy_url_2: string;
};

function groupOptionLabel(group: Group): string {
  return `${"  ".repeat(Math.max(0, group.level - 1))}${group.name}`;
}

function collectDescendantGroupIds(groups: Group[], groupId: number): Set<number> {
  const descendants = new Set<number>();
  const visit = (parentId: number) => {
    groups
      .filter((group) => group.parent_id === parentId)
      .forEach((group) => {
        descendants.add(group.id);
        visit(group.id);
      });
  };
  visit(groupId);
  return descendants;
}

function groupSubtreeDepth(groups: Group[], groupId: number): number {
  const children = groups.filter((group) => group.parent_id === groupId);
  if (children.length === 0) return 0;
  return 1 + Math.max(...children.map((group) => groupSubtreeDepth(groups, group.id)));
}

function AccountsView({
  groups,
  tags,
  accounts,
  settings,
  busy,
  onImport,
  onCreateGroup,
  onUpdateGroup,
  onDeleteGroup,
  onCreateTag,
  onDeleteAccount,
  onBatchAccounts,
  onExportAccounts,
  onUpdateAccount,
  onRevealAccountSecrets,
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
  onUpdateGroup: (input: Parameters<typeof api.updateGroup>[0]) => void;
  onDeleteGroup: (groupId: number) => void;
  onCreateTag: (name: string, color: string) => void;
  onDeleteAccount: (accountId: number) => void;
  onBatchAccounts: (input: Parameters<typeof api.batchAccounts>[0]) => void;
  onExportAccounts: (groupId?: number | null, accountIds?: number[]) => void;
  onUpdateAccount: (input: Parameters<typeof api.updateAccount>[0]) => void;
  onRevealAccountSecrets: (input: Parameters<typeof api.revealAccountSecrets>[0]) => Promise<Awaited<ReturnType<typeof api.revealAccountSecrets>>>;
  onGenerateOAuthUrl: (input: { client_id: string; redirect_uri: string; login_hint?: string; provider?: string }) => Promise<string>;
  onExchangeOAuthToken: (input: { account_id?: number; client_id: string; redirect_uri: string; code_or_url: string; provider?: string }) => void;
}) {
  const [raw, setRaw] = useState("");
  const [groupId, setGroupId] = useState<number | null>(groups[0]?.id ?? null);
  const [groupName, setGroupName] = useState("");
  const [selectedManageGroupId, setSelectedManageGroupId] = useState<number | undefined>(groups[0]?.id);
  const [groupDraft, setGroupDraft] = useState<GroupDraft>({
    name: "",
    description: "",
    color: colors[0],
    parent_id: "",
    sort_order: "0",
    proxy_url: "",
    fallback_proxy_url_1: "",
    fallback_proxy_url_2: ""
  });
  const [tagName, setTagName] = useState("");
  const [colorIndex, setColorIndex] = useState(0);
  const [selectedAccountId, setSelectedAccountId] = useState<number | undefined>(accounts[0]?.id);
  const [accountSearch, setAccountSearch] = useState("");
  const [selectedAccountIds, setSelectedAccountIds] = useState<number[]>([]);
  const [batchGroupId, setBatchGroupId] = useState<number | "">(groups[0]?.id ?? "");
  const [batchTagId, setBatchTagId] = useState<number | "">(tags[0]?.id ?? "");
  const [oauthUrl, setOauthUrl] = useState("");
  const [oauthCallback, setOauthCallback] = useState("");
  const visibleAccounts = useMemo(() => {
    const keyword = accountSearch.trim().toLowerCase();
    if (!keyword) return accounts;
    return accounts.filter((account) => {
      const haystack = [
        account.email,
        account.remark,
        account.group_name ?? "",
        ...account.aliases,
        ...account.tags.map((tag) => tag.name)
      ]
        .join(" ")
        .toLowerCase();
      return haystack.includes(keyword);
    });
  }, [accounts, accountSearch]);
  const selectedAccount =
    visibleAccounts.find((account) => account.id === selectedAccountId) ?? visibleAccounts[0] ?? accounts[0];
  const selectedAccountIdSet = useMemo(() => new Set(selectedAccountIds), [selectedAccountIds]);
  const visibleAccountIds = useMemo(() => visibleAccounts.map((account) => account.id), [visibleAccounts]);
  const allVisibleAccountsSelected =
    visibleAccountIds.length > 0 && visibleAccountIds.every((accountId) => selectedAccountIdSet.has(accountId));
  const selectedAccountCount = selectedAccountIds.length;
  const parsedRows = useMemo(() => parseAccountRows(raw), [raw]);
  const selectedManageGroup = groups.find((group) => group.id === selectedManageGroupId) ?? groups[0];
  const selectedDescendantGroupIds = useMemo(
    () => (selectedManageGroup ? collectDescendantGroupIds(groups, selectedManageGroup.id) : new Set<number>()),
    [groups, selectedManageGroup?.id]
  );
  const selectedSubtreeDepth = useMemo(
    () => (selectedManageGroup ? groupSubtreeDepth(groups, selectedManageGroup.id) : 0),
    [groups, selectedManageGroup?.id]
  );
  const parentGroupOptions = useMemo(() => {
    if (!selectedManageGroup) return [];
    return groups.filter(
      (group) =>
        group.id !== selectedManageGroup.id &&
        !selectedDescendantGroupIds.has(group.id) &&
        group.level + 1 + selectedSubtreeDepth <= 3
    );
  }, [groups, selectedDescendantGroupIds, selectedManageGroup?.id, selectedSubtreeDepth]);

  useEffect(() => {
    if (groups.length === 0) {
      setSelectedManageGroupId(undefined);
      return;
    }
    if (!selectedManageGroupId || !groups.some((group) => group.id === selectedManageGroupId)) {
      setSelectedManageGroupId(groups[0].id);
    }
  }, [groups, selectedManageGroupId]);

  useEffect(() => {
    if (!selectedManageGroup) return;
    setGroupDraft({
      name: selectedManageGroup.name,
      description: selectedManageGroup.description,
      color: selectedManageGroup.color || colors[0],
      parent_id: selectedManageGroup.parent_id ?? "",
      sort_order: String(selectedManageGroup.sort_order),
      proxy_url: selectedManageGroup.proxy_url,
      fallback_proxy_url_1: selectedManageGroup.fallback_proxy_url_1,
      fallback_proxy_url_2: selectedManageGroup.fallback_proxy_url_2
    });
  }, [
    selectedManageGroup?.id,
    selectedManageGroup?.name,
    selectedManageGroup?.description,
    selectedManageGroup?.color,
    selectedManageGroup?.parent_id,
    selectedManageGroup?.sort_order,
    selectedManageGroup?.proxy_url,
    selectedManageGroup?.fallback_proxy_url_1,
    selectedManageGroup?.fallback_proxy_url_2
  ]);

  useEffect(() => {
    const accountIds = new Set(accounts.map((account) => account.id));
    setSelectedAccountIds((current) => current.filter((accountId) => accountIds.has(accountId)));
  }, [accounts]);

  useEffect(() => {
    if (batchGroupId !== "" && !groups.some((group) => group.id === batchGroupId)) {
      setBatchGroupId(groups[0]?.id ?? "");
    }
  }, [groups, batchGroupId]);

  useEffect(() => {
    if (batchTagId !== "" && !tags.some((tag) => tag.id === batchTagId)) {
      setBatchTagId(tags[0]?.id ?? "");
    }
  }, [tags, batchTagId]);

  return (
    <section className="managementGrid">
      <div className="panel">
        <div className="panelHeader">
          <h2>导入账号</h2>
          <Upload size={18} />
        </div>
        <textarea
          className="textarea"
          value={raw}
          onChange={(event) => setRaw(event.target.value)}
          placeholder="邮箱----密码----client_id----refresh_token----备注"
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
            导入 {parsedRows.length || ""}
          </button>
        </div>
      </div>

      <div className="panel">
        <div className="panelHeader">
          <h2>分组和标签</h2>
          <Tags size={18} />
        </div>
        <div className="formLine">
          <input className="input grow" value={groupName} placeholder="新分组" onChange={(event) => setGroupName(event.target.value)} />
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
            分组
          </button>
        </div>
        {selectedManageGroup && (
          <div className="groupManageGrid">
            <div className="groupTree" aria-label="分组列表">
              {groups.map((group) => (
                <button
                  className={selectedManageGroup.id === group.id ? "groupTreeButton active" : "groupTreeButton"}
                  key={group.id}
                  onClick={() => setSelectedManageGroupId(group.id)}
                  style={{ paddingLeft: 12 + Math.max(0, group.level - 1) * 14 }}
                >
                  <span className="dot" style={{ backgroundColor: group.color }} />
                  <span>{group.name}</span>
                  <small>{group.account_count}</small>
                </button>
              ))}
            </div>
            <div className="groupEditor">
              <div className="formLine">
                <input
                  className="input grow"
                  value={groupDraft.name}
                  placeholder="分组名称"
                  onChange={(event) => setGroupDraft({ ...groupDraft, name: event.target.value })}
                />
                <input
                  className="input smallInput"
                  type="number"
                  value={groupDraft.sort_order}
                  title="排序"
                  onChange={(event) => setGroupDraft({ ...groupDraft, sort_order: event.target.value })}
                />
              </div>
              <div className="formLine">
                <select
                  className="select grow"
                  value={groupDraft.parent_id}
                  onChange={(event) =>
                    setGroupDraft({
                      ...groupDraft,
                      parent_id: event.target.value ? Number(event.target.value) : ""
                    })
                  }
                >
                  <option value="">顶级分组</option>
                  {parentGroupOptions.map((group) => (
                    <option value={group.id} key={group.id}>
                      {groupOptionLabel(group)}
                    </option>
                  ))}
                </select>
                <div className="colorSwatches" aria-label="分组颜色">
                  {colors.map((color) => (
                    <button
                      className={groupDraft.color === color ? "colorSwatch active" : "colorSwatch"}
                      key={color}
                      title={color}
                      style={{ backgroundColor: color }}
                      onClick={() => setGroupDraft({ ...groupDraft, color })}
                    />
                  ))}
                  <input
                    className="colorInput"
                    type="color"
                    aria-label="自定义分组颜色"
                    value={groupDraft.color}
                    onChange={(event) => setGroupDraft({ ...groupDraft, color: event.target.value })}
                  />
                </div>
              </div>
              <input
                className="input"
                value={groupDraft.description}
                placeholder="分组说明"
                onChange={(event) => setGroupDraft({ ...groupDraft, description: event.target.value })}
              />
              <input
                className="input"
                value={groupDraft.proxy_url}
                placeholder="分组主代理：http://127.0.0.1:7890"
                onChange={(event) => setGroupDraft({ ...groupDraft, proxy_url: event.target.value })}
              />
              <div className="formLine">
                <input
                  className="input grow"
                  value={groupDraft.fallback_proxy_url_1}
                  placeholder="备用代理 1"
                  onChange={(event) => setGroupDraft({ ...groupDraft, fallback_proxy_url_1: event.target.value })}
                />
                <input
                  className="input grow"
                  value={groupDraft.fallback_proxy_url_2}
                  placeholder="备用代理 2"
                  onChange={(event) => setGroupDraft({ ...groupDraft, fallback_proxy_url_2: event.target.value })}
                />
              </div>
              <div className="formLine">
                <button
                  className="button primary grow"
                  disabled={busy || !groupDraft.name.trim()}
                  onClick={() => {
                    const sortOrder = Number.parseInt(groupDraft.sort_order, 10);
                    onUpdateGroup({
                      id: selectedManageGroup.id,
                      name: groupDraft.name,
                      description: groupDraft.description,
                      color: groupDraft.color,
                      parent_id: groupDraft.parent_id === "" ? null : groupDraft.parent_id,
                      sort_order: Number.isFinite(sortOrder) ? sortOrder : selectedManageGroup.sort_order,
                      proxy_url: groupDraft.proxy_url,
                      fallback_proxy_url_1: groupDraft.fallback_proxy_url_1,
                      fallback_proxy_url_2: groupDraft.fallback_proxy_url_2
                    });
                  }}
                >
                  <CheckCircle2 size={16} />
                  保存分组
                </button>
                <button
                  className="button danger"
                  disabled={busy || selectedManageGroup.id === 1}
                  onClick={() => onDeleteGroup(selectedManageGroup.id)}
                >
                  <Trash2 size={16} />
                  删除
                </button>
              </div>
            </div>
          </div>
        )}
        <div className="formLine">
          <input className="input grow" value={tagName} placeholder="新标签" onChange={(event) => setTagName(event.target.value)} />
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
            标签
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
        tags={tags}
        settings={settings}
        busy={busy}
        oauthUrl={oauthUrl}
        oauthCallback={oauthCallback}
        onOauthCallbackChange={setOauthCallback}
        onSave={(input) => onUpdateAccount(input)}
        onRevealAccountSecrets={(input) => onRevealAccountSecrets(input)}
        onGenerateOAuthUrl={async (input) => {
          const url = await onGenerateOAuthUrl(input);
          setOauthUrl(url);
        }}
        onExchangeOAuthToken={(input) => onExchangeOAuthToken(input)}
      />

      <div className="panel widePanel">
        <div className="panelHeader">
          <h2>邮箱库存</h2>
          <div className="rowActions">
            <span>{visibleAccounts.length}/{accounts.length} 个账号</span>
            <button className="iconMini" title="导出全部账号" disabled={accounts.length === 0 || busy} onClick={() => onExportAccounts()}>
              <Download size={15} />
            </button>
          </div>
        </div>
        <label className="searchBox">
          <Search size={15} />
          <input
            value={accountSearch}
            placeholder="搜索邮箱、别名、备注、分组或标签"
            onChange={(event) => setAccountSearch(event.target.value)}
          />
        </label>
        {selectedAccountCount > 0 && (
          <div className="bulkBar accountBulkBar">
            <span>已选择 {selectedAccountCount} 个账号</span>
            <select
              className="select"
              value={batchGroupId}
              onChange={(event) => setBatchGroupId(event.target.value ? Number(event.target.value) : "")}
            >
              <option value="">无分组</option>
              {groups.map((group) => (
                <option value={group.id} key={group.id}>
                  {groupOptionLabel(group)}
                </option>
              ))}
            </select>
            <button
              className="button compact secondary"
              disabled={busy}
              onClick={() =>
                onBatchAccounts({
                  account_ids: selectedAccountIds,
                  action: "move_group",
                  group_id: batchGroupId === "" ? null : batchGroupId
                })
              }
            >
              <FolderKanban size={14} />
              移动
            </button>
            <button
              className="button compact secondary"
              disabled={busy}
              onClick={() => onBatchAccounts({ account_ids: selectedAccountIds, action: "set_forward", forward_enabled: true })}
            >
              <CheckCircle2 size={14} />
              转发开
            </button>
            <button
              className="button compact secondary"
              disabled={busy}
              onClick={() => onBatchAccounts({ account_ids: selectedAccountIds, action: "set_forward", forward_enabled: false })}
            >
              <XCircle size={14} />
              转发关
            </button>
            <select
              className="select"
              value={batchTagId}
              onChange={(event) => setBatchTagId(event.target.value ? Number(event.target.value) : "")}
            >
              <option value="">选择标签</option>
              {tags.map((tag) => (
                <option value={tag.id} key={tag.id}>
                  {tag.name}
                </option>
              ))}
            </select>
            <button
              className="button compact secondary"
              disabled={busy || batchTagId === ""}
              onClick={() => onBatchAccounts({ account_ids: selectedAccountIds, action: "add_tags", tag_ids: [Number(batchTagId)] })}
            >
              <Plus size={14} />
              加标签
            </button>
            <button
              className="button compact secondary"
              disabled={busy || batchTagId === ""}
              onClick={() => onBatchAccounts({ account_ids: selectedAccountIds, action: "remove_tags", tag_ids: [Number(batchTagId)] })}
            >
              <XCircle size={14} />
              移标签
            </button>
            <button className="button compact secondary" disabled={busy} onClick={() => onExportAccounts(null, selectedAccountIds)}>
              <Download size={14} />
              导出
            </button>
            <button
              className="button compact danger"
              disabled={busy}
              onClick={() => onBatchAccounts({ account_ids: selectedAccountIds, action: "delete" })}
            >
              <Trash2 size={14} />
              删除
            </button>
            <button className="button compact ghost" onClick={() => setSelectedAccountIds([])}>
              清除
            </button>
          </div>
        )}
        <div className="table">
          <div className="tableHeader">
            <span className="selectCell">
              <input
                type="checkbox"
                aria-label="选择当前列表账号"
                checked={allVisibleAccountsSelected}
                disabled={visibleAccountIds.length === 0}
                onChange={(event) => {
                  const visibleIdSet = new Set(visibleAccountIds);
                  setSelectedAccountIds((current) =>
                    event.target.checked
                      ? Array.from(new Set([...current, ...visibleAccountIds]))
                      : current.filter((accountId) => !visibleIdSet.has(accountId))
                  );
                }}
              />
            </span>
            <span>邮箱</span>
            <span>分组</span>
            <span>状态</span>
            <span>凭据</span>
            <span />
          </div>
          {visibleAccounts.map((account) => (
            <div
              className={selectedAccount?.id === account.id ? "tableRow active" : "tableRow"}
              key={account.id}
              onClick={() => setSelectedAccountId(account.id)}
            >
              <span className="selectCell">
                <input
                  type="checkbox"
                  aria-label={`选择 ${account.email}`}
                  checked={selectedAccountIdSet.has(account.id)}
                  onClick={(event) => event.stopPropagation()}
                  onChange={(event) =>
                    setSelectedAccountIds((current) =>
                      event.target.checked
                        ? Array.from(new Set([...current, account.id]))
                        : current.filter((accountId) => accountId !== account.id)
                    )
                  }
                />
              </span>
              <span className="accountText">
                <strong>{account.email}</strong>
                {account.aliases.length > 0 && <small>{account.aliases.join(", ")}</small>}
              </span>
              <span>{account.group_name ?? "无"}</span>
              <span>{formatStatus(account.last_refresh_status)}</span>
              <span>{account.has_refresh_token ? "Graph" : account.has_imap_password || account.has_password ? "IMAP" : "无"}</span>
              <button
                className="iconMini danger"
                title="删除账号"
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

function RefreshManagementView({
  accounts,
  retryQueue,
  refreshLogs,
  automationRuns,
  schedulerStatus,
  busy,
  onRefreshAccount,
  onRefreshAll,
  onRunRetryQueue,
  onRetryQueueItem,
  onDismissRetryItem
}: {
  accounts: Account[];
  retryQueue: RetryQueueItem[];
  refreshLogs: RefreshLog[];
  automationRuns: AutomationRun[];
  schedulerStatus: SchedulerStatus | null;
  busy: boolean;
  onRefreshAccount: (accountId: number) => Promise<void> | void;
  onRefreshAll: () => void;
  onRunRetryQueue: () => void;
  onRetryQueueItem: (retryId: number) => void;
  onDismissRetryItem: (retryId: number) => void;
}) {
  const [accountFilter, setAccountFilter] = useState("failed");
  const [historyFilter, setHistoryFilter] = useState("all");
  const [accountSearch, setAccountSearch] = useState("");
  const [selectedAccountIds, setSelectedAccountIds] = useState<number[]>([]);
  const [batchRunning, setBatchRunning] = useState(false);
  const stopBatchRef = useRef(false);

  const refreshRetryQueue = useMemo(() => retryQueue.filter((item) => item.task_type === "refresh_account"), [retryQueue]);
  const refreshRuns = useMemo(() => automationRuns.filter((run) => run.job_type === "refresh"), [automationRuns]);
  const readyCount = accounts.filter(isRefreshReady).length;
  const failedCount = accounts.filter((account) => account.last_refresh_status === "failed").length;
  const successCount = accounts.filter((account) => account.last_refresh_status === "success").length;
  const neverCount = accounts.filter((account) => account.last_refresh_status === "never").length;
  const selectedSet = useMemo(() => new Set(selectedAccountIds), [selectedAccountIds]);
  const visibleAccounts = useMemo(() => {
    const keyword = accountSearch.trim().toLowerCase();
    return accounts.filter((account) => {
      if (accountFilter === "failed" && account.last_refresh_status !== "failed") return false;
      if (accountFilter === "success" && account.last_refresh_status !== "success") return false;
      if (accountFilter === "never" && account.last_refresh_status !== "never") return false;
      if (accountFilter === "ready" && !isRefreshReady(account)) return false;
      if (accountFilter === "missing" && isRefreshReady(account)) return false;
      if (!keyword) return true;
      return [account.email, account.group_name ?? "", account.remark, account.last_refresh_error ?? "", ...account.aliases]
        .join(" ")
        .toLowerCase()
        .includes(keyword);
    });
  }, [accounts, accountFilter, accountSearch]);
  const visibleAccountIds = useMemo(() => visibleAccounts.map((account) => account.id), [visibleAccounts]);
  const allVisibleSelected = visibleAccountIds.length > 0 && visibleAccountIds.every((accountId) => selectedSet.has(accountId));
  const filteredRefreshLogs = useMemo(
    () => refreshLogs.filter((log) => historyFilter === "all" || log.status === historyFilter),
    [refreshLogs, historyFilter]
  );

  useEffect(() => {
    const accountIds = new Set(accounts.map((account) => account.id));
    setSelectedAccountIds((current) => current.filter((accountId) => accountIds.has(accountId)));
  }, [accounts]);

  async function runSelectedRefreshBatch() {
    if (selectedAccountIds.length === 0 || batchRunning) return;
    stopBatchRef.current = false;
    setBatchRunning(true);
    try {
      const batch = [...selectedAccountIds];
      for (const accountId of batch) {
        if (stopBatchRef.current) break;
        await onRefreshAccount(accountId);
      }
    } finally {
      setBatchRunning(false);
      stopBatchRef.current = false;
    }
  }

  return (
    <section className="refreshGrid">
      <div className="panel widePanel">
        <div className="panelHeader">
          <h2>刷新管理</h2>
          <RefreshCw size={18} />
        </div>
        <div className="statStrip refreshStats">
          <Stat label="账号总数" value={accounts.length} />
          <Stat label="凭据可用" value={readyCount} />
          <Stat label="刷新成功" value={successCount} />
          <Stat label="刷新失败" value={failedCount} />
          <Stat label="从未刷新" value={neverCount} />
          <Stat label="重试队列" value={refreshRetryQueue.length} />
        </div>
        <div className="runStatusGrid">
          <RunStatus label="上次定时刷新" value={schedulerStatus?.last_refresh_at} />
        </div>
        <div className="tableActions">
          <button className="button primary" disabled={busy || batchRunning || accounts.length === 0} onClick={onRefreshAll}>
            {busy && !batchRunning ? <Loader2 className="spin" size={16} /> : <RefreshCw size={16} />}
            刷新全部
          </button>
          <button
            className="button secondary"
            disabled={busy || batchRunning || selectedAccountIds.length === 0}
            onClick={() => void runSelectedRefreshBatch()}
          >
            {batchRunning ? <Loader2 className="spin" size={16} /> : <RefreshCw size={16} />}
            刷新选中
          </button>
          <button className="button secondary" disabled={!batchRunning} onClick={() => (stopBatchRef.current = true)}>
            <XCircle size={16} />
            停止批量
          </button>
          <button className="button secondary" disabled={busy || batchRunning || refreshRetryQueue.length === 0} onClick={onRunRetryQueue}>
            <RotateCcw size={16} />
            运行到期重试
          </button>
        </div>
      </div>

      <div className="panel widePanel">
        <div className="panelHeader">
          <h2>账号刷新状态</h2>
          <Users size={18} />
        </div>
        <div className="automationFilters">
          <select className="select" value={accountFilter} onChange={(event) => setAccountFilter(event.target.value)}>
            <option value="failed">失败账号</option>
            <option value="ready">凭据可用</option>
            <option value="missing">缺少凭据</option>
            <option value="success">成功</option>
            <option value="never">从未刷新</option>
            <option value="all">全部账号</option>
          </select>
          <input
            className="input grow"
            value={accountSearch}
            placeholder="搜索邮箱、别名、分组或错误"
            onChange={(event) => setAccountSearch(event.target.value)}
          />
          <button className="button secondary" disabled={selectedAccountIds.length === 0} onClick={() => setSelectedAccountIds([])}>
            清除选择
          </button>
        </div>
        <div className="logTable refreshAccountTable">
          <div className="logHeader">
            <span className="selectCell">
              <input
                type="checkbox"
                aria-label="选择当前账号"
                checked={allVisibleSelected}
                disabled={visibleAccountIds.length === 0}
                onChange={(event) => {
                  const visibleIdSet = new Set(visibleAccountIds);
                  setSelectedAccountIds((current) =>
                    event.target.checked
                      ? Array.from(new Set([...current, ...visibleAccountIds]))
                      : current.filter((accountId) => !visibleIdSet.has(accountId))
                  );
                }}
              />
            </span>
            <span>账号</span>
            <span>凭据</span>
            <span>状态</span>
            <span>上次刷新</span>
            <span>邮件</span>
            <span>错误</span>
            <span>操作</span>
          </div>
          {visibleAccounts.map((account) => (
            <div className="logRow" key={account.id}>
              <span className="selectCell">
                <input
                  type="checkbox"
                  aria-label={`选择 ${account.email}`}
                  checked={selectedSet.has(account.id)}
                  onChange={(event) =>
                    setSelectedAccountIds((current) =>
                      event.target.checked
                        ? Array.from(new Set([...current, account.id]))
                        : current.filter((accountId) => accountId !== account.id)
                    )
                  }
                />
              </span>
              <span>{account.email}</span>
              <span>{refreshCredentialLabel(account)}</span>
              <StatusPill status={account.last_refresh_status} />
              <span>{account.last_refresh_at ? formatDate(account.last_refresh_at) : "从未"}</span>
              <span>{account.message_count}</span>
              <span>{account.last_refresh_error ?? ""}</span>
              <span className="rowActions">
                <button className="iconMini" title="刷新账号" disabled={busy || batchRunning} onClick={() => onRefreshAccount(account.id)}>
                  <RefreshCw size={14} />
                </button>
              </span>
            </div>
          ))}
        </div>
        {visibleAccounts.length === 0 && <EmptyState icon={<RefreshCw size={24} />} text="没有匹配账号。" />}
      </div>

      <div className="panel widePanel">
        <div className="panelHeader">
          <h2>刷新重试队列</h2>
          <RotateCcw size={18} />
        </div>
        <div className="logTable refreshRetryTable">
          <div className="logHeader">
            <span>时间</span>
            <span>账号</span>
            <span>次数</span>
            <span>下次</span>
            <span>错误</span>
            <span>操作</span>
          </div>
          {refreshRetryQueue.map((item) => (
            <div className="logRow" key={item.id}>
              <span>{formatDate(item.updated_at)}</span>
              <span>{item.account_email}</span>
              <span>{item.attempts} / {item.max_attempts}</span>
              <span>{item.next_attempt_at ? formatDate(item.next_attempt_at) : "-"}</span>
              <span>{item.error_message}</span>
              <span className="rowActions">
                <button className="iconMini" title="立即重试" disabled={busy || batchRunning} onClick={() => onRetryQueueItem(item.id)}>
                  <RotateCcw size={14} />
                </button>
                <button className="iconMini danger" title="忽略" disabled={busy || batchRunning} onClick={() => onDismissRetryItem(item.id)}>
                  <Trash2 size={14} />
                </button>
              </span>
            </div>
          ))}
        </div>
        {refreshRetryQueue.length === 0 && <EmptyState icon={<RotateCcw size={24} />} text="暂无刷新重试项。" />}
      </div>

      <div className="panel widePanel">
        <div className="panelHeader">
          <h2>刷新历史</h2>
          <RefreshCw size={18} />
        </div>
        <div className="automationFilters">
          <select className="select" value={historyFilter} onChange={(event) => setHistoryFilter(event.target.value)}>
            <option value="all">全部结果</option>
            <option value="success">成功</option>
            <option value="failed">失败</option>
          </select>
        </div>
        <div className="logTable refreshHistoryTable">
          <div className="logHeader">
            <span>时间</span>
            <span>账号</span>
            <span>类型</span>
            <span>状态</span>
            <span>详情</span>
          </div>
          {filteredRefreshLogs.map((log) => (
            <div className="logRow" key={log.id}>
              <span>{formatDate(log.created_at)}</span>
              <span>{log.account_email}</span>
              <span>{log.refresh_type}</span>
              <StatusPill status={log.status} />
              <span>{log.error_message ?? ""}</span>
            </div>
          ))}
        </div>
        {filteredRefreshLogs.length === 0 && <EmptyState icon={<RefreshCw size={24} />} text="暂无刷新历史。" />}
      </div>

      <div className="panel widePanel">
        <div className="panelHeader">
          <h2>刷新任务历史</h2>
          <RefreshCw size={18} />
        </div>
        <div className="logTable refreshRunTable">
          <div className="logHeader">
            <span>时间</span>
            <span>触发</span>
            <span>状态</span>
            <span>数量</span>
            <span>耗时</span>
            <span>详情</span>
          </div>
          {refreshRuns.map((run) => (
            <div className="logRow" key={run.id}>
              <span>{formatDate(run.finished_at)}</span>
              <span>{formatAutomationTrigger(run.trigger_type)}</span>
              <StatusPill status={run.status} />
              <span>{run.refreshed} 成功 / {run.failed} 失败</span>
              <span>{formatDuration(run.duration_ms)}</span>
              <span>{formatResultMessage(run.message)}</span>
            </div>
          ))}
        </div>
        {refreshRuns.length === 0 && <EmptyState icon={<RefreshCw size={24} />} text="暂无刷新任务记录。" />}
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
  onGenerateCloudflareBatch,
  onImport,
  onRefresh,
  onUpdate,
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
  onGenerateCloudflareBatch: (input: Parameters<typeof api.generateCloudflareTempEmails>[0]) => void;
  onImport: (input: { raw: string; provider: string; channel_id?: number | null }) => Promise<ImportAccountsResult>;
  onRefresh: (email: string) => void;
  onUpdate: (input: Parameters<typeof api.updateTempEmail>[0]) => void;
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
  const [tempSearch, setTempSearch] = useState("");
  const [tempProviderFilter, setTempProviderFilter] = useState("all");
  const [tempTagFilter, setTempTagFilter] = useState("all");
  const [tempTagsText, setTempTagsText] = useState("");
  const [batchCount, setBatchCount] = useState(10);
  const [batchPrefix, setBatchPrefix] = useState("cf");
  const [batchTagsText, setBatchTagsText] = useState("");
  const [importProgress, setImportProgress] = useState({ active: false, done: 0, total: 0 });
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
  const tempTags = useMemo(() => {
    const tags = new Set<string>();
    tempEmails.forEach((item) => item.tags.forEach((tag) => tags.add(tag)));
    return [...tags].sort((a, b) => a.localeCompare(b));
  }, [tempEmails]);
  const visibleTempEmails = useMemo(() => {
    const keyword = tempSearch.trim().toLowerCase();
    return tempEmails.filter((item) => {
      if (tempProviderFilter !== "all" && item.provider !== tempProviderFilter) return false;
      if (tempTagFilter !== "all" && !item.tags.some((tag) => tag.toLowerCase() === tempTagFilter.toLowerCase())) return false;
      if (!keyword) return true;
      return [item.email, item.provider, item.last_refresh_status, item.last_refresh_error ?? "", ...item.tags]
        .join(" ")
        .toLowerCase()
        .includes(keyword);
    });
  }, [tempEmails, tempProviderFilter, tempSearch, tempTagFilter]);

  useEffect(() => {
    setTempTagsText(selectedTemp?.tags.join(", ") ?? "");
  }, [selectedTemp?.email, selectedTemp?.updated_at]);

  async function runTempImport() {
    const rows = importRaw
      .split(/\r?\n/)
      .map((line) => line.trim())
      .filter(Boolean);
    if (rows.length === 0) return;
    const chunkSize = provider === "cloudflare" ? 50 : rows.length;
    setImportProgress({ active: true, done: 0, total: rows.length });
    try {
      for (let index = 0; index < rows.length; index += chunkSize) {
        const chunk = rows.slice(index, index + chunkSize);
        await onImport({
          raw: chunk.join("\n"),
          provider,
          channel_id: provider === "cloudflare" ? channelId : undefined
        });
        setImportProgress({ active: true, done: Math.min(index + chunk.length, rows.length), total: rows.length });
      }
      setImportRaw("");
    } finally {
      setImportProgress((current) => ({ ...current, active: false }));
    }
  }

  return (
    <section className="tempGrid">
      <aside className="panel tempControlPanel">
        <div className="panelHeader">
          <h2>临时邮箱</h2>
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
              <option value="">通道</option>
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
            placeholder={provider === "gptmail" ? "前缀" : "用户名"}
            onChange={(event) => (provider === "gptmail" ? setPrefix(event.target.value) : setUsername(event.target.value))}
          />
          <button
            className="iconMini"
            title="生成智能用户名"
            onClick={() => {
              const nextName = smartTempUsername();
              if (provider === "gptmail") setPrefix(nextName);
              else setUsername(nextName);
            }}
          >
            <WandSparkles size={15} />
          </button>
          <input className="input grow" value={domain} placeholder="域名" onChange={(event) => setDomain(event.target.value)} />
        </div>
        {provider === "duckmail" && (
          <input
            className="input fullWidth tempPassword"
            type="password"
            value={password}
            placeholder="DuckMail 密码"
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
          生成
        </button>
        {provider === "cloudflare" && (
          <div className="tempBatchBox">
            <div className="formLine">
              <input
                className="input grow"
                value={batchPrefix}
                placeholder="批量前缀"
                onChange={(event) => setBatchPrefix(event.target.value)}
              />
              <input
                className="input smallInput"
                type="number"
                min={1}
                max={200}
                value={batchCount}
                onChange={(event) => setBatchCount(Math.min(Math.max(Number(event.target.value) || 1, 1), 200))}
              />
            </div>
            <input
              className="input"
              value={batchTagsText}
              placeholder="批量标签，逗号分隔"
              onChange={(event) => setBatchTagsText(event.target.value)}
            />
            <button
              className="button secondary fullWidth"
              disabled={busy || !channelId}
              onClick={() =>
                onGenerateCloudflareBatch({
                  channel_id: channelId,
                  prefix: batchPrefix || undefined,
                  domain: domain || undefined,
                  count: batchCount,
                  tags: parseTagText(batchTagsText)
                })
              }
            >
              <Plus size={16} />
              批量生成 {batchCount}
            </button>
          </div>
        )}

        <textarea
          className="textarea compact tempImportBox"
          value={importRaw}
          onChange={(event) => setImportRaw(event.target.value)}
          placeholder={provider === "duckmail" ? "邮箱----密码" : "邮箱地址"}
        />
        <button
          className="button secondary fullWidth"
          disabled={busy || importProgress.active || !importRaw.trim() || (provider === "cloudflare" && !channelId)}
          onClick={() => void runTempImport()}
        >
          {importProgress.active ? <Loader2 className="spin" size={16} /> : <Upload size={16} />}
          导入
        </button>
        {importProgress.total > 0 && (
          <div className="importProgress">
            <div>
              <span style={{ width: `${Math.round((importProgress.done / importProgress.total) * 100)}%` }} />
            </div>
            <small>
              {importProgress.done}/{importProgress.total}
            </small>
          </div>
        )}
      </aside>

      <aside className="panel tempListPanel">
        <div className="panelHeader">
          <h2>地址</h2>
          <span>{visibleTempEmails.length}/{tempEmails.length}</span>
        </div>
        <div className="tempFilters">
          <input
            className="input"
            value={tempSearch}
            placeholder="搜索地址、标签或状态"
            onChange={(event) => setTempSearch(event.target.value)}
          />
          <div className="formLine">
            <select className="select grow" value={tempProviderFilter} onChange={(event) => setTempProviderFilter(event.target.value)}>
              <option value="all">全部服务</option>
              <option value="gptmail">GPTMail</option>
              <option value="duckmail">DuckMail</option>
              <option value="cloudflare">Cloudflare</option>
            </select>
            <select className="select grow" value={tempTagFilter} onChange={(event) => setTempTagFilter(event.target.value)}>
              <option value="all">全部标签</option>
              {tempTags.map((tag) => (
                <option value={tag} key={tag}>
                  {tag}
                </option>
              ))}
            </select>
          </div>
        </div>
        <div className="tempRows">
          {visibleTempEmails.map((item) => (
            <button key={item.id} className={selectedEmail === item.email ? "tempEmailRow active" : "tempEmailRow"} onClick={() => onSelect(item.email)}>
              <strong>{item.email}</strong>
              <small>
                {item.provider} · {item.message_count} 条消息 · {formatStatus(item.last_refresh_status)}
              </small>
              {item.tags.length > 0 && (
                <span className="tempTagLine">
                  {item.tags.map((tag) => (
                    <span className="chip" key={tag}>
                      {tag}
                    </span>
                  ))}
                </span>
              )}
            </button>
          ))}
        </div>
        {visibleTempEmails.length === 0 && <EmptyState icon={<Cloud size={24} />} text="暂无匹配临时邮箱。" />}
      </aside>

      <section className="panel tempMessagePanel">
        <div className="panelHeader">
          <h2>{selectedTemp?.email ?? "消息"}</h2>
          <div className="rowActions">
            <button className="iconMini" title="刷新" disabled={!selectedEmail || busy} onClick={() => selectedEmail && onRefresh(selectedEmail)}>
              <RefreshCw size={15} />
            </button>
            <button className="iconMini danger" title="删除" disabled={!selectedEmail || busy} onClick={() => selectedEmail && onDelete(selectedEmail)}>
              <Trash2 size={15} />
            </button>
          </div>
        </div>
        {selectedTemp && (
          <div className="tempTagEditor">
            <input
              className="input grow"
              value={tempTagsText}
              placeholder="标签，逗号分隔"
              onChange={(event) => setTempTagsText(event.target.value)}
            />
            <button
              className="button secondary"
              disabled={busy}
              onClick={() => onUpdate({ email: selectedTemp.email, tags: parseTagText(tempTagsText) })}
            >
              <Tags size={15} />
              保存标签
            </button>
          </div>
        )}
        <div className="tempMessageRows">
          {messages.map((message) => (
            <button
              key={message.message_id}
              className={selectedMessage?.message_id === message.message_id ? "messageRow active" : "messageRow"}
              onClick={() => onMessageSelect(message.message_id)}
            >
              <span className="messageTop">
                <strong>{message.subject || "（无主题）"}</strong>
                <small>{message.timestamp ? formatUnixDate(message.timestamp) : formatDate(message.created_at)}</small>
              </span>
              <span className="sender">{message.from_address}</span>
              <span className="preview">{message.content || message.html_content}</span>
            </button>
          ))}
        </div>
        {messages.length === 0 && <EmptyState icon={<Mail size={24} />} text="暂无缓存临时邮件。" />}
      </section>

      <article className="panel tempDetailPanel">
        {selectedMessage ? (
          <>
            <div className="detailHeader">
              <h2>{selectedMessage.subject || "（无主题）"}</h2>
              <p>{selectedMessage.from_address}</p>
            </div>
            <div className="metaGrid">
              <span>邮箱</span>
              <strong>{selectedMessage.email_address}</strong>
              <span>接收时间</span>
              <strong>{selectedMessage.timestamp ? formatUnixDate(selectedMessage.timestamp) : formatDate(selectedMessage.created_at)}</strong>
            </div>
            <MessageBody
              body={selectedMessage.has_html ? selectedMessage.html_content : selectedMessage.content}
              bodyType={selectedMessage.has_html ? "html" : "text"}
            />
          </>
        ) : (
          <EmptyState icon={<Mail size={24} />} text="请选择一封临时邮件。" />
        )}
      </article>

      <section className="panel widePanel">
        <div className="panelHeader">
          <h2>Cloudflare 通道</h2>
          <Cloud size={18} />
        </div>
        <div className="channelEditor">
          <input className="input" value={channelDraft.name} placeholder="名称" onChange={(event) => setChannelDraft({ ...channelDraft, name: event.target.value })} />
          <input
            className="input"
            value={channelDraft.worker_domain}
            placeholder="Worker 域名"
            onChange={(event) => setChannelDraft({ ...channelDraft, worker_domain: event.target.value })}
          />
          <input
            className="input"
            value={channelDraft.email_domains}
            placeholder="域名，逗号分隔"
            onChange={(event) => setChannelDraft({ ...channelDraft, email_domains: event.target.value })}
          />
          <input
            className="input"
            type="password"
            value={channelDraft.admin_password}
            placeholder="管理密码"
            onChange={(event) => setChannelDraft({ ...channelDraft, admin_password: event.target.value })}
          />
          <label className="checkLine">
            <input type="checkbox" checked={channelDraft.enabled} onChange={(event) => setChannelDraft({ ...channelDraft, enabled: event.target.checked })} />
            <span>启用</span>
          </label>
          <label className="checkLine">
            <input type="checkbox" checked={channelDraft.is_default} onChange={(event) => setChannelDraft({ ...channelDraft, is_default: event.target.checked })} />
            <span>默认</span>
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
            保存
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
                <button className="iconMini" title="编辑" onClick={() => setChannelDraft({ id: channel.id, name: channel.name, worker_domain: channel.worker_domain, email_domains: channel.email_domains.join(", "), admin_password: "", enabled: channel.enabled, is_default: channel.is_default })}>
                  <SettingsIcon size={15} />
                </button>
                <button className="iconMini" title="测试" onClick={() => onTestChannel(channel.id)}>
                  <RefreshCw size={15} />
                </button>
                <button className="iconMini danger" title="删除" disabled={channel.reference_count > 0} onClick={() => onDeleteChannel(channel.id)}>
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
  tags,
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
  tags: Tag[];
  busy: boolean;
  onCreate: (input: {
    name: string;
    project_key?: string;
    description?: string;
    scope_mode?: string;
    use_alias_email?: boolean;
    group_ids?: number[];
    tag_ids?: number[];
  }) => void;
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
  const [useAliasEmail, setUseAliasEmail] = useState(false);
  const [selectedGroupIds, setSelectedGroupIds] = useState<number[]>([]);
  const [selectedTagIds, setSelectedTagIds] = useState<number[]>([]);

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
          <h2>项目</h2>
          <FolderKanban size={18} />
        </div>
        <div className="projectCreate">
          <input className="input" value={name} placeholder="项目名称" onChange={(event) => setName(event.target.value)} />
          <input className="input" value={projectKey} placeholder="项目标识，可选" onChange={(event) => setProjectKey(event.target.value)} />
          <textarea
            className="textarea compact"
            value={description}
            placeholder="描述"
            onChange={(event) => setDescription(event.target.value)}
          />
          <select className="select" value={scopeMode} onChange={(event) => setScopeMode(event.target.value)}>
            <option value="all">全部启用账号</option>
            <option value="groups">指定分组</option>
            <option value="tags">指定标签</option>
          </select>
          <label className="checkLine toggleLine">
            <input
              type="checkbox"
              checked={useAliasEmail}
              onChange={(event) => setUseAliasEmail(event.target.checked)}
            />
            <span>项目账号池优先使用账号别名</span>
          </label>
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
          {scopeMode === "tags" && (
            <div className="groupPicker">
              {tags.map((tag) => (
                <label className="checkLine" key={tag.id}>
                  <input
                    type="checkbox"
                    checked={selectedTagIds.includes(tag.id)}
                    onChange={(event) => {
                      setSelectedTagIds((current) =>
                        event.target.checked ? [...current, tag.id] : current.filter((id) => id !== tag.id)
                      );
                    }}
                  />
                  <span className="dot" style={{ backgroundColor: tag.color }} />
                  <span>{tag.name}</span>
                </label>
              ))}
            </div>
          )}
          <button
            className="button primary fullWidth"
            disabled={
              busy ||
              !name.trim() ||
              (scopeMode === "groups" && selectedGroupIds.length === 0) ||
              (scopeMode === "tags" && selectedTagIds.length === 0)
            }
            onClick={() => {
              onCreate({
                name,
                project_key: projectKey || undefined,
                description,
                scope_mode: scopeMode,
                use_alias_email: useAliasEmail,
                group_ids: scopeMode === "groups" ? selectedGroupIds : [],
                tag_ids: scopeMode === "tags" ? selectedTagIds : []
              });
              setName("");
              setProjectKey("");
              setDescription("");
              setUseAliasEmail(false);
            }}
          >
            <Plus size={16} />
            创建项目
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
              <span>{project.stats.to_claim} 个可领取 · {project.stats.success} 个已完成</span>
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
                {selectedProject.use_alias_email && <span className="chip">使用账号别名</span>}
              </div>
              <div className="topActions">
                <button className="button secondary" disabled={busy} onClick={() => onSync(selectedProject.id)}>
                  <RefreshCw size={16} />
                  同步
                </button>
                <button className="button secondary" disabled={busy || accounts.length === 0} onClick={() => onExport(selectedProject.id)}>
                  <Download size={16} />
                  导出
                </button>
                <button className="button primary" disabled={busy} onClick={() => onClaim(selectedProject.id)}>
                  <Archive size={16} />
                  领取
                </button>
              </div>
            </div>

            <div className="statStrip">
              <Stat label="总数" value={selectedProject.stats.total} />
              <Stat label="可领取" value={selectedProject.stats.to_claim} />
              <Stat label="已领取" value={selectedProject.stats.claimed} />
              <Stat label="成功" value={selectedProject.stats.success} />
              <Stat label="失败" value={selectedProject.stats.failed} />
              <Stat label="已移除" value={selectedProject.stats.removed} />
            </div>

            <div className="projectAccountTable">
              <div className="projectTableHeader">
                <span>邮箱</span>
                <span>状态</span>
                <span>领取次数</span>
                <span>租约</span>
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
                        <button className="iconMini" title="标记成功" onClick={() => onAction(selectedProject.id, "success", account.id)}>
                          <CheckCircle2 size={15} />
                        </button>
                        <button className="iconMini danger" title="标记失败" onClick={() => onAction(selectedProject.id, "failed", account.id)}>
                          <XCircle size={15} />
                        </button>
                        <button className="iconMini" title="释放" onClick={() => onAction(selectedProject.id, "release", account.id)}>
                          <RefreshCw size={15} />
                        </button>
                      </>
                    )}
                    {account.status !== "removed" ? (
                      <button className="iconMini danger" title="移除" onClick={() => onAction(selectedProject.id, "remove", account.id)}>
                        <Trash2 size={15} />
                      </button>
                    ) : (
                      <button className="iconMini" title="恢复" onClick={() => onAction(selectedProject.id, "restore", account.id)}>
                        <RefreshCw size={15} />
                      </button>
                    )}
                  </span>
                </div>
              ))}
            </div>
            {accounts.length === 0 && <EmptyState icon={<FolderKanban size={24} />} text="同步项目范围后会填充账号。" />}
          </>
        ) : (
          <EmptyState icon={<FolderKanban size={24} />} text="创建项目后开始分配账号。" />
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
  return <span className={`statusPill status-${status}`}>{formatStatus(status)}</span>;
}

function isRefreshReady(account: Account) {
  return account.has_refresh_token || account.has_imap_password || account.has_password;
}

function refreshCredentialLabel(account: Account) {
  if (account.has_refresh_token) return "Graph";
  if (account.has_imap_password) return "IMAP 密码";
  if (account.has_password) return "账号密码";
  return "缺少凭据";
}

function AccountEditor({
  account,
  groups,
  tags,
  settings,
  busy,
  oauthUrl,
  oauthCallback,
  onOauthCallbackChange,
  onSave,
  onRevealAccountSecrets,
  onGenerateOAuthUrl,
  onExchangeOAuthToken
}: {
  account?: Account;
  groups: Group[];
  tags: Tag[];
  settings: Settings | null;
  busy: boolean;
  oauthUrl: string;
  oauthCallback: string;
  onOauthCallbackChange: (value: string) => void;
  onSave: (input: Parameters<typeof api.updateAccount>[0]) => void;
  onRevealAccountSecrets: (input: Parameters<typeof api.revealAccountSecrets>[0]) => Promise<Awaited<ReturnType<typeof api.revealAccountSecrets>>>;
  onGenerateOAuthUrl: (input: { client_id: string; redirect_uri: string; login_hint?: string; provider?: string }) => void;
  onExchangeOAuthToken: (input: { account_id?: number; client_id: string; redirect_uri: string; code_or_url: string; provider?: string }) => void;
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
    proxy_url: "",
    fallback_proxy_url_1: "",
    fallback_proxy_url_2: "",
    password: "",
    client_id: "",
    refresh_token: "",
    imap_password: "",
    tag_ids: [] as number[],
    aliasesText: ""
  });
  const [revealPassword, setRevealPassword] = useState("");
  const [revealedSecrets, setRevealedSecrets] = useState<Awaited<ReturnType<typeof api.revealAccountSecrets>> | null>(null);
  const [revealError, setRevealError] = useState<string | null>(null);
  const [revealing, setRevealing] = useState(false);

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
      proxy_url: account.proxy_url,
      fallback_proxy_url_1: account.fallback_proxy_url_1,
      fallback_proxy_url_2: account.fallback_proxy_url_2,
      password: "",
      client_id: settings?.graph_client_id ?? "",
      refresh_token: "",
      imap_password: "",
      tag_ids: account.tags.map((tag) => tag.id),
      aliasesText: account.aliases.join("\n")
    });
    onOauthCallbackChange("");
    setRevealPassword("");
    setRevealedSecrets(null);
    setRevealError(null);
  }, [account?.id, account?.updated_at, settings?.graph_client_id]);

  if (!account) {
    return (
      <div className="panel">
        <EmptyState icon={<KeyRound size={24} />} text="请选择一个账号设置授权。" />
      </div>
    );
  }

  const redirectUri = settings?.oauth_redirect_uri || "http://localhost:8080";

  return (
    <div className="panel">
      <div className="panelHeader">
        <h2>授权设置</h2>
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
          placeholder="备注"
          onChange={(event) => setDraft({ ...draft, remark: event.target.value })}
        />
      </div>
      <label className="checkLine toggleLine">
        <input
          type="checkbox"
          checked={draft.forward_enabled}
          onChange={(event) => setDraft({ ...draft, forward_enabled: event.target.checked })}
        />
        <span>转发此账号的缓存邮件</span>
      </label>
      <textarea
        className="textarea compact"
        value={draft.aliasesText}
        placeholder="别名邮箱，每行一个；项目池开启别名时会优先使用第一个"
        onChange={(event) => setDraft({ ...draft, aliasesText: event.target.value })}
      />
      {tags.length > 0 && (
        <div className="groupPicker tagPicker">
          {tags.map((tag) => (
            <label className="checkLine" key={tag.id}>
              <input
                type="checkbox"
                checked={draft.tag_ids.includes(tag.id)}
                onChange={(event) => {
                  setDraft((current) => ({
                    ...current,
                    tag_ids: event.target.checked
                      ? [...current.tag_ids, tag.id]
                      : current.tag_ids.filter((id) => id !== tag.id)
                  }));
                }}
              />
              <span className="dot" style={{ backgroundColor: tag.color }} />
              <span>{tag.name}</span>
            </label>
          ))}
        </div>
      )}
      <div className="formLine">
        <input
          className="input grow"
          value={draft.client_id}
          placeholder="Microsoft 客户端 ID"
          onChange={(event) => setDraft({ ...draft, client_id: event.target.value })}
        />
        <button
          className="button secondary"
          disabled={!draft.client_id.trim()}
          onClick={() => onGenerateOAuthUrl({ client_id: draft.client_id, redirect_uri: redirectUri, login_hint: draft.email, provider: draft.provider })}
        >
          <KeyRound size={16} />
          OAuth 链接
        </button>
      </div>
      {oauthUrl && <textarea className="textarea compact" readOnly value={oauthUrl} />}
      <div className="formLine">
        <input
          className="input grow"
          value={oauthCallback}
          placeholder="粘贴回调 URL 或授权码"
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
              code_or_url: oauthCallback,
              provider: draft.provider
            })
          }
        >
          保存 OAuth
        </button>
      </div>
      <div className="formLine">
        <input
          className="input grow"
          value={draft.imap_host}
          placeholder="IMAP 主机"
          onChange={(event) => setDraft({ ...draft, imap_host: event.target.value })}
        />
        <input
          className="input smallInput"
          type="number"
          value={draft.imap_port}
          onChange={(event) => setDraft({ ...draft, imap_port: Number(event.target.value) || 993 })}
        />
      </div>
      <input
        className="input"
        value={draft.proxy_url}
        placeholder="账号主代理，留空则继承分组代理"
        onChange={(event) => setDraft({ ...draft, proxy_url: event.target.value })}
      />
      <div className="formLine">
        <input
          className="input grow"
          value={draft.fallback_proxy_url_1}
          placeholder="账号备用代理 1"
          onChange={(event) => setDraft({ ...draft, fallback_proxy_url_1: event.target.value })}
        />
        <input
          className="input grow"
          value={draft.fallback_proxy_url_2}
          placeholder="账号备用代理 2"
          onChange={(event) => setDraft({ ...draft, fallback_proxy_url_2: event.target.value })}
        />
      </div>
      <div className="formLine">
        <input
          className="input grow"
          type="password"
          value={draft.password}
          placeholder="账号密码，可选"
          onChange={(event) => setDraft({ ...draft, password: event.target.value })}
        />
        <input
          className="input grow"
          type="password"
          value={draft.imap_password}
          placeholder="IMAP 密码，可选"
          onChange={(event) => setDraft({ ...draft, imap_password: event.target.value })}
        />
      </div>
      <div className="secretRevealBox">
        <div className="formLine">
          <input
            className="input grow"
            type="password"
            value={revealPassword}
            placeholder="本地应用密码"
            onChange={(event) => setRevealPassword(event.target.value)}
          />
          <button
            className="button secondary"
            disabled={revealing || revealPassword.length < 8}
            onClick={async () => {
              setRevealError(null);
              setRevealing(true);
              try {
                setRevealedSecrets(await onRevealAccountSecrets({ account_id: account.id, password: revealPassword }));
              } catch (err) {
                setRevealedSecrets(null);
                setRevealError(readError(err));
              } finally {
                setRevealing(false);
              }
            }}
          >
            {revealing ? <Loader2 className="spin" size={16} /> : <KeyRound size={16} />}
            查看敏感信息
          </button>
          <button
            className="button ghost"
            disabled={!revealedSecrets && !revealPassword}
            onClick={() => {
              setRevealPassword("");
              setRevealedSecrets(null);
              setRevealError(null);
            }}
          >
            清除
          </button>
        </div>
        {revealError && <div className="formError">{revealError}</div>}
        {revealedSecrets && (
          <div className="secretPreviewGrid">
            <label className="field">
              账号密码
              <input className="input" readOnly value={revealedSecrets.password} />
            </label>
            <label className="field">
              Client ID
              <input className="input" readOnly value={revealedSecrets.client_id} />
            </label>
            <label className="field">
              Refresh Token 预览
              <input className="input" readOnly value={revealedSecrets.refresh_token_preview} />
            </label>
            <label className="field">
              IMAP 密码
              <input className="input" readOnly value={revealedSecrets.imap_password} />
            </label>
          </div>
        )}
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
            proxy_url: draft.proxy_url,
            fallback_proxy_url_1: draft.fallback_proxy_url_1,
            fallback_proxy_url_2: draft.fallback_proxy_url_2,
            client_id: draft.client_id || undefined,
            password: draft.password || undefined,
            imap_password: draft.imap_password || undefined,
            refresh_token: draft.refresh_token || undefined,
            tag_ids: draft.tag_ids,
            aliases: parseAliasText(draft.aliasesText)
          })
        }
      >
        {busy ? <Loader2 className="spin" size={16} /> : <SettingsIcon size={16} />}
        保存账号
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
  retryQueue,
  localRetention,
  schedulerStatus,
  busy,
  onSave,
  onRunForwarding,
  onRunBackup,
  onRestoreBackup,
  onFilterAutomationRuns,
  onClearAutomationRuns,
  onClearLocalData,
  onRunRetryQueue,
  onRetryQueueItem,
  onDismissRetryItem
}: {
  status: AppStatus;
  settings: Settings;
  forwardingLogs: ForwardingLog[];
  backupLogs: BackupLog[];
  automationRuns: AutomationRun[];
  retryQueue: RetryQueueItem[];
  localRetention: LocalRetentionSummary | null;
  schedulerStatus: SchedulerStatus | null;
  busy: boolean;
  onSave: (settings: Settings) => void;
  onRunForwarding: () => void;
  onRunBackup: () => void;
  onRestoreBackup: (backupLogId: number) => void;
  onFilterAutomationRuns: (query: AutomationRunQuery) => void;
  onClearAutomationRuns: (query: AutomationRunQuery & { clear_all?: boolean }) => void;
  onClearLocalData: (input: ClearLocalDataInput) => void;
  onRunRetryQueue: () => void;
  onRetryQueueItem: (retryId: number) => void;
  onDismissRetryItem: (retryId: number) => void;
}) {
  const [draft, setDraft] = useState(settings);
  const [runFilters, setRunFilters] = useState({ job_type: "all", trigger_type: "all", status: "all", search: "" });
  const [clearLocal, setClearLocal] = useState({
    clear_mail_cache: false,
    clear_temp_mail_cache: false,
    clear_attachments: false,
    clear_exports: false,
    confirm: ""
  });
  useEffect(() => setDraft(settings), [settings]);

  const hasClearSelection =
    clearLocal.clear_mail_cache || clearLocal.clear_temp_mail_cache || clearLocal.clear_attachments || clearLocal.clear_exports;

  function setField<K extends keyof Settings>(key: K, value: Settings[K]) {
    setDraft((current) => ({ ...current, [key]: value }));
  }

  function automationRunQuery(): AutomationRunQuery {
    return {
      job_type: runFilters.job_type === "all" ? undefined : runFilters.job_type,
      trigger_type: runFilters.trigger_type === "all" ? undefined : runFilters.trigger_type,
      status: runFilters.status === "all" ? undefined : runFilters.status,
      search: runFilters.search.trim() || undefined
    };
  }

  return (
    <section className="settingsGrid">
      <div className="panel">
        <div className="panelHeader">
          <h2>服务商设置</h2>
          <SettingsIcon size={18} />
        </div>
        <Field label="Microsoft Graph 客户端 ID" value={draft.graph_client_id} onChange={(value) => setField("graph_client_id", value)} />
        <Field label="OAuth 回调地址" value={draft.oauth_redirect_uri} onChange={(value) => setField("oauth_redirect_uri", value)} />
        <Field label="GPTMail 基础地址" value={draft.gptmail_base_url} onChange={(value) => setField("gptmail_base_url", value)} />
        <SecretField label="GPTMail API 密钥" value={draft.gptmail_api_key} onChange={(value) => setField("gptmail_api_key", value)} />
        <Field label="DuckMail 基础地址" value={draft.duckmail_base_url} onChange={(value) => setField("duckmail_base_url", value)} />
        <SecretField label="DuckMail API 密钥" value={draft.duckmail_api_key} onChange={(value) => setField("duckmail_api_key", value)} />
      </div>

      <div className="panel">
        <div className="panelHeader">
          <h2>调度器</h2>
          <RefreshCw size={18} />
        </div>
        <label className="checkLine toggleLine">
          <input
            type="checkbox"
            checked={draft.scheduler_refresh_enabled}
            onChange={(event) => setField("scheduler_refresh_enabled", event.target.checked)}
          />
          <span>定时刷新邮箱</span>
        </label>
        <div className="formLine">
          <NumberField
            label="刷新间隔"
            value={draft.scheduler_refresh_interval_minutes}
            min={1}
            onChange={(value) => setField("scheduler_refresh_interval_minutes", value)}
          />
          <NumberField
            label="每账号邮件数"
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
          <span>定时转发</span>
        </label>
        <NumberField
          label="转发间隔"
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
          <span>定时 WebDAV 备份</span>
        </label>
        <NumberField
          label="备份间隔"
          value={draft.backup_interval_minutes}
          min={1}
          onChange={(value) => setField("backup_interval_minutes", value)}
        />
      </div>

      <div className="panel">
        <div className="panelHeader">
          <h2>转发通道</h2>
          <Mail size={18} />
        </div>
        <div className="formLine">
          <Field label="SMTP 主机" value={draft.forward_smtp_host} onChange={(value) => setField("forward_smtp_host", value)} />
          <NumberField
            label="端口"
            value={draft.forward_smtp_port}
            min={1}
            max={65535}
            onChange={(value) => setField("forward_smtp_port", value)}
          />
        </div>
        <Field label="SMTP 用户名" value={draft.forward_smtp_username} onChange={(value) => setField("forward_smtp_username", value)} />
        <SecretField
          label="SMTP 密码"
          value={draft.forward_smtp_password}
          onChange={(value) => setField("forward_smtp_password", value)}
        />
        <Field label="SMTP 发件人" value={draft.forward_smtp_from} onChange={(value) => setField("forward_smtp_from", value)} />
        <Field label="SMTP 收件人" value={draft.forward_smtp_to} onChange={(value) => setField("forward_smtp_to", value)} />
        <SecretField
          label="Telegram 机器人 Token"
          value={draft.forward_telegram_bot_token}
          onChange={(value) => setField("forward_telegram_bot_token", value)}
        />
        <Field
          label="Telegram 会话 ID"
          value={draft.forward_telegram_chat_id}
          onChange={(value) => setField("forward_telegram_chat_id", value)}
        />
        <SecretField
          label="企业微信 Webhook"
          value={draft.forward_wecom_webhook}
          onChange={(value) => setField("forward_wecom_webhook", value)}
        />
      </div>

      <div className="panel">
        <div className="panelHeader">
          <h2>WebDAV 备份</h2>
          <Archive size={18} />
        </div>
        <Field label="WebDAV 地址" value={draft.webdav_url} onChange={(value) => setField("webdav_url", value)} />
        <Field label="WebDAV 用户名" value={draft.webdav_username} onChange={(value) => setField("webdav_username", value)} />
        <SecretField label="WebDAV 密码" value={draft.webdav_password} onChange={(value) => setField("webdav_password", value)} />
        <div className="actionGrid">
          <button className="button secondary" disabled={busy} onClick={onRunForwarding}>
            {busy ? <Loader2 className="spin" size={16} /> : <Mail size={16} />}
            运行转发
          </button>
          <button className="button secondary" disabled={busy} onClick={onRunBackup}>
            {busy ? <Loader2 className="spin" size={16} /> : <Archive size={16} />}
            运行备份
          </button>
        </div>
      </div>

      <div className="panel widePanel">
        <div className="panelHeader">
          <h2>本地存储和运行状态</h2>
          <Lock size={18} />
        </div>
        <div className="storageLine">
          <span>SQLite 数据库</span>
          <code>{status.db_path}</code>
        </div>
        <div className="runStatusGrid">
          <RunStatus label="刷新" value={schedulerStatus?.last_refresh_at} />
          <RunStatus label="转发" value={schedulerStatus?.last_forwarding_at} />
          <RunStatus label="备份" value={schedulerStatus?.last_backup_at} />
        </div>
        {localRetention && (
          <>
            <div className="retentionStats">
              <Stat label="本地邮件" value={localRetention.mail_message_count} />
              <Stat label="未读" value={localRetention.unread_message_count} />
              <Stat label="临时消息" value={localRetention.temp_message_count} />
              <Stat label="附件文件" value={localRetention.attachment_file_count} />
              <Stat label="导出文件" value={localRetention.export_file_count} />
              <Stat label="待重试" value={localRetention.retry_queue_count} />
            </div>
            <div className="retentionSizeGrid">
              <span>数据库</span>
              <strong>{formatBytes(localRetention.database_size) || "0 B"}</strong>
              <span>附件</span>
              <strong>{formatBytes(localRetention.attachments_size) || "0 B"}</strong>
              <span>导出</span>
              <strong>{formatBytes(localRetention.exports_size) || "0 B"}</strong>
              <span>备份</span>
              <strong>{formatBytes(localRetention.backups_size) || "0 B"}</strong>
              <span>最新邮件</span>
              <strong>{localRetention.latest_mail_received_at ? formatDate(localRetention.latest_mail_received_at) : "-"}</strong>
              <span>账号刷新</span>
              <strong>{localRetention.latest_account_refresh_at ? formatDate(localRetention.latest_account_refresh_at) : "-"}</strong>
            </div>
            <div className="localStateRow">
              <StatusPill status="local" />
              <span>清理本地缓存不删除远端邮件</span>
            </div>
            <div className="localClearBox">
              <label className="checkLine">
                <input
                  type="checkbox"
                  checked={clearLocal.clear_mail_cache}
                  onChange={(event) => setClearLocal({ ...clearLocal, clear_mail_cache: event.target.checked })}
                />
                <span>邮件缓存</span>
              </label>
              <label className="checkLine">
                <input
                  type="checkbox"
                  checked={clearLocal.clear_temp_mail_cache}
                  onChange={(event) => setClearLocal({ ...clearLocal, clear_temp_mail_cache: event.target.checked })}
                />
                <span>临时邮箱消息</span>
              </label>
              <label className="checkLine">
                <input
                  type="checkbox"
                  checked={clearLocal.clear_attachments}
                  onChange={(event) => setClearLocal({ ...clearLocal, clear_attachments: event.target.checked })}
                />
                <span>附件下载</span>
              </label>
              <label className="checkLine">
                <input
                  type="checkbox"
                  checked={clearLocal.clear_exports}
                  onChange={(event) => setClearLocal({ ...clearLocal, clear_exports: event.target.checked })}
                />
                <span>导出文件</span>
              </label>
              <input
                className="input grow"
                value={clearLocal.confirm}
                placeholder="CLEAR LOCAL DATA"
                onChange={(event) => setClearLocal({ ...clearLocal, confirm: event.target.value })}
              />
              <button
                className="button danger"
                disabled={busy || !hasClearSelection || clearLocal.confirm !== "CLEAR LOCAL DATA"}
                onClick={() => onClearLocalData(clearLocal)}
              >
                <Trash2 size={16} />
                清理本地数据
              </button>
            </div>
          </>
        )}
        <button className="button primary" disabled={busy} onClick={() => onSave(draft)}>
          {busy ? <Loader2 className="spin" size={16} /> : <SettingsIcon size={16} />}
          保存设置
        </button>
      </div>

      <div className="panel widePanel">
        <div className="panelHeader">
          <h2>重试队列</h2>
          <RotateCcw size={18} />
        </div>
        <div className="tableActions">
          <button className="button secondary" disabled={busy || retryQueue.length === 0} onClick={onRunRetryQueue}>
            {busy ? <Loader2 className="spin" size={16} /> : <RotateCcw size={16} />}
            运行到期重试
          </button>
        </div>
        <div className="logTable retryQueueTable">
          <div className="logHeader">
            <span>时间</span>
            <span>任务</span>
            <span>状态</span>
            <span>账号</span>
            <span>目标</span>
            <span>次数</span>
            <span>下次</span>
            <span>错误</span>
            <span>操作</span>
          </div>
          {retryQueue.map((item) => (
            <div className="logRow" key={item.id}>
              <span>{formatDate(item.updated_at)}</span>
              <span>{formatRetryTask(item)}</span>
              <StatusPill status={item.status} />
              <span>{item.account_email || "-"}</span>
              <span>{item.channel ? `${item.message_id} / ${item.channel}` : item.message_id}</span>
              <span>{item.attempts} / {item.max_attempts}</span>
              <span>{item.next_attempt_at ? formatDate(item.next_attempt_at) : "-"}</span>
              <span>{item.error_message}</span>
              <span className="rowActions">
                <button className="iconMini" title="立即重试" disabled={busy} onClick={() => onRetryQueueItem(item.id)}>
                  <RotateCcw size={14} />
                </button>
                <button className="iconMini danger" title="忽略" disabled={busy} onClick={() => onDismissRetryItem(item.id)}>
                  <Trash2 size={14} />
                </button>
              </span>
            </div>
          ))}
        </div>
        {retryQueue.length === 0 && <EmptyState icon={<RotateCcw size={24} />} text="暂无待处理重试项。" />}
      </div>

      <div className="panel widePanel">
        <div className="panelHeader">
          <h2>自动化历史</h2>
          <RefreshCw size={18} />
        </div>
        <div className="automationFilters">
          <select
            className="select"
            value={runFilters.job_type}
            onChange={(event) => setRunFilters({ ...runFilters, job_type: event.target.value })}
          >
            <option value="all">全部任务</option>
            <option value="refresh">刷新</option>
            <option value="forwarding">转发</option>
            <option value="backup">备份</option>
            <option value="retry">重试</option>
          </select>
          <select
            className="select"
            value={runFilters.trigger_type}
            onChange={(event) => setRunFilters({ ...runFilters, trigger_type: event.target.value })}
          >
            <option value="all">全部触发</option>
            <option value="manual">手动</option>
            <option value="schedule">定时</option>
          </select>
          <select
            className="select"
            value={runFilters.status}
            onChange={(event) => setRunFilters({ ...runFilters, status: event.target.value })}
          >
            <option value="all">全部状态</option>
            <option value="success">成功</option>
            <option value="failed">失败</option>
          </select>
          <input
            className="input grow"
            value={runFilters.search}
            placeholder="搜索详情"
            onChange={(event) => setRunFilters({ ...runFilters, search: event.target.value })}
            onKeyDown={(event) => {
              if (event.key === "Enter") onFilterAutomationRuns(automationRunQuery());
            }}
          />
          <button className="button secondary" disabled={busy} onClick={() => onFilterAutomationRuns(automationRunQuery())}>
            <Search size={16} />
            应用
          </button>
          <button
            className="button danger"
            disabled={busy || automationRuns.length === 0}
            onClick={() => {
              const query = automationRunQuery();
              const clearAll = !query.job_type && !query.trigger_type && !query.status && !query.search;
              if (window.confirm(clearAll ? "确认清空全部自动化历史？" : "确认清理匹配的自动化历史？")) {
                onClearAutomationRuns({ ...query, clear_all: clearAll });
              }
            }}
          >
            <Trash2 size={16} />
            清理
          </button>
        </div>
        <div className="logTable automationLogTable">
          <div className="logHeader">
            <span>时间</span>
            <span>任务</span>
            <span>触发</span>
            <span>状态</span>
            <span>数量</span>
            <span>耗时</span>
            <span>详情</span>
          </div>
          {automationRuns.map((run) => (
            <div className="logRow" key={run.id}>
              <span>{formatDate(run.finished_at)}</span>
              <span>{formatAutomationJob(run.job_type)}</span>
              <span>{formatAutomationTrigger(run.trigger_type)}</span>
              <StatusPill status={run.status} />
              <span>{run.refreshed} 成功 / {run.failed} 失败</span>
              <span>{formatDuration(run.duration_ms)}</span>
              <span>{formatResultMessage(run.message)}</span>
            </div>
          ))}
        </div>
        {automationRuns.length === 0 && <EmptyState icon={<RefreshCw size={24} />} text="暂无自动化运行记录。" />}
      </div>

      <div className="panel widePanel">
        <div className="panelHeader">
          <h2>转发日志</h2>
          <Mail size={18} />
        </div>
        <div className="logTable forwardingLogTable">
          <div className="logHeader">
            <span>时间</span>
            <span>账号</span>
            <span>通道</span>
            <span>状态</span>
            <span>详情</span>
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
        {forwardingLogs.length === 0 && <EmptyState icon={<Mail size={24} />} text="暂无转发记录。" />}
      </div>

      <div className="panel widePanel">
        <div className="panelHeader">
          <h2>备份日志</h2>
          <Archive size={18} />
        </div>
        <div className="logTable backupLogTable">
          <div className="logHeader">
            <span>时间</span>
            <span>文件</span>
            <span>状态</span>
            <span>大小</span>
            <span>目标</span>
            <span>操作</span>
          </div>
          {backupLogs.map((log) => (
            <div className="logRow" key={log.id}>
              <span>{formatDate(log.created_at)}</span>
              <span>{log.file_name}</span>
              <StatusPill status={log.status} />
              <span>{formatBytes(log.size)}</span>
              <span>{log.error_message || log.target}</span>
              <span className="rowActions">
                <button
                  className="iconMini"
                  title="恢复备份"
                  disabled={busy || log.status !== "success"}
                  onClick={() => {
                    if (window.confirm("确认从此备份恢复当前工作区？恢复前会先创建安全快照。")) {
                      onRestoreBackup(log.id);
                    }
                  }}
                >
                  <RotateCcw size={14} />
                </button>
              </span>
            </div>
          ))}
        </div>
        {backupLogs.length === 0 && <EmptyState icon={<Archive size={24} />} text="暂无备份记录。" />}
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
      <strong>{value ? formatDate(value) : "从未"}</strong>
    </div>
  );
}

function MessageBody({ body, bodyType }: { body: string; bodyType?: string | null }) {
  if (bodyType?.toLowerCase() === "html" && body.trim()) {
    return (
      <iframe
        className="messageHtmlFrame"
        title="邮件正文"
        sandbox=""
        referrerPolicy="no-referrer"
        srcDoc={buildSandboxedEmailHtml(body)}
      />
    );
  }
  return <div className="messageBody">{body}</div>;
}

function RemoteFailurePanel({
  failure,
  busy,
  onRetry,
  onDismiss
}: {
  failure: RemoteSyncFailure;
  busy: boolean;
  onRetry: (retryId: number) => void;
  onDismiss: (retryId: number) => void;
}) {
  return (
    <div className="remoteFailurePanel">
      <div>
        <strong>{formatRemoteFailureAction(failure.action)}在远端邮箱执行失败</strong>
        <p>{failure.error_message}</p>
        <small>
          {formatStatus(failure.status)} · {failure.attempts} / {failure.max_attempts} 次
          {failure.next_attempt_at ? ` · 下次 ${formatDate(failure.next_attempt_at)}` : ""}
        </small>
      </div>
      <div className="remoteFailureActions">
        <button className="button compact secondary" disabled={busy} onClick={() => onRetry(failure.retry_id)}>
          <RotateCcw size={14} />
          重试
        </button>
        <button className="button compact ghost" disabled={busy} onClick={() => onDismiss(failure.retry_id)}>
          <Trash2 size={14} />
          忽略
        </button>
      </div>
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
      <span className="railLabel">{title}</span>
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
  return new Intl.DateTimeFormat("zh-CN", {
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

function parseAliasText(value: string) {
  const aliases = value
    .split(/[\n,;]+/)
    .map((item) => item.trim().toLowerCase())
    .filter(Boolean);
  return Array.from(new Set(aliases));
}

function parseTagText(value: string) {
  const tags = value
    .split(/[\n,;，；]+/)
    .map((item) => item.trim())
    .filter(Boolean);
  const seen = new Set<string>();
  return tags.filter((tag) => {
    const key = tag.toLowerCase();
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

function smartTempUsername() {
  const adjectives = ["clear", "fast", "nova", "quiet", "prime", "bright", "solid", "fresh"];
  const nouns = ["mail", "orbit", "relay", "pilot", "signal", "matrix", "portal", "vertex"];
  const adjective = adjectives[Math.floor(Math.random() * adjectives.length)];
  const noun = nouns[Math.floor(Math.random() * nouns.length)];
  const suffix = Math.floor(1000 + Math.random() * 9000);
  return `${adjective}${noun}${suffix}`;
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

function formatRetryTask(item: RetryQueueItem) {
  if (item.task_type === "mail_mark") return item.action === "mark_read" ? "标记已读" : "标记未读";
  if (item.task_type === "mail_delete") return "删除邮件";
  if (item.task_type === "forward_message") return "转发邮件";
  if (item.task_type === "temp_refresh") return "刷新临时邮箱";
  if (item.task_type === "refresh_account") return "刷新账号";
  if (item.task_type === "backup_job") return "执行备份";
  return item.task_type;
}

function formatRemoteFailureAction(action: string) {
  if (action === "mark_read") return "标记已读";
  if (action === "mark_unread") return "标记未读";
  if (action === "delete") return "删除邮件";
  return action || "远端同步";
}

function formatStatus(status: string) {
  const map: Record<string, string> = {
    active: "启用",
    disabled: "停用",
    never: "从未",
    success: "成功",
    failed: "失败",
    pending: "待处理",
    removed: "已移除",
    toClaim: "可领取",
    claimed: "已领取",
    read: "已读",
    unread: "未读",
    local: "本地"
  };
  return map[status] ?? status;
}

function formatAutomationJob(job: string) {
  const map: Record<string, string> = {
    refresh: "刷新",
    forwarding: "转发",
    backup: "备份",
    retry: "重试"
  };
  return map[job] ?? job;
}

function formatAutomationTrigger(trigger: string) {
  const map: Record<string, string> = {
    manual: "手动",
    schedule: "定时"
  };
  return map[trigger] ?? trigger;
}

function formatResultMessage(message: string) {
  if (!message) return message;
  if (message === "No retry item(s) due") return "暂无到期重试项";
  if (message === "Refresh job accepted. Provider adapters are available in the Tauri runtime.") {
    return "刷新任务已接收。服务商适配器将在 Tauri 运行时可用。";
  }
  if (message === "Backup preview completed") return "备份预览已完成";
  if (message === "Temp mailbox refreshed") return "临时邮箱已刷新";
  if (message === "Cloudflare channel connected") return "Cloudflare 通道连接成功";
  if (message === "uploaded") return "已上传";

  const localMailFailure = message.match(/^(Marked read|Marked unread|Deleted) (\d+) local message\(s\), (\d+) remote sync failed: (.+)$/);
  if (localMailFailure) {
    const [, action, changed, failed, detail] = localMailFailure;
    return `${formatMailAction(action, changed, "本地邮件")}，${failed} 封远端同步失败：${detail}`;
  }

  const simpleMail = message.match(/^(Marked read|Marked unread|Deleted) (\d+) message\(s\)$/);
  if (simpleMail) {
    const [, action, count] = simpleMail;
    return formatMailAction(action, count, "邮件");
  }

  const refreshedWithFailures = message.match(/^Refreshed (\d+) account\(s\), (\d+) failed: (.+)$/);
  if (refreshedWithFailures) {
    const [, refreshed, failed, detail] = refreshedWithFailures;
    return `已刷新 ${refreshed} 个账号，${failed} 个失败：${detail}`;
  }

  const refreshedWithCachedFailures = message.match(/^Refreshed (\d+) account\(s\), cached (\d+) message\(s\), (\d+) failed: (.+)$/);
  if (refreshedWithCachedFailures) {
    const [, refreshed, cached, failed, detail] = refreshedWithCachedFailures;
    return `已刷新 ${refreshed} 个账号，缓存 ${cached} 封邮件，${failed} 个失败：${detail}`;
  }

  const refreshedWithCached = message.match(/^Refreshed (\d+) account\(s\), cached (\d+) message\(s\)$/);
  if (refreshedWithCached) return `已刷新 ${refreshedWithCached[1]} 个账号，缓存 ${refreshedWithCached[2]} 封邮件`;

  const refreshed = message.match(/^Refreshed (\d+) account\(s\)$/);
  if (refreshed) return `已刷新 ${refreshed[1]} 个账号`;

  const refreshedTemp = message.match(/^Refreshed (\d+) temp message\(s\)$/);
  if (refreshedTemp) return `已刷新 ${refreshedTemp[1]} 条临时邮件`;

  const forwardedWithFailures = message.match(/^Forwarded (\d+) message channel\(s\), (\d+) failed: (.+)$/);
  if (forwardedWithFailures) {
    const [, forwarded, failed, detail] = forwardedWithFailures;
    return `已转发 ${forwarded} 个消息通道，${failed} 个失败：${detail}`;
  }

  const forwarded = message.match(/^Forwarded (\d+) message channel\(s\), skipped (\d+)$/);
  if (forwarded) return `已转发 ${forwarded[1]} 个消息通道，跳过 ${forwarded[2]} 个`;

  const forwardedPreview = message.match(/^Forwarded (\d+) preview item\(s\)$/);
  if (forwardedPreview) return `已转发 ${forwardedPreview[1]} 个预览项`;

  const retriedWithFailures = message.match(/^Retried (\d+) item\(s\), (\d+) failed: (.+)$/);
  if (retriedWithFailures) {
    const [, retried, failed, detail] = retriedWithFailures;
    return `已重试 ${retried} 项，${failed} 项失败：${detail}`;
  }

  const retried = message.match(/^Retried (\d+) item\(s\)$/);
  if (retried) return `已重试 ${retried[1]} 项`;

  const dismissed = message.match(/^Dismissed (\d+) retry item\(s\)$/);
  if (dismissed) return `已忽略 ${dismissed[1]} 个重试项`;

  const cleared = message.match(/^Cleared (\d+) automation run\(s\)$/);
  if (cleared) return `已清理 ${cleared[1]} 条自动化记录`;

  const clearedLocal = message.match(/^Cleared local data: (\d+) mail message\(s\), (\d+) temp message\(s\), (\d+) file\(s\)$/);
  if (clearedLocal) return `已清理本地数据：${clearedLocal[1]} 封邮件、${clearedLocal[2]} 条临时消息、${clearedLocal[3]} 个文件`;

  const batch = message.match(/^Batch (delete|move_group|set_forward|add_tags|remove_tags) processed (\d+) account\(s\)$/);
  if (batch) {
    const actionMap: Record<string, string> = {
      delete: "删除",
      move_group: "移动",
      set_forward: "更新转发开关",
      add_tags: "添加标签",
      remove_tags: "移除标签"
    };
    return `已批量${actionMap[batch[1]]} ${batch[2]} 个账号`;
  }

  const uploaded = message.match(/^Uploaded (.+)$/);
  if (uploaded) return `已上传 ${uploaded[1]}`;

  const restored = message.match(/^Restored local backup (.+)$/);
  if (restored) return `已恢复本地备份 ${restored[1]}`;

  return message;
}

function formatMailAction(action: string, count: string, target: string) {
  if (action === "Marked read") return `已将 ${count} 封${target}标记为已读`;
  if (action === "Marked unread") return `已将 ${count} 封${target}标记为未读`;
  if (action === "Deleted") return `已删除 ${count} 封${target}`;
  return action;
}

function exportNotice(result: ExportResult) {
  const size = formatBytes(result.size);
  return `已导出 ${result.item_count} 项到 ${result.path}${size ? `（${size}）` : ""}`;
}

function readError(err: unknown) {
  if (err instanceof Error) return err.message;
  if (typeof err === "object" && err && "message" in err) return String((err as { message: unknown }).message);
  return String(err);
}

export default App;
