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
    credentialLabel: "Google OAuth",
    credentialPlaceholder: "Gmail 不使用网页登录密码",
    setupHint: "Gmail 使用 Google OAuth；已用旧 scope 授权的 Gmail 账号需要重新授权后才能远端标记已读和移入垃圾箱。",
    accountType: "gmail",
    defaultImapHost: "",
    defaultImapPort: 993,
    aliases: ["google", "googlemail"],
    domains: ["gmail.com", "googlemail.com"],
    capabilities: ["read_mail", "download_attachments", "mark_read", "trash", "history_sync"]
  },
  {
    id: "qq",
    label: "QQ 邮箱",
    credentialLabel: "IMAP 授权码",
    credentialPlaceholder: "QQ 邮箱 IMAP/SMTP 授权码",
    setupHint: "QQ 邮箱请填写网页端生成的 IMAP/SMTP 客户端授权码，不是 QQ 登录密码；导入行的 password 字段也按授权码处理。",
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
    setupHint: "163 邮箱请填写客户端授权密码或应用密码，不是网页登录密码；导入行的 password 字段也按授权密码处理。",
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
