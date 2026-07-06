import { describe, expect, it } from "vitest";
import {
  accountProviderDefinition,
  providerCapabilitySummary,
  providerFailureHint,
  providerSupportsCapability
} from "./providerRegistry";

describe("providerRegistry capabilities", () => {
  it("tracks Gmail API capabilities separately from generic IMAP providers", () => {
    expect(providerSupportsCapability("gmail", "history_sync")).toBe(true);
    expect(providerSupportsCapability("gmail", "trash")).toBe(true);
    expect(providerSupportsCapability("gmail", "imap_folders")).toBe(false);
    expect(providerCapabilitySummary("gmail")).toContain("增量同步");
  });

  it("tracks QQ and 163 as IMAP providers with folder discovery", () => {
    expect(accountProviderDefinition("qq").capabilities).toEqual(
      expect.arrayContaining(["read_mail", "download_attachments", "mark_read", "remote_delete", "imap_folders"])
    );
    expect(accountProviderDefinition("163").capabilities).toEqual(
      expect.arrayContaining(["read_mail", "download_attachments", "mark_read", "remote_delete", "imap_folders"])
    );
    expect(providerSupportsCapability("qq", "history_sync")).toBe(false);
  });

  it("provides provider-specific refresh failure hints", () => {
    expect(providerFailureHint("gmail", "HTTP 403 insufficientPermissions missing gmail.modify scope")).toContain("gmail.modify");
    expect(providerFailureHint("qq", "NO [AUTHENTICATIONFAILED] invalid QQ 授权码")).toContain("QQ 邮箱已开启 IMAP/SMTP");
    expect(providerFailureHint("netease_163", "163 客户端授权密码错误")).toContain("163 已开启客户端授权");
    expect(providerFailureHint("imap_custom", "connection refused by proxy")).toContain("代理链");
  });
});
