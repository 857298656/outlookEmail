import {
  Activity,
  Archive,
  CheckCircle2,
  ChevronDown,
  ChevronUp,
  Cloud,
  Copy,
  Download,
  ExternalLink,
  FileText,
  FolderKanban,
  Inbox,
  KeyRound,
  Loader2,
  Lock,
  Mail,
  Menu,
  Minus,
  PanelLeftClose,
  PanelLeftOpen,
  Plus,
  RefreshCw,
  RotateCcw,
  Search,
  Settings as SettingsIcon,
  Share2,
  Square,
  Tags,
  Trash2,
  Upload,
  Users,
  WandSparkles,
  X,
  XCircle
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties, ReactNode } from "react";
import { api } from "./api";
import { Toast } from "./components/Toast";
import type { ToastMessage } from "./components/Toast";
import { buildSandboxedEmailHtml } from "./lib/emailHtml";
import { formatMessageListPreview } from "./lib/mailPreview";
import { extractVerificationCode } from "./lib/verificationCode";
import { parseAccountRows, rawWithDefaultProvider } from "./lib/importParser";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import {
  accountProviderDefinition,
  accountProviderLabel,
  accountProviderRegistry,
  normalizeAccountProviderId,
  providerCapabilitySummary,
  providerFailureHint,
  providerReadiness,
  providerAccountType,
  providerDefaultImap
} from "./lib/providerRegistry";
import type {
  Account,
  AppStatus,
  AutomationObservability,
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
  MailShareRecord,
  OAuthSaveAccountInput,
  OAuthSaveAccountResult,
  OAuthTokenResult,
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
  CloudflareChannel,
  UpdateLoginPasswordInput,
  WorkspaceKeyRecord
} from "./types";

type View = "mail" | "accounts" | "refresh" | "automation" | "temp" | "projects" | "settings";
type MailFilters = {
  search: string;
  readState: "all" | "read" | "unread";
  attachmentFilter: "all" | "attachments" | "plain";
  sortBy: "date" | "subject" | "sender" | "read" | "attachments" | "folder";
  sortOrder: "asc" | "desc";
};
type AccountCredentialFilter = "all" | "outlook" | "gmail" | "qq" | "netease_163" | "imap";
type OAuthAuthUrlRequest = { client_id: string; redirect_uri: string; login_hint?: string; provider?: string; code_verifier?: string };
type OAuthTokenExchangeRequest = { account_id?: number; client_id: string; redirect_uri: string; code_or_url: string; provider?: string; code_verifier?: string };

const claudeAccent = "#b5725f";
const colors = ["#111111", "#b5725f", "#8a7a70", "#4a4a45", "#c05f42", "#e0a17f"];
const mailPageSize = 25;
const mailSearchDebounceMs = 450;
const defaultGraphClientId = "6daa9f56-5e67-4cb6-ae52-ef89ef912d36";
const defaultOAuthRedirectUri = "http://localhost:8080";
const loginWindowSize = { width: 600, height: 600, minWidth: 600, minHeight: 600 };
const workspaceWindowSize = { width: 1360, height: 860, minWidth: 1100, minHeight: 720 };
const themePresets = [
  { id: "default", label: "默认", rail: "#f3f0ea", railText: "#46413a", surface: "#ffffff", subtle: "#faf9f8" },
  { id: "graphite", label: "石墨", rail: "#ecebea", railText: "#3a3a38", surface: "#ffffff", subtle: "#f3f2f0" },
  { id: "ocean", label: "纸张", rail: "#eaeae8", railText: "#2d2926", surface: "#ffffff", subtle: "#f8f8f6" },
  { id: "forest", label: "晨雾", rail: "#e5e3da", railText: "#383630", surface: "#ffffff", subtle: "#eeede6" },
  { id: "rose", label: "暖灰", rail: "#ebe5e0", railText: "#423c38", surface: "#ffffff", subtle: "#f1ece8" }
];

type SkinStyle = CSSProperties & Record<`--${string}`, string>;

function buildSkin(settings: Settings | null): { className: string; style: SkinStyle } {
  const preset = themePresets.find((item) => item.id === settings?.appearance_theme) ?? themePresets[0];
  const accent = normalizeAccent(settings?.accent_color);
  return {
    className: `skin-${preset.id}`,
    style: {
      "--skin-rail": preset.rail,
      "--skin-rail-text": preset.railText,
      "--skin-surface": preset.surface,
      "--skin-subtle": preset.subtle,
      "--skin-accent": accent,
      "--skin-accent-soft": `${accent}18`,
      "--skin-accent-border": `${accent}40`
    }
  };
}

function normalizeAccent(value?: string) {
  const normalized = /^#[0-9a-fA-F]{6}$/.test(value ?? "") ? value! : claudeAccent;
  return normalized.toLowerCase() === "#2563eb" ? claudeAccent : normalized;
}

type TauriRuntimeWindow = Window & { __TAURI_INTERNALS__?: unknown };

function isTauriRuntime() {
  return typeof window !== "undefined" && Boolean((window as TauriRuntimeWindow).__TAURI_INTERNALS__);
}

async function applyAuthWindowMode(unlocked: boolean) {
  if (!isTauriRuntime()) return;
  const target = unlocked ? workspaceWindowSize : loginWindowSize;
  const appWindow = getCurrentWindow();

  if (await appWindow.isMaximized().catch(() => false)) {
    await appWindow.unmaximize().catch(() => undefined);
  }

  await appWindow.setDecorations(false).catch(() => undefined);
  await appWindow.setResizable(true).catch(() => undefined);
  await appWindow.setMaxSize(null).catch(() => undefined);

  await appWindow.setMinSize(new LogicalSize(target.minWidth, target.minHeight));
  await appWindow.setSize(new LogicalSize(target.width, target.height));
  await appWindow.center();
}

function searchTokens(value: string) {
  return value
    .trim()
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean);
}

function matchesSearchTokens(parts: Array<string | number | null | undefined>, tokens: string[]) {
  if (tokens.length === 0) return true;
  const haystack = parts
    .filter((part) => part !== null && part !== undefined)
    .join(" ")
    .toLowerCase();
  return tokens.every((token) => haystack.includes(token));
}

function accountMatchesSearch(account: Account, tokens: string[]) {
  return matchesSearchTokens(
    [
      account.email,
      account.remark,
      account.group_name,
      account.provider,
      accountProviderLabel(account.provider),
      accountProviderDefinition(account.provider).credentialLabel,
      account.account_type,
      account.status,
      formatStatus(account.status),
      account.last_refresh_status,
      formatStatus(account.last_refresh_status),
      account.last_refresh_error,
      account.forward_enabled ? "转发 已启用 enabled forwarding" : "未转发 disabled",
      account.has_client_id ? "Client ID" : "",
      account.has_refresh_token ? "Outlook Graph OAuth" : "",
      account.has_imap_password ? "IMAP" : "",
      providerReadiness(account).label,
      providerReadiness(account).detail,
      ...account.aliases,
      ...account.tags.map((tag) => tag.name)
    ],
    tokens
  );
}

function groupMatchesSearch(group: Group, tokens: string[]) {
  return matchesSearchTokens([group.name, group.description, group.proxy_url], tokens);
}

function accountMatchesCredentialFilter(account: Account, filter: AccountCredentialFilter) {
  if (filter === "all") return true;
  const provider = normalizeAccountProviderId(account.provider);
  if (filter === "outlook") return provider === "graph" || account.provider === "outlook";
  if (filter === "imap") return provider === "imap" || provider === "imap_custom";
  return provider === filter;
}

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
  const [workspaceKeyRecords, setWorkspaceKeyRecords] = useState<WorkspaceKeyRecord[]>([]);
  const [mailShareRecords, setMailShareRecords] = useState<MailShareRecord[]>([]);
  const [automationRuns, setAutomationRuns] = useState<AutomationRun[]>([]);
  const [retryQueue, setRetryQueue] = useState<RetryQueueItem[]>([]);
  const [refreshLogs, setRefreshLogs] = useState<RefreshLog[]>([]);
  const [automationObservability, setAutomationObservability] = useState<AutomationObservability | null>(null);
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
  const [mailTotalCount, setMailTotalCount] = useState(0);
  const [selectedMessageIds, setSelectedMessageIds] = useState<number[]>([]);
  const [view, setView] = useState<View>("mail");
  const [railExpanded, setRailExpanded] = useState(false);
  const [railMenuOpen, setRailMenuOpen] = useState(false);
  const railRef = useRef<HTMLElement | null>(null);
  const railMenuRef = useRef<HTMLDivElement | null>(null);
  const railMenuPopupRef = useRef<HTMLDivElement | null>(null);
  const [railMenuStyle, setRailMenuStyle] = useState<CSSProperties>({});
  const [busy, setBusy] = useState(false);
  const [busyMessage, setBusyMessage] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [toast, setToast] = useState<ToastMessage | null>(null);

  const selectedAccount = accounts.find((account) => account.id === selectedAccountId);
  const selectedMessage = messages.find((message) => message.id === selectedMessageId);
  const selectedTempMessage = tempMessages.find((message) => message.message_id === selectedTempMessageId);
  const railIdentity = selectedAccount?.email ?? accounts[0]?.email ?? "管理员";
  const railInitial = railIdentity === "管理员" ? "管" : railIdentity.slice(0, 1).toUpperCase();
  const skin = buildSkin(settings);

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

  useEffect(() => {
    if (!railMenuOpen) return;

    let raf = 0;
    const updatePosition = () => {
      const anchor = railMenuRef.current;
      const menu = railMenuPopupRef.current;
      if (!anchor || !menu) return;

      const anchorRect = anchor.getBoundingClientRect();
      const menuRect = menu.getBoundingClientRect();
      const viewportW = window.innerWidth;
      const viewportH = window.innerHeight;

      const gap = 10;
      const preferredTop = anchorRect.top - menuRect.height - gap;
      const canPlaceAbove = preferredTop >= 8;
      const topCandidate = canPlaceAbove ? preferredTop : anchorRect.bottom + gap;
      const top = Math.min(Math.max(8, topCandidate), Math.max(8, viewportH - menuRect.height - 8));

      const left = Math.min(
        Math.max(8, anchorRect.left),
        Math.max(8, viewportW - menuRect.width - 8)
      );

      setRailMenuStyle({ top, left });
    };

    const schedule = () => {
      cancelAnimationFrame(raf);
      raf = requestAnimationFrame(updatePosition);
    };

    schedule();
    window.addEventListener("resize", schedule);
    window.addEventListener("scroll", schedule, true);
    railRef.current?.addEventListener("scroll", schedule, { passive: true });

    return () => {
      cancelAnimationFrame(raf);
      window.removeEventListener("resize", schedule);
      window.removeEventListener("scroll", schedule, true);
      railRef.current?.removeEventListener("scroll", schedule);
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

  async function loadMailboxMessages(
    accountId = selectedAccountId,
    nextFolder = folder,
    filters = mailFilters,
    page = mailPage,
    options: { preservePreview?: boolean } = {}
  ) {
    const query = buildMailQuery(accountId, nextFolder, filters, page);
    const countQuery = { ...query, limit: undefined, offset: undefined };
    const [nextMessages, nextTotalCount] = await Promise.all([
      api.listMessages(accountId, nextFolder, query),
      api.countMessages(accountId, nextFolder, countQuery)
    ]);
    if (page > 0 && nextMessages.length === 0 && nextTotalCount > 0) {
      const lastPage = Math.max(0, Math.ceil(nextTotalCount / mailPageSize) - 1);
      setMailPage(lastPage);
      return loadMailboxMessages(accountId, nextFolder, filters, lastPage, options);
    }
    setMessages(nextMessages);
    setMailTotalCount(nextTotalCount);
    setSelectedMessageId((current) => {
      if (!options.preservePreview) return undefined;
      return nextMessages.some((message) => message.id === current) ? current : undefined;
    });
    setSelectedMessageIds([]);
  }

  async function loadStatus() {
    setStatus(await api.status());
  }

  async function loadWorkspace(
    accountId: number | undefined | null = selectedAccountId,
    nextFolder = folder,
    filters = mailFilters,
    page = mailPage,
    options: { preservePreview?: boolean } = {}
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
    const query = buildMailQuery(firstAccountId, nextFolder, filters, page);
    const countQuery = { ...query, limit: undefined, offset: undefined };
    const [nextMessages, nextTotalCount] = await Promise.all([
      api.listMessages(firstAccountId, nextFolder, query),
      api.countMessages(firstAccountId, nextFolder, countQuery)
    ]);
    if (page > 0 && nextMessages.length === 0 && nextTotalCount > 0) {
      const lastPage = Math.max(0, Math.ceil(nextTotalCount / mailPageSize) - 1);
      setMailPage(lastPage);
      return loadWorkspace(accountId, nextFolder, filters, lastPage, options);
    }
    setMessages(nextMessages);
    setMailTotalCount(nextTotalCount);
    setSelectedMessageId((current) => {
      if (!options.preservePreview) return undefined;
      return nextMessages.some((message) => message.id === current) ? current : undefined;
    });
    setSelectedMessageIds([]);
  }

  async function loadProjects(projectId?: number) {
    const nextProjects = await api.listProjects();
    setProjects(nextProjects);
    const selectedProject = nextProjects.find((project) => project.id === projectId) ?? nextProjects[0];
    setProjectAccounts(selectedProject ? await api.listProjectAccounts(selectedProject.id) : []);
  }

  async function loadMailShares() {
    setMailShareRecords(await api.listMailShareRecords(80));
  }

  async function loadAutomation() {
    const [
      nextSettings,
      nextForwardingLogs,
      nextBackupLogs,
      nextWorkspaceKeyRecords,
      nextAutomationRuns,
      nextRetryQueue,
      nextRefreshLogs,
      nextSchedulerStatus,
      nextAutomationObservability,
      nextLocalRetention
    ] = await Promise.all([
      api.getSettings(),
      api.listForwardingLogs(80),
      api.listBackupLogs(40),
      api.listWorkspaceKeyRecords(),
      api.listAutomationRuns({}, 80),
      api.listRetryQueue({}, 80),
      api.listRefreshLogs(null, 100),
      api.schedulerStatus(),
      api.getAutomationObservability(),
      api.getLocalRetentionSummary()
    ]);
    setSettings(nextSettings);
    setForwardingLogs(nextForwardingLogs);
    setBackupLogs(nextBackupLogs);
    setWorkspaceKeyRecords(nextWorkspaceKeyRecords);
    setAutomationRuns(nextAutomationRuns);
    setRetryQueue(nextRetryQueue);
    setRefreshLogs(nextRefreshLogs);
    setSchedulerStatus(nextSchedulerStatus);
    setAutomationObservability(nextAutomationObservability);
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
    if (!status) return;
    applyAuthWindowMode(status.unlocked).catch(() => undefined);
  }, [status?.unlocked]);

  useEffect(() => {
    if (!status?.unlocked) return;
    loadWorkspace().catch((err) => setError(readError(err)));
    loadProjects().catch((err) => setError(readError(err)));
    loadMailShares().catch((err) => setError(readError(err)));
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

  useEffect(() => {
    if (!toast) return;
    const timeoutId = window.setTimeout(() => setToast(null), 2200);
    return () => window.clearTimeout(timeoutId);
  }, [toast?.id]);

  function showToast(message: string) {
    setToast({ id: Date.now(), message });
  }

  async function runAction(action: () => Promise<void>, success?: string, loadingMessage = "处理中...") {
    setBusy(true);
    setBusyMessage(loadingMessage);
    setError(null);
    setNotice(null);
    try {
      await action();
      if (success) setNotice(success);
    } catch (err) {
      setError(readError(err));
    } finally {
      setBusy(false);
      setBusyMessage("");
    }
  }

  async function importAccounts(raw: string, groupId: number | null) {
    setBusy(true);
    setBusyMessage("正在导入账号...");
    setError(null);
    setNotice(null);
    try {
      const result = await api.importAccounts({ raw, group_id: groupId });
      const importedAccounts = result.accounts ?? [];
      let refreshSucceeded = 0;
      let refreshFailed = 0;
      for (let index = 0; index < importedAccounts.length; index += 1) {
        const account = importedAccounts[index];
        setBusyMessage(`正在刷新导入账号 ${index + 1}/${importedAccounts.length}：${account.email}`);
        try {
          const refreshResult = await api.runRefreshJob(account.id, "all", 0);
          if (refreshResult.success) refreshSucceeded += 1;
          else refreshFailed += 1;
        } catch {
          refreshFailed += 1;
        }
      }
      const focusAccountId = importedAccounts[0]?.id ?? selectedAccountId;
      setMailPage(0);
      await loadWorkspace(focusAccountId, folder, mailFilters, 0);
      await loadStatus();
      await loadAutomation();
      setNotice(
        importedAccounts.length > 0
          ? `账号已导入 ${result.imported} 个，刷新成功 ${refreshSucceeded} 个，失败 ${refreshFailed} 个`
          : `账号已导入 ${result.imported} 个`
      );
    } catch (err) {
      setError(readError(err));
      throw err;
    } finally {
      setBusy(false);
      setBusyMessage("");
    }
  }

  async function saveOAuthAccount(input: OAuthSaveAccountInput) {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const result = await api.saveOAuthAccount(input);
      setNotice(`OAuth 账号已保存：${result.account.email}（${result.refresh_token_preview}）`);
      await loadWorkspace(result.account.id, folder);
      await loadStatus();
      return result;
    } catch (err) {
      setError(readError(err));
      throw err;
    } finally {
      setBusy(false);
    }
  }

  async function copyVerificationCode(message: MailMessage) {
    const code = extractVerificationCode(message);
    if (!code) {
      setNotice(null);
      setError("未识别到验证码");
      return;
    }
    try {
      await writeTextToClipboard(code);
      setError(null);
      setNotice(null);
      showToast(`验证码已复制：${code}`);
    } catch {
      setNotice(null);
      setError("复制验证码失败，请打开邮件后手动复制");
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
      <LoginScreen
        initialized={status.initialized}
        busy={busy}
        error={error}
        onSubmit={(username, password) =>
          runAction(async () => {
            setStatus(await api.login({ username, password }));
          }, undefined, "正在登录...")
        }
      />
    );
  }

  return (
    <div className={`appContainer ${skin.className}`} style={skin.style}>
      <AppWindowChrome />
      <div className={railExpanded ? "appShell railExpanded" : "appShell"}>
        {busy && <GlobalLoadingOverlay message={busyMessage || "处理中..."} />}
      <aside className={railExpanded ? "rail expanded" : "rail"} ref={railRef}>
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
          active={view === "automation"}
          title="自动化"
          onClick={() => {
            setView("automation");
            setRailMenuOpen(false);
          }}
        >
          <Activity size={20} />
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
            <div className="railAccountMenu" ref={railMenuPopupRef} style={railMenuStyle}>
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
        <Toast toast={toast} />
        <header className="topBar">
          <div>
            <h1>OutlookEmail 桌面版</h1>
            <p>{status.account_count} 个账号 · {status.message_count} 封缓存邮件</p>
          </div>
          <div className="topActions">
            {notice && <span className="notice" title={notice}>{notice}</span>}
            {error && <span className="errorText" title={error}>{error}</span>}
          </div>
        </header>

        {view === "mail" && (
          <MailWorkspace
            groups={groups}
            settings={settings}
            accounts={accounts}
            messages={messages}
            selectedGroupId={selectedGroupId}
            selectedAccountId={selectedAccountId}
            selectedMessage={selectedMessage}
            selectedMessageIds={selectedMessageIds}
            mailShareRecords={mailShareRecords}
            folder={folder}
            filters={mailFilters}
            page={mailPage}
            totalCount={mailTotalCount}
            busy={busy}
            onGroupChange={(groupId) => {
              setSelectedGroupId(groupId);
              const groupAccountIds =
                groupId === "all" ? null : new Set<number>([groupId, ...Array.from(collectDescendantGroupIds(groups, groupId))]);
              const nextAccount =
                groupId === "all"
                  ? accounts[0]
                  : accounts.find((account) => account.group_id !== null && groupAccountIds?.has(account.group_id));
              setSelectedAccountId(nextAccount?.id);
              setMailPage(0);
              void runAction(async () => loadMailboxMessages(nextAccount?.id, folder, mailFilters, 0));
            }}
            onAccountSelect={(accountId) =>
              runAction(async () => {
                const account = accounts.find((item) => item.id === accountId);
                setSelectedAccountId(accountId);
                setSelectedGroupId(account?.group_id ?? "all");
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
            onRefreshCurrentAccount={() =>
              runAction(
                async () => {
                  if (!selectedAccountId) return;
                  const result = await api.runRefreshJob(selectedAccountId);
                  setNotice(formatResultMessage(result.message));
                  await loadWorkspace(selectedAccountId, folder, mailFilters, mailPage, { preservePreview: true });
                  await loadStatus();
                },
                undefined,
                "正在刷新当前账号邮件..."
              )
            }
            onMessageSelect={setSelectedMessageId}
            onMessageClose={() => setSelectedMessageId(undefined)}
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
                const lastPage = Math.max(0, Math.ceil(mailTotalCount / mailPageSize) - 1);
                const page = Math.min(Math.max(0, nextPage), lastPage);
                setMailPage(page);
                await loadMailboxMessages(selectedAccountId, folder, mailFilters, page);
              })
            }
            onMarkMessages={(messageIds, isRead) =>
              runAction(async () => {
                const result = await api.markMessagesRead(messageIds, isRead);
                setNotice(formatResultMessage(result.message));
                await loadMailboxMessages(selectedAccountId, folder, mailFilters, mailPage, { preservePreview: true });
                await loadAutomation();
              })
            }
            onDeleteMessages={(messageIds) =>
              runAction(async () => {
                const result = await api.deleteMessages(messageIds);
                setNotice(formatResultMessage(result.message));
                await loadMailboxMessages(selectedAccountId, folder, mailFilters, mailPage, { preservePreview: true });
                await loadStatus();
                await loadAutomation();
              })
            }
            onCopyVerificationCode={(message) => void copyVerificationCode(message)}
            onExportMessages={(messageIds) =>
              runAction(async () => {
                const result = await api.exportMailMessages(messageIds, "OutlookEmail 邮件导出");
                setNotice(exportNotice(result));
              })
            }
            onCreateMailShare={(messageIds, expiresInDays) =>
              runAction(async () => {
                const result = await api.createMailShare(messageIds, "OutlookEmail 本地分享", expiresInDays);
                setNotice(`已创建本地分享：${result.file_name}`);
                await loadMailShares();
              })
            }
            onRevokeMailShare={(shareId) =>
              runAction(async () => {
                await api.revokeMailShare(shareId);
                await loadMailShares();
                setNotice("本地分享已撤销");
              })
            }
            onImportAccounts={importAccounts}
            onGenerateOAuthUrl={(input) => api.generateOAuthAuthUrl(input)}
            onPreviewOAuthToken={(input) => api.exchangeOAuthToken(input)}
            onSaveOAuthAccount={saveOAuthAccount}
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
                await loadMailboxMessages(selectedAccountId, folder, mailFilters, mailPage, { preservePreview: true });
                await loadAutomation();
              })
            }
            onDismissRemoteFailure={(retryId) =>
              runAction(async () => {
                const result = await api.dismissRetryItem(retryId);
                setNotice(formatResultMessage(result.message));
                await loadMailboxMessages(selectedAccountId, folder, mailFilters, mailPage, { preservePreview: true });
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
            onCreateGroup={(input) =>
              runAction(async () => {
                await api.createGroup(input);
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
            onExportAccountSecrets={(accountIds, password, confirm) =>
              runAction(async () => {
                const result = await api.exportAccountSecrets(accountIds, password, confirm);
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
              runAction(
                async () => {
                  const result = await api.runRefreshJob(accountId);
                  setNotice(formatResultMessage(result.message));
                  await loadWorkspace(accountId, folder, mailFilters, mailPage);
                  await loadAutomation();
                  await loadStatus();
                },
                undefined,
                "正在刷新账号邮件..."
              )
            }
            onRefreshAll={() =>
              runAction(
                async () => {
                  const result = await api.runRefreshJob(undefined);
                  setNotice(formatResultMessage(result.message));
                  await loadWorkspace(selectedAccountId, folder, mailFilters, mailPage);
                  await loadAutomation();
                  await loadStatus();
                },
                undefined,
                "正在刷新全部账号邮件..."
              )
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

        {view === "automation" && (
          <AutomationDashboardView
            observability={automationObservability}
            automationRuns={automationRuns}
            retryQueue={retryQueue}
            schedulerStatus={schedulerStatus}
            busy={busy}
            onFilterAutomationRuns={(query) =>
              runAction(async () => {
                setAutomationRuns(await api.listAutomationRuns(query, 80));
                setAutomationObservability(await api.getAutomationObservability());
              })
            }
            onClearAutomationRuns={(query) =>
              runAction(async () => {
                const result = await api.clearAutomationRuns(query);
                setNotice(formatResultMessage(result.message));
                setAutomationRuns(await api.listAutomationRuns(query, 80));
                setAutomationObservability(await api.getAutomationObservability());
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
            workspaceKeyRecords={workspaceKeyRecords}
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
            onUpdateLoginPassword={async (input) => {
              let succeeded = false;
              await runAction(
                async () => {
                  await api.updateLoginPassword(input);
                  succeeded = true;
                },
                "登录密码已更新",
                "正在修改登录密码..."
              );
              return succeeded;
            }}
            onGenerateWorkspaceKey={(purpose) => api.generateWorkspaceKey({ purpose })}
            onUpdateWorkspaceKeyRecord={async (recordId, purpose) => {
              const updated = await api.updateWorkspaceKeyRecord({ id: recordId, purpose });
              setWorkspaceKeyRecords(await api.listWorkspaceKeyRecords());
              return updated;
            }}
            onRefreshWorkspaceKeyRecords={async () => {
              setWorkspaceKeyRecords(await api.listWorkspaceKeyRecords());
            }}
            onShowToast={showToast}
            onDeleteWorkspaceKeyRecord={async (recordId) => {
              await api.deleteWorkspaceKeyRecord(recordId);
              setWorkspaceKeyRecords(await api.listWorkspaceKeyRecords());
            }}
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
                await loadMailShares();
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
                await loadMailShares();
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
    </div>
  );
}

function AppWindowChrome() {
  const appWindow = isTauriRuntime() ? getCurrentWindow() : null;
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    if (!appWindow) return;
    let disposed = false;

    appWindow.isMaximized().then((value) => {
      if (!disposed) setMaximized(value);
    });

    const unlistenPromise = appWindow.onResized(() => {
      appWindow.isMaximized().then((value) => {
        if (!disposed) setMaximized(value);
      });
    });

    return () => {
      disposed = true;
      unlistenPromise.then((unlisten) => unlisten()).catch(() => undefined);
    };
  }, [appWindow]);

  if (!appWindow) return null;

  return (
    <header className="appChrome">
      <div className="appChromeTitle">OutlookEmail Desktop</div>
      <div
        className="appChromeDrag"
        data-tauri-drag-region
        onDoubleClick={() => appWindow.toggleMaximize()}
      />
      <div className="appChromeControls">
        <button
          type="button"
          className="appChromeButton"
          aria-label="最小化"
          onClick={() => appWindow.minimize()}
        >
          <Minus size={14} strokeWidth={2.1} />
        </button>
        <button
          type="button"
          className="appChromeButton"
          aria-label={maximized ? "向下还原" : "最大化"}
          onClick={() => appWindow.toggleMaximize()}
        >
          {maximized ? <Copy size={13} strokeWidth={2.1} /> : <Square size={12} strokeWidth={2.1} />}
        </button>
        <button
          type="button"
          className="appChromeButton appChromeClose"
          aria-label="关闭"
          onClick={() => appWindow.close()}
        >
          <X size={14} strokeWidth={2.1} />
        </button>
      </div>
    </header>
  );
}

function LoginWindowChrome() {
  const appWindow = isTauriRuntime() ? getCurrentWindow() : null;
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    if (!appWindow) return;
    let disposed = false;

    appWindow.isMaximized().then((value) => {
      if (!disposed) setMaximized(value);
    });

    const unlistenPromise = appWindow.onResized(() => {
      appWindow.isMaximized().then((value) => {
        if (!disposed) setMaximized(value);
      });
    });

    return () => {
      disposed = true;
      unlistenPromise.then((unlisten) => unlisten()).catch(() => undefined);
    };
  }, [appWindow]);

  if (!appWindow) return null;

  return (
    <header className="loginChrome">
      <button type="button" className="loginChromeButton" aria-label="菜单">
        <Menu size={16} strokeWidth={2.1} />
      </button>
      <div className="loginChromeDrag" data-tauri-drag-region />
      <div className="loginChromeControls">
        <button type="button" className="loginChromeButton" aria-label="最小化" onClick={() => appWindow.minimize()}>
          <Minus size={14} strokeWidth={2.1} />
        </button>
        <button
          type="button"
          className="loginChromeButton"
          aria-label={maximized ? "还原" : "最大化"}
          onClick={() => appWindow.toggleMaximize()}
        >
          {maximized ? <Copy size={13} strokeWidth={2.1} /> : <Square size={12} strokeWidth={2.1} />}
        </button>
        <button type="button" className="loginChromeButton loginChromeClose" aria-label="关闭" onClick={() => appWindow.close()}>
          <X size={14} strokeWidth={2.1} />
        </button>
      </div>
    </header>
  );
}

function LoginScreen({
  initialized,
  busy,
  error,
  onSubmit
}: {
  initialized: boolean;
  busy: boolean;
  error: string | null;
  onSubmit: (username: string, password: string) => void;
}) {
  const [username, setUsername] = useState("admin");
  const [password, setPassword] = useState("admin123");

  useEffect(() => {
    setUsername("admin");
    setPassword("admin123");
  }, [initialized]);

  return (
    <div className="loginShell">
      <LoginWindowChrome />
      <div className="lockScreen">
        <form
          className="lockPanel"
          onSubmit={(event) => {
            event.preventDefault();
            onSubmit(username, password);
          }}
        >
        <div className="lockMark" aria-hidden="true">
          <span />
        </div>
        <div className="lockCopy">
          <h1>OutlookEmail</h1>
        </div>
        <label className="lockField">
          <span>账号</span>
          <input
            className="input"
            value={username}
            autoComplete="username"
            placeholder="admin"
            onChange={(event) => setUsername(event.target.value)}
          />
        </label>
        <label className="lockField">
          <span>密码</span>
          <input
            className="input"
            type="password"
            minLength={8}
            value={password}
            autoComplete={initialized ? "current-password" : "new-password"}
            placeholder="admin123"
            onChange={(event) => setPassword(event.target.value)}
          />
        </label>
        {error && <div className="formError">{error}</div>}
        <button className="button primary" disabled={busy || !username.trim() || password.length < 8}>
          {busy && <Loader2 className="spin" size={16} />}
          登录
        </button>
        </form>
      </div>
    </div>
  );
}

function GlobalLoadingOverlay({ message }: { message: string }) {
  return (
    <div className="globalLoadingOverlay" role="status" aria-live="polite">
      <div className="globalLoadingPanel">
        <Loader2 className="spin" size={24} />
        <strong>{message}</strong>
      </div>
    </div>
  );
}

function MailWorkspace({
  groups,
  settings,
  accounts,
  messages,
  selectedGroupId,
  selectedAccountId,
  selectedMessage,
  selectedMessageIds,
  mailShareRecords,
  folder,
  filters,
  page,
  totalCount,
  busy,
  onGroupChange,
  onAccountSelect,
  onFolderChange,
  onRefreshCurrentAccount,
  onMessageSelect,
  onMessageClose,
  onToggleMessageSelect,
  onSelectVisibleMessages,
  onClearSelection,
  onFilterApply,
  onPageChange,
  onMarkMessages,
  onDeleteMessages,
  onCopyVerificationCode,
  onExportMessages,
  onCreateMailShare,
  onRevokeMailShare,
  onImportAccounts,
  onGenerateOAuthUrl,
  onPreviewOAuthToken,
  onSaveOAuthAccount,
  onDownloadAttachment,
  onDownloadAllAttachments,
  onViewRawMessage,
  onRetryRemoteFailure,
  onDismissRemoteFailure
}: {
  groups: Group[];
  settings: Settings | null;
  accounts: Account[];
  messages: MailMessage[];
  selectedGroupId: number | "all";
  selectedAccountId?: number;
  selectedMessage?: MailMessage;
  selectedMessageIds: number[];
  mailShareRecords: MailShareRecord[];
  folder: string;
  filters: MailFilters;
  page: number;
  totalCount: number;
  busy: boolean;
  onGroupChange: (groupId: number | "all") => void;
  onAccountSelect: (accountId: number) => void;
  onFolderChange: (folder: string) => void;
  onRefreshCurrentAccount: () => void;
  onMessageSelect: (messageId: number) => void;
  onMessageClose: () => void;
  onToggleMessageSelect: (messageId: number) => void;
  onSelectVisibleMessages: () => void;
  onClearSelection: () => void;
  onFilterApply: (filters: MailFilters) => void;
  onPageChange: (page: number) => void;
  onMarkMessages: (messageIds: number[], isRead: boolean) => void;
  onDeleteMessages: (messageIds: number[]) => void;
  onCopyVerificationCode: (message: MailMessage) => void;
  onExportMessages: (messageIds: number[]) => void;
  onCreateMailShare: (messageIds: number[], expiresInDays: number) => void;
  onRevokeMailShare: (shareId: number) => void;
  onImportAccounts: (raw: string, groupId: number | null) => Promise<void> | void;
  onGenerateOAuthUrl: (input: OAuthAuthUrlRequest) => Promise<string>;
  onPreviewOAuthToken: (input: OAuthTokenExchangeRequest) => Promise<OAuthTokenResult>;
  onSaveOAuthAccount: (input: OAuthSaveAccountInput) => Promise<OAuthSaveAccountResult>;
  onDownloadAttachment: (message: MailMessage, attachmentId: string) => void | Promise<void>;
  onDownloadAllAttachments: (message: MailMessage) => Promise<void>;
  onViewRawMessage: (message: MailMessage) => Promise<MailRawContent>;
  onRetryRemoteFailure: (retryId: number) => void;
  onDismissRemoteFailure: (retryId: number) => void;
}) {
  const totalPages = Math.max(1, Math.ceil(totalCount / mailPageSize));
  const lastPage = totalPages - 1;
  const [draftFilters, setDraftFilters] = useState(filters);
  const [pageInput, setPageInput] = useState(String(page + 1));
  const [downloadingAttachmentId, setDownloadingAttachmentId] = useState<string | null>(null);
  const [downloadingAllAttachments, setDownloadingAllAttachments] = useState(false);
  const [attachmentError, setAttachmentError] = useState<string | null>(null);
  const [rawContent, setRawContent] = useState<MailRawContent | null>(null);
  const [rawBusy, setRawBusy] = useState(false);
  const [rawError, setRawError] = useState<string | null>(null);
  const [importDialogOpen, setImportDialogOpen] = useState(false);
  const [oauthSaveOpen, setOauthSaveOpen] = useState(false);
  const [accountSearch, setAccountSearch] = useState("");
  const [accountCredentialFilter, setAccountCredentialFilter] = useState<AccountCredentialFilter>("all");
  const searchApplyTimerRef = useRef<number | null>(null);
  const selectedCount = selectedMessageIds.length;
  const selectedAccount = accounts.find((account) => account.id === selectedAccountId);
  const accountMailRetentionDays = Math.max(1, Math.min(3650, selectedAccount?.mail_retention_days ?? 30));
  const visibleShareRecords = useMemo(
    () =>
      selectedAccountId
        ? mailShareRecords.filter((record) => record.account_id === selectedAccountId)
        : mailShareRecords,
    [mailShareRecords, selectedAccountId]
  );
  const groupIds = useMemo(() => new Set(groups.map((group) => group.id)), [groups]);
  const childGroupsByParent = useMemo(() => {
    const map = new Map<number | null, Group[]>();
    groups.forEach((group) => {
      const key = group.parent_id ?? null;
      const list = map.get(key) ?? [];
      list.push(group);
      map.set(key, list);
    });
    return map;
  }, [groups]);
  const accountsByGroup = useMemo(() => {
    const map = new Map<number | null, Account[]>();
    accounts.forEach((account) => {
      const key = account.group_id !== null && groupIds.has(account.group_id) ? account.group_id : null;
      const list = map.get(key) ?? [];
      list.push(account);
      map.set(key, list);
    });
    return map;
  }, [accounts, groupIds]);
  const accountSearchTokens = useMemo(() => searchTokens(accountSearch), [accountSearch]);
  const accountSearchActive = accountSearchTokens.length > 0;
  const accountCredentialFilterActive = accountCredentialFilter !== "all";
  const accountFilterActive = accountSearchActive || accountCredentialFilterActive;
  const accountTree = useMemo(() => {
    if (!accountFilterActive) {
      return { childGroupsByParent, accountsByGroup };
    }

    const groupById = new Map(groups.map((group) => [group.id, group]));
    const visibleGroupIds = new Set<number>();
    const visibleAccountIds = new Set<number>();

    const normalizeGroupId = (groupId: number | null) => (groupId !== null && groupById.has(groupId) ? groupId : null);
    const markGroupAncestors = (groupId: number | null) => {
      const visited = new Set<number>();
      let currentId = normalizeGroupId(groupId);
      while (currentId !== null && !visited.has(currentId)) {
        visited.add(currentId);
        visibleGroupIds.add(currentId);
        currentId = normalizeGroupId(groupById.get(currentId)?.parent_id ?? null);
      }
    };

    const markGroupAndDescendants = (groupId: number, visited = new Set<number>()) => {
      if (!groupById.has(groupId) || visited.has(groupId)) return;
      visited.add(groupId);
      visibleGroupIds.add(groupId);
      (accountsByGroup.get(groupId) ?? []).forEach((account) => {
        if (accountMatchesCredentialFilter(account, accountCredentialFilter)) visibleAccountIds.add(account.id);
      });
      (childGroupsByParent.get(groupId) ?? []).forEach((child) => markGroupAndDescendants(child.id, visited));
    };

    accounts.forEach((account) => {
      if (!accountMatchesCredentialFilter(account, accountCredentialFilter) || !accountMatchesSearch(account, accountSearchTokens)) return;
      visibleAccountIds.add(account.id);
      markGroupAncestors(account.group_id);
    });

    groups.forEach((group) => {
      if (!accountSearchActive || !groupMatchesSearch(group, accountSearchTokens)) return;
      markGroupAncestors(group.id);
      markGroupAndDescendants(group.id);
    });

    const nextChildGroupsByParent = new Map<number | null, Group[]>();
    groups.forEach((group) => {
      if (!visibleGroupIds.has(group.id)) return;
      const key = group.parent_id !== null && visibleGroupIds.has(group.parent_id) ? group.parent_id : null;
      const list = nextChildGroupsByParent.get(key) ?? [];
      list.push(group);
      nextChildGroupsByParent.set(key, list);
    });

    const nextAccountsByGroup = new Map<number | null, Account[]>();
    accounts.forEach((account) => {
      if (!visibleAccountIds.has(account.id)) return;
      const key = account.group_id !== null && visibleGroupIds.has(account.group_id) ? account.group_id : null;
      const list = nextAccountsByGroup.get(key) ?? [];
      list.push(account);
      nextAccountsByGroup.set(key, list);
    });

    return { childGroupsByParent: nextChildGroupsByParent, accountsByGroup: nextAccountsByGroup };
  }, [accountCredentialFilter, accountFilterActive, accountSearchActive, accountSearchTokens, accounts, accountsByGroup, childGroupsByParent, groups]);
  const treeChildGroupsByParent = accountTree.childGroupsByParent;
  const treeAccountsByGroup = accountTree.accountsByGroup;
  const hasAccountTreeItems = (treeChildGroupsByParent.get(null)?.length ?? 0) > 0 || (treeAccountsByGroup.get(null)?.length ?? 0) > 0;

  useEffect(() => {
    setDraftFilters(filters);
  }, [filters]);

  useEffect(() => {
    setPageInput(String(page + 1));
  }, [page]);

  useEffect(() => {
    return () => {
      if (searchApplyTimerRef.current !== null) {
        window.clearTimeout(searchApplyTimerRef.current);
      }
    };
  }, []);

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

  function applyDraftFilters(nextFilters: MailFilters) {
    if (searchApplyTimerRef.current !== null) {
      window.clearTimeout(searchApplyTimerRef.current);
      searchApplyTimerRef.current = null;
    }
    setDraftFilters(nextFilters);
    onFilterApply(nextFilters);
  }

  function scheduleDraftFilters(nextFilters: MailFilters) {
    setDraftFilters(nextFilters);
    if (searchApplyTimerRef.current !== null) {
      window.clearTimeout(searchApplyTimerRef.current);
    }
    searchApplyTimerRef.current = window.setTimeout(() => {
      searchApplyTimerRef.current = null;
      onFilterApply(nextFilters);
    }, mailSearchDebounceMs);
  }

  function commitPageInput() {
    const parsed = Number.parseInt(pageInput, 10);
    const targetPage = Number.isFinite(parsed) ? Math.min(Math.max(parsed, 1), totalPages) - 1 : page;
    setPageInput(String(targetPage + 1));
    if (targetPage !== page) onPageChange(targetPage);
  }

  function treeDepthStyle(depth: number): CSSProperties {
    return { "--tree-indent": `${depth * 16}px` } as CSSProperties;
  }

  function visibleTreeGroupAccountCount(group: Group): number {
    if (!accountSearchActive) return group.account_count;
    const directCount = treeAccountsByGroup.get(group.id)?.length ?? 0;
    const childCount = (treeChildGroupsByParent.get(group.id) ?? []).reduce(
      (total, child) => total + visibleTreeGroupAccountCount(child),
      0
    );
    return directCount + childCount;
  }

  function renderTreeAccount(account: Account, depth: number) {
    return (
      <button
        key={`account-${account.id}`}
        className={selectedAccountId === account.id ? "mailTreeRow mailTreeAccount active" : "mailTreeRow mailTreeAccount"}
        style={treeDepthStyle(depth)}
        onClick={() => onAccountSelect(account.id)}
      >
        <span className="mailTreeText">
          <strong>{account.email}</strong>
          <span className="mailTreeMeta">
            <ProviderBadge provider={account.provider} compact showMark={false} />
            <small>{formatStatus(account.last_refresh_status)} · {account.message_count} 封邮件</small>
          </span>
        </span>
      </button>
    );
  }

  function renderTreeGroup(group: Group, depth: number): ReactNode {
    const childGroups = treeChildGroupsByParent.get(group.id) ?? [];
    const groupAccounts = treeAccountsByGroup.get(group.id) ?? [];

    return (
      <div className="mailTreeNode" key={`group-${group.id}`}>
        <button
          className={selectedGroupId === group.id ? "mailTreeRow mailTreeGroup active" : "mailTreeRow mailTreeGroup"}
          style={treeDepthStyle(depth)}
          onClick={() => onGroupChange(group.id)}
        >
          <span className="dot" style={{ backgroundColor: group.color }} />
          <span className="mailTreeLabel">{group.name}</span>
          <small>{visibleTreeGroupAccountCount(group)}</small>
        </button>
        {childGroups.map((child) => renderTreeGroup(child, depth + 1))}
        {groupAccounts.map((account) => renderTreeAccount(account, depth + 1))}
      </div>
    );
  }

  return (
    <section className="workspaceGrid">
      {importDialogOpen && (
        <AccountImportDialog
          groups={groups}
          selectedGroupId={selectedGroupId}
          busy={busy}
          onClose={() => setImportDialogOpen(false)}
          onImport={onImportAccounts}
        />
      )}
      {oauthSaveOpen && (
        <OAuthAccountSaveDialog
          groups={groups}
          settings={settings}
          selectedGroupId={selectedGroupId}
          busy={busy}
          onClose={() => setOauthSaveOpen(false)}
          onGenerateOAuthUrl={onGenerateOAuthUrl}
          onPreviewOAuthToken={onPreviewOAuthToken}
          onSaveOAuthAccount={onSaveOAuthAccount}
        />
      )}
      <aside className="pane mailTreePane">
        <div className="paneHeader">
          <h2>账号</h2>
          <div className="paneHeaderActions">
            <button className="iconMini" title="导入账号" onClick={() => setImportDialogOpen(true)} disabled={busy}>
              <Upload size={16} />
            </button>
            <button className="iconMini" title="授权保存 OAuth 账号" onClick={() => setOauthSaveOpen(true)} disabled={busy}>
              <KeyRound size={16} />
            </button>
          </div>
        </div>
        <div className="accountSearchTools">
          <label className="searchBox accountSearchBox">
            <Search size={15} />
            <input
              value={accountSearch}
              placeholder="搜索账号、分组或备注"
              onChange={(event) => setAccountSearch(event.target.value)}
            />
            {accountSearch && (
              <button className="searchClear" type="button" title="清空搜索" onClick={() => setAccountSearch("")}>
                <X size={14} />
              </button>
            )}
          </label>
          <select
            className="select accountCredentialFilter"
            value={accountCredentialFilter}
            title="按服务商筛选账号"
            onChange={(event) => setAccountCredentialFilter(event.target.value as AccountCredentialFilter)}
          >
            <option value="all">全部</option>
            <option value="outlook">Outlook</option>
            <option value="gmail">Gmail</option>
            <option value="qq">QQ 邮箱</option>
            <option value="netease_163">163 邮箱</option>
            <option value="imap">IMAP</option>
          </select>
        </div>
        {hasAccountTreeItems ? (
          <div className="mailTree">
            {(treeChildGroupsByParent.get(null) ?? []).map((group) => renderTreeGroup(group, 0))}
            {(treeAccountsByGroup.get(null) ?? []).map((account) => renderTreeAccount(account, 0))}
          </div>
        ) : (
          <EmptyState icon={<Mail size={24} />} text={accountFilterActive ? "没有匹配的账号、服务商或分组。" : "导入账号后开始使用。"} />
        )}
      </aside>

      <section className="pane messagePane">
        <div className="paneHeader">
          <h2>邮件</h2>
          <button
            className="button compact secondary"
            disabled={busy || !selectedAccountId}
            title={selectedAccountId ? "刷新当前账号邮件" : "请选择账号后刷新"}
            onClick={onRefreshCurrentAccount}
          >
            {busy ? <Loader2 className="spin" size={14} /> : <RefreshCw size={14} />}
            刷新
          </button>
        </div>
        <div className="messageTools">
          <label className="searchBox messageSearch">
            <Search size={15} />
            <input
              value={draftFilters.search}
              placeholder="搜索发件人、主题、正文"
              onChange={(event) => scheduleDraftFilters({ ...draftFilters, search: event.target.value })}
              onKeyDown={(event) => {
                if (event.key === "Enter") applyDraftFilters(draftFilters);
              }}
            />
          </label>
          <div className="filterRow">
            <select className="select" value={folder} onChange={(event) => onFolderChange(event.target.value)}>
              <option value="all">全部</option>
              <option value="inbox">收件箱</option>
              <option value="junkemail">垃圾邮件</option>
              <option value="deleteditems">已删除</option>
            </select>
            <select
              className="select"
              value={draftFilters.readState}
              onChange={(event) =>
                applyDraftFilters({ ...draftFilters, readState: event.target.value as MailFilters["readState"] })
              }
            >
              <option value="all">全部邮件</option>
              <option value="unread">未读</option>
              <option value="read">已读</option>
            </select>
            <select
              className="select"
              value={draftFilters.attachmentFilter}
              onChange={(event) =>
                applyDraftFilters({ ...draftFilters, attachmentFilter: event.target.value as MailFilters["attachmentFilter"] })
              }
            >
              <option value="all">全部附件状态</option>
              <option value="attachments">有附件</option>
              <option value="plain">无附件</option>
            </select>
            <select
              className="select"
              value={draftFilters.sortBy}
              onChange={(event) =>
                applyDraftFilters({ ...draftFilters, sortBy: event.target.value as MailFilters["sortBy"] })
              }
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
              onChange={(event) =>
                applyDraftFilters({ ...draftFilters, sortOrder: event.target.value as MailFilters["sortOrder"] })
              }
            >
              <option value="desc">降序</option>
              <option value="asc">升序</option>
            </select>
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
            <button className="button compact secondary" onClick={() => onCreateMailShare(selectedMessageIds, accountMailRetentionDays)}>
              <Share2 size={14} />
              分享
            </button>
            <button className="button compact ghost" onClick={onClearSelection}>
              清除
            </button>
          </div>
        )}
        {messages.map((message) => (
          <div key={message.id} className={selectedMessage?.id === message.id ? "messageRow active" : "messageRow"}>
            <input
              className="messageSelect"
              type="checkbox"
              aria-label={`选择 ${message.subject || "邮件"}`}
              checked={selectedMessageIds.includes(message.id)}
              onChange={() => onToggleMessageSelect(message.id)}
            />
            <button className={message.is_read ? "messageOpen" : "messageOpen unread"} onClick={() => onMessageSelect(message.id)}>
              <span className="sender">{formatSenderDisplayName(message.sender)}</span>
              <span className="messageLine">
                <strong className="messageSubject">{message.subject || "（无主题）"}</strong>
                {message.remote_sync_failure && (
                  <span className="remoteFailureInline">
                    <XCircle size={12} />
                    {formatRemoteFailureAction(message.remote_sync_failure.action)} 远端同步失败
                  </span>
                )}
                <span className="preview">{formatMessageListPreview(message)}</span>
              </span>
            </button>
            <div className="messageRowEnd">
              <small className="messageDate">{formatDate(message.received_at)}</small>
              <div className="messageQuickActions" aria-label="邮件快捷操作">
                <button
                  className="iconMini"
                  title="复制验证码"
                  onClick={(event) => {
                    event.stopPropagation();
                    onCopyVerificationCode(message);
                  }}
                >
                  <Copy size={15} />
                </button>
                <button
                  className="iconMini"
                  title={message.is_read ? "标为未读" : "标为已读"}
                  onClick={(event) => {
                    event.stopPropagation();
                    onMarkMessages([message.id], !message.is_read);
                  }}
                >
                  {message.is_read ? <Mail size={15} /> : <CheckCircle2 size={15} />}
                </button>
                <button
                  className="iconMini"
                  title="分享"
                  onClick={(event) => {
                    event.stopPropagation();
                    onCreateMailShare([message.id], accountMailRetentionDays);
                  }}
                >
                  <Share2 size={15} />
                </button>
                <button
                  className="iconMini"
                  title="导出"
                  onClick={(event) => {
                    event.stopPropagation();
                    onExportMessages([message.id]);
                  }}
                >
                  <Download size={15} />
                </button>
                <button
                  className="iconMini danger"
                  title="删除"
                  onClick={(event) => {
                    event.stopPropagation();
                    onDeleteMessages([message.id]);
                  }}
                >
                  <Trash2 size={15} />
                </button>
              </div>
            </div>
          </div>
        ))}
        {messages.length === 0 && <EmptyState icon={<Inbox size={24} />} text="暂无缓存邮件。" />}
        {messages.length > 0 && (
          <div className="pagerBar">
            <button className="button compact secondary" onClick={onSelectVisibleMessages}>
              选择本页
            </button>
            <span className="pagerSummary">
              共 {totalCount} 封 · 第 {page + 1} / {totalPages} 页
            </span>
            <button className="button compact secondary" disabled={page === 0} onClick={() => onPageChange(0)}>
              第一页
            </button>
            <button className="button compact secondary" disabled={page === 0} onClick={() => onPageChange(page - 1)}>
              上一页
            </button>
            <label className="pageJump">
              <span>跳至</span>
              <input
                className="input pageJumpInput"
                type="text"
                inputMode="numeric"
                pattern="[0-9]*"
                value={pageInput}
                onChange={(event) => setPageInput(event.target.value)}
                onBlur={commitPageInput}
                onKeyDown={(event) => {
                  if (event.key === "Enter") commitPageInput();
                }}
              />
            </label>
            <button className="button compact secondary" disabled={page >= lastPage} onClick={() => onPageChange(page + 1)}>
              下一页
            </button>
            <button className="button compact secondary" disabled={page >= lastPage} onClick={() => onPageChange(lastPage)}>
              最后一页
            </button>
          </div>
        )}
      </section>

      {selectedMessage && (
        <div
          className="oauthDialogBackdrop mailPreviewBackdrop"
          onMouseDown={(event) => {
            if (event.target === event.currentTarget) onMessageClose();
          }}
        >
          <article className="mailPreviewDialog" role="dialog" aria-modal="true" aria-label="邮件预览">
            <div className="mailPreviewTopbar">
              <button className="iconMini previewCloseButton" title="关闭预览" onClick={onMessageClose}>
                <X size={18} />
              </button>
            </div>
            <div className="detailHeader">
              <div className="mailPreviewIdentity">
                <div>
                  <strong>{formatSenderDisplayName(selectedMessage.sender) || "未知发件人"}</strong>
                  <span>发送给 {selectedMessage.recipients || "我"}</span>
                </div>
              </div>
              <div className="mailPreviewHeaderActions">
                <time dateTime={selectedMessage.received_at}>{formatDate(selectedMessage.received_at)}</time>
                <div className="detailActions">
                  <button className="button compact secondary" onClick={() => onMarkMessages([selectedMessage.id], !selectedMessage.is_read)}>
                    {selectedMessage.is_read ? <Mail size={14} /> : <CheckCircle2 size={14} />}
                    {selectedMessage.is_read ? "标为未读" : "标为已读"}
                  </button>
                  <button className="button compact secondary" disabled={rawBusy} onClick={() => handleViewRawMessage(selectedMessage)}>
                    {rawBusy ? <Loader2 className="spin" size={14} /> : <FileText size={14} />}
                    Raw
                  </button>
                  <button className="button compact secondary" onClick={() => onCreateMailShare([selectedMessage.id], accountMailRetentionDays)}>
                    <Share2 size={14} />
                    分享
                  </button>
                  <button className="button compact secondary" onClick={() => onExportMessages([selectedMessage.id])}>
                    <Download size={14} />
                    导出
                  </button>
                </div>
              </div>
            </div>
            <div className="mailPreviewContent">
              {selectedMessage.remote_sync_failure && (
                <RemoteFailurePanel
                  failure={selectedMessage.remote_sync_failure}
                  busy={busy}
                  onRetry={onRetryRemoteFailure}
                  onDismiss={onDismissRemoteFailure}
                />
              )}
              <MailSharePanel records={visibleShareRecords} busy={busy} onRevoke={onRevokeMailShare} />
              <div className="mailPreviewSubjectBlock">
                <h2>{selectedMessage.subject || "（无主题）"}</h2>
              </div>
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
            </div>
          </article>
        </div>
      )}
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
  onCreateGroup,
  onUpdateGroup,
  onDeleteGroup,
  onCreateTag,
  onDeleteAccount,
  onBatchAccounts,
  onExportAccounts,
  onExportAccountSecrets,
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
  onCreateGroup: (input: Parameters<typeof api.createGroup>[0]) => void;
  onUpdateGroup: (input: Parameters<typeof api.updateGroup>[0]) => void;
  onDeleteGroup: (groupId: number) => void;
  onCreateTag: (name: string, color: string) => void;
  onDeleteAccount: (accountId: number) => void;
  onBatchAccounts: (input: Parameters<typeof api.batchAccounts>[0]) => void;
  onExportAccounts: (groupId?: number | null, accountIds?: number[]) => void;
  onExportAccountSecrets: (accountIds: number[], password: string, confirm: string) => void;
  onUpdateAccount: (input: Parameters<typeof api.updateAccount>[0]) => void;
  onRevealAccountSecrets: (input: Parameters<typeof api.revealAccountSecrets>[0]) => Promise<Awaited<ReturnType<typeof api.revealAccountSecrets>>>;
  onGenerateOAuthUrl: (input: OAuthAuthUrlRequest) => Promise<string>;
  onExchangeOAuthToken: (input: OAuthTokenExchangeRequest) => void;
}) {
  const [selectedManageGroupId, setSelectedManageGroupId] = useState<number | "all">("all");
  const [groupSettingsOpen, setGroupSettingsOpen] = useState(false);
  const [selectedAccountId, setSelectedAccountId] = useState<number | undefined>(accounts[0]?.id);
  const [authAccountId, setAuthAccountId] = useState<number | undefined>();
  const [accountSearch, setAccountSearch] = useState("");
  const [selectedAccountIds, setSelectedAccountIds] = useState<number[]>([]);
  const [batchGroupId, setBatchGroupId] = useState<number | "">(groups[0]?.id ?? "");
  const [batchTagId, setBatchTagId] = useState<number | "">(tags[0]?.id ?? "");
  const [secretExportPassword, setSecretExportPassword] = useState("");
  const [secretExportConfirm, setSecretExportConfirm] = useState("");
  const accountSearchTokens = useMemo(() => searchTokens(accountSearch), [accountSearch]);
  const visibleAccounts = useMemo(() => {
    return accounts.filter((account) => {
      if (selectedManageGroupId !== "all" && account.group_id !== selectedManageGroupId) return false;
      return accountMatchesSearch(account, accountSearchTokens);
    });
  }, [accounts, accountSearchTokens, selectedManageGroupId]);
  const authAccount = accounts.find((account) => account.id === authAccountId);
  const selectedAccountIdSet = useMemo(() => new Set(selectedAccountIds), [selectedAccountIds]);
  const visibleAccountIds = useMemo(() => visibleAccounts.map((account) => account.id), [visibleAccounts]);
  const allVisibleAccountsSelected =
    visibleAccountIds.length > 0 && visibleAccountIds.every((accountId) => selectedAccountIdSet.has(accountId));
  const selectedAccountCount = selectedAccountIds.length;
  const selectedManageGroup =
    selectedManageGroupId === "all" ? undefined : groups.find((group) => group.id === selectedManageGroupId);

  useEffect(() => {
    if (selectedManageGroupId !== "all" && !groups.some((group) => group.id === selectedManageGroupId)) {
      setSelectedManageGroupId("all");
    }
  }, [groups, selectedManageGroupId]);

  useEffect(() => {
    const accountIds = new Set(accounts.map((account) => account.id));
    setSelectedAccountIds((current) => current.filter((accountId) => accountIds.has(accountId)));
    setSelectedAccountId((current) => (current && accountIds.has(current) ? current : accounts[0]?.id));
    setAuthAccountId((current) => (current && accountIds.has(current) ? current : undefined));
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
    <section className="managementGrid accountManagementGrid">
      {groupSettingsOpen && (
        <GroupSettingsDialog
          groups={groups}
          tags={tags}
          selectedGroup={selectedManageGroup}
          busy={busy}
          onClose={() => setGroupSettingsOpen(false)}
          onSelectGroup={(groupId) => setSelectedManageGroupId(groupId)}
          onCreateGroup={onCreateGroup}
          onUpdateGroup={onUpdateGroup}
          onDeleteGroup={onDeleteGroup}
          onCreateTag={onCreateTag}
        />
      )}
      {authAccount && (
        <AccountAuthDialog
          account={authAccount}
          groups={groups}
          tags={tags}
          settings={settings}
          busy={busy}
          onClose={() => setAuthAccountId(undefined)}
          onSave={(input) => onUpdateAccount(input)}
          onRevealAccountSecrets={(input) => onRevealAccountSecrets(input)}
          onGenerateOAuthUrl={onGenerateOAuthUrl}
          onExchangeOAuthToken={(input) => onExchangeOAuthToken(input)}
        />
      )}

      <aside className="panel groupInventoryPanel">
        <div className="panelHeader">
          <h2>分组</h2>
          <button className="iconMini" title="新增或修改分组" disabled={busy} onClick={() => setGroupSettingsOpen(true)}>
            <SettingsIcon size={15} />
          </button>
        </div>
        <div className="groupTree accountGroupTree" aria-label="分组列表">
          {groups.map((group) => (
            <button
              className={selectedManageGroupId === group.id ? "groupTreeButton active" : "groupTreeButton"}
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
      </aside>

      <section className="panel accountInventoryPanel">
        <div className="panelHeader">
          <h2>账号</h2>
          <div className="rowActions">
            <button className="iconMini" title="导出账号" disabled={accounts.length === 0 || busy} onClick={() => onExportAccounts()}>
              <Download size={15} />
            </button>
          </div>
        </div>
        <label className="searchBox accountInventorySearch">
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
            <div className="secretExportInputs">
              <input
                className="input"
                type="password"
                value={secretExportPassword}
                placeholder="本地密码"
                onChange={(event) => setSecretExportPassword(event.target.value)}
              />
              <input
                className="input"
                value={secretExportConfirm}
                placeholder="EXPORT ACCOUNT SECRETS"
                onChange={(event) => setSecretExportConfirm(event.target.value)}
              />
              <button
                className="button compact danger"
                disabled={busy || !secretExportPassword || secretExportConfirm !== "EXPORT ACCOUNT SECRETS"}
                onClick={() => {
                  onExportAccountSecrets(selectedAccountIds, secretExportPassword, secretExportConfirm);
                  setSecretExportPassword("");
                  setSecretExportConfirm("");
                }}
              >
                <KeyRound size={14} />
                密钥
              </button>
            </div>
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
            <span>服务商</span>
            <span>操作</span>
          </div>
          {visibleAccounts.map((account) => (
            <div
              className={selectedAccountId === account.id ? "tableRow active" : "tableRow"}
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
                {account.aliases.length > 0 && (
                  <span className="accountMetaLine">
                    <small>{account.aliases.join(", ")}</small>
                  </span>
                )}
              </span>
              <span>{account.group_name ?? "无"}</span>
              <span>{formatStatus(account.last_refresh_status)}</span>
              <ProviderBadge provider={account.provider} compact showMark={false} />
              <span className="rowActions accountRowActions">
                <button
                  className="iconMini"
                  title="账号设置"
                  onClick={(event) => {
                    event.stopPropagation();
                    setSelectedAccountId(account.id);
                    setAuthAccountId(account.id);
                  }}
                >
                  <KeyRound size={15} />
                </button>
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
              </span>
            </div>
          ))}
          {visibleAccounts.length === 0 && <div className="tableEmptyRow">当前分组暂无账号</div>}
        </div>
      </section>
    </section>
  );
}

function groupDraftFromGroup(group?: Group): GroupDraft {
  return {
    name: group?.name ?? "",
    description: group?.description ?? "",
    color: group?.color || colors[0],
    parent_id: group?.parent_id ?? "",
    sort_order: String(group?.sort_order ?? 0),
    proxy_url: group?.proxy_url ?? "",
    fallback_proxy_url_1: group?.fallback_proxy_url_1 ?? "",
    fallback_proxy_url_2: group?.fallback_proxy_url_2 ?? ""
  };
}

function GroupSettingsDialog({
  groups,
  tags,
  selectedGroup,
  busy,
  onClose,
  onSelectGroup,
  onCreateGroup,
  onUpdateGroup,
  onDeleteGroup,
  onCreateTag
}: {
  groups: Group[];
  tags: Tag[];
  selectedGroup?: Group;
  busy: boolean;
  onClose: () => void;
  onSelectGroup: (groupId: number | "all") => void;
  onCreateGroup: (input: Parameters<typeof api.createGroup>[0]) => void;
  onUpdateGroup: (input: Parameters<typeof api.updateGroup>[0]) => void;
  onDeleteGroup: (groupId: number) => void;
  onCreateTag: (name: string, color: string) => void;
}) {
  const [mode, setMode] = useState<"create" | "edit">(selectedGroup ? "edit" : "create");
  const [editingGroupId, setEditingGroupId] = useState<number | undefined>(selectedGroup?.id ?? groups[0]?.id);
  const editingGroup = groups.find((group) => group.id === editingGroupId);
  const activeGroup = mode === "edit" ? editingGroup : undefined;
  const [draft, setDraft] = useState<GroupDraft>(groupDraftFromGroup(selectedGroup));
  const [tagName, setTagName] = useState("");
  const [colorIndex, setColorIndex] = useState(0);
  const selectedDescendantGroupIds = useMemo(
    () => (activeGroup ? collectDescendantGroupIds(groups, activeGroup.id) : new Set<number>()),
    [groups, activeGroup?.id]
  );
  const selectedSubtreeDepth = useMemo(
    () => (activeGroup ? groupSubtreeDepth(groups, activeGroup.id) : 0),
    [groups, activeGroup?.id]
  );
  const parentGroupOptions = useMemo(() => {
    if (mode === "create") return groups.filter((group) => group.level < 3);
    if (!activeGroup) return [];
    return groups.filter(
      (group) =>
        group.id !== activeGroup.id &&
        !selectedDescendantGroupIds.has(group.id) &&
        group.level + 1 + selectedSubtreeDepth <= 3
    );
  }, [groups, mode, activeGroup?.id, selectedDescendantGroupIds, selectedSubtreeDepth]);

  useEffect(() => {
    if (mode !== "edit") return;
    if (!editingGroupId || !groups.some((group) => group.id === editingGroupId)) {
      setEditingGroupId(groups[0]?.id);
    }
  }, [groups, editingGroupId, mode]);

  useEffect(() => {
    setDraft(mode === "edit" ? groupDraftFromGroup(activeGroup) : groupDraftFromGroup());
  }, [
    mode,
    activeGroup?.id,
    activeGroup?.name,
    activeGroup?.description,
    activeGroup?.color,
    activeGroup?.parent_id,
    activeGroup?.sort_order,
    activeGroup?.proxy_url,
    activeGroup?.fallback_proxy_url_1,
    activeGroup?.fallback_proxy_url_2
  ]);

  function saveGroup() {
    const sortOrder = Number.parseInt(draft.sort_order, 10);
    const input = {
      name: draft.name,
      description: draft.description,
      color: draft.color,
      parent_id: draft.parent_id === "" ? null : draft.parent_id,
      proxy_url: draft.proxy_url,
      fallback_proxy_url_1: draft.fallback_proxy_url_1,
      fallback_proxy_url_2: draft.fallback_proxy_url_2
    };
    if (mode === "create") {
      onCreateGroup(input);
      onClose();
      return;
    }
    if (!activeGroup) return;
    onUpdateGroup({
      id: activeGroup.id,
      ...input,
      sort_order: Number.isFinite(sortOrder) ? sortOrder : activeGroup.sort_order
    });
    onSelectGroup(activeGroup.id);
    onClose();
  }

  function deleteGroup() {
    if (!activeGroup) return;
    onDeleteGroup(activeGroup.id);
    onSelectGroup("all");
    onClose();
  }

  return (
    <div
      className="oauthDialogBackdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !busy) onClose();
      }}
    >
      <div className="oauthDialog groupSettingsDialog" role="dialog" aria-modal="true" aria-labelledby="groupSettingsTitle">
        <div className="oauthDialogHeader">
          <div>
            <span className="oauthDialogIcon">
              <FolderKanban size={18} />
            </span>
            <h2 id="groupSettingsTitle">分组设置</h2>
          </div>
          <button className="iconMini" title="关闭" disabled={busy} onClick={onClose}>
            <X size={18} />
          </button>
        </div>

        <div className="oauthDialogBody">
          <section className="oauthAccountBox">
            <div className="modalTabLine">
              <button className={mode === "create" ? "button compact primary" : "button compact secondary"} onClick={() => setMode("create")}>
                <Plus size={14} />
                新建
              </button>
              <button
                className={mode === "edit" ? "button compact primary" : "button compact secondary"}
                disabled={groups.length === 0}
                onClick={() => setMode("edit")}
              >
                <SettingsIcon size={14} />
                编辑
              </button>
            </div>
            {mode === "edit" && (
              <label className="field">
                选择分组
                <select className="select" value={editingGroupId ?? ""} onChange={(event) => setEditingGroupId(Number(event.target.value))}>
                  {groups.map((group) => (
                    <option value={group.id} key={group.id}>
                      {groupOptionLabel(group)}
                    </option>
                  ))}
                </select>
              </label>
            )}
            <div className="oauthFieldGrid">
              <label className="field grow">
                分组名称
                <input className="input" value={draft.name} onChange={(event) => setDraft({ ...draft, name: event.target.value })} />
              </label>
              <label className="field grow">
                排序
                <input
                  className="input"
                  type="number"
                  value={draft.sort_order}
                  disabled={mode === "create"}
                  onChange={(event) => setDraft({ ...draft, sort_order: event.target.value })}
                />
              </label>
              <label className="field grow">
                父级分组
                <select
                  className="select"
                  value={draft.parent_id}
                  onChange={(event) => setDraft({ ...draft, parent_id: event.target.value ? Number(event.target.value) : "" })}
                >
                  <option value="">顶级分组</option>
                  {parentGroupOptions.map((group) => (
                    <option value={group.id} key={group.id}>
                      {groupOptionLabel(group)}
                    </option>
                  ))}
                </select>
              </label>
              <label className="field grow">
                颜色
                <div className="colorSwatches">
                  {colors.map((color) => (
                    <button
                      className={draft.color === color ? "colorSwatch active" : "colorSwatch"}
                      key={color}
                      title={color}
                      style={{ backgroundColor: color }}
                      onClick={() => setDraft({ ...draft, color })}
                    />
                  ))}
                  <input
                    className="colorInput"
                    type="color"
                    aria-label="自定义分组颜色"
                    value={draft.color}
                    onChange={(event) => setDraft({ ...draft, color: event.target.value })}
                  />
                </div>
              </label>
            </div>
            <textarea
              className="textarea compact"
              value={draft.description}
              placeholder="分组说明"
              onChange={(event) => setDraft({ ...draft, description: event.target.value })}
            />
            <input
              className="input"
              value={draft.proxy_url}
              placeholder="分组主代理：http://127.0.0.1:7890"
              onChange={(event) => setDraft({ ...draft, proxy_url: event.target.value })}
            />
            <div className="formLine">
              <input
                className="input grow"
                value={draft.fallback_proxy_url_1}
                placeholder="备用代理 1"
                onChange={(event) => setDraft({ ...draft, fallback_proxy_url_1: event.target.value })}
              />
              <input
                className="input grow"
                value={draft.fallback_proxy_url_2}
                placeholder="备用代理 2"
                onChange={(event) => setDraft({ ...draft, fallback_proxy_url_2: event.target.value })}
              />
            </div>
          </section>

          <section className="oauthStep">
            <h3>标签</h3>
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
            <div className="chipCloud modalChipCloud">
              {tags.map((tag) => (
                <span className="chip" key={tag.id}>
                  <span className="dot" style={{ backgroundColor: tag.color }} />
                  {tag.name}
                </span>
              ))}
            </div>
          </section>
        </div>

        <div className="oauthDialogFooter">
          {mode === "edit" && activeGroup && (
            <button className="button danger" disabled={busy || activeGroup.id === 1} onClick={deleteGroup}>
              <Trash2 size={16} />
              删除
            </button>
          )}
          <button className="button secondary" disabled={busy} onClick={onClose}>
            关闭
          </button>
          <button className="button primary" disabled={busy || !draft.name.trim() || (mode === "edit" && !activeGroup)} onClick={saveGroup}>
            <CheckCircle2 size={16} />
            {mode === "create" ? "创建分组" : "保存分组"}
          </button>
        </div>
      </div>
    </div>
  );
}

type OAuthAccountSaveDraft = {
  provider: string;
  email: string;
  password: string;
  client_id: string;
  group_id: number | null;
  forward_enabled: boolean;
  callback_url: string;
};

type ImportProviderChoice = "gmail" | "qq" | "netease_163";

const importProviderHelp: Record<ImportProviderChoice, { prefix: string; linkText: string; suffix: string; url: string }> = {
  gmail: {
    prefix: "先在 Google 账号中创建",
    linkText: "应用专用密码",
    suffix: "，再粘贴到 password 字段。",
    url: "https://myaccount.google.com/apppasswords"
  },
  qq: {
    prefix: "先登录 QQ 邮箱，在 设置 > 账号安全 > 安全设置 > POP3/IMAP/SMTP 服务 中生成",
    linkText: "授权码",
    suffix: "，再粘贴到 password 字段。",
    url: "https://wx.mail.qq.com/"
  },
  netease_163: {
    prefix: "先",
    linkText: "登录 163 邮箱",
    suffix: "，在 设置 > POP3/SMTP/IMAP 中开启服务并生成客户端授权密码，再粘贴到 password 字段。",
    url: "https://mail.163.com/"
  }
};

const importProviderOptions: Array<{
  value: ImportProviderChoice;
  label: string;
  description: string;
  placeholder: string;
}> = [
  {
    value: "gmail",
    label: "Gmail",
    description: "每行：Gmail 邮箱----Google 应用专用密码。",
    placeholder: "your@gmail.com----abcdefghijklmnop"
  },
  {
    value: "qq",
    label: "QQ 邮箱",
    description: "每行：QQ/Foxmail 邮箱----IMAP/SMTP 授权码。",
    placeholder: "user@qq.com----imap-smtp-auth-code"
  },
  {
    value: "netease_163",
    label: "163 邮箱",
    description: "每行：163 邮箱----客户端授权密码。",
    placeholder: "user@163.com----client-auth-password"
  }
];

function AccountImportDialog({
  groups,
  selectedGroupId,
  busy,
  onClose,
  onImport
}: {
  groups: Group[];
  selectedGroupId: number | "all";
  busy: boolean;
  onClose: () => void;
  onImport: (raw: string, groupId: number | null) => Promise<void> | void;
}) {
  const initialGroupId =
    selectedGroupId !== "all" && groups.some((group) => group.id === selectedGroupId) ? selectedGroupId : groups[0]?.id ?? null;
  const [raw, setRaw] = useState("");
  const [importProvider, setImportProvider] = useState<ImportProviderChoice>("gmail");
  const [groupId, setGroupId] = useState<number | null>(initialGroupId);
  const [localBusy, setLocalBusy] = useState(false);
  const [localError, setLocalError] = useState<string | null>(null);
  const selectedImportOption = importProviderOptions.find((option) => option.value === importProvider) ?? importProviderOptions[0];
  const selectedImportHelp = importProviderHelp[importProvider];
  const selectedDefaultProvider = importProvider;
  const parsedRows = useMemo(() => parseAccountRows(raw, { defaultProvider: selectedDefaultProvider }), [raw, selectedDefaultProvider]);
  const importBlockReason = useMemo(() => outlookImportBlockReason(parsedRows), [parsedRows]);
  const providerPreview = useMemo(() => formatProviderPreview(parsedRows), [parsedRows]);
  const previewRows = useMemo(() => parsedRows.slice(0, 8), [parsedRows]);
  const hiddenPreviewCount = parsedRows.length - previewRows.length;
  const loading = busy || localBusy;

  useEffect(() => {
    setGroupId((current) => {
      if (current !== null && groups.some((group) => group.id === current)) return current;
      return initialGroupId;
    });
  }, [groups, initialGroupId]);

  async function handleImport() {
    if (parsedRows.length === 0) {
      setLocalError("请粘贴至少一行账号");
      return;
    }
    if (importBlockReason) {
      setLocalError(importBlockReason);
      return;
    }
    setLocalBusy(true);
    setLocalError(null);
    try {
      await onImport(rawWithDefaultProvider(raw, selectedDefaultProvider), groupId);
      onClose();
    } catch (err) {
      setLocalError(readError(err));
    } finally {
      setLocalBusy(false);
    }
  }

  async function openImportProviderHelpPage() {
    setLocalError(null);
    try {
      await api.openExternalUrl(selectedImportHelp.url);
    } catch (err) {
      setLocalError(readError(err));
    }
  }

  return (
    <div
      className="oauthDialogBackdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !loading) onClose();
      }}
    >
      <div className="oauthDialog importDialog" role="dialog" aria-modal="true" aria-labelledby="accountImportTitle">
        <div className="oauthDialogHeader">
          <div>
            <span className="oauthDialogIcon">
              <Upload size={18} />
            </span>
            <h2 id="accountImportTitle">导入账号</h2>
          </div>
          <button className="iconMini" title="关闭" disabled={loading} onClick={onClose}>
            <X size={18} />
          </button>
        </div>

        <div className="oauthDialogBody">
          <div className="formLine importTypeLine">
            <label className="field importTypeField">
              <span>类型：</span>
              <select
                className="select"
                value={importProvider}
                onChange={(event) => {
                  setImportProvider(event.target.value as ImportProviderChoice);
                  setLocalError(null);
                }}
              >
                {importProviderOptions.map((option) => (
                  <option value={option.value} key={option.value}>
                    {option.label}
                  </option>
                ))}
              </select>
            </label>
          </div>
          <section className="oauthAccountBox">
            <p className="oauthHint importProviderHelp">
              {selectedImportHelp.prefix}{" "}
              <button type="button" className="importHelpLink" onClick={openImportProviderHelpPage}>
                {selectedImportHelp.linkText}
              </button>
              {selectedImportHelp.suffix}
            </p>
            <p>{selectedImportOption.description}</p>
            <textarea
              className="textarea importTextarea"
              value={raw}
              autoFocus
              onChange={(event) => {
                setRaw(event.target.value);
                setLocalError(null);
              }}
              placeholder={selectedImportOption.placeholder}
            />
            <div className="formLine importDialogControls">
              <label className="field grow">
                目标分组
                <select
                  className="select"
                  value={groupId ?? ""}
                  onChange={(event) => setGroupId(event.target.value ? Number(event.target.value) : null)}
                >
                  {groups.map((group) => (
                    <option value={group.id} key={group.id}>
                      {groupOptionLabel(group)}
                    </option>
                  ))}
                </select>
              </label>
              <span className="dialogMeta">已识别 {parsedRows.length} 个账号{providerPreview ? ` · ${providerPreview}` : ""}</span>
            </div>
            {previewRows.length > 0 && (
              <div className="importPreviewTable" aria-label="账号导入预览">
                <div className="importPreviewHeader">
                  <span>邮箱</span>
                  <span>服务商</span>
                  <span>凭据类型</span>
                  <span>备注</span>
                </div>
                {previewRows.map((row, index) => {
                  const provider = accountProviderDefinition(row.provider);
                  return (
                    <div className="importPreviewRow" key={`${row.email}-${index}`}>
                      <span className="importPreviewEmail" title={row.email}>
                        {row.email}
                      </span>
                      <ProviderBadge provider={provider.id} />
                      <span className="credentialBadge" title={provider.setupHint || provider.credentialLabel}>
                        {provider.credentialLabel}
                      </span>
                      <span className="importPreviewRemark" title={row.remark || "未填写"}>
                        {row.remark || "未填写"}
                      </span>
                    </div>
                  );
                })}
                {hiddenPreviewCount > 0 && <div className="importPreviewMore">另有 {hiddenPreviewCount} 个账号将按识别结果导入</div>}
              </div>
            )}
          </section>
          {(localError || importBlockReason) && <div className="formError">{localError || importBlockReason}</div>}
        </div>

        <div className="oauthDialogFooter">
          <button className="button secondary" disabled={loading} onClick={onClose}>
            关闭
          </button>
          <button className="button primary" disabled={loading || parsedRows.length === 0 || Boolean(importBlockReason)} onClick={handleImport}>
            {localBusy ? <Loader2 className="spin" size={16} /> : <Download size={16} />}
            导入 {parsedRows.length || ""}
          </button>
        </div>
      </div>
    </div>
  );
}

function OAuthAccountSaveDialog({
  groups,
  settings,
  selectedGroupId,
  busy,
  onClose,
  onGenerateOAuthUrl,
  onPreviewOAuthToken,
  onSaveOAuthAccount
}: {
  groups: Group[];
  settings: Settings | null;
  selectedGroupId?: number | "all";
  busy: boolean;
  onClose: () => void;
  onGenerateOAuthUrl: (input: OAuthAuthUrlRequest) => Promise<string>;
  onPreviewOAuthToken: (input: OAuthTokenExchangeRequest) => Promise<OAuthTokenResult>;
  onSaveOAuthAccount: (input: OAuthSaveAccountInput) => Promise<OAuthSaveAccountResult>;
}) {
  const initialGroupId =
    selectedGroupId !== "all" && selectedGroupId !== undefined && groups.some((group) => group.id === selectedGroupId)
      ? selectedGroupId
      : groups[0]?.id ?? null;
  const [draft, setDraft] = useState<OAuthAccountSaveDraft>({
    provider: "graph",
    email: "",
    password: "",
    client_id: "",
    group_id: initialGroupId,
    forward_enabled: false,
    callback_url: ""
  });
  const [authUrl, setAuthUrl] = useState("");
  const [oauthCodeVerifier, setOauthCodeVerifier] = useState("");
  const [preview, setPreview] = useState<{
    provider: string;
    email: string;
    password: string;
    client_id: string;
    group_id: number | null;
    group_name: string;
    forward_enabled: boolean;
    refresh_token: string;
    refresh_token_preview: string;
    scope: string;
    expires_in: number;
  } | null>(null);
  const [localBusy, setLocalBusy] = useState<"url" | "open" | "preview" | "save" | null>(null);
  const [localError, setLocalError] = useState<string | null>(null);
  const [localNotice, setLocalNotice] = useState<string | null>(null);
  const redirectUri = settings?.oauth_redirect_uri || defaultOAuthRedirectUri;
  const selectedProvider = accountProviderDefinition(draft.provider);
  const defaultClientId = draft.provider === "graph" ? settings?.graph_client_id || defaultGraphClientId : "";
  const activeClientId = draft.client_id.trim() || defaultClientId;
  const loading = busy || localBusy !== null;

  useEffect(() => {
    setDraft((current) => {
      if (current.group_id && groups.some((group) => group.id === current.group_id)) return current;
      return { ...current, group_id: initialGroupId };
    });
  }, [groups, initialGroupId]);

  useEffect(() => {
    let cancelled = false;
    const clientId = activeClientId;
    const codeVerifier = "";

    if (!clientId) {
      setAuthUrl("");
      setOauthCodeVerifier("");
      return () => {
        cancelled = true;
      };
    }

    setLocalBusy("url");
    setOauthCodeVerifier(codeVerifier);
    onGenerateOAuthUrl({
      client_id: clientId,
      redirect_uri: redirectUri,
      login_hint: draft.email.trim() || undefined,
      provider: draft.provider,
      code_verifier: codeVerifier || undefined
    })
      .then((url) => {
        if (!cancelled) setAuthUrl(url);
      })
      .catch((err) => {
        if (!cancelled) setLocalError(readError(err));
      })
      .finally(() => {
        if (!cancelled) setLocalBusy(null);
      });

    return () => {
      cancelled = true;
    };
  }, [activeClientId, draft.email, draft.provider, redirectUri, onGenerateOAuthUrl]);

  function updateDraft(next: Partial<OAuthAccountSaveDraft>) {
    const shouldResetPreview = "provider" in next || "client_id" in next || "callback_url" in next;
    setDraft((current) => ({ ...current, ...next }));
    if (shouldResetPreview) {
      setPreview(null);
    }
    setLocalError(null);
    setLocalNotice(null);
  }

  function validateBase(requireCallback: boolean) {
    if (!activeClientId.trim()) return `请填写 ${selectedProvider.label} Client ID`;
    if (requireCallback && !draft.callback_url.trim()) return "请粘贴授权后的完整回调 URL";
    return null;
  }

  async function handleCopyUrl() {
    if (!authUrl) return;
    try {
      await navigator.clipboard.writeText(authUrl);
      setLocalError(null);
      setLocalNotice("授权链接已复制");
    } catch {
      setLocalNotice(null);
      setLocalError("复制失败，请手动选中授权链接复制");
    }
  }

  async function handleOpenUrl() {
    if (!authUrl) return;
    setDraft((current) => ({ ...current, callback_url: "" }));
    setPreview(null);
    setLocalError(null);
    setLocalNotice(null);
    setLocalBusy("open");
    try {
      await api.openExternalUrl(authUrl);
      setLocalNotice("已在默认浏览器打开授权页面");
    } catch (err) {
      setLocalError(readError(err));
    } finally {
      setLocalBusy(null);
    }
  }

  async function handlePreview() {
    const validation = validateBase(true);
    if (validation) {
      setLocalError(validation);
      return false;
    }
    setLocalBusy("preview");
    setLocalError(null);
    setLocalNotice(null);
    try {
      const result = await onPreviewOAuthToken({
        client_id: activeClientId,
        redirect_uri: redirectUri,
        code_or_url: draft.callback_url.trim(),
        provider: draft.provider,
        code_verifier: oauthCodeVerifier || undefined
      });
      if (!result.refresh_token) {
        throw new Error("OAuth 响应没有返回 refresh token");
      }
      const group = groups.find((item) => item.id === draft.group_id);
      const clientId = activeClientId;
      setPreview({
        provider: draft.provider,
        email: draft.email.trim(),
        password: draft.password,
        client_id: clientId,
        group_id: draft.group_id ?? null,
        group_name: group?.name ?? "",
        forward_enabled: draft.forward_enabled,
        refresh_token: result.refresh_token,
        refresh_token_preview: result.refresh_token_preview,
        scope: result.scope,
        expires_in: result.expires_in
      });
      return true;
    } catch (err) {
      setLocalError(readError(err));
      return false;
    } finally {
      setLocalBusy(null);
    }
  }

  async function handleSave() {
    if (!draft.email.trim()) {
      setLocalError("保存账号前需要填写邮箱账号");
      return;
    }
    const validation = preview ? validateBase(false) : validateBase(true);
    if (validation) {
      setLocalError(validation);
      return;
    }
    setLocalBusy("save");
    setLocalError(null);
    setLocalNotice(null);
    try {
      await onSaveOAuthAccount({
        email: draft.email.trim(),
        password: draft.password || undefined,
        group_id: draft.group_id ?? undefined,
        forward_enabled: draft.forward_enabled,
        client_id: preview?.client_id ?? activeClientId,
        redirect_uri: redirectUri,
        code_or_url: preview ? undefined : draft.callback_url.trim(),
        refresh_token: preview?.refresh_token,
        provider: draft.provider,
        code_verifier: oauthCodeVerifier || undefined
      });
      onClose();
    } catch (err) {
      setLocalError(readError(err));
    } finally {
      setLocalBusy(null);
    }
  }

  return (
    <div
      className="oauthDialogBackdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !loading) onClose();
      }}
    >
      <div className="oauthDialog" role="dialog" aria-modal="true" aria-labelledby="oauthSaveTitle">
        <div className="oauthDialogHeader">
          <div>
            <span className="oauthDialogIcon">
              <KeyRound size={18} />
            </span>
            <h2 id="oauthSaveTitle">授权并保存 OAuth 账号</h2>
          </div>
          <button className="iconMini" title="关闭" disabled={loading} onClick={onClose}>
            <X size={18} />
          </button>
        </div>

        <div className="oauthDialogBody">
          <section className="oauthAccountBox">
            <h3>待入库账号</h3>
            <p>预览只需要回调 URL；保存账号时需要邮箱，密码和目标分组可稍后补充。</p>
            <div className="oauthFieldGrid">
              <label className="field grow">
                服务商
                <select className="select" value={draft.provider} onChange={(event) => updateDraft({ provider: event.target.value })}>
                  <option value="graph">Outlook</option>
                </select>
              </label>
              <label className="field grow">
                邮箱账号
                <input
                  className="input"
                  value={draft.email}
                  placeholder="your@outlook.com"
                  onChange={(event) => updateDraft({ email: event.target.value })}
                />
              </label>
              <label className="field grow">
                密码
                <input
                  className="input"
                  type="password"
                  value={draft.password}
                  placeholder="保存时可选"
                  onChange={(event) => updateDraft({ password: event.target.value })}
                />
              </label>
              <label className="field grow">
                {selectedProvider.label} Client ID
                <input
                  className="input"
                  value={draft.client_id}
                  placeholder="留空使用默认 Client ID"
                  onChange={(event) => updateDraft({ client_id: event.target.value })}
                />
              </label>
              <label className="field grow">
                目标分组
                <select
                  className="select"
                  value={draft.group_id ?? ""}
                  onChange={(event) => updateDraft({ group_id: event.target.value ? Number(event.target.value) : null })}
                >
                  {groups.map((group) => (
                    <option value={group.id} key={group.id}>
                      {groupOptionLabel(group)}
                    </option>
                  ))}
                </select>
              </label>
            </div>
            <label className="checkLine oauthForwardToggle">
              <input
                type="checkbox"
                checked={draft.forward_enabled}
                onChange={(event) => updateDraft({ forward_enabled: event.target.checked })}
              />
              <span>保存后启用邮件转发</span>
            </label>
          </section>

          <section className="oauthStep">
            <h3>步骤 1: 打开授权页面</h3>
            <div className="oauthUrlLine">
              <input className="input grow monoInput" readOnly value={authUrl} placeholder={`正在准备 ${selectedProvider.label} 授权链接`} />
              <button className="button secondary" disabled={loading || !authUrl} onClick={handleCopyUrl}>
                <Copy size={16} />
                复制
              </button>
              <button className="button primary" disabled={loading || !authUrl} onClick={handleOpenUrl}>
                {localBusy === "open" ? <Loader2 className="spin" size={16} /> : <ExternalLink size={16} />}
                打开
              </button>
            </div>
            {localNotice && <div className="formSuccess">{localNotice}</div>}
          </section>

          <section className="oauthStep">
            <h3>步骤 2: 粘贴授权后的回调 URL</h3>
            <textarea
              className="textarea compact monoInput"
              value={draft.callback_url}
              placeholder="授权成功后，复制浏览器地址栏中的完整 URL 并粘贴到这里"
              onChange={(event) => updateDraft({ callback_url: event.target.value })}
            />
            <p className="oauthHint">URL 格式类似：http://localhost:8080/?code=xxxxx&amp;state=12345</p>
          </section>

          {preview && (
            <section className="oauthPreviewBox">
              <div className="oauthPreviewTitle">
                <CheckCircle2 size={16} />
                保存预览
              </div>
              <div className="oauthFieldGrid">
                <label className="field grow">
                  服务商
                  <input className="input" readOnly value={accountProviderLabel(preview.provider)} />
                </label>
                <label className="field grow">
                  邮箱
                  <input className="input" readOnly value={draft.email.trim() || "未填写"} />
                </label>
                <label className="field grow">
                  目标分组
                  <input className="input" readOnly value={groups.find((group) => group.id === draft.group_id)?.name || "未选择"} />
                </label>
                <label className="field grow">
                  Client ID
                  <input className="input monoInput" readOnly value={preview.client_id} />
                </label>
                <label className="field grow">
                  Refresh Token
                  <input className="input monoInput" readOnly value={preview.refresh_token_preview} />
                </label>
              </div>
              <p className="oauthHint">已换取的 refresh token 会临时保存在当前弹窗状态中，点击保存不会再次消耗授权码。</p>
            </section>
          )}

          {localError && <div className="formError">{localError}</div>}
        </div>

        <div className="oauthDialogFooter">
          <button className="button secondary" disabled={loading} onClick={onClose}>
            关闭
          </button>
          <button className="button primary" disabled={loading || !!preview} onClick={handlePreview}>
            {localBusy === "preview" ? <Loader2 className="spin" size={16} /> : <CheckCircle2 size={16} />}
            {preview ? "已换取" : "换取并预览"}
          </button>
          <button className="button primary" disabled={loading} onClick={handleSave}>
            {localBusy === "save" ? <Loader2 className="spin" size={16} /> : <KeyRound size={16} />}
            {preview ? "保存预览结果" : "直接保存（自动换取）"}
          </button>
        </div>
      </div>
    </div>
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
  const providerFailureSummaries = useMemo(() => summarizeProviderFailures(accounts), [accounts]);
  const selectedSet = useMemo(() => new Set(selectedAccountIds), [selectedAccountIds]);
  const accountSearchTokens = useMemo(() => searchTokens(accountSearch), [accountSearch]);
  const visibleAccounts = useMemo(() => {
    return accounts.filter((account) => {
      if (accountFilter === "failed" && account.last_refresh_status !== "failed") return false;
      if (accountFilter === "success" && account.last_refresh_status !== "success") return false;
      if (accountFilter === "never" && account.last_refresh_status !== "never") return false;
      if (accountFilter === "ready" && !isRefreshReady(account)) return false;
      if (accountFilter === "missing" && isRefreshReady(account)) return false;
      return accountMatchesSearch(account, accountSearchTokens);
    });
  }, [accounts, accountFilter, accountSearchTokens]);
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
        {providerFailureSummaries.length > 0 && (
          <div className="providerFailureGrid" aria-label="刷新失败服务商汇总">
            {providerFailureSummaries.map((summary) => (
              <button
                className="providerFailureCard"
                type="button"
                key={summary.providerId}
                title={`查看 ${summary.label} 失败账号`}
                onClick={() => {
                  setAccountFilter("failed");
                  setAccountSearch(summary.label);
                }}
              >
                <ProviderBadge provider={summary.providerId} />
                <strong>{summary.count} 个失败</strong>
                <small title={summary.topError}>{summary.topError}</small>
                <small className="providerFailureHint" title={summary.hint}>{summary.hint}</small>
              </button>
            ))}
          </div>
        )}
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
              <RefreshCredentialCell account={account} />
              <StatusPill status={account.last_refresh_status} />
              <span>{account.last_refresh_at ? formatDate(account.last_refresh_at) : "从未"}</span>
              <span>{account.message_count}</span>
              <RefreshErrorCell account={account} />
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

function ProviderBadge({ provider, compact = false, showMark = true }: { provider: string; compact?: boolean; showMark?: boolean }) {
  const providerId = normalizeAccountProviderId(provider);
  const definition = accountProviderDefinition(providerId);
  return (
    <span
      className={`providerBadge provider-${providerId}${compact ? " compact" : ""}`}
      title={`${definition.label} · ${definition.credentialLabel} · ${providerCapabilitySummary(providerId)}`}
    >
      {showMark && <span className="providerBadgeMark">{providerBadgeCode(providerId)}</span>}
      <span className="providerBadgeText">{definition.label}</span>
    </span>
  );
}

function providerBadgeCode(provider: string) {
  switch (normalizeAccountProviderId(provider)) {
    case "graph":
      return "O";
    case "gmail":
      return "G";
    case "qq":
      return "Q";
    case "netease_163":
      return "163";
    case "imap":
      return "IM";
    default:
      return "C";
  }
}

function isRefreshReady(account: Account) {
  return providerReadiness(account).status === "ready";
}

function RefreshCredentialCell({ account }: { account: Account }) {
  const readiness = providerReadiness(account);
  const text = readiness.status === "missing" ? readiness.detail : readiness.label;
  return (
    <span className={`credentialCell readiness-${readiness.status}`} title={readiness.detail}>
      {text}
    </span>
  );
}

function RefreshErrorCell({ account }: { account: Account }) {
  if (!account.last_refresh_error) return <span />;
  const hint = providerFailureHint(account.provider, account.last_refresh_error);
  return (
    <span className="refreshErrorCell">
      <span title={account.last_refresh_error}>{account.last_refresh_error}</span>
      <small title={hint}>{hint}</small>
    </span>
  );
}

function summarizeProviderFailures(accounts: Account[]) {
  const grouped = new Map<string, { count: number; errors: Map<string, number> }>();
  for (const account of accounts) {
    if (account.last_refresh_status !== "failed") continue;
    const providerId = normalizeAccountProviderId(account.provider);
    const summary = grouped.get(providerId) ?? { count: 0, errors: new Map<string, number>() };
    const error = account.last_refresh_error?.trim() || "暂无错误详情";
    summary.count += 1;
    summary.errors.set(error, (summary.errors.get(error) ?? 0) + 1);
    grouped.set(providerId, summary);
  }

  return Array.from(grouped.entries())
    .map(([providerId, summary]) => {
      const topError =
        Array.from(summary.errors.entries()).sort(([leftError, leftCount], [rightError, rightCount]) => {
          if (rightCount !== leftCount) return rightCount - leftCount;
          return leftError.localeCompare(rightError);
        })[0]?.[0] ?? "暂无错误详情";
      return {
        providerId,
        label: accountProviderLabel(providerId),
        count: summary.count,
        topError,
        hint: providerFailureHint(providerId, topError)
      };
    })
    .sort((left, right) => {
      if (right.count !== left.count) return right.count - left.count;
      return left.label.localeCompare(right.label);
    });
}

function AccountAuthDialog({
  account,
  groups,
  tags,
  settings,
  busy,
  onClose,
  onSave,
  onRevealAccountSecrets,
  onGenerateOAuthUrl,
  onExchangeOAuthToken
}: {
  account: Account;
  groups: Group[];
  tags: Tag[];
  settings: Settings | null;
  busy: boolean;
  onClose: () => void;
  onSave: (input: Parameters<typeof api.updateAccount>[0]) => void;
  onRevealAccountSecrets: (input: Parameters<typeof api.revealAccountSecrets>[0]) => Promise<Awaited<ReturnType<typeof api.revealAccountSecrets>>>;
  onGenerateOAuthUrl: (input: OAuthAuthUrlRequest) => Promise<string>;
  onExchangeOAuthToken: (input: OAuthTokenExchangeRequest) => void;
}) {
  const [oauthUrl, setOauthUrl] = useState("");
  const [oauthCallback, setOauthCallback] = useState("");
  const [oauthCodeVerifier, setOauthCodeVerifier] = useState("");

  useEffect(() => {
    setOauthUrl("");
    setOauthCallback("");
    setOauthCodeVerifier("");
  }, [account.id]);

  return (
    <div
      className="oauthDialogBackdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !busy) onClose();
      }}
    >
      <div className="oauthDialog accountAuthDialog" role="dialog" aria-modal="true" aria-labelledby="accountAuthTitle">
        <div className="oauthDialogHeader">
          <div>
            <span className="oauthDialogIcon">
              <KeyRound size={18} />
            </span>
            <h2 id="accountAuthTitle">账号设置 · {account.email}</h2>
          </div>
          <button className="iconMini" title="关闭" disabled={busy} onClick={onClose}>
            <X size={18} />
          </button>
        </div>
        <div className="oauthDialogBody accountAuthDialogBody">
          <AccountEditor
            account={account}
            groups={groups}
            tags={tags}
            settings={settings}
            busy={busy}
            hideHeader
            oauthUrl={oauthUrl}
            oauthCallback={oauthCallback}
            oauthCodeVerifier={oauthCodeVerifier}
            onOauthUrlChange={setOauthUrl}
            onOauthCallbackChange={setOauthCallback}
            onOauthCodeVerifierChange={setOauthCodeVerifier}
            onSave={onSave}
            onRevealAccountSecrets={onRevealAccountSecrets}
            onGenerateOAuthUrl={async (input) => {
              setOauthUrl(await onGenerateOAuthUrl(input));
            }}
            onExchangeOAuthToken={onExchangeOAuthToken}
          />
        </div>
      </div>
    </div>
  );
}

function AccountEditor({
  account,
  groups,
  tags,
  settings,
  busy,
  oauthUrl,
  oauthCallback,
  oauthCodeVerifier,
  onOauthUrlChange,
  onOauthCallbackChange,
  onOauthCodeVerifierChange,
  onSave,
  onRevealAccountSecrets,
  onGenerateOAuthUrl,
  onExchangeOAuthToken,
  hideHeader = false
}: {
  account?: Account;
  groups: Group[];
  tags: Tag[];
  settings: Settings | null;
  busy: boolean;
  hideHeader?: boolean;
  oauthUrl: string;
  oauthCallback: string;
  oauthCodeVerifier: string;
  onOauthUrlChange: (value: string) => void;
  onOauthCallbackChange: (value: string) => void;
  onOauthCodeVerifierChange: (value: string) => void;
  onSave: (input: Parameters<typeof api.updateAccount>[0]) => void;
  onRevealAccountSecrets: (input: Parameters<typeof api.revealAccountSecrets>[0]) => Promise<Awaited<ReturnType<typeof api.revealAccountSecrets>>>;
  onGenerateOAuthUrl: (input: OAuthAuthUrlRequest) => void;
  onExchangeOAuthToken: (input: OAuthTokenExchangeRequest) => void;
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
    mail_retention_days: 30,
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
    const provider = normalizeAccountProviderId(account.provider);
    setDraft({
      email: account.email,
      group_id: account.group_id,
      provider,
      account_type: account.account_type || providerAccountType(provider),
      remark: account.remark,
      forward_enabled: account.forward_enabled,
      imap_host: account.imap_host,
      imap_port: account.imap_port || 993,
      proxy_url: account.proxy_url,
      fallback_proxy_url_1: account.fallback_proxy_url_1,
      fallback_proxy_url_2: account.fallback_proxy_url_2,
      mail_retention_days: account.mail_retention_days ?? 30,
      password: "",
      client_id: provider === "graph" || provider === "imap" ? settings?.graph_client_id || defaultGraphClientId : "",
      refresh_token: "",
      imap_password: "",
      tag_ids: account.tags.map((tag) => tag.id),
      aliasesText: account.aliases.join("\n")
    });
    onOauthUrlChange("");
    onOauthCallbackChange("");
    onOauthCodeVerifierChange("");
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

  const redirectUri = settings?.oauth_redirect_uri || defaultOAuthRedirectUri;
  const selectedProvider = accountProviderDefinition(draft.provider);
  const oauthLinkSupported = draft.provider === "graph" || draft.provider === "imap";

  function updateProvider(provider: string) {
    const normalizedProvider = normalizeAccountProviderId(provider);
    const defaults = providerDefaultImap(normalizedProvider);
    const accountType = providerAccountType(normalizedProvider);
    const defaultClientId = normalizedProvider === "graph" || normalizedProvider === "imap" ? settings?.graph_client_id || defaultGraphClientId : "";
    onOauthUrlChange("");
    onOauthCallbackChange("");
    onOauthCodeVerifierChange("");
    setDraft({
      ...draft,
      provider: normalizedProvider,
      account_type: accountType,
      client_id: defaultClientId,
      imap_host: defaults.host || (accountType === "imap" ? draft.imap_host : ""),
      imap_port: defaults.port
    });
  }

  return (
    <div className="panel">
      {!hideHeader && (
        <div className="panelHeader">
          <h2>授权设置</h2>
          <KeyRound size={18} />
        </div>
      )}
      <div className="formLine">
        <input className="input grow" value={draft.email} onChange={(event) => setDraft({ ...draft, email: event.target.value })} />
        <select className="select" value={draft.provider} onChange={(event) => updateProvider(event.target.value)}>
          {accountProviderRegistry.map((provider) => (
            <option value={provider.id} key={provider.id}>
              {provider.label}
            </option>
          ))}
        </select>
      </div>
      {selectedProvider.setupHint && <p className="oauthHint">{selectedProvider.setupHint}</p>}
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
      <label className="field">
        邮箱保留天数
        <input
          className="input"
          type="number"
          min={1}
          max={3650}
          value={draft.mail_retention_days}
          onChange={(event) =>
            setDraft({
              ...draft,
              mail_retention_days: Math.max(1, Math.min(3650, Number(event.target.value) || 30))
            })
          }
        />
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
      {oauthLinkSupported && (
        <>
          <div className="formLine">
            <input
              className="input grow"
              value={draft.client_id}
              placeholder={`${selectedProvider.label} Client ID`}
              onChange={(event) => setDraft({ ...draft, client_id: event.target.value })}
            />
            <button
              className="button secondary"
              disabled={!draft.client_id.trim()}
              onClick={() => {
                onOauthCodeVerifierChange("");
                onGenerateOAuthUrl({
                  client_id: draft.client_id,
                  redirect_uri: redirectUri,
                  login_hint: draft.email,
                  provider: draft.provider
                });
              }}
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
                  provider: draft.provider,
                  code_verifier: oauthCodeVerifier || undefined
                })
              }
            >
              保存 OAuth
            </button>
          </div>
        </>
      )}
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
          placeholder={`${selectedProvider.credentialPlaceholder}，可选`}
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
            mail_retention_days: draft.mail_retention_days,
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

function AutomationDashboardView({
  observability,
  automationRuns,
  retryQueue,
  schedulerStatus,
  busy,
  onFilterAutomationRuns,
  onClearAutomationRuns,
  onRunRetryQueue,
  onRetryQueueItem,
  onDismissRetryItem
}: {
  observability: AutomationObservability | null;
  automationRuns: AutomationRun[];
  retryQueue: RetryQueueItem[];
  schedulerStatus: SchedulerStatus | null;
  busy: boolean;
  onFilterAutomationRuns: (query: AutomationRunQuery) => void;
  onClearAutomationRuns: (query: AutomationRunQuery & { clear_all?: boolean }) => void;
  onRunRetryQueue: () => void;
  onRetryQueueItem: (retryId: number) => void;
  onDismissRetryItem: (retryId: number) => void;
}) {
  const [runFilters, setRunFilters] = useState({ job_type: "all", trigger_type: "all", status: "all", search: "" });

  function automationRunQuery(): AutomationRunQuery {
    return {
      job_type: runFilters.job_type === "all" ? undefined : runFilters.job_type,
      trigger_type: runFilters.trigger_type === "all" ? undefined : runFilters.trigger_type,
      status: runFilters.status === "all" ? undefined : runFilters.status,
      search: runFilters.search.trim() || undefined
    };
  }

  return (
    <section className="settingsGrid automationDashboard">
      <div className="panel widePanel">
        <div className="panelHeader">
          <h2>自动化仪表盘</h2>
          <Activity size={18} />
        </div>
        {observability ? (
          <>
            <div className="statStrip observabilityStrip">
              <Stat label="任务记录" value={observability.run_count} />
              <Stat label="成功" value={observability.successful_run_count} />
              <Stat label="失败" value={observability.failed_run_count} />
              <Stat label="待重试" value={observability.retry_pending_count} />
              <Stat label="到期重试" value={observability.retry_due_count} />
              <Stat label="熔断通道" value={observability.open_circuit_count} />
            </div>
            <div className="runStatusGrid">
              <RunStatus label="上次刷新" value={schedulerStatus?.last_refresh_at} />
              <RunStatus label="上次转发" value={schedulerStatus?.last_forwarding_at} />
              <RunStatus label="上次备份" value={schedulerStatus?.last_backup_at} />
            </div>
            <div className="automationSummaryGrid">
              {observability.job_summaries.map((summary) => (
                <div className="summaryTile" key={summary.job_type}>
                  <div className="summaryTop">
                    <strong>{formatAutomationJob(summary.job_type)}</strong>
                    <StatusPill status={summary.failed > 0 ? "failed" : summary.total > 0 ? "success" : "never"} />
                  </div>
                  <div className="summaryStats">
                    <span>{summary.total} 次</span>
                    <span>{summary.success} 成功</span>
                    <span>{summary.failed} 失败</span>
                    <span>{formatDuration(summary.average_duration_ms)}</span>
                  </div>
                  <small>{summary.last_finished_at ? formatDate(summary.last_finished_at) : "从未运行"}</small>
                </div>
              ))}
            </div>
          </>
        ) : (
          <EmptyState icon={<Activity size={24} />} text="暂无自动化观测数据。" />
        )}
      </div>

      <div className="panel widePanel">
        <div className="panelHeader">
          <h2>转发通道健康</h2>
          <Mail size={18} />
        </div>
        <div className="logTable channelHealthTable">
          <div className="logHeader">
            <span>通道</span>
            <span>状态</span>
            <span>近期失败</span>
            <span>待重试</span>
            <span>熔断至</span>
            <span>上次成功</span>
            <span>错误</span>
          </div>
          {(observability?.channel_circuits ?? []).map((channel) => (
            <div className="logRow" key={channel.channel}>
              <span>{formatForwardingChannel(channel.channel)}</span>
              <StatusPill status={channel.status} />
              <span>{channel.recent_failures}</span>
              <span>{channel.pending_retries}</span>
              <span>{channel.open_until ? formatDate(channel.open_until) : "-"}</span>
              <span>{channel.last_success_at ? formatDate(channel.last_success_at) : "-"}</span>
              <span>{channel.last_error || "-"}</span>
            </div>
          ))}
        </div>
      </div>

      <div className="panel widePanel">
        <div className="panelHeader">
          <h2>错误分类</h2>
          <XCircle size={18} />
        </div>
        <div className="logTable errorBucketTable">
          <div className="logHeader">
            <span>类别</span>
            <span>次数</span>
            <span>最近时间</span>
            <span>详情</span>
          </div>
          {(observability?.error_buckets ?? []).map((bucket) => (
            <div className="logRow" key={bucket.category}>
              <span>{formatErrorCategory(bucket.category)}</span>
              <span>{bucket.count}</span>
              <span>{bucket.latest_at ? formatDate(bucket.latest_at) : "-"}</span>
              <span>{formatResultMessage(bucket.latest_message)}</span>
            </div>
          ))}
        </div>
        {observability?.error_buckets.length === 0 && <EmptyState icon={<CheckCircle2 size={24} />} text="最近没有失败错误。" />}
      </div>

      <div className="panel widePanel">
        <div className="panelHeader">
          <h2>重试退避</h2>
          <RotateCcw size={18} />
        </div>
        <div className="automationSummaryGrid retrySummaryGrid">
          {(observability?.retry_summaries ?? []).map((summary) => (
            <div className="summaryTile" key={summary.task_type}>
              <div className="summaryTop">
                <strong>{formatRetryTaskType(summary.task_type)}</strong>
                <StatusPill status={summary.failed > 0 ? "failed" : summary.pending > 0 ? "pending" : "success"} />
              </div>
              <div className="summaryStats">
                <span>{summary.pending} 待处理</span>
                <span>{summary.due} 到期</span>
                <span>{summary.failed} 失败</span>
                <span>{summary.exhausted} 耗尽</span>
              </div>
              <small>{summary.next_attempt_at ? `下次 ${formatDate(summary.next_attempt_at)}` : summary.last_error || "无待处理项"}</small>
            </div>
          ))}
        </div>
        <div className="tableActions">
          <button className="button secondary" disabled={busy || retryQueue.length === 0} onClick={onRunRetryQueue}>
            {busy ? <Loader2 className="spin" size={16} /> : <RotateCcw size={16} />}
            运行到期重试
          </button>
        </div>
        <div className="logTable retryObservabilityTable">
          <div className="logHeader">
            <span>更新时间</span>
            <span>任务</span>
            <span>状态</span>
            <span>错误类</span>
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
              <span>{formatErrorCategory(item.error_category)}</span>
              <span>{item.attempts} / {item.max_attempts}</span>
              <span>{item.next_attempt_at ? `${formatDate(item.next_attempt_at)}（${formatRetryDelay(item)}）` : item.due_now ? "已到期" : "-"}</span>
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
          <h2>任务历史</h2>
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
        <div className="logTable automationDashboardLogTable">
          <div className="logHeader">
            <span>时间</span>
            <span>任务</span>
            <span>触发</span>
            <span>状态</span>
            <span>错误类</span>
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
              <span>{formatErrorCategory(run.error_category)}</span>
              <span>{run.refreshed} 成功 / {run.failed} 失败</span>
              <span>{formatDuration(run.duration_ms)}</span>
              <span>{formatResultMessage(run.message)}</span>
            </div>
          ))}
        </div>
        {automationRuns.length === 0 && <EmptyState icon={<RefreshCw size={24} />} text="暂无自动化运行记录。" />}
      </div>
    </section>
  );
}

function WorkspaceKeyRevealDialog({
  revealed,
  busy,
  onSave,
  onClose,
  onShowToast
}: {
  revealed: { recordId: number; purpose: string; workspace_key: string };
  busy: boolean;
  onSave: (recordId: number, purpose: string) => Promise<WorkspaceKeyRecord>;
  onClose: () => void | Promise<void>;
  onShowToast: (message: string) => void;
}) {
  const [purposeDraft, setPurposeDraft] = useState(revealed.purpose);
  const [dialogError, setDialogError] = useState("");
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    setPurposeDraft(revealed.purpose);
    setDialogError("");
  }, [revealed.recordId, revealed.purpose]);

  async function copyWorkspaceKey() {
    try {
      await writeTextToClipboard(revealed.workspace_key);
      setDialogError("");
      onShowToast("复制成功");
    } catch {
      setDialogError("复制失败，请手动复制密钥");
    }
  }

  async function savePurpose() {
    setDialogError("");
    setSaving(true);
    try {
      await onSave(revealed.recordId, purposeDraft.trim());
      onShowToast("密钥已保存");
      await onClose();
    } catch (err) {
      setDialogError(readError(err));
    } finally {
      setSaving(false);
    }
  }

  const dialogBusy = busy || saving;

  return (
    <div className="oauthDialogBackdrop workspaceKeyDialogBackdrop">
      <div className="oauthDialog workspaceKeyDialog" role="dialog" aria-modal="true" aria-labelledby="workspaceKeyDialogTitle">
        <div className="oauthDialogHeader">
          <div>
            <span className="oauthDialogIcon">
              <KeyRound size={18} />
            </span>
            <h2 id="workspaceKeyDialogTitle">工作区密钥已生成</h2>
          </div>
        </div>

        <div className="oauthDialogBody workspaceKeyDialogBody">
          <p className="workspaceKeyDialogHint">
            请立即复制并妥善保存此密钥。保存并关闭后将无法再次查看完整内容，列表中仅保留用途和生成时间。
          </p>
          <button className="workspaceKeyCard" type="button" onClick={copyWorkspaceKey} title="点击复制密钥">
            <span className="workspaceKeyCardValue">{revealed.workspace_key}</span>
            <span className="workspaceKeyCardAction">
              <Copy size={16} />
              点击复制
            </span>
          </button>
          {dialogError && <div className="formError">{dialogError}</div>}
          <label className="field">
            <span>用途（可选）</span>
            <input
              className="input"
              placeholder="留空则自动命名为 密钥_1、密钥_2……"
              value={purposeDraft}
              onChange={(event) => {
                setDialogError("");
                setPurposeDraft(event.target.value);
              }}
            />
          </label>
        </div>

        <div className="oauthDialogFooter workspaceKeyDialogFooter">
          <button className="button primary" disabled={dialogBusy} onClick={savePurpose}>
            {saving ? <Loader2 className="spin" size={16} /> : <CheckCircle2 size={16} />}
            保存
          </button>
        </div>
      </div>
    </div>
  );
}

function SettingsView({
  status,
  settings,
  forwardingLogs,
  backupLogs,
  workspaceKeyRecords,
  automationRuns,
  retryQueue,
  localRetention,
  schedulerStatus,
  busy,
  onSave,
  onUpdateLoginPassword,
  onGenerateWorkspaceKey,
  onUpdateWorkspaceKeyRecord,
  onRefreshWorkspaceKeyRecords,
  onDeleteWorkspaceKeyRecord,
  onShowToast,
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
  workspaceKeyRecords: WorkspaceKeyRecord[];
  automationRuns: AutomationRun[];
  retryQueue: RetryQueueItem[];
  localRetention: LocalRetentionSummary | null;
  schedulerStatus: SchedulerStatus | null;
  busy: boolean;
  onSave: (settings: Settings) => void | Promise<void>;
  onUpdateLoginPassword: (input: UpdateLoginPasswordInput) => Promise<boolean>;
  onGenerateWorkspaceKey: (purpose: string) => Promise<{ record: WorkspaceKeyRecord; workspace_key: string }>;
  onUpdateWorkspaceKeyRecord: (recordId: number, purpose: string) => Promise<WorkspaceKeyRecord>;
  onRefreshWorkspaceKeyRecords: () => Promise<void> | void;
  onDeleteWorkspaceKeyRecord: (recordId: number) => Promise<void> | void;
  onShowToast: (message: string) => void;
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
  const [passwordDraft, setPasswordDraft] = useState({
    current_password: "",
    new_password: "",
    confirm_password: ""
  });
  const [passwordError, setPasswordError] = useState("");
  const [workspaceKeyError, setWorkspaceKeyError] = useState("");
  const [workspaceKeyBusy, setWorkspaceKeyBusy] = useState(false);
  const [revealedWorkspaceKey, setRevealedWorkspaceKey] = useState<{
    recordId: number;
    purpose: string;
    workspace_key: string;
  } | null>(null);
  useEffect(() => setDraft(settings), [settings]);

  const hasClearSelection =
    clearLocal.clear_mail_cache || clearLocal.clear_temp_mail_cache || clearLocal.clear_attachments || clearLocal.clear_exports;
  const settingsChanged = useMemo(() => JSON.stringify(draft) !== JSON.stringify(settings), [draft, settings]);

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

  async function saveLoginPassword() {
    const currentPassword = passwordDraft.current_password;
    const newPassword = passwordDraft.new_password;
    if (!currentPassword) {
      setPasswordError("请输入当前密码");
      return;
    }
    if (newPassword.length < 8) {
      setPasswordError("新密码至少 8 位");
      return;
    }
    if (newPassword !== passwordDraft.confirm_password) {
      setPasswordError("两次输入的新密码不一致");
      return;
    }
    setPasswordError("");
    const saved = await onUpdateLoginPassword({
      current_password: currentPassword,
      new_password: newPassword
    });
    if (saved) {
      setPasswordDraft({
        current_password: "",
        new_password: "",
        confirm_password: ""
      });
    }
  }

  async function generateWorkspaceKey() {
    setWorkspaceKeyError("");
    setWorkspaceKeyBusy(true);
    try {
      const result = await onGenerateWorkspaceKey("");
      setRevealedWorkspaceKey({
        recordId: result.record.id,
        purpose: result.record.purpose,
        workspace_key: result.workspace_key
      });
    } catch (err) {
      setWorkspaceKeyError(readError(err));
    } finally {
      setWorkspaceKeyBusy(false);
    }
  }

  async function closeWorkspaceKeyDialog() {
    setRevealedWorkspaceKey(null);
    await onRefreshWorkspaceKeyRecords();
  }

  async function deleteWorkspaceKeyRecord(recordId: number) {
    if (!window.confirm("确认删除这条工作区密钥记录？删除后无法恢复密钥内容。")) return;
    setWorkspaceKeyError("");
    setWorkspaceKeyBusy(true);
    try {
      await onDeleteWorkspaceKeyRecord(recordId);
      onShowToast("密钥已删除");
    } catch (err) {
      setWorkspaceKeyError(readError(err));
    } finally {
      setWorkspaceKeyBusy(false);
    }
  }

  return (
    <section className="settingsGrid">
      {revealedWorkspaceKey && (
        <WorkspaceKeyRevealDialog
          revealed={revealedWorkspaceKey}
          busy={busy || workspaceKeyBusy}
          onSave={(recordId, purpose) => onUpdateWorkspaceKeyRecord(recordId, purpose)}
          onClose={closeWorkspaceKeyDialog}
          onShowToast={onShowToast}
        />
      )}
      <div className="settingsSaveBar">
        <div>
          <strong>设置草稿</strong>
          <span>{settingsChanged ? "有未保存修改" : "已保存"}</span>
        </div>
        <button className="button primary" disabled={busy || !settingsChanged} onClick={() => onSave(draft)}>
          {busy ? <Loader2 className="spin" size={16} /> : <SettingsIcon size={16} />}
          保存设置
        </button>
      </div>

      <div className="panel">
        <div className="panelHeader">
          <h2>外观</h2>
          <WandSparkles size={18} />
        </div>
        <div className="themePresetGrid">
          {themePresets.map((preset) => (
            <button
              key={preset.id}
              className={draft.appearance_theme === preset.id ? "themePreset active" : "themePreset"}
              onClick={() => setField("appearance_theme", preset.id)}
            >
              <span className="themePreview" style={{ background: preset.rail }}>
                <i style={{ background: normalizeAccent(draft.accent_color) }} />
              </span>
              <strong>{preset.label}</strong>
            </button>
          ))}
        </div>
        <div className="accentPicker">
          <label>
            <span>强调色</span>
            <input
              type="color"
              value={normalizeAccent(draft.accent_color)}
              onChange={(event) => setField("accent_color", event.target.value)}
            />
          </label>
          <div className="accentSwatches">
            {["#b5725f", "#111111", "#8a7a70", "#4a4a45", "#c05f42", "#e0a17f"].map((accent) => (
              <button
                key={accent}
                className={normalizeAccent(draft.accent_color).toLowerCase() === accent ? "accentSwatch active" : "accentSwatch"}
                style={{ background: accent }}
                title={accent}
                onClick={() => setField("accent_color", accent)}
              />
            ))}
          </div>
        </div>
      </div>

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

      <div className="panel widePanel">
        <div className="panelHeader">
          <h2>工作区密钥</h2>
          <KeyRound size={18} />
        </div>
        <p className="oauthHint">
          随机生成 16 字节 workspace key（Base64 编码）。密钥仅在生成时显示一次，保存后无法再次查看；列表仅保留用途和生成时间。用途在生成弹窗中填写，留空时自动命名为 密钥_1、密钥_2……
        </p>
        <div className="workspaceKeyGenerateAction">
          <button
            className="button primary"
            disabled={busy || workspaceKeyBusy || Boolean(revealedWorkspaceKey)}
            onClick={generateWorkspaceKey}
          >
            {busy || workspaceKeyBusy ? <Loader2 className="spin" size={16} /> : <Plus size={16} />}
            生成密钥
          </button>
        </div>
        {workspaceKeyError && <div className="formError">{workspaceKeyError}</div>}

        <div className="logTable workspaceKeyTable">
          <div className="logHeader">
            <span>用途</span>
            <span>生成时间</span>
            <span>操作</span>
          </div>
          {workspaceKeyRecords.map((record) => (
            <div className="logRow" key={record.id}>
              <span>{record.purpose}</span>
              <span>{formatDate(record.created_at)}</span>
              <span className="rowActions">
                <button
                  className="button danger workspaceKeyDeleteButton"
                  title="删除密钥"
                  disabled={busy || workspaceKeyBusy}
                  onClick={() => deleteWorkspaceKeyRecord(record.id)}
                >
                  <Trash2 size={14} />
                  删除
                </button>
              </span>
            </div>
          ))}
        </div>
        {workspaceKeyRecords.length === 0 && <EmptyState icon={<KeyRound size={24} />} text="暂无工作区密钥记录。" />}
      </div>

      <div className="panel">
        <div className="panelHeader">
          <h2>登录密码</h2>
          <Lock size={18} />
        </div>
        <SecretField
          label="当前密码"
          value={passwordDraft.current_password}
          onChange={(value) => {
            setPasswordError("");
            setPasswordDraft((current) => ({ ...current, current_password: value }));
          }}
        />
        <SecretField
          label="新密码"
          value={passwordDraft.new_password}
          onChange={(value) => {
            setPasswordError("");
            setPasswordDraft((current) => ({ ...current, new_password: value }));
          }}
        />
        <SecretField
          label="确认新密码"
          value={passwordDraft.confirm_password}
          onChange={(value) => {
            setPasswordError("");
            setPasswordDraft((current) => ({ ...current, confirm_password: value }));
          }}
        />
        {passwordError && <div className="formError">{passwordError}</div>}
        <button className="button primary" disabled={busy} onClick={saveLoginPassword}>
          {busy ? <Loader2 className="spin" size={16} /> : <KeyRound size={16} />}
          修改密码
        </button>
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
            label="默认刷新邮件数"
            value={draft.scheduler_refresh_top}
            min={1}
            max={1000}
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
  function parseValue(raw: string) {
    const fallback = min || 1;
    let parsed = Number(raw) || fallback;
    if (min !== undefined) parsed = Math.max(min, parsed);
    if (max !== undefined) parsed = Math.min(max, parsed);
    return parsed;
  }

  return (
    <label className="field grow">
      <span>{label}</span>
      <input
        className="input"
        type="number"
        min={min}
        max={max}
        value={value}
        onChange={(event) => onChange(parseValue(event.target.value))}
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

function MailSharePanel({
  records: _records,
  busy: _busy,
  onRevoke: _onRevoke
}: {
  records: MailShareRecord[];
  busy: boolean;
  onRevoke: (shareId: number) => void;
}) {
  // 邮件预览页不展示“本地分享记录”
  return null;
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

async function writeTextToClipboard(value: string) {
  if (navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(value);
    return;
  }

  const textarea = document.createElement("textarea");
  textarea.value = value;
  textarea.style.position = "fixed";
  textarea.style.left = "-9999px";
  textarea.style.top = "0";
  document.body.appendChild(textarea);
  textarea.focus();
  textarea.select();
  try {
    if (!document.execCommand("copy")) {
      throw new Error("copy command failed");
    }
  } finally {
    document.body.removeChild(textarea);
  }
}

function formatDate(value: string) {
  if (!value) return "";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat("zh-CN", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit"
  }).format(date);
}

function formatSenderDisplayName(sender: string) {
  const value = sender.trim();
  if (!value) return "";

  const angleMatch = value.match(/^\s*"?([^"<]*)"?\s*<([^>]+)>/);
  if (angleMatch) {
    const name = cleanSenderName(angleMatch[1]);
    if (name) return name;
    return senderNameFromEmail(angleMatch[2]);
  }

  if (!value.includes("@")) return cleanSenderName(value) || value;
  return senderNameFromEmail(value);
}

function cleanSenderName(value: string) {
  return value
    .replace(/^"+|"+$/g, "")
    .replace(/\s+/g, " ")
    .trim();
}

function senderNameFromEmail(email: string) {
  const normalized = email.trim().replace(/^<|>$/g, "").toLowerCase();
  const domain = normalized.split("@")[1] ?? "";
  const local = normalized.split("@")[0] ?? "";
  const brand = senderBrandFromDomain(domain);
  if (brand) return brand;
  const localName = local
    .replace(/^(no-?reply|noreply|notification|notifications|account|security|mailer|support)[._-]*/i, "")
    .replace(/[._-]+/g, " ")
    .trim();
  const domainParts = domain.split(".").filter((part) => !["com", "net", "org", "cn", "io", "co"].includes(part));
  const source = localName || domainParts[domainParts.length - 1] || email;
  return source.replace(/\b\w/g, (char) => char.toUpperCase());
}

function senderBrandFromDomain(domain: string) {
  const value = domain.toLowerCase();
  if (value.endsWith("openai.com")) return "OpenAI";
  if (value.endsWith("email.claude.com")) return "Claude Team";
  if (value.endsWith("claude.com")) return "Claude";
  if (value.endsWith("anthropic.com")) return "Anthropic";
  if (value.endsWith("google.com") || value.endsWith("gmail.com")) return "Google";
  if (value.endsWith("accountprotection.microsoft.com")) return "Microsoft account";
  if (value.endsWith("microsoft.com")) return "Microsoft";
  if (value.endsWith("github.com")) return "GitHub";
  if (value.endsWith("notion.so")) return "Notion";
  return "";
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

function formatProviderPreview(rows: Array<{ provider: string }>) {
  if (rows.length === 0) return "";
  const counts = rows.reduce((map, row) => {
    const provider = normalizeAccountProviderId(row.provider);
    map.set(provider, (map.get(provider) ?? 0) + 1);
    return map;
  }, new Map<string, number>());
  return Array.from(counts.entries())
    .map(([provider, count]) => `${accountProviderLabel(provider)} ${count}`)
    .join(" / ");
}

function outlookImportBlockReason(rows: Array<{ email: string; provider: string }>) {
  const outlookDomains = accountProviderDefinition("graph").domains;
  const outlookRow = rows.find((row) => {
    const domain = row.email.trim().toLowerCase().split("@").pop() ?? "";
    return normalizeAccountProviderId(row.provider) === "graph" || outlookDomains.includes(domain);
  });
  if (!outlookRow) return "";
  return `Outlook/Microsoft 账号请使用授权页面添加，导入页不再支持导入${outlookRow.email ? `：${outlookRow.email}` : ""}`;
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
  return formatRetryTaskType(item.task_type);
}

function formatRetryTaskType(taskType: string) {
  if (taskType === "mail_mark") return "标记邮件";
  if (taskType === "mail_delete") return "删除邮件";
  if (taskType === "forward_message") return "转发邮件";
  if (taskType === "temp_refresh") return "刷新临时邮箱";
  if (taskType === "refresh_account") return "刷新账号";
  if (taskType === "backup_job") return "执行备份";
  return taskType;
}

function formatErrorCategory(category: string) {
  const map: Record<string, string> = {
    none: "-",
    auth: "认证",
    rate_limit: "限流",
    network: "网络",
    config: "配置",
    storage: "存储",
    data: "数据",
    provider: "服务商",
    unknown: "未知"
  };
  return map[category] ?? category;
}

function formatForwardingChannel(channel: string) {
  const map: Record<string, string> = {
    smtp: "SMTP",
    telegram: "Telegram",
    wecom: "企业微信",
    preview: "预览"
  };
  return map[channel] ?? channel;
}

function formatRetryDelay(item: RetryQueueItem) {
  if (item.due_now) return "已到期";
  if (!item.next_delay_minutes) return "小于 1 分钟";
  if (item.next_delay_minutes < 60) return `${item.next_delay_minutes} 分钟后`;
  return `${(item.next_delay_minutes / 60).toFixed(1)} 小时后`;
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
    expired: "已过期",
    revoked: "已撤销",
    healthy: "健康",
    degraded: "降级",
    open: "熔断",
    not_configured: "未配置",
    none: "-",
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

  const forwardedWithCircuit = message.match(/^Forwarded (\d+) message channel\(s\), (\d+) failed, (\d+) circuit skipped: (.+)$/);
  if (forwardedWithCircuit) {
    const [, forwarded, failed, skipped, detail] = forwardedWithCircuit;
    return `已转发 ${forwarded} 个消息通道，${failed} 个失败，${skipped} 个因熔断跳过：${detail}`;
  }

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
  const message =
    err instanceof Error
      ? err.message
      : typeof err === "object" && err && "message" in err
        ? String((err as { message: unknown }).message)
        : String(err);
  if (message.includes("AADSTS70000") || message.includes("code has expired")) {
    return "OAuth 授权码已过期或已被使用，请重新点击“打开”完成授权，然后粘贴新的回调 URL。";
  }
  return message.replace(/^invalid input:\s*/i, "").replace(/^internal error:\s*/i, "");
}

export default App;
