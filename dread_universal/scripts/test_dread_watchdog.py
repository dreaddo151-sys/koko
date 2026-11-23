import json
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

import dread_watchdog as watchdog


class WatchdogPolicyTests(unittest.TestCase):
    def test_health_url_accepts_only_explicit_port_loopback_health_endpoint(self):
        self.assertEqual(
            watchdog.validate_health_url("http://127.0.0.1:8765/health"),
            "http://127.0.0.1:8765/health",
        )
        self.assertEqual(
            watchdog.validate_health_url("http://[::1]:8765/health"),
            "http://[::1]:8765/health",
        )
        for url in [
            "https://127.0.0.1:8765/health",
            "http://127.0.0.1/health",
            "http://192.168.1.10:8765/health",
            "http://localhost:8765/admin",
            "http://user:pass@127.0.0.1:8765/health",
            "http://127.0.0.1:8765/health?x=1",
            "http://127.0.0.1:8765/health#fragment",
        ]:
            with self.subTest(url=url), self.assertRaises(ValueError):
                watchdog.validate_health_url(url)

    def test_restart_allowed_below_ceiling(self):
        allowed, _ = watchdog.restart_allowed({"attempts": [50.0]}, 120.0)
        self.assertTrue(allowed)

    def test_restart_ceiling_stops_repeated_restarts(self):
        allowed, reason = watchdog.restart_allowed(
            {"attempts": [100.0, 200.0, 300.0]}, 400.0
        )
        self.assertFalse(allowed)
        self.assertIn("ceiling", reason)

    def test_cooldown_prevents_restart_storm(self):
        allowed, reason = watchdog.restart_allowed({"attempts": [100.0]}, 110.0)
        self.assertFalse(allowed)
        self.assertIn("cooldown", reason)

    def test_state_file_is_private_and_round_trips(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "state.json"
            watchdog.write_state(path, {"attempts": [1.0]})
            self.assertEqual(watchdog.read_state(path)["attempts"], [1.0])
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)
            self.assertEqual(path.parent.stat().st_mode & 0o777, 0o700)

    def test_corrupt_attempts_are_ignored_safely(self):
        allowed, _ = watchdog.restart_allowed(
            {"attempts": ["broken", None, 100.0]}, 200.0
        )
        self.assertTrue(allowed)

    def test_incidents_are_appended_as_private_jsonl(self):
        with tempfile.TemporaryDirectory() as directory:
            state_path = Path(directory) / "watchdog.json"
            watchdog.append_incident(state_path, {"action": "dry_run"})
            watchdog.append_incident(state_path, {"action": "escalate"})
            journal = state_path.with_name("incidents.jsonl")
            lines = journal.read_text(encoding="utf-8").splitlines()
            self.assertEqual(len(lines), 2)
            self.assertEqual(json.loads(lines[0])["action"], "dry_run")
            self.assertEqual(journal.stat().st_mode & 0o777, 0o600)

    def test_health_check_accepts_only_status_ok(self):
        class Response:
            status = 200

            def __enter__(self):
                return self

            def __exit__(self, *_):
                return False

            def read(self, limit):
                self.asserted_limit = limit
                return b'{"status":"ok"}'

        class Opener:
            def open(self, request, timeout):
                self.request = request
                self.timeout = timeout
                return Response()

        opener = Opener()
        with patch.object(watchdog.urllib.request, "build_opener", return_value=opener):
            self.assertTrue(watchdog.is_healthy("http://127.0.0.1:8765/health"))
        self.assertEqual(opener.timeout, 2.0)
        self.assertEqual(opener.request.full_url, "http://127.0.0.1:8765/health")

    def test_default_mode_is_dry_run_and_never_calls_systemctl(self):
        with tempfile.TemporaryDirectory() as directory:
            state = Path(directory) / "watchdog.json"
            with (
                patch.object(watchdog, "is_healthy", return_value=False),
                patch.object(watchdog.subprocess, "run") as run,
            ):
                self.assertEqual(watchdog.main(["--state-file", str(state)]), 0)
            run.assert_not_called()
            journal = state.with_name("incidents.jsonl")
            record = json.loads(journal.read_text(encoding="utf-8").splitlines()[-1])
            self.assertEqual(record["action"], "dry_run")

    def test_apply_restarts_only_the_allowlisted_dread_service(self):
        with tempfile.TemporaryDirectory() as directory:
            state = Path(directory) / "watchdog.json"
            result = SimpleNamespace(returncode=0)
            with (
                patch.object(watchdog, "is_healthy", return_value=False),
                patch.object(watchdog.subprocess, "run", return_value=result) as run,
            ):
                self.assertEqual(
                    watchdog.main(["--apply", "--state-file", str(state)]),
                    0,
                )
            run.assert_called_once()
            self.assertEqual(
                run.call_args.args[0],
                ["systemctl", "--user", "restart", "dread-universal.service"],
            )
            stored = watchdog.read_state(state)
            self.assertEqual(stored["last_result"], "restart_requested")
            self.assertEqual(len(stored["attempts"]), 1)

    def test_restart_ceiling_prevents_systemctl_even_with_apply(self):
        with tempfile.TemporaryDirectory() as directory:
            state = Path(directory) / "watchdog.json"
            watchdog.write_state(state, {"attempts": [100.0, 200.0, 300.0]})
            with (
                patch.object(watchdog, "is_healthy", return_value=False),
                patch.object(watchdog.time, "time", return_value=400.0),
                patch.object(watchdog.subprocess, "run") as run,
            ):
                self.assertEqual(
                    watchdog.main(["--apply", "--state-file", str(state)]),
                    2,
                )
            run.assert_not_called()


if __name__ == "__main__":
    unittest.main()
