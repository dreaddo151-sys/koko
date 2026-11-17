import json
import unittest
from contextlib import contextmanager

from dread_gpu_supervisor_bridge import (
    TENSOR_UPSTREAM_URL,
    SupervisorBridge,
    validate_bind,
    validate_payload,
)


class SupervisorBridgeTests(unittest.TestCase):
    def test_payload_requires_non_streaming_text_only_chat(self):
        normalized = validate_payload({
            "model": "local-model",
            "messages": [{"role": "user", "content": "hello"}],
        })
        self.assertIs(normalized["stream"], False)
        for payload in (
            {"model": "local-model", "messages": [{"role": "user", "content": "hello"}],
             "tools": [{"type": "function"}]},
            {"model": "local-model", "messages": [{"role": "tool", "content": "hello"}]},
            {"model": "local-model", "messages": [{"role": "user", "content": "hello"}],
             "stream": True},
        ):
            with self.assertRaises(ValueError):
                validate_payload(payload)

    def test_bridge_holds_one_lease_and_calls_managed_upstream_directly(self):
        events = []

        @contextmanager
        def fake_lease(mode):
            events.append(("enter", mode))
            try:
                yield
            finally:
                events.append(("exit", mode))

        class FakeResponse:
            status = 200
            def __enter__(self): return self
            def __exit__(self, *_args): return False
            def read(self, _limit):
                events.append(("response-read", None))
                return json.dumps({"choices": [{"message": {"content": "ok"}}]}).encode()

        def fake_open(request, timeout):
            events.append(("upstream", request.full_url, timeout))
            return FakeResponse()

        bridge = SupervisorBridge(fake_lease, fake_open)
        status, result = bridge.execute({
            "model": "local-model",
            "messages": [{"role": "user", "content": "hello"}],
            "stream": False,
        })
        self.assertEqual(status, 200)
        self.assertEqual(result["choices"][0]["message"]["content"], "ok")
        self.assertEqual(events[0], ("enter", "tensor"))
        self.assertEqual(events[1][0], "upstream")
        self.assertEqual(events[1][1], TENSOR_UPSTREAM_URL)
        self.assertEqual(events[2][0], "response-read")
        self.assertEqual(events[3], ("exit", "tensor"))
        self.assertEqual(TENSOR_UPSTREAM_URL, "http://127.0.0.1:11501/v1/chat/completions")
        self.assertNotIn("11500", TENSOR_UPSTREAM_URL)

    def test_bridge_releases_supervisor_lease_when_upstream_fails(self):
        events = []

        @contextmanager
        def fake_lease(mode):
            events.append("enter")
            try:
                yield
            finally:
                events.append("exit")

        def fail_open(_request, timeout):
            raise OSError("mock tensor upstream offline")

        bridge = SupervisorBridge(fake_lease, fail_open)
        status, result = bridge.execute({
            "model": "local-model",
            "messages": [{"role": "user", "content": "hello"}],
        })
        self.assertEqual(status, 503)
        self.assertIn("mock tensor upstream offline", result["error"])
        self.assertEqual(events, ["enter", "exit"])

    def test_bind_is_loopback_only(self):
        self.assertEqual(validate_bind("127.0.0.1:11502"), ("127.0.0.1", 11502))
        with self.assertRaises(ValueError):
            validate_bind("0.0.0.0:11502")
        with self.assertRaises(ValueError):
            validate_bind("192.168.1.2:11502")


if __name__ == "__main__":
    unittest.main()
