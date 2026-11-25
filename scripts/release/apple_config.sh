#!/usr/bin/env bash
# Complete Apple signing builds macOS; none at all ships Windows alone. A
# partial configuration is a mistake and stops the release.
set -euo pipefail
keys=(APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY
  APPLE_TEAM_ID APPLE_API_ISSUER APPLE_API_KEY APPLE_API_KEY_PATH)
missing=()
for name in "${keys[@]}"; do
  [[ -n "${!name:-}" ]] || missing+=("${name}")
done
if (( ${#missing[@]} == 0 )); then
  echo 'enabled=true' >> "${GITHUB_OUTPUT}"
elif (( ${#missing[@]} == ${#keys[@]} )); then
  echo 'enabled=false' >> "${GITHUB_OUTPUT}"
  echo '::notice::No Apple signing is configured: this release ships Windows alone.'
else
  echo "::error title=Incomplete Apple signing::Add ${missing[*]} as repository Actions secrets, or remove every Apple secret to release Windows alone (see .github/RELEASE.md)."
  exit 1
fi
