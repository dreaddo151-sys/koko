#!/usr/bin/env bash
set -euo pipefail

usage() {
  printf 'Usage: %s [path/to/SHA256SUMS]\n' "${0##*/}" >&2
}

MANIFEST="${1:-SHA256SUMS}"
if [[ $# -gt 1 || ! -f "$MANIFEST" || -L "$MANIFEST" ]]; then
  usage
  printf 'Release checksum manifest must be one regular, non-symlink file.\n' >&2
  exit 2
fi

if ! command -v cosign >/dev/null 2>&1; then
  printf 'Cosign is required to verify this signed release. Install Sigstore Cosign first.\n' >&2
  exit 2
fi

MANIFEST_DIR="$(cd -- "$(dirname -- "$MANIFEST")" && pwd -P)"
MANIFEST_NAME="$(basename -- "$MANIFEST")"
MANIFEST_PATH="$MANIFEST_DIR/$MANIFEST_NAME"
BUNDLE_PATH="$MANIFEST_PATH.sigstore.json"

if [[ ! -s "$BUNDLE_PATH" || -L "$BUNDLE_PATH" ]]; then
  printf 'Missing regular Sigstore bundle: %s\n' "$BUNDLE_PATH" >&2
  exit 2
fi

EXPECTED_IDENTITY_REGEX='^https://github\.com/dreaddo151-sys/koko/\.github/workflows/dread-universal-release\.yml@refs/(tags/v[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z.-]+)?|heads/main)$'
EXPECTED_OIDC_ISSUER='https://token.actions.githubusercontent.com'

printf 'Verifying release signature and GitHub workflow identity...\n'
cosign verify-blob \
  --bundle "$BUNDLE_PATH" \
  --certificate-identity-regexp "$EXPECTED_IDENTITY_REGEX" \
  --certificate-oidc-issuer "$EXPECTED_OIDC_ISSUER" \
  "$MANIFEST_PATH"

printf 'Verifying release artifact checksums...\n'
(
  cd -- "$MANIFEST_DIR"
  sha256sum --check --strict "$MANIFEST_NAME"
)

printf 'PROJECT DREAD release signature and artifact checksums verified.\n'
