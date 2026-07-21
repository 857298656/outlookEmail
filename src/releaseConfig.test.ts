import { describe, expect, it } from "vitest";
import packageJson from "../package.json";
import macConfig from "../src-tauri/tauri.macos.conf.json";
import tauriConfig from "../src-tauri/tauri.conf.json";
import windowsConfig from "../src-tauri/tauri.windows.conf.json";

describe("desktop release configuration", () => {
  it("keeps Windows on NSIS and adds a universal macOS app plus DMG", () => {
    expect(windowsConfig.bundle.targets).toBe("nsis");
    expect(macConfig.bundle.targets).toEqual(["app", "dmg"]);
    expect(macConfig.bundle.macOS.signingIdentity).toBe("-");
    expect(tauriConfig.bundle.icon).toEqual([
      "icons/icon.ico",
      "icons/icon.icns"
    ]);
    expect(packageJson.scripts["tauri:build:mac"]).toContain(
      "--target universal-apple-darwin"
    );
    expect(packageJson.scripts["tauri:build:mac"]).toContain(
      "--bundles app,dmg"
    );
  });
});
