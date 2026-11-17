# DREAD Operations Completion Contract

This is the release contract for the local-first DREAD platform. A dashboard panel is complete only when its data comes from a real source, its API is authenticated, and its failure modes are tested.

## Non-negotiable architecture

- **Inference: llama.cpp only.** Rust uses the loopback DREAD gateway at `127.0.0.1:11500`; the Python GPU Supervisor bridge forwards to the configured llama.cpp upstream at `127.0.0.1:11501`. Ollama is not an inference dependency.
- **GPU ownership:** Rust never creates CUDA contexts or manages GPU leases. The existing Python Supervisor remains the single GPU ownership authority.
- **Rendering:** ComfyUI at `127.0.0.1:8188` is an independent workload. DREAD monitoring is read-only and must never restart, kill, cancel, or overwrite ComfyUI work.
- **Security:** loopback binding, bearer authentication, explicit mission approval, allowlisted actions, audit events, bounded retries and backup-before-change.
- **Migration:** preserve the Python Hub until Rust parity and live workstation acceptance are demonstrated. No silent cutover.

## Feature acceptance matrix

| Domain | Required behavior | Evidence required |
|---|---|---|
| Fixed command composer | Docked at viewport bottom on every page; responsive desktop/tablet/mobile | Browser E2E at all breakpoints |
| Prompt enhancer | llama.cpp through Supervisor; preview then explicit Apply/Discard; never auto-submit | API, lease and browser tests |
| Token monitor | Exact provider usage counters only; missing usage separate; no estimates | Usage fixtures and API tests |
| Mission control | Durable lifecycle, approvals, cancellation, event history, elapsed time and results | Restart recovery and browser E2E |
| ComfyUI monitor | Read-only queue, progress events, history, outputs and errors | Mocked queue/WebSocket tests and live non-destructive workflow |
| Dual-GPU telemetry | Real utilization, VRAM, temperature, power, PCIe link and timestamp | Supervisor contract tests and live dual-3090 validation |
| Service health | Explicit loopback probes, latency, last success, stale/failure detail | Timeout, redirect, 4xx/5xx and refusal tests |
| Agent fabric | Explicit health endpoints; provider adapters only after review | Adapter tests; no port scans or auto-launch |
| Watchdog | Independent systemd watchdog for DREAD-owned services only; restart ceiling, cooldown, incident journal | Fault injection and restart ceiling tests |
| Self-repair | Diagnosis, explainable plan, backup, allowlisted action, verification and rollback; source/config mutations require approval | Backup integrity, rollback and audit tests |
| Release/upgrade | Versioned config/DB migrations, preflight, backups, health gates, rollback and operator cutover | Fresh install, upgrade and failed-upgrade acceptance |

## Implemented on this branch

- Service inventory points to the llama.cpp-compatible gateway at `127.0.0.1:11500`; Ollama has been removed from DREAD service configuration.
- `GET /api/usage/tokens` aggregates exact prompt/completion/total usage counters from completed `gpu_chat` results. Missing metadata is counted separately.
- The fixed bottom composer displays measured token totals and prompt/completion split.
- `POST /api/prompt/enhance` uses the selected local model through the authenticated Python Supervisor bridge. The browser requires review and Apply/Discard.
- Existing mission lifecycle persists approvals, cancellation, events, results and startup recovery.
- This increment adds authenticated, read-only `GET /api/comfyui/monitor`, which polls ComfyUI `/queue` at the loopback default `127.0.0.1:8188` (override with `DREAD_COMFYUI_URL`). It reports real running/pending prompt IDs and queue positions, plus up to ten recent history records with actual status, output filenames and execution error text. Queue/history responses are streamed with a 2 MiB cap and a 3-second per-request timeout. It does not claim percentage progress or execution-node telemetry.
- The dashboard polls the render queue every five seconds without blocking the other service refreshes.
- The authenticated Python Supervisor bridge exposes read-only `/v1/telemetry` using `nvidia-smi` (utilization, VRAM, temperature, power and PCIe link generation/width). Rust proxies this through `GET /api/gpu/telemetry`; the UI shows only returned samples and leaves unsupported fields blank.
- A separate user-systemd watchdog template probes only the Rust API health endpoint. It is dry-run by default; the installed oneshot uses an explicit `--apply` flag and can restart only `dread-universal.service`, with a three-restarts-per-15-minute ceiling and 60-second cooldown. It never targets ComfyUI, the GPU gateway, Supervisor or legacy Hub. Templates are not installed or enabled by this PR.

## Remaining work — not yet complete

1. ComfyUI WebSocket execution events and node-level progress (history, output filenames and execution errors are now summarized).
2. Live validation of the new read-only dual-GPU telemetry on both RTX 3090s, including PCIe field support.
3. Install and fault-inject the bounded user-systemd watchdog; verify restart ceiling and cooldown.
4. Diagnostics and a repair-plan engine with backup, allowlisted actions, verification, rollback and operator approval for mutations.
5. Provider adapters, planner/delegation, result aggregation and resource-aware scheduling.
6. Installer, service units, migrations, upgrade/rollback and live dual-3090 acceptance.

## Definition of done

DREAD is complete only after the API, UI, llama.cpp route, Supervisor bridge, ComfyUI monitoring, dual-GPU telemetry, watchdog, bounded repair, persistence, security and rollback pass automated tests and live workstation acceptance. CI cannot prove the user's local GPU or services are healthy.
