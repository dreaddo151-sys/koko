# Verification and Release Gates

## Automated Rust gates — canonical source branch passed

Canonical-source workflow run `44019266094` validated the tracked workspace on `integration/dread-canonical-source-run81` without downloading or reapplying the baseline archive patches:

1. Pstack provenance pin/tree and all 60 `files_sha256` entries — PASS
2. Pstack importer unit tests — PASS, 9 tests
3. Pstack importer dry-run — PASS, 58 skills and 2 agents, all disabled
4. `cargo fmt --all -- --check` — PASS
5. `cargo clippy --workspace --all-targets --locked -- -D warnings` — PASS
6. `cargo test --workspace --locked` — PASS
7. `cargo build --workspace --release --locked` — PASS

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

- API binds to loopback only until authentication/authorization exists.
- JSON request body limit: 32 MiB; oversize requests must be rejected, not truncated.
- Exact-value payload round-trip coverage includes 5 KB, 10 KB and 20 KB.
- Local service and agent discovery accept explicit-port loopback HTTP URLs only.
- Discovery clients do not follow redirects.
- Agent discovery sends GET only, times out after two seconds and probes only explicitly configured endpoints.
- Agent status mapping: 2xx ONLINE; 3xx REDIRECTED; 4xx REACHABLE; 5xx DEGRADED; connection failure OFFLINE; timeout UNKNOWN.
- Disabled agent entries make no network request.
- Task creation enters `pending_approval`; approval records state/audit only and does not start execution.
- `approved_by` is caller-provided text, not authenticated identity.
- Capability imports/records remain disabled by default; no imported code or hooks execute.
- WebSocket task events are emitted only after the corresponding SQLite transaction commits.
- WebSocket replay uses `after_sequence`, bounded 1,000-event pages, and SQLite recovery after broadcast-channel lag.
- A unit test verifies committed task events reach broadcast subscribers; a full network/browser WebSocket handshake and reconnect integration test remains outstanding.

## Pstack importer (CI verified; live API apply pending)

Run #77 passed all nine standard-library importer unit tests, fetched the exact catalog-pinned upstream commit/tree and completed the 58-skill/2-agent dry-run. The pinned catalog is Pstack `0.15.43`, commit `2f87c854fe3e2fcda44ca86b46ede35aad3c46b7`, subtree `71d8e1d1d718e2149dc43b2535618064f29cba6b`, MIT. The active importer reads `integrations/pstack/upstream/` at `0.15.43`; the separate `integrations/pstack-upstream/` snapshot remains `0.15.42` and is retained as historical/reference content. The live Rust API `--apply` operation, persisted registration and post-import reconciliation remain unverified.

## Bounded execution kernel (CI verified; integration pending)

The `dread-orchestrator` crate passed formatting, Clippy, workspace tests and optimized release build in run #77. Its six unit tests cover concurrency, admission overflow, retry limits, cancellation and timeout. It bounds admitted work and concurrent runner futures, limits attempts, applies per-attempt timeouts and observes watch-channel cancellation.

It is not yet wired to the SQLite task API or any provider adapter; it has no durable queue, priority policy, process launch or GPU ownership. The Python GPU Supervisor remains authoritative. This verification does not establish that approved tasks execute; the task API still records approval only.

## Artifact integrity

Run #77 uploaded artifact `DREAD_UNIVERSAL_AGENT_DISCOVERY_FOUNDATION` (artifact ID `14298763422`). The outer artifact ZIP SHA-256 is `704ac77f8f17a3aab7f2387f3a58cbfda5934815b90a8aede958200b8c5c57b6`. Its inner source ZIP SHA-256 is `22b12427f1f84ae537e1e5dce153912bf6c6c415a400b19be146203482f13509`; ZIP integrity passed, with 4,324 entries, no nested ZIPs and 22 historical backup/original files preserved. The bundle contains the Rust API, agent discovery, Pstack importer and bounded orchestrator source.

## Not yet verified

- Live API smoke test and integration against the operator's actual local services.
- End-to-end WebSocket handshake/reconnect using a real client or browser.
- Hermes/OpenHands/other agent endpoint contracts and provider-specific adapters.
- Live Pstack API `--apply`, registration persistence and post-import reconciliation.
- Authentication/authorization and remote-access security.
- Bounded task execution integration, cancellation of a live provider task and resource-aware scheduling.
- Frontend, GPU contention/lease handoff, installer/upgrade/rollback and dual-RTX-3090 acceptance.

The existing Python GPU Supervisor remains the only GPU lease authority. Rust may use only its local gateway at `127.0.0.1:11500`; it must not contact the upstream model server directly or acquire a second GPU lock. Do not stop or modify the existing DREAD Hub, ComfyUI, Ollama or GPU Supervisor as part of Rust development.


## Master-plan acceptance matrix (do not overstate completion)

| Phase | Current evidence | Acceptance still required |
|---|---|---|
| 0 Baseline | Historical paths/endpoints and GPU policy documented | Confirm only changed prerequisites; current local Rust toolchain and exact Supervisor adapter contract remain unverified |
| 1 Rust foundation | Canonical source committed in `dread_universal/`; run `44019266094` passes fmt, Clippy, tests and release build directly from branch | Live non-conflicting workstation health smoke test |
| 2 Service adapters | Generic read-only configured agent health probing | Dedicated Hermes/OpenHands/OpenCode/Ollama/llama.cpp/ComfyUI/DREAD adapters; one read-only request each |
| 3 Capability registry | Disabled-by-default registry plus pinned Pstack dry-run, 9 tests | Cursor manifest/agent ingestion, compatibility taxonomy, MCP/OpenAPI/CLI inventory and representative provenance checks |
| 4 Orchestration | Task state/approval audit, event stream, bounded runner crate | Planner, worker adapter delegation, durable queue integration, result aggregation, cancellation and one delegated multi-agent test |
| 5 GPU/resources | Existing gateway boundary documented | Python GPU Supervisor adapter, resource discovery, lease ownership and one controlled inference/handoff test |
| 6 UI | No frontend acceptance evidence | React/TypeScript/Tailwind/shadcn production build and live API/WebSocket interaction |
| 7 Repair | No repair executor acceptance evidence | Diagnose/plan/simulate/approve/apply/monitor/rollback with a safe reversible test |
| 8 Release | CI candidate artifact only | Self-contained source, installer/upgrade/rollback, feature parity, workstation acceptance and explicit user-approved migration |

## CI provenance status

The archive-and-patch limitation applied to historical feature-branch runs #77–#83. It is resolved for ongoing Rust development by `integration/dread-canonical-source-run81`, where the full workspace is tracked under `dread_universal/`. Run `44019266094` checked out that branch and passed the complete Rust quality gates directly from committed source. The archive and patch series remain preserved for provenance and rollback; they are no longer required to reconstruct the canonical workspace. Draft PR #3 remains unmerged.
