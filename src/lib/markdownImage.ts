export type MarkdownImageRequest = {
  caption: string;
  src: string;
};

function escapeCaption(value: string) {
  return value.replace(/\\/g, "\\\\").replace(/\]/g, "\\]");
}

function unescapeCaption(value: string) {
  return value.replace(/\\([\\\]])/g, "$1");
}

export function formatMarkdownImageRequest(caption: string, src: string) {
  return `![${escapeCaption(caption)}](${src})`;
}

export function parseMarkdownImageRequest(
  value: string,
  fallbackCaption: string
): MarkdownImageRequest {
  const trimmed = value.trim();
  const match = /^!\[((?:\\.|[^\]])*)\]\(([\s\S]*)\)$/.exec(trimmed);
  if (!match) {
    return {
      caption: fallbackCaption,
      src: trimmed
    };
  }

  return {
    caption: unescapeCaption(match[1] ?? ""),
    src: (match[2] ?? "").trim()
  };
}
