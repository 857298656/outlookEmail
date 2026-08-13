import { describe, expect, it } from "vitest";
import {
  accountProviderDefinition,
  providerCapabilitySummary,
  providerFailureHint,
  providerReadiness,
  providerSupportsCapability
} from "./providerRegistry";

describe("providerRegistry capabilities", () => {
  it("tracks Gmail as an IMAP provider", () => {
    expect(accountProviderDefinition("gmail").accountType).toBe("imap");
    expect(accountProviderDefinition("gmail").defaultImapHost).toBe("imap.gmail.com");
    expect(providerSupportsCapability("gmail", "imap_folders")).toBe(true);
    expect(providerSupportsCapability("gmail", "remote_delete")).toBe(true);
    expect(providerSupportsCapability("gmail", "history_sync")).toBe(false);
    expect(providerCapabilitySummary("gmail")).toContain("IMAP 文件夹");
  });

  it("tracks QQ and 163 as IMAP providers with folder discovery", () => {
    expect(accountProviderDefinition("qq").capabilities).toEqual(
      expect.arrayContaining(["read_mail", "download_attachments", "mark_read", "remote_delete", "imap_folders"])
    );
    expect(accountProviderDefinition("163").capabilities).toEqual(
      expect.arrayContaining(["read_mail", "download_attachments", "mark_read", "remote_delete", "imap_folders"])
    );
    expect(accountProviderDefinition("163").emptyMailboxHint).toContain("收取全部邮件");
    expect(providerSupportsCapability("qq", "history_sync")).toBe(false);
  });

  it("provides provider-specific refresh failure hints", () => {
    expect(providerFailureHint("gmail", "IMAP login failed: authentication failed")).toContain("应用专用密码");
    expect(providerFailureHint("qq", "NO [AUTHENTICATIONFAILED] invalid QQ 授权码")).toContain("QQ 邮箱已开启 IMAP/SMTP");
    expect(providerFailureHint("netease_163", "163 客户端授权密码错误")).toContain("163 已开启客户端授权");
    expect(providerFailureHint("imap_custom", "connection refused by proxy")).toContain("代理链");
  });

  it("checks provider readiness from non-secret account fields", () => {
    expect(providerReadiness({ provider: "gmail", has_refresh_token: true, imap_host: "imap.gmail.com", imap_port: 993 }).status).toBe("ready");
    expect(
      providerReadiness({
        provider: "gmail",
        has_refresh_token: false,
        imap_host: "imap.gmail.com",
        imap_port: 993
      }).missing
    ).toContain("Gmail 应用专用密码");
    expect(
      providerReadiness({
        provider: "qq",
        has_refresh_token: false,
        imap_host: "imap.qq.com",
        imap_port: 993
      }).missing
    ).toContain("QQ IMAP/SMTP 授权码");
    expect(
      providerReadiness({
        provider: "netease_163",
        has_refresh_token: true,
        imap_host: "imap.163.com",
        imap_port: 993
      }).detail
    ).toContain("imap.163.com:993");
    expect(providerReadiness({ provider: "imap_custom", has_refresh_token: true, imap_host: "", imap_port: 993 }).missing).toContain(
      "IMAP host"
    );
  });
});
