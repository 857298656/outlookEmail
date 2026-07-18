import { describe, expect, it } from "vitest";
import {
  formatMarkdownImageRequest,
  parseMarkdownImageRequest
} from "./markdownImage";

describe("markdown image request text", () => {
  it("formats and parses a selected image request", () => {
    const request = formatMarkdownImageRequest("Desktop.png", "attachment:549cd35d");

    expect(request).toBe("![Desktop.png](attachment:549cd35d)");
    expect(parseMarkdownImageRequest(request, "fallback.png")).toEqual({
      caption: "Desktop.png",
      src: "attachment:549cd35d"
    });
  });

  it("preserves escaped caption characters", () => {
    const request = formatMarkdownImageRequest("screen]shot.png", "data:image/png;base64,abc");

    expect(parseMarkdownImageRequest(request, "")).toEqual({
      caption: "screen]shot.png",
      src: "data:image/png;base64,abc"
    });
  });

  it("treats plain or malformed text as the request path", () => {
    expect(parseMarkdownImageRequest("attachment:missing", "Desktop.png")).toEqual({
      caption: "Desktop.png",
      src: "attachment:missing"
    });
  });
});
