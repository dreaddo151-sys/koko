# PROJECT DREAD UNIVERSAL

**Target:** Ubuntu 24.04 LTS and later Ubuntu releases only.  
**Product:** A local-first autonomous AI engineering Master Hub.  
**Controlling specification:** [docs/MASTER-PLAN.md](docs/MASTER-PLAN.md).  
**Workstation and engineering handoff:** [docs/WORK-LOG.md](docs/WORK-LOG.md).

DREAD is intended to provide one coherent operator environment for autonomous engineering missions, models, agents, skills, tools, GPU inference, ComfyUI image/video workflows, diagnostics, safe repair and transactional upgrades.

## Non-negotiable integration rules

- The existing `dread_web_hub.py` remains the Master Hub while the Rust core is integrated incrementally. Do not replace it with a disconnected second product.
- Preserve ComfyUI, the Python GPU Supervisor, authenticated bridge, model files, databases, configuration and user data.
- llama.cpp is the active inference backend. Ollama is not started or used as the active model server.
- The Python GPU Supervisor remains the only GPU lease and process-lifecycle authority. Rust must not launch a competing llama-server or bypass the authenticated bridge.
- Imported skills and agent prompt profiles start disabled and are untrusted reference content. Discovery/import does not execute code.
- Changes must be backed up, reversible and verified against real postconditions.

## Current Rust foundation

The workspace includes an Axum API, SQLite-backed registry/task state, authentication and approval boundaries, bounded task orchestration, task event streaming, a local llama.cpp gateway client, Supervisor bridge integration, read-only host/agent discovery, Pstack skill/profile import controls and a React/TypeScript console.

This is a foundation, not the completed Universal Hub. Read [docs/WORK-LOG.md](docs/WORK-LOG.md) for dated evidence and known limitations.

## Ubuntu build and test

Requirements: Ubuntu 24.04+, stable Rust with rustfmt/Clippy, Node.js 22+ and npm for the web console.

From `dread_universal/`:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --release --locked
cd web
npm ci
npm run build
```

## Development run

```bash
export DREAD_API_TOKEN="$(openssl rand -hex 32)"
export DREAD_OPERATOR_ID="local-operator"
cargo run -p dread-api
```

In another terminal:

```bash
cd web
npm run dev
```

Open `http://127.0.0.1:5173`; Vite proxies API calls to `127.0.0.1:8765`. The token stays in page memory. The API serves the compiled console from `DREAD_WEB_ROOT` (default `web/dist`) on the same loopback origin; Vite is only needed for development. The static-file handler rejects path traversal and symlinks.

## Runtime configuration

| Variable | Default | Purpose |
|---|---|---|
| `DREAD_BIND` | `127.0.0.1:8765` | API bind address; loopback only |
| `DREAD_DB_PATH` | `data/dread_registry.sqlite3` | SQLite registry |
| `DREAD_API_TOKEN` | required | Bearer token for protected routes |
| `DREAD_OPERATOR_ID` | `local-operator` | Server-side approval identity |
| `DREAD_GPU_GATEWAY_URL` | `http://127.0.0.1:11500` | Local llama.cpp gateway |
| `DREAD_GPU_SUPERVISOR_URL` | unset | Authenticated Supervisor bridge URL |
| `DREAD_GPU_SUPERVISOR_TOKEN` | unset | Supervisor bridge token |
| `DREAD_SERVICES_CONFIG` | `config/services.toml` | Local service discovery |
| `DREAD_AGENTS_CONFIG` | `config/agents.toml` | Explicit agent health endpoints |
| `DREAD_SKILLS_ROOT` | catalog candidates | Pstack source root |
| `DREAD_PSTACK_VERSION` | `0.15.43` | Pstack catalog version |
| `DREAD_PSTACK_REVISION` | unverified local copy | Verified source revision |
| `DREAD_LLM_SERVER` | PATH discovery | Optional llama-server path |

Never commit API or Supervisor tokens. Store them outside the repository with restrictive permissions. Bearer authentication does not authorize LAN/public exposure.

## API surface

| Method | Path | Purpose |
|---|---|---|
| GET | `/health` | API liveness/version |
| GET | `/api/system/profile` | Read-only host discovery |
| GET | `/api/services` | Configured local service health |
| GET | `/api/agents` | Read-only configured/known endpoint probes |
| GET | `/api/capabilities/discover` | Discover local Pstack skills/profiles |
| POST | `/api/capabilities/import` | Register discovered profiles disabled |
| GET | `/api/capabilities/{id}/preview` | Preview source |
| POST | `/api/capabilities/{id}/enable` | Explicit operator approval |
| POST | `/api/capabilities/{id}/disable` | Disable profile |
| GET/POST | `/api/tasks` | List/create tasks |
| POST | `/api/tasks/{id}/approve` | Record approval; does not itself execute |
| POST | `/api/tasks/{id}/run` | Queue supported approved task |
| GET | `/api/gpu/models` | Models advertised by llama.cpp gateway |
| POST | `/api/gpu/v1/chat/completions` | Supervisor-managed inference |

## Agent and extension boundaries

Current OpenHands and LangGraph probes are read-only health checks. A healthy endpoint is not a live task adapter. Hermes, OpenHands and DeerFlow task execution require separately tested AgentAdapter implementations. Pstack `agents/*.md` files are prompt profiles, not running agents.

Pstack pin: 0.15.43, commit `2f87c854fe3e2fcda44ca86b46ede35aad3c46b7`, subtree `71d8e1d1d718e2149dc43b2535618064f29cba6b`, MIT; 58 skills and 2 prompt profiles. Use cryptographic SHA-256 provenance for production extension/update integrity.

## Ubuntu-only CI and release

GitHub Actions targets Ubuntu 24.04 only. The release workflow creates an Ubuntu x86_64 build bundle and SHA-256 manifest. A production `.deb`, systemd-integrated installer, signed updater, atomic activation and rollback remain required before calling the package an installer or updater complete.

## GPU ownership

The Python GPU Supervisor is the sole authority for GPU leases and process lifecycle. Rust discovers models through the local gateway and sends approved inference through the authenticated bridge. Never call the tensor upstream directly from Rust or start a second llama-server. Do not stop ComfyUI as part of DREAD development, installation, update or repair.

## Completion criteria

See [docs/MASTER-PLAN.md](docs/MASTER-PLAN.md) for clean Ubuntu installation, real model/agent discovery and task execution, dual-3090 lease safety, valid image/video artifacts, legacy Hub regression coverage, self-repair/rollback drills and workstation acceptance.


## Ubuntu packaging

After building the API and console:

```bash
cargo build --release --locked --bin dread-universal
cd web
npm ci --ignore-scripts --no-audit --no-fund
npm run build
cd ..
bash scripts/package-ubuntu.sh
```

The packaging script creates an Ubuntu `.deb`, a portable `.tar.gz`, and `SHA256SUMS` under `dist/release/`. It packages the API, production console, default service/agent configuration, bridge helper, user systemd unit and operating documentation.

Portable install (run as the regular desktop user, not with sudo):

```bash
tar -xzf dist/release/dread-universal-ubuntu-*.tar.gz
./bundle/install-user.sh
```

Debian package install:

```bash
sudo dpkg -i dist/release/dread-universal_*.deb
/usr/share/dread-universal/configure-user.sh
```

Both installers create a per-user API token file at `~/.config/dread-universal/dread.env`; keep it private and enter its token in the console. Existing user configuration and database files are preserved. The portable installer backs up the previous application directory and performs a health check before accepting the update; a failed update restores the previous application files. Neither installer stops or replaces the legacy Python Master Hub, ComfyUI or the Python GPU Supervisor.

The packaged API listens only on `127.0.0.1:8765`. GPU inference remains disabled until the already-running authenticated Supervisor bridge URL and token are configured in the private environment file. Do not copy the bridge token into Git or browser storage.

The `.deb` and tarball are packaging candidates, not yet a release acceptance claim: clean-machine installation, upgrade/rollback, reboot persistence, and workstation service-preservation tests remain required.


## Preservation baseline before installation

Before installing or upgrading on the workstation, run:

```bash
bash scripts/capture-workstation-baseline.sh
```

The read-only baseline records Ubuntu/kernel identity, user service states, the legacy Hub/ComfyUI/Supervisor/API port status, NVIDIA driver and current-versus-maximum PCIe link data, and workspace filesystem capacity. It saves a private report under `~/.local/share/dread-universal/baselines/`. It does not start, stop or restart any service and does not dump HTTP response bodies or authentication tokens. Review and retain the report before any live migration.
