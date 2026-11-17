# DREAD user-service deployment notes

These are operator-run steps. The templates are not installed, enabled or started by CI.

## Preconditions

- Build the release binary from `dread_universal`.
- Ensure no foreground `dread-universal` process already owns `127.0.0.1:8765`. Never start a second API instance against the same SQLite database.
- Keep ComfyUI (`127.0.0.1:8188`), the llama.cpp gateway (`127.0.0.1:11500`), its upstream (`127.0.0.1:11501`), the Python GPU Supervisor bridge (`127.0.0.1:11502`) and the legacy Hub untouched.
- Create a private API environment file from `api.env.example`. Populate `DREAD_API_TOKEN` from the operator's existing private token file locally; do not paste or commit it. Add Supervisor URL/token only after the bridge itself has been configured and tested.

## Install templates (operator action)

From the repository root:

```bash
mkdir -p "$HOME/.config/dread" "$HOME/.local/state/dread" \
  "$HOME/.config/systemd/user" "$HOME/dread_rust_redesign/dread_universal/data"
chmod 700 "$HOME/.config/dread" "$HOME/.local/state/dread"
install -m 600 dread_universal/deploy/api.env.example "$HOME/.config/dread/api.env"
install -m 600 dread_universal/deploy/systemd/dread-universal.service "$HOME/.config/systemd/user/"
install -m 600 dread_universal/deploy/systemd/dread-watchdog.service "$HOME/.config/systemd/user/"
install -m 600 dread_universal/deploy/systemd/dread-watchdog.timer "$HOME/.config/systemd/user/"
systemctl --user daemon-reload
```

Edit `~/.config/dread/api.env` locally and set a valid `DREAD_API_TOKEN` (at least 32 characters). Never store secrets in Git.

Before enabling anything, confirm that the service's `WorkingDirectory` and `ExecStart` match the checked-out workspace and release binary. If port 8765 is already occupied, resolve the existing foreground process deliberately; do not launch a duplicate.

## Start and verify (operator action)

```bash
systemctl --user enable --now dread-universal.service
curl --fail --silent http://127.0.0.1:8765/health
systemctl --user status dread-universal.service --no-pager
```

Only after the API health check succeeds, enable the watchdog timer:

```bash
systemctl --user enable --now dread-watchdog.timer
systemctl --user list-timers dread-watchdog.timer
```

Watchdog policy:
- It probes only `http://127.0.0.1:8765/health`.
- The installed oneshot passes `--apply`; an unconfigured/manual invocation is dry-run.
- It may restart only `dread-universal.service`, at most three times in a rolling 15-minute window and no more than once per 60 seconds.
- It records private state in `~/.local/state/dread/watchdog.json` and append-only incident entries in `incidents.jsonl`.
- When the ceiling is reached it escalates and stops restarting. It does not patch source/configuration or attempt destructive repairs.
- It never targets ComfyUI, llama.cpp, the Python GPU Supervisor bridge, the GPU gateway or the legacy Hub.

Inspect incidents with:

```bash
tail -n 50 "$HOME/.local/state/dread/incidents.jsonl"
journalctl --user -u dread-universal.service -u dread-watchdog.service -n 100 --no-pager
```

Rollback is operator-controlled: stop/disable only the DREAD units you installed and restore the previous DREAD launcher. Do not remove databases, backups, ComfyUI state or legacy Hub files.
