import { existsSync } from "node:fs";
import path from "node:path";
import { spawn } from "node:child_process";
import process from "node:process";
import { applyRustEnv, repoRoot, requireCargo } from "./apply-rust-env.mjs";

const env = applyRustEnv();
requireCargo(env);

const tauriScript = path.join(
  repoRoot,
  "node_modules",
  "@tauri-apps",
  "cli",
  "tauri.js",
);

if (!existsSync(tauriScript)) {
  console.error("Tauri CLI was not found. Run pnpm install first.");
  process.exit(1);
}

const child = spawn(process.execPath, [tauriScript, ...process.argv.slice(2)], {
  cwd: repoRoot,
  env,
  stdio: "inherit",
});

child.on("exit", (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
    return;
  }

  process.exit(code ?? 1);
});
