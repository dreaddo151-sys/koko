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
| `DREAD_API_TOKEN` | required | Bearer token for all `/api/*` routes; at least 32 characters |
| `DREAD_OPERATOR_ID` | `local-operator` | Server-side identity recorded for approvals |
| `RUST_LOG` | `info` | Tracing filter |

Generate a token locally and start the API with:

```bash
export DREAD_API_TOKEN="$(openssl rand -hex 32)"
export DREAD_OPERATOR_ID="local-operator"
cargo run -p dread-api
```

Never commit the token to Git. Every `/api/*` route requires `Authorization: Bearer $DREAD_API_TOKEN`. If the token is missing, API routes fail closed with HTTP 503; an invalid or missing request token returns HTTP 401. `/health` remains unauthenticated for basic liveness checks. The API still rejects non-loopback bind addresses; bearer authentication does not authorize LAN/public exposure. JSON request bodies are limited to 32 MiB.

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
| POST | `/api/tasks/{id}/approve` | Record approval using server-configured operator identity; approval alone does not execute |
| POST | `/api/tasks/{id}/run` | Queue the approved built-in read-only service-discovery task |
| GET | `/api/tasks/{id}/result` | Read persisted execution result |
| POST | `/api/tasks/{id}/cancel` | Cancel pending/queued/running task and signal active runner |
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

Creating a task creates a durable `pending_approval` record. Approval accepts a reason and records the server-configured `DREAD_OPERATOR_ID`; caller-supplied approver names are ignored. Approval alone does not start execution. The explicit `POST /api/tasks/{id}/run` endpoint currently accepts only approved tasks with no capability ID and task type `service_discovery` or `gpu_chat`. Service discovery performs configured loopback-only HTTP health probes. `gpu_chat` accepts bounded text-only messages, checks the requested model against the local gateway's advertised model list, forces non-streaming inference through `127.0.0.1:11500`, and stores a bounded JSON response. Both runners use the bounded execution gate (2 concurrent, 8 admitted by default, one attempt, 120-second timeout) and persist results for `GET /api/tasks/{id}/result`. Running tasks can be cancelled through the cancellation watch channel. The GPU runner never acquires GPU leases or contacts upstream model servers directly. No shell commands, Pstack skills, plugins, imported agents or arbitrary capabilities are executed. Unsupported task types are rejected. The API uses a configured Bearer token and records the server-configured operator identity; it remains loopback-only. Startup recovery resumes only queued, approved, capability-free `service_discovery` tasks. GPU inference is not automatically replayed after a restart; queued `gpu_chat` tasks are failed closed to avoid duplicate compute. Tasks found in `running` state after a crash are marked failed with an interruption result and are never replayed. The WebSocket stream emits committed task events, replays persisted history using `after_sequence`, bounds replay pages and recovers missed notifications from SQLite when a subscriber falls behind. An authenticated network-level WebSocket test verifies the 101 handshake, replay after a sequence cursor, live committed-event delivery and reconnect replay. CI run `44025427057` passed the WebSocket test.

## Pstack catalog import

The catalog pins Pstack `0.15.43` (commit `2f87c854fe3e2fcda44ca86b46ede35aad3c46b7`, subtree `71d8e1d1d718e2149dc43b2535618064f29cba6b`, MIT): 58 skills and 2 agents. The active pinned source is `integrations/pstack/upstream/` at version `0.15.43`. The separate `integrations/pstack-upstream/` copy is an older retained `0.15.42` snapshot and is not used by the importer.

Canonical-source CI run `44025427057` verified the committed Pstack pin/tree and all 60 provenance hashes, passed 11 importer unit tests (including Bearer-header validation), imported 58 skills and 2 agents through the authenticated Rust API, read back all 60 as disabled and verified idempotent re-import (0 created, 60 already present). First run `scripts/import-pstack.sh` to fetch and verify the exact catalog commit/tree. Then run `python3 scripts/import_pstack_capabilities.py` for validation/dry-run. Only add `--apply` when the loopback Rust API is running, metadata registration is intended and `DREAD_API_TOKEN` is configured. The live import was tested only against an isolated temporary CI API; it has not been applied to the operator's workstation.

## GPU ownership and service safety

The Python GPU Supervisor remains the only GPU ownership, VRAM lease and process-lifecycle authority. Rust may call only the local gateway at `127.0.0.1:11500`; it must not call the upstream model server directly or acquire a second GPU lock. Keep the Python Hub and ComfyUI running while Rust development continues.

## Run the operator console

The React/TypeScript console is a separate loopback-only Vite app. Start the Rust API first, then run:

```bash
cd dread_universal/web
npm install
npm run dev
```

Open `http://127.0.0.1:5173` and enter `DREAD_API_TOKEN`. The token is held only in page memory, not browser storage. The UI polls authenticated HTTP endpoints for task, agent and model state; it does not place the bearer token in a WebSocket URL. Run `npm run build` for a production bundle or `npm run preview` to serve that bundle on loopback. The frontend is not yet embedded in the Rust API server.

## Scope

This is a tested Rust foundation with a canonical tracked source branch, not the complete Universal Hub. WebSocket task-event streaming, read-only agent discovery, disabled-by-default capability metadata, authenticated live Pstack import and idempotency, and approval-gated local GPU chat through the gateway have CI evidence. The bounded orchestration kernel is connected to the durable task API for two reviewed built-in runners. Startup recovery and the authenticated WebSocket handshake/reconnect path are CI verified. Remaining milestones include dedicated provider/agent adapters, React operator UI, legacy Hub parity, Supervisor/resource integration, installer/upgrade/rollback and live dual-RTX-3090 acceptance.

See [project status](docs/PROJECT_STATUS.md) and [verification gates](docs/VERIFICATION.md).

## Bounded orchestration kernel

The `dread-orchestrator` crate supplies a bounded execution gate for a caller-selected runner: max concurrent operations, max admitted work, retry count, per-attempt timeout and watch-channel cancellation. It is connected to the durable task API for reviewed service-discovery and GPU-chat runners. Startup recovery resumes only queued, approved, capability-free service-discovery tasks; queued GPU inference is failed closed to avoid duplicate compute after restart, and interrupted running tasks are marked failed rather than replayed. It has no priority scheduling, provider/agent adapters, process launching or GPU lease ownership. The Python GPU Supervisor remains the only GPU owner. Approval does not execute a task; the separate run endpoint is required and rejects unsupported task types.
