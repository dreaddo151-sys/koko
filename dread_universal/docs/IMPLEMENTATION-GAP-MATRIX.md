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
| Multi-GPU / accelerator awareness | Host profile enumerates discovered GPU records and now returns NVIDIA UUID/PCI bus identity where available, plus explicit enumeration index. AMD `rocm-smi --showproductname` and Apple `system_profiler` provide only enumeration indexes in the current adapter; missing stable IDs remain unknown. The host console shows the identity fields separately. GPU Supervisor remains the only owner of GPU process lifecycle | Existing host-discovery tests and static single-owner CI | Intel enumeration, AMD/Apple stable identity, persistence across device reordering, heterogeneous GPU/resource scheduling, parser coverage on real non-NVIDIA hosts and actual multi-GPU concurrent contention tests on more than the reference machine |
| Model discovery | Gateway-advertised model list is separate from new bounded local weight-file inventory. Local candidates are explicitly marked discovered_not_loaded | Model inventory tests cover format classification, root scan limits, external-link refusal, in-root Hugging Face snapshot symlinks and GGUF header parsing; authenticated API route test | Complete GGUF/projector metadata and dependency checks, format-specific metadata for safetensors/ONNX, compatibility, residency, resource fit and safe catalog refresh. No model may be loaded by discovery |
| Agent discovery | Configured/known loopback health probes remain separate from new local agent.toml / *.agent.toml manifest discovery | New tests cover valid, malformed, remote URL and symlink manifests. Candidate records are disabled and unprobed | Manifest signing/trust policy, provider adapter compatibility review, enable/disable UI and real installed-agent health/identity proof. Never port-sweep, auto-start or execute an unknown agent |
| Pstack and plugins | Existing Pstack pinned importer/preview/approval controls. New Cursor/Claude/generic plugin manifest inventory labels all results untrusted and disabled | Prior importer/provenance/idempotency tests; new bounded scanner tests for malformed manifests, symlinks and safe defaults | Signed publisher/catalog verification, remote source acquisition, permissions, tool/MCP contracts, staged install/update/remove, atomic activation and rollback |
| Live agent execution | Existing provider adapters/task registry and shared execution gate on the later Phase 6 stack | Mock provider lifecycle, approval, idempotency, event/recovery and cancellation tests are recorded in the existing work log | Live container/network/mount/secret/tool isolation evidence for Hermes, DeerFlow and OpenHands. Attestation flags alone do not prove sandbox enforcement |
| ComfyUI/media monitoring | New bounded read-only ComfyUI /system_stats, /queue, /history?max_items=10 monitor | Unit tests for endpoint URL restrictions, queue/history parsing, response limits. API route is protected | WebSocket node progress, cancellation and result delivery; live ComfyUI/dual-GPU concurrency; valid image, MiniMax H3 playable MP4 and FlashVSR artifact acceptance |
| ComfyUI workflow catalog | New bounded metadata-only graph/API-prompt JSON scanner; output omits widget values and workflow payloads; symlink paths rejected | New graph/API-prompt, path and scan-budget tests | Reviewed workflow compatibility/requirements, approved run submission through Supervisor resource policy, result/progress and artifact validation. Discovery never executes workflows |
| Self-monitoring | Existing service/host discovery plus new media monitor and proposal-only repair endpoint | Unit tests and authenticated route test for discovery APIs | Persisted health history, event correlation, alerting, uptime/resource trends, policy thresholds and monitoring while tasks are active |
| Self-repair | New deterministic diagnostic planner classifies observations and returns recommendations marked proposed_not_applied; no command is run | Tests for healthy, offline/degraded, unknown-state and evidence bounds | Full policy state machine: plan, simulate, backup, precondition verification, approval, allowlisted apply, postcondition verification, monitoring and rollback. No arbitrary shell or automatic driver/firmware/credential changes |
| Orchestration/resource control | Existing mission DAG planning/approval/dependency advancement and shared admission gate | Workspace tests and PR #13 managed cancellation regression additions | Resource-class quotas for GPU/model/media/CPU/agent, mission retry/idempotency/timeout/cancellation aggregation, interruption recovery and actual multi-GPU concurrency |
| Security/audit | Bearer auth, loopback checks, disabled-by-default imports, idempotency/approval records and single Supervisor GPU ownership | Current workspace security and static boundary tests; new API route auth tests | Full PASS/DENY/ASK policy, signed extension/release provenance, stronger process/container attestation, secrets review and live provider sandbox checks |
| Upgrade/install | Ubuntu package and portable bundle workflows exist; tagged release workflow now requests GitHub OIDC, signs SHA256SUMS with Cosign and publishes the Sigstore bundle; installer preserves/merges host-specific gateway and discovery settings | Prior Ubuntu 24.04 disposable package smoke-test evidence | Fresh release-workflow execution proving Cosign OIDC signing and in-job verification, release-asset download verification, clean-machine install, in-place upgrade, reboot persistence, failed upgrade rollback and service/data preservation on real host |
| Release quality | Rust format, strict Clippy, workspace tests, release build, web build, package checks are configured | Earlier successful runs apply only to their recorded commits. Run #972 passed the web build but failed Rust formatting on an earlier PR #14 head; subsequent formatting fixes are committed, but the latest head has no fresh green CI result yet | Current head must pass all CI jobs after the latest source changes; then live workstation and release acceptance remain separate |

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
