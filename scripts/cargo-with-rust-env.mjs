import { spawn } from "node:child_process";
import process from "node:process";
import path from "node:path";
import { applyRustEnv, cargoExe, repoRoot, requireCargo } from "./apply-rust-env.mjs";

const env = applyRustEnv();
requireCargo(env);

const tauriDir = path.join(repoRoot, "src-tauri");
const child = spawn(cargoExe, process.argv.slice(2), {
  cwd: tauriDir,
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
