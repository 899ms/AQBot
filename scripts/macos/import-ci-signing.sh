#!/usr/bin/env bash

set -euo pipefail
set +x

source "$(dirname "${BASH_SOURCE[0]}")/ci-run-with-timeout.sh"

: "${APPLE_CERTIFICATE:?APPLE_CERTIFICATE is required}"
: "${APPLE_CERTIFICATE_PASSWORD:?APPLE_CERTIFICATE_PASSWORD is required}"
: "${RUNNER_TEMP:?RUNNER_TEMP is required}"
: "${GITHUB_ENV:?GITHUB_ENV is required}"

signing_dir="${RUNNER_TEMP}/aqbot-macos-signing"
keychain_path="${signing_dir}/aqbot-signing.keychain-db"
certificate_p12="${signing_dir}/aqbot-release.p12"
certificate_pem="${signing_dir}/aqbot-release.pem"
keychain_password="$(openssl rand -hex 32)"

{
  echo "AQBOT_SIGNING_DIR=${signing_dir}"
  echo "AQBOT_SIGNING_KEYCHAIN=${keychain_path}"
  echo "AQBOT_SIGNING_CERTIFICATE=${certificate_pem}"
} >> "${GITHUB_ENV}"

mkdir -p "${signing_dir}"
chmod 700 "${signing_dir}"
printf '%s' "${APPLE_CERTIFICATE}" | base64 -D > "${certificate_p12}"
chmod 600 "${certificate_p12}"

security create-keychain -p "${keychain_password}" "${keychain_path}"
security set-keychain-settings -lut 21600 "${keychain_path}"
security unlock-keychain -p "${keychain_password}" "${keychain_path}"
security import "${certificate_p12}" \
  -k "${keychain_path}" \
  -f pkcs12 \
  -P "${APPLE_CERTIFICATE_PASSWORD}" \
  -T /usr/bin/codesign \
  -T /usr/bin/security
security set-key-partition-list \
  -S apple-tool:,apple:,codesign: \
  -s \
  -k "${keychain_password}" \
  "${keychain_path}"

current_keychains=()
while IFS= read -r item; do
  item="${item#"${item%%[![:space:]]*}"}"
  item="${item%"${item##*[![:space:]]}"}"
  item="${item#\"}"
  item="${item%\"}"
  [[ -n "${item}" ]] && current_keychains+=("${item}")
done < <(security list-keychains -d user)
security list-keychains -d user -s "${keychain_path}" "${current_keychains[@]}"

openssl pkcs12 \
  -in "${certificate_p12}" \
  -nokeys \
  -passin env:APPLE_CERTIFICATE_PASSWORD \
  -out "${certificate_pem}"
# Admin-domain trust writes are allowed for root without a prompt (unlike
# remove-trusted-cert), but Apple keeps tightening trust-settings policy per
# OS release — bound it so a future regression fails fast instead of hanging
# the job until the 6h runner limit. Output goes to a file, not the step
# pipes, so a SIGKILL-orphaned root process cannot keep the step open.
add_trusted_cert_log="${signing_dir}/add-trusted-cert.log"
run_with_timeout 60 sudo security add-trusted-cert \
  -d \
  -r trustRoot \
  -p codeSign \
  -k /Library/Keychains/System.keychain \
  "${certificate_pem}" >"${add_trusted_cert_log}" 2>&1 || {
  cat "${add_trusted_cert_log}" >&2 2>/dev/null || true
  echo "security add-trusted-cert failed or timed out; cannot trust AQBot Release certificate." >&2
  exit 1
}

identity_count="$(
  security find-identity -v -p codesigning "${keychain_path}" \
    | grep -c '"AQBot Release"' \
    || true
)"
if [[ "${identity_count}" != "1" ]]; then
  echo "Expected exactly one valid AQBot Release code-signing identity, found ${identity_count}." >&2
  exit 1
fi

{
  echo "APPLE_SIGNING_IDENTITY=AQBot Release"
} >> "${GITHUB_ENV}"

echo "Imported AQBot Release into an ephemeral CI keychain."
