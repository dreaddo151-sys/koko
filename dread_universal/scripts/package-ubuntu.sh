#!/usr/bin/env bash
set -euo pipefail

if [[ ! -r /etc/os-release ]]; then
  printf 'PROJECT DREAD supports Ubuntu 24.04 LTS and later only.\n' >&2
  exit 1
fi
# shellcheck disable=SC1091
. /etc/os-release
if [[ "${ID:-}" != ubuntu ]] || ! dpkg --compare-versions "${VERSION_ID:-0}" ge 24.04; then
  printf 'Unsupported operating system: %s %s (requires Ubuntu 24.04 or later).\n' "${PRETTY_NAME:-unknown}" "${VERSION_ID:-unknown}" >&2
  exit 1
fi

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

for command in dpkg dpkg-deb sha256sum python3 tar install; do
  command -v "$command" >/dev/null 2>&1 || {
    printf 'Missing packaging prerequisite: %s\n' "$command" >&2
    exit 1
  }
done

VERSION="$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml", "rb"))["workspace"]["package"]["version"])')"
ARCH="$(dpkg --print-architecture)"
case "$ARCH" in
  amd64|arm64) ;;
  *) printf 'Unsupported Ubuntu package architecture: %s\n' "$ARCH" >&2; exit 1 ;;
esac

API_BIN="$ROOT/target/release/dread-universal"
VERIFIER_BIN="$ROOT/target/release/dread-result-verifier"
WEB_ROOT="$ROOT/web/dist"
if [[ ! -f "$API_BIN" || ! -f "$VERIFIER_BIN" || ! -s "$WEB_ROOT/index.html" ]]; then
  printf 'Build prerequisites missing. Run cargo build --workspace --release --locked and npm run build first.\n' >&2
  exit 1
fi
# GitHub Actions artifact restore may not preserve executable mode; restore it explicitly.
chmod 0755 "$API_BIN" "$VERIFIER_BIN"

OUT_DIR="${DREAD_RELEASE_DIR:-$ROOT/dist/release}"
mkdir -p "$OUT_DIR"
TMP="$(mktemp -d)"
trap 'rm -rf -- "$TMP"' EXIT

BUNDLE="$TMP/bundle"
mkdir -p "$BUNDLE/bin" "$BUNDLE/web/dist" "$BUNDLE/config" "$BUNDLE/scripts" "$BUNDLE/docs" "$BUNDLE/systemd"
install -m 0755 "$API_BIN" "$BUNDLE/bin/dread-universal"
install -m 0755 "$VERIFIER_BIN" "$BUNDLE/bin/dread-result-verifier"
cp -a "$WEB_ROOT/." "$BUNDLE/web/dist/"
install -m 0644 config/services.toml config/agents.toml config/independent-result-verifier.policy.example.json config/independent-result-verifier.env.example "$BUNDLE/config/"
install -m 0644 .env.example "$BUNDLE/.env.example"
install -m 0755 scripts/install-user.sh "$BUNDLE/install-user.sh"
install -m 0644 scripts/dread_gpu_supervisor_bridge.py "$BUNDLE/scripts/"
install -m 0755 scripts/capture-workstation-baseline.sh "$BUNDLE/scripts/"
install -m 0755 scripts/verify-release.sh "$BUNDLE/scripts/"
install -m 0755 scripts/dread_watchdog.py "$BUNDLE/scripts/"
install -m 0644 systemd/dread-watchdog.service systemd/dread-watchdog.timer systemd/dread-result-verifier.service.example systemd/dread-result-verifier.tmpfiles.example "$BUNDLE/systemd/"
install -m 0644 README.md "$BUNDLE/README.md"
install -m 0644 docs/MASTER-PLAN.md docs/VERIFICATION.md docs/GPU_OWNERSHIP_BOUNDARY.md "$BUNDLE/docs/"

SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(git log -1 --format=%ct 2>/dev/null || date +%s)}"
TAR_PATH="$OUT_DIR/dread-universal-ubuntu-${VERSION}-${ARCH}.tar.gz"
tar --sort=name --mtime="@${SOURCE_DATE_EPOCH}" --owner=0 --group=0 --numeric-owner \
  -C "$TMP" -czf "$TAR_PATH" bundle

DEB="$TMP/deb"
mkdir -p "$DEB/DEBIAN" "$DEB/usr/bin" "$DEB/usr/share/dread-universal/web/dist" \
  "$DEB/usr/share/dread-universal/config" "$DEB/usr/share/dread-universal/scripts" \
  "$DEB/usr/share/doc/dread-universal" "$DEB/usr/lib/systemd/user" "$DEB/usr/share/dread-universal/systemd"
install -m 0755 "$API_BIN" "$DEB/usr/bin/dread-universal"
install -m 0755 "$VERIFIER_BIN" "$DEB/usr/bin/dread-result-verifier"
cp -a "$WEB_ROOT/." "$DEB/usr/share/dread-universal/web/dist/"
install -m 0644 config/services.toml config/agents.toml config/independent-result-verifier.policy.example.json config/independent-result-verifier.env.example "$DEB/usr/share/dread-universal/config/"
install -m 0644 scripts/dread_gpu_supervisor_bridge.py "$DEB/usr/share/dread-universal/scripts/"
install -m 0755 scripts/capture-workstation-baseline.sh "$DEB/usr/share/dread-universal/scripts/"
install -m 0755 scripts/verify-release.sh "$DEB/usr/share/dread-universal/scripts/"
install -m 0755 scripts/dread_watchdog.py "$DEB/usr/share/dread-universal/scripts/"
install -m 0644 systemd/dread-watchdog.service systemd/dread-watchdog.timer systemd/dread-result-verifier.service.example systemd/dread-result-verifier.tmpfiles.example "$DEB/usr/share/dread-universal/systemd/"
install -m 0755 scripts/configure-user.sh "$DEB/usr/share/dread-universal/configure-user.sh"
install -m 0644 systemd/dread-universal.service "$DEB/usr/lib/systemd/user/"
install -m 0644 README.md "$DEB/usr/share/doc/dread-universal/"
install -m 0644 docs/MASTER-PLAN.md docs/VERIFICATION.md docs/GPU_OWNERSHIP_BOUNDARY.md "$DEB/usr/share/doc/dread-universal/"

cat > "$DEB/DEBIAN/control" <<EOF
Package: dread-universal
Version: $VERSION
Section: utils
Priority: optional
Architecture: $ARCH
Depends: libc6, ca-certificates, curl, openssl
Maintainer: PROJECT DREAD contributors
Description: Local-first PROJECT DREAD Universal Master Hub for Ubuntu
 Preserves existing DREAD, ComfyUI and GPU Supervisor services. The user
 service is enabled explicitly with the packaged configure-user.sh helper.
EOF

DEB_PATH="$OUT_DIR/dread-universal_${VERSION}_${ARCH}.deb"
dpkg-deb --build --root-owner-group "$DEB" "$DEB_PATH" >/dev/null
dpkg-deb --contents "$DEB_PATH" >/dev/null
# Ship the verifier beside the manifest so users can verify the release before installing it.
install -m 0755 scripts/verify-release.sh "$OUT_DIR/verify-release.sh"
(
  cd "$OUT_DIR"
  sha256sum "$(basename "$TAR_PATH")" "$(basename "$DEB_PATH")" verify-release.sh > SHA256SUMS
)

printf 'Ubuntu packages created:\n  %s\n  %s\n  %s/SHA256SUMS\n' "$TAR_PATH" "$DEB_PATH" "$OUT_DIR"
