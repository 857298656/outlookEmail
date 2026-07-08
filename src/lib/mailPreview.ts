export type MailPreviewSource = {
  body_preview: string;
  body?: string | null;
};

export function formatMessageListPreview(message: MailPreviewSource) {
  const preview = formatMailPreview(message.body_preview);
  if (preview) return preview;
  return message.body ? formatMailPreview(message.body) : "";
}

export function formatMailPreview(value: string) {
  const withoutBlocks = value
    .replace(/<script\b[^>]*>[\s\S]*?<\/script>/gi, " ")
    .replace(/<style\b[^>]*>[\s\S]*?<\/style>/gi, " ");
  const withBreaks = withoutBlocks.replace(/<\/?(br|p|div|li|tr|td|table|ul|ol|h[1-6])\b[^>]*>/gi, " ");
  const withoutTags = withBreaks.replace(/<[^>]+>/g, " ");
  return stripCssFragments(decodeHtmlEntities(withoutTags)).replace(/\s+/g, " ").trim();
}

function stripCssFragments(value: string) {
  let text = value.replace(/\/\*[\s\S]*?\*\//g, " ");
  for (let index = 0; index < 16; index += 1) {
    const next = stripOneCssFragment(text);
    if (next === text) break;
    text = next;
  }
  return stripStandaloneCssDeclarations(text);
}

function stripOneCssFragment(value: string) {
  let cursor = 0;
  while (cursor < value.length) {
    const open = value.indexOf("{", cursor);
    if (open < 0) return value;
    const close = value.indexOf("}", open + 1);
    const blockEnd = close >= 0 ? close : value.length;
    const block = value.slice(open + 1, blockEnd);
    if (looksLikeCssDeclarationBlock(block)) {
      const start = cssSelectorStart(value, open);
      if (start >= 0) {
        return `${value.slice(0, start)} ${value.slice(close >= 0 ? close + 1 : value.length)}`;
      }
    }
    if (close < 0) return value;
    cursor = close + 1;
  }
  return value;
}

function looksLikeCssDeclarationBlock(value: string) {
  return /(?:^|[;\s])-{0,2}[a-z][a-z-]*\s*:/i.test(value) || /!important|#[0-9a-f]{3,8}\b|url\(/i.test(value);
}

function cssSelectorStart(value: string, open: number) {
  const prefixStart = Math.max(0, open - 260);
  const prefix = value.slice(prefixStart, open);
  const trimmedLength = prefix.trimEnd().length;
  const trimmed = prefix.slice(0, trimmedLength);
  if (!trimmed) return -1;

  const markers = Array.from(trimmed.matchAll(/[@.#*\[]/g)).reverse();
  for (const marker of markers) {
    let start = marker.index ?? -1;
    if (start < 0) continue;
    if (trimmed[start] === "[") start = selectorTokenStart(trimmed, start);
    if (looksLikeCssSelector(trimmed.slice(start))) return prefixStart + start;
  }

  const tailStart = Math.max(trimmed.lastIndexOf(" "), trimmed.lastIndexOf("\n"), trimmed.lastIndexOf("\t"), trimmed.lastIndexOf("\r")) + 1;
  return looksLikeCssSelector(trimmed.slice(tailStart)) ? prefixStart + tailStart : -1;
}

function selectorTokenStart(value: string, index: number) {
  let start = index;
  while (start > 0 && /[\w-]/.test(value[start - 1])) start -= 1;
  return start;
}

function looksLikeCssSelector(value: string) {
  const selector = value.trim();
  if (!selector || selector.length > 260 || /[;{}]/.test(selector)) return false;
  if (/^@[a-z-]+$/i.test(selector)) return true;
  if (/^\.[a-z_-][\w-]*(?:[\s.#:[\]>+~,"'=\w-]*)?$/i.test(selector)) return true;
  if (/^#[a-z_-][\w-]*(?:[\s.#:[\]>+~,"'=\w-]*)?$/i.test(selector)) return true;
  if (/^\*(?:[\s.#:[\]>+~,"'=\w-]*)?$/i.test(selector)) return true;
  return /^(?:body|html|a|p|div|span|table|td|th|img|font|strong|em|u|h[1-6])(?:[\s.#:[\]>+~,"'=\w-]*)?$/i.test(selector);
}

function stripStandaloneCssDeclarations(value: string) {
  const cssProperty =
    "(?:-webkit-[a-z-]+|-ms-[a-z-]+|mso-[a-z-]+|background(?:-color|-image|-size|-position)?|border(?:-radius|-color|-style|-width)?|box-sizing|color|display|font(?:-family|-size|-style|-weight)?|height|line-height|margin(?:-[a-z]+)?|max-width|min-width|opacity|padding(?:-[a-z]+)?|text(?:-align|-decoration|-transform)?|vertical-align|white-space|width)";
  const declarations = new RegExp(`(?:^|\\s)(?:${cssProperty}\\s*:\\s*[^;{}]{0,180};\\s*)+(?:${cssProperty}\\s*:\\s*[^.;{}]{0,180})?`, "gi");
  return value.replace(declarations, " ").replace(/(?:^|\s)@(font-face|media|supports|keyframes)\b[^.;。！？\n]{0,240}/gi, " ");
}

function decodeHtmlEntities(value: string) {
  if (typeof document !== "undefined") {
    const textarea = document.createElement("textarea");
    textarea.innerHTML = value;
    return textarea.value;
  }

  return value.replace(/&(#\d+|#x[\da-f]+|amp|lt|gt|quot|apos|nbsp);/gi, (_, entity: string) => {
    const normalized = entity.toLowerCase();
    if (normalized.startsWith("#x")) return String.fromCodePoint(Number.parseInt(normalized.slice(2), 16));
    if (normalized.startsWith("#")) return String.fromCodePoint(Number.parseInt(normalized.slice(1), 10));
    const named: Record<string, string> = {
      amp: "&",
      lt: "<",
      gt: ">",
      quot: "\"",
      apos: "'",
      nbsp: " "
    };
    return named[normalized] ?? _;
  });
}
