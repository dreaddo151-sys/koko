# PROJECT DREAD UNIVERSAL — Engineering Status

**Project version:** 0.4.0  
**Development branch:** `feature/agent-capability-foundation-2026-11-17`  
**Integration policy:** additive development alongside the existing Python Hub; no merge to `main` or workstation service migration is implied.

## Verified source and CI results

The hardened baseline passed formatting, Clippy with warnings denied, workspace tests and an optimized release build in GitHub Actions run `44002596952`.

The combined task-lifecycle, read-only agent-discovery, WebSocket event-stream, pinned Pstack importer and bounded-orchestrator candidate passed the full Rust quality workflow in GitHub Actions run `44016613531` (run #77, commit `d4df1a8e85ab44264b4f86d00a624af962422689`):
- `cargo fmt --all -- --check`: PASS
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: PASS
- `cargo test --workspace --locked`: PASS — 34 Rust tests
- `cargo build --workspace --release --locked`: PASS
- Pstack importer unit tests: PASS — 9 tests
- Pinned upstream Pstack fetch/tree verification and dry-run: PASS — 58 skills and 2 agents, all disabled by default.
- Source packaging, ZIP integrity test and artifact upload: PASS.

The run applied the candidate patches to the exact uploaded baseline archive before running checks. The uploaded artifact is identified by SHA-256 `704ac77f8f17a3aab7f2387f3a58cbfda5934815b90a8aede958200b8c5c57b6`; its inner source ZIP has SHA-256 `22b12427f1f84ae537e1e5dce153912bf6c6c415a400b19be146203482f13509`. The inner ZIP passed integrity validation, contains 4,324 entries, includes the Rust API and orchestrator source, has no nested ZIPs, and preserves 22 historical backup/original files. CI validates source build and unit tests; it does not constitute end-to-end testing against the operator's live workstation services, a browser WebSocket client or the live Rust API importer.

## Implemented

- Axum Rust API foundation with SQLite persistence.
- Strict loopback-only API binding until authentication and authorization are implemented.
- 32 MiB JSON request-body ceiling; large-message exact-value round-trip tests for 5 KB, 10 KB and 20 KB.
- Local service discovery for explicitly configured loopback HTTP endpoints.
- GPU gateway client for health, model discovery and OpenAI-compatible inference/SSE forwarding.
- Provider-neutral agent/task/event contracts in `dread-agents`; no provider-specific adapter is enabled.
- SQLite-backed capability registry. Imported/API-created capability records are forced to disabled by default; no plugin, skill or hook is run by cataloging it.
- Durable SQLite task records and ordered task-event audit history:
  - `POST /api/tasks` creates a task in `pending_approval`.
  - `GET /api/tasks`, `GET /api/tasks/{id}` and `GET /api/tasks/{id}/events` read task state/history.
  - `POST /api/tasks/{id}/approve` records an approver label and reason and allows a single transition to `approved`.
  - `POST /api/tasks/{id}/cancel` records cancellation.
  - Approval does **not** execute a task; the response reports `execution_started: false`. No task executor or provider dispatch is present.
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
  - Formatting, Clippy, compilation and the committed-event subscriber unit test pass in run #77. A full browser/client WebSocket handshake and reconnect test remains outstanding.
- Pstack capability importer:
  - Catalog pins Pstack `0.15.43`, commit `2f87c854fe3e2fcda44ca86b46ede35aad3c46b7`, subtree `71d8e1d1d718e2149dc43b2535618064f29cba6b`, MIT; inventory is 58 skills and 2 agents.
  - The bundled `integrations/pstack-upstream` snapshot reports `0.15.42`; it is intentionally not imported as `0.15.43`.
  - `scripts/import-pstack.sh` verifies the pinned commit/tree and manifest before writing a provenance marker with per-file SHA-256 values.
  - `scripts/import_pstack_capabilities.py` defaults to dry-run; `--apply` is required to POST metadata to the explicit-port loopback API. It refuses missing/mismatched markers, changed files, unsafe paths, redirects and existing capabilities not confirmed disabled.
  - Run #77 passed nine importer tests, fetched the pinned remote source and completed the dry-run. Imported metadata starts disabled. No skills, agents, scripts or hooks are executed. The live API `--apply` operation has not been run.
- Bounded execution kernel (`dread-orchestrator`, CI verified in run #77):
  - Provides a bounded execution gate for explicitly supplied runner functions.
  - Enforces a maximum number of active runners and a separate cap on admitted work (running plus waiting).
  - Applies per-attempt timeouts, a configured maximum retry count and watch-channel cancellation; cancellation drops the active runner future and reports the active attempt count.
  - Six orchestrator unit tests passed, covering concurrency, admission overflow, retry bounds, active cancellation and timeout.
  - This is an execution primitive only: it has no provider discovery, task database integration, priority scheduler, durable queue, process launcher or provider dispatch. It does not acquire GPU leases or bypass the Python GPU Supervisor.

## Runtime safety boundaries

- The existing Python GPU Supervisor remains the sole GPU ownership, VRAM lease and process-lifecycle authority.
- Rust may communicate only with the local GPU gateway at `127.0.0.1:11500`; it must not call the upstream model server directly or acquire a second GPU lock.
- Keep the Python Hub and ComfyUI running while Rust features are developed and tested.
- The Rust API is loopback-only and currently has no authentication. Do not expose it to a LAN or public network.
- The `approved_by` value is a caller-supplied audit label, not a verified identity. Approval is not an authorization boundary until authentication is added.
- Agent discovery is read-only. Provider endpoints remain unconfigured until verified on the workstation.
- OpenCode is intentionally excluded from the current integration direction.
- No destructive repair, credential change, live service migration or GitHub `main` merge is authorized by this work.

## Remaining blueprint

1. [x] Rust API foundation, local service discovery and GPU gateway boundary.
2. [x] Provider-neutral agent/capability contracts and disabled-by-default capability registry.
3. [x] Durable SQLite task lifecycle and approval/cancellation audit history.
4. [x] Read-only, loopback-only configured agent health discovery.
5. [x] WebSocket task-event fanout, durable replay cursor and bounded lag recovery (unit/CI verified; client handshake test pending).
6. [ ] Authentication/authorization before any network exposure or trusted approval identity.
7. [ ] Provider-specific adapters, enabled only after each local endpoint contract is verified.
8. [x] Pstack pinned metadata importer and disabled-by-default dry-run validation (live API import still pending).
9. [x] Bounded execution kernel primitive (unit/CI verified); [ ] integration with durable task lifecycle and priority/resource-aware queue policy.
10. [ ] React/TypeScript operator UI driven by real API and event state.
11. [ ] Incremental legacy Python Hub workflow parity, installer/upgrade/rollback, backup/restore tests and dual-RTX-3090 workstation acceptance.

The Python Hub, ComfyUI, Ollama and GPU Supervisor remain separate runtime/reference components. Full Universal Hub parity has not been achieved.


## Master-plan alignment — explicit phase ledger

The master architecture plan is the target specification, not a declaration of completed functionality. Current status is:

| Master-plan phase | Status | Evidence / remaining gate |
|---|---|---|
| 0 — Baseline | PARTIAL | Historical workstation map exists. Current local Rust/rustup installation, current service states and GPU Supervisor adapter contract have not been revalidated in this GitHub workflow. Do not repeat a full workstation audit. |
| 1 — Rust foundation | PARTIAL / CI PASS | Rust API, SQLite task/capability records, health/service routes and CI release build exist in the candidate source. A clean, reproducible build from the repository itself and live non-conflicting workstation health smoke test remain outstanding. |
| 2 — Existing service adapters | NOT VERIFIED | No completed Hermes, OpenHands, OpenCode, Ollama, llama.cpp gateway, ComfyUI or DREAD Hub adapter contract and representative read-only request is evidenced by this workflow. Agent health discovery is generic configured HTTP probing, not those dedicated adapters. |
| 3 — Capability registry | PARTIAL | Capability metadata registry and pinned Pstack dry-run/importer tests pass. Full Cursor plugin ingestion, compatibility classification, MCP/OpenAPI/CLI/service inventory and representative sample reconciliation remain outstanding. |
| 4 — Agent orchestration | PARTIAL | Durable task lifecycle, approval audit, WebSocket event stream and bounded runner kernel pass CI. There is no planner, worker delegation, adapter-backed execution, result aggregation, task-kernel integration or multi-agent end-to-end test. |
| 5 — GPU/resource coordination | NOT VERIFIED | The documented GPU gateway boundary exists, but no Rust adapter to the existing Python GPU Supervisor, resource inventory, lease/handoff test or controlled inference verification is evidenced here. Do not create a second scheduler. |
| 6 — Modern interface | NOT STARTED / NOT VERIFIED | No React/TypeScript/Tailwind/shadcn frontend or production UI build is evidenced. |
| 7 — Diagnostics and controlled repair | NOT STARTED | No diagnosis/repair workflow, simulation, approval-gated patch application, backup/rollback executor or reversible repair test is evidenced. |
| 8 — Migration and release | NOT READY | CI source candidate is not a workstation release or migration. Feature parity, install/upgrade/rollback, live acceptance and user-approved cutover remain outstanding. |

### Source-of-truth and CI reproducibility gap

The current GitHub feature branch stores workflow files, audit documentation and patch files; it does **not** store the Rust workspace at `dread_universal/`. The workflow downloads the external GitHub release asset `DREAD_UNIVERSAL_COMPLETE_TRUE.zip` from tag `dread-universal-v0.4.0-test`, verifies its pinned SHA-256, applies patches from this branch, then builds and packages the result.

Therefore run #77/#79 verifies a patched derivative of that fixed external baseline archive, not a Rust source tree committed at the feature-branch commit. The artifact is useful CI evidence, but the branch alone is not a self-contained, reproducible source checkout. The baseline archive is also immutable-by-convention only: changing it requires deliberately updating the URL and checksum together with provenance.

**Required next engineering milestone:** establish one canonical, version-controlled source-of-truth for the Rust workspace. Safely import the verified baseline and applied changes into a dedicated integration branch, preserving source history/provenance and all backups; then make CI build that committed workspace directly. Do not delete the external archive or patch history until the replacement build is independently verified. Do not merge to `main` or alter the workstation as part of this step.

### Architecture constraints carried forward

- Existing Python DREAD Hub, ComfyUI, Ollama, GPU Supervisor, Hermes, OpenHands and OpenCode remain external systems to discover and adapt; do not replace them.
- The Python GPU Supervisor remains the single GPU lease authority. Rust must use a defined adapter and must not independently start/stop GPU workloads or acquire another GPU lock.
- Rust API remains loopback-only until authentication and authorization are implemented.
- Capability ingestion is metadata-only and disabled by default; unknown code/hooks are not executed.
- Approval currently records task state only; it is not authenticated identity and does not start execution.
- A bounded runner primitive is not yet a main orchestrator.
