# DREAD Operations Completion Contract

This document is the release contract for the DREAD local-first operations platform. It separates implemented behavior from acceptance work still required. A dashboard panel is not considered complete until its data comes from a real source, its API is authenticated, and its failure modes are tested.

## Non-negotiable architecture

- **Inference engine: llama.cpp only.** The Rust API talks to the existing loopback GPU gateway at `127.0.0.1:11500`. The gateway and authenticated Python GPU Supervisor bridge retain GPU ownership. The bridge forwards to the configured llama.cpp upstream at `127.0.0.1:11501`. Ollama is not a DREAD inference dependency and must not be reintroduced in service configuration, examples, tests, or UI copy.
- **GPU ownership:** Rust never acquires CUDA contexts, schedules VRAM, or manages GPU leases. The existing Python Supervisor remains the single authority.
- **Rendering:** ComfyUI at `127.0.0.1:8188` is a separate workload. Monitoring may inspect its queue/history/progress, but DREAD must never restart, kill, or overwrite a render owned by ComfyUI.
- **Security:** loopback binding, bearer authentication, explicit operator approval for missions, allowlisted actions, audit events, bounded retries, and backup-before-change.
- **Legacy migration:** the Python Hub remains available until Rust parity and workstation acceptance are demonstrated. Do not delete or silently replace it.

## Feature acceptance matrix

| Domain | Required behavior | Acceptance evidence |
|---|---|---|
| Fixed command composer | Composer stays docked at the viewport bottom on every page; dashboard panels scroll above it; responsive mobile layout | Browser tests at desktop/tablet/mobile sizes |
| Prompt enhancer | Explicit user action sends prompt to selected local llama.cpp model through Supervisor; returns a preview; user must Apply or Discard; never auto-submits | Authenticated API test, Supervisor lease test, browser preview/apply/discard test |
| Token monitor | Aggregate exact prompt/completion/total token counters returned by inference; distinguish missing usage metadata; no estimates presented as measured | API tests with present/missing usage; model response fixture; visible totals |
| Mission control | Durable queued/approved/running/completed/failed/cancelled states; event history; elapsed time; cancellation; result/error inspection | Restart recovery tests, event replay tests, browser E2E |
| ComfyUI render monitor | Read-only running/pending queue, job IDs, execution progress, node/status messages, outputs and errors; no synthetic progress | Mocked API/WebSocket tests and live ComfyUI acceptance with a non-destructive test workflow |
| Dual-GPU telemetry | Real per-device utilization, VRAM used/free, temperature, power, PCIe link state and sample timestamp; label unavailable values honestly | Supervisor telemetry contract tests and live validation for both RTX 3090s |
| Service health | Explicitly configured loopback services only; latency, last success, failure detail and stale state | Probe tests including timeout, 3xx, 4xx, 5xx and connection refusal |
| Agent fabric | Explicitly configured read-only health checks first; provider adapters and task dispatch added only after separate review | Adapter contract tests; no port scanning or auto-launch |
| Watchdog | Independent systemd-managed health checker for DREAD-owned control-plane processes; bounded restart attempts, cooldown, incident journal and escalation | Fault-injection test proves API recovery and restart ceiling |
| Self-repair | Diagnose first; produce an explainable plan; backup before a change; run only allowlisted repairs; verify; rollback or stop on failure; operator approval for source/config mutation | Fault-injection, backup integrity, rollback and audit tests |
| Release/upgrade | Versioned config and DB migrations, preflight, backups, health gates, rollback, and explicit operator-controlled cutover | Fresh-install, upgrade, failed-upgrade rollback and workstation acceptance |

## Current implementation status on this branch

- The local inference service inventory now points to the DREAD llama.cpp-compatible gateway at `127.0.0.1:11500`; the Ollama entry has been removed.
- `GET /api/usage/tokens` aggregates exact usage counters stored with completed `gpu_chat` results. Responses without complete usage metadata are counted separately. The endpoint does not estimate tokens.
- The fixed composer displays the measured token totals and prompt/completion split.
- `POST /api/prompt/enhance` uses the selected model through the authenticated Python Supervisor bridge. It returns a suggestion only; the browser requires the operator to review and apply or discard it.
- Existing durable task state, approval, cancellation, event, result, and startup recovery remain the mission foundation.
- This branch does **not** yet implement the ComfyUI queue/progress monitor, Supervisor hardware telemetry contract, independent watchdog, allowlisted repair executor, provider delegation, or full release/rollback system.

## Required implementation order

1. Finish and validate the llama.cpp path, token usage, and prompt-enhancement flow.
2. Add read-only ComfyUI queue/history and WebSocket progress monitoring.
3. Add read-only dual-GPU telemetry to the existing Supervisor and consume it through an authenticated Rust API.
4. Add a separate systemd watchdog for DREAD-owned services only, with bounded restart/circuit-breaker behavior.
5. Add diagnostics and a repair-plan engine. Keep repairs preview-only until approved; use backups and verified rollback.
6. Complete agent/provider adapters, planner/delegation, result aggregation and resource-aware scheduling.
7. Complete installer, migrations, service units, upgrade/rollback, and live dual-3090 acceptance.
8. Cut over from the Python Hub only after all acceptance gates pass and the operator approves.

## Definition of done

DREAD is not "complete" because the UI looks complete or CI is green. It is complete only when the full API, frontend, llama.cpp route, Supervisor bridge, ComfyUI monitoring, dual-GPU telemetry, watchdog, bounded repair, persistence, security, and rollback have passed both automated tests and live workstation acceptance. CI cannot prove the user's GPU hardware or local services are healthy.
