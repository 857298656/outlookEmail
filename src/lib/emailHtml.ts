const EMAIL_HTML_CSP = [
  "default-src 'none'",
  "img-src data: blob: cid:",
  "media-src data: blob:",
  "font-src data:",
  "style-src 'unsafe-inline'",
  "object-src 'none'",
  "frame-src 'none'",
  "form-action 'none'",
  "base-uri 'none'"
].join("; ");

const blockedTags = ["script", "iframe", "object", "embed", "base", "link", "meta", "form", "input", "button", "textarea", "select", "option"];

export function buildSandboxedEmailHtml(html: string) {
  const sanitized = sanitizeEmailHtml(html);
  return [
    "<!doctype html>",
    "<html>",
    "<head>",
    "<meta charset=\"utf-8\" />",
    `<meta http-equiv="Content-Security-Policy" content="${EMAIL_HTML_CSP}" />`,
    "<style>",
    "html,body{margin:0;padding:0;background:#fff;color:#273444;font:14px/1.6 Segoe UI,Arial,sans-serif;overflow-wrap:anywhere}",
    "body{padding:16px}",
    "img{max-width:100%;height:auto}",
    "table{max-width:100%;border-collapse:collapse}",
    "a{color:#d97757}",
    "</style>",
    "</head>",
    `<body>${sanitized}</body>`,
    "</html>"
  ].join("");
}

export function sanitizeEmailHtml(html: string) {
  if (!html.trim()) return "";
  const parser = new DOMParser();
  const document = parser.parseFromString(html, "text/html");
  document.body.querySelectorAll(blockedTags.join(",")).forEach((element) => element.remove());
  document.body.querySelectorAll("*").forEach((element) => {
    for (const attribute of Array.from(element.attributes)) {
      const name = attribute.name.toLowerCase();
      const value = attribute.value.trim();
      if (
        name.startsWith("on") ||
        name === "srcdoc" ||
        ((name === "src" || name === "href" || name === "xlink:href") && hasUnsafeUrl(value))
      ) {
        element.removeAttribute(attribute.name);
      }
    }
    if (element instanceof HTMLAnchorElement && element.href) {
      element.target = "_blank";
      element.rel = "noreferrer noopener";
    }
  });
  return document.body.innerHTML;
}

function hasUnsafeUrl(value: string) {
  const normalized = value.replace(/[\u0000-\u001f\s]+/g, "").toLowerCase();
  return normalized.startsWith("javascript:") || normalized.startsWith("vbscript:") || normalized.startsWith("file:");
}
