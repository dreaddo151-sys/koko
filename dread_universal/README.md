# PROJECT DREAD UNIVERSAL — Expanded Foundation 0.3.0

This source tree is the foundation release candidate: Rust API, service discovery, SQLite component registry, regression tests, and GitHub CI gates.

## Build and test
Requires Ubuntu/Linux and Rust/Cargo (edition 2024 compatible).

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --locked
cargo build --workspace --release --locked
```

Run from this directory:
```bash
./dread-universal
```

The API defaults to loopback at `127.0.0.1:8765`; `DREAD_BIND` can override it. No authentication is included. Do not expose it to untrusted networks.

Endpoints: `GET /health`, `GET /api/services`, `GET/POST /api/registry`, `GET/DELETE /api/registry/{id}`.

Existing DREAD Hub, ComfyUI, Ollama, and GPU Supervisor are not stopped or modified. The GPU Supervisor remains the sole GPU coordination authority.

The full autonomous orchestrator, production agent adapters, plugin importer, repair engine, and native installers are not yet implemented. See `docs/VERIFICATION.md` for release gates.
