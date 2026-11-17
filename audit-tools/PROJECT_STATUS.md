# PROJECT DREAD UNIVERSAL — Engineering Status

**Baseline:** uploaded source bundle `DREAD_UNIVERSAL_COMPLETE_20261117_032111.zip`  
**Baseline SHA-256:** `87a9f53911599a06d1501ed1c4ae5751f523be95184932fe3a446f7cd8927305`  
**Project version in Cargo manifests:** `0.4.0`

## Verified in the hosted Rust runner

The exact uploaded archive was downloaded from the GitHub release and its SHA-256 was checked before compilation.

Baseline checks on Rust 1.100.0:

- `cargo fmt --all -- --check`: PASS
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: PASS
- `cargo test --workspace --locked`: PASS — 13 tests (6 API, 3 discovery, 4 GPU)
- `cargo build --workspace --release --locked`: PASS

The hardening candidate adds strict local URL validation, disabled redirects, loopback-only API binding, an explicit 32 MiB JSON body ceiling, and 5 KB / 10 KB / 20 KB payload round-trip tests. See the current CI run before treating the hardening candidate as verified.

## Implemented in this bundle

- Rust API foundation using Axum and SQLite.
- Local service discovery for configured loopback endpoints.
- GPU gateway client for health, model discovery, and OpenAI-compatible inference/SSE forwarding.
- Existing Python Hub, ComfyUI and GPU Supervisor retained as separate reference/runtime components.
- Pstack and Cursor plugin source inventories, with Pstack marked disabled until reviewed.

## Not yet a complete Universal Hub

The following capabilities are not implemented as complete Rust product features in this source bundle:

- Provider-neutral agent adapters with live health, capabilities, task lifecycle, events, cancellation and result retrieval.
- Multi-agent planning, execution, scheduling, resource-aware concurrency and durable task state.
- First-class Pstack/Cursor/MCP/OpenAPI/CLI capability registry and safe import/review lifecycle.
- Approval queue, repair simulation, bounded apply/monitor/rollback lifecycle and audit log.
- Complete React/TypeScript operator frontend and live WebSocket event stream.
- Full migration parity for image/video, mission, terminal, diagnostics, visual development and existing Python Hub workflows.
- Installer, upgrade/rollback tooling, backup/restore validation and hardware acceptance tests on the dual-RTX-3090 workstation.

## Non-negotiable runtime boundaries

- The existing Python GPU Supervisor remains the only GPU ownership and VRAM lease authority.
- Rust may call only the local GPU gateway at `127.0.0.1:11500`; it must not call the upstream model server or acquire a second GPU lock.
- Keep the Python Hub and ComfyUI running while Rust features are developed and tested.
- No imported skill, plugin hook, automation or agent instruction is executed merely because it is present in the catalog.
- OpenCode is intentionally excluded from the current integration direction.
- No live service migration, destructive repair, credential change or GitHub main-branch merge is implied by this source package.

## Recommended implementation sequence

1. Finish and verify the Rust API security and message-integrity hardening.
2. Add structured capability and agent-adapter contracts, then implement read-only discovery for verified local agents.
3. Add durable tasks, approval gates, event streaming and bounded orchestration.
4. Implement the Pstack/Cursor importer and capability review registry with pinned provenance and disabled-by-default execution.
5. Build the React operator UI against the real API and WebSocket events.
6. Port legacy Hub workflows incrementally behind adapters, verifying parity without interrupting existing services.
7. Run end-to-end acceptance on the user's workstation before calling the platform complete.
