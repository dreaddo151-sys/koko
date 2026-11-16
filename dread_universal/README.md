# PROJECT DREAD UNIVERSAL — Rust API Foundation 0.4.0

A loopback-first Rust service for local DREAD service discovery and a persistent SQLite component registry. This crate is additive: it does not replace or control the existing Python DREAD Hub, ComfyUI, Ollama, or GPU Supervisor.

## Requirements

- Rust stable with Edition 2024 support
- Cargo
- Ubuntu/Linux is the primary target

## Build and verify

Run commands from this directory:

```bash
cargo generate-lockfile
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
```

Run the API:

```bash
cargo run -p dread-api
```

The executable is named `dread-universal`.

## Runtime configuration

| Variable | Default | Purpose |
|---|---|---|
| `DREAD_BIND` | `127.0.0.1:8765` | API listen address |
| `DREAD_DB_PATH` | `data/dread_registry.sqlite3` | SQLite registry database |
| `DREAD_SERVICES_CONFIG` | `config/services.toml` | Local service-discovery configuration |
| `RUST_LOG` | `info` | Tracing filter |

The API intentionally binds to loopback by default. There is no authentication in this foundation release. Do not bind to a LAN/public interface or expose it through a reverse proxy without adding authentication and authorization first.

The sample discovery configuration checks:
- DREAD Hub at `127.0.0.1:8080`
- ComfyUI at `127.0.0.1:8188`
- Ollama at `127.0.0.1:11434`

Discovery is read-only: it sends HTTP GET requests and reports ONLINE, REACHABLE, DEGRADED, OFFLINE, or UNKNOWN. It does not restart, stop, or reconfigure services.

## HTTP API

All responses are JSON except the successful DELETE response, which is HTTP 204.

| Method | Path | Purpose |
|---|---|---|
| GET | `/health` | API health and version |
| GET | `/api/services` | Probe configured local services |
| GET | `/api/registry` | List registry entries |
| POST | `/api/registry` | Create a registry entry |
| GET | `/api/registry/{id}` | Read one entry |
| DELETE | `/api/registry/{id}` | Delete one entry |

Example registry creation:

```bash
curl -sS -X POST http://127.0.0.1:8765/api/registry \
  -H 'content-type: application/json' \
  -d '{"name":"ComfyUI","kind":"service","endpoint":"http://127.0.0.1:8188","metadata":{"role":"media-runtime"}}'
```

List entries:

```bash
curl -sS http://127.0.0.1:8765/api/registry
```

## Data and safety

- SQLite uses WAL mode and creates its parent directory automatically.
- Back up the database file before manual maintenance or migration.
- The registry stores metadata as JSON.
- The service discovery configuration currently accepts loopback HTTP endpoints only.
- The API does not invoke shell commands, launch models, schedule GPU work, or modify existing services.

## Scope and remaining work

This is a working API foundation, not the complete autonomous DREAD platform. Production authentication/authorization, audit logging, migrations, agent adapters, plugin import, repair orchestration, GPU-supervisor integration, native installers, and full end-to-end hardware validation remain separate milestones. The GPU Supervisor must remain the sole GPU coordination authority.

See [docs/VERIFICATION.md](docs/VERIFICATION.md) for release gates and operational constraints.
