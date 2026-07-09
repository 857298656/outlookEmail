export type AccountProviderId = "graph" | "outlook" | "gmail" | "qq" | "netease_163" | "imap_custom" | "imap";
export type AccountProviderCapability =
  | "read_mail"
  | "download_attachments"
  | "mark_read"
  | "remote_delete"
  | "trash"
  | "history_sync"
  | "imap_folders";

export type AccountProviderDefinition = {
  id: AccountProviderId;
  label: string;
  credentialLabel: string;
  credentialPlaceholder: string;
  setupHint: string;
  accountType: string;
  defaultImapHost: string;
  defaultImapPort: number;
  aliases: string[];
  domains: string[];
  capabilities: AccountProviderCapability[];
};

export type ProviderReadinessInput = {
  provider?: string | null;
  account_type?: string | null;
  has_client_id?: boolean | null;
  has_refresh_token?: boolean | null;
  imap_host?: string | null;
  imap_port?: number | null;
};

export type ProviderReadinessStatus = "ready" | "missing";

export type ProviderReadinessResult = {
  status: ProviderReadinessStatus;
  label: string;
  detail: string;
  missing: string[];
};

export const accountProviderRegistry: AccountProviderDefinition[] = [
  {
    id: "graph",
    label: "Outlook",
    credentialLabel: "Microsoft OAuth",
    credentialPlaceholder: "Outlook 密码，可选",
    setupHint: "Outlook Graph 账号优先使用 Microsoft OAuth；需要 IMAP OAuth 时可在账号设置中生成授权链接。",
    accountType: "outlook",
    defaultImapHost: "",
    defaultImapPort: 993,
    aliases: ["outlook", "microsoft", "msgraph"],
    domains: ["outlook.com", "hotmail.com", "live.com", "msn.com"],
    capabilities: ["read_mail", "download_attachments", "mark_read", "remote_delete"]
  },
  {
    id: "gmail",
    label: "Gmail",
    credentialLabel: "Gmail 应用专用密码",
    credentialPlaceholder: "Gmail 应用专用密码",
    setupHint: "Gmail 当前使用 IMAP 接入；请在 Gmail 设置中开启 IMAP，并填写 Google 账号应用专用密码，不是网页登录密码。",
    accountType: "imap",
    defaultImapHost: "imap.gmail.com",
    defaultImapPort: 993,
    aliases: ["google", "googlemail"],
    domains: ["gmail.com", "googlemail.com"],
    capabilities: ["read_mail", "download_attachments", "mark_read", "remote_delete", "imap_folders"]
  },
  {
    id: "qq",
    label: "QQ 邮箱",
    credentialLabel: "IMAP 授权码",
    credentialPlaceholder: "QQ 邮箱 IMAP/SMTP 授权码",
    setupHint: "QQ 邮箱请先登录邮箱，在 设置 > 账号安全 > 安全设置 > POP3/IMAP/SMTP 服务 中开启 IMAP/SMTP 并生成授权码；这里填写授权码，不是 QQ 登录密码。",
    accountType: "imap",
    defaultImapHost: "imap.qq.com",
    defaultImapPort: 993,
    aliases: ["qqmail"],
    domains: ["qq.com", "foxmail.com"],
    capabilities: ["read_mail", "download_attachments", "mark_read", "remote_delete", "imap_folders"]
  },
  {
    id: "imap",
    label: "IMAP",
    credentialLabel: "IMAP OAuth/密码",
    credentialPlaceholder: "IMAP 密码或 OAuth token",
    setupHint: "通用 IMAP 账号使用已配置的 host、port 和 IMAP 密码；Outlook IMAP OAuth 仍可使用 Client ID 和 OAuth 链接。",
    accountType: "imap",
    defaultImapHost: "",
    defaultImapPort: 993,
    aliases: ["outlook_imap"],
    domains: [],
    capabilities: ["read_mail", "download_attachments", "mark_read", "remote_delete", "imap_folders"]
  },
  {
    id: "netease_163",
    label: "163 邮箱",
    credentialLabel: "IMAP 授权密码",
    credentialPlaceholder: "163 客户端授权密码",
    setupHint: "163 邮箱请先登录邮箱，在 设置 > POP3/SMTP/IMAP 中开启服务并生成客户端授权密码；这里填写授权密码，不是网页登录密码。",
    accountType: "imap",
    defaultImapHost: "imap.163.com",
    defaultImapPort: 993,
    aliases: ["163", "netease", "163mail"],
    domains: ["163.com"],
    capabilities: ["read_mail", "download_attachments", "mark_read", "remote_delete", "imap_folders"]
  },
  {
    id: "imap_custom",
    label: "Custom IMAP",
    credentialLabel: "IMAP 密码",
    credentialPlaceholder: "IMAP 密码",
    setupHint: "Custom IMAP 需要手动填写 host、port，以及该邮箱服务商提供的 IMAP 密码或应用密码。",
    accountType: "imap",
    defaultImapHost: "",
    defaultImapPort: 993,
    aliases: ["custom_imap", "custom"],
    domains: [],
    capabilities: ["read_mail", "download_attachments", "mark_read", "remote_delete", "imap_folders"]
  }
];

export function normalizeAccountProviderId(value?: string | null): AccountProviderId {
  const normalized = (value ?? "").trim().toLowerCase();
  if (!normalized) return "imap_custom";
  const direct = accountProviderRegistry.find((provider) => provider.id === normalized);
  if (direct) return direct.id;
  const alias = accountProviderRegistry.find((provider) => provider.aliases.includes(normalized));
  return alias?.id ?? "imap_custom";
}

export function accountProviderDefinition(value?: string | null): AccountProviderDefinition {
  const providerId = normalizeAccountProviderId(value);
  return accountProviderRegistry.find((provider) => provider.id === providerId) ?? accountProviderRegistry[accountProviderRegistry.length - 1];
}

export function detectAccountProvider(email: string, hasRefreshToken = false, explicitProvider?: string | null): AccountProviderId {
  if (explicitProvider?.trim()) return normalizeAccountProviderId(explicitProvider);
  if (hasRefreshToken) return "graph";
  const domain = email.trim().toLowerCase().split("@").pop() ?? "";
  const provider = accountProviderRegistry.find((item) => item.domains.includes(domain));
  return provider?.id ?? "imap_custom";
}

export function accountProviderLabel(value?: string | null): string {
  return accountProviderDefinition(value).label;
}

export function providerAccountType(value?: string | null): string {
  return accountProviderDefinition(value).accountType;
}

export function providerDefaultImap(value?: string | null) {
  const provider = accountProviderDefinition(value);
  return {
    host: provider.defaultImapHost,
    port: provider.defaultImapPort
  };
}

export function providerSupportsCapability(value: string | null | undefined, capability: AccountProviderCapability): boolean {
  return accountProviderDefinition(value).capabilities.includes(capability);
}

export function providerCapabilitySummary(value?: string | null): string {
  const labels: Record<AccountProviderCapability, string> = {
    read_mail: "读取",
    download_attachments: "附件",
    mark_read: "已读/未读",
    remote_delete: "远端删除",
    trash: "移入垃圾箱",
    history_sync: "增量同步",
    imap_folders: "IMAP 文件夹"
  };
  return accountProviderDefinition(value)
    .capabilities.map((capability) => labels[capability])
    .join(" / ");
}

export function providerReadiness(account: ProviderReadinessInput): ProviderReadinessResult {
  const provider = accountProviderDefinition(account.provider ?? account.account_type);
  const host = (account.imap_host ?? provider.defaultImapHost).trim();
  const port = account.imap_port ?? provider.defaultImapPort;
  const hasClientId = Boolean(account.has_client_id);
  const hasRefreshToken = Boolean(account.has_refresh_token);
  const missing: string[] = [];

  if (provider.accountType !== "imap") {
    if (!hasClientId) missing.push("Microsoft Client ID");
    if (!hasRefreshToken) missing.push("Microsoft refresh token");
    return readinessResult(missing, provider.credentialLabel, `${provider.credentialLabel} 已保存`);
  }

  if (!host) missing.push("IMAP host");
  if (!Number.isFinite(port) || port < 1 || port > 65535) missing.push("IMAP port");

  if (provider.id === "qq") {
    if (!hasRefreshToken) missing.push("QQ IMAP/SMTP 授权码");
    return readinessResult(missing, provider.credentialLabel, `${provider.credentialLabel} + ${host || provider.defaultImapHost}:${port || provider.defaultImapPort}`);
  }

  if (provider.id === "netease_163") {
    if (!hasRefreshToken) missing.push("163 客户端授权密码");
    return readinessResult(missing, provider.credentialLabel, `${provider.credentialLabel} + ${host || provider.defaultImapHost}:${port || provider.defaultImapPort}`);
  }

  if (provider.id === "gmail") {
    if (!hasRefreshToken) missing.push("Gmail 应用专用密码");
    return readinessResult(missing, provider.credentialLabel, `${provider.credentialLabel} + ${host || provider.defaultImapHost}:${port || provider.defaultImapPort}`);
  }

  const hasImapOAuth = hasClientId && hasRefreshToken;
  if (provider.id === "imap") {
    if (!hasImapOAuth) missing.push("IMAP OAuth client ID 和 refresh token");
    return readinessResult(missing, "IMAP OAuth", `IMAP OAuth + ${host}:${port}`);
  }

  if (!hasRefreshToken) {
    missing.push("IMAP 密码或应用密码");
  }

  return readinessResult(missing, provider.credentialLabel, `${provider.credentialLabel} + ${host}:${port}`);
}

function readinessResult(missing: string[], readyLabel: string, readyDetail: string): ProviderReadinessResult {
  if (missing.length > 0) {
    return {
      status: "missing",
      label: "缺少配置",
      detail: `缺少 ${missing.join("、")}`,
      missing
    };
  }
  return {
    status: "ready",
    label: readyLabel,
    detail: readyDetail,
    missing
  };
}

export function providerFailureHint(value?: string | null, error?: string | null): string {
  const provider = accountProviderDefinition(value);
  const lower = (error ?? "").toLowerCase();
  const isAuthError =
    lower.includes("auth") ||
    lower.includes("unauthorized") ||
    lower.includes("forbidden") ||
    lower.includes("http 401") ||
    lower.includes("http 403") ||
    lower.includes("invalid_grant") ||
    lower.includes("insufficientpermissions") ||
    lower.includes("scope") ||
    lower.includes("credential") ||
    lower.includes("password") ||
    lower.includes("token") ||
    lower.includes("login") ||
    lower.includes("授权码") ||
    lower.includes("授权密码") ||
    lower.includes("客户端授权") ||
    lower.includes("网页登录密码") ||
    lower.includes("未授权") ||
    lower.includes("权限不足") ||
    lower.includes("认证") ||
    lower.includes("鉴权") ||
    lower.includes("令牌");
  const isNetworkError =
    lower.includes("timeout") ||
    lower.includes("timed out") ||
    lower.includes("connection") ||
    lower.includes("connect") ||
    lower.includes("dns") ||
    lower.includes("network") ||
    lower.includes("proxy") ||
    lower.includes("tls") ||
    lower.includes("refused");

  if (provider.id === "gmail" && lower.includes("history") && lower.includes("404")) {
    return "Gmail 当前走 IMAP 同步；请确认该账号已配置 IMAP host、端口和应用专用密码。";
  }
  if (provider.id === "gmail" && isAuthError) {
    return "检查 Gmail 已开启 IMAP，并填写 Google 账号应用专用密码，不是网页登录密码。";
  }
  if (provider.id === "graph" && isAuthError) {
    return "重新授权 Outlook，确认 Microsoft Client ID 和回调地址与设置一致。";
  }
  if (provider.id === "qq" && isAuthError) {
    return "检查 QQ 邮箱已开启 IMAP/SMTP，并填写网页端生成的授权码。";
  }
  if (provider.id === "netease_163" && isAuthError) {
    return "检查 163 已开启客户端授权，并填写授权密码或应用密码。";
  }
  if ((provider.id === "imap" || provider.id === "imap_custom") && isAuthError) {
    return "检查 IMAP host、端口、TLS 和该服务商提供的应用密码。";
  }
  if (isNetworkError) {
    return "检查账号代理链、网络连通性和服务商访问限制。";
  }
  if (provider.id === "qq" || provider.id === "netease_163") {
    return "优先确认 IMAP 已开启、授权码有效，再检查文件夹映射和远端 UID 操作。";
  }
  return provider.setupHint || "查看错误详情并按服务商要求修复配置后重试。";
}
