#!/usr/bin/env python3
"""Bounded watchdog for the DREAD Rust control plane only.

Default mode is dry-run. The installed user service passes --apply.
Never targets ComfyUI, the GPU gateway, the Python Supervisor, or legacy Hub.
"""
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

HEALTH_URL = "http://127.0.0.1:8765/health"
UNIT = "dread-universal.service"
WINDOW_SECONDS = 900
MAX_RESTARTS = 3
COOLDOWN_SECONDS = 60


def read_state(path: Path) -> dict:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
        return value if isinstance(value, dict) else {}
    except (OSError, ValueError):
        return {}


def write_state(path: Path, state: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(state, indent=2) + "\n", encoding="utf-8")
    os.chmod(temporary, 0o600)
    temporary.replace(path)


def is_healthy(url: str = HEALTH_URL, timeout: float = 2.0) -> bool:
    try:
        with urllib.request.urlopen(url, timeout=timeout) as response:
            if response.status != 200:
                return False
            payload = json.loads(response.read(4096).decode("utf-8"))
            return isinstance(payload, dict) and payload.get("status") == "ok"
    except (OSError, ValueError, urllib.error.URLError):
        return False


def recent_attempts(state: dict, now: float) -> list[float]:
    recent = []
    raw = state.get("attempts", [])
    if not isinstance(raw, list):
        return recent
    for item in raw:
        try:
            stamp = float(item)
        except (TypeError, ValueError):
            continue
        if 0 <= now - stamp < WINDOW_SECONDS:
            recent.append(stamp)
    return recent


def restart_allowed(state: dict, now: float) -> tuple[bool, str]:
    attempts = recent_attempts(state, now)
    if len(attempts) >= MAX_RESTARTS:
        return False, "restart ceiling reached; operator intervention required"
    last = max(attempts, default=0.0)
    if last and now - last < COOLDOWN_SECONDS:
        return False, "restart cooldown active"
    return True, "bounded restart permitted"


def append_incident(state_path: Path, incident: dict) -> None:
    journal = state_path.with_name("incidents.jsonl")
    journal.parent.mkdir(parents=True, exist_ok=True)
    payload = json.dumps(incident, separators=(",", ":")) + "\n"
    descriptor = os.open(journal, os.O_WRONLY | os.O_CREAT | os.O_APPEND, 0o600)
    try:
        os.write(descriptor, payload.encode("utf-8"))
    finally:
        os.close(descriptor)
    os.chmod(journal, 0o600)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--apply", action="store_true", help="perform the allowlisted user-service restart")
    parser.add_argument("--health-url", default=HEALTH_URL)
    parser.add_argument("--state-file", type=Path, default=Path.home() / ".local/state/dread/watchdog.json")
    args = parser.parse_args()

    now = time.time()
    if is_healthy(args.health_url):
        append_incident(args.state_file, {"at_unix": now, "health": "ok", "action": "none"})
        print("DREAD API healthy; no action")
        return 0

    state = read_state(args.state_file)
    allowed, reason = restart_allowed(state, now)
    print(f"DREAD API unhealthy; {reason}")
    if not allowed:
        append_incident(args.state_file, {"at_unix": now, "health": "failed", "action": "escalate", "reason": reason})
        return 2
    if not args.apply:
        append_incident(args.state_file, {"at_unix": now, "health": "failed", "action": "dry_run", "reason": reason})
        print(f"DRY RUN: would restart only {UNIT}")
        return 0

    attempts = recent_attempts(state, now)
    attempts.append(now)
    state["attempts"] = attempts
    state["last_action"] = "restart"
    state["last_result"] = "started"
    state["last_attempt_unix"] = now
    write_state(args.state_file, state)

    result = subprocess.run(
        ["systemctl", "--user", "restart", UNIT],
        check=False,
        timeout=30,
        capture_output=True,
        text=True,
    )
    state["last_result"] = "success" if result.returncode == 0 else "failed"
    state["last_exit_code"] = result.returncode
    write_state(args.state_file, state)
    append_incident(args.state_file, {
        "at_unix": time.time(),
        "health": "failed",
        "action": "restart",
        "unit": UNIT,
        "exit_code": result.returncode,
    })
    if result.returncode:
        print("Allowlisted DREAD API restart failed; no other service was touched", file=sys.stderr)
        return 1
    print("Restart requested for DREAD API only; next timer cycle verifies health")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
