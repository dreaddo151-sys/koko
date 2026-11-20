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

BUNDLE_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
PREFIX="${DREAD_INSTALL_DIR:-$HOME/.local/opt/dread-universal}"
CONFIG_DIR="$HOME/.config/dread-universal"
DATA_DIR="$HOME/.local/share/dread-universal"
UNIT_DIR="$HOME/.config/systemd/user"
ENV_FILE="$CONFIG_DIR/dread.env"
SERVICE="dread-universal.service"
UNIT_FILE="$UNIT_DIR/$SERVICE"
STAMP="$(date -u +%Y%m%dT%H%M%S%N)"
BACKUP=""
ROLLBACK_DIR=""
ENV_FILE_EXISTED=false
UNIT_FILE_EXISTED=false
WAS_ENABLED=false

for command in systemctl openssl curl install dpkg; do
  command -v "$command" >/dev/null 2>&1 || {
    printf 'Missing required command: %s\n' "$command" >&2
    exit 1
  }
done

case "$PREFIX" in
  "$HOME"/*) ;;
  *) printf 'Install directory must be inside the current home directory.\n' >&2; exit 1 ;;
esac
case "$PREFIX" in
  *[[:space:]]*) printf 'Install directory cannot contain whitespace.\n' >&2; exit 1 ;;
esac

for required in bin/dread-universal web/dist/index.html config/services.toml config/agents.toml; do
  [[ -e "$BUNDLE_DIR/$required" ]] || {
    printf 'Bundle is incomplete: missing %s\n' "$required" >&2
    exit 1
  }
done

install -d -m 0700 "$CONFIG_DIR" "$DATA_DIR/data" "$UNIT_DIR" "$HOME/.cache"

was_active=false
install_complete=false
STAGE=""
ROLLBACK_DIR="$(mktemp -d "$HOME/.cache/dread-universal-rollback.XXXXXX")"
chmod 0700 "$ROLLBACK_DIR"
if [[ -e "$ENV_FILE" ]]; then
  ENV_FILE_EXISTED=true
  cp -a -- "$ENV_FILE" "$ROLLBACK_DIR/dread.env"
fi
if [[ -e "$UNIT_FILE" ]]; then
  UNIT_FILE_EXISTED=true
  cp -a -- "$UNIT_FILE" "$ROLLBACK_DIR/dread-universal.service"
fi
if systemctl --user is-enabled --quiet "$SERVICE" 2>/dev/null; then
  WAS_ENABLED=true
fi
cleanup() {
  if [[ -n "${STAGE:-}" && -d "$STAGE" ]]; then
    rm -rf -- "$STAGE"
  fi
  if [[ -n "${ROLLBACK_DIR:-}" && -d "$ROLLBACK_DIR" ]]; then
    rm -rf -- "$ROLLBACK_DIR"
  fi
  if [[ "$was_active" == true && "$install_complete" != true ]]; then
    systemctl --user start "$SERVICE" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

# Stop only DREAD's own API before touching its SQLite files; never stop ComfyUI or the legacy Hub.
if systemctl --user is-active --quiet "$SERVICE"; then
  was_active=true
  systemctl --user stop "$SERVICE"
elif curl --fail --silent http://127.0.0.1:8765/health 2>/dev/null | grep -q 'dread-universal'; then
  printf 'An unmanaged DREAD API is using port 8765. Stop it before installing to avoid copying a live database.\n' >&2
  exit 1
fi

# Migrate legacy app-local config and SQLite data without overwriting newer user data.
if [[ -d "$PREFIX/config" ]]; then
  for name in services.toml agents.toml; do
    if [[ ! -e "$CONFIG_DIR/$name" && -f "$PREFIX/config/$name" ]]; then
      install -m 0600 "$PREFIX/config/$name" "$CONFIG_DIR/$name"
    fi
  done
fi
for name in services.toml agents.toml; do
  if [[ ! -e "$CONFIG_DIR/$name" ]]; then
    install -m 0600 "$BUNDLE_DIR/config/$name" "$CONFIG_DIR/$name"
  fi
done
if [[ -d "$PREFIX/data" ]]; then
  cp -a -n "$PREFIX/data/." "$DATA_DIR/data/"
fi

# Preserve known runtime credentials/settings from a prior local .env, never copy unrelated variables.
if [[ ! -e "$ENV_FILE" ]]; then
  umask 077
  : > "$ENV_FILE"
  if [[ -f "$PREFIX/.env" ]]; then
    grep -E '^(DREAD_API_TOKEN|DREAD_OPERATOR_ID|DREAD_BIND|DREAD_DB_PATH|DREAD_WEB_ROOT|DREAD_SERVICES_CONFIG|DREAD_AGENTS_CONFIG|DREAD_GPU_GATEWAY_URL|DREAD_GPU_SUPERVISOR_URL|DREAD_GPU_SUPERVISOR_TOKEN|DREAD_COMFYUI_URL|DREAD_COMFYUI_ROOT|COMFYUI_HOME|DREAD_WORKSPACE|DREAD_MODEL_ROOTS|DREAD_COMFYUI_USER_ROOT|DREAD_COMFYUI_WORKFLOW_ROOTS|DREAD_PLUGIN_ROOTS|DREAD_AGENT_MANIFEST_ROOTS|DREAD_SKILLS_ROOT|DREAD_PSTACK_VERSION|DREAD_PSTACK_REVISION)=' "$PREFIX/.env" >> "$ENV_FILE" || true
  fi
fi
if ! grep -q '^DREAD_API_TOKEN=' "$ENV_FILE"; then
  printf 'DREAD_API_TOKEN=%s\n' "$(openssl rand -hex 32)" >> "$ENV_FILE"
fi
if ! grep -q '^DREAD_OPERATOR_ID=' "$ENV_FILE"; then
  printf 'DREAD_OPERATOR_ID=local-operator\n' >> "$ENV_FILE"
fi
chmod 0600 "$ENV_FILE"

install -d -m 0700 "$(dirname -- "$PREFIX")"
STAGE="$(mktemp -d "${PREFIX}.stage.XXXXXX")"
cp -a "$BUNDLE_DIR/bin" "$BUNDLE_DIR/web" "$BUNDLE_DIR/scripts" "$BUNDLE_DIR/docs" "$STAGE/"
cp -a "$BUNDLE_DIR/config" "$STAGE/"
if [[ -d "$PREFIX/config" ]]; then
  cp -a "$PREFIX/config/." "$STAGE/config/"
fi
if [[ -d "$PREFIX/data" ]]; then
  mkdir -p "$STAGE/data"
  cp -a -n "$PREFIX/data/." "$STAGE/data/"
fi

if [[ -e "$PREFIX" ]]; then
  BACKUP="${PREFIX}.backup.$STAMP"
  mv -- "$PREFIX" "$BACKUP"
fi
mv -- "$STAGE" "$PREFIX"
STAGE=""

cat > "$UNIT_FILE" <<EOF
[Unit]
Description=PROJECT DREAD Universal Master Hub
After=default.target

[Service]
Type=simple
EnvironmentFile=-$ENV_FILE
Environment=DREAD_BIND=127.0.0.1:8765
Environment=DREAD_DB_PATH=$DATA_DIR/data/dread_registry.sqlite3
Environment=DREAD_WEB_ROOT=$PREFIX/web/dist
Environment=DREAD_SERVICES_CONFIG=$CONFIG_DIR/services.toml
Environment=DREAD_AGENTS_CONFIG=$CONFIG_DIR/agents.toml
WorkingDirectory=$PREFIX
ExecStart=$PREFIX/bin/dread-universal
Restart=on-failure
RestartSec=3
UMask=0077
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=full
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectControlGroups=true
RestrictSUIDSGID=true
LockPersonality=true
RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6

[Install]
WantedBy=default.target
EOF
chmod 0600 "$UNIT_FILE"

rollback() {
  systemctl --user stop "$SERVICE" >/dev/null 2>&1 || true
  if [[ -e "$PREFIX" ]]; then
    mv -- "$PREFIX" "${PREFIX}.failed.$STAMP"
  fi
  if [[ -n "$BACKUP" && -e "$BACKUP" ]]; then
    mv -- "$BACKUP" "$PREFIX"
  fi

  # Restore service metadata and credentials as well as the application tree.
  # Without this, a failed update could leave the previous binary paired with
  # a newly generated token file or a newly written systemd unit.
  # Disable the failed unit before removing its unit file; systemd may no
  # longer resolve the unit name after daemon-reload if this was a first install.
  if [[ "$WAS_ENABLED" != true ]]; then
    systemctl --user disable "$SERVICE" >/dev/null 2>&1 || true
  fi
  if [[ "$ENV_FILE_EXISTED" == true ]]; then
    install -m 0600 "$ROLLBACK_DIR/dread.env" "$ENV_FILE"
  else
    rm -f -- "$ENV_FILE"
  fi
  if [[ "$UNIT_FILE_EXISTED" == true ]]; then
    install -m 0600 "$ROLLBACK_DIR/dread-universal.service" "$UNIT_FILE"
  else
    rm -f -- "$UNIT_FILE"
  fi
  systemctl --user daemon-reload || true
  if [[ "$WAS_ENABLED" == true ]]; then
    systemctl --user enable "$SERVICE" >/dev/null 2>&1 || true
  fi
  if [[ "$was_active" == true ]]; then
    systemctl --user start "$SERVICE" >/dev/null 2>&1 || true
  fi
  install_complete=true
}

systemctl --user daemon-reload
if ! systemctl --user enable --now "$SERVICE"; then
  rollback
  printf 'DREAD service failed to start; previous application files were restored.\n' >&2
  exit 1
fi

healthy=false
for _ in $(seq 1 30); do
  if curl --fail --silent http://127.0.0.1:8765/health 2>/dev/null | grep -q 'dread-universal'; then
    healthy=true
    break
  fi
  sleep 1
done
if [[ "$healthy" != true ]]; then
  rollback
  printf 'DREAD health check failed; previous application files were restored.\n' >&2
  exit 1
fi
install_complete=true

printf 'PROJECT DREAD installed and healthy at http://127.0.0.1:8765\n'
printf 'API token file: %s (keep private; paste its value into the console)\n' "$ENV_FILE"
printf 'Service: systemctl --user status %s\n' "$SERVICE"
if [[ -n "$BACKUP" ]]; then
  printf 'Previous application files backed up at: %s\n' "$BACKUP"
fi
printf 'Existing user data and configuration were preserved. GPU inference requires the existing Supervisor bridge credentials in %s.\n' "$ENV_FILE"
