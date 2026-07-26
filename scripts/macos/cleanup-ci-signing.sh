#!/usr/bin/env bash

set -euo pipefail
set +x

if [[ -n "${AQBOT_SIGNING_CERTIFICATE:-}" && -f "${AQBOT_SIGNING_CERTIFICATE}" ]]; then
  sudo security remove-trusted-cert -d "${AQBOT_SIGNING_CERTIFICATE}" >/dev/null 2>&1 || true
fi

if [[ -n "${AQBOT_SIGNING_KEYCHAIN:-}" && -f "${AQBOT_SIGNING_KEYCHAIN}" ]]; then
  security delete-keychain "${AQBOT_SIGNING_KEYCHAIN}" >/dev/null 2>&1 || true
fi

if [[ -n "${AQBOT_SIGNING_DIR:-}" && "${AQBOT_SIGNING_DIR}" == "${RUNNER_TEMP:-}/aqbot-macos-signing" ]]; then
  rm -rf "${AQBOT_SIGNING_DIR}"
fi
