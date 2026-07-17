import { describe, expect, it } from "vitest";
import { buildTempEmailImportChunks } from "./tempEmailImport";

describe("buildTempEmailImportChunks", () => {
  it("splits ordinary provider imports into fixed-size chunks", () => {
    expect(
      buildTempEmailImportChunks("one@example.com\ntwo@example.com\nthree@example.com", "gptmail", 2)
    ).toEqual(["one@example.com\ntwo@example.com", "three@example.com"]);
  });

  it("carries the active Cloudflare channel header into each chunk", () => {
    expect(
      buildTempEmailImportChunks(
        [
          "[cloudflare:Primary]",
          "one@example.com",
          "two@example.com",
          "three@example.com",
          "[cloudflare:Backup]",
          "four@example.com"
        ].join("\n"),
        "cloudflare",
        2
      )
    ).toEqual([
      "[cloudflare:Primary]\none@example.com\ntwo@example.com",
      "[cloudflare:Primary]\nthree@example.com\n[cloudflare:Backup]\nfour@example.com"
    ]);
  });
});
