#!/usr/bin/env python3
"""Authenticated loopback inference bridge using the existing Python GPU Supervisor lease."""

from __future__ import annotations

import hmac
import ipaddress
import json
import os
import runpy
import signal
import threading
import urllib.error
import urllib.request
from contextlib import AbstractContextManager
from http.server import BaseHTTPRequestHandler, HTTPServer
from typing import Callable

MAX_BODY_BYTES = 1_048_576
GATEWAY_URL = "http://127.0.0.1:11500/v1/chat/completions"
INFERENCE_TIMEOUT = 1805


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, file_pointer, code, message, headers, new_url):
        return None


GATEWAY_OPENER = urllib.request.build_opener(NoRedirect())


def validate_payload(payload: object) -> dict:
    if not isinstance(payload, dict):
        raise ValueError("request body must be a JSON object")
    allowed = {"model", "messages", "temperature", "max_tokens", "stream"}
    if set(payload) - allowed:
        raise ValueError("unsupported chat completion fields")
    model = payload.get("model")
    if not isinstance(model, str) or not model.strip() or len(model) > 200:
        raise ValueError("model must be a non-empty id of at most 200 characters")
    messages = payload.get("messages")
    if not isinstance(messages, list) or not 1 <= len(messages) <= 128:
        raise ValueError("messages must contain between 1 and 128 entries")
    total = 0
    for message in messages:
        if not isinstance(message, dict) or set(message) != {"role", "content"}:
            raise ValueError("messages may contain only role and content")
        if message["role"] not in {"system", "user", "assistant"}:
            raise ValueError("message role must be system, user or assistant")
        if not isinstance(message["content"], str) or len(message["content"].encode()) > 65536:
            raise ValueError("message content must be text of at most 65536 bytes")
        total += len(message["content"].encode())
    if total > MAX_BODY_BYTES:
        raise ValueError("total message content exceeds 1 MiB")
    if "temperature" in payload:
        value = payload["temperature"]
        if isinstance(value, bool) or not isinstance(value, (int, float)) or not 0 <= value <= 2:
            raise ValueError("temperature must be between 0 and 2")
    if "max_tokens" in payload:
        value = payload["max_tokens"]
        if isinstance(value, bool) or not isinstance(value, int) or not 1 <= value <= 32768:
            raise ValueError("max_tokens must be between 1 and 32768")
    if payload.get("stream", False) is not False:
        raise ValueError("Supervisor bridge currently permits non-streaming inference only")
    normalized = dict(payload)
    normalized["stream"] = False
    return normalized


class SupervisorBridge:
    def __init__(self, gpu_lease: Callable[[str], AbstractContextManager], gateway_open=GATEWAY_OPENER.open):
        self.gpu_lease = gpu_lease
        self.gateway_open = gateway_open

    def execute(self, payload: object) -> tuple[int, dict]:
        try:
            normalized = validate_payload(payload)
        except ValueError as error:
            return 400, {"error": str(error)}
        request = urllib.request.Request(
            GATEWAY_URL,
            data=json.dumps(normalized).encode(),
            method="POST",
            headers={"Content-Type": "application/json", "Accept": "application/json"},
        )
        try:
            # The existing Supervisor owns the lock, waits for ComfyUI idle,
            # and starts/stops its managed tensor service for this request.
            with self.gpu_lease("tensor"):
                with self.gateway_open(request, timeout=INFERENCE_TIMEOUT) as response:
                    status = response.status
                    body = response.read(MAX_BODY_BYTES + 1)
        except urllib.error.HTTPError as error:
            status = error.code
            body = error.read(MAX_BODY_BYTES + 1)
        except Exception as error:
            return 503, {"error": f"GPU Supervisor lease or inference failed: {error}"}
        if len(body) > MAX_BODY_BYTES:
            return 502, {"error": "GPU gateway response exceeds the 1 MiB result limit"}
        try:
            decoded = json.loads(body.decode())
        except (UnicodeDecodeError, json.JSONDecodeError):
            return 502, {"error": "GPU gateway returned invalid JSON"}
        if not 200 <= status < 300:
            return status, decoded if isinstance(decoded, dict) else {"error": "GPU gateway request failed"}
        if not isinstance(decoded, dict):
            return 502, {"error": "GPU gateway response must be a JSON object"}
        return status, decoded


def load_gpu_lease(source_path: str):
    if not source_path or not os.path.isfile(source_path):
        raise RuntimeError("DREAD_GPU_SUPERVISOR_SOURCE must point to the existing Python GPU Supervisor file")
    namespace = runpy.run_path(source_path)
    lease = namespace.get("gpu_lease")
    if not callable(lease):
        raise RuntimeError("configured Supervisor source does not expose gpu_lease(mode)")
    return lease


def validate_bind(raw: str) -> tuple[str, int]:
    host, separator, port_text = raw.rpartition(":")
    if not separator or not port_text.isdigit():
        raise ValueError("bind address must include an explicit port")
    if host == "localhost":
        host = "127.0.0.1"
    address = ipaddress.ip_address(host.strip("[]"))
    if not address.is_loopback:
        raise ValueError("GPU Supervisor bridge must bind to loopback")
    port = int(port_text)
    if not 1 <= port <= 65535:
        raise ValueError("invalid bridge port")
    return str(address), port


def make_handler(bridge: SupervisorBridge, token: str):
    class Handler(BaseHTTPRequestHandler):
        server_version = "DREAD-GPU-SUPERVISOR/1"

        def _send(self, status: int, payload: dict):
            body = json.dumps(payload, separators=(",", ":")).encode()
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            self.wfile.write(body)

        def _authorized(self) -> bool:
            supplied = self.headers.get("Authorization", "")
            expected = f"Bearer {token}"
            return hmac.compare_digest(supplied.encode(), expected.encode())

        def do_GET(self):
            if self.path == "/health":
                self._send(200, {"status": "ok", "supervisor": "configured"})
            else:
                self._send(404, {"error": "not found"})

        def do_POST(self):
            if self.path != "/v1/chat/completions":
                self._send(404, {"error": "only /v1/chat/completions is exposed"})
                return
            if not self._authorized():
                self._send(401, {"error": "invalid bearer token"})
                return
            try:
                length = int(self.headers.get("Content-Length", "0"))
            except ValueError:
                self._send(400, {"error": "invalid Content-Length"})
                return
            if length <= 0 or length > MAX_BODY_BYTES:
                self._send(413, {"error": "request body must be between 1 byte and 1 MiB"})
                return
            try:
                payload = json.loads(self.rfile.read(length))
            except (UnicodeDecodeError, json.JSONDecodeError):
                self._send(400, {"error": "request body is not valid JSON"})
                return
            status, result = bridge.execute(payload)
            self._send(status, result)

        def log_message(self, format_string, *args):
            super().log_message(format_string, *args)

    return Handler


def main():
    token = os.environ.get("DREAD_GPU_SUPERVISOR_TOKEN", "")
    if len(token) < 32 or token.strip() != token:
        raise SystemExit("DREAD_GPU_SUPERVISOR_TOKEN must be at least 32 characters")
    bind = os.environ.get("DREAD_GPU_SUPERVISOR_BIND", "127.0.0.1:11502")
    host, port = validate_bind(bind)
    source = os.environ.get("DREAD_GPU_SUPERVISOR_SOURCE", "")
    bridge = SupervisorBridge(load_gpu_lease(source))
    server = HTTPServer((host, port), make_handler(bridge, token))

    def request_shutdown(_signum, _frame):
        threading.Thread(target=server.shutdown, daemon=True).start()

    signal.signal(signal.SIGTERM, request_shutdown)
    signal.signal(signal.SIGINT, request_shutdown)
    print(f"DREAD GPU Supervisor bridge listening on http://{host}:{port}", flush=True)
    try:
        server.serve_forever(poll_interval=0.5)
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
