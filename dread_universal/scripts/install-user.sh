#!/usr/bin/env bash
set -euo pipefail

BUNDLE_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
PREFIX="${DREAD_INSTALL_DIR:-$HOME/.local/opt/dread-universal}"
CONFIG_DIR="$HOME/.config/dread-universal"
DATA_DIR="$HOME/.local/share/dread-universal"
UNIT_DIR="$HOME/.config/systemd/user"
ENV_FILE="$CONFIG_DIR/dread.env"
SERVICE="dread-universal.service"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
BACKUP=""

for command in systemctl openssl curl install; do
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

install -d -m 0700 "$CONFIG_DIR" "$DATA_DIR/data" "$UNIT_DIR"
for name in services.toml agents.toml; do
  if [[ ! -e "$CONFIG_DIR/$name" ]]; then
    install -m 0600 "$BUNDLE_DIR/config/$name" "$CONFIG_DIR/$name"
  fi
done
if [[ ! -e "$ENV_FILE" ]]; then
  umask 077
  {
    printf 'DREAD_API_TOKEN=%s\n' "$(openssl rand -hex 32)"
    printf 'DREAD_OPERATOR_ID=local-operator\n'
  } > "$ENV_FILE"
fi
chmod 0600 "$ENV_FILE"

install -d -m 0700 "$(dirname -- "$PREFIX")"
STAGE="$(mktemp -d "${PREFIX}.stage.XXXXXX")"
cleanup() {
  [[ -n "${STAGE:-}" && -d "$STAGE" ]] && rm -rf -- "$STAGE"
}
trap cleanup EXIT
cp -a "$BUNDLE_DIR/bin" "$BUNDLE_DIR/web" "$BUNDLE_DIR/scripts" "$BUNDLE_DIR/docs" "$STAGE/"
cp -a "$BUNDLE_DIR/config" "$STAGE/"

was_active=false
if systemctl --user is-active --quiet "$SERVICE"; then
  was_active=true
  systemctl --user stop "$SERVICE"
fi

if [[ -e "$PREFIX" ]]; then
  BACKUP="${PREFIX}.backup.$STAMP"
  mv -- "$PREFIX" "$BACKUP"
fi
mv -- "$STAGE" "$PREFIX"
STAGE=""

cat > "$UNIT_DIR/$SERVICE" <<EOF
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
Environment=DREAD_GPU_GATEWAY_URL=http://127.0.0.1:11500
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
chmod 0600 "$UNIT_DIR/$SERVICE"

rollback() {
  systemctl --user stop "$SERVICE" >/dev/null 2>&1 || true
  if [[ -e "$PREFIX" ]]; then
    mv -- "$PREFIX" "${PREFIX}.failed.$STAMP"
  fi
  if [[ -n "$BACKUP" && -e "$BACKUP" ]]; then
    mv -- "$BACKUP" "$PREFIX"
    systemctl --user daemon-reload || true
    systemctl --user start "$SERVICE" >/dev/null 2>&1 || true
  else
    systemctl --user disable "$SERVICE" >/dev/null 2>&1 || true
    rm -f -- "$UNIT_DIR/$SERVICE"
    systemctl --user daemon-reload || true
  fi
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

printf 'PROJECT DREAD installed and healthy at http://127.0.0.1:8765\n'
printf 'API token file: %s (keep private; paste its value into the console)\n' "$ENV_FILE"
printf 'Service: systemctl --user status %s\n' "$SERVICE"
if [[ -n "$BACKUP" ]]; then
  printf 'Previous application files backed up at: %s\n' "$BACKUP"
fi
printf 'Existing user data and configuration were preserved. GPU inference requires the existing Supervisor bridge credentials in %s.\n' "$ENV_FILE"
