import { existsSync } from "node:fs";
import path from "node:path";
import { spawn, spawnSync } from "node:child_process";
import process from "node:process";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptDir, "..");
const cargoExe = process.platform === "win32" ? "cargo.exe" : "cargo";
const env = { ...process.env };
const pathKey =
  Object.keys(env).find((key) => key.toLowerCase() === "path") || "PATH";

function hasCargo(currentEnv) {
  const result = spawnSync(cargoExe, ["--version"], {
    env: currentEnv,
    stdio: "ignore",
  });

  return result.status === 0;
}

function candidateCargoBins() {
  const candidates = [];

  if (env.CARGO_HOME) {
    candidates.push(path.join(env.CARGO_HOME, "bin"));
  }

  const home = env.USERPROFILE || env.HOME;
  if (home) {
    candidates.push(path.join(home, ".cargo", "bin"));
  }

  if (process.platform === "win32") {
    const projectDrive = path.parse(repoRoot).root;
    candidates.push(path.join(projectDrive, "RustCache", ".cargo", "bin"));
  }

  return candidates;
}

if (!hasCargo(env)) {
  const cargoBin = candidateCargoBins().find((candidate) =>
    existsSync(path.join(candidate, cargoExe)),
  );

  if (cargoBin) {
    env[pathKey] = [cargoBin, env[pathKey] || ""]
      .filter(Boolean)
      .join(path.delimiter);
    env.CARGO_HOME ||= path.dirname(cargoBin);

    const rustupHome = path.resolve(env.CARGO_HOME, "..", ".rustup");
    if (!env.RUSTUP_HOME && existsSync(rustupHome)) {
      env.RUSTUP_HOME = rustupHome;
    }
  }
}

if (!hasCargo(env)) {
  console.error(
    "Cargo was not found. Install Rust or add Cargo's bin directory to PATH before running Tauri.",
  );
  process.exit(1);
}

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
