import { describe, expect, it } from "vitest";
import {
  accountProviderDefinition,
  providerCapabilitySummary,
  providerFailureHint,
  providerReadiness,
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

  it("checks provider readiness from non-secret account fields", () => {
    expect(providerReadiness({ provider: "gmail", has_client_id: true, has_refresh_token: true }).status).toBe("ready");
    expect(providerReadiness({ provider: "gmail", has_client_id: false, has_refresh_token: true }).missing).toContain("Google Client ID");
    expect(
      providerReadiness({
        provider: "qq",
        has_password: true,
        has_imap_password: false,
        imap_host: "imap.qq.com",
        imap_port: 993
      }).missing
    ).toContain("QQ IMAP/SMTP 授权码");
    expect(
      providerReadiness({
        provider: "netease_163",
        has_imap_password: true,
        imap_host: "imap.163.com",
        imap_port: 993
      }).detail
    ).toContain("imap.163.com:993");
    expect(providerReadiness({ provider: "imap_custom", has_imap_password: true, imap_host: "", imap_port: 993 }).missing).toContain(
      "IMAP host"
    );
  });
});
