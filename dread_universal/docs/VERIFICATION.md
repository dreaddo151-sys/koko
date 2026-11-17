# Verification and Release Gates

## Automated Rust gates — canonical source branch passed

Canonical-source workflow run `44068498087` (run #35) validated the tracked workspace on `integration/dread-canonical-source-run81` after bounded task execution, bearer authentication, authenticated Pstack apply-token integration and startup queue recovery, without downloading or reapplying the baseline archive patches:

1. Pstack provenance pin/tree and all 60 `files_sha256` entries — PASS
2. Pstack importer unit tests — PASS, 11 tests
3. Pstack importer dry-run — PASS, 58 skills and 2 agents, all disabled
4. `cargo fmt --all -- --check` — PASS
5. `cargo clippy --workspace --all-targets --locked -- -D warnings` — PASS
6. `cargo test --workspace --locked` — PASS — 43 Rust tests, including GPU-chat gateway dispatch/result, unsafe-payload/model-allowlist rejection, bearer authentication, WebSocket handshake/replay/reconnect and startup-recovery tests
7. `cargo build --workspace --release --locked` — PASS
8. Authenticated API integration — PASS: unauthenticated capability request returned 401; 58 skills + 2 agents imported; 60 records read back with `disabled` status; second import created 0 and found 60 already present.

The canonical branch contains the source at `dread_universal/`; the CI job checked out that branch and ran the gates directly in that directory. Draft PR #3 is open against the feature branch and has not been merged.


GitHub Actions run #77, `44016613531`, commit `d4df1a8e85ab44264b4f86d00a624af962422689`, passed the complete workflow after applying the WebSocket and bounded-orchestrator patches to the exact uploaded baseline:

1. `cargo fmt --all -- --check` — PASS
2. `cargo clippy --workspace --all-targets --locked -- -D warnings` — PASS
3. `cargo test --workspace --locked` — PASS, 34 Rust tests
4. `cargo build --workspace --release --locked` — PASS
5. Pstack importer standard-library tests — PASS, 9 tests
6. Pinned Pstack upstream commit/tree verification and metadata dry-run — PASS, 58 skills and 2 agents, all disabled by default
7. Source packaging, ZIP integrity check and artifact upload — PASS

The historical run #77 workflow applied candidate patches to the exact uploaded baseline archive. The newer canonical-source run validates the committed workspace directly. Neither CI run substitutes for workstation integration tests.

## Security and behavior gates

- API binds to loopback only; all `/api/*` routes require a Bearer token; `/health` remains unauthenticated.
- JSON request body limit: 32 MiB; oversize requests must be rejected, not truncated.
- Exact-value payload round-trip coverage includes 5 KB, 10 KB and 20 KB.
- Local service and agent discovery accept explicit-port loopback HTTP URLs only.
- Discovery clients do not follow redirects.
- Agent discovery sends GET only, times out after two seconds and probes only explicitly configured endpoints.
- Agent status mapping: 2xx ONLINE; 3xx REDIRECTED; 4xx REACHABLE; 5xx DEGRADED; connection failure OFFLINE; timeout UNKNOWN.
- Disabled agent entries make no network request.
- Task creation enters `pending_approval`; approval alone records state/audit and does not start execution.
- Explicit `POST /api/tasks/{id}/run` dispatches only approved, capability-free `service_discovery` and `gpu_chat` tasks. Unsupported task types and capability-backed tasks return conflict. `gpu_chat` accepts bounded text-only messages, verifies model availability through the local gateway, and forces non-streaming inference.
- The service-discovery runner performs configured loopback HTTP probes. The `gpu_chat` runner uses only the loopback GPU gateway at `127.0.0.1:11500`, verifies model availability and stores a bounded JSON result; it does not acquire GPU leases or call upstream model servers directly. No shell, plugin, skill or agent code is executed.
- Execution uses the bounded gate (2 concurrent, 8 admitted by default, 1 attempt, 120-second timeout); results are persisted at `GET /api/tasks/{id}/result`.
- Startup recovery resumes only queued, approved, capability-free service-discovery tasks. Queued `gpu_chat` inference is failed closed after restart to avoid duplicate compute; interrupted running tasks are marked failed and not replayed. Unsupported or capability-linked queued tasks fail closed. This behavior is covered by the recovery test.
- `DREAD_API_TOKEN` must be configured and at least 32 characters; missing server token configuration fails closed with 503, and missing/invalid request credentials return 401.
- Approval identity is derived from server-side `DREAD_OPERATOR_ID`; request-supplied approver names are not trusted.
- Authentication is single-operator bearer-token authentication. Role-based authorization, token rotation and remote-access policy are not implemented; API binding remains loopback-only.
- Capability imports/records remain disabled by default; no imported code or hooks execute.
- Pstack importer dry-run needs no API token. Live `--apply` requires `DREAD_API_TOKEN` and sends `Authorization: Bearer` on every lookup/create request.
- WebSocket task events are emitted only after the corresponding SQLite transaction commits.
- WebSocket replay uses `after_sequence`, bounded 1,000-event pages, and SQLite recovery after broadcast-channel lag. A network-level test validates authenticated 101 handshake, persisted replay, live event delivery and reconnect replay.
- Network-level WebSocket integration test verifies the authenticated 101 handshake, persisted replay after a sequence cursor, live committed-event delivery and reconnect replay. Browser UI integration remains outstanding.

## Pstack importer (live API apply and idempotency CI verified)

Run #77 passed all nine standard-library importer unit tests, fetched the exact catalog-pinned upstream commit/tree and completed the 58-skill/2-agent dry-run. The pinned catalog is Pstack `0.15.43`, commit `2f87c854fe3e2fcda44ca86b46ede35aad3c46b7`, subtree `71d8e1d1d718e2149dc43b2535618064f29cba6b`, MIT. The active importer reads `integrations/pstack/upstream/` at `0.15.43`; the separate `integrations/pstack-upstream/` snapshot remains `0.15.42` and is retained as historical/reference content. Run `44025427057` started a temporary loopback Rust API with a CI-only token, verified missing credentials return 401, applied the importer, read back all 60 records and verified every record remained disabled. A second apply created zero records and reported all 60 already present. The importer requires `DREAD_API_TOKEN` for `--apply`; dry-run remains offline and token-free. This is an isolated CI integration, not a workstation import.

## Bounded execution kernel (CI verified; gateway mock integration passed)

The `dread-orchestrator` crate passed formatting, Clippy, workspace tests and optimized release build in run #77. Its six unit tests cover concurrency, admission overflow, retry limits, cancellation and timeout. It bounds admitted work and concurrent runner futures, limits attempts, applies per-attempt timeouts and observes watch-channel cancellation.

It is wired to the SQLite task API for two reviewed runners: read-only service discovery and bounded non-streaming GPU chat through the existing loopback gateway. API tests verify approved service-discovery execution, GPU-chat execution through a mock gateway, persisted results, rejection of unsupported task types and rejection of invalid or unadvertised GPU models. Startup recovery resumes only queued, approved, capability-free service-discovery tasks. Tasks found running after restart are marked failed with a persisted interruption result and are not replayed; unsupported/capability-backed queued tasks fail closed. GPU inference is not automatically replayed after restart. This CI test uses a mock gateway only; it does not verify the operator's live GPUs or Supervisor lease handoff. It still has no provider/agent adapters, priority policy, process launch or GPU ownership. The Python GPU Supervisor remains authoritative. Approval alone still does not execute anything; the separate explicit run endpoint is required.

## Artifact integrity

Run #77 uploaded artifact `DREAD_UNIVERSAL_AGENT_DISCOVERY_FOUNDATION` (artifact ID `14298763422`). The outer artifact ZIP SHA-256 is `704ac77f8f17a3aab7f2387f3a58cbfda5934815b90a8aede958200b8c5c57b6`. Its inner source ZIP SHA-256 is `22b12427f1f84ae537e1e5dce153912bf6c6c415a400b19be146203482f13509`; ZIP integrity passed, with 4,324 entries, no nested ZIPs and 22 historical backup/original files preserved. The bundle contains the Rust API, agent discovery, Pstack importer and bounded orchestrator source.

## Not yet verified

- Live API smoke test and integration against the operator's actual local services.
- Browser UI integration for the WebSocket handshake/replay/reconnect protocol.
- Hermes/OpenHands/other agent endpoint contracts and provider-specific adapters.
- Role-based authorization, token rotation and any future remote-access security review.
- Priority/resource-aware scheduling, cancellation of a live provider task and provider-specific execution adapters. Startup queue recovery is implemented and covered by the current canonical CI run.
- Frontend, live GPU contention/lease handoff and dual-RTX-3090 inference, installer/upgrade/rollback and workstation acceptance.

The existing Python GPU Supervisor remains the only GPU lease authority. Rust may use only its local gateway at `127.0.0.1:11500`; it must not contact the upstream model server directly or acquire a second GPU lock. Do not stop or modify the existing DREAD Hub, ComfyUI, Ollama or GPU Supervisor as part of Rust development.


## Master-plan acceptance matrix (do not overstate completion)

| Phase | Current evidence | Acceptance still required |
|---|---|---|
| 0 Baseline | Historical paths/endpoints and GPU policy documented | Confirm only changed prerequisites; current local Rust toolchain and exact Supervisor adapter contract remain unverified |
| 1 Rust foundation | Canonical source committed in `dread_universal/`; run `44068498087` passes fmt, Clippy, 43 tests, release build, authenticated Pstack API apply/readback/idempotency and mock GPU-chat integration; loopback Bearer auth and startup recovery verified | Live non-conflicting workstation health smoke test |
| 2 Service adapters | Generic read-only configured agent health probing | Dedicated Hermes/OpenHands/OpenCode/Ollama/llama.cpp/ComfyUI/DREAD adapters; one read-only request each |
| 3 Capability registry | Disabled-by-default registry plus pinned Pstack dry-run, 11 importer tests | Cursor manifest/agent ingestion, compatibility taxonomy, MCP/OpenAPI/CLI inventory and representative provenance checks |
| 4 Orchestration | Task state/approval audit, event stream, bounded runner integrated with service-discovery and GPU-chat tasks; fail-closed restart handling is CI verified | Planner, provider/agent delegation, priority scheduling, result aggregation, live provider cancellation and multi-agent end-to-end test |
| 5 GPU/resources | Existing gateway boundary documented | Python GPU Supervisor adapter, resource discovery, lease ownership and one controlled inference/handoff test |
| 6 UI | No frontend acceptance evidence | React/TypeScript/Tailwind/shadcn production build and live API/WebSocket interaction |
| 7 Repair | No repair executor acceptance evidence | Diagnose/plan/simulate/approve/apply/monitor/rollback with a safe reversible test |
| 8 Release | CI candidate artifact only | Self-contained source, installer/upgrade/rollback, feature parity, workstation acceptance and explicit user-approved migration |

## CI provenance status

The archive-and-patch limitation applied to historical feature-branch runs #77–#83. It is resolved for ongoing Rust development by `integration/dread-canonical-source-run81`, where the full workspace is tracked under `dread_universal/`. Run `44019266094` checked out that branch and passed the complete Rust quality gates directly from committed source. The archive and patch series remain preserved for provenance and rollback; they are no longer required to reconstruct the canonical workspace. Draft PR #3 remains unmerged.
