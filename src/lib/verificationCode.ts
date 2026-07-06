export type VerificationCodeSource = {
  subject?: string | null;
  body_preview?: string | null;
  body?: string | null;
};

const verificationKeyword =
  "(?:验证码|校验码|动态码|安全码|登录码|确认码|验证代码|一次性代码|临时验证码|verification\\s+code|security\\s+code|login\\s+code|confirmation\\s+code|authentication\\s+code|one[-\\s]?time\\s+(?:code|password)|passcode|otp|2fa\\s+code|code)";
const tokenPattern =
  "([A-Z0-9]{4}[\\s-][A-Z0-9]{4}|[A-Z0-9]{3}[\\s-][A-Z0-9]{3}|[A-Z0-9]{2}[\\s-][A-Z0-9]{4}|[A-Z0-9]{4,10})";
const blockedCandidates = new Set([
  "CODE",
  "EMAIL",
  "HTTPS",
  "HTTP",
  "LOGIN",
  "VALID",
  "VERIFY",
  "YOUR",
  "THIS"
]);

export function extractVerificationCode(source: VerificationCodeSource): string | null {
  const text = normalizeVerificationText([source.subject, source.body_preview, source.body].filter(Boolean).join("\n"));
  if (!text) return null;

  for (const pattern of [
    new RegExp(`${verificationKeyword}[^\\n\\r]{0,80}?\\b${tokenPattern}\\b`, "giu"),
    new RegExp(`\\b${tokenPattern}\\b[^\\n\\r]{0,80}?${verificationKeyword}`, "giu")
  ]) {
    const code = firstCodeMatch(text, pattern);
    if (code) return code;
  }

  if (!new RegExp(verificationKeyword, "iu").test(text)) return null;
  return firstCodeMatch(text, new RegExp(`\\b${tokenPattern}\\b`, "giu"));
}

function firstCodeMatch(text: string, pattern: RegExp) {
  for (const match of text.matchAll(pattern)) {
    const raw = match[1];
    const code = normalizeVerificationCandidate(raw);
    if (code) return code;
  }
  return null;
}

function normalizeVerificationCandidate(raw: string | undefined) {
  const code = (raw ?? "").replace(/[\s-]+/g, "").toUpperCase();
  if (!/^[A-Z0-9]{4,10}$/.test(code)) return null;
  if (!/[0-9]/.test(code)) return null;
  if (/^\d+$/.test(code) && (code.length < 4 || code.length > 8)) return null;
  if (!/^\d+$/.test(code) && code.length < 5) return null;
  if (blockedCandidates.has(code)) return null;
  return code;
}

function normalizeVerificationText(value: string) {
  return value
    .replace(/<style[\s\S]*?<\/style>/gi, " ")
    .replace(/<script[\s\S]*?<\/script>/gi, " ")
    .replace(/<[^>]+>/g, " ")
    .replace(/&nbsp;/gi, " ")
    .replace(/&amp;/gi, "&")
    .replace(/&#(\d+);/g, (_, code) => String.fromCharCode(Number(code)))
    .replace(/\s+/g, " ")
    .trim();
}
