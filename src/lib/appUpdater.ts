import type { DownloadEvent, Update } from "@tauri-apps/plugin-updater";
import packageJson from "../../package.json";

type TauriRuntimeWindow = Window & { __TAURI_INTERNALS__?: unknown };

export const appVersion = packageJson.version;

export function isTauriRuntime() {
  return typeof window !== "undefined" && Boolean((window as TauriRuntimeWindow).__TAURI_INTERNALS__);
}

export type AppUpdateSummary = {
  version: string;
  date?: string | null;
  body?: string | null;
};

export function summarizeUpdate(update: Update): AppUpdateSummary {
  return {
    version: update.version,
    date: update.date ?? null,
    body: update.body ?? null
  };
}

export async function checkForAppUpdate(): Promise<Update | null> {
  if (!isTauriRuntime()) return null;
  const { check } = await import("@tauri-apps/plugin-updater");
  return check();
}

export async function installAppUpdate(
  update: Update,
  onProgress?: (percent: number) => void
): Promise<void> {
  const { relaunch } = await import("@tauri-apps/plugin-process");
  let downloaded = 0;
  let contentLength = 0;

  await update.downloadAndInstall((event: DownloadEvent) => {
    switch (event.event) {
      case "Started":
        contentLength = event.data.contentLength ?? 0;
        onProgress?.(0);
        break;
      case "Progress":
        downloaded += event.data.chunkLength;
        if (contentLength > 0) {
          onProgress?.(Math.min(100, Math.round((downloaded / contentLength) * 100)));
        }
        break;
      case "Finished":
        onProgress?.(100);
        break;
    }
  });

  await relaunch();
}

export function formatUpdateError(error: unknown) {
  if (error instanceof Error) return error.message;
  return String(error);
}
