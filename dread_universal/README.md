# PROJECT DREAD UNIVERSAL — Rust foundation

This is an actively implemented Rust control-plane foundation for the local-first DREAD workstation. It is not yet the complete autonomous engineering platform; see [Architecture](docs/ARCHITECTURE.md) and [Verification](docs/VERIFICATION.md).

## Backend and service boundaries

- **LLM inference: llama.cpp only**, using its OpenAI-compatible API. Default: `http://127.0.0.1:11501`. Ollama is intentionally unsupported.
- **Image/video:** ComfyUI at `http://127.0.0.1:8188`.
- **GPU ownership:** existing Python GPU Supervisor / gateway remains authoritative. Rust health checks are read-only and never kill, reset, or reconfigure GPU processes.
- **Compatibility:** existing Python Hub remains available during staged migration.

## Build

Requires stable Rust (edition 2024), Cargo and a working network for first dependency resolution.

```bash
cargo generate-lockfile
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --release --locked
```

Run from this directory:

```bash
cp .env.example .env
set -a; source .env; set +a
cargo run -p dread-api --bin dread-universal
```

API defaults to loopback at `127.0.0.1:8765`. Do not expose it to untrusted networks; authentication and TLS are not yet implemented.

## API v0.4

- `GET /health`: API health and llama.cpp backend identity.
- `GET /api/services`: read-only llama.cpp and ComfyUI reachability.
- `GET /api/models`: proxy llama.cpp `/v1/models`.
- `POST /api/chat`: proxy chat completions to llama.cpp (non-streaming).
- `GET /api/comfyui/status`: read-only ComfyUI system stats.

## Configuration

See `.env.example` and `config/services.toml`. The expected workstation endpoints are llama.cpp API `11501`, GPU gateway `11500`, ComfyUI `8188`, and legacy Hub `8080`.

## Current scope

Implemented: Rust workspace, service discovery library, llama.cpp-only chat/model proxy, ComfyUI status proxy, loopback API defaults, pstack import inventory (disabled until reviewed), and strict CI gates.

Not yet implemented: durable SQLite registry, authentication, streaming/SSE, persistent chat history, token accounting, prompt enhancement, render/job queue and recovery, full pstack adapters, native install/upgrade/rollback, and real dual-GPU hardware acceptance. These are release gates, not claimed features.
