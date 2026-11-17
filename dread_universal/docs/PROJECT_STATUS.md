# PROJECT DREAD UNIVERSAL — Engineering Status

**Project version:** 0.4.0  
**Canonical integration branch:** `integration/dread-canonical-source-run81`  
**Working feature branch:** `feature/agent-capability-foundation-2026-11-17`  
**Integration policy:** additive development alongside the existing Python Hub; no merge to `main` or workstation service migration is implied.

## Verified source and CI results

Latest canonical-source workflow run `44077407357` (run #78) passed Pstack pin/provenance, 11 importer tests, Rust formatting, Clippy with warnings denied, 50 Rust tests (including 7 orchestrator tests), optimized release build, React/TypeScript production build, 4 Python GPU Supervisor bridge tests, the single-owner GPU boundary guard, authenticated live Pstack API import/readback/idempotency and source inventory hashing. All 60 Pstack records remained disabled. Browser-level live API acceptance and workstation deployment are still outstanding.

The canonical-source consolidation is committed on `integration/dread-canonical-source-run81`. GitHub Actions run `44019266094` passed against that committed workspace. The subsequent task-kernel integration also passed the full direct-source quality workflow in run `44020353017`: Pstack provenance/dry-run, nine importer tests, Rust formatting, Clippy with warnings denied, 36 Rust tests, and optimized release build. The branch contains `dread_universal/` as a normal tracked workspace, `legacy-reference/`, historical Cargo manifest backups, and `SOURCE_PROVENANCE_RUN81.md`. Draft integration PR: [#3](https://github.com/dreaddo151-sys/koko/pull/3). This closes the earlier external-archive-only source-of-truth gap; it does not make the platform workstation-ready.

The hardened baseline passed formatting, Clippy with warnings denied, workspace tests and an optimized release build in GitHub Actions run `44002596952`.

The canonical-source branch passed the direct-source quality workflow in GitHub Actions run `44023186920` (run #23): Pstack pin/tree/provenance, 11 importer tests, Rust formatting, Clippy with warnings denied, 39 Rust tests including startup queue recovery, and optimized release build. This run includes Bearer authentication, server-derived approval identity and restart-safe queue handling.
- `cargo fmt --all -- --check`: PASS
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: PASS
- `cargo test --workspace --locked`: PASS — 34 Rust tests
- `cargo build --workspace --release --locked`: PASS
- Pstack importer unit tests: PASS — 11 tests
- Pinned upstream Pstack fetch/tree verification and dry-run: PASS — 58 skills and 2 agents, all disabled by default.
- Source packaging, ZIP integrity test and artifact upload: PASS.

The run applied the candidate patches to the exact uploaded baseline archive before running checks. The uploaded artifact is identified by SHA-256 `704ac77f8f17a3aab7f2387f3a58cbfda5934815b90a8aede958200b8c5c57b6`; its inner source ZIP has SHA-256 `22b12427f1f84ae537e1e5dce153912bf6c6c415a400b19be146203482f13509`. The inner ZIP passed integrity validation, contains 4,324 entries, includes the Rust API and orchestrator source, has no nested ZIPs, and preserves 22 historical backup/original files. CI validates source build and unit tests; it does not constitute end-to-end testing against the operator's live workstation services, a browser WebSocket client or the live Rust API importer.

## Implemented

- Axum Rust API foundation with SQLite persistence.
- All `/api/*` routes require a configured Bearer token and the API remains strictly loopback-only. Authentication does not authorize LAN/public exposure; role-based authorization and token rotation remain future work.
- 32 MiB JSON request-body ceiling; large-message exact-value round-trip tests for 5 KB, 10 KB and 20 KB.
- Local service discovery for explicitly configured loopback HTTP endpoints; dedicated Ollama model-tag and ComfyUI system/device metadata parsers are bounded to 128 KiB. Invalid adapter payloads become `DEGRADED`; DREAD Hub remains generic HTTP reachability.
- GPU gateway client for health, model discovery and OpenAI-compatible inference/SSE forwarding.
- Provider-neutral agent/task/event contracts in `dread-agents`; no provider-specific adapter is enabled.
- SQLite-backed capability registry. Imported/API-created capability records are forced to disabled by default; no plugin, skill or hook is run by cataloging it.
- Durable SQLite task records and ordered task-event audit history:
  - `POST /api/tasks` creates a task in `pending_approval`.
  - `GET /api/tasks`, `GET /api/tasks/{id}` and `GET /api/tasks/{id}/events` read task state/history.
  - `POST /api/tasks/{id}/approve` records an approver label and reason and allows a single transition to `approved`; approval alone never starts execution.
  - `POST /api/tasks/{id}/run` explicitly queues only reviewed built-in `service_discovery` and `gpu_chat` runners, and only for approved tasks without a capability ID.
  - `GET /api/tasks/{id}/result` reads persisted execution results.
  - `POST /api/tasks/{id}/cancel` cancels pending, approved, queued or running tasks and signals the active runner.
  - The runner uses the bounded `dread-orchestrator` gate (default max concurrency 2, max admitted 8, one attempt, 1,805-second per-attempt timeout). It performs configured loopback-only HTTP health probes; it does not execute shell commands, imported skills, plugins or agent code.
  - Results and lifecycle events are persisted. API CI verifies successful execution, result retrieval, terminal task state and success-event history.
  - This is deliberately narrow: there is no provider/agent delegation, capability-backed runner or priority policy. Startup recovery resumes only queued approved capability-free `service_discovery` tasks. Queued `gpu_chat` work is failed closed after restart to avoid duplicate inference; interrupted `running` tasks are marked failed and never replayed. Unsupported or capability-linked queued work is failed closed.
- React/TypeScript operator console in `web/`: model discovery, approval-gated GPU chat task creation, task approval/run/cancel, task audit/result inspection and read-only agent status views. The bearer token remains in page memory with a disconnect control; the UI polls authenticated HTTP APIs and does not put credentials in WebSocket URLs. Vite production build passes CI; the UI is not embedded in the Rust API server and live browser/API acceptance remains outstanding.
- Read-only local agent health discovery at `GET /api/agents`:
  - Reads `config/agents.toml` or `DREAD_AGENTS_CONFIG`.
  - Empty configuration by default; at most 32 entries and unique IDs.
  - Explicit-port loopback HTTP URLs only; userinfo, query strings and fragments are rejected.
  - GET-only health probes with a two-second timeout and redirects disabled.
  - Agent probe status: 2xx ONLINE, 3xx REDIRECTED, 4xx REACHABLE, 5xx DEGRADED, connection failure OFFLINE, timeout UNKNOWN.
  - Disabled entries generate no network request.
  - It does not scan arbitrary ports, inspect processes, start agents, dispatch tasks or execute plugins.
- Task-event WebSocket endpoint `GET /api/tasks/{id}/stream?after_sequence=N`:
  - Streams task events only after their SQLite transaction commits.
  - Subscribes before loading persisted history to avoid a replay/live-event gap.
  - Replays events after the supplied sequence cursor in bounded 1,000-event pages.
  - Uses a bounded broadcast channel and recovers missed events from SQLite if a subscriber falls behind.
  - Formatting, Clippy, compilation and the committed-event subscriber unit test pass in run #77. Run `44025427057` passed the authenticated network-level WebSocket test covering handshake, replay, live event delivery and reconnect using a sequence cursor.
- Pstack capability importer:
  - Catalog pins Pstack `0.15.43`, commit `2f87c854fe3e2fcda44ca86b46ede35aad3c46b7`, subtree `71d8e1d1d718e2149dc43b2535618064f29cba6b`, MIT; inventory is 58 skills and 2 agents.
  - The active pinned source is `integrations/pstack/upstream/` at `0.15.43`, verified against the commit/tree and all 60 provenance hashes. The separate `integrations/pstack-upstream/` directory is a retained older `0.15.42` snapshot; it is not the importer source and has not been relabelled or overwritten.
  - `scripts/import-pstack.sh` verifies the pinned commit/tree and manifest before writing a provenance marker with per-file SHA-256 values.
  - `scripts/import_pstack_capabilities.py` defaults to dry-run; `--apply` is required to POST metadata to the explicit-port loopback API and now requires `DREAD_API_TOKEN` (at least 32 characters), sent as a Bearer token for every API request. It refuses missing/mismatched markers, changed files, unsafe paths, redirects and existing capabilities not confirmed disabled.
  - Canonical-source run `44025427057` passed the live authenticated API import: 58 skills and 2 agents were registered, all 60 records were read back disabled, and a second `--apply` returned 0 created / 60 already present. The run also passed all 11 importer tests, Pstack pin/tree and 60 per-file provenance hashes, Rust formatting, Clippy with warnings denied, all workspace tests and the optimized release build. No skills, agents, scripts or hooks were executed.
- Bounded execution kernel (`dread-orchestrator`) is now wired to the durable task API for two reviewed built-in runners:
  - Enforces a maximum of two active runners and eight admitted tasks by default.
  - Applies a 1,805-second per-attempt timeout, one attempt by default, and watch-channel cancellation.
  - The API queues and runs only approved capability-free `service_discovery` and `gpu_chat` tasks. `gpu_chat` validates bounded text-only messages, checks model availability through the local gateway at `127.0.0.1:11500`, then submits non-streaming inference to the authenticated bridge at `127.0.0.1:11502`. The Python bridge holds the existing `gpu_lease("tensor")` context and calls the managed tensor server at `127.0.0.1:11501` directly. Rust never acquires or releases GPU leases.
  - Execution results are stored in SQLite; task queued/started/succeeded/failed/cancelled events are appended transactionally and broadcast after commit.
  - Cancellation can signal a running task. Startup recovery resumes only queued approved capability-free `service_discovery` tasks; interrupted running tasks are marked failed and not replayed. Unsupported or capability-linked queued tasks fail closed.
  - Orchestrator unit tests cover concurrency, admission overflow, retry bounds, active cancellation and timeout. API tests cover the allowlisted dispatch, persisted result and rejection of unsupported task types.
  - No provider/agent dispatch, process launcher, priority scheduling or capability execution is present. Rust owns no GPU lease; the separate Python bridge delegates to the existing Supervisor context. It does not bypass or replace the Python GPU Supervisor.

## Runtime safety boundaries

- The existing Python GPU Supervisor remains the sole GPU ownership, VRAM lease and process-lifecycle authority.
- Rust uses `127.0.0.1:11500` for model discovery and the authenticated Supervisor bridge at `127.0.0.1:11502` for inference. The bridge owns the existing Python Supervisor lease and calls `127.0.0.1:11501` directly while that lease is held; it must not call the lease-owning gateway at `:11500` or it would recursively acquire the same lock. Rust must never call the tensor upstream directly or acquire a second GPU lock.
- Keep the Python Hub and ComfyUI running while Rust features are developed and tested.
- All `/api/*` routes require a Bearer token from `DREAD_API_TOKEN`; missing server configuration fails closed with 503, and missing/invalid request credentials return 401. `/health` remains unauthenticated.
- `DREAD_API_TOKEN` must be at least 32 characters. The token is supplied through the process environment and must never be committed to the repository.
- Approval audit identity is taken from server configuration `DREAD_OPERATOR_ID`, not caller-supplied request data.
- The API remains loopback-only. This is single-operator bearer authentication, not role-based authorization, token rotation or approval policy for remote access. Do not expose it to a LAN or public network.
- Agent discovery is read-only. Provider endpoints remain unconfigured until verified on the workstation.
- OpenCode remains in the master plan as an external system; its dedicated adapter and read-only contract are not yet implemented or verified.
- No destructive repair, credential change, live service migration or GitHub `main` merge is authorized by this work.

## Remaining blueprint

1. [x] Rust API foundation, local service discovery and GPU gateway boundary.
2. [x] Provider-neutral agent/capability contracts and disabled-by-default capability registry.
3. [x] Durable SQLite task lifecycle and approval/cancellation audit history.
4. [x] Read-only, loopback-only configured agent health discovery.
5. [x] WebSocket task-event fanout, durable replay cursor, bounded lag recovery and authenticated network handshake/reconnect test (run `44025427057` passed).
6. [x] Loopback API Bearer authentication, fail-closed token configuration and server-derived approval identity (CI verified); [ ] role-based authorization, token rotation and any remote-access policy.
7. [ ] Provider-specific adapters, enabled only after each local endpoint contract is verified.
8. [x] Pstack pinned metadata importer, authenticated live API apply, persisted readback, disabled-state verification and idempotent re-import (run `44025427057` passed).
9. [x] Bounded execution kernel integrated with the durable task lifecycle: read-only service discovery and approval-gated local GPU chat through the existing gateway; fail-closed restart recovery (CI verified); [ ] priority/resource-aware policy and provider/agent adapters.
10. [~] React/TypeScript operator console foundation and production build (CI verified); [ ] browser/API acceptance, deployment/embedding and npm lockfile.
11. [ ] Incremental legacy Python Hub workflow parity, installer/upgrade/rollback, backup/restore tests and dual-RTX-3090 workstation acceptance.

The Python Hub, ComfyUI, Ollama and GPU Supervisor remain separate runtime/reference components. Full Universal Hub parity has not been achieved.


## Master-plan alignment — explicit phase ledger

The master architecture plan is the target specification, not a declaration of completed functionality. Current status is:

| Master-plan phase | Status | Evidence / remaining gate |
|---|---|---|
| 0 — Baseline | PARTIAL | Historical workstation map exists. Current local Rust/rustup installation and service states have not been revalidated. The preserved Supervisor/gateway source contract was reviewed; the deployed workstation copy and bridge process remain unverified. Do not repeat a full workstation audit. |
| 1 — Rust foundation | CANONICAL SOURCE / CI PASS | The complete Rust workspace is committed at `dread_universal/`; run `44077407357` (run #78) passed Pstack provenance, 11 importer tests, formatting, Clippy, 50 Rust tests, optimized release build, React production build, four bridge tests, the single-owner guard and authenticated live Pstack import/readback/idempotency. Loopback Bearer auth, server-derived approval identity and startup recovery are included. A live non-conflicting workstation health smoke test remains outstanding. |
| 2 — Existing service adapters | PARTIAL / READ-ONLY CI VERIFIED | Dedicated Ollama `/api/tags` and ComfyUI `/system_stats` metadata parsers pass schema/configuration tests with a 128 KiB response bound. DREAD Hub is generic HTTP reachability only. No live workstation request, Hermes/OpenHands/OpenCode/llama.cpp adapter, or GPU Supervisor handoff is verified. |
| 3 — Capability registry | PARTIAL | Capability metadata registry and pinned Pstack dry-run/importer tests pass. Full Cursor plugin ingestion, compatibility classification, MCP/OpenAPI/CLI/service inventory and representative sample reconciliation remain outstanding. |
| 4 — Agent orchestration | PARTIAL | Durable task lifecycle, approval audit, authenticated WebSocket replay/live stream, bounded runner with service-discovery and GPU-chat tasks, and fail-closed restart handling pass CI. No planner, provider/agent delegation, priority scheduling, result aggregation or multi-agent end-to-end test yet. |
| 5 — GPU/resource coordination | SOURCE + CI VERIFIED / LIVE HANDOFF PENDING | Authenticated Python bridge loads the preserved `gpu_lease("tensor")` context, calls the managed tensor upstream directly and has four unit tests plus a single-owner source guard in run #76. Rust uses the bridge; it owns no GPU lease. No bridge service has been installed or started on the workstation, and live dual-3090 handoff/resource inventory remain unverified. |
| 6 — Modern interface | PARTIAL / BUILD VERIFIED | React/TypeScript/Tailwind console with token gate, task controls, model discovery, agent view and event/result polling; Vite production build passes run `44070114961`. No browser-level live API test, embedded serving, committed npm lockfile or workstation deployment yet. |
| 7 — Diagnostics and controlled repair | NOT STARTED | No diagnosis/repair workflow, simulation, approval-gated patch application, backup/rollback executor or reversible repair test is evidenced. |
| 8 — Migration and release | NOT READY | CI source candidate is not a workstation release or migration. Feature parity, install/upgrade/rollback, live acceptance and user-approved cutover remain outstanding. |

### Source-of-truth consolidation — completed on integration branch

The earlier feature branch used a fixed external release archive plus a patch series. That made its CI candidate reproducible only as a derived artifact, not as a fresh checkout of committed Rust source.

This gap is now closed on the dedicated branch `integration/dread-canonical-source-run81`:

- `dread_universal/` is the complete tracked Rust workspace.
- `legacy-reference/` preserves the historical Python service/GPU reference files.
- `SOURCE_PROVENANCE_RUN81.md` records the exact run #81 artifact ID and outer/inner SHA-256 digests.
- The old archive, patch series, older Pstack snapshot and source backups remain preserved.
- Canonical-source CI run `44019266094` passed Pstack verification/dry-run, 9 importer tests, `cargo fmt`, Clippy with warnings denied, `cargo test --workspace --locked`, and `cargo build --workspace --release --locked` directly from the committed workspace.
- Draft PR #3 proposes integration into the feature branch; it has not been merged. No change to `main` or the workstation is implied.

The original archive/patch workflow remains historical provenance. New Rust development should target the canonical integration branch and build the committed workspace directly. The source-consolidation milestone is complete; remaining milestones are service integration, orchestration, authentication, UI, repair and workstation acceptance.

### Architecture constraints carried forward

- Existing Python DREAD Hub, ComfyUI, Ollama, GPU Supervisor, Hermes, OpenHands and OpenCode remain external systems to discover and adapt; do not replace them.
- The Python GPU Supervisor remains the single GPU lease authority. Rust must use a defined adapter and must not independently start/stop GPU workloads or acquire another GPU lock.
- Rust API remains loopback-only even with Bearer authentication; remote exposure is not authorized.
- Capability ingestion is metadata-only and disabled by default; unknown code/hooks are not executed.
- Approval identity is derived from server-configured `DREAD_OPERATOR_ID`; approval alone does not execute a task, and explicit run dispatch remains restricted to the built-in service-discovery runner.
- A bounded runner primitive is not yet a main orchestrator.


## GPU Supervisor bridge (source and CI verified; not deployed)

The authenticated bridge is `scripts/dread_gpu_supervisor_bridge.py`. It binds to loopback only (default `127.0.0.1:11502`), loads the existing Python `gpu_lease("tensor")` context manager from `DREAD_GPU_SUPERVISOR_SOURCE`, requires a bearer token, bounds request and response bodies to 1 MiB, disables redirects and accepts non-streaming text chat only. While holding the lease it calls the managed tensor endpoint at `127.0.0.1:11501` directly. It must not call the legacy gateway at `:11500`, because that gateway acquires the same lease and would deadlock. Rust checks model availability through `:11500` and sends inference to the authenticated bridge at `:11502`. Run #76 passed four bridge tests and the source-level single-owner guard. These tests use a fake lease and mock upstream; they do not start the bridge or touch the workstation.

Configure `DREAD_GPU_SUPERVISOR_SOURCE`, `DREAD_GPU_SUPERVISOR_BIND` and `DREAD_GPU_SUPERVISOR_TOKEN` for the Python bridge, and matching `DREAD_GPU_SUPERVISOR_URL=http://127.0.0.1:11502` and `DREAD_GPU_SUPERVISOR_TOKEN` for the Rust API. The bridge is a separate process; no systemd unit has been installed or enabled. GPU chat's orchestrator timeout is aligned to the bridge's 1,805-second inference timeout. Cancelling the Rust task closes its request, but the current Python bridge cannot interrupt an already-running upstream generation; it retains the lease until inference completes or the upstream timeout fires. Treat GPU cancellation as best-effort until upstream cancellation is implemented.
