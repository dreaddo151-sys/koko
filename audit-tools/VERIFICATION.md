# Verification and Release Gates

## Automated Rust gates

The task-lifecycle, read-only agent-discovery, WebSocket event-stream and Pstack metadata-import candidate passed these GitHub Actions gates in run `44014112205`:

1. `cargo fmt --all -- --check` — PASS
2. `cargo clippy --workspace --all-targets --locked -- -D warnings` — PASS
3. `cargo test --workspace --locked` — PASS, 28 tests
4. `cargo build --workspace --release --locked` — PASS

The workflow applies the candidate patches to the exact uploaded baseline archive before running these checks. The CI result is not a substitute for workstation integration tests. This run predates the later bounded-orchestrator patch; that crate must remain marked CI-pending until a successful run verifies the current feature-branch head.

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

Run `44014112205` passed all nine standard-library importer unit tests, fetched the exact catalog-pinned upstream commit/tree and completed the 58-skill/2-agent dry-run. The pinned catalog is Pstack `0.15.43`, commit `2f87c854fe3e2fcda44ca86b46ede35aad3c46b7`, subtree `71d8e1d1d718e2149dc43b2535618064f29cba6b`, MIT. The bundled `0.15.42` snapshot is not represented as the pinned `0.15.43` source. The live Rust API `--apply` operation, persisted registration and post-import reconciliation remain unverified.

## Bounded execution kernel (current-head CI pending)

The `dread-orchestrator` crate is a generic execution gate for caller-supplied runners. It bounds admitted work and concurrent runner futures, limits attempts, applies per-attempt timeouts and observes watch-channel cancellation. Its tests cover concurrency, admission overflow, retry limits, cancellation and timeout. It is not yet wired to the SQLite task API or any provider adapter; it has no durable queue, priority policy, process launch or GPU ownership. The Python GPU Supervisor remains authoritative. The current-head workflow must pass before this crate is called CI-verified.

## Not yet verified

- Successful formatting, Clippy, full tests and release build on the latest feature-branch head after the bounded-orchestrator patch.
- Live API smoke test and integration against the operator's actual local services.
- End-to-end WebSocket handshake/reconnect using a real client or browser.
- Hermes/OpenHands/other agent endpoint contracts and provider-specific adapters.
- Live Pstack API `--apply`, registration persistence and post-import reconciliation.
- Authentication/authorization and remote-access security.
- Frontend, GPU contention/lease handoff, installer/upgrade/rollback and dual-RTX-3090 acceptance.

The existing Python GPU Supervisor remains the only GPU lease authority. Rust may use only its local gateway at `127.0.0.1:11500`; it must not contact the upstream model server directly or acquire a second GPU lock. Do not stop or modify the existing DREAD Hub, ComfyUI, Ollama or GPU Supervisor as part of Rust development.
