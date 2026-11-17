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
