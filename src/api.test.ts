import { describe, expect, it } from "vitest";
import { api } from "./api";

describe("account refresh API", () => {
  it("refreshes every account when using the settings-based all-account action", async () => {
    const imported = await api.importAccounts({
      raw: [
        "provider=gmail----first-refresh-test@gmail.com----app-password",
        "provider=qq----second-refresh-test@qq.com----authorization-code"
      ].join("\n")
    });

    const result = await api.refreshAllAccountsWithDefaultLimit();
    const accounts = await api.listAccounts();
    const importedAccounts = imported.accounts ?? [];
    const importedIds = new Set(importedAccounts.map((account) => account.id));
    const refreshedImports = accounts.filter((account) => importedIds.has(account.id));

    expect(importedAccounts).toHaveLength(2);
    expect(result.success).toBe(true);
    expect(result.refreshed).toBeGreaterThanOrEqual(importedAccounts.length);
    expect(refreshedImports).toHaveLength(importedAccounts.length);
    expect(refreshedImports.every((account) => account.last_refresh_status === "success")).toBe(true);
  });
});
