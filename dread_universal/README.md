# PROJECT DREAD UNIVERSAL — Rust control plane

Actively implemented local-first control plane for the DREAD workstation. This repository is not yet the complete autonomous engineering platform; hardware acceptance and execution adapters remain gated.

## Runtime boundaries
- **Inference:** llama.cpp only, OpenAI-compatible API at `127.0.0.1:11501` by default. Ollama is intentionally unsupported.
- **Image/video:** existing ComfyUI at `127.0.0.1:8188`.
- **GPU ownership:** existing Python GPU Supervisor remains authoritative. Rust health checks never kill, reset or reconfigure GPU processes.
- **Compatibility:** existing Python Hub stays available during staged migration.

## Build and run
Requires stable Rust (edition 2024), Cargo and network access for dependency resolution.

```bash
cargo generate-lockfile
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --release --locked
cp .env.example .env
set -a; source .env; set +a
cargo run -p dread-api --bin dread-universal
```

API binds to loopback `127.0.0.1:8765`. Keep it on loopback until authentication and TLS are implemented.

## API
- `GET /health`: API health and configured llama.cpp endpoint.
- `GET /api/services`: read-only llama.cpp and ComfyUI checks.
- `GET /api/models`: llama.cpp model list.
- `POST /api/chat`: OpenAI-compatible chat proxy; `stream:true` relays SSE chunks. Non-stream responses are persisted.
- `GET /api/usage`: persisted prompt/completion/total token aggregates.
- `POST /api/tokenize`: exact token count from llama.cpp `/tokenize`.
- `POST /api/prompt/boost`: lossless whitespace normalization with before/after llama.cpp token counts and optional budget check.
- `GET /api/conversations/{id}`: retrieve persisted messages.
- `GET /api/comfyui/status`: read-only ComfyUI stats.
- `POST /api/jobs`: create durable queued job metadata (does not execute it).
- `GET /api/jobs`: list latest 100 jobs.
- `POST /api/jobs/{id}/cancel`: cancel queued metadata only.

Example chat:
```json
{"messages":[{"role":"user","content":"Hello"}],"stream":false}
```
Streaming:
```json
{"messages":[{"role":"user","content":"Hello"}],"stream":true}
```
A response includes `x-dread-conversation-id`; pass that as `conversation_id` on later non-stream requests to group history.

## Persistence and token accounting
SQLite defaults to `data/dread.sqlite3` (override with `DREAD_DATABASE`). WAL mode is enabled. Non-stream chat exchanges and llama.cpp-reported usage are committed transactionally. Streaming is a byte-for-byte SSE relay today; streamed conversation capture and final usage-frame accounting are not yet implemented, so streamed requests do not appear in the usage ledger. Token counting delegates to the configured llama.cpp tokenizer endpoint; no guessed token estimates are returned. The current prompt booster is deliberately lossless whitespace normalization, not semantic summarization.

## Job safety
Job endpoints currently persist metadata and allow cancellation only while queued. They do not invoke shell commands, pstack, ComfyUI workflows or GPU operations. A worker, bounded retry policy, execution sandbox and durable recovery are still required before jobs can run.

## Environment
See `.env.example` and `config/services.toml`. Defaults expect llama.cpp 11501, GPU gateway 11500, ComfyUI 8188 and legacy Hub 8080. Endpoint reachability must be verified on the workstation; CI cannot prove those services are running.

## Remaining release gates
Authentication/authorization, streaming persistence and usage extraction, job workers and recovery, semantic prompt compression/quality evaluation, pstack adapter review/sandbox, ComfyUI render lifecycle, native install/upgrade/rollback, and real dual-3090 acceptance remain incomplete. Do not represent this branch as a finished product.
