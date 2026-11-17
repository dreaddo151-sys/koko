import importlib.util
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

SCRIPT = Path(__file__).with_name("dread_watchdog.py")
SPEC = importlib.util.spec_from_file_location("dread_watchdog", SCRIPT)
watchdog = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(watchdog)


class WatchdogPolicyTests(unittest.TestCase):
    def test_restart_allowed_below_ceiling(self):
        allowed, _ = watchdog.restart_allowed({"attempts": [100.0]}, 120.0)
        self.assertTrue(allowed)

    def test_restart_ceiling_stops_repeated_restarts(self):
        allowed, reason = watchdog.restart_allowed({"attempts": [100.0, 200.0, 300.0]}, 400.0)
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

    def test_corrupt_attempts_are_ignored_safely(self):
        allowed, _ = watchdog.restart_allowed({"attempts": ["broken", None, 100.0]}, 200.0)
        self.assertTrue(allowed)

    def test_incidents_are_appended_as_private_jsonl(self):
        with tempfile.TemporaryDirectory() as directory:
            state_path = Path(directory) / "watchdog.json"
            watchdog.append_incident(state_path, {"action": "dry_run"})
            watchdog.append_incident(state_path, {"action": "escalate"})
            journal = state_path.with_name("incidents.jsonl")
            lines = journal.read_text(encoding="utf-8").splitlines()
            self.assertEqual(len(lines), 2)
            self.assertEqual(__import__("json").loads(lines[0])["action"], "dry_run")
            self.assertEqual(journal.stat().st_mode & 0o777, 0o600)

    def test_health_check_accepts_only_status_ok(self):
        class Response:
            status = 200
            def __enter__(self): return self
            def __exit__(self, *_): return False
            def read(self, _): return b'{"status":"ok"}'
        with patch.object(watchdog.urllib.request, "urlopen", return_value=Response()):
            self.assertTrue(watchdog.is_healthy())


if __name__ == "__main__":
    unittest.main()
