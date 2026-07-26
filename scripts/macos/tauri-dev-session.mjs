import { spawnSync } from "node:child_process";
import { existsSync, readFileSync, rmSync } from "node:fs";

const DEFAULT_QUIT_TIMEOUT_MS = 3_000;

function escapeExtendedRegex(value) {
  return value.replace(/[.[\]{}()*+?^$\\|]/g, "\\$&");
}

export function clearOwnedSessionMarker(markerPath, pid, warn = console.warn) {
  if (!existsSync(markerPath)) return false;
  try {
    const session = JSON.parse(readFileSync(markerPath, "utf8"));
    if (session.pid !== pid) return false;
    rmSync(markerPath, { force: true });
    return true;
  } catch (error) {
    warn(`Could not inspect AQBot Dev session marker: ${error.message}`);
    return false;
  }
}

export function matchingExecutablePids(executable) {
  const pattern = `^${escapeExtendedRegex(executable)}([[:space:]]|$)`;
  const result = spawnSync("pgrep", ["-f", pattern], {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  });
  if (result.error) {
    throw new Error(`Could not inspect existing AQBot Dev processes: ${result.error.message}`);
  }
  if (result.status === 1) return [];
  if (result.status !== 0) {
    throw new Error(
      `Could not inspect existing AQBot Dev processes: ${result.stderr.trim()}`,
    );
  }
  return result.stdout
    .split(/\s+/)
    .filter(Boolean)
    .map(Number)
    .filter((pid) => Number.isSafeInteger(pid) && pid > 0);
}

function processIsAlive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return error?.code === "EPERM";
  }
}

function terminateIfAlive(pid) {
  try {
    process.kill(pid, "SIGTERM");
  } catch (error) {
    if (error?.code !== "ESRCH") throw error;
  }
}

async function waitForExit(pids, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  let remaining = pids.filter(processIsAlive);
  while (remaining.length > 0 && Date.now() < deadline) {
    await new Promise((resolve) => setTimeout(resolve, 50));
    remaining = remaining.filter(processIsAlive);
  }
  return remaining;
}

export async function stopExistingApp({
  executable,
  markerPath,
  bundleIdentifier,
  quitTimeoutMs = DEFAULT_QUIT_TIMEOUT_MS,
  warn = console.warn,
}) {
  const pids = matchingExecutablePids(executable);
  if (pids.length === 0) return [];

  for (const pid of pids) clearOwnedSessionMarker(markerPath, pid, warn);

  if (bundleIdentifier) {
    const result = spawnSync(
      "/usr/bin/osascript",
      ["-e", `tell application id "${bundleIdentifier}" to quit`],
      { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] },
    );
    if (result.error) {
      warn(`Could not request AQBot Dev to quit: ${result.error.message}`);
    } else if (result.status !== 0) {
      warn(`Could not request AQBot Dev to quit: ${result.stderr.trim()}`);
    }
  } else {
    for (const pid of pids) terminateIfAlive(pid);
  }

  const remaining = await waitForExit(pids, quitTimeoutMs);
  for (const pid of remaining) {
    warn(`AQBot Dev did not quit within ${quitTimeoutMs}ms; terminating PID ${pid}.`);
    terminateIfAlive(pid);
  }
  return pids;
}
