#!/usr/bin/env python3
"""OpenAI-compatible DREAD gateway with true SSE streaming."""
from http.server import ThreadingHTTPServer, BaseHTTPRequestHandler
import json
import urllib.request
import urllib.error

from dread_gpu_supervisor import gpu_lease

UPSTREAM = "http://127.0.0.1:11501"
MODEL = "qwen3.8:27b"


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def send_json(self, code, obj):
        body = json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if self.path in ("/health", "/"):
            return self.send_json(200, {"status": "ok", "gpu": "on-demand"})
        if self.path == "/api/tags":
            return self.send_json(200, {"models": [{"name": MODEL, "model": MODEL}]})
        if self.path == "/v1/models":
            return self.send_json(200, {
                "object": "list",
                "data": [{"id": MODEL, "object": "model", "owned_by": "DREAD"}]
            })
        return self.send_json(404, {"error": "Not found"})

    def do_POST(self):
        if not self.path.startswith("/v1/"):
            return self.send_json(404, {"error": "Not found"})

        length = int(self.headers.get("Content-Length", "0"))
        if length <= 0 or length > 32 * 1024 * 1024:
            return self.send_json(400, {"error": "Invalid request size"})

        try:
            payload = json.loads(self.rfile.read(length))
            if not isinstance(payload, dict):
                return self.send_json(400, {"error": "JSON object required"})

            streaming = payload.get("stream") is True
            payload["model"] = MODEL
            body = json.dumps(payload).encode()

            req = urllib.request.Request(
                UPSTREAM + self.path,
                data=body,
                method="POST",
                headers={"Content-Type": "application/json"}
            )

            # Keep the GPU lease for the entire upstream response and forwarding.
            with gpu_lease("tensor"):
                try:
                    upstream = urllib.request.urlopen(req, timeout=1800)
                except urllib.error.HTTPError as error:
                    upstream = error

                with upstream:
                    code = upstream.status
                    content_type = upstream.headers.get(
                        "Content-Type", "application/json"
                    )

                    self.send_response(code)
                    self.send_header("Content-Type", content_type)

                    if streaming and code < 400:
                        self.send_header("Cache-Control", "no-cache")
                        self.send_header("X-Accel-Buffering", "no")
                        self.send_header("Connection", "close")
                        self.end_headers()
                        self.close_connection = True

                        while True:
                            chunk = upstream.read1(8192)
                            if not chunk:
                                break
                            self.wfile.write(chunk)
                            self.wfile.flush()

                        print("[DREAD gateway] SSE stream completed", flush=True)
                    else:
                        result = upstream.read()
                        self.send_header("Content-Length", str(len(result)))
                        self.end_headers()
                        self.wfile.write(result)

        except (BrokenPipeError, ConnectionResetError):
            print("[DREAD gateway] Client disconnected during response", flush=True)
            self.close_connection = True
        except Exception as error:
            print(f"[DREAD gateway] Request failed: {error}", flush=True)
            try:
                if not self.wfile.closed:
                    self.send_json(503, {"error": {"message": str(error)}})
            except (BrokenPipeError, ConnectionResetError, OSError):
                self.close_connection = True

    def log_message(self, fmt, *args):
        print("[DREAD gateway]", fmt % args, flush=True)


if __name__ == "__main__":
    ThreadingHTTPServer(("127.0.0.1", 11500), Handler).serve_forever()
