# Verification and release gates

A source archive is not error-free merely because it packages. Release gates:

1. cargo fmt --all -- --check
2. cargo clippy --workspace --all-targets -- -D warnings
3. cargo test --workspace --locked
4. cargo build --workspace --release --locked
5. API smoke test with a temporary database
6. Integration checks against the operator's real local services

GitHub Actions runs gates 1–4. The API binds to loopback by default. There is no authentication; do not expose it to untrusted networks.

Discovery statuses:
- ONLINE: HTTP 2xx/3xx
- REACHABLE: HTTP 4xx (host answered, route/request rejected)
- DEGRADED: HTTP 5xx
- OFFLINE: connection failure
- UNKNOWN: timeout or unclassified response

The persistent SQLite registry must be backed up before manual maintenance. This application does not stop or modify the existing DREAD Hub, ComfyUI, Ollama, or GPU Supervisor.
