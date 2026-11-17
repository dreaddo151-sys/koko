#!/usr/bin/env python3
"""Loopback-only mock GPU gateway and Supervisor bridge for Playwright CI."""

import json
import os
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

SUPERVISOR_TOKEN = os.environ.get("E2E_SUPERVISOR_TOKEN", "")


def handler_for(role):
    class Handler(BaseHTTPRequestHandler):
        def send_json(self, status, payload):
            body = json.dumps(payload).encode("utf-8")
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_GET(self):
            if self.path == "/health":
                if role == "gateway":
                    self.send_json(200, {"status": "ok", "gpu": "on-demand"})
                else:
                    self.send_json(200, {"status": "ok", "supervisor": "configured"})
                return
            if role == "gateway" and self.path == "/v1/models":
                self.send_json(
                    200,
                    {
                        "object": "list",
                        "data": [
                            {
                                "id": "qwen3.8:27b",
                                "object": "model",
                                "owned_by": "playwright-ci",
                            }
                        ],
                    },
                )
                return
            self.send_json(404, {"error": "not found"})

        def do_POST(self):
            if role != "supervisor" or self.path != "/v1/chat/completions":
                self.send_json(404, {"error": "not found"})
                return
            if self.headers.get("Authorization") != f"Bearer {SUPERVISOR_TOKEN}":
                self.send_json(401, {"error": "unauthorized"})
                return
            try:
                length = int(self.headers.get("Content-Length", "0"))
                if length < 1 or length > 1_048_576:
                    self.send_json(413, {"error": "invalid body size"})
                    return
                payload = json.loads(self.rfile.read(length))
            except (ValueError, json.JSONDecodeError):
                self.send_json(400, {"error": "invalid JSON"})
                return
            if payload.get("stream") is not False:
                self.send_json(400, {"error": "streaming is disabled in this fixture"})
                return
            self.send_json(
                200,
                {
                    "id": "chatcmpl-playwright",
                    "object": "chat.completion",
                    "choices": [
                        {
                            "index": 0,
                            "message": {
                                "role": "assistant",
                                "content": "E2E mock inference response",
                            },
                            "finish_reason": "stop",
                        }
                    ],
                },
            )

        def log_message(self, _format, *_args):
            return

    return Handler


def main():
    servers = [
        ThreadingHTTPServer(("127.0.0.1", 11500), handler_for("gateway")),
        ThreadingHTTPServer(("127.0.0.1", 11502), handler_for("supervisor")),
    ]
    threads = [
        threading.Thread(target=server.serve_forever, daemon=True)
        for server in servers
    ]
    for thread in threads:
        thread.start()
    try:
        threading.Event().wait()
    except KeyboardInterrupt:
        for server in servers:
            server.shutdown()
            server.server_close()


if __name__ == "__main__":
    main()
