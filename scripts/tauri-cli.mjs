import { spawn, spawnSync } from "node:child_process";
import { existsSync, readdirSync, rmSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, "..");
const args = process.argv.slice(2);

function isBundleCommand() {
  return args[0] === "build" || args[0] === "bundle";
}

function cleanStaleDmgTempFiles() {
  const macosBundleDir = path.join(repoRoot, "src-tauri", "target", "release", "bundle", "macos");
  if (!existsSync(macosBundleDir)) return;

  for (const entry of readdirSync(macosBundleDir)) {
    if (/^rw\..+\.dmg$/.test(entry)) {
      rmSync(path.join(macosBundleDir, entry), { force: true });
    }
  }
}

function hasConfigOverride() {
  return args.some((arg) => arg === "--config" || arg === "-c" || arg.startsWith("--config="));
}

function defaultCargoTargetDir(environment) {
  if (process.platform === "darwin") {
    return path.join(os.homedir(), "Library", "Caches", "aqbot", "cargo-target");
  }
  if (process.platform === "win32") {
    const cacheRoot = environment.LOCALAPPDATA || path.join(os.homedir(), "AppData", "Local");
    return path.join(cacheRoot, "aqbot", "cargo-target");
  }
  const cacheRoot = environment.XDG_CACHE_HOME || path.join(os.homedir(), ".cache");
  return path.join(cacheRoot, "aqbot", "cargo-target");
}

const env = { ...process.env };

if (args[0] === "dev" && !env.CARGO_TARGET_DIR) {
  env.CARGO_TARGET_DIR = defaultCargoTargetDir(env);
}

if (process.platform === "darwin") {
  env.MACOSX_DEPLOYMENT_TARGET ??= "11.0";
}

if (process.platform === "darwin" && args[0] === "dev") {
  const identityCheck = spawnSync(
    "security",
    ["find-identity", "-v", "-p", "codesigning"],
    { encoding: "utf8" },
  );
  if (
    identityCheck.status !== 0
    || !identityCheck.stdout.split("\n").some((line) => line.includes('"AQBot Dev"'))
  ) {
    console.error(
      "AQBot Dev code-signing identity is missing. Run `pnpm macos:signing:bootstrap` first.",
    );
    process.exit(1);
  }

  const hasRunner = args.some(
    (arg) => arg === "--runner" || arg === "-r" || arg.startsWith("--runner="),
  );
  if (!hasRunner) {
    args.push(
      "--runner",
      path.join(repoRoot, "scripts", "macos", "tauri-dev-runner.mjs"),
    );
  }
  args.push(
    "--config",
    JSON.stringify({
      productName: "AQBot Dev",
      identifier: "top.aqbot.desktop.dev",
    }),
  );
}

if (isBundleCommand()) {
  cleanStaleDmgTempFiles();
  if (!env.TAURI_BUNDLER_DMG_IGNORE_CI) {
    env.CI = "true";
  }
  if (!env.TAURI_SIGNING_PRIVATE_KEY && !hasConfigOverride()) {
    args.push("--config", JSON.stringify({ bundle: { createUpdaterArtifacts: false } }));
  }
}

const tauriCliScript = path.join(
  repoRoot,
  "node_modules",
  "@tauri-apps",
  "cli",
  "tauri.js",
);

const child = spawn(process.execPath, [tauriCliScript, ...args], {
  cwd: repoRoot,
  env,
  stdio: "inherit",
});

child.on("error", (error) => {
  console.error(error);
  process.exit(1);
});

child.on("exit", (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
    return;
  }
  process.exit(code ?? 1);
});
