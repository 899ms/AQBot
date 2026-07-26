import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import {
  clearOwnedSessionMarker,
  stopExistingApp,
} from "./tauri-dev-session.mjs";

function processIsAlive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return error?.code === "EPERM";
  }
}

async function waitForExit(pid) {
  const deadline = Date.now() + 2_000;
  while (processIsAlive(pid) && Date.now() < deadline) {
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
}

test("intentional Dev App replacement clears the exact process session marker", async () => {
  const tempRoot = mkdtempSync(path.join(os.tmpdir(), "aqbot-dev-session-"));
  const executable = path.join(
    tempRoot,
    "AQBot Dev.app",
    "Contents",
    "MacOS",
    "AQBot",
  );
  const markerPath = path.join(tempRoot, "current-session.json");
  mkdirSync(path.dirname(executable), { recursive: true });
  symlinkSync(process.execPath, executable);
  const child = spawn(executable, ["-e", "setInterval(() => {}, 1000)"], {
    stdio: "ignore",
  });

  try {
    writeFileSync(markerPath, JSON.stringify({ pid: child.pid }));

    await stopExistingApp({ executable, markerPath });
    await waitForExit(child.pid);

    assert.equal(processIsAlive(child.pid), false, "old Dev App should stop");
    assert.equal(
      existsSync(markerPath),
      false,
      "expected replacement must not be reported as a crash",
    );
  } finally {
    if (processIsAlive(child.pid)) child.kill("SIGKILL");
    rmSync(tempRoot, { force: true, recursive: true });
  }
});

test("session cleanup never removes a marker owned by another PID", () => {
  const tempRoot = mkdtempSync(path.join(os.tmpdir(), "aqbot-dev-session-"));
  const markerPath = path.join(tempRoot, "current-session.json");

  try {
    writeFileSync(markerPath, JSON.stringify({ pid: 42 }));

    assert.equal(clearOwnedSessionMarker(markerPath, 84), false);
    assert.equal(existsSync(markerPath), true);
  } finally {
    rmSync(tempRoot, { force: true, recursive: true });
  }
});
