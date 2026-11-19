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
| GET | `/api/agent-adapters` | Authenticated inventory of explicitly configured provider adapters |
| GET | `/api/capabilities/discover` | Discover local Pstack skills/profiles |
| POST | `/api/capabilities/import` | Register discovered profiles disabled from the configured local tree |
| POST | `/api/capabilities/import-batch` | Atomically import up to 128 disabled capability records; exact matches are idempotent, conflicts roll back the entire batch |
| GET | `/api/capabilities/{id}/preview` | Preview source and verify its SHA-256 fingerprint |
| POST | `/api/capabilities/{id}/enable` | Explicit operator approval; records the previous state, operator and reason in audit history |
| POST | `/api/capabilities/{id}/disable` | Revoke an enabled profile's approval with an operator reason and audit event |
| GET | `/api/capabilities/{id}/history` | Read retained enable/disable/removal audit events, including the actor and reason |
| POST | `/api/missions` | Validate and persist a dependency-aware mission plan (planning only; no auto-execution) |
| GET | `/api/missions/{id}` | Retrieve a persisted mission plan and per-node result/error/attempt/timing metadata |
| GET/POST | `/api/tasks` | List/create tasks |
| POST | `/api/tasks/{id}/approve` | Record approval; does not itself execute |
| POST | `/api/tasks/{id}/run` | Queue supported approved task |
| GET | `/api/gpu/models` | Models advertised by llama.cpp gateway |
| POST | `/api/gpu/v1/chat/completions` | Supervisor-managed inference |

## Agent and extension boundaries

The `/api/agents` endpoint remains a read-only probe of explicitly configured health URLs. It does not execute agents. The separate authenticated `/api/agent-adapters` endpoint reports the provider adapters actually constructed from private environment configuration. Adapter registration is opt-in and fail-closed. Leaving a provider's variables unset keeps it disabled; setting only part of a provider's required variables fails API startup rather than creating a partially configured adapter.

### Registered provider task adapters

The API now registers three concrete `AgentAdapter` implementations when their complete configuration and isolation attestations are supplied.

- **Hermes.** The Runs API adapter supports authenticated idempotent run creation, status/result retrieval, SSE event replay and cooperative stop. Its session/run identity is derived from the DREAD task ID. A supported backend and one workspace per task must be explicitly attested. The stock Hermes Docker backend is rejected because Hermes documents one persistent container shared across the entire Hermes process; changing `container_persistent` does not create one container per DREAD task.
- **DeerFlow.** The thread-scoped Gateway adapter supports PAT-authenticated creation, status, event replay, cancellation and results. The provider class must be AIO or BoxLite with one isolated thread/workspace per DREAD task. LocalSandboxProvider and non-empty remote provisioner URLs are rejected. The current DeerFlow configuration guide says AIO's compatibility default has open egress; use `isolated` or a reviewed `allowlist` network mode and review all mounts. BoxLite scopes a warm-pool VM by `(user_id, thread_id)`; DREAD derives a unique thread ID from each task, but same-thread reuse is expected.
- **OpenHands.** The Agent Server adapter uses the session API key, task-scoped conversation ID, status/event pagination, pause, result extraction and DREAD-mediated action confirmation. Before advertising or creating a new conversation it requires authenticated `GET /server_info` to report `execution_runtime: "docker"` and to list `terminal`, `file_editor` and `task_tracker` in `usable_tools`. It explicitly requests `DockerExecutionWorkspace` at `/workspace` with host worktree setup disabled; it never falls back to `LocalWorkspace`.

All adapters validate explicit-port loopback HTTP endpoints and task-scoped identity/idempotency. These are registered task adapters, not arbitrary automatic discovery. The DREAD API does not start a provider just because its health endpoint responds.

### Task execution and recovery

A task remains pending until an authenticated operator approves it and calls `POST /api/tasks/{id}/run`. The worker dispatches only to the selected, enabled capability and configured provider adapter. It persists provider identity, provider events, the event cursor, final result and error in SQLite. Task event publication follows the corresponding database commit.

On API restart, recovery resumes only tasks that still have recorded approval, an enabled capability mapped to the same provider and an enabled adapter. Provider task identities are deterministic/idempotent so a worker can reattach instead of creating duplicate work. Event polling resumes from the persisted cursor. Tasks that no longer meet those checks are failed and their mission is blocked rather than replayed blindly. This is bounded queue recovery, not resource-aware scheduling or a general retry policy.

### Provider configuration and safety

See `.env.example` for the optional `DREAD_HERMES_*`, `DREAD_DEERFLOW_*` and `DREAD_OPENHANDS_*` groups. Do not add tokens to Git. All three providers now require a separate `DREAD_*_SANDBOX_POLICY_VERIFIED=true` operator attestation in addition to per-task isolation/secret-store configuration. These flags are not runtime proofs that the provider's sandbox, workspace, network policy, mounts or secret store are secure. OpenHands must advertise `execution_runtime: "docker"` from authenticated `/server_info`; DREAD also requires `usable_tools` to include exactly the tools it requests (`terminal`, `file_editor`, `task_tracker`). New conversations explicitly request `DockerExecutionWorkspace` at `/workspace` and set `worktree=false`; old/unknown runtime metadata fails closed rather than using `LocalWorkspace`. Configure `OH_CONVERSATION_RUNTIME=docker`, or `OH_CONVERSATION_RUNTIME_SELECTABLE=1` when Docker is selectable alongside local runtime. Configure `OH_SECRET_KEY` on the Agent Server itself and keep it stable; `DREAD_OPENHANDS_SECRET_STORE_CONFIGURED=true` only records the operator's attestation. The adapter sends `AlwaysConfirm`; this is an approval gate, not a command allowlist: the `terminal` tool can still execute arbitrary commands inside the selected workspace. Do not enable live tasks until the actual per-conversation container, mounts, network policy, and tool/command boundary have been independently verified on the workstation. Cancellation uses the documented pause operation, which is resumable on the provider side and must not be mistaken for permanent conversation deletion.

The repository's reviewed Pstack snapshot is 0.15.43 at commit `2f87c854fe3e2fcda44ca86b46ede35aad3c46b7`, subtree `71d8e1d1d718e2149dc43b2535618064f29cba6b`, MIT; 58 skills and 2 prompt profiles. The runtime importer independently hard-pins SHA-256 for the catalog, import marker, upstream manifest and LICENSE, in addition to the repository/commit/tree/version/license fields. It checks each source file hash and rejects path traversal and symlink escapes. The batch API is transactional and accepts only disabled metadata; an exact match is skipped, but any differing existing record rolls back the whole batch. This is stronger than trusting a mutable marker and catalog together, but it is not a publisher signature. Runtime local discovery/import remains explicitly labelled `unverified_local_copy`, omits an upstream URL and invented version/revision, and treats Markdown only as untrusted context. Signed publisher provenance and transactional extension update/remove are still required for production extension integrity.

The read-only host profile also enumerates persistent and network-mounted filesystems using GNU `df` with bounded execution. It reports source, filesystem type, mount point, total, used and available MiB, while excluding common pseudo-filesystems (for example `tmpfs`, `proc`, `sysfs` and `overlay`). The overview console renders the discovered mounts. This is mount-level capacity discovery, not physical-disk topology, SMART health or RAID status.

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

Both installers create a per-user API token file at `~/.config/dread-universal/dread.env`; keep it private and enter its token in the console. Existing user configuration and database files are preserved. The portable installer backs up the previous application directory, credential file and systemd unit, records the prior enabled/active service state, and performs a health check before accepting the update. A failed upgrade restores the previous application, credentials, unit and service state; a failed first install removes the newly created unit and application tree. Neither installer stops or replaces the legacy Python Master Hub, ComfyUI or the Python GPU Supervisor.

The packaged API listens only on `127.0.0.1:8765`. GPU inference remains disabled until the already-running authenticated Supervisor bridge URL and token are configured in the private environment file. Do not copy the bridge token into Git or browser storage.

The `.deb` and tarball are packaging candidates, not yet a release acceptance claim: clean-machine installation, upgrade/rollback, reboot persistence, and workstation service-preservation tests remain required.


## Preservation baseline before installation

Before installing or upgrading on the workstation, run:

```bash
bash scripts/capture-workstation-baseline.sh
```

The read-only baseline records Ubuntu/kernel identity, user service states, the legacy Hub/ComfyUI/Supervisor/API port status, NVIDIA driver and current-versus-maximum PCIe link data, and workspace filesystem capacity. It saves a private report under `~/.local/share/dread-universal/baselines/`. It does not start, stop or restart any service and does not dump HTTP response bodies or authentication tokens. Review and retain the report before any live migration.


## Mission planning API

Create a bounded, dependency-validated mission plan with:

```http
POST /api/missions
Content-Type: application/json
Authorization: Bearer <DREAD_API_TOKEN>
```

The request body contains `mission_id` and `nodes`; each node has `id`, `task_type`, optional `depends_on`, and optional numeric `priority`. Plans are limited to 128 nodes, reject missing dependencies and cycles, and are persisted in SQLite. Retrieve a plan with `GET /api/missions/{id}`.

**Safety boundary:** creating a plan only stores its validated graph and deterministic execution order. Explicitly instantiate the plan to create disabled/pending-approval tasks; approve each task with an operator reason. An operator starts the mission by running an approved root task. At start, the scheduler dispatches all other approved independent roots in the graph's deterministic priority order; after each successful completion, it dispatches newly ready approved dependents. Nodes awaiting approval remain held; a failed or cancelled node blocks further dispatch. The shared execution gate bounds concurrency. `GET /api/missions/{id}` includes per-node results, errors, attempts and timing. Priority/resource-aware scheduling, retries, mission-level aggregation policy and recovery remain unfinished acceptance gates. Provider adapters are not automatically invoked by mission planning.
