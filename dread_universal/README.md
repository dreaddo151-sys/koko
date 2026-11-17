# PROJECT DREAD UNIVERSAL

Local-first Rust control plane for model discovery, task approval, local agent endpoint discovery, capability management and GPU inference through a single configured llama.cpp gateway.

## Current implementation

- Rust workspace with API, service discovery, agent contracts, GPU gateway client, bounded task orchestration, Pstack context loader and portable host discovery.
- Host profile: OS, architecture, CPU threads, NVIDIA/AMD/Apple GPU metadata when supported diagnostic utilities are installed, and local `llama-server` executable discovery.
- Local model discovery through the configured llama.cpp OpenAI-compatible gateway. Ollama is not an active backend.
- Read-only local agent health discovery for explicitly configured endpoints plus allowlisted OpenHands and LangGraph loopback endpoints.
- Pstack skills and Markdown agent prompt profiles can be discovered, imported disabled, previewed, explicitly approved, disabled and selected as bounded model context.
- Imported Markdown is untrusted reference material. It never executes scripts, hooks, MCP tools or shell commands.
- GPU inference remains behind the authenticated Python GPU Supervisor bridge. Rust does not acquire GPU leases or launch a second inference server.
- Native Linux, Windows and macOS Rust builds are configured in CI. The current GPU Supervisor implementation is the Linux workstation adapter; cross-platform GPU runtime adapters and installers are not yet complete.

## Requirements

- Rust stable with Rust 2024 Edition support.
- Node.js 22+ and npm for the web console.
- A local llama.cpp OpenAI-compatible gateway.
- For GPU tasks, the authenticated DREAD Python GPU Supervisor bridge.
- Optional: `nvidia-smi`, `rocm-smi`, `xpu-smi` or macOS `system_profiler` for richer host discovery.

## Build and verify

From this directory:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --release --locked
```

Run the API:

```bash
export DREAD_API_TOKEN="$(openssl rand -hex 32)"
export DREAD_OPERATOR_ID="local-operator"
cargo run -p dread-api
```

Run the web console in another terminal:

```bash
cd web
npm install
npm run dev
```

Open `http://127.0.0.1:5173` and enter the API token. Vite proxies API requests to `127.0.0.1:8765`. The token stays in page memory and is not persisted to browser storage.

## Runtime configuration

| Variable | Default | Purpose |
|---|---|---|
| `DREAD_BIND` | `127.0.0.1:8765` | API listen address; non-loopback binding is rejected |
| `DREAD_DB_PATH` | `data/dread_registry.sqlite3` | SQLite database |
| `DREAD_API_TOKEN` | required | Bearer token for protected API routes; at least 32 characters |
| `DREAD_OPERATOR_ID` | `local-operator` | Server-side identity recorded for approvals |
| `DREAD_GPU_GATEWAY_URL` | `http://127.0.0.1:11500` | Local llama.cpp gateway; must be loopback HTTP with an explicit port |
| `DREAD_GPU_SUPERVISOR_URL` | unset | Optional authenticated Supervisor bridge URL |
| `DREAD_GPU_SUPERVISOR_TOKEN` | unset | Supervisor bridge token; configure together with its URL |
| `DREAD_SERVICES_CONFIG` | `config/services.toml` | Local service discovery |
| `DREAD_AGENTS_CONFIG` | `config/agents.toml` | Explicit agent health endpoints |
| `DREAD_SKILLS_ROOT` | local catalog candidates | Pstack root containing `skills/` and optionally `agents/` |
| `DREAD_PSTACK_VERSION` | `0.15.43` | Imported catalog version metadata |
| `DREAD_PSTACK_REVISION` | `unverified-local-copy` | Source revision metadata; set to a verified commit when available |
| `DREAD_LLM_SERVER` | PATH discovery | Optional explicit path to `llama-server` for host discovery |

Never commit API or Supervisor tokens. Store them outside the repository with restrictive permissions. If the API token is missing, protected routes fail closed. `/health` remains unauthenticated for liveness checks. Bearer authentication does not authorize LAN/public exposure.

## API surface

| Method | Path | Purpose |
|---|---|---|
| GET | `/health` | API health/version |
| GET | `/api/system/profile` | Cached read-only host and runtime discovery |
| GET | `/api/services` | Local service probes, including configured llama.cpp gateway |
| GET | `/api/agents` | Configured agents plus allowlisted OpenHands/LangGraph health probes |
| GET/POST | `/api/capabilities` | List or register capability metadata; new imports remain disabled |
| GET | `/api/capabilities/discover` | Discover local Pstack skills and agent profiles |
| POST | `/api/capabilities/import` | Import discovered profiles as disabled records |
| GET | `/api/capabilities/{id}/preview` | Preview exact imported Markdown source |
| POST | `/api/capabilities/{id}/enable` | Explicit operator approval of a reviewed context profile |
| POST | `/api/capabilities/{id}/disable` | Disable a context profile |
| GET/POST | `/api/tasks` | List tasks / create a pending-approval task |
| POST | `/api/tasks/{id}/approve` | Record operator approval; does not execute |
| POST | `/api/tasks/{id}/run` | Queue an approved supported task |
| GET | `/api/gpu/models` | List models advertised by the configured llama.cpp gateway |
| POST | `/api/gpu/v1/chat/completions` | Supervisor-managed direct inference; no skill context |

## Agent discovery boundaries

DREAD probes only explicit loopback health URLs and a small known-endpoint allowlist:
- OpenHands Agent Server: `http://127.0.0.1:8000/health`
- LangGraph Agent Server: `http://127.0.0.1:2024/ok`

The probes use GET, short timeouts and no redirects. They do not scan arbitrary ports, contact LAN hosts, launch agents or prove that a task lifecycle adapter is installed. A healthy endpoint still needs a reviewed `AgentAdapter` implementation before it can execute tasks.

A Pstack `agents/<name>.md` file is a prompt profile, not a live agent process. It can be imported as untrusted context; it is not represented as a remote AgentAdapter.

## Pstack extension workflow

Set `DREAD_SKILLS_ROOT` to the local Pstack root. DREAD discovers:
- `skills/<name>/SKILL.md`
- `agents/<name>.md`

The console provides one-click discovery/import, source preview, explicit approval and disable. Imports are disabled by default. Content is bounded, local paths are checked, and the stored FNV-1a fingerprint is rechecked before approval/use. This fingerprint is a change detector, not a cryptographic signature.

Enabled profiles can be selected for an approved `gpu_chat` task. Their Markdown is injected as untrusted user reference context beneath a separate DREAD policy message. Direct GPU inference cannot attach profiles. No imported scripts, hooks, shell commands, MCP tools or other code are executed.

## GPU ownership contract

The Python GPU Supervisor remains the sole GPU lease and process-lifecycle authority for the current workstation. Rust uses the configured gateway for model discovery and the authenticated bridge for inference. It must not start another llama-server while the Supervisor owns GPU execution. ComfyUI remains independently managed and must not be stopped by DREAD installation or update operations.

## Portable releases

- `.github/workflows/dread-universal-ci.yml` runs formatting, Clippy, tests and release builds on Linux, Windows and macOS, plus the web build.
- `.github/workflows/dread-universal-release.yml` builds native binaries and web assets for version tags and publishes SHA-256 checksums.

A successful native compile does not mean GPU inference is supported on that OS. Windows/macOS GPU Supervisor adapters, signed installers, automatic dependency installation, transactional upgrades and rollback remain future acceptance gates.
