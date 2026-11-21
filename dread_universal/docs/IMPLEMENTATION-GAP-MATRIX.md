# PROJECT DREAD Universal — Master-plan implementation matrix

**Review date:** 2026-11-21  
**Branch:** work/offline-continuation-reconcile-2026-11-21  
**Stack:** PR #14 is a draft stacked continuation on PR #13, which is stacked on PR #12. No PR in this stack is merged by this work.  
**Decision rule:** A row is not complete because a UI panel or API route exists. It needs implementation, automated tests, current CI and the required live acceptance evidence.

## Product invariant

“Universal” means that one DREAD setup adapts to different dual-/multi-GPU PC hardware, accelerator counts/vendors supported by the selected runtime, model layouts, agent ecosystems, Pstack/plugin sources and locally available services. It must not assume the original RTX 3090 machine, /mnt/sabrent, a particular home directory, a fixed number of GPUs or one static ComfyUI/model directory.

Ubuntu 24.04 x86_64 is the current CI/package baseline. Other OS and accelerator adapters remain unaccepted until their own builds, installation, service, update and recovery paths are tested. A platform is never advertised as supported merely because portable Rust code compiles.

## Requirement-by-requirement review

| Master-plan area | Current source implementation | Tests / evidence | Remaining acceptance |
|---|---|---|---|
| One coherent Master Hub | Rust API + React console; historical Python Hub preservation is stated and packages are separate | Ubuntu API/web/package CI in prior runs | Exact installed Python Hub route/auth/stream/cancel/upload/output parity, side-by-side migration and rollback. Do not replace the Python Hub without parity |
| Portable host setup | Existing host profile discovers CPU/RAM/storage and NVIDIA/AMD/Apple telemetry where available. New scanners use environment, current/install-relative and per-user roots rather than the original workstation path | Unit tests exist for host parsing and new path-root scanners | Complete OS-specific host, service/process, filesystem and packaging adapters. Windows/macOS/additional Linux targets are not accepted |
| Multi-GPU / accelerator awareness | Host profile enumerates NVIDIA UUID/PCI, AMD/Apple enumeration, and Intel XPU devices via bounded `xpu-smi query --json`, static discovery-JSON fallback and identity-only `--query-gpu` CSV fallback. Optional Intel telemetry remains unknown when unavailable. GPU Supervisor remains the sole owner of GPU process lifecycle | Parser tests cover multiple Intel GPUs, documented identity/telemetry fields, memory-unit handling, static discovery and CSV identity fallback; run #1196 (`44842488739`) passed all Ubuntu 24.04 CI jobs including strict Clippy, workspace tests, release build, installer/Python checks and package smoke test | Physical Intel XPU CLI/driver acceptance and version compatibility, AMD/Apple stable identity, persistence across device reordering, heterogeneous GPU/resource scheduling, parser coverage on real non-NVIDIA hosts, actual multi-GPU concurrent contention and live acceptance of Supervisor telemetry against the user's Python bridge remain open |
| Model discovery | Gateway-advertised model list is separate from new bounded local weight-file inventory. Local candidates are explicitly marked discovered_not_loaded | Model inventory tests cover format classification, root scan limits, external-link refusal, in-root Hugging Face snapshot symlinks and GGUF header parsing; authenticated API route test | Complete GGUF/projector metadata and dependency checks, format-specific metadata for safetensors/ONNX, compatibility, residency, resource fit and safe catalog refresh. No model may be loaded by discovery |
| Agent discovery | Configured/known loopback health probes remain separate from new local agent.toml / *.agent.toml manifest discovery | New tests cover valid, malformed, remote URL and symlink manifests. Candidate records are disabled and unprobed | Manifest signing/trust policy, provider adapter compatibility review, enable/disable UI and real installed-agent health/identity proof. Never port-sweep, auto-start or execute an unknown agent |
| Pstack and plugins | Existing Pstack pinned importer/preview/approval controls. New Cursor/Claude/generic plugin manifest inventory labels all results untrusted and disabled | Prior importer/provenance/idempotency tests; new bounded scanner tests for malformed manifests, symlinks and safe defaults | Signed publisher/catalog verification, remote source acquisition, permissions, tool/MCP contracts, staged install/update/remove, atomic activation and rollback |
| Live agent execution | Existing provider adapters/task registry and shared execution gate on the later Phase 6 stack | Mock provider lifecycle, approval, idempotency, event/recovery and cancellation tests are recorded in the existing work log | Live container/network/mount/secret/tool isolation evidence for Hermes, DeerFlow and OpenHands. Attestation flags alone do not prove sandbox enforcement |
| ComfyUI/media monitoring | New bounded read-only ComfyUI /system_stats, /queue, /history?max_items=10 monitor | Unit tests for endpoint URL restrictions, queue/history parsing, response limits. API route is protected | WebSocket node progress, cancellation and result delivery; live ComfyUI/dual-GPU concurrency; valid image, MiniMax H3 playable MP4 and FlashVSR artifact acceptance |
| ComfyUI workflow catalog | New bounded metadata-only graph/API-prompt JSON scanner; output omits widget values and workflow payloads; symlink paths rejected | New graph/API-prompt, path and scan-budget tests | Reviewed workflow compatibility/requirements, approved run submission through Supervisor resource policy, result/progress and artifact validation. Discovery never executes workflows |
| Token observability / prompt assistance | Restored from earlier PR #9 semantics on Phase 6: bounded exact provider-token aggregation and Supervisor-routed prompt preview; UI offers explicit Apply/Discard and never submits the suggestion | New aggregation tests for complete/missing usage, API response contract, model/prompt validation, review-only response and no-task-creation assertion; bearer-auth route regression | Green current-head Rust/Clippy/test/release/Web CI and live gateway/Supervisor acceptance on an installed host; verify provider usage shape for target llama.cpp versions |
| Self-monitoring | Existing service/host discovery plus read-only ComfyUI monitor, proposal-only repair diagnostics and a bounded opt-in watchdog for the Rust API service only | Watchdog unit tests cover strict loopback URL, health response, dry-run, allowlisted restart, rate ceiling, cooldown and private state/journal; installer tests verify timer is installed disabled and customized units survive upgrade | Persisted cross-component health history, event correlation, alerting, uptime/resource trends, monitoring during active tasks and real systemd timer acceptance on a clean host |
| Self-repair | New deterministic diagnostic planner classifies observations and returns recommendations marked proposed_not_applied; no command is run | Tests for healthy, offline/degraded, unknown-state and evidence bounds | Full policy state machine: plan, simulate, backup, precondition verification, approval, allowlisted apply, postcondition verification, monitoring and rollback. No arbitrary shell or automatic driver/firmware/credential changes |
| Orchestration/resource control | Existing mission DAG planning/approval/dependency advancement and shared admission gate | Workspace tests and PR #13 managed cancellation regression additions | Resource-class quotas for GPU/model/media/CPU/agent, mission retry/idempotency/timeout/cancellation aggregation, interruption recovery and actual multi-GPU concurrency |
| Security/audit | Bearer auth, loopback checks, disabled-by-default imports, idempotency/approval records and single Supervisor GPU ownership | Current workspace security and static boundary tests; new API route auth tests | Full PASS/DENY/ASK policy, signed extension/release provenance, stronger process/container attestation, secrets review and live provider sandbox checks |
| Upgrade/install | Ubuntu package and portable bundle workflows now include the watchdog files/templates; install paths leave its timer disabled, preserve operator unit overrides, and preserve/merge host-specific gateway and discovery settings; tagged release workflow requests GitHub OIDC and signs SHA256SUMS with Cosign | Prior Ubuntu 24.04 disposable package smoke-test evidence | Current CI installer/Python tests; fresh release-workflow execution proving Cosign OIDC signing; clean-machine package install, opt-in watchdog timer, reboot persistence, failed-upgrade rollback and preservation of ComfyUI/Supervisor/Hub on a real host |
| Release quality | Rust format, strict Clippy, workspace tests, release build, Web build and package checks are configured | Run #1196 (`44842488739`) passed on PR #14 head `8efecbe99375d7270205a53ef19a8a0ea691db0a`: Rust fmt/Clippy/workspace tests/release build; Web production build; installer/release verifier checks; portable install migration/rollback; Python bridge/importer/watchdog tests; GPU single-owner guard; Ubuntu `.deb`/tar assembly, checksums/content and packaged API/Web smoke test | Tagged release signature/bundle publication, physical workstation install/upgrade/reboot/rollback and real Intel/AMD GPU plus media/provider task acceptance remain separate acceptance gates |

## New additive work in this continuation

1. dread-media: bounded read-only ComfyUI status/queue/history monitor and local ComfyUI workflow metadata catalog.
2. dread-gpu::models: bounded model file inventory that distinguishes files found from gateway-advertised and loaded models.
3. dread-agents::extensions: read-only Cursor/Claude/generic plugin manifest discovery; disabled/untrusted by default.
4. dread-discovery: local agent adapter manifest discovery; does not probe or start providers.
5. dread-platform::repair: proposal-only diagnostic plan; no repair is applied.
6. Authenticated Rust routes and an operator inventory workspace for the above capabilities.
7. New regression coverage for scan limits, symlink refusal, disabled defaults, endpoint restrictions, API authentication and managed-runner cancellation.
8. Master architecture and README updated so the original 3090 machine/path is a reference fixture, not the product boundary.

## Non-negotiable preservation

- No workstation was accessed or modified by this branch.
- No package was installed and no service was started, stopped or restarted.
- No ComfyUI workflow or agent task was submitted.
- No model was loaded/unloaded and no GPU lease was acquired.
- The legacy Python Master Hub, ComfyUI and Python GPU Supervisor remain outside this repository write scope.
- PR #12, PR #13 and main are not modified by the PR #14 feature commits.

## Review rule for every next change

Before a source change, name the master-plan row it advances and identify its safety boundary. After the change, add or update tests, inspect the diff, run fmt/Clippy/tests/release build and web build, record the exact CI run/head, then update this matrix. Do not change a row to complete without the corresponding acceptance evidence.
