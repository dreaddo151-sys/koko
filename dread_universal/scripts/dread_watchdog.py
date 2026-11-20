#!/usr/bin/env python3
"""Bounded, opt-in watchdog for the DREAD Rust API service only.

Default operation is dry-run. The shipped systemd oneshot passes --apply only
after an operator explicitly enables dread-watchdog.timer. This watchdog never
targets ComfyUI, llama.cpp, the Python GPU Supervisor, or the legacy Python Hub.
"""
from __future__ import annotations

import argparse
import ipaddress
import json
import os
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any

DEFAULT_HEALTH_URL = "http://127.0.0.1:8765/health"
UNIT = "dread-universal.service"
WINDOW_SECONDS = 900
MAX_RESTARTS = 3
COOLDOWN_SECONDS = 60
MAX_HEALTH_BYTES = 4096


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def validate_health_url(raw: str) -> str:
    try:
        parsed = urllib.parse.urlsplit(raw)
        port = parsed.port
    except ValueError as exc:
        raise ValueError("watchdog health URL has an invalid port") from exc
    if parsed.scheme != "http" or not parsed.hostname or port is None:
        raise ValueError("watchdog health URL must be explicit-port loopback HTTP")
    if parsed.username is not None or parsed.password is not None:
        raise ValueError("watchdog health URL cannot contain credentials")
    if parsed.query or parsed.fragment or parsed.path != "/health":
        raise ValueError("watchdog health URL must use only the /health path")
    host = parsed.hostname.lower()
    if host != "localhost":
        try:
            if not ipaddress.ip_address(host).is_loopback:
                raise ValueError("watchdog health URL must target loopback only")
        except ValueError as exc:
            raise ValueError("watchdog health URL must target loopback only") from exc
    return raw


def health_url_from_environment() -> str:
    return validate_health_url(os.environ.get("DREAD_WATCHDOG_HEALTH_URL", DEFAULT_HEALTH_URL))


def read_state(path: Path) -> dict[str, Any]:
    try:
        if path.is_symlink():
            return {}
        value = json.loads(path.read_text(encoding="utf-8"))
        return value if isinstance(value, dict) else {}
    except (OSError, ValueError):
        return {}


def write_state(path: Path, state: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    try:
        os.chmod(path.parent, 0o700)
    except OSError:
        pass
    fd, temporary_name = tempfile.mkstemp(prefix=".watchdog-", suffix=".tmp", dir=path.parent)
    temporary = Path(temporary_name)
    try:
        os.fchmod(fd, 0o600)
        with os.fdopen(fd, "w", encoding="utf-8") as stream:
            json.dump(state, stream, indent=2)
            stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
        os.chmod(path, 0o600)
    finally:
        try:
            temporary.unlink()
        except FileNotFoundError:
            pass


def is_healthy(url: str | None = None, timeout: float = 2.0) -> bool:
    try:
        checked_url = validate_health_url(url or health_url_from_environment())
        opener = urllib.request.build_opener(NoRedirect())
        request = urllib.request.Request(checked_url, headers={"Accept": "application/json"})
        with opener.open(request, timeout=timeout) as response:
            if response.status != 200:
                return False
            body = response.read(MAX_HEALTH_BYTES + 1)
            if len(body) > MAX_HEALTH_BYTES:
                return False
            payload = json.loads(body.decode("utf-8"))
            return isinstance(payload, dict) and payload.get("status") == "ok"
    except (OSError, ValueError, urllib.error.URLError, urllib.error.HTTPError):
        return False


def recent_attempts(state: dict[str, Any], now: float) -> list[float]:
    recent: list[float] = []
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
    return sorted(recent)


def restart_allowed(state: dict[str, Any], now: float) -> tuple[bool, str]:
    attempts = recent_attempts(state, now)
    if len(attempts) >= MAX_RESTARTS:
        return False, "restart ceiling reached; operator intervention required"
    last = max(attempts, default=0.0)
    if last and now - last < COOLDOWN_SECONDS:
        return False, "restart cooldown active"
    return True, "bounded restart permitted"


def append_incident(state_path: Path, incident: dict[str, Any]) -> None:
    journal = state_path.with_name("incidents.jsonl")
    journal.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    try:
        os.chmod(journal.parent, 0o700)
    except OSError:
        pass
    payload = (json.dumps(incident, separators=(",", ":")) + "\n").encode("utf-8")
    flags = os.O_WRONLY | os.O_CREAT | os.O_APPEND
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    descriptor = os.open(journal, flags, 0o600)
    try:
        os.write(descriptor, payload)
        os.fchmod(descriptor, 0o600)
    finally:
        os.close(descriptor)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--apply", action="store_true", help="restart only dread-universal.service")
    parser.add_argument(
        "--state-file",
        type=Path,
        default=Path.home() / ".local" / "state" / "dread-universal" / "watchdog.json",
    )
    args = parser.parse_args(argv)
    now = time.time()

    try:
        health_url = health_url_from_environment()
    except ValueError as exc:
        print(str(exc), file=sys.stderr)
        return 2

    if is_healthy(health_url):
        append_incident(args.state_file, {"at_unix": now, "health": "ok", "action": "none"})
        print("DREAD API healthy; no action")
        return 0

    state = read_state(args.state_file)
    allowed, reason = restart_allowed(state, now)
    print(f"DREAD API unhealthy; {reason}")
    if not allowed:
        append_incident(
            args.state_file,
            {"at_unix": now, "health": "failed", "action": "escalate", "reason": reason},
        )
        return 2
    if not args.apply:
        append_incident(
            args.state_file,
            {"at_unix": now, "health": "failed", "action": "dry_run", "reason": reason},
        )
        print(f"DRY RUN: would restart only {UNIT}")
        return 0

    attempts = recent_attempts(state, now)
    attempts.append(now)
    state["attempts"] = attempts
    state["last_action"] = "restart"
    state["last_result"] = "requested"
    state["last_attempt_unix"] = now
    write_state(args.state_file, state)

    try:
        result = subprocess.run(
            ["systemctl", "--user", "restart", UNIT],
            check=False,
            timeout=30,
            capture_output=True,
            text=True,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        state["last_result"] = "failed"
        state["last_error"] = str(exc)[:512]
        write_state(args.state_file, state)
        append_incident(
            args.state_file,
            {"at_unix": time.time(), "health": "failed", "action": "restart_failed", "unit": UNIT},
        )
        return 1

    state["last_result"] = "restart_requested" if result.returncode == 0 else "failed"
    state["last_exit_code"] = result.returncode
    write_state(args.state_file, state)
    append_incident(
        args.state_file,
        {
            "at_unix": time.time(),
            "health": "failed",
            "action": "restart_requested" if result.returncode == 0 else "restart_failed",
            "unit": UNIT,
            "exit_code": result.returncode,
        },
    )
    if result.returncode:
        print("Allowlisted DREAD API restart failed; no other service was touched", file=sys.stderr)
        return 1
    print("Restart requested for DREAD API only; the next timer cycle verifies health")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
