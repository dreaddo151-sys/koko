#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
VERIFY="$ROOT/verify-release.sh"
if [[ ! -f "$VERIFY" ]]; then
  printf 'Release verifier is missing or not executable: %s\n' "$VERIFY" >&2
  exit 1
fi

TMP="$(mktemp -d)"
trap 'rm -rf -- "$TMP"' EXIT
mkdir -p "$TMP/bin" "$TMP/release"
FAKE_COSIGN="$TMP/bin/cosign"

cat > "$FAKE_COSIGN" <<'COSIGN'
#!/usr/bin/env bash
set -euo pipefail
[[ "${1:-}" == verify-blob ]] || exit 41
shift

bundle=""
identity_regex=""
issuer=""
manifest=""
while (($#)); do
  case "$1" in
    --bundle)
      bundle="$2"
      shift 2
      ;;
    --certificate-identity-regexp)
      identity_regex="$2"
      shift 2
      ;;
    --certificate-oidc-issuer)
      issuer="$2"
      shift 2
      ;;
    *)
      manifest="$1"
      shift
      ;;
  esac
done

[[ -s "$bundle" ]] || exit 42
[[ -n "$manifest" && -s "$manifest" ]] || exit 43
[[ "$issuer" == "https://token.actions.githubusercontent.com" ]] || exit 44
expected_identity_regex='^https://github\\.com/dreaddo151-sys/koko/\\.github/workflows/dread-universal-release\\.yml@refs/(tags/v[0-9]+\\.[0-9]+\\.[0-9]+([.-][0-9A-Za-z.-]+)?|heads/main)
[[ "${FAKE_COSIGN_FAIL:-0}" != 1 ]] || exit 49
COSIGN
chmod 0755 "$FAKE_COSIGN"

printf 'portable archive fixture\n' > "$TMP/release/dread-universal.tar.gz"
printf 'deb fixture\n' > "$TMP/release/dread-universal.deb"
(
  cd "$TMP/release"
  sha256sum dread-universal.tar.gz dread-universal.deb > SHA256SUMS
)
printf '{"fake":"bundle"}\n' > "$TMP/release/SHA256SUMS.sigstore.json"

PATH="$TMP/bin:$PATH" bash "$VERIFY" "$TMP/release/SHA256SUMS" >/dev/null

if FAKE_COSIGN_FAIL=1 PATH="$TMP/bin:$PATH" bash "$VERIFY" "$TMP/release/SHA256SUMS" >/dev/null 2>&1; then
  printf 'Verifier accepted a rejected signature.\n' >&2
  exit 1
fi

printf 'tampered archive\n' >> "$TMP/release/dread-universal.tar.gz"
if PATH="$TMP/bin:$PATH" bash "$VERIFY" "$TMP/release/SHA256SUMS" >/dev/null 2>&1; then
  printf 'Verifier accepted an artifact with a mismatched checksum.\n' >&2
  exit 1
fi

printf 'Release verifier checks passed.\n'

[[ "$identity_regex" == "$expected_identity_regex" ]] || exit 45
[[ "${FAKE_COSIGN_FAIL:-0}" != 1 ]] || exit 49
COSIGN
chmod 0755 "$FAKE_COSIGN"

printf 'portable archive fixture\n' > "$TMP/release/dread-universal.tar.gz"
printf 'deb fixture\n' > "$TMP/release/dread-universal.deb"
(
  cd "$TMP/release"
  sha256sum dread-universal.tar.gz dread-universal.deb > SHA256SUMS
)
printf '{"fake":"bundle"}\n' > "$TMP/release/SHA256SUMS.sigstore.json"

PATH="$TMP/bin:$PATH" bash "$VERIFY" "$TMP/release/SHA256SUMS" >/dev/null

if FAKE_COSIGN_FAIL=1 PATH="$TMP/bin:$PATH" bash "$VERIFY" "$TMP/release/SHA256SUMS" >/dev/null 2>&1; then
  printf 'Verifier accepted a rejected signature.\n' >&2
  exit 1
fi

printf 'tampered archive\n' >> "$TMP/release/dread-universal.tar.gz"
if PATH="$TMP/bin:$PATH" bash "$VERIFY" "$TMP/release/SHA256SUMS" >/dev/null 2>&1; then
  printf 'Verifier accepted an artifact with a mismatched checksum.\n' >&2
  exit 1
fi

printf 'Release verifier checks passed.\n'
