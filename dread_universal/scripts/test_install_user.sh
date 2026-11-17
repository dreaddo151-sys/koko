#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf -- "$TMP"' EXIT

HOME="$TMP/home"
PREFIX="$HOME/.local/opt/dread-universal"
MOCK_BIN="$TMP/mock-bin"
STATE="$TMP/systemd-state"
mkdir -p "$HOME" "$MOCK_BIN" "$PREFIX/config" "$PREFIX/data" "$PREFIX/bin" "$PREFIX/web/dist"

cat > "$MOCK_BIN/systemctl" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
[[ "$1" == "--user" ]] || exit 2
shift
case "$1" in
  is-active)
    [[ -f "$MOCK_SYSTEMD_STATE" && "$(cat "$MOCK_SYSTEMD_STATE")" == active ]]
    ;;
  is-enabled)
    [[ -f "$MOCK_SYSTEMD_ENABLED" && "$(cat "$MOCK_SYSTEMD_ENABLED")" == enabled ]]
    ;;
  stop)
    printf 'stopped\n' > "$MOCK_SYSTEMD_STATE"
    ;;
  disable)
    printf 'disabled\n' > "$MOCK_SYSTEMD_ENABLED"
    ;;
  start)
    printf 'active\n' > "$MOCK_SYSTEMD_STATE"
    ;;
  enable)
    printf 'enabled\n' > "$MOCK_SYSTEMD_ENABLED"
    shift
    if [[ "${1:-}" == "--now" ]]; then
      printf 'active\n' > "$MOCK_SYSTEMD_STATE"
    fi
    ;;
  daemon-reload)
    ;;
  *)
    exit 2
    ;;
esac
MOCK
cat > "$MOCK_BIN/curl" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${MOCK_FAIL_HEALTH:-0}" == 1 ]]; then exit 7; fi
if [[ -f "$MOCK_SYSTEMD_STATE" && "$(cat "$MOCK_SYSTEMD_STATE")" == active ]]; then
  printf '{"status":"ok","service":"dread-universal"}\n'
else
  exit 7
fi
MOCK
cat > "$MOCK_BIN/openssl" <<'MOCK'
#!/usr/bin/env bash
printf '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\n'
MOCK
cat > "$MOCK_BIN/sleep" <<'MOCK'
#!/usr/bin/env bash
exit 0
MOCK
chmod +x "$MOCK_BIN/"*

# Simulate a legacy local installation with user config, a database and Supervisor credentials.
printf 'custom-services-config\n' > "$PREFIX/config/services.toml"
printf 'custom-agents-config\n' > "$PREFIX/config/agents.toml"
printf 'legacy-sqlite-bytes\n' > "$PREFIX/data/dread_registry.sqlite3"
printf 'DREAD_API_TOKEN=legacy-api-token-012345678901234567890123456789\nDREAD_GPU_SUPERVISOR_URL=http://127.0.0.1:11502\nDREAD_GPU_SUPERVISOR_TOKEN=legacy-supervisor-token-012345678901234567890123456789\n' > "$PREFIX/.env"

make_bundle() {
  local version="$1"
  local bundle="$TMP/bundle-$version"
  mkdir -p "$bundle/bin" "$bundle/web/dist/assets" "$bundle/config" "$bundle/scripts" "$bundle/docs"
  printf '#!/usr/bin/env bash\necho %s\n' "$version" > "$bundle/bin/dread-universal"
  chmod +x "$bundle/bin/dread-universal"
  printf '<div id="root"></div>\n' > "$bundle/web/dist/index.html"
  printf 'body{}\n' > "$bundle/web/dist/assets/app.css"
  printf 'default-services\n' > "$bundle/config/services.toml"
  printf 'default-agents\n' > "$bundle/config/agents.toml"
  printf '# docs\n' > "$bundle/README.md"
  printf '# docs\n' > "$bundle/docs/README.md"
  printf '#!/usr/bin/env bash\n' > "$bundle/scripts/helper.sh"
  install -m 0755 "$ROOT/scripts/install-user.sh" "$bundle/install-user.sh"
  printf 'test\n' > "$bundle/.env.example"
  printf '%s\n' "$bundle"
}

run_installer() {
  local bundle="$1"
  HOME="$HOME" \
  DREAD_INSTALL_DIR="$PREFIX" \
  MOCK_SYSTEMD_STATE="$STATE" \
  MOCK_SYSTEMD_ENABLED="$TMP/systemd-enabled" \
  PATH="$MOCK_BIN:$PATH" \
  bash "$bundle/install-user.sh" 2>/dev/null
}

BUNDLE1="$(make_bundle v1)"
run_installer "$BUNDLE1"
[[ "$("$PREFIX/bin/dread-universal")" == v1 ]]
grep -q 'custom-services-config' "$HOME/.config/dread-universal/services.toml"
grep -q 'legacy-sqlite-bytes' "$HOME/.local/share/dread-universal/data/dread_registry.sqlite3"
grep -q 'legacy-api-token' "$HOME/.config/dread-universal/dread.env"
grep -q 'legacy-supervisor-token' "$HOME/.config/dread-universal/dread.env"
[[ "$(stat -c '%a' "$HOME/.config/dread-universal/dread.env")" == 600 ]]
[[ "$(cat "$STATE")" == active ]]

printf 'keep-backup-marker\n' > "$PREFIX/previous-release-marker"
BUNDLE2="$(make_bundle v2)"
run_installer "$BUNDLE2"
[[ "$("$PREFIX/bin/dread-universal")" == v2 ]]
[[ "$(cat "$STATE")" == active ]]
backup_marker="$(find "$HOME/.local/opt" -maxdepth 2 -type f -name previous-release-marker -print -quit)"
if [[ -z "$backup_marker" ]]; then
  printf 'Prior application backup did not retain the marker file. Backup directories:\n' >&2
  find "$HOME/.local/opt" -maxdepth 2 -type d -name 'dread-universal.backup.*' -print >&2
  exit 1
fi
grep -q 'keep-backup-marker' "$backup_marker"

printf 'Portable update backup preserved the prior application tree.\n'
# Force a failed update after the installer has rewritten its unit and augmented
# an existing credential file. Rollback must restore both files byte-for-byte.
UNIT_FILE="$HOME/.config/systemd/user/dread-universal.service"
ENV_FILE="$HOME/.config/dread-universal/dread.env"
printf '[Unit]\nDescription=pre-update-unit-sentinel\n' > "$UNIT_FILE"
chmod 0600 "$UNIT_FILE"
sed -i '/^DREAD_OPERATOR_ID=/d' "$ENV_FILE"
cp "$UNIT_FILE" "$TMP/unit-before-failure"
cp "$ENV_FILE" "$TMP/env-before-failure"

BUNDLE3="$(make_bundle v3)"
if HOME="$HOME" DREAD_INSTALL_DIR="$PREFIX" MOCK_SYSTEMD_STATE="$STATE" MOCK_SYSTEMD_ENABLED="$TMP/systemd-enabled" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE3/install-user.sh" >/dev/null; then
  printf 'Expected the forced health-check failure to trigger rollback.\n' >&2
  exit 1
fi
[[ "$("$PREFIX/bin/dread-universal")" == v2 ]]
[[ "$(cat "$STATE")" == active ]]
cmp -s "$TMP/unit-before-failure" "$UNIT_FILE"
cmp -s "$TMP/env-before-failure" "$ENV_FILE"
[[ "$(cat "$TMP/systemd-enabled")" == enabled ]]
failed_install="$(find "$HOME/.local/opt" -maxdepth 1 -type d -name 'dread-universal.failed.*' -print -quit)"
if [[ -z "$failed_install" ]]; then
  printf 'Failed release tree was not retained for diagnosis.\n' >&2
  find "$HOME/.local/opt" -maxdepth 1 -mindepth 1 -print >&2
  exit 1
fi

printf 'Portable installer migration, permissions, backup, unit/credential restoration and rollback: PASS\n'
