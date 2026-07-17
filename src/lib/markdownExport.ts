const EDITOR_ONLY_SELECTOR = [
  ".markdownTableToolbar",
  ".markdownInlineLinkSyntax",
  ".markdownInlineLinkEditor",
  ".milkdown-block-handle",
  ".milkdown-toolbar",
  ".milkdown-top-bar",
  ".tools",
  ".drag-preview",
  ".ProseMirror-gapcursor",
  "button",
  "input",
  "select",
  "textarea"
].join(",");

const UNSAFE_ELEMENT_SELECTOR = "script, style, iframe, object, embed, form, link, meta";
const EDITOR_ONLY_ATTRIBUTES = new Set([
  "contenteditable",
  "draggable",
  "spellcheck",
  "tabindex"
]);

function escapeHtml(value: string) {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function cleanUrlAttribute(element: Element, attribute: "href" | "src") {
  const value = element.getAttribute(attribute)?.trim();
  if (!value) return;
  const normalized = value.replace(/[\u0000-\u0020]+/g, "").toLocaleLowerCase();
  if (
    normalized.startsWith("javascript:") ||
    normalized.startsWith("vbscript:") ||
    (normalized.startsWith("data:") &&
      !(element instanceof HTMLImageElement && normalized.startsWith("data:image/")))
  ) {
    element.removeAttribute(attribute);
  }
}

function replaceCodeEditor(block: Element) {
  const content = block.querySelector(".cm-content");
  if (!content) return;
  const lines = Array.from(content.querySelectorAll(".cm-line"));
  const text = lines.length > 0
    ? lines.map((line) => line.textContent ?? "").join("\n")
    : content.textContent ?? "";
  const pre = block.ownerDocument.createElement("pre");
  const code = block.ownerDocument.createElement("code");
  code.textContent = text;
  pre.append(code);
  block.replaceWith(pre);
}

export function cleanMarkdownEditorHtml(rendered: HTMLElement) {
  const clone = rendered.cloneNode(true) as HTMLElement;
  clone.querySelectorAll(".milkdown-code-block").forEach(replaceCodeEditor);
  clone.querySelectorAll(UNSAFE_ELEMENT_SELECTOR).forEach((element) => element.remove());
  clone.querySelectorAll(EDITOR_ONLY_SELECTOR).forEach((element) => element.remove());
  clone.querySelectorAll("*").forEach((element) => {
    Array.from(element.attributes).forEach((attribute) => {
      if (
        EDITOR_ONLY_ATTRIBUTES.has(attribute.name) ||
        attribute.name.startsWith("data-") ||
        attribute.name.startsWith("aria-") ||
        attribute.name.startsWith("on")
      ) {
        element.removeAttribute(attribute.name);
      }
    });
    cleanUrlAttribute(element, "href");
    cleanUrlAttribute(element, "src");
  });
  return clone.innerHTML;
}

export function standaloneMarkdownHtml(title: string, body: string) {
  return `<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>${escapeHtml(title)}</title>
  <style>
    body { max-width: 980px; margin: 0 auto; padding: 48px; color: #202020; font: 16px/1.75 system-ui, "Microsoft YaHei", sans-serif; }
    img { max-width: 100%; height: auto; } table { width: 100%; border-collapse: collapse; }
    th, td { padding: 8px 10px; border: 1px solid #d9d9d9; } blockquote { margin-left: 0; padding-left: 16px; border-left: 4px solid #ddd; color: #666; }
    pre { padding: 14px; overflow: auto; border-radius: 6px; background: #f5f5f5; } code { font-family: Consolas, monospace; }
  </style>
</head>
<body>${body}</body>
</html>`;
}
