import { describe, expect, it } from "vitest";
import {
  MARKDOWN_FILE_EXTENSIONS,
  MARKDOWN_IMPORT_EXTENSIONS,
  markdownImportTitle
} from "./markdownImport";

describe("Markdown workspace imports", () => {
  it("offers Markdown, TXT, and JSON for imports while keeping linked files Markdown-only", () => {
    expect(MARKDOWN_IMPORT_EXTENSIONS).toEqual(["md", "markdown", "txt", "json"]);
    expect(MARKDOWN_FILE_EXTENSIONS).toEqual(["md", "markdown"]);
  });

  it.each([
    ["release.md", "release"],
    ["notes.MARKDOWN", "notes"],
    ["readme.txt", "readme"],
    ["settings.JSON", "settings"]
  ])("derives the note title from %s", (fileName, expected) => {
    expect(markdownImportTitle(fileName)).toBe(expected);
  });
});
