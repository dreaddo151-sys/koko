# Verification and release gates

## Automated CI (required; no skip-on-missing-source path)
1. Workspace source presence.
2. Dependency lock generation.
3. `cargo fmt --all -- --check`.
4. `cargo clippy --workspace --all-targets --locked -- -D warnings`.
5. `cargo test --workspace --locked`.
6. `cargo build --workspace --release --locked`.
7. Live API smoke test proving the health response identifies `llama.cpp`.
8. SQLite unit tests for exchange persistence, usage totals, conversation history and queued-job cancellation invariants.

## Manual workstation acceptance
- Confirm llama.cpp OpenAI-compatible API at `127.0.0.1:11501`; verify `/v1/models` and a real `/v1/chat/completions` request.
- Confirm ComfyUI `127.0.0.1:8188/system_stats`; run a real image workflow and a real video workflow.
- Confirm GPU gateway `127.0.0.1:11500` and both RTX 3090 devices through the existing GPU Supervisor.
- Confirm legacy Hub `127.0.0.1:8080` remains online throughout deployment.
- Exercise failure/timeouts without killing or restarting any pre-existing service.

## Discovery classifications
- ONLINE: HTTP 2xx/3xx
- REACHABLE: HTTP 4xx (host answered, route/request rejected)
- DEGRADED: HTTP 5xx
- OFFLINE: connection failure
- UNKNOWN: timeout or unclassified response

## Not yet accepted
Authentication, streamed message/usage persistence, executing job workers and recovery, render lifecycle, semantic prompt compression/benchmarking, pstack execution adapters, installers, and dual-GPU end-to-end acceptance. SQLite, non-stream token ledger, conversation retrieval, prompt optimization proposal, and job metadata are implemented but still require workstation acceptance. Do not report these as complete until implemented and tested.
