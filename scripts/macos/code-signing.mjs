import { randomBytes } from "node:crypto";
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  statSync,
} from "node:fs";
import os from "node:os";
import path from "node:path";

import { spawnSync } from "node:child_process";

const identities = ["AQBot Dev", "AQBot Release"];
const repository = "AQBot-Desktop/AQBot";
const passwordService = "top.aqbot.desktop.codesigning";
const passwordAccount = "AQBot Release P12";
const signingDir = path.join(os.homedir(), ".aqbot", "signing");
const releaseP12 = path.join(signingDir, "aqbot-release.p12");

function fail(message) {
  throw new Error(message);
}

function execute(command, args, options = {}) {
  const result = spawnSync(command, args, {
    encoding: options.encoding ?? "utf8",
    env: { ...process.env, ...options.env },
    input: options.input,
    stdio: options.inherit
      ? "inherit"
      : [options.input === undefined ? "ignore" : "pipe", "pipe", "pipe"],
  });
  if (result.error) fail(`${command} failed: ${result.error.message}`);
  if (result.status !== 0 && !options.allowFailure) {
    const detail = `${result.stderr ?? ""}`.trim();
    fail(`${command} ${args[0] ?? ""} failed${detail ? `: ${detail}` : ""}`);
  }
  return result;
}

function loginKeychain() {
  const output = execute("security", ["default-keychain", "-d", "user"]).stdout.trim();
  return output.replace(/^"(.*)"$/, "$1");
}

function validIdentity(name, keychain = loginKeychain()) {
  const result = execute(
    "security",
    ["find-identity", "-v", "-p", "codesigning", keychain],
    { allowFailure: true },
  );
  return result.status === 0
    && result.stdout.split("\n").some((line) => line.includes(`"${name}"`));
}

function certificateExists(name, keychain = loginKeychain()) {
  return execute(
    "security",
    ["find-certificate", "-c", name, keychain],
    { allowFailure: true },
  ).status === 0;
}

function opensslCertificate(name, directory, password) {
  const key = path.join(directory, "identity.key");
  const certificate = path.join(directory, "identity.pem");
  const p12 = path.join(directory, "identity.p12");
  execute("openssl", [
    "req",
    "-new",
    "-newkey",
    "rsa:3072",
    "-x509",
    "-sha256",
    "-days",
    "3650",
    "-nodes",
    "-subj",
    `/CN=${name}/O=AQBot`,
    "-addext",
    "basicConstraints=critical,CA:TRUE",
    "-addext",
    "keyUsage=critical,digitalSignature,keyCertSign,cRLSign",
    "-addext",
    "extendedKeyUsage=codeSigning",
    "-keyout",
    key,
    "-out",
    certificate,
  ]);
  execute(
    "openssl",
    [
      "pkcs12",
      "-export",
      "-inkey",
      key,
      "-in",
      certificate,
      "-name",
      name,
      "-out",
      p12,
      "-passout",
      "env:AQBOT_P12_PASSWORD",
    ],
    { env: { AQBOT_P12_PASSWORD: password } },
  );
  return { certificate, p12 };
}

function importIdentity(name, material, password, keychain) {
  execute(
    "security",
    [
      "import",
      material.p12,
      "-k",
      keychain,
      "-f",
      "pkcs12",
      "-P",
      password,
      "-T",
      "/usr/bin/codesign",
      "-T",
      "/usr/bin/security",
    ],
    { inherit: true },
  );
  console.log(`Trusting ${name} for code signing; macOS may request authentication.`);
  execute(
    "security",
    [
      "add-trusted-cert",
      "-r",
      "trustRoot",
      "-p",
      "codeSign",
      "-k",
      keychain,
      material.certificate,
    ],
    { inherit: true },
  );
  if (!validIdentity(name, keychain)) {
    fail(`${name} was imported but is not a valid code-signing identity.`);
  }
}

function saveReleaseBackup(p12, password, keychain) {
  mkdirSync(signingDir, { mode: 0o700, recursive: true });
  chmodSync(signingDir, 0o700);
  copyFileSync(p12, releaseP12);
  chmodSync(releaseP12, 0o600);
  execute(
    "security",
    [
      "add-generic-password",
      "-U",
      "-a",
      passwordAccount,
      "-s",
      passwordService,
      "-w",
      password,
      keychain,
    ],
    { inherit: true },
  );
}

function createIdentity(name, keychain) {
  if (validIdentity(name, keychain)) {
    console.log(`${name}: valid identity already exists.`);
    return;
  }
  if (certificateExists(name, keychain)) {
    fail(`${name} exists in the login keychain but is not a valid identity; refusing to overwrite it.`);
  }

  const temporary = mkdtempSync(path.join(os.tmpdir(), "aqbot-signing-"));
  chmodSync(temporary, 0o700);
  const password = randomBytes(48).toString("base64url");
  try {
    const material = opensslCertificate(name, temporary, password);
    importIdentity(name, material, password, keychain);
    if (name === "AQBot Release") {
      saveReleaseBackup(material.p12, password, keychain);
    }
    console.log(`${name}: created and trusted for code signing.`);
  } finally {
    rmSync(temporary, { force: true, recursive: true });
  }
}

function releasePassword(keychain) {
  return execute(
    "security",
    [
      "find-generic-password",
      "-w",
      "-a",
      passwordAccount,
      "-s",
      passwordService,
      keychain,
    ],
  ).stdout.trim();
}

function uploadSecret(name, value) {
  execute(
    "gh",
    ["secret", "set", name, "--repo", repository],
    { input: value, inherit: false },
  );
}

function secretNames() {
  const output = execute("gh", ["secret", "list", "--repo", repository]).stdout;
  return new Set(
    output
      .trim()
      .split("\n")
      .filter(Boolean)
      .map((line) => line.split(/\s+/)[0]),
  );
}

function uploadReleaseSecrets(keychain) {
  if (!existsSync(releaseP12)) {
    fail(`Release P12 backup is missing: ${releaseP12}`);
  }
  execute("gh", ["auth", "status"]);
  const password = releasePassword(keychain);
  const certificate = readFileSync(releaseP12).toString("base64");
  uploadSecret("APPLE_CERTIFICATE", certificate);
  uploadSecret("APPLE_CERTIFICATE_PASSWORD", password);
  const names = secretNames();
  for (const name of ["APPLE_CERTIFICATE", "APPLE_CERTIFICATE_PASSWORD"]) {
    if (!names.has(name)) fail(`GitHub Actions secret ${name} was not created.`);
  }
  console.log(`GitHub Actions secrets uploaded to ${repository}.`);
}

function verifyIdentity(name) {
  const temporary = mkdtempSync(path.join(os.tmpdir(), "aqbot-codesign-check-"));
  const executable = path.join(temporary, "codesign-check");
  try {
    copyFileSync("/usr/bin/true", executable);
    chmodSync(executable, 0o755);
    execute("codesign", [
      "--force",
      "--sign",
      name,
      "--identifier",
      `top.aqbot.desktop.${name === "AQBot Dev" ? "dev-check" : "release-check"}`,
      "--timestamp=none",
      executable,
    ]);
    execute("codesign", ["--verify", "--strict", "--verbose=2", executable]);
  } finally {
    rmSync(temporary, { force: true, recursive: true });
  }
}

function verify() {
  const keychain = loginKeychain();
  for (const identity of identities) {
    if (!validIdentity(identity, keychain)) fail(`${identity} is not a valid code-signing identity.`);
    verifyIdentity(identity);
  }
  if (!existsSync(releaseP12)) fail(`Release P12 backup is missing: ${releaseP12}`);
  const mode = statSync(releaseP12).mode & 0o777;
  if (mode !== 0o600) fail(`${releaseP12} must have mode 0600, found ${mode.toString(8)}.`);
  const names = secretNames();
  for (const name of ["APPLE_CERTIFICATE", "APPLE_CERTIFICATE_PASSWORD"]) {
    if (!names.has(name)) fail(`GitHub Actions secret ${name} is missing.`);
  }
  console.log("AQBot Dev and AQBot Release signing setup is valid.");
}

function bootstrap() {
  if (process.platform !== "darwin") fail("Code-signing bootstrap must run on macOS.");
  const keychain = loginKeychain();
  for (const identity of identities) createIdentity(identity, keychain);
  uploadReleaseSecrets(keychain);
  verify();
}

try {
  const command = process.argv[2];
  if (command === "bootstrap") bootstrap();
  else if (command === "verify") verify();
  else fail("Usage: code-signing.mjs <bootstrap|verify>");
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}
