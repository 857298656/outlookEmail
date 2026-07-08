import { detectAccountProvider, normalizeAccountProviderId } from "./providerRegistry";

export type ParsedAccount = {
  email: string;
  password: string;
  client_id: string;
  refresh_token: string;
  remark: string;
  provider: string;
};

export type ParseAccountRowsOptions = {
  defaultProvider?: string | null;
};

export function parseAccountRows(raw: string, options: ParseAccountRowsOptions = {}): ParsedAccount[] {
  const defaultProvider = normalizeDefaultProvider(options.defaultProvider);
  return raw
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.length > 0 && !line.startsWith("#"))
    .map((line) => parseParts(splitLine(line), defaultProvider))
    .filter((row): row is ParsedAccount => row !== null);
}

export function rawWithDefaultProvider(raw: string, provider?: string | null): string {
  const defaultProvider = normalizeDefaultProvider(provider);
  if (!defaultProvider) return raw;

  return raw
    .split(/\r?\n/)
    .map((line) => {
      const trimmed = line.trim();
      if (!trimmed || trimmed.startsWith("#")) return line;
      return `provider=${defaultProvider}----${line}`;
    })
    .join("\n");
}

function splitLine(line: string): string[] {
  const delimiter = ["----", "|||", "\t", ","].find((item) => line.includes(item));
  return delimiter ? line.split(delimiter) : [line];
}

function parseParts(parts: string[], defaultProvider = ""): ParsedAccount | null {
  const trimmedParts = parts.map((part) => part.trim());
  const explicitProvider = findExplicitProvider(trimmedParts);
  const positionalParts = trimmedParts.filter((part) => !isProviderAssignment(part));
  let provider = defaultProvider || explicitProvider;

  if (!provider && positionalParts.length >= 2 && isProviderToken(positionalParts[0]) && positionalParts[1].includes("@")) {
    provider = positionalParts[0];
    positionalParts.shift();
  }

  const emailIndex = positionalParts.findIndex((part) => part.includes("@"));
  if (emailIndex < 0) return null;

  const email = positionalParts[emailIndex].toLowerCase();
  const password = positionalParts[emailIndex + 1] ?? "";
  const client_id = positionalParts[emailIndex + 2] ?? "";
  const refresh_token = positionalParts[emailIndex + 3] ?? "";
  const remark = positionalParts[emailIndex + 4] ?? "";
  const detectedProvider = detectAccountProvider(email, Boolean(refresh_token), provider);

  return {
    email,
    password,
    client_id,
    refresh_token,
    remark,
    provider: detectedProvider
  };
}

function normalizeDefaultProvider(value?: string | null) {
  const trimmed = (value ?? "").trim();
  if (!trimmed || trimmed === "auto") return "";
  return normalizeAccountProviderId(trimmed);
}

function findExplicitProvider(parts: string[]) {
  for (const part of parts) {
    if (!isProviderAssignment(part)) continue;
    const value = part.split(/[:=]/, 2)[1]?.trim();
    if (value) return normalizeAccountProviderId(value);
  }
  return "";
}

function isProviderAssignment(value: string) {
  return /^provider\s*[:=]/i.test(value);
}

function isProviderToken(value: string) {
  const normalized = normalizeAccountProviderId(value);
  return normalized !== "imap_custom" || ["imap", "custom", "custom_imap", "imap_custom"].includes(value.trim().toLowerCase());
}
