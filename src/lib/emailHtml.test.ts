import { describe, expect, it } from "vitest";
import { buildSandboxedEmailHtml, sanitizeEmailHtml } from "./emailHtml";

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
});
