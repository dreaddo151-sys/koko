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
- `POST /api/tokenize`: exact token count delegated to llama.cpp `/tokenize`.
- `POST /api/prompt/boost`: conservative whitespace cleanup plus tokenizer-backed before/after counts and budget check.
- `POST /api/prompt/optimize`: model-powered rewrite proposal, exact token comparison, optional budget check, and approval gate.
- `GET /api/conversations/{id}`: persisted conversation messages.
- `GET /api/comfyui/status`
- `POST/GET /api/jobs`, `POST /api/jobs/{id}/cancel`: durable metadata lifecycle only.

## SQLite
WAL database at `data/dread.sqlite3` by default. Tables: conversations, messages, token_usage, jobs. Non-stream request/response persistence and usage aggregation are transactional. Streamed content/usage is not yet captured. No migration framework or backup/restore automation yet. Conversation retrieval is capped at 200 most recent messages.

## Token quality and "booster"
Current token accounting records actual `prompt_tokens`, `completion_tokens`, and `total_tokens` reported by llama.cpp for non-stream responses. It does not fabricate tokenizer estimates or claim to increase model context. Exact tokenization is delegated to the configured llama.cpp `/tokenize` endpoint. The basic booster performs conservative whitespace cleanup. The model-powered optimizer proposes a rewrite with preservation instructions and requires user review. It reports exact token counts, but token reduction is not proof of better output. Semantic compression, model-aware context selection and quality/latency benchmarks remain future work.

## Explicitly incomplete
Auth/TLS, streaming persistence, conversation retrieval, job execution/recovery, pstack sandbox adapter, prompt booster, ComfyUI render lifecycle, installer/rollback and physical dual-GPU acceptance remain release gates.
