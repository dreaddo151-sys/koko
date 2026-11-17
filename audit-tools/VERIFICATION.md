# Verification and Release Gates

## Automated Rust gates

The following checks passed for the task-lifecycle and read-only agent-discovery candidate in GitHub Actions run `44009858035`:

1. `cargo fmt --all -- --check` — PASS
2. `cargo clippy --workspace --all-targets --locked -- -D warnings` — PASS
3. `cargo test --workspace --locked` — PASS, 27 tests
4. `cargo build --workspace --release --locked` — PASS

The workflow applies the candidate patches to the exact uploaded baseline archive before running these checks. The CI result is not a substitute for a workstation integration test.

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

## Not yet verified

- Live API smoke test and integration against the operator's actual local services.
- Hermes/OpenHands/other agent endpoint contracts and provider-specific adapters.
- WebSocket event delivery, reconnect/replay and task execution.
- Pstack/Cursor importer end-to-end behavior.
- Authentication/authorization and remote-access security.
- Frontend, GPU contention/lease handoff, installer/upgrade/rollback and dual-RTX-3090 acceptance.

The existing Python GPU Supervisor remains the only GPU lease authority. Rust may use only its local gateway at `127.0.0.1:11500`; it must not contact the upstream model server directly or acquire a second GPU lock. Do not stop or modify the existing DREAD Hub, ComfyUI, Ollama or GPU Supervisor as part of Rust development.
