export type AccountProviderId = "graph" | "outlook" | "gmail" | "qq" | "netease_163" | "imap_custom" | "imap";

export type AccountProviderDefinition = {
  id: AccountProviderId;
  label: string;
  credentialLabel: string;
  accountType: string;
  defaultImapHost: string;
  defaultImapPort: number;
  aliases: string[];
  domains: string[];
};

export const accountProviderRegistry: AccountProviderDefinition[] = [
  {
    id: "graph",
    label: "Outlook",
    credentialLabel: "Microsoft OAuth",
    accountType: "outlook",
    defaultImapHost: "",
    defaultImapPort: 993,
    aliases: ["outlook", "microsoft", "msgraph"],
    domains: ["outlook.com", "hotmail.com", "live.com", "msn.com"]
  },
  {
    id: "gmail",
    label: "Gmail",
    credentialLabel: "Google OAuth",
    accountType: "gmail",
    defaultImapHost: "",
    defaultImapPort: 993,
    aliases: ["google", "googlemail"],
    domains: ["gmail.com", "googlemail.com"]
  },
  {
    id: "qq",
    label: "QQ 邮箱",
    credentialLabel: "IMAP 授权码",
    accountType: "imap",
    defaultImapHost: "imap.qq.com",
    defaultImapPort: 993,
    aliases: ["qqmail"],
    domains: ["qq.com", "foxmail.com"]
  },
  {
    id: "imap",
    label: "IMAP",
    credentialLabel: "IMAP OAuth/密码",
    accountType: "imap",
    defaultImapHost: "",
    defaultImapPort: 993,
    aliases: ["outlook_imap"],
    domains: []
  },
  {
    id: "netease_163",
    label: "163 邮箱",
    credentialLabel: "IMAP 授权密码",
    accountType: "imap",
    defaultImapHost: "imap.163.com",
    defaultImapPort: 993,
    aliases: ["163", "netease", "163mail"],
    domains: ["163.com"]
  },
  {
    id: "imap_custom",
    label: "Custom IMAP",
    credentialLabel: "IMAP 密码",
    accountType: "imap",
    defaultImapHost: "",
    defaultImapPort: 993,
    aliases: ["custom_imap", "custom"],
    domains: []
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
