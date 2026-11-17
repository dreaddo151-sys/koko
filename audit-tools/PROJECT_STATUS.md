# PROJECT DREAD UNIVERSAL — Engineering Status

**Project version:** 0.4.0  
**Development branch:** `feature/agent-capability-foundation-2026-11-17`  
**Integration policy:** additive development alongside the existing Python Hub; no merge to `main` or workstation service migration is implied.

## Verified source and CI results

The baseline hardened source passed formatting, Clippy with warnings denied, workspace tests, and an optimized release build in GitHub Actions run `44002596952`.

The task-lifecycle and read-only agent-discovery candidate passed the same four gates in GitHub Actions run `44009858035`:
- `cargo fmt --all -- --check`: PASS
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: PASS
- `cargo test --workspace --locked`: PASS — 27 tests
- `cargo build --workspace --release --locked`: PASS

The candidate archive was downloaded and independently checked: ZIP integrity PASS and SHA-256 matched the generated checksum. These CI results validate the Rust workspace build and tests; they do not constitute end-to-end testing against the user's live workstation services.

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
5. [ ] WebSocket task-event streaming, reconnect/replay semantics and bounded orchestration.
6. [ ] Authentication/authorization before any network exposure or trusted approval identity.
7. [ ] Provider-specific adapters, enabled only after each local endpoint contract is verified.
8. [ ] Pstack/Cursor/MCP/OpenAPI/CLI importer, provenance/license/dependency review and disabled-by-default activation.
9. [ ] React/TypeScript operator UI driven by real API and event state.
10. [ ] Incremental legacy Python Hub workflow parity, installer/upgrade/rollback, backup/restore tests and dual-RTX-3090 workstation acceptance.

The Python Hub, ComfyUI, Ollama and GPU Supervisor remain separate runtime/reference components. Full Universal Hub parity has not been achieved.
