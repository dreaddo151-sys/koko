# PROJECT DREAD UNIVERSAL architecture

## Runtime boundaries
- Rust API/control plane: HTTP facade, persistence, usage ledger and job metadata.
- llama.cpp: sole LLM inference backend via OpenAI-compatible API (default `127.0.0.1:11501`).
- ComfyUI: image/video workflow API at `127.0.0.1:8188`.
- Python GPU Supervisor: sole GPU scheduling/allocation authority. Rust must never reset, kill or reconfigure GPUs.
- Existing Python Hub: compatibility UI during staged migration.

## Safety
Loopback bind by default. No arbitrary shell endpoint. Job rows are metadata only; no worker is enabled. Health probes are read-only. pstack execution remains disabled pending reviewed allowlists and sandboxing.

## API
- `GET /health`, `GET /api/services`, `GET /api/models`
- `POST /api/chat`: non-stream JSON or SSE relay to llama.cpp.
- `GET /api/usage`: aggregate SQLite token ledger.
- `GET /api/comfyui/status`
- `POST/GET /api/jobs`, `POST /api/jobs/{id}/cancel`: durable metadata lifecycle only.

## SQLite
WAL database at `data/dread.sqlite3` by default. Tables: conversations, messages, token_usage, jobs. Non-stream request/response persistence and usage aggregation are transactional. Streamed content/usage is not yet captured. No migration framework or backup/restore automation yet.

## Token quality and "booster"
Current token accounting records actual `prompt_tokens`, `completion_tokens`, and `total_tokens` reported by llama.cpp for non-stream responses. It does not fabricate tokenizer estimates or claim to increase model context. Prompt enhancement, context compression, model-aware token budgeting and tokenizer parity are future features and must be measured against baseline quality/latency.

## Explicitly incomplete
Auth/TLS, streaming persistence, conversation retrieval, job execution/recovery, pstack sandbox adapter, prompt booster, ComfyUI render lifecycle, installer/rollback and physical dual-GPU acceptance remain release gates.
