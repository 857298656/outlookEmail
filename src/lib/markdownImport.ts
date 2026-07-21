export const MARKDOWN_FILE_EXTENSIONS = ["md", "markdown"] as const;
export const MARKDOWN_IMPORT_EXTENSIONS = [
  ...MARKDOWN_FILE_EXTENSIONS,
  "txt",
  "json"
] as const;

export function markdownImportTitle(fileName: string) {
  return fileName.replace(/\.(md|markdown|txt|json)$/i, "").trim();
}
