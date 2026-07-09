import { existsSync, mkdirSync } from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import process from "node:process";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
export const repoRoot = path.resolve(scriptDir, "..");
export const cargoExe = process.platform === "win32" ? "cargo.exe" : "cargo";

function pathKeyFor(env) {
  return Object.keys(env).find((key) => key.toLowerCase() === "path") || "PATH";
}

function defaultRustCacheRoot() {
  if (process.platform === "win32") {
    const driveRoot = path.parse(repoRoot).root;
    const driveCache = path.join(driveRoot, "RustCache");
    if (driveRoot.toUpperCase().startsWith("E:") || existsSync(driveRoot)) {
      return driveCache;
    }
  }

  const home = process.env.USERPROFILE || process.env.HOME;
  return home ? path.join(home, "RustCache") : path.join(repoRoot, ".rust-cache");
}

function ensureDir(dirPath) {
  if (!existsSync(dirPath)) {
    mkdirSync(dirPath, { recursive: true });
  }
}

function hasCargo(env) {
  return (
    spawnSync(cargoExe, ["--version"], {
      env,
      stdio: "ignore",
    }).status === 0
  );
}

function candidateCargoBins(env) {
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
    candidates.push("E:\\RustCache\\.cargo\\bin");
  }

  return candidates;
}

/**
 * Apply local Rust cache paths so dependency downloads, toolchains, build output,
 * and compiler temp files stay off the system drive when possible.
 */
export function applyRustEnv(baseEnv = process.env) {
  const env = { ...baseEnv };
  const pathKey = pathKeyFor(env);
  const cacheRoot = env.RUST_CACHE_ROOT || defaultRustCacheRoot();

  env.RUST_CACHE_ROOT = cacheRoot;
  env.RUSTUP_HOME ||= path.join(cacheRoot, ".rustup");
  env.CARGO_HOME ||= path.join(cacheRoot, ".cargo");
  // Always override Cursor sandbox target/temp redirection.
  env.CARGO_TARGET_DIR = path.join(cacheRoot, "cargo-target", "outlookEmail");

  const tempDir = env.RUST_TEMP_DIR || path.join(cacheRoot, "temp");
  ensureDir(tempDir);
  ensureDir(env.CARGO_TARGET_DIR);
  env.TEMP = tempDir;
  env.TMP = tempDir;

  if (!hasCargo(env)) {
    const cargoBin = candidateCargoBins(env).find((candidate) =>
      existsSync(path.join(candidate, cargoExe)),
    );

    if (cargoBin) {
      env[pathKey] = [cargoBin, env[pathKey] || ""]
        .filter(Boolean)
        .join(path.delimiter);
      env.CARGO_HOME = path.dirname(cargoBin);

      const rustupHome = path.resolve(env.CARGO_HOME, "..", ".rustup");
      if (existsSync(rustupHome)) {
        env.RUSTUP_HOME = rustupHome;
      }
    }
  } else {
    const cargoBin = path.join(env.CARGO_HOME, "bin");
    if (existsSync(path.join(cargoBin, cargoExe))) {
      env[pathKey] = [cargoBin, env[pathKey] || ""]
        .filter(Boolean)
        .join(path.delimiter);
    }
  }

  return env;
}

export function requireCargo(env) {
  if (!hasCargo(env)) {
    console.error(
      "Cargo was not found. Install Rust or add Cargo's bin directory to PATH before running this command.",
    );
    process.exit(1);
  }
}
