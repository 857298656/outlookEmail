import { describe, expect, it } from "vitest";
import { formatMailPreview, formatMessageListPreview } from "./mailPreview";

describe("formatMailPreview", () => {
  it("strips Gmail selector CSS from cached preview text", () => {
    const preview =
      ".aw a {color: #FFFFFF; text-decoration: none;} .abml a {color: #000000; font-family: Roboto-Medium,Helvetica,Arial";

    expect(formatMailPreview(preview)).toBe("");
  });

  it("strips compact Gmail and Apple detector CSS", () => {
    const preview = "*{box-sizing:border-box}body{margin:0;padding:0}a[x-apple-data-detectors]{color:inherit!important;text-decoration:none}";

    expect(formatMailPreview(preview)).toBe("");
  });

  it("falls back to full HTML body when cached preview is CSS only", () => {
    const preview = formatMessageListPreview({
      body_preview: "*{box-sizing:border-box}body{margin:0;padding:0}a[x-apple-data-detectors]{color:inherit!important;text-decoration:none}",
      body: "<html><head><style>.hidden{display:none}</style></head><body><p>Hello <strong>Gmail</strong> &amp; code</p></body></html>"
    });

    expect(preview).toBe("Hello Gmail & code");
  });
});
