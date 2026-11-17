#!/usr/bin/env bash
set -euo pipefail

CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/dread-universal"
DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/dread-universal"
UNIT_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
ENV_FILE="$CONFIG_DIR/dread.env"
SERVICE="dread-universal.service"

for command in systemctl openssl curl; do
  command -v "$command" >/dev/null 2>&1 || {
    printf 'Missing required command: %s\n' "$command" >&2
    exit 1
  }
done

install -d -m 0700 "$CONFIG_DIR" "$DATA_DIR/data" "$UNIT_DIR"
for name in services.toml agents.toml; do
  source="/usr/share/dread-universal/config/$name"
  destination="$CONFIG_DIR/$name"
  if [[ ! -e "$destination" ]]; then
    install -m 0600 "$source" "$destination"
  fi
done

if [[ ! -e "$ENV_FILE" ]]; then
  umask 077
  token="$(openssl rand -hex 32)"
  {
    printf 'DREAD_API_TOKEN=%s\n' "$token"
    printf 'DREAD_OPERATOR_ID=local-operator\n'
  } > "$ENV_FILE"
fi
chmod 0600 "$ENV_FILE"

systemctl --user daemon-reload
systemctl --user enable --now "$SERVICE"
healthy=false
for _ in $(seq 1 30); do
  if curl --fail --silent --show-error http://127.0.0.1:8765/health 2>/dev/null | grep -q 'dread-universal'; then
    healthy=true
    break
  fi
  sleep 1
done
if [[ "$healthy" != true ]]; then
  printf 'DREAD API did not become healthy. Inspect: systemctl --user status %s\n' "$SERVICE" >&2
  exit 1
fi

printf 'PROJECT DREAD is running at http://127.0.0.1:8765\n'
printf 'API token file: %s (keep private; paste its value into the console)\n' "$ENV_FILE"
printf 'Service: systemctl --user status %s\n' "$SERVICE"
printf 'GPU inference remains disabled until the existing authenticated Supervisor bridge URL and token are added to %s.\n' "$ENV_FILE"
