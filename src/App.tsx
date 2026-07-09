import {
  CheckCircle2,
  ChevronDown,
  ChevronUp,
  Copy,
  Download,
  ExternalLink,
  FileText,
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
  Search,
  Settings as SettingsIcon,
  Share2,
  Square,
  Tags,
  Trash2,
  Upload,
  Users,
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
  ClearLocalDataInput,
  ExportResult,
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
  SchedulerStatus,
  Settings,
  Tag,
  UpdateLoginPasswordInput,
  WorkspaceKeyRecord
} from "./types";

type View = "mail" | "accounts" | "settings";
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

function buildSkin(_settings: Settings | null): { className: string; style: SkinStyle } {
  const preset = themePresets[0];
  const accent = normalizeAccent(undefined);
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
  const [settings, setSettings] = useState<Settings | null>(null);
  const [workspaceKeyRecords, setWorkspaceKeyRecords] = useState<WorkspaceKeyRecord[]>([]);
  const [mailShareRecords, setMailShareRecords] = useState<MailShareRecord[]>([]);
  const [localRetention, setLocalRetention] = useState<LocalRetentionSummary | null>(null);
  const [schedulerStatus, setSchedulerStatus] = useState<SchedulerStatus | null>(null);
  const [selectedGroupId, setSelectedGroupId] = useState<number | "all">("all");
  const [selectedAccountId, setSelectedAccountId] = useState<number | undefined>();
  const [selectedMessageId, setSelectedMessageId] = useState<number | undefined>();
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


  async function loadMailShares() {
    setMailShareRecords(await api.listMailShareRecords(80));
  }

  async function loadSettingsData() {
    const [nextSettings, nextWorkspaceKeyRecords, nextSchedulerStatus, nextLocalRetention] = await Promise.all([
      api.getSettings(),
      api.listWorkspaceKeyRecords(),
      api.schedulerStatus(),
      api.getLocalRetentionSummary()
    ]);
    setSettings(nextSettings);
    setWorkspaceKeyRecords(nextWorkspaceKeyRecords);
    setSchedulerStatus(nextSchedulerStatus);
    setLocalRetention(nextLocalRetention);
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
    loadMailShares().catch((err) => setError(readError(err)));
    loadSettingsData().catch((err) => setError(readError(err)));
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
      await loadSettingsData();
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
                await loadSettingsData();
              })
            }
            onDeleteMessages={(messageIds) =>
              runAction(async () => {
                const result = await api.deleteMessages(messageIds);
                setNotice(formatResultMessage(result.message));
                await loadMailboxMessages(selectedAccountId, folder, mailFilters, mailPage, { preservePreview: true });
                await loadStatus();
                await loadSettingsData();
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


        {view === "settings" && settings && (
          <SettingsView
            status={status}
            settings={settings}
            workspaceKeyRecords={workspaceKeyRecords}
            localRetention={localRetention}
            schedulerStatus={schedulerStatus}
            busy={busy}
            onSave={(nextSettings) =>
              runAction(async () => {
                setSettings(await api.updateSettings(nextSettings));
                await loadSettingsData();
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
            onClearLocalData={(input) =>
              runAction(async () => {
                const result = await api.clearLocalData(input);
                setNotice(formatResultMessage(result.message));
                await loadStatus();
                await loadWorkspace(selectedAccountId, folder, mailFilters, mailPage);
                await loadMailShares();
                await loadSettingsData();
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
  onViewRawMessage
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
              <Tags size={14} />
              移动
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
              <Tags size={18} />
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
  workspaceKeyRecords,
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
  onClearLocalData
}: {
  status: AppStatus;
  settings: Settings;
  workspaceKeyRecords: WorkspaceKeyRecord[];
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
  onClearLocalData: (input: ClearLocalDataInput) => void;
}) {
  const [draft, setDraft] = useState(settings);
  const [clearLocal, setClearLocal] = useState({
    clear_mail_cache: false,
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
    clearLocal.clear_mail_cache || clearLocal.clear_attachments || clearLocal.clear_exports;
  const settingsChanged = useMemo(() => JSON.stringify(draft) !== JSON.stringify(settings), [draft, settings]);

  function setField<K extends keyof Settings>(key: K, value: Settings[K]) {
    setDraft((current) => ({ ...current, [key]: value }));
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
        </div>
        {localRetention && (
          <>
            <div className="retentionStats">
              <Stat label="本地邮件" value={localRetention.mail_message_count} />
              <Stat label="未读" value={localRetention.unread_message_count} />
              <Stat label="附件文件" value={localRetention.attachment_file_count} />
              <Stat label="导出文件" value={localRetention.export_file_count} />
            </div>
            <div className="retentionSizeGrid">
              <span>数据库</span>
              <strong>{formatBytes(localRetention.database_size) || "0 B"}</strong>
              <span>附件</span>
              <strong>{formatBytes(localRetention.attachments_size) || "0 B"}</strong>
              <span>导出</span>
              <strong>{formatBytes(localRetention.exports_size) || "0 B"}</strong>
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
    none: "-",
    read: "已读",
    unread: "未读",
    local: "本地"
  };
  return map[status] ?? status;
}



function formatResultMessage(message: string) {
  if (!message) return message;
  if (message === "Refresh job accepted. Provider adapters are available in the Tauri runtime.") {
    return "刷新任务已接收。服务商适配器将在 Tauri 运行时可用。";
  }

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

  const batch = message.match(/^Batch (delete|move_group|add_tags|remove_tags) processed (\d+) account\(s\)$/);
  if (batch) {
    const actionMap: Record<string, string> = {
      delete: "删除",
      move_group: "移动",
      add_tags: "添加标签",
      remove_tags: "移除标签"
    };
    return `已批量${actionMap[batch[1]]} ${batch[2]} 个账号`;
  }

  const clearedLocal = message.match(/^Cleared local data: (\d+) mail message\(s\), (\d+) file\(s\)$/);
  if (clearedLocal) return `已清理本地数据：${clearedLocal[1]} 封邮件、${clearedLocal[2]} 个文件`;

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
