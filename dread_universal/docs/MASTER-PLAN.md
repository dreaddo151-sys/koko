# PROJECT DREAD UNIVERSAL — Master Architecture, Workstation Knowledge & Verified Build Plan

**Specification:** v2.0  
**Status:** Controlling specification; implementation in progress, not complete  
**Target:** Ubuntu 24.04 LTS and later Ubuntu releases only  
**Primary acceptance host:** Ubuntu dual-NVIDIA RTX 3090 workstation  
**Workspace:** `/mnt/sabrent/dread_workspace`  
**Master Hub:** `dread_web_hub.py`  
**Rust workspace:** `dread_universal/`  
**Repository:** https://github.com/dreaddo151-sys/koko

This is the single controlling product specification. It supersedes earlier cross-platform/portable-OS wording. The companion `WORK-LOG.md` records dated implementation evidence and handoff state.

## 1. Mission

Build PROJECT DREAD into one local-first autonomous AI engineering Master Hub for Ubuntu 24.04+. "Universal" means hardware- and capability-adaptive across supported Ubuntu machines, not Windows/macOS portability.

DREAD must discover what a host can actually do, register its models, agents, skills, tools and services, plan and execute engineering work, coordinate inference and media workloads, observe its own operation, diagnose faults, propose safe repairs, verify outcomes and recover from failed changes.

It is not merely a chat interface, model launcher, dashboard or collection of disconnected agents.

The finished operator experience must provide:
- Autonomous engineering missions: plan → execute → test → repair → report.
- Live progress, tool activity, logs, task events and streaming output.
- Persistent projects, sessions, checkpoints, history and results.
- Automatic discovery and registration of available capabilities.
- Controlled agent/model/tool selection and multi-agent orchestration.
- Chat, Code, Image, Video, MiniMax H3 and FlashVSR workflows.
- Security, approvals, audit history, backups, rollback and health monitoring.
- One-click review, addition, activation, updating and removal of skills and agent adapters.
- One-click DREAD upgrades with integrity verification and recovery.

## 2. Platform and non-negotiable constraints

1. **Ubuntu only:** Ubuntu 24.04 LTS and later Ubuntu releases. CI, packaging, installer and acceptance are Ubuntu-focused. Windows/macOS deliverables are out of scope.
2. **Master Hub continuity:** `dread_web_hub.py` remains the existing Master Hub during migration. Do not replace it with a disconnected second product. Add Rust capabilities behind/alongside it, then migrate workflows only after parity and acceptance tests.
3. **Preservation:** Preserve ComfyUI, Python GPU Supervisor, authenticated bridge, models, user data, databases, configs and working Hub features. Do not stop or reconfigure them merely to simplify development.
4. **Single GPU authority:** `dread_gpu_supervisor.py` remains the sole owner of GPU leases, VRAM handoff and inference process lifecycle. Never start a competing llama-server or take a second GPU lock.
5. **Inference:** llama.cpp is the active backend. Do not start or depend on Ollama. Legacy Ollama blob storage may be read as a source of model files only where verified; that does not make Ollama an active server.
6. **Additive and reversible:** Back up before changes, use narrow patches, preserve rollback points and verify after each change. No destructive reset or blind replacement.
7. **Approval and security:** Imported content is untrusted; discovery is read-only by default; execution is allowlisted and auditable; consequential changes require operator approval.
8. **Evidence-based completion:** A compile, mocked response, health endpoint or green unit test alone does not prove end-to-end functionality. Label exactly what was tested.

## 3. Target architecture

### 3.1 Master Hub and operator interface
- Preserve proven Python Hub workflows while Rust is integrated incrementally.
- Deliver one coherent React/TypeScript command interface for missions, models, agents, skills, tools, services, media, approvals, logs and telemetry.
- Show live task state, streamed events, progress, tool calls, errors, outputs and resource use.
- Persist sessions, checkpoints, mission state and results; recover safely after restart.
- Final Ubuntu installation must launch a coherent product without requiring a Vite development server.

### 3.2 Rust control plane and persistence
- Rust owns typed domain models, API boundaries, capability registry, task state machine, policy checks, orchestration and durable records.
- SQLite stores capabilities, config references, missions, tasks, events, approvals, provenance, versions, checkpoints and audit records.
- Validate inputs at boundaries; use explicit adapters for external services.
- Authenticated loopback APIs and reconnectable event streams; no LAN/public binding by default.

### 3.3 Automatic host and capability discovery
Detect and report, without changing the host:
- Ubuntu release, kernel, architecture, CPU model/thread count.
- RAM total/available, disks and workspace capacity.
- GPU vendor/model/count, VRAM, driver and CUDA/ROCm/oneAPI capability where available.
- Installed llama.cpp binaries, gateway state and model inventory.
- ComfyUI, GPU Supervisor, bridge, legacy Hub and configured local services.
- Known local agent endpoints, tools, MCP servers and skill catalogs.
- Dependency versions, health, compatibility and provenance.

Distinguish **detected**, **configured**, **healthy**, **usable**, **approved** and **unknown**. Never infer that a service is executable merely because its port responds. No arbitrary port sweep, LAN scan or unapproved executable launch.

### 3.4 Models and GPU runtime
- llama.cpp is the only active model serving backend.
- Discover GGUF and explicitly supported model formats, metadata, quantization, context limits and projector dependencies.
- Discover models advertised by the managed gateway; report unavailable models accurately.
- Route inference through the existing authenticated Supervisor bridge and lease path.
- Respect VRAM, context, batch, tensor split and concurrent workload limits.
- Coordinate chat, ComfyUI and video through the same Supervisor authority.
- Rust must not call the tensor upstream directly or recursively acquire the same lease.

### 3.5 Agent ecosystem and orchestration
Desired provider integrations behind reviewed adapters:
- Hermes
- OpenHands
- DeerFlow
- Additional locally installed agents discovered through explicit manifests/configuration.

OpenCode is historical/optional ecosystem context, not an assumed default adapter. Do not silently reinstall or re-enable it.

A live AgentAdapter must implement and test: discovery/health, capability declaration, task start, status, event stream, cancellation, result retrieval, timeout, error mapping and recovery. A Markdown prompt profile is not a live agent.

The orchestrator must decompose missions, select agents/models using verified capability and availability, coordinate handoffs, bound concurrency/retries/timeouts/queue depth/resources, persist state, recover safely, prevent duplicate non-idempotent work and surface failures for approval.

### 3.6 Pstack, skills, tools and extension manager
- Integrate the pinned Pstack catalog and supported Cursor plugin/skill sources.
- Current pin: Pstack 0.15.43; commit `2f87c854fe3e2fcda44ca86b46ede35aad3c46b7`; subtree `71d8e1d1d718e2149dc43b2535618064f29cba6b`; MIT; known inventory 58 skills and 2 agent prompt profiles.
- Discover, preview, import, validate, enable, disable, update and remove extensions.
- Show source, version, license, provenance, compatibility, permissions and integrity.
- Imports start disabled. No imported script, hook, shell command or MCP action runs merely because it was discovered/imported.
- Remote installation requires allowlisted sources, pinned manifests, cryptographic hash/signature verification, staged download, backup, atomic activation, audit and rollback.
- One-click means reviewable and reversible, not silent execution.

### 3.7 Media pipelines
Preserve and integrate ComfyUI image generation, MiniMax H3 text/image-to-video, FlashVSR/video enhancement, job status/cancellation/logs/output/download and GPU lease coordination. A workflow is accepted only when it produces and verifies the expected artifact; for video, a valid playable MP4, not merely a loaded node graph.

### 3.8 Engineering, security and self-repair
Support workspace inspection, coding, builds, tests, security scans, fuzzing, diagnostics, root-cause analysis and repair proposals.

Mandatory lifecycle:

**Discover → Understand → Diagnose → Plan → Simulate → Verify → Approve → Apply → Monitor → Rollback**

- Record who/what requested, approved and executed each operation.
- Least privilege and explicit command/tool allowlists.
- Keep secrets out of source, UI persistence and logs.
- Validate paths, URLs, request sizes, payloads, permissions and service boundaries.
- Unknown/denied permissions must never become ALLOW.
- Back up files/config/database consistently before mutation.
- Verify real postconditions; roll back if verification fails.
- Unattended mode is policy-limited; sensitive operations remain approval-gated.

### 3.9 Ubuntu installer, executable and updater
Final distribution targets:
- Ubuntu x86_64 first, matching the primary workstation.
- `.deb` package and portable `.tar.gz`.
- Optional AppImage only after core packaging is stable.

The installed product must include the Rust API, compiled web assets, default config templates, systemd integration, CLI/launcher and migrations; create private config/state/log directories; preserve existing DREAD workspace/models/ComfyUI/Supervisor/data; detect prerequisites without silently changing GPU drivers; provide service status/log/health commands; and support safe uninstall without deleting user data by default.

The updater must download a pinned release, verify signatures/checksums, stage files, back up binaries/config/database, apply safe migrations, atomically activate, check health and roll back on failure. No release is called an installer until clean Ubuntu install, upgrade and rollback have been exercised.

## 4. Known workstation and integration map

Historical observed environment:
- Workspace `/mnt/sabrent/dread_workspace`.
- Legacy Master Hub `dread_web_hub.py`, historically port 8080; it was later offline and must not be falsely reported online.
- ComfyUI `127.0.0.1:8188`; preserve it.
- GPU Supervisor gateway `127.0.0.1:11500`.
- Authenticated Supervisor bridge `127.0.0.1:11502`.
- Managed llama.cpp tensor server `127.0.0.1:11501`, on demand through `dread-tensor.service`.
- Rust API in live stage checkout `127.0.0.1:8765`.
- Qwen3.8 27B GGUF with Qwen3-VL projector; dual RTX 3090 detected.
- Tensor lease and inference must remain Supervisor-controlled; never launch llama-server manually.
- Pstack local source previously at `/mnt/sabrent/dread_workspace/integrations/cursor-plugins/pstack`; configure only after verifying pinned revision.
- Python Hub and Rust API have existed as separate processes/checkouts. Their coexistence is transitional; final product must have one coherent Master Hub experience.

### Historical November 8 Hub audit findings
These are recorded regression targets, not claims about today's live state:
- 44-route API inventory was recorded.
- Build missions persisted in SQLite while some interactive jobs were in-memory.
- `/api/task/*` and `/api/approval/*` had empty-reply defects.
- A permission path involving `/etc/shadow` resolved to ALLOW; it must fail closed.
- Live PASS/DENY/ASK permission evidence and `DREAD_CURRENT_CHECKPOINT.md` were absent at that audit.
- MiniMax H3 assets/custom nodes existed but a valid MP4 was not yet verified.
- LTX was disabled at that audit.
Retest each before declaring fixed.

## 5. Evidence ledger and current implementation state

### Previously verified at earlier checkpoints
- Earlier development checkout passed `cargo fmt --all -- --check`, `cargo check --workspace --locked`, `cargo test --workspace --locked` (50 tests), release build, Clippy with warnings denied and React/TypeScript production build. These predate PR #12 and do not validate its new code.
- Earlier canonical-source GitHub Actions run `44077407357` passed Pstack provenance, 11 importer tests, Rust format, Clippy, 50 Rust tests, release build, React/TypeScript build, four Python Supervisor bridge tests, GPU single-owner guard, authenticated live Pstack API import/readback/idempotency and source inventory hashing. All 60 Pstack records remained disabled.
- Earlier WebSocket network test `44025427057` passed authenticated handshake, replay, live delivery and reconnect with sequence cursor.
- Earlier Rust API inference via authenticated Supervisor bridge returned HTTP 200 and exact content `DREAD GPU INFERENCE OK`. This proves that one inference path at that checkpoint, not the whole platform.
- ComfyUI 0.43.0 and dual RTX 3090 hardware were previously observed online. Recheck before acting.

### Current GitHub candidate
- PR #12: https://github.com/dreaddo151-sys/koko/pull/12
- Branch: `feature/universal-capability-discovery-clean-2026-11-17`
- Base: `work/futuristic-command-center-2026-11-17`
- Last recorded state: open, not merged, mergeable true; 37 commits, 19 files changed, 1,822 additions and 112 deletions at that checkpoint.
- Head at last check: `2168dc5366b9edc161e4940193751528b084e69e`.
- Candidate adds read-only host discovery, configurable llama.cpp gateway, OpenHands/LangGraph health probes, Pstack skill/profile discovery/import/preview/enable/disable and UI controls.
- At the last check no workflow run/status was reported for PR #12. New source must pass Ubuntu CI before merge.
- GitHub branch edits are not workstation installation.

### Known incomplete or unaccepted
- Full legacy Hub parity and single-Hub integration are incomplete.
- Pstack Markdown agent profiles are context profiles, not live AgentAdapter processes.
- Agent health probes do not dispatch tasks.
- Host discovery needs RAM/disk/driver details and stronger inventory.
- FNV-1a in candidate is a local change detector, not cryptographic integrity; use SHA-256 for production provenance/updater.
- UI build is separate from Rust API; production install must not require Vite dev server.
- Transactional installer/updater/rollback is incomplete.
- Hermes/OpenHands/DeerFlow task lifecycle adapters are not all implemented/verified.
- Full mission planning/delegation, resource-aware scheduling and self-repair are incomplete.
- H3/FlashVSR must be tested by producing real artifacts.
- No claim of project completion until all acceptance gates pass.

## 6. Phased roadmap

**Phase 0 — Baseline and preservation:** inventory checkouts, branches, databases, configs, services, ports, systemd units, GPU leases, models and outputs; create timestamped backups and machine-readable baseline.

**Phase 1 — Canonical Rust foundation and Hub continuity:** reconcile live API, development checkout and GitHub source; establish canonical branch/workspace; keep Python Hub as Master Hub while integrating Rust APIs; remove stale source ambiguity; Ubuntu-only CI.

**Phase 2 — Durable registry and full discovery:** complete OS/CPU/RAM/storage/GPU/driver/runtime discovery; persist models, services, agents, tools and skills with provenance, health and capability state.

**Phase 3 — Task kernel and mission orchestration:** durable state machine, approval, event stream, cancellation, bounded concurrency, restart recovery, dependency graph, priorities and resource-aware admission; mission planning and delegation.

**Phase 4 — Real agent adapters:** verify Hermes, OpenHands and DeerFlow API contracts; implement health, capabilities, start/status/events/cancel/result, timeout and recovery. Health-only probes do not count.

**Phase 5 — Extension manager:** pinned Pstack provenance, preview/install/update/disable/remove, compatibility and permission UI, SHA-256/signatures, atomic activation, audit and rollback; reviewed tool/MCP manifests.

**Phase 6 — llama.cpp/GPU integration:** model metadata/compatibility, gateway discovery, resource scheduling; prove single-owner Supervisor invariant under concurrent chat, ComfyUI and video.

**Phase 7 — Media parity:** restore and verify Chat/Image/Video, MiniMax H3 and FlashVSR; real image and MP4 tests, cancellation, progress and downloads.

**Phase 8 — Security and self-repair:** fix legacy permission and task/approval defects; live PASS/DENY/ASK evidence; approval identity/audit; diagnostics, safe repairs, backups, postcondition verification and rollback drills.

**Phase 9 — Unified interface:** one Master Hub UI with sessions, checkpoints, history, live events, tool activity, agent/model/skill management, telemetry and media outputs.

**Phase 10 — Ubuntu installation/updater:** production static UI packaging, `.deb` and `.tar.gz`, systemd, first-run setup, migrations, health checks, backup/restore, signed manifest, atomic updater and rollback; clean Ubuntu 24.04 and later release tests.

**Phase 11 — Release acceptance:** automated suite and dual-3090 workstation acceptance; checksummed artifacts, SBOM/license inventory, release notes and recovery instructions.

## 7. Definition of done

### Ubuntu install
- Clean Ubuntu 24.04 install from `.deb` and `.tar.gz`.
- Launcher and systemd service work after reboot.
- No Vite dev server required.
- Upgrade preserves DB/config/models/outputs; failed upgrade rolls back.
- Uninstall preserves user data by default.

### Discovery
- Correct OS/CPU/RAM/storage/GPU/driver report.
- Both RTX 3090 devices detected with VRAM/driver state.
- ComfyUI, Supervisor, bridge, llama.cpp and configured agent endpoints detected.
- Model files and gateway-advertised models discovered without Ollama server.
- Pinned Pstack source reports 58 skills/2 prompt profiles when present.

### Execution
- Mission planned, approved, decomposed, executed, monitored and persisted.
- Real AgentAdapters execute tasks and return results.
- Cancellation, timeout, restart recovery and duplicate prevention tested.
- Long prompts/payloads round-trip exactly, including 5 KB, 10 KB and 20 KB regression cases.

### GPU/media
- Real inference returns through Supervisor bridge.
- Concurrent workloads respect one GPU lease owner and do not corrupt ComfyUI.
- Image pipeline produces valid image.
- Video workflow produces valid playable MP4.
- FlashVSR produces verifiable enhanced output when enabled.

### Security/recovery
- PASS/DENY/ASK has live regression evidence; unknown permissions fail closed.
- No secrets in Git/logs/browser persistent storage.
- Imported extensions remain disabled until reviewed and cannot execute themselves.
- Backup/restore and failed-upgrade rollback drills pass.
- Repairs record before/after state and verify postconditions.

### Release
- Ubuntu-only CI passes format, Clippy, Rust tests, release build, frontend build, Python bridge/installer tests and security boundary tests.
- Artifact integrity and SHA-256 manifest verify.
- Workstation acceptance report records actual commands and artifact paths.

## 8. Rules for every future session
- Read this file and `WORK-LOG.md` before implementation.
- Use prior terminal evidence, source, GitHub history, tools and skills; do not ask the operator to repeat known facts.
- Avoid repetitive discovery loops; consolidate diagnosis then implement a backed-up fix.
- Never claim a terminal command ran unless it ran on the workstation. Label GitHub CI and workstation evidence separately.
- Never claim Codex, another model or an agent swarm ran unless its execution tool actually ran. Online availability is not the same as a connected execution tool.
- Work in small reviewable commits. Do not merge PRs, alter live services or perform destructive operations without required approval.
- Report: verified complete, implemented/unverified, blocked, next concrete action.

**Controlling outcome:** one complete, installable, maintainable PROJECT DREAD Master Hub for Ubuntu 24.04+, with real discovery, agent/model orchestration, GPU-safe inference, media workflows, secure self-repair and transactional upgrades.

## Current audit addendum — 2026-11-17 (PR #12)

This dated addendum supersedes stale candidate counts and run numbers elsewhere in this document. It does not change the product scope or mark any phase complete.

- Repository: dreaddo151-sys/koko; PR #12 remains open, unmerged and mergeable.
- Candidate branch: feature/universal-capability-discovery-clean-2026-11-17.
- Candidate HEAD after CI workflow correction: 2418155aaf268b136158c7dd8008aff79efe3aac.
- PR base: work/futuristic-command-center-2026-11-17 at b474b0f200c9c800898e67dd0957c2ac927d4aa1.
- PR scope at audit: 52 changed files. Historical backup/original files are included in the PR change list; do not delete them until their content and provenance are inspected. Attempts to retrieve several reported backup paths at the current branch returned 404, so no deletion was attempted.
- GitHub Actions run #306 (44160690656) is the run for this HEAD. Web build and npm lockfile checks passed; Rust formatting, strict Clippy and Rust tests passed. Optimized release build was still running at the latest observation; installer, Python regression, GPU ownership and package smoke-test jobs had not yet completed. Recheck the run before relying on any later result.
- Two workflow invocation defects were corrected on this branch: package assembly now runs via bash scripts/package-ubuntu.sh in CI and release workflows, avoiding dependence on the executable bit. The CI package job also validates installer syntax before assembly. These changes are in the candidate HEAD above.
- Source review confirms dread-agents currently defines a provider-neutral AgentAdapter contract only; no concrete Hermes/OpenHands/DeerFlow task adapter is implemented by that contract. Pstack agent Markdown files remain untrusted prompt profiles, not executing agents.
- The current task kernel executes only reviewed service-discovery and GPU-chat runners. No mission DAG/dependency planner, priority scheduler, multi-agent delegation or provider cancellation is accepted.
- The capability model and Pstack import path keep imported records disabled by default and use SHA-256 fingerprints for skill content. A complete transactional extension install/update/remove manager, signature verification, atomic activation and rollback are still not accepted.
- Host discovery now includes Linux OS/kernel, CPU thread count, RAM, root filesystem capacity and NVIDIA telemetry fields (VRAM, driver, PCIe link, utilization, temperature and power where the utility reports them). It does not yet prove complete physical-disk/workspace inventory, CPU model/topology, CUDA toolkit/runtime compatibility, model-file/projector inventory or all configured local-service states.
- Ubuntu package workflow source now includes .deb and tarball assembly, checksum manifest and disposable package smoke testing. A signed updater, .deb failed-upgrade rollback, clean-install/reboot/upgrade tests and actual workstation install remain unaccepted.
- No live workstation action was performed during this audit. ComfyUI, Python Hub, GPU Supervisor, llama.cpp and user data were not modified by these GitHub-only changes.

### Updated phase ledger (acceptance, not code-presence)

| Phase | Current status | Outstanding acceptance |
|---|---|---|
| 0 Preservation | Historical facts documented; current baseline not captured | Capture fresh machine-readable baseline and verify live services/data on workstation |
| 1 Rust/Hub continuity | Rust API and console build; not one Master Hub | Integrate with legacy Python Hub, route parity, regression and rollback |
| 2 Discovery | Partial read-only host and configured service discovery | Complete CPU/storage/CUDA/model/tool/MCP inventory and persist provenance/health states |
| 3 Missions | Bounded task runner and durable task records | Mission planner, DAG, priorities, resource-aware scheduling, delegation and aggregation |
| 4 Agents | Interface contract and health/profile discovery only | Concrete Hermes/OpenHands/DeerFlow adapters with start/status/events/cancel/result/recovery tests |
| 5 Extensions | Pstack pinned importer, preview and disabled-by-default records | Transactional update/remove, signed provenance, atomic activation, rollback, compatibility and reviewed tool/MCP runtime |
| 6 GPU | Supervisor bridge design and mocked CI coverage | Live bridge credentials/source, llama.cpp inference, concurrent chat/ComfyUI/video lease test on both GPUs |
| 7 Media | Existing ComfyUI/H3 assets documented historically | Image artifact, playable MP4, FlashVSR output, progress/cancel/download and concurrency acceptance |
| 8 Security/repair | Bearer auth, bounded inputs, approval audit and fail-closed imports in part | Complete PASS/DENY/ASK; repair simulation/approval/apply/postconditions/rollback; secret and sandbox audit |
| 9 Unified UI | React console production build | Live browser API/WebSocket, Python Hub integration, sessions/checkpoints/history/tool activity and media acceptance |
| 10 Installer/updater | CI package assembly and portable migration rollback tests in progress | Complete run #306; signed updater, .deb rollback, install/upgrade/reboot/uninstall preservation on Ubuntu |
| 11 Release | Not accepted | All gates above plus dual-3090 workstation report, SBOM/license inventory and verified release artifacts |

Release remains blocked. Do not merge or install merely because the current CI run becomes green; workstation and end-to-end acceptance remain mandatory.


## CI evidence refresh — 2026-11-18

GitHub Actions run #316 (`44169900862`) for candidate HEAD `116406e7eb99dc89c3da90985e6bf9b1248b9960` completed successfully. The Rust, web, and Ubuntu package/smoke-test jobs all passed. This supersedes the older #306 pending status and previous statements that the expanded package smoke test or npm lockfile validation were not yet complete.

The passing CI evidence covers source formatting, strict Clippy, Rust tests and release build; Python bridge/importer tests; the GPU single-owner static guard; installer syntax and portable migration/rollback tests; locked web dependency installation and production build; package checksum/archive verification; disposable Ubuntu 24.04 `.deb` installation; API health; production static UI; and API authentication rejection/acceptance.

This is not live workstation acceptance. Agent execution, mission orchestration, Hub parity, transactional signed extension management, real dual-3090 concurrent inference, ComfyUI image/video pipelines, MiniMax H3/FlashVSR output, complete repair/security policy, signed updater, and install/upgrade/reboot preservation remain open. PR #12 must remain unmerged until those gates are met on the Ubuntu workstation.


## Workstation evidence reconciliation — 2026-11-18

This addendum restores previously completed Ubuntu workstation evidence that must not be treated as untested. It is historical evidence from earlier checkpoints, not a claim that PR #12 has been installed or that every integration remains healthy today.

### Historical live/API checks already completed

- **2026-11-08 — Hub and service API checks:** DREAD Hub at `127.0.0.1:8080`, ComfyUI at `127.0.0.1:8188`, Ollama API at `127.0.0.1:11434`, and OpenCode at `127.0.0.1:4096` were exercised through health/model/process/info/telemetry endpoints, including ComfyUI `/system_stats`, Ollama `/api/tags` and `/api/ps`, and DREAD `/api/hub/vram` and `/api/telemetry`. The recorded model `qwen3.8:27b` was resident with approximately 29.3 GB VRAM reported; two RTX 3090 devices were detected. These checks prove those API observations at that checkpoint only. Ollama is not the approved active runtime for the final architecture; llama.cpp remains the required backend.
- **2026-11-16 — Rust API and discovery:** `127.0.0.1:8765/health` returned HTTP 200 with project `PROJECT DREAD UNIVERSAL`, status `online`, version `0.1.0`. `/api/services` returned nine discovered services. The same checkpoint reported ComfyUI, GPU Gateway, Hermes and OpenHands ingress/server reachable, while legacy Hub 8080, Ollama 11434, llama.cpp 11501 and OpenCode 4096 were unreachable. This was useful discovery evidence, not proof that all nine services were usable task adapters.
- **2026-11-17 — GPU inference:** dual-RTX-3090 inference through the Python GPU Supervisor was previously proven. The Rust API → authenticated bridge → Supervisor lease → managed llama.cpp path returned HTTP 200 and exact content `DREAD GPU INFERENCE OK`. This is a real prior inference proof and must be credited. It does not prove that PR #12's new candidate has been installed or that concurrent ComfyUI/video workloads are safe.
- **ComfyUI:** the live API/system-stats endpoint and installed ComfyUI version were previously checked. MiniMax H3 model assets and relevant workflow nodes were inspected. A valid image artifact from the current integrated path, playable H3 MP4, and FlashVSR output were not accepted by those checks.

### Revised interpretation

- GPU hardware detection and basic managed llama.cpp inference are **historically verified**, not blank/unstarted.
- DREAD API health and service discovery are **historically verified at specific checkpoints**; service reachability must not be conflated with executable agent capability.
- ComfyUI API health/model-node presence are **historically verified**; media artifact production remains open.
- Do not repeat broad GPU discovery, generic API health, or standalone inference tests without a reason. For the PR #12 candidate, run only a compact integration regression: candidate API → authenticated Supervisor bridge → existing Supervisor-owned llama.cpp inference, plus health responsiveness during inference. Then test dual-GPU/concurrent media lease behavior only when media integration is ready.
- Capture a fresh preservation baseline before any workstation install or mutation. Historical service states are not a substitute for a current baseline.


## Mission planning API status — 2026-11-18

The Rust API now exposes persistent mission planning: authenticated `POST /api/missions` validates and stores a bounded dependency graph and deterministic priority-aware order; `GET /api/missions/{id}` retrieves it. Plans remain `planned` and are never auto-executed. This closes the plan-validation/persistence API gap only. Mission DAG execution, task dependency propagation, aggregation, retries, cancellation across a mission, provider delegation and resource-aware scheduling remain incomplete and must not be marked accepted until implemented and tested.
