#!/usr/bin/env bash

set -euo pipefail
set +x

source "$(dirname "${BASH_SOURCE[0]}")/ci-run-with-timeout.sh"

# Do NOT use `security remove-trusted-cert -d` here: since macOS 14.7.5/15.4
# it pops an interactive authorization dialog even as root and blocks forever
# on headless runners (actions/runner-images#12116). Deleting the certificate
# from the System keychain by SHA-1 hash needs no authorization; the stale
# trust-settings entry it leaves behind is harmless on an ephemeral VM.
if [[ -n "${AQBOT_SIGNING_CERTIFICATE:-}" && -f "${AQBOT_SIGNING_CERTIFICATE}" ]]; then
  cert_hash="$(
    openssl x509 -in "${AQBOT_SIGNING_CERTIFICATE}" -noout -fingerprint -sha1 2>/dev/null \
      | cut -d= -f2 | tr -d ':'
  )" || cert_hash=""
  if [[ -n "${cert_hash}" ]]; then
    run_with_timeout 30 sudo security delete-certificate -Z "${cert_hash}" /Library/Keychains/System.keychain >/dev/null 2>&1 \
      || echo "Warning: failed to delete signing certificate from System keychain (non-fatal)." >&2
  fi
fi

if [[ -n "${AQBOT_SIGNING_KEYCHAIN:-}" && -f "${AQBOT_SIGNING_KEYCHAIN}" ]]; then
  run_with_timeout 30 security delete-keychain "${AQBOT_SIGNING_KEYCHAIN}" >/dev/null 2>&1 \
    || echo "Warning: failed to delete signing keychain (non-fatal)." >&2
fi

if [[ -n "${AQBOT_SIGNING_DIR:-}" && "${AQBOT_SIGNING_DIR}" == "${RUNNER_TEMP:-}/aqbot-macos-signing" ]]; then
  rm -rf "${AQBOT_SIGNING_DIR}"
fi
