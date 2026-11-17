# PROJECT DREAD UNIVERSAL architecture

## Runtime boundaries
- Rust API/orchestrator: local control plane, HTTP facade and service health.
- llama.cpp: sole LLM inference backend through its OpenAI-compatible `/v1` API (default `127.0.0.1:11501`).
- ComfyUI: image/video workflow execution API at `127.0.0.1:8188`.
- Python GPU Supervisor / gateway: sole GPU scheduling and allocation authority. Rust must never independently reset, kill, or reconfigure GPUs.
- Existing Python Hub: compatibility UI during staged migration.

## Safety defaults
Loopback bind only. No arbitrary shell execution endpoint. No automatic process killing or service restart. Health probes are read-only. All long-running jobs must later be represented by durable job IDs with bounded retries, cancellation and explicit resource ownership.

## API v0.4
- `GET /health`: API health and configured llama.cpp URL.
- `GET /api/services`: llama.cpp and ComfyUI reachability.
- `GET /api/models`: proxy llama.cpp `/v1/models`.
- `POST /api/chat`: proxy non-streaming chat completions to llama.cpp.
- `GET /api/comfyui/status`: read-only ComfyUI system stats.

## Explicitly not claimed complete
SQLite persistence, authentication, streaming/SSE, job queue, render lifecycle, token-cost ledger, prompt-enhancement agent, pstack execution adapters, install/upgrade/rollback, and end-to-end hardware acceptance remain subsequent release gates. Do not mark these complete until implemented and tested.
