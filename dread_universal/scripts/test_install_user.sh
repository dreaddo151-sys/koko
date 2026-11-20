#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf -- "$TMP"' EXIT

HOME="$TMP/home"
PREFIX="$HOME/.local/opt/dread-universal"
MOCK_BIN="$TMP/mock-bin"
STATE="$TMP/systemd-state"
SYSTEMCTL_LOG="$TMP/systemctl.log"
mkdir -p "$HOME" "$MOCK_BIN" "$PREFIX/config" "$PREFIX/data" "$PREFIX/bin" "$PREFIX/web/dist"

cat > "$MOCK_BIN/systemctl" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
[[ "$1" == "--user" ]] || exit 2
if [[ -n "${MOCK_SYSTEMCTL_LOG:-}" ]]; then
  printf '%s\n' "$*" >> "$MOCK_SYSTEMCTL_LOG"
fi
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
    [[ -e "$MOCK_UNIT_FILE" ]] || exit 1
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
printf 'DREAD_API_TOKEN=legacy-api-token-012345678901234567890123456789\nDREAD_GPU_SUPERVISOR_URL=http://127.0.0.1:11502\nDREAD_GPU_SUPERVISOR_TOKEN=legacy-supervisor-token-012345678901234567890123456789\nDREAD_GPU_GATEWAY_URL=http://127.0.0.1:11515\nDREAD_COMFYUI_URL=http://127.0.0.1:8189\nDREAD_WORKSPACE=/opt/dread-workspace\nDREAD_MODEL_ROOTS=/models/gguf:/models/comfyui\nDREAD_COMFYUI_WORKFLOW_ROOTS=/workflows\nDREAD_PLUGIN_ROOTS=/plugins\nDREAD_AGENT_MANIFEST_ROOTS=/agents\n' > "$PREFIX/.env"

make_bundle() {
  local version="$1"
  local bundle="$TMP/bundle-$version"
  mkdir -p "$bundle/bin" "$bundle/web/dist/assets" "$bundle/config" "$bundle/scripts" "$bundle/docs"
  mkdir -p "$bundle/systemd"
  printf '#!/usr/bin/env bash\necho %s\n' "$version" > "$bundle/bin/dread-universal"
  chmod +x "$bundle/bin/dread-universal"
  printf '<div id="root"></div>\n' > "$bundle/web/dist/index.html"
  printf 'body{}\n' > "$bundle/web/dist/assets/app.css"
  printf 'default-services\n' > "$bundle/config/services.toml"
  printf 'default-agents\n' > "$bundle/config/agents.toml"
  printf '# docs\n' > "$bundle/README.md"
  printf '# docs\n' > "$bundle/docs/README.md"
  printf '#!/usr/bin/env bash\n' > "$bundle/scripts/helper.sh"
  install -m 0755 "$ROOT/scripts/dread_watchdog.py" "$bundle/scripts/dread_watchdog.py"
  install -m 0644 "$ROOT/systemd/dread-watchdog.service" "$bundle/systemd/dread-watchdog.service"
  install -m 0644 "$ROOT/systemd/dread-watchdog.timer" "$bundle/systemd/dread-watchdog.timer"
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
  MOCK_UNIT_FILE="$HOME/.config/systemd/user/dread-universal.service" \
  MOCK_SYSTEMCTL_LOG="$SYSTEMCTL_LOG" \
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
grep -q '^DREAD_GPU_GATEWAY_URL=http://127.0.0.1:11515$' "$HOME/.config/dread-universal/dread.env"
grep -q '^DREAD_COMFYUI_URL=http://127.0.0.1:8189$' "$HOME/.config/dread-universal/dread.env"
grep -q '^DREAD_WORKSPACE=/opt/dread-workspace$' "$HOME/.config/dread-universal/dread.env"
grep -q '^DREAD_MODEL_ROOTS=/models/gguf:/models/comfyui$' "$HOME/.config/dread-universal/dread.env"
grep -q '^DREAD_COMFYUI_WORKFLOW_ROOTS=/workflows$' "$HOME/.config/dread-universal/dread.env"
grep -q '^DREAD_PLUGIN_ROOTS=/plugins$' "$HOME/.config/dread-universal/dread.env"
grep -q '^DREAD_AGENT_MANIFEST_ROOTS=/agents$' "$HOME/.config/dread-universal/dread.env"
! grep -q '^Environment=DREAD_GPU_GATEWAY_URL=' "$HOME/.config/systemd/user/dread-universal.service"
WATCHDOG_SERVICE_FILE="$HOME/.config/systemd/user/dread-watchdog.service"
WATCHDOG_TIMER_FILE="$HOME/.config/systemd/user/dread-watchdog.timer"
[[ -f "$WATCHDOG_SERVICE_FILE" && -f "$WATCHDOG_TIMER_FILE" ]]
grep -Fq "$PREFIX/scripts/dread_watchdog.py --apply" "$WATCHDOG_SERVICE_FILE"
grep -Fq "$HOME/.local/state/dread-universal" "$WATCHDOG_SERVICE_FILE"
grep -Fq 'OnUnitActiveSec=60s' "$WATCHDOG_TIMER_FILE"
! grep -q 'enable --now dread-watchdog.timer' "$SYSTEMCTL_LOG"
[[ "$(stat -c '%a' "$WATCHDOG_SERVICE_FILE")" == 600 ]]
[[ "$(stat -c '%a' "$WATCHDOG_TIMER_FILE")" == 600 ]]
[[ "$(stat -c '%a' "$HOME/.config/dread-universal/dread.env")" == 600 ]]
[[ "$(cat "$STATE")" == active ]]

printf 'keep-backup-marker\n' > "$PREFIX/previous-release-marker"
# Preserve operator-managed watchdog policy through a normal upgrade.
printf '[Service]\n# operator-watchdog-override\n' > "$HOME/.config/systemd/user/dread-watchdog.service"
printf '[Timer]\n# operator-watchdog-timer-override\n' > "$HOME/.config/systemd/user/dread-watchdog.timer"
# A legacy per-app .env may still exist alongside an already-created per-user
# config. Missing allowlisted keys must be merged without replacing user values.
printf 'DREAD_COMFYUI_URL=http://127.0.0.1:8999\nDREAD_COMFYUI_USER_ROOT=/custom/comfyui-user\n' > "$PREFIX/.env"
BUNDLE2="$(make_bundle v2)"
run_installer "$BUNDLE2"
[[ "$("$PREFIX/bin/dread-universal")" == v2 ]]
[[ "$(cat "$STATE")" == active ]]
grep -q 'operator-watchdog-override' "$HOME/.config/systemd/user/dread-watchdog.service"
grep -q 'operator-watchdog-timer-override' "$HOME/.config/systemd/user/dread-watchdog.timer"
grep -q '^DREAD_COMFYUI_URL=http://127.0.0.1:8189$' "$HOME/.config/dread-universal/dread.env"
grep -q '^DREAD_COMFYUI_USER_ROOT=/custom/comfyui-user$' "$HOME/.config/dread-universal/dread.env"
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
if HOME="$HOME" DREAD_INSTALL_DIR="$PREFIX" MOCK_SYSTEMD_STATE="$STATE" MOCK_SYSTEMD_ENABLED="$TMP/systemd-enabled" MOCK_UNIT_FILE="$UNIT_FILE" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE3/install-user.sh" >/dev/null; then
  printf 'Expected the forced health-check failure to trigger rollback.\n' >&2
  exit 1
fi
[[ "$("$PREFIX/bin/dread-universal")" == v2 ]]
[[ "$(cat "$STATE")" == active ]]
cmp -s "$TMP/unit-before-failure" "$UNIT_FILE"
cmp -s "$TMP/env-before-failure" "$ENV_FILE"
[[ "$(cat "$TMP/systemd-enabled")" == enabled ]]
# A failed first install must remove its newly enabled unit even though no
# previous unit or application tree exists.
FRESH_HOME="$TMP/fresh-home"
FRESH_PREFIX="$FRESH_HOME/.local/opt/dread-universal"
FRESH_STATE="$TMP/fresh-systemd-state"
FRESH_ENABLED="$TMP/fresh-systemd-enabled"
FRESH_UNIT="$FRESH_HOME/.config/systemd/user/dread-universal.service"
mkdir -p "$FRESH_HOME"
BUNDLE4="$(make_bundle fresh)"
if HOME="$FRESH_HOME" DREAD_INSTALL_DIR="$FRESH_PREFIX" MOCK_SYSTEMD_STATE="$FRESH_STATE" MOCK_SYSTEMD_ENABLED="$FRESH_ENABLED" MOCK_UNIT_FILE="$FRESH_UNIT" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE4/install-user.sh" >/dev/null; then
  printf 'Expected a failed first install to trigger rollback.\n' >&2
  exit 1
fi
[[ ! -e "$FRESH_PREFIX" ]]
[[ ! -e "$FRESH_UNIT" ]]
[[ "$(cat "$FRESH_ENABLED")" == disabled ]]

failed_install="$(find "$HOME/.local/opt" -maxdepth 1 -type d -name 'dread-universal.failed.*' -print -quit)"
if [[ -z "$failed_install" ]]; then
  printf 'Failed release tree was not retained for diagnosis.\n' >&2
  find "$HOME/.local/opt" -maxdepth 1 -mindepth 1 -print >&2
  exit 1
fi

printf 'Portable installer migration, permissions, upgrade rollback and failed first-install cleanup: PASS\n'
 "$HOME/.config/dread-universal/dread.env"
grep -q '^DREAD_COMFYUI_URL=http://127.0.0.1:8189
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
if HOME="$HOME" DREAD_INSTALL_DIR="$PREFIX" MOCK_SYSTEMD_STATE="$STATE" MOCK_SYSTEMD_ENABLED="$TMP/systemd-enabled" MOCK_UNIT_FILE="$UNIT_FILE" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE3/install-user.sh" >/dev/null; then
  printf 'Expected the forced health-check failure to trigger rollback.\n' >&2
  exit 1
fi
[[ "$("$PREFIX/bin/dread-universal")" == v2 ]]
[[ "$(cat "$STATE")" == active ]]
cmp -s "$TMP/unit-before-failure" "$UNIT_FILE"
cmp -s "$TMP/env-before-failure" "$ENV_FILE"
[[ "$(cat "$TMP/systemd-enabled")" == enabled ]]
# A failed first install must remove its newly enabled unit even though no
# previous unit or application tree exists.
FRESH_HOME="$TMP/fresh-home"
FRESH_PREFIX="$FRESH_HOME/.local/opt/dread-universal"
FRESH_STATE="$TMP/fresh-systemd-state"
FRESH_ENABLED="$TMP/fresh-systemd-enabled"
FRESH_UNIT="$FRESH_HOME/.config/systemd/user/dread-universal.service"
mkdir -p "$FRESH_HOME"
BUNDLE4="$(make_bundle fresh)"
if HOME="$FRESH_HOME" DREAD_INSTALL_DIR="$FRESH_PREFIX" MOCK_SYSTEMD_STATE="$FRESH_STATE" MOCK_SYSTEMD_ENABLED="$FRESH_ENABLED" MOCK_UNIT_FILE="$FRESH_UNIT" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE4/install-user.sh" >/dev/null; then
  printf 'Expected a failed first install to trigger rollback.\n' >&2
  exit 1
fi
[[ ! -e "$FRESH_PREFIX" ]]
[[ ! -e "$FRESH_UNIT" ]]
[[ "$(cat "$FRESH_ENABLED")" == disabled ]]

failed_install="$(find "$HOME/.local/opt" -maxdepth 1 -type d -name 'dread-universal.failed.*' -print -quit)"
if [[ -z "$failed_install" ]]; then
  printf 'Failed release tree was not retained for diagnosis.\n' >&2
  find "$HOME/.local/opt" -maxdepth 1 -mindepth 1 -print >&2
  exit 1
fi

printf 'Portable installer migration, permissions, upgrade rollback and failed first-install cleanup: PASS\n'
 "$HOME/.config/dread-universal/dread.env"
grep -q '^DREAD_WORKSPACE=/opt/dread-workspace
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
if HOME="$HOME" DREAD_INSTALL_DIR="$PREFIX" MOCK_SYSTEMD_STATE="$STATE" MOCK_SYSTEMD_ENABLED="$TMP/systemd-enabled" MOCK_UNIT_FILE="$UNIT_FILE" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE3/install-user.sh" >/dev/null; then
  printf 'Expected the forced health-check failure to trigger rollback.\n' >&2
  exit 1
fi
[[ "$("$PREFIX/bin/dread-universal")" == v2 ]]
[[ "$(cat "$STATE")" == active ]]
cmp -s "$TMP/unit-before-failure" "$UNIT_FILE"
cmp -s "$TMP/env-before-failure" "$ENV_FILE"
[[ "$(cat "$TMP/systemd-enabled")" == enabled ]]
# A failed first install must remove its newly enabled unit even though no
# previous unit or application tree exists.
FRESH_HOME="$TMP/fresh-home"
FRESH_PREFIX="$FRESH_HOME/.local/opt/dread-universal"
FRESH_STATE="$TMP/fresh-systemd-state"
FRESH_ENABLED="$TMP/fresh-systemd-enabled"
FRESH_UNIT="$FRESH_HOME/.config/systemd/user/dread-universal.service"
mkdir -p "$FRESH_HOME"
BUNDLE4="$(make_bundle fresh)"
if HOME="$FRESH_HOME" DREAD_INSTALL_DIR="$FRESH_PREFIX" MOCK_SYSTEMD_STATE="$FRESH_STATE" MOCK_SYSTEMD_ENABLED="$FRESH_ENABLED" MOCK_UNIT_FILE="$FRESH_UNIT" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE4/install-user.sh" >/dev/null; then
  printf 'Expected a failed first install to trigger rollback.\n' >&2
  exit 1
fi
[[ ! -e "$FRESH_PREFIX" ]]
[[ ! -e "$FRESH_UNIT" ]]
[[ "$(cat "$FRESH_ENABLED")" == disabled ]]

failed_install="$(find "$HOME/.local/opt" -maxdepth 1 -type d -name 'dread-universal.failed.*' -print -quit)"
if [[ -z "$failed_install" ]]; then
  printf 'Failed release tree was not retained for diagnosis.\n' >&2
  find "$HOME/.local/opt" -maxdepth 1 -mindepth 1 -print >&2
  exit 1
fi

printf 'Portable installer migration, permissions, upgrade rollback and failed first-install cleanup: PASS\n'
 "$HOME/.config/dread-universal/dread.env"
grep -q '^DREAD_MODEL_ROOTS=/models/gguf:/models/comfyui
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
if HOME="$HOME" DREAD_INSTALL_DIR="$PREFIX" MOCK_SYSTEMD_STATE="$STATE" MOCK_SYSTEMD_ENABLED="$TMP/systemd-enabled" MOCK_UNIT_FILE="$UNIT_FILE" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE3/install-user.sh" >/dev/null; then
  printf 'Expected the forced health-check failure to trigger rollback.\n' >&2
  exit 1
fi
[[ "$("$PREFIX/bin/dread-universal")" == v2 ]]
[[ "$(cat "$STATE")" == active ]]
cmp -s "$TMP/unit-before-failure" "$UNIT_FILE"
cmp -s "$TMP/env-before-failure" "$ENV_FILE"
[[ "$(cat "$TMP/systemd-enabled")" == enabled ]]
# A failed first install must remove its newly enabled unit even though no
# previous unit or application tree exists.
FRESH_HOME="$TMP/fresh-home"
FRESH_PREFIX="$FRESH_HOME/.local/opt/dread-universal"
FRESH_STATE="$TMP/fresh-systemd-state"
FRESH_ENABLED="$TMP/fresh-systemd-enabled"
FRESH_UNIT="$FRESH_HOME/.config/systemd/user/dread-universal.service"
mkdir -p "$FRESH_HOME"
BUNDLE4="$(make_bundle fresh)"
if HOME="$FRESH_HOME" DREAD_INSTALL_DIR="$FRESH_PREFIX" MOCK_SYSTEMD_STATE="$FRESH_STATE" MOCK_SYSTEMD_ENABLED="$FRESH_ENABLED" MOCK_UNIT_FILE="$FRESH_UNIT" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE4/install-user.sh" >/dev/null; then
  printf 'Expected a failed first install to trigger rollback.\n' >&2
  exit 1
fi
[[ ! -e "$FRESH_PREFIX" ]]
[[ ! -e "$FRESH_UNIT" ]]
[[ "$(cat "$FRESH_ENABLED")" == disabled ]]

failed_install="$(find "$HOME/.local/opt" -maxdepth 1 -type d -name 'dread-universal.failed.*' -print -quit)"
if [[ -z "$failed_install" ]]; then
  printf 'Failed release tree was not retained for diagnosis.\n' >&2
  find "$HOME/.local/opt" -maxdepth 1 -mindepth 1 -print >&2
  exit 1
fi

printf 'Portable installer migration, permissions, upgrade rollback and failed first-install cleanup: PASS\n'
 "$HOME/.config/dread-universal/dread.env"
grep -q '^DREAD_COMFYUI_WORKFLOW_ROOTS=/workflows
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
if HOME="$HOME" DREAD_INSTALL_DIR="$PREFIX" MOCK_SYSTEMD_STATE="$STATE" MOCK_SYSTEMD_ENABLED="$TMP/systemd-enabled" MOCK_UNIT_FILE="$UNIT_FILE" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE3/install-user.sh" >/dev/null; then
  printf 'Expected the forced health-check failure to trigger rollback.\n' >&2
  exit 1
fi
[[ "$("$PREFIX/bin/dread-universal")" == v2 ]]
[[ "$(cat "$STATE")" == active ]]
cmp -s "$TMP/unit-before-failure" "$UNIT_FILE"
cmp -s "$TMP/env-before-failure" "$ENV_FILE"
[[ "$(cat "$TMP/systemd-enabled")" == enabled ]]
# A failed first install must remove its newly enabled unit even though no
# previous unit or application tree exists.
FRESH_HOME="$TMP/fresh-home"
FRESH_PREFIX="$FRESH_HOME/.local/opt/dread-universal"
FRESH_STATE="$TMP/fresh-systemd-state"
FRESH_ENABLED="$TMP/fresh-systemd-enabled"
FRESH_UNIT="$FRESH_HOME/.config/systemd/user/dread-universal.service"
mkdir -p "$FRESH_HOME"
BUNDLE4="$(make_bundle fresh)"
if HOME="$FRESH_HOME" DREAD_INSTALL_DIR="$FRESH_PREFIX" MOCK_SYSTEMD_STATE="$FRESH_STATE" MOCK_SYSTEMD_ENABLED="$FRESH_ENABLED" MOCK_UNIT_FILE="$FRESH_UNIT" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE4/install-user.sh" >/dev/null; then
  printf 'Expected a failed first install to trigger rollback.\n' >&2
  exit 1
fi
[[ ! -e "$FRESH_PREFIX" ]]
[[ ! -e "$FRESH_UNIT" ]]
[[ "$(cat "$FRESH_ENABLED")" == disabled ]]

failed_install="$(find "$HOME/.local/opt" -maxdepth 1 -type d -name 'dread-universal.failed.*' -print -quit)"
if [[ -z "$failed_install" ]]; then
  printf 'Failed release tree was not retained for diagnosis.\n' >&2
  find "$HOME/.local/opt" -maxdepth 1 -mindepth 1 -print >&2
  exit 1
fi

printf 'Portable installer migration, permissions, upgrade rollback and failed first-install cleanup: PASS\n'
 "$HOME/.config/dread-universal/dread.env"
grep -q '^DREAD_PLUGIN_ROOTS=/plugins
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
if HOME="$HOME" DREAD_INSTALL_DIR="$PREFIX" MOCK_SYSTEMD_STATE="$STATE" MOCK_SYSTEMD_ENABLED="$TMP/systemd-enabled" MOCK_UNIT_FILE="$UNIT_FILE" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE3/install-user.sh" >/dev/null; then
  printf 'Expected the forced health-check failure to trigger rollback.\n' >&2
  exit 1
fi
[[ "$("$PREFIX/bin/dread-universal")" == v2 ]]
[[ "$(cat "$STATE")" == active ]]
cmp -s "$TMP/unit-before-failure" "$UNIT_FILE"
cmp -s "$TMP/env-before-failure" "$ENV_FILE"
[[ "$(cat "$TMP/systemd-enabled")" == enabled ]]
# A failed first install must remove its newly enabled unit even though no
# previous unit or application tree exists.
FRESH_HOME="$TMP/fresh-home"
FRESH_PREFIX="$FRESH_HOME/.local/opt/dread-universal"
FRESH_STATE="$TMP/fresh-systemd-state"
FRESH_ENABLED="$TMP/fresh-systemd-enabled"
FRESH_UNIT="$FRESH_HOME/.config/systemd/user/dread-universal.service"
mkdir -p "$FRESH_HOME"
BUNDLE4="$(make_bundle fresh)"
if HOME="$FRESH_HOME" DREAD_INSTALL_DIR="$FRESH_PREFIX" MOCK_SYSTEMD_STATE="$FRESH_STATE" MOCK_SYSTEMD_ENABLED="$FRESH_ENABLED" MOCK_UNIT_FILE="$FRESH_UNIT" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE4/install-user.sh" >/dev/null; then
  printf 'Expected a failed first install to trigger rollback.\n' >&2
  exit 1
fi
[[ ! -e "$FRESH_PREFIX" ]]
[[ ! -e "$FRESH_UNIT" ]]
[[ "$(cat "$FRESH_ENABLED")" == disabled ]]

failed_install="$(find "$HOME/.local/opt" -maxdepth 1 -type d -name 'dread-universal.failed.*' -print -quit)"
if [[ -z "$failed_install" ]]; then
  printf 'Failed release tree was not retained for diagnosis.\n' >&2
  find "$HOME/.local/opt" -maxdepth 1 -mindepth 1 -print >&2
  exit 1
fi

printf 'Portable installer migration, permissions, upgrade rollback and failed first-install cleanup: PASS\n'
 "$HOME/.config/dread-universal/dread.env"
grep -q '^DREAD_AGENT_MANIFEST_ROOTS=/agents
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
if HOME="$HOME" DREAD_INSTALL_DIR="$PREFIX" MOCK_SYSTEMD_STATE="$STATE" MOCK_SYSTEMD_ENABLED="$TMP/systemd-enabled" MOCK_UNIT_FILE="$UNIT_FILE" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE3/install-user.sh" >/dev/null; then
  printf 'Expected the forced health-check failure to trigger rollback.\n' >&2
  exit 1
fi
[[ "$("$PREFIX/bin/dread-universal")" == v2 ]]
[[ "$(cat "$STATE")" == active ]]
cmp -s "$TMP/unit-before-failure" "$UNIT_FILE"
cmp -s "$TMP/env-before-failure" "$ENV_FILE"
[[ "$(cat "$TMP/systemd-enabled")" == enabled ]]
# A failed first install must remove its newly enabled unit even though no
# previous unit or application tree exists.
FRESH_HOME="$TMP/fresh-home"
FRESH_PREFIX="$FRESH_HOME/.local/opt/dread-universal"
FRESH_STATE="$TMP/fresh-systemd-state"
FRESH_ENABLED="$TMP/fresh-systemd-enabled"
FRESH_UNIT="$FRESH_HOME/.config/systemd/user/dread-universal.service"
mkdir -p "$FRESH_HOME"
BUNDLE4="$(make_bundle fresh)"
if HOME="$FRESH_HOME" DREAD_INSTALL_DIR="$FRESH_PREFIX" MOCK_SYSTEMD_STATE="$FRESH_STATE" MOCK_SYSTEMD_ENABLED="$FRESH_ENABLED" MOCK_UNIT_FILE="$FRESH_UNIT" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE4/install-user.sh" >/dev/null; then
  printf 'Expected a failed first install to trigger rollback.\n' >&2
  exit 1
fi
[[ ! -e "$FRESH_PREFIX" ]]
[[ ! -e "$FRESH_UNIT" ]]
[[ "$(cat "$FRESH_ENABLED")" == disabled ]]

failed_install="$(find "$HOME/.local/opt" -maxdepth 1 -type d -name 'dread-universal.failed.*' -print -quit)"
if [[ -z "$failed_install" ]]; then
  printf 'Failed release tree was not retained for diagnosis.\n' >&2
  find "$HOME/.local/opt" -maxdepth 1 -mindepth 1 -print >&2
  exit 1
fi

printf 'Portable installer migration, permissions, upgrade rollback and failed first-install cleanup: PASS\n'
 "$HOME/.config/dread-universal/dread.env"
! grep -q '^Environment=DREAD_GPU_GATEWAY_URL=' "$HOME/.config/systemd/user/dread-universal.service"
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
if HOME="$HOME" DREAD_INSTALL_DIR="$PREFIX" MOCK_SYSTEMD_STATE="$STATE" MOCK_SYSTEMD_ENABLED="$TMP/systemd-enabled" MOCK_UNIT_FILE="$UNIT_FILE" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE3/install-user.sh" >/dev/null; then
  printf 'Expected the forced health-check failure to trigger rollback.\n' >&2
  exit 1
fi
[[ "$("$PREFIX/bin/dread-universal")" == v2 ]]
[[ "$(cat "$STATE")" == active ]]
cmp -s "$TMP/unit-before-failure" "$UNIT_FILE"
cmp -s "$TMP/env-before-failure" "$ENV_FILE"
[[ "$(cat "$TMP/systemd-enabled")" == enabled ]]
# A failed first install must remove its newly enabled unit even though no
# previous unit or application tree exists.
FRESH_HOME="$TMP/fresh-home"
FRESH_PREFIX="$FRESH_HOME/.local/opt/dread-universal"
FRESH_STATE="$TMP/fresh-systemd-state"
FRESH_ENABLED="$TMP/fresh-systemd-enabled"
FRESH_UNIT="$FRESH_HOME/.config/systemd/user/dread-universal.service"
mkdir -p "$FRESH_HOME"
BUNDLE4="$(make_bundle fresh)"
if HOME="$FRESH_HOME" DREAD_INSTALL_DIR="$FRESH_PREFIX" MOCK_SYSTEMD_STATE="$FRESH_STATE" MOCK_SYSTEMD_ENABLED="$FRESH_ENABLED" MOCK_UNIT_FILE="$FRESH_UNIT" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE4/install-user.sh" >/dev/null; then
  printf 'Expected a failed first install to trigger rollback.\n' >&2
  exit 1
fi
[[ ! -e "$FRESH_PREFIX" ]]
[[ ! -e "$FRESH_UNIT" ]]
[[ "$(cat "$FRESH_ENABLED")" == disabled ]]

failed_install="$(find "$HOME/.local/opt" -maxdepth 1 -type d -name 'dread-universal.failed.*' -print -quit)"
if [[ -z "$failed_install" ]]; then
  printf 'Failed release tree was not retained for diagnosis.\n' >&2
  find "$HOME/.local/opt" -maxdepth 1 -mindepth 1 -print >&2
  exit 1
fi

printf 'Portable installer migration, permissions, upgrade rollback and failed first-install cleanup: PASS\n'
 "$HOME/.config/dread-universal/dread.env"
grep -q '^DREAD_COMFYUI_USER_ROOT=/custom/comfyui-user"$(find "$HOME/.local/opt" -maxdepth 2 -type f -name previous-release-marker -print -quit)"
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
if HOME="$HOME" DREAD_INSTALL_DIR="$PREFIX" MOCK_SYSTEMD_STATE="$STATE" MOCK_SYSTEMD_ENABLED="$TMP/systemd-enabled" MOCK_UNIT_FILE="$UNIT_FILE" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE3/install-user.sh" >/dev/null; then
  printf 'Expected the forced health-check failure to trigger rollback.\n' >&2
  exit 1
fi
[[ "$("$PREFIX/bin/dread-universal")" == v2 ]]
[[ "$(cat "$STATE")" == active ]]
cmp -s "$TMP/unit-before-failure" "$UNIT_FILE"
cmp -s "$TMP/env-before-failure" "$ENV_FILE"
[[ "$(cat "$TMP/systemd-enabled")" == enabled ]]
# A failed first install must remove its newly enabled unit even though no
# previous unit or application tree exists.
FRESH_HOME="$TMP/fresh-home"
FRESH_PREFIX="$FRESH_HOME/.local/opt/dread-universal"
FRESH_STATE="$TMP/fresh-systemd-state"
FRESH_ENABLED="$TMP/fresh-systemd-enabled"
FRESH_UNIT="$FRESH_HOME/.config/systemd/user/dread-universal.service"
mkdir -p "$FRESH_HOME"
BUNDLE4="$(make_bundle fresh)"
if HOME="$FRESH_HOME" DREAD_INSTALL_DIR="$FRESH_PREFIX" MOCK_SYSTEMD_STATE="$FRESH_STATE" MOCK_SYSTEMD_ENABLED="$FRESH_ENABLED" MOCK_UNIT_FILE="$FRESH_UNIT" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE4/install-user.sh" >/dev/null; then
  printf 'Expected a failed first install to trigger rollback.\n' >&2
  exit 1
fi
[[ ! -e "$FRESH_PREFIX" ]]
[[ ! -e "$FRESH_UNIT" ]]
[[ "$(cat "$FRESH_ENABLED")" == disabled ]]

failed_install="$(find "$HOME/.local/opt" -maxdepth 1 -type d -name 'dread-universal.failed.*' -print -quit)"
if [[ -z "$failed_install" ]]; then
  printf 'Failed release tree was not retained for diagnosis.\n' >&2
  find "$HOME/.local/opt" -maxdepth 1 -mindepth 1 -print >&2
  exit 1
fi

printf 'Portable installer migration, permissions, upgrade rollback and failed first-install cleanup: PASS\n'
 "$HOME/.config/dread-universal/dread.env"
grep -q '^DREAD_COMFYUI_URL=http://127.0.0.1:8189
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
if HOME="$HOME" DREAD_INSTALL_DIR="$PREFIX" MOCK_SYSTEMD_STATE="$STATE" MOCK_SYSTEMD_ENABLED="$TMP/systemd-enabled" MOCK_UNIT_FILE="$UNIT_FILE" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE3/install-user.sh" >/dev/null; then
  printf 'Expected the forced health-check failure to trigger rollback.\n' >&2
  exit 1
fi
[[ "$("$PREFIX/bin/dread-universal")" == v2 ]]
[[ "$(cat "$STATE")" == active ]]
cmp -s "$TMP/unit-before-failure" "$UNIT_FILE"
cmp -s "$TMP/env-before-failure" "$ENV_FILE"
[[ "$(cat "$TMP/systemd-enabled")" == enabled ]]
# A failed first install must remove its newly enabled unit even though no
# previous unit or application tree exists.
FRESH_HOME="$TMP/fresh-home"
FRESH_PREFIX="$FRESH_HOME/.local/opt/dread-universal"
FRESH_STATE="$TMP/fresh-systemd-state"
FRESH_ENABLED="$TMP/fresh-systemd-enabled"
FRESH_UNIT="$FRESH_HOME/.config/systemd/user/dread-universal.service"
mkdir -p "$FRESH_HOME"
BUNDLE4="$(make_bundle fresh)"
if HOME="$FRESH_HOME" DREAD_INSTALL_DIR="$FRESH_PREFIX" MOCK_SYSTEMD_STATE="$FRESH_STATE" MOCK_SYSTEMD_ENABLED="$FRESH_ENABLED" MOCK_UNIT_FILE="$FRESH_UNIT" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE4/install-user.sh" >/dev/null; then
  printf 'Expected a failed first install to trigger rollback.\n' >&2
  exit 1
fi
[[ ! -e "$FRESH_PREFIX" ]]
[[ ! -e "$FRESH_UNIT" ]]
[[ "$(cat "$FRESH_ENABLED")" == disabled ]]

failed_install="$(find "$HOME/.local/opt" -maxdepth 1 -type d -name 'dread-universal.failed.*' -print -quit)"
if [[ -z "$failed_install" ]]; then
  printf 'Failed release tree was not retained for diagnosis.\n' >&2
  find "$HOME/.local/opt" -maxdepth 1 -mindepth 1 -print >&2
  exit 1
fi

printf 'Portable installer migration, permissions, upgrade rollback and failed first-install cleanup: PASS\n'
 "$HOME/.config/dread-universal/dread.env"
grep -q '^DREAD_WORKSPACE=/opt/dread-workspace
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
if HOME="$HOME" DREAD_INSTALL_DIR="$PREFIX" MOCK_SYSTEMD_STATE="$STATE" MOCK_SYSTEMD_ENABLED="$TMP/systemd-enabled" MOCK_UNIT_FILE="$UNIT_FILE" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE3/install-user.sh" >/dev/null; then
  printf 'Expected the forced health-check failure to trigger rollback.\n' >&2
  exit 1
fi
[[ "$("$PREFIX/bin/dread-universal")" == v2 ]]
[[ "$(cat "$STATE")" == active ]]
cmp -s "$TMP/unit-before-failure" "$UNIT_FILE"
cmp -s "$TMP/env-before-failure" "$ENV_FILE"
[[ "$(cat "$TMP/systemd-enabled")" == enabled ]]
# A failed first install must remove its newly enabled unit even though no
# previous unit or application tree exists.
FRESH_HOME="$TMP/fresh-home"
FRESH_PREFIX="$FRESH_HOME/.local/opt/dread-universal"
FRESH_STATE="$TMP/fresh-systemd-state"
FRESH_ENABLED="$TMP/fresh-systemd-enabled"
FRESH_UNIT="$FRESH_HOME/.config/systemd/user/dread-universal.service"
mkdir -p "$FRESH_HOME"
BUNDLE4="$(make_bundle fresh)"
if HOME="$FRESH_HOME" DREAD_INSTALL_DIR="$FRESH_PREFIX" MOCK_SYSTEMD_STATE="$FRESH_STATE" MOCK_SYSTEMD_ENABLED="$FRESH_ENABLED" MOCK_UNIT_FILE="$FRESH_UNIT" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE4/install-user.sh" >/dev/null; then
  printf 'Expected a failed first install to trigger rollback.\n' >&2
  exit 1
fi
[[ ! -e "$FRESH_PREFIX" ]]
[[ ! -e "$FRESH_UNIT" ]]
[[ "$(cat "$FRESH_ENABLED")" == disabled ]]

failed_install="$(find "$HOME/.local/opt" -maxdepth 1 -type d -name 'dread-universal.failed.*' -print -quit)"
if [[ -z "$failed_install" ]]; then
  printf 'Failed release tree was not retained for diagnosis.\n' >&2
  find "$HOME/.local/opt" -maxdepth 1 -mindepth 1 -print >&2
  exit 1
fi

printf 'Portable installer migration, permissions, upgrade rollback and failed first-install cleanup: PASS\n'
 "$HOME/.config/dread-universal/dread.env"
grep -q '^DREAD_MODEL_ROOTS=/models/gguf:/models/comfyui
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
if HOME="$HOME" DREAD_INSTALL_DIR="$PREFIX" MOCK_SYSTEMD_STATE="$STATE" MOCK_SYSTEMD_ENABLED="$TMP/systemd-enabled" MOCK_UNIT_FILE="$UNIT_FILE" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE3/install-user.sh" >/dev/null; then
  printf 'Expected the forced health-check failure to trigger rollback.\n' >&2
  exit 1
fi
[[ "$("$PREFIX/bin/dread-universal")" == v2 ]]
[[ "$(cat "$STATE")" == active ]]
cmp -s "$TMP/unit-before-failure" "$UNIT_FILE"
cmp -s "$TMP/env-before-failure" "$ENV_FILE"
[[ "$(cat "$TMP/systemd-enabled")" == enabled ]]
# A failed first install must remove its newly enabled unit even though no
# previous unit or application tree exists.
FRESH_HOME="$TMP/fresh-home"
FRESH_PREFIX="$FRESH_HOME/.local/opt/dread-universal"
FRESH_STATE="$TMP/fresh-systemd-state"
FRESH_ENABLED="$TMP/fresh-systemd-enabled"
FRESH_UNIT="$FRESH_HOME/.config/systemd/user/dread-universal.service"
mkdir -p "$FRESH_HOME"
BUNDLE4="$(make_bundle fresh)"
if HOME="$FRESH_HOME" DREAD_INSTALL_DIR="$FRESH_PREFIX" MOCK_SYSTEMD_STATE="$FRESH_STATE" MOCK_SYSTEMD_ENABLED="$FRESH_ENABLED" MOCK_UNIT_FILE="$FRESH_UNIT" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE4/install-user.sh" >/dev/null; then
  printf 'Expected a failed first install to trigger rollback.\n' >&2
  exit 1
fi
[[ ! -e "$FRESH_PREFIX" ]]
[[ ! -e "$FRESH_UNIT" ]]
[[ "$(cat "$FRESH_ENABLED")" == disabled ]]

failed_install="$(find "$HOME/.local/opt" -maxdepth 1 -type d -name 'dread-universal.failed.*' -print -quit)"
if [[ -z "$failed_install" ]]; then
  printf 'Failed release tree was not retained for diagnosis.\n' >&2
  find "$HOME/.local/opt" -maxdepth 1 -mindepth 1 -print >&2
  exit 1
fi

printf 'Portable installer migration, permissions, upgrade rollback and failed first-install cleanup: PASS\n'
 "$HOME/.config/dread-universal/dread.env"
grep -q '^DREAD_COMFYUI_WORKFLOW_ROOTS=/workflows
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
if HOME="$HOME" DREAD_INSTALL_DIR="$PREFIX" MOCK_SYSTEMD_STATE="$STATE" MOCK_SYSTEMD_ENABLED="$TMP/systemd-enabled" MOCK_UNIT_FILE="$UNIT_FILE" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE3/install-user.sh" >/dev/null; then
  printf 'Expected the forced health-check failure to trigger rollback.\n' >&2
  exit 1
fi
[[ "$("$PREFIX/bin/dread-universal")" == v2 ]]
[[ "$(cat "$STATE")" == active ]]
cmp -s "$TMP/unit-before-failure" "$UNIT_FILE"
cmp -s "$TMP/env-before-failure" "$ENV_FILE"
[[ "$(cat "$TMP/systemd-enabled")" == enabled ]]
# A failed first install must remove its newly enabled unit even though no
# previous unit or application tree exists.
FRESH_HOME="$TMP/fresh-home"
FRESH_PREFIX="$FRESH_HOME/.local/opt/dread-universal"
FRESH_STATE="$TMP/fresh-systemd-state"
FRESH_ENABLED="$TMP/fresh-systemd-enabled"
FRESH_UNIT="$FRESH_HOME/.config/systemd/user/dread-universal.service"
mkdir -p "$FRESH_HOME"
BUNDLE4="$(make_bundle fresh)"
if HOME="$FRESH_HOME" DREAD_INSTALL_DIR="$FRESH_PREFIX" MOCK_SYSTEMD_STATE="$FRESH_STATE" MOCK_SYSTEMD_ENABLED="$FRESH_ENABLED" MOCK_UNIT_FILE="$FRESH_UNIT" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE4/install-user.sh" >/dev/null; then
  printf 'Expected a failed first install to trigger rollback.\n' >&2
  exit 1
fi
[[ ! -e "$FRESH_PREFIX" ]]
[[ ! -e "$FRESH_UNIT" ]]
[[ "$(cat "$FRESH_ENABLED")" == disabled ]]

failed_install="$(find "$HOME/.local/opt" -maxdepth 1 -type d -name 'dread-universal.failed.*' -print -quit)"
if [[ -z "$failed_install" ]]; then
  printf 'Failed release tree was not retained for diagnosis.\n' >&2
  find "$HOME/.local/opt" -maxdepth 1 -mindepth 1 -print >&2
  exit 1
fi

printf 'Portable installer migration, permissions, upgrade rollback and failed first-install cleanup: PASS\n'
 "$HOME/.config/dread-universal/dread.env"
grep -q '^DREAD_PLUGIN_ROOTS=/plugins
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
if HOME="$HOME" DREAD_INSTALL_DIR="$PREFIX" MOCK_SYSTEMD_STATE="$STATE" MOCK_SYSTEMD_ENABLED="$TMP/systemd-enabled" MOCK_UNIT_FILE="$UNIT_FILE" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE3/install-user.sh" >/dev/null; then
  printf 'Expected the forced health-check failure to trigger rollback.\n' >&2
  exit 1
fi
[[ "$("$PREFIX/bin/dread-universal")" == v2 ]]
[[ "$(cat "$STATE")" == active ]]
cmp -s "$TMP/unit-before-failure" "$UNIT_FILE"
cmp -s "$TMP/env-before-failure" "$ENV_FILE"
[[ "$(cat "$TMP/systemd-enabled")" == enabled ]]
# A failed first install must remove its newly enabled unit even though no
# previous unit or application tree exists.
FRESH_HOME="$TMP/fresh-home"
FRESH_PREFIX="$FRESH_HOME/.local/opt/dread-universal"
FRESH_STATE="$TMP/fresh-systemd-state"
FRESH_ENABLED="$TMP/fresh-systemd-enabled"
FRESH_UNIT="$FRESH_HOME/.config/systemd/user/dread-universal.service"
mkdir -p "$FRESH_HOME"
BUNDLE4="$(make_bundle fresh)"
if HOME="$FRESH_HOME" DREAD_INSTALL_DIR="$FRESH_PREFIX" MOCK_SYSTEMD_STATE="$FRESH_STATE" MOCK_SYSTEMD_ENABLED="$FRESH_ENABLED" MOCK_UNIT_FILE="$FRESH_UNIT" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE4/install-user.sh" >/dev/null; then
  printf 'Expected a failed first install to trigger rollback.\n' >&2
  exit 1
fi
[[ ! -e "$FRESH_PREFIX" ]]
[[ ! -e "$FRESH_UNIT" ]]
[[ "$(cat "$FRESH_ENABLED")" == disabled ]]

failed_install="$(find "$HOME/.local/opt" -maxdepth 1 -type d -name 'dread-universal.failed.*' -print -quit)"
if [[ -z "$failed_install" ]]; then
  printf 'Failed release tree was not retained for diagnosis.\n' >&2
  find "$HOME/.local/opt" -maxdepth 1 -mindepth 1 -print >&2
  exit 1
fi

printf 'Portable installer migration, permissions, upgrade rollback and failed first-install cleanup: PASS\n'
 "$HOME/.config/dread-universal/dread.env"
grep -q '^DREAD_AGENT_MANIFEST_ROOTS=/agents
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
if HOME="$HOME" DREAD_INSTALL_DIR="$PREFIX" MOCK_SYSTEMD_STATE="$STATE" MOCK_SYSTEMD_ENABLED="$TMP/systemd-enabled" MOCK_UNIT_FILE="$UNIT_FILE" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE3/install-user.sh" >/dev/null; then
  printf 'Expected the forced health-check failure to trigger rollback.\n' >&2
  exit 1
fi
[[ "$("$PREFIX/bin/dread-universal")" == v2 ]]
[[ "$(cat "$STATE")" == active ]]
cmp -s "$TMP/unit-before-failure" "$UNIT_FILE"
cmp -s "$TMP/env-before-failure" "$ENV_FILE"
[[ "$(cat "$TMP/systemd-enabled")" == enabled ]]
# A failed first install must remove its newly enabled unit even though no
# previous unit or application tree exists.
FRESH_HOME="$TMP/fresh-home"
FRESH_PREFIX="$FRESH_HOME/.local/opt/dread-universal"
FRESH_STATE="$TMP/fresh-systemd-state"
FRESH_ENABLED="$TMP/fresh-systemd-enabled"
FRESH_UNIT="$FRESH_HOME/.config/systemd/user/dread-universal.service"
mkdir -p "$FRESH_HOME"
BUNDLE4="$(make_bundle fresh)"
if HOME="$FRESH_HOME" DREAD_INSTALL_DIR="$FRESH_PREFIX" MOCK_SYSTEMD_STATE="$FRESH_STATE" MOCK_SYSTEMD_ENABLED="$FRESH_ENABLED" MOCK_UNIT_FILE="$FRESH_UNIT" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE4/install-user.sh" >/dev/null; then
  printf 'Expected a failed first install to trigger rollback.\n' >&2
  exit 1
fi
[[ ! -e "$FRESH_PREFIX" ]]
[[ ! -e "$FRESH_UNIT" ]]
[[ "$(cat "$FRESH_ENABLED")" == disabled ]]

failed_install="$(find "$HOME/.local/opt" -maxdepth 1 -type d -name 'dread-universal.failed.*' -print -quit)"
if [[ -z "$failed_install" ]]; then
  printf 'Failed release tree was not retained for diagnosis.\n' >&2
  find "$HOME/.local/opt" -maxdepth 1 -mindepth 1 -print >&2
  exit 1
fi

printf 'Portable installer migration, permissions, upgrade rollback and failed first-install cleanup: PASS\n'
 "$HOME/.config/dread-universal/dread.env"
! grep -q '^Environment=DREAD_GPU_GATEWAY_URL=' "$HOME/.config/systemd/user/dread-universal.service"
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
if HOME="$HOME" DREAD_INSTALL_DIR="$PREFIX" MOCK_SYSTEMD_STATE="$STATE" MOCK_SYSTEMD_ENABLED="$TMP/systemd-enabled" MOCK_UNIT_FILE="$UNIT_FILE" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE3/install-user.sh" >/dev/null; then
  printf 'Expected the forced health-check failure to trigger rollback.\n' >&2
  exit 1
fi
[[ "$("$PREFIX/bin/dread-universal")" == v2 ]]
[[ "$(cat "$STATE")" == active ]]
cmp -s "$TMP/unit-before-failure" "$UNIT_FILE"
cmp -s "$TMP/env-before-failure" "$ENV_FILE"
[[ "$(cat "$TMP/systemd-enabled")" == enabled ]]
# A failed first install must remove its newly enabled unit even though no
# previous unit or application tree exists.
FRESH_HOME="$TMP/fresh-home"
FRESH_PREFIX="$FRESH_HOME/.local/opt/dread-universal"
FRESH_STATE="$TMP/fresh-systemd-state"
FRESH_ENABLED="$TMP/fresh-systemd-enabled"
FRESH_UNIT="$FRESH_HOME/.config/systemd/user/dread-universal.service"
mkdir -p "$FRESH_HOME"
BUNDLE4="$(make_bundle fresh)"
if HOME="$FRESH_HOME" DREAD_INSTALL_DIR="$FRESH_PREFIX" MOCK_SYSTEMD_STATE="$FRESH_STATE" MOCK_SYSTEMD_ENABLED="$FRESH_ENABLED" MOCK_UNIT_FILE="$FRESH_UNIT" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE4/install-user.sh" >/dev/null; then
  printf 'Expected a failed first install to trigger rollback.\n' >&2
  exit 1
fi
[[ ! -e "$FRESH_PREFIX" ]]
[[ ! -e "$FRESH_UNIT" ]]
[[ "$(cat "$FRESH_ENABLED")" == disabled ]]

failed_install="$(find "$HOME/.local/opt" -maxdepth 1 -type d -name 'dread-universal.failed.*' -print -quit)"
if [[ -z "$failed_install" ]]; then
  printf 'Failed release tree was not retained for diagnosis.\n' >&2
  find "$HOME/.local/opt" -maxdepth 1 -mindepth 1 -print >&2
  exit 1
fi

printf 'Portable installer migration, permissions, upgrade rollback and failed first-install cleanup: PASS\n'
 "$HOME/.config/dread-universal/dread.env"
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
if HOME="$HOME" DREAD_INSTALL_DIR="$PREFIX" MOCK_SYSTEMD_STATE="$STATE" MOCK_SYSTEMD_ENABLED="$TMP/systemd-enabled" MOCK_UNIT_FILE="$UNIT_FILE" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE3/install-user.sh" >/dev/null; then
  printf 'Expected the forced health-check failure to trigger rollback.\n' >&2
  exit 1
fi
[[ "$("$PREFIX/bin/dread-universal")" == v2 ]]
[[ "$(cat "$STATE")" == active ]]
cmp -s "$TMP/unit-before-failure" "$UNIT_FILE"
cmp -s "$TMP/env-before-failure" "$ENV_FILE"
[[ "$(cat "$TMP/systemd-enabled")" == enabled ]]
# A failed first install must remove its newly enabled unit even though no
# previous unit or application tree exists.
FRESH_HOME="$TMP/fresh-home"
FRESH_PREFIX="$FRESH_HOME/.local/opt/dread-universal"
FRESH_STATE="$TMP/fresh-systemd-state"
FRESH_ENABLED="$TMP/fresh-systemd-enabled"
FRESH_UNIT="$FRESH_HOME/.config/systemd/user/dread-universal.service"
mkdir -p "$FRESH_HOME"
BUNDLE4="$(make_bundle fresh)"
if HOME="$FRESH_HOME" DREAD_INSTALL_DIR="$FRESH_PREFIX" MOCK_SYSTEMD_STATE="$FRESH_STATE" MOCK_SYSTEMD_ENABLED="$FRESH_ENABLED" MOCK_UNIT_FILE="$FRESH_UNIT" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE4/install-user.sh" >/dev/null; then
  printf 'Expected a failed first install to trigger rollback.\n' >&2
  exit 1
fi
[[ ! -e "$FRESH_PREFIX" ]]
[[ ! -e "$FRESH_UNIT" ]]
[[ "$(cat "$FRESH_ENABLED")" == disabled ]]

failed_install="$(find "$HOME/.local/opt" -maxdepth 1 -type d -name 'dread-universal.failed.*' -print -quit)"
if [[ -z "$failed_install" ]]; then
  printf 'Failed release tree was not retained for diagnosis.\n' >&2
  find "$HOME/.local/opt" -maxdepth 1 -mindepth 1 -print >&2
  exit 1
fi

printf 'Portable installer migration, permissions, upgrade rollback and failed first-install cleanup: PASS\n'
 "$HOME/.config/dread-universal/dread.env"
grep -q '^DREAD_COMFYUI_URL=http://127.0.0.1:8189
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
if HOME="$HOME" DREAD_INSTALL_DIR="$PREFIX" MOCK_SYSTEMD_STATE="$STATE" MOCK_SYSTEMD_ENABLED="$TMP/systemd-enabled" MOCK_UNIT_FILE="$UNIT_FILE" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE3/install-user.sh" >/dev/null; then
  printf 'Expected the forced health-check failure to trigger rollback.\n' >&2
  exit 1
fi
[[ "$("$PREFIX/bin/dread-universal")" == v2 ]]
[[ "$(cat "$STATE")" == active ]]
cmp -s "$TMP/unit-before-failure" "$UNIT_FILE"
cmp -s "$TMP/env-before-failure" "$ENV_FILE"
[[ "$(cat "$TMP/systemd-enabled")" == enabled ]]
# A failed first install must remove its newly enabled unit even though no
# previous unit or application tree exists.
FRESH_HOME="$TMP/fresh-home"
FRESH_PREFIX="$FRESH_HOME/.local/opt/dread-universal"
FRESH_STATE="$TMP/fresh-systemd-state"
FRESH_ENABLED="$TMP/fresh-systemd-enabled"
FRESH_UNIT="$FRESH_HOME/.config/systemd/user/dread-universal.service"
mkdir -p "$FRESH_HOME"
BUNDLE4="$(make_bundle fresh)"
if HOME="$FRESH_HOME" DREAD_INSTALL_DIR="$FRESH_PREFIX" MOCK_SYSTEMD_STATE="$FRESH_STATE" MOCK_SYSTEMD_ENABLED="$FRESH_ENABLED" MOCK_UNIT_FILE="$FRESH_UNIT" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE4/install-user.sh" >/dev/null; then
  printf 'Expected a failed first install to trigger rollback.\n' >&2
  exit 1
fi
[[ ! -e "$FRESH_PREFIX" ]]
[[ ! -e "$FRESH_UNIT" ]]
[[ "$(cat "$FRESH_ENABLED")" == disabled ]]

failed_install="$(find "$HOME/.local/opt" -maxdepth 1 -type d -name 'dread-universal.failed.*' -print -quit)"
if [[ -z "$failed_install" ]]; then
  printf 'Failed release tree was not retained for diagnosis.\n' >&2
  find "$HOME/.local/opt" -maxdepth 1 -mindepth 1 -print >&2
  exit 1
fi

printf 'Portable installer migration, permissions, upgrade rollback and failed first-install cleanup: PASS\n'
 "$HOME/.config/dread-universal/dread.env"
grep -q '^DREAD_WORKSPACE=/opt/dread-workspace
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
if HOME="$HOME" DREAD_INSTALL_DIR="$PREFIX" MOCK_SYSTEMD_STATE="$STATE" MOCK_SYSTEMD_ENABLED="$TMP/systemd-enabled" MOCK_UNIT_FILE="$UNIT_FILE" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE3/install-user.sh" >/dev/null; then
  printf 'Expected the forced health-check failure to trigger rollback.\n' >&2
  exit 1
fi
[[ "$("$PREFIX/bin/dread-universal")" == v2 ]]
[[ "$(cat "$STATE")" == active ]]
cmp -s "$TMP/unit-before-failure" "$UNIT_FILE"
cmp -s "$TMP/env-before-failure" "$ENV_FILE"
[[ "$(cat "$TMP/systemd-enabled")" == enabled ]]
# A failed first install must remove its newly enabled unit even though no
# previous unit or application tree exists.
FRESH_HOME="$TMP/fresh-home"
FRESH_PREFIX="$FRESH_HOME/.local/opt/dread-universal"
FRESH_STATE="$TMP/fresh-systemd-state"
FRESH_ENABLED="$TMP/fresh-systemd-enabled"
FRESH_UNIT="$FRESH_HOME/.config/systemd/user/dread-universal.service"
mkdir -p "$FRESH_HOME"
BUNDLE4="$(make_bundle fresh)"
if HOME="$FRESH_HOME" DREAD_INSTALL_DIR="$FRESH_PREFIX" MOCK_SYSTEMD_STATE="$FRESH_STATE" MOCK_SYSTEMD_ENABLED="$FRESH_ENABLED" MOCK_UNIT_FILE="$FRESH_UNIT" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE4/install-user.sh" >/dev/null; then
  printf 'Expected a failed first install to trigger rollback.\n' >&2
  exit 1
fi
[[ ! -e "$FRESH_PREFIX" ]]
[[ ! -e "$FRESH_UNIT" ]]
[[ "$(cat "$FRESH_ENABLED")" == disabled ]]

failed_install="$(find "$HOME/.local/opt" -maxdepth 1 -type d -name 'dread-universal.failed.*' -print -quit)"
if [[ -z "$failed_install" ]]; then
  printf 'Failed release tree was not retained for diagnosis.\n' >&2
  find "$HOME/.local/opt" -maxdepth 1 -mindepth 1 -print >&2
  exit 1
fi

printf 'Portable installer migration, permissions, upgrade rollback and failed first-install cleanup: PASS\n'
 "$HOME/.config/dread-universal/dread.env"
grep -q '^DREAD_MODEL_ROOTS=/models/gguf:/models/comfyui
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
if HOME="$HOME" DREAD_INSTALL_DIR="$PREFIX" MOCK_SYSTEMD_STATE="$STATE" MOCK_SYSTEMD_ENABLED="$TMP/systemd-enabled" MOCK_UNIT_FILE="$UNIT_FILE" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE3/install-user.sh" >/dev/null; then
  printf 'Expected the forced health-check failure to trigger rollback.\n' >&2
  exit 1
fi
[[ "$("$PREFIX/bin/dread-universal")" == v2 ]]
[[ "$(cat "$STATE")" == active ]]
cmp -s "$TMP/unit-before-failure" "$UNIT_FILE"
cmp -s "$TMP/env-before-failure" "$ENV_FILE"
[[ "$(cat "$TMP/systemd-enabled")" == enabled ]]
# A failed first install must remove its newly enabled unit even though no
# previous unit or application tree exists.
FRESH_HOME="$TMP/fresh-home"
FRESH_PREFIX="$FRESH_HOME/.local/opt/dread-universal"
FRESH_STATE="$TMP/fresh-systemd-state"
FRESH_ENABLED="$TMP/fresh-systemd-enabled"
FRESH_UNIT="$FRESH_HOME/.config/systemd/user/dread-universal.service"
mkdir -p "$FRESH_HOME"
BUNDLE4="$(make_bundle fresh)"
if HOME="$FRESH_HOME" DREAD_INSTALL_DIR="$FRESH_PREFIX" MOCK_SYSTEMD_STATE="$FRESH_STATE" MOCK_SYSTEMD_ENABLED="$FRESH_ENABLED" MOCK_UNIT_FILE="$FRESH_UNIT" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE4/install-user.sh" >/dev/null; then
  printf 'Expected a failed first install to trigger rollback.\n' >&2
  exit 1
fi
[[ ! -e "$FRESH_PREFIX" ]]
[[ ! -e "$FRESH_UNIT" ]]
[[ "$(cat "$FRESH_ENABLED")" == disabled ]]

failed_install="$(find "$HOME/.local/opt" -maxdepth 1 -type d -name 'dread-universal.failed.*' -print -quit)"
if [[ -z "$failed_install" ]]; then
  printf 'Failed release tree was not retained for diagnosis.\n' >&2
  find "$HOME/.local/opt" -maxdepth 1 -mindepth 1 -print >&2
  exit 1
fi

printf 'Portable installer migration, permissions, upgrade rollback and failed first-install cleanup: PASS\n'
 "$HOME/.config/dread-universal/dread.env"
grep -q '^DREAD_COMFYUI_WORKFLOW_ROOTS=/workflows
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
if HOME="$HOME" DREAD_INSTALL_DIR="$PREFIX" MOCK_SYSTEMD_STATE="$STATE" MOCK_SYSTEMD_ENABLED="$TMP/systemd-enabled" MOCK_UNIT_FILE="$UNIT_FILE" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE3/install-user.sh" >/dev/null; then
  printf 'Expected the forced health-check failure to trigger rollback.\n' >&2
  exit 1
fi
[[ "$("$PREFIX/bin/dread-universal")" == v2 ]]
[[ "$(cat "$STATE")" == active ]]
cmp -s "$TMP/unit-before-failure" "$UNIT_FILE"
cmp -s "$TMP/env-before-failure" "$ENV_FILE"
[[ "$(cat "$TMP/systemd-enabled")" == enabled ]]
# A failed first install must remove its newly enabled unit even though no
# previous unit or application tree exists.
FRESH_HOME="$TMP/fresh-home"
FRESH_PREFIX="$FRESH_HOME/.local/opt/dread-universal"
FRESH_STATE="$TMP/fresh-systemd-state"
FRESH_ENABLED="$TMP/fresh-systemd-enabled"
FRESH_UNIT="$FRESH_HOME/.config/systemd/user/dread-universal.service"
mkdir -p "$FRESH_HOME"
BUNDLE4="$(make_bundle fresh)"
if HOME="$FRESH_HOME" DREAD_INSTALL_DIR="$FRESH_PREFIX" MOCK_SYSTEMD_STATE="$FRESH_STATE" MOCK_SYSTEMD_ENABLED="$FRESH_ENABLED" MOCK_UNIT_FILE="$FRESH_UNIT" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE4/install-user.sh" >/dev/null; then
  printf 'Expected a failed first install to trigger rollback.\n' >&2
  exit 1
fi
[[ ! -e "$FRESH_PREFIX" ]]
[[ ! -e "$FRESH_UNIT" ]]
[[ "$(cat "$FRESH_ENABLED")" == disabled ]]

failed_install="$(find "$HOME/.local/opt" -maxdepth 1 -type d -name 'dread-universal.failed.*' -print -quit)"
if [[ -z "$failed_install" ]]; then
  printf 'Failed release tree was not retained for diagnosis.\n' >&2
  find "$HOME/.local/opt" -maxdepth 1 -mindepth 1 -print >&2
  exit 1
fi

printf 'Portable installer migration, permissions, upgrade rollback and failed first-install cleanup: PASS\n'
 "$HOME/.config/dread-universal/dread.env"
grep -q '^DREAD_PLUGIN_ROOTS=/plugins
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
if HOME="$HOME" DREAD_INSTALL_DIR="$PREFIX" MOCK_SYSTEMD_STATE="$STATE" MOCK_SYSTEMD_ENABLED="$TMP/systemd-enabled" MOCK_UNIT_FILE="$UNIT_FILE" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE3/install-user.sh" >/dev/null; then
  printf 'Expected the forced health-check failure to trigger rollback.\n' >&2
  exit 1
fi
[[ "$("$PREFIX/bin/dread-universal")" == v2 ]]
[[ "$(cat "$STATE")" == active ]]
cmp -s "$TMP/unit-before-failure" "$UNIT_FILE"
cmp -s "$TMP/env-before-failure" "$ENV_FILE"
[[ "$(cat "$TMP/systemd-enabled")" == enabled ]]
# A failed first install must remove its newly enabled unit even though no
# previous unit or application tree exists.
FRESH_HOME="$TMP/fresh-home"
FRESH_PREFIX="$FRESH_HOME/.local/opt/dread-universal"
FRESH_STATE="$TMP/fresh-systemd-state"
FRESH_ENABLED="$TMP/fresh-systemd-enabled"
FRESH_UNIT="$FRESH_HOME/.config/systemd/user/dread-universal.service"
mkdir -p "$FRESH_HOME"
BUNDLE4="$(make_bundle fresh)"
if HOME="$FRESH_HOME" DREAD_INSTALL_DIR="$FRESH_PREFIX" MOCK_SYSTEMD_STATE="$FRESH_STATE" MOCK_SYSTEMD_ENABLED="$FRESH_ENABLED" MOCK_UNIT_FILE="$FRESH_UNIT" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE4/install-user.sh" >/dev/null; then
  printf 'Expected a failed first install to trigger rollback.\n' >&2
  exit 1
fi
[[ ! -e "$FRESH_PREFIX" ]]
[[ ! -e "$FRESH_UNIT" ]]
[[ "$(cat "$FRESH_ENABLED")" == disabled ]]

failed_install="$(find "$HOME/.local/opt" -maxdepth 1 -type d -name 'dread-universal.failed.*' -print -quit)"
if [[ -z "$failed_install" ]]; then
  printf 'Failed release tree was not retained for diagnosis.\n' >&2
  find "$HOME/.local/opt" -maxdepth 1 -mindepth 1 -print >&2
  exit 1
fi

printf 'Portable installer migration, permissions, upgrade rollback and failed first-install cleanup: PASS\n'
 "$HOME/.config/dread-universal/dread.env"
grep -q '^DREAD_AGENT_MANIFEST_ROOTS=/agents
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
if HOME="$HOME" DREAD_INSTALL_DIR="$PREFIX" MOCK_SYSTEMD_STATE="$STATE" MOCK_SYSTEMD_ENABLED="$TMP/systemd-enabled" MOCK_UNIT_FILE="$UNIT_FILE" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE3/install-user.sh" >/dev/null; then
  printf 'Expected the forced health-check failure to trigger rollback.\n' >&2
  exit 1
fi
[[ "$("$PREFIX/bin/dread-universal")" == v2 ]]
[[ "$(cat "$STATE")" == active ]]
cmp -s "$TMP/unit-before-failure" "$UNIT_FILE"
cmp -s "$TMP/env-before-failure" "$ENV_FILE"
[[ "$(cat "$TMP/systemd-enabled")" == enabled ]]
# A failed first install must remove its newly enabled unit even though no
# previous unit or application tree exists.
FRESH_HOME="$TMP/fresh-home"
FRESH_PREFIX="$FRESH_HOME/.local/opt/dread-universal"
FRESH_STATE="$TMP/fresh-systemd-state"
FRESH_ENABLED="$TMP/fresh-systemd-enabled"
FRESH_UNIT="$FRESH_HOME/.config/systemd/user/dread-universal.service"
mkdir -p "$FRESH_HOME"
BUNDLE4="$(make_bundle fresh)"
if HOME="$FRESH_HOME" DREAD_INSTALL_DIR="$FRESH_PREFIX" MOCK_SYSTEMD_STATE="$FRESH_STATE" MOCK_SYSTEMD_ENABLED="$FRESH_ENABLED" MOCK_UNIT_FILE="$FRESH_UNIT" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE4/install-user.sh" >/dev/null; then
  printf 'Expected a failed first install to trigger rollback.\n' >&2
  exit 1
fi
[[ ! -e "$FRESH_PREFIX" ]]
[[ ! -e "$FRESH_UNIT" ]]
[[ "$(cat "$FRESH_ENABLED")" == disabled ]]

failed_install="$(find "$HOME/.local/opt" -maxdepth 1 -type d -name 'dread-universal.failed.*' -print -quit)"
if [[ -z "$failed_install" ]]; then
  printf 'Failed release tree was not retained for diagnosis.\n' >&2
  find "$HOME/.local/opt" -maxdepth 1 -mindepth 1 -print >&2
  exit 1
fi

printf 'Portable installer migration, permissions, upgrade rollback and failed first-install cleanup: PASS\n'
 "$HOME/.config/dread-universal/dread.env"
! grep -q '^Environment=DREAD_GPU_GATEWAY_URL=' "$HOME/.config/systemd/user/dread-universal.service"
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
if HOME="$HOME" DREAD_INSTALL_DIR="$PREFIX" MOCK_SYSTEMD_STATE="$STATE" MOCK_SYSTEMD_ENABLED="$TMP/systemd-enabled" MOCK_UNIT_FILE="$UNIT_FILE" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE3/install-user.sh" >/dev/null; then
  printf 'Expected the forced health-check failure to trigger rollback.\n' >&2
  exit 1
fi
[[ "$("$PREFIX/bin/dread-universal")" == v2 ]]
[[ "$(cat "$STATE")" == active ]]
cmp -s "$TMP/unit-before-failure" "$UNIT_FILE"
cmp -s "$TMP/env-before-failure" "$ENV_FILE"
[[ "$(cat "$TMP/systemd-enabled")" == enabled ]]
# A failed first install must remove its newly enabled unit even though no
# previous unit or application tree exists.
FRESH_HOME="$TMP/fresh-home"
FRESH_PREFIX="$FRESH_HOME/.local/opt/dread-universal"
FRESH_STATE="$TMP/fresh-systemd-state"
FRESH_ENABLED="$TMP/fresh-systemd-enabled"
FRESH_UNIT="$FRESH_HOME/.config/systemd/user/dread-universal.service"
mkdir -p "$FRESH_HOME"
BUNDLE4="$(make_bundle fresh)"
if HOME="$FRESH_HOME" DREAD_INSTALL_DIR="$FRESH_PREFIX" MOCK_SYSTEMD_STATE="$FRESH_STATE" MOCK_SYSTEMD_ENABLED="$FRESH_ENABLED" MOCK_UNIT_FILE="$FRESH_UNIT" MOCK_FAIL_HEALTH=1 PATH="$MOCK_BIN:$PATH" bash "$BUNDLE4/install-user.sh" >/dev/null; then
  printf 'Expected a failed first install to trigger rollback.\n' >&2
  exit 1
fi
[[ ! -e "$FRESH_PREFIX" ]]
[[ ! -e "$FRESH_UNIT" ]]
[[ "$(cat "$FRESH_ENABLED")" == disabled ]]

failed_install="$(find "$HOME/.local/opt" -maxdepth 1 -type d -name 'dread-universal.failed.*' -print -quit)"
if [[ -z "$failed_install" ]]; then
  printf 'Failed release tree was not retained for diagnosis.\n' >&2
  find "$HOME/.local/opt" -maxdepth 1 -mindepth 1 -print >&2
  exit 1
fi

printf 'Portable installer migration, permissions, upgrade rollback and failed first-install cleanup: PASS\n'
