# PROJECT DREAD UNIVERSAL — Rust API Foundation 0.4.0

This is an additive, loopback-first Rust service developed alongside the existing Python DREAD Hub. It does not replace or control ComfyUI, Ollama or the Python GPU Supervisor.

## Requirements and build

- Rust stable with Rust 2024 Edition support
- Cargo
- Ubuntu/Linux is the primary target

Run from this directory:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --release --locked
cargo run -p dread-api
```

## Runtime configuration

| Variable | Default | Purpose |
|---|---|---|
| `DREAD_BIND` | `127.0.0.1:8765` | API listen address |
| `DREAD_DB_PATH` | `data/dread_registry.sqlite3` | SQLite database |
| `DREAD_SERVICES_CONFIG` | `config/services.toml` | Local service discovery |
| `DREAD_AGENTS_CONFIG` | `config/agents.toml` | Explicit local agent health probes |
| `RUST_LOG` | `info` | Tracing filter |

The API rejects non-loopback bind addresses because authentication/authorization is not implemented. JSON request bodies are limited to 32 MiB.

## HTTP API

| Method | Path | Purpose |
|---|---|---|
| GET | `/health` | API health/version |
| GET | `/api/services` | Read-only probes of configured local services |
| GET | `/api/agents` | Read-only probes of explicitly configured local agent health endpoints |
| GET/POST | `/api/capabilities` | List/create capability metadata; new records remain disabled |
| GET | `/api/capabilities/{id}` | Read one capability record |
| GET/POST | `/api/tasks` | List tasks / create a task in `pending_approval` |
| GET | `/api/tasks/{id}` | Read task state |
| GET | `/api/tasks/{id}/events` | Read ordered task audit events |
| GET (WebSocket upgrade) | `/api/tasks/{id}/stream?after_sequence=N` | Stream live events and replay persisted history after a sequence cursor |
| POST | `/api/tasks/{id}/approve` | Record approval; does not execute the task |
| POST | `/api/tasks/{id}/cancel` | Record cancellation |
| GET | `/api/gpu/health` | Read GPU gateway health |
| GET | `/api/gpu/models` | List models exposed by the gateway |
| POST | `/api/gpu/v1/{path}` | Forward OpenAI-compatible inference/SSE through the gateway |
| GET/POST/DELETE | `/api/registry[/{id}]` | Component registry operations |

### Agent discovery

`config/agents.toml` is empty by default. Only explicit-port loopback HTTP health URLs are accepted. Probes use GET, a two-second timeout and no redirects. Disabled entries do not make network requests. The endpoint does not scan ports, inspect processes, launch agents or dispatch tasks.

Example entry (keep disabled until the endpoint is verified locally):

```toml
[[agents]]
id = "hermes-local"
name = "Hermes local gateway"
adapter = "hermes"
health_url = "http://127.0.0.1:8642/health"
enabled = false
```

Agent health states distinguish 2xx ONLINE, 3xx REDIRECTED, 4xx REACHABLE, 5xx DEGRADED, connection failure OFFLINE and timeout UNKNOWN.

### Task approval and event-stream boundary

Creating a task only creates a durable `pending_approval` record. Approval records a caller-supplied approver label and reason, and changes the state to `approved`; it does not start a worker, invoke an agent or execute commands. The approver label is not authenticated identity. The WebSocket stream emits committed task events, replays persisted history using `after_sequence`, bounds replay pages and recovers missed notifications from SQLite when a subscriber falls behind. A full browser/client WebSocket handshake and reconnect test remains outstanding. There is no task executor or provider dispatch yet.

## Pstack catalog import

The catalog pins Pstack `0.15.43` (commit `2f87c854fe3e2fcda44ca86b46ede35aad3c46b7`, subtree `71d8e1d1d718e2149dc43b2535618064f29cba6b`, MIT): 58 skills and 2 agents. The bundled `integrations/pstack-upstream` copy is version `0.15.42` and is not accepted as the pinned source.

CI run `44014112205` fetched and verified the pinned upstream commit/tree, passed nine importer unit tests and completed a metadata dry-run. First run `scripts/import-pstack.sh` to fetch and verify the exact catalog commit/tree. Then run `python3 scripts/import_pstack_capabilities.py` for validation/dry-run. Only add `--apply` when the loopback Rust API is running and metadata registration is intended. The live API `--apply` operation remains unverified. Existing entries must be confirmed disabled. This importer records metadata only: it never executes skill contents, agents, hooks or scripts.

## GPU ownership and service safety

The Python GPU Supervisor remains the only GPU ownership, VRAM lease and process-lifecycle authority. Rust may call only the local gateway at `127.0.0.1:11500`; it must not call the upstream model server directly or acquire a second GPU lock. Keep the Python Hub and ComfyUI running while Rust development continues.

## Scope

This is a tested Rust foundation, not the complete Universal Hub. WebSocket task-event streaming, read-only agent discovery, disabled-by-default capability metadata and the Pstack pinned metadata dry-run have CI evidence. The bounded orchestration kernel exists as an isolated crate but is not connected to the durable task API or a provider adapter. Remaining milestones include an end-to-end WebSocket client handshake/reconnect test, live Pstack API `--apply`, authentication, provider-specific adapters, React operator UI, legacy Hub parity, installer/upgrade/rollback and live dual-RTX-3090 acceptance.

See [project status](docs/PROJECT_STATUS.md) and [verification gates](docs/VERIFICATION.md).

## Bounded orchestration kernel

The `dread-orchestrator` crate supplies a bounded execution gate for a caller-selected runner: max concurrent operations, max admitted work, retry count, per-attempt timeout and watch-channel cancellation. It is a foundation, not yet connected to the durable task API or a provider adapter. It has no persistent queue, priority scheduling, process launching or GPU lease ownership. The Python GPU Supervisor remains the only GPU owner. Do not treat approval as execution; the task API still records approval only.
