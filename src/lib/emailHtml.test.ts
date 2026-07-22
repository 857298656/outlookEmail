import { describe, expect, it, vi } from "vitest";
import {
  bindExternalEmailLinks,
  buildSandboxedEmailHtml,
  normalizeExternalEmailUrl,
  sanitizeEmailHtml
} from "./emailHtml";

describe("email HTML rendering", () => {
  it("removes active content and unsafe attributes", () => {
    const sanitized = sanitizeEmailHtml(
      '<div onclick="alert(1)">Hello<script>alert(2)</script><img src="javascript:alert(3)" onerror="alert(4)"><a href="javascript:alert(5)">x</a></div>'
    );

    expect(sanitized).toContain("Hello");
    expect(sanitized).not.toContain("<script");
    expect(sanitized).not.toContain("onclick");
    expect(sanitized).not.toContain("onerror");
    expect(sanitized).not.toContain("javascript:");
  });

  it("wraps email HTML with a restrictive content security policy", () => {
    const document = buildSandboxedEmailHtml("<p>Body</p>");

    expect(document).toContain("Content-Security-Policy");
    expect(document).toContain("default-src 'none'");
    expect(document).toContain("form-action 'none'");
    expect(document).toContain("<p>Body</p>");
  });

  it("only accepts absolute HTTP(S) links for the default browser", () => {
    expect(normalizeExternalEmailUrl("https://example.com/verify?id=1")).toBe(
      "https://example.com/verify?id=1"
    );
    expect(normalizeExternalEmailUrl("//example.com/verify")).toBe(
      "https://example.com/verify"
    );
    expect(normalizeExternalEmailUrl("mailto:test@example.com")).toBeNull();
    expect(normalizeExternalEmailUrl("javascript:alert(1)")).toBeNull();
    expect(normalizeExternalEmailUrl("/relative/path")).toBeNull();
  });

  it("intercepts links inside the sandbox frame", () => {
    const frame = document.createElement("iframe");
    document.body.append(frame);
    const frameDocument = frame.contentDocument;
    expect(frameDocument).not.toBeNull();
    if (!frameDocument) return;
    frameDocument.body.innerHTML =
      '<a href="https://example.com/verify"><span id="button-label">Verify</span></a>';
    const onOpen = vi.fn();
    const unbind = bindExternalEmailLinks(frame, onOpen);
    const click = new MouseEvent("click", { bubbles: true, cancelable: true, button: 0 });

    const dispatched = frameDocument.getElementById("button-label")?.dispatchEvent(click);

    expect(dispatched).toBe(false);
    expect(onOpen).toHaveBeenCalledWith("https://example.com/verify");
    unbind();
    frame.remove();
  });
});
