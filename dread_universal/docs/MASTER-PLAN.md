# PROJECT DREAD UNIVERSAL — Master Architecture, Workstation Knowledge & Verified Build Plan

**Specification:** v2.1  
**Status:** Controlling specification; implementation in progress, not complete  
**Target:** Host-agnostic dual-/multi-GPU PCs, with explicit OS and accelerator adapters  
**Current CI baseline:** Ubuntu 24.04 x86_64  
**Primary acceptance fixture:** Known dual-NVIDIA RTX 3090 workstation; not the only supported hardware profile  
**Workspace:** Discovered from installation/configuration/environment; no workstation-specific default  
**Master Hub:** `dread_web_hub.py`  
**Rust workspace:** `dread_universal/`  
**Repository:** https://github.com/dreaddo151-sys/koko

This is the controlling product specification. The 20 November 2026 user clarification restores the universal product requirement: support other dual-/multi-GPU PCs through host, OS, vendor, runtime and capability adapters. The companion `WORK-LOG.md` records dated implementation evidence and handoff state.

## 1. Mission

Build PROJECT DREAD into one local-first autonomous AI engineering Master Hub for dual-/multi-GPU PCs. "Universal" means the same hub can inventory and adapt to different host machines, GPU vendors, GPU counts, model layouts, installed agent ecosystems, Pstack/plugin sources and runtime capabilities without source edits or assumptions about one user's disk layout.

Ubuntu 24.04 is the current validated CI and packaging baseline, not the product boundary. Windows and macOS, additional Linux distributions, and non-NVIDIA accelerator paths require platform adapters and their own CI/acceptance evidence before they can be advertised as supported. Core domain models, discovery manifests, registry, orchestration and policy must remain portable and must not contain workstation-specific paths or device assumptions.

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

1. **Universal host contract:** The product must adapt to a host with one or more supported accelerators and explicitly support dual-/multi-GPU PC configurations without assuming NVIDIA, RTX 3090, a fixed GPU count, a specific PCI bus layout or fixed ComfyUI/model directories. Platform and accelerator support belongs behind adapters. A platform is not called supported until its host discovery, service management, packaging, upgrade and rollback paths pass CI and real acceptance.
2. **Current validation baseline:** Ubuntu 24.04 x86_64 remains the first fully exercised target. This is the present validation boundary, not a permanent product restriction. Add Windows, macOS and additional Linux distribution CI/runtime adapters without changing the portable Rust control-plane contracts.
3. **Master Hub continuity:** `dread_web_hub.py` remains the existing Master Hub during migration. Do not replace it with a disconnected second product. Add Rust capabilities behind/alongside it, then migrate workflows only after parity and acceptance tests.
4. **Preservation:** Preserve ComfyUI, Python GPU Supervisor, authenticated bridge, models, user data, databases, configs and working Hub features. Do not stop or reconfigure them merely to simplify development.
5. **Single GPU authority:** `dread_gpu_supervisor.py` remains the sole owner of GPU leases, VRAM handoff and inference process lifecycle. Never start a competing llama-server or take a second GPU lock.
6. **Inference:** llama.cpp is the active backend. Do not start or depend on Ollama. Legacy Ollama blob storage may be read as a source of model files only where verified; that does not make Ollama an active server.
7. **Additive and reversible:** Back up before changes, use narrow patches, preserve rollback points and verify after each change. No destructive reset or blind replacement.
8. **Approval and security:** Imported content is untrusted; discovery is read-only by default; execution is allowlisted and auditable; consequential changes require operator approval.
9. **Evidence-based completion:** A compile, mocked response, health endpoint or green unit test alone does not prove end-to-end functionality. Label exactly what was tested.

## 3. Target architecture

### 3.1 Master Hub and operator interface
- Preserve proven Python Hub workflows while Rust is integrated incrementally.
- Deliver one coherent React/TypeScript command interface for missions, models, agents, skills, tools, services, media, approvals, logs and telemetry.
- Show live task state, streamed events, progress, tool calls, errors, outputs and resource use.
- Persist sessions, checkpoints, mission state and results; recover safely after restart.
- Final Ubuntu installation must launch a coherent product without requiring a Vite development server.

### 3.2 Rust control plane and persistence
- Rust owns typed domain models, API boundaries, capability registry, task state machine, policy checks, orchestration and durable records.
- Keep the core host-neutral. Move OS commands, filesystem conventions, GPU telemetry providers, process/service management and package operations behind typed platform adapters.
- SQLite stores capabilities, config references, missions, tasks, events, approvals, provenance, versions, checkpoints and audit records.
- Validate inputs at boundaries; use explicit adapters for external services.
- Authenticated loopback APIs and reconnectable event streams; no LAN/public binding by default.

### 3.3 Automatic host and capability discovery
Detect and report, without changing the host:
- OS, distribution/release, kernel, architecture, CPU model/topology/thread count and memory.
- Physical disks, filesystems, workspace capacity and installation roots.
- Every visible GPU with stable vendor identity (UUID/serial where exposed), PCI bus address, current enumeration index, vendor/model/count, VRAM, PCIe link, driver and available CUDA/ROCm/oneAPI/Metal capabilities. Enumeration index is not treated as a persistent identity; missing identity fields remain unknown rather than fabricated.
- llama.cpp binaries, gateway state, model files, model metadata and gateway-advertised model inventory.
- ComfyUI, GPU Supervisor, bridge, existing Hub and configured local services.
- Pstack, Cursor/Claude and other extension catalogs; local agent manifests, known runtime health endpoints, tools and MCP declarations.
- Dependency versions, health, compatibility, provenance, permissions and resource limits.

Discovery roots come from explicit environment/config, installation-relative paths, standard per-user data locations (including HOME/XDG and USERPROFILE/LOCALAPPDATA where present) and platform adapters. Never embed a developer's home directory, mount point, workspace name or device ID as a universal default. Scan budgets and symlink/path boundaries apply on every platform. For model caches, internal symlinks may be followed only after canonical resolution proves the target remains under the configured model root; outside-root targets and cycles are rejected.

Distinguish **detected**, **configured**, **healthy**, **usable**, **approved** and **unknown**. Never infer that a service is executable merely because its port responds. No arbitrary port sweep, LAN scan or unapproved executable launch.

### 3.4 Models and GPU runtime
- llama.cpp is the only active model serving backend.
- Discover GGUF and explicitly supported model formats, metadata, quantization, context limits, projector/dependency files and model-specific hardware requirements. Read bounded GGUF header metadata (name, architecture, context length, file type and quantization version) without reading tensors or loading the model.
- Support arbitrary GPU counts, including heterogeneous devices where the selected runtime supports them; scheduling must use discovered device identities and measured free resources rather than assuming exactly two identical cards.
- Distinguish a model file discovered on disk, a model advertised by the gateway, a model loadable on this host and a model currently resident. These are separate states.
- Discover models advertised by the managed gateway; report unavailable models accurately.
- Route inference through the existing authenticated Supervisor bridge and lease path.
- Respect VRAM, context, batch, tensor split and concurrent workload limits.
- Coordinate chat, ComfyUI and video through the same Supervisor authority.
- Rust must not call the tensor upstream directly or recursively acquire the same lease.

### 3.5 Agent ecosystem and orchestration
Desired provider integrations behind reviewed adapters:
- Hermes, OpenHands and DeerFlow.
- Pstack prompt profiles and supported agent manifests.
- Additional local and remote agent runtimes discovered through signed or operator-trusted manifests and explicit loopback/configured endpoints.
- A plugin provider must be addable through a versioned adapter/manifest contract without editing the core scheduler.

Discovery must not sweep arbitrary ports, scan LAN ranges, execute an agent binary or treat a prompt profile as a running agent. It should automatically find installed manifests and known registrations, then health-check only endpoints authorized by those manifests/configuration. Report discovered, configured, enabled, healthy, usable and approved as distinct states.

OpenCode is historical/optional ecosystem context, not an assumed default adapter. Do not silently reinstall or re-enable it.

A live AgentAdapter must implement and test: discovery/health, capability declaration, task start, status, event stream, cancellation, result retrieval, timeout, error mapping and recovery. A Markdown prompt profile is not a live agent.

The orchestrator must decompose missions, select agents/models using verified capability and availability, coordinate handoffs, bound concurrency/retries/timeouts/queue depth/resources, persist state, recover safely, prevent duplicate non-idempotent work and surface failures for approval.

### 3.6 Pstack, skills, tools and extension manager
- Integrate the pinned Pstack catalog and supported Cursor plugin/skill sources.
- Current pin: Pstack 0.15.43; commit `2f87c854fe3e2fcda44ca86b46ede35aad3c46b7`; subtree `71d8e1d1d718e2149dc43b2535618064f29cba6b`; MIT; known inventory 58 skills and 2 agent prompt profiles.
- Discover, preview, import, validate, enable, disable, update and remove extensions from Pstack and additional supported ecosystems.
- Show source, version, license, provenance, compatibility, permissions and integrity.
- Imports start disabled. No imported script, hook, shell command or MCP action runs merely because it was discovered/imported.
- Remote installation requires allowlisted sources, pinned manifests, cryptographic hash/signature verification, staged download, backup, atomic activation, audit and rollback.
- One-click means reviewable and reversible, not silent execution.

### 3.7 Media pipelines
Preserve and integrate ComfyUI image generation, MiniMax H3 text/image-to-video, FlashVSR/video enhancement, job status/cancellation/logs/output/download and GPU lease coordination. A workflow is accepted only when it produces and verifies the expected artifact; for video, a valid playable MP4, not merely a loaded node graph.

### 3.8 Engineering, security and self-repair
Support workspace inspection, coding, builds, tests, security scans, fuzzing, diagnostics, root-cause analysis and actual bounded self-repair—not only repair proposals.

Mandatory lifecycle:

**Discover → Understand → Diagnose → Plan → Simulate → Backup → Verify Preconditions → Approve/Policy Check → Apply → Verify Postconditions → Monitor → Rollback**

- The repair engine has explicit modes: observe-only, propose, operator-approved apply and policy-authorized unattended apply. Unattended mode is restricted to pre-approved, idempotent, low-risk runbooks with strict scope and rate limits.
- Each repair runbook declares platform/component applicability, preconditions, permitted operations, expected postconditions, backup strategy, health checks, timeout, rollback strategy and audit evidence.
- A repair is not successful until postconditions are verified. Failed verification triggers rollback and records both the repair and recovery result.
- Never auto-update GPU drivers, firmware, kernel, model weights, credentials, firewall/network policy, user data or arbitrary plugin code.

- Record who/what requested, approved and executed each operation.
- Least privilege and explicit command/tool allowlists.
- Keep secrets out of source, UI persistence and logs.
- Validate paths, URLs, request sizes, payloads, permissions and service boundaries.
- Unknown/denied permissions must never become ALLOW.
- Back up files/config/database consistently before mutation.
- Verify real postconditions; roll back if verification fails.
- Unattended mode is policy-limited; sensitive operations remain approval-gated.

### 3.9 Portable installation, executable and updater
Final distribution targets are platform-specific packages built from the same signed, versioned release manifest:
- Current first validated target: Ubuntu x86_64 with `.deb` and portable `.tar.gz`.
- Future platform adapters: additional Linux distributions, Windows and macOS with native installer/service integration.
- Optional AppImage or equivalent portable packages only after each platform's upgrade and rollback are proven.

The installed product must include the Rust API, compiled web assets, default config templates, systemd integration, CLI/launcher and migrations; create private config/state/log directories; preserve existing DREAD workspace/models/ComfyUI/Supervisor/data; detect prerequisites without silently changing GPU drivers; provide service status/log/health commands; and support safe uninstall without deleting user data by default.

The updater must download a pinned release, verify signatures/checksums, stage files, back up binaries/config/database, apply safe migrations, atomically activate, check health and roll back on failure. No release is called an installer until clean Ubuntu install, upgrade and rollback have been exercised.

## 4. Reference workstation and integration map

The following is a historical acceptance fixture used to prove real integration. It must never be treated as the only supported installation or used as a built-in discovery default.

Historical observed environment:
- Workspace `/mnt/sabrent/dread_workspace` on the historical fixture only. Other hosts must use installation discovery or configure `DREAD_WORKSPACE` explicitly.
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

**Controlling outcome:** one complete, installable, maintainable PROJECT DREAD Universal Master Hub that adapts to other dual-/multi-GPU PCs through portable host and accelerator adapters, real Pstack/agent/model discovery, GPU-safe inference, media workflows, policy-controlled self-repair and signed transactional upgrades. Ubuntu 24.04 remains the first verified acceptance target; it is not the universal product boundary.

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


## Acceptance evidence refresh — 2026-11-18, run #343

The current candidate at `924cda887479741b67168f20524863b84e29bf49` passed GitHub Actions run #343 (`44184194589`) across all three jobs: Rust / Ubuntu 24.04, Web console / Ubuntu 24.04, and Ubuntu packages / smoke test. This includes formatting, strict Clippy, Rust tests, release build, installer syntax, portable migration/rollback tests, Python bridge/importer regression tests, GPU single-owner static guard, locked npm install and production build, package checksum/archive validation, disposable Ubuntu 24.04 `.deb` installation, API health, production UI, unauthenticated 401, and authenticated host-profile access. The mission planning API tests were included in the Rust workspace suite.

This supersedes any earlier statement that run #343 was pending. It does **not** change the acceptance verdict for live workstation deployment or product-level gates. The implementation remains partial in these areas:

- Mission DAG execution, dependency-aware task dispatch/aggregation/retry/cancel, resource scheduling and agent delegation.
- Real Hermes/OpenHands/DeerFlow adapters; health probes and Pstack prompt profiles are not task execution.
- Python Master Hub parity and safe live integration/rollback.
- Complete hardware/storage/CUDA/model/tool inventory.
- Signed provenance and transactional Pstack install/update/remove with atomic activation/rollback.
- Current-candidate live Supervisor/llama.cpp integration regression and dual-GPU concurrency acceptance.
- ComfyUI image, MiniMax H3 playable MP4, FlashVSR output, progress/cancel/download and resource coordination.
- Complete PASS/DENY/ASK permission model, sandboxing, repair simulation/postconditions and recovery drills.
- Signed updater and failed-upgrade rollback.
- Clean Ubuntu 24.04 workstation install/upgrade/reboot, preservation baseline and service/data rollback acceptance.

**Release verdict remains BLOCKED / NOT ACCEPTED.** The CI pipeline is green, but the remaining gates require additional implementation and/or direct evidence on the Ubuntu workstation. Do not merge or install the candidate on the strength of CI alone. Preserve the legacy Hub, ComfyUI and Python GPU Supervisor; do not create a second GPU lifecycle owner.


## Candidate acceptance refresh — 2026-11-18, PR run #359

Latest code candidate `482a1bc1526a64160e2047600cea2c3214365512` passed GitHub Actions PR run #359 (`44186835645`) across all three jobs: Rust / Ubuntu 24.04, Web console / Ubuntu 24.04, and Ubuntu packages / smoke test. The installer now restores the prior app, credentials and systemd unit on failed upgrade, and cleans up a newly enabled unit on failed first install; both paths are regression-tested. The console now renders the host GPU telemetry that the API already exposes and reports Supervisor lease availability independently. The passing pipeline includes formatting, strict Clippy, Rust tests, release build, installer syntax, portable migration/rollback tests, Python bridge/importer tests, GPU single-owner guard, TypeScript type-check/Vite build, package checksum/content validation and disposable Ubuntu 24.04 `.deb` installation/API/UI/auth smoke tests.

### Updated phase ledger (acceptance, not code-presence)

| Phase | Status after run #359 | Remaining acceptance |
|---|---|---|
| 0 — Baseline and preservation | Historical workstation evidence documented; read-only baseline tool packaged | Fresh current-candidate baseline on the physical Ubuntu workstation, including exact unit/port/process/model/output inventory and backup verification |
| 1 — Rust foundation and Hub continuity | Rust API, auth, persistence and packaging CI pass | Legacy `dread_web_hub.py` source is not in this repository; no live Hub integration/parity, handoff or rollback evidence |
| 2 — Durable registry and discovery | Loopback service/agent probes, model discovery, host RAM/CPU thread/root-disk, NVIDIA telemetry, and persistent/network mount capacity; UI displays GPU and mount inventory | Full CPU topology, physical disk/partition/SMART/RAID data, CUDA/runtime/model/tool inventory and durable provenance/health reconciliation |
| 3 — Task kernel and mission orchestration | Task state/approval/events/cancel/recovery baseline; bounded retry/backoff primitive; active runner now receives a 5-second bounded cleanup window on cancellation | Mission-level retry policy, resource-aware scheduling, dependency-aware dispatch, priority/resource admission, aggregation and interrupted-run recovery |
| 4 — Real agent adapters | Hermes and DeerFlow HTTP adapter libraries plus fail-closed environment configuration and read-only GET /api/agent-adapters inventory; loopback/auth/input bounds/idempotency and sandbox attestations; mock HTTP lifecycle tests pass | Still not dispatched as persisted DREAD tasks/missions; OpenHands remains health-probe only; implement task registration/execution/event persistence/provider cancellation and recovery; no live provider or physical sandbox acceptance |
| 5 — Extension manager | Pstack discover/import/preview/enable/disable, SHA-256 fingerprint check, untrusted Markdown-only context | Pinned source provenance, signed manifests, transactional install/update/remove, atomic activation, compatibility/permission UI and rollback |
| 6 — llama.cpp/GPU integration | Supervisor-routed inference bridge; historical dual-3090 inference evidence; current CI single-owner guard | Current-candidate live inference regression and concurrent chat + ComfyUI + video lease contention test on the workstation |
| 7 — Media parity | ComfyUI endpoint and H3 assets documented historically | Integrated image pipeline artifact, playable MiniMax H3 MP4, FlashVSR output, progress/cancel/download and concurrent GPU ownership |
| 8 — Security and self-repair | Bearer auth, loopback-only boundaries, task approval gate, bounded inputs, untrusted Pstack context and audit events | Full PASS/DENY/ASK policy, caller/approval identity, sandbox isolation, repair simulation/postconditions, backups and live recovery drills |
| 9 — Unified interface | React console production build and API-backed overview/tasks/agents/capabilities/models/GPU pages | Python Hub parity, sessions/checkpoints/history, live event/tool activity, media outputs and live browser acceptance |
| 10 — Ubuntu install/updater | `.deb` and portable package CI smoke tests; migration and rollback regression tests now include credentials/unit/first-install cleanup | Signed manifest/provenance verification, atomic updater, upgrade/reboot/preservation and rollback on the physical workstation |
| 11 — Release acceptance | Current CI fully green | All remaining phases above, dual-3090 workstation report, SBOM/license inventory, recovery instructions and verified release artifacts |

**Release remains BLOCKED / NOT ACCEPTED.** CI run #359 is green, but it is not live workstation acceptance. Do not merge or install this candidate on CI evidence alone. The current tool connection exposes GitHub and documentation actions but no SSH/terminal session to the Ubuntu workstation; therefore this run did not touch the user's services or files. Preserve the existing Python Hub, ComfyUI and Python GPU Supervisor until a controlled live acceptance window is available.


## Provider and provenance gate update — 2026-11-18

Candidate `bfa432d51e103e08bb36c77e61695459330c40d0` passed CI run #389 (`44191004156`) across Rust, web console and Ubuntu package smoke tests. The provider-neutral agent crate now contains a reviewed, loopback-only Hermes Runs API adapter with bounded input/output, authenticated requests, run lifecycle, SSE replay and cancellation support. It is deliberately library-only: no API task/mission registration or execution dispatch exists yet, and it must remain unregistered until DREAD approval and sandbox/tool-policy enforcement are wired. Pstack local runtime imports now record honest local/unverified provenance rather than claiming the repository's pinned upstream snapshot or fabricating a version/revision. These changes advance Phase 4 and Phase 5, but neither phase is accepted.

Updated phase deltas:
- **Phase 4:** Hermes adapter contract and unit tests exist. Remaining: safe registration, DREAD approval-gated dispatch, provider sandbox and real local Hermes API acceptance; OpenHands and DeerFlow adapters.
- **Phase 5:** Local import provenance is honest and unverified by default. Remaining: source revision verification, signed manifests, transactional lifecycle and atomic rollback.
- **Phase 8:** Hermes adapter has endpoint restrictions and bounded input/output, but the full PASS/DENY/ASK engine, provider tool sandbox, repair simulation and recovery drills remain incomplete.

CI evidence is not live agent or workstation acceptance. Keep release blocked and do not register or run the Hermes adapter until its approval and sandbox boundary is connected.


## Phase 2 mounted-storage acceptance update — 2026-11-18

The host profile now enumerates persistent and network-mounted filesystems via a bounded, read-only `df` probe and exposes source/type/mount point/total/used/available MiB to the console. Pseudo-filesystems are filtered. Parser regression and the full Ubuntu 24.04 CI pipeline passed in run #403 (`44192143563`), candidate `78c8d4db95686edfe8fbb5c0bc9b86de42da3d4b`. This closes the mount-capacity inventory slice only; it does not establish physical device topology, SMART, RAID, NVMe health or complete CPU/storage discovery. Phase 2 remains partial.


## Acceptance refresh — 2026-11-18, PR run #472

**Latest validated candidate:** `aa78e66c2c9228fa14416de7df0d84aeacf41127`.

### Newly closed sub-gates
- **Mission result visibility:** the mission read API returns persisted node results, errors, attempts, elapsed time and completion time. Autonomous DAG scheduling, dependency-triggered dispatch, aggregation policy and retry remain incomplete.
- **Hermes adapter contract:** local DREAD task identity is now kept distinct from provider run identity across status, SSE events, stop and result methods. Strict input allowlisting and a loopback HTTP lifecycle test pass. The adapter is not wired to task execution; Hermes host-tool sandbox/isolation must be independently verified before enabling it.
- **Pstack provenance and import atomicity:** catalog, marker, manifest and license SHA-256 pins are enforced by the runtime importer; source paths reject traversal/symlink escape and every imported source file is checked against the pinned marker. The authenticated batch import endpoint is bounded, idempotent for exact disabled records, and transactional; conflicts roll back all rows. The Python importer submits one batch and validates a complete response.
- **Installer rollback:** earlier acceptance remains; failed upgrades restore application, credentials, systemd unit and enabled/active state; failed first installs remove the newly enabled unit.
- **GPU console:** actual host telemetry is displayed independently of Supervisor inference-lease status.

### Current candidate CI evidence
GitHub Actions PR run #472 (`44195876703`) passed all three jobs:
1. Rust / Ubuntu 24.04 — formatting, strict Clippy, workspace tests, release build, installer syntax, portable migration/rollback tests, Python bridge/importer regressions and GPU single-owner boundary.
2. Web console / Ubuntu 24.04 — lockfile validation, dependency install and production build.
3. Ubuntu packages / smoke test — `.deb` and portable package assembly, checksums/content verification, disposable Ubuntu 24.04 package installation, API health, production UI, unauthenticated 401 and authenticated host-profile.

### Remaining gates (release still blocked)
- **Phase 1:** no live migration/handoff/rollback with the installed Python Master Hub; the legacy Hub source is not present in this repository.
- **Phase 2:** complete CPU topology, all physical/mounted storage, CUDA/runtime/model/tool inventory and live current-candidate discovery.
- **Phase 3:** autonomous dependency scheduler, resource-aware parallel dispatch, mission-level result aggregation policy, retry/timeout/cancel propagation and mission recovery.
- **Phase 4:** verified Hermes/OpenHands/DeerFlow task adapters wired into the DREAD task lifecycle; tool sandbox, workspace isolation, cancellation and recovery acceptance.
- **Phase 5:** signed source provenance beyond pinned hashes, transactional extension update/remove and atomic activation/rollback.
- **Phase 6:** current-candidate physical dual-3090 inference and concurrent GPU lease contention with ComfyUI/video.
- **Phase 7:** integrated image/video adapters, actual image artifact, playable MiniMax H3 MP4 and FlashVSR artifact, progress/cancel/download.
- **Phase 8:** complete PASS/DENY/ASK authorization, sandbox isolation, repair simulation/postconditions and live recovery drills.
- **Release:** signed updater/SBOM/license inventory and physical Ubuntu install/upgrade/reboot/preservation/rollback acceptance.

CI run #472 is repository and disposable-runner evidence only. It does not certify a live workstation deployment. Keep PR #12 open and unmerged; preserve the existing Python Hub, ComfyUI and Python GPU Supervisor until a controlled workstation acceptance session is available.


## Mission scheduler acceptance refresh — 2026-11-18, PR run #509

**Validated code candidate:** `8c19a5b904fc32c0e811c55ed0d96bb5cd15cc1a`.

Phase 3 now has a tested approval-gated DAG auto-advance path: after an operator starts an approved root task, successful completion dispatches all newly ready nodes that already have explicit approval. A dependent node approved after its prerequisites complete is also dispatched. Pending-approval nodes remain held; task failure/cancellation blocks further dispatch. Dispatch uses the existing task queue and shared execution gate, and conditional state transitions protect against duplicate scheduler races. Mission GET returns per-node result/error/attempt/timing metadata.

**Still open in phase 3:** priority/resource-aware scheduling, explicit retry/backoff policy, timeout/cancel propagation to all child tasks, mission-level aggregation policy, and recovery/reconciliation of interrupted DAGs.

GitHub Actions PR run #509 (`44197296018`) passed all three jobs on this code candidate: Rust formatting, strict Clippy, workspace tests including both mission auto-advance/late-approval integration cases, optimized release build, installer syntax, portable migration/rollback, Python bridge/importer regressions, GPU single-owner guard, React production build, package checksums/content, disposable Ubuntu 24.04 package installation, API/UI/authentication/host-profile smoke tests.

This remains CI evidence, not physical workstation acceptance. Existing legacy Hub/ComfyUI/Supervisor preservation boundaries remain unchanged. Release remains blocked on the outstanding phases and live Ubuntu acceptance.


## Priority-ordered DAG scheduler acceptance — 2026-11-18, PR run #520

**Validated code candidate:** `c41ea4b4424c30f1da536305dc0fef7e8f3f4129`.

The approval-gated mission scheduler now:
- Requires operator approval per task and an explicit operator start of an approved root.
- Immediately dispatches other approved dependency-free roots in deterministic graph priority order, bounded by the shared execution semaphore.
- After successful completion, dispatches all newly ready approved dependent nodes.
- Dispatches a node approved after its prerequisites have already completed.
- Holds pending-approval nodes and blocks further dispatch after task failure/cancellation.
- Exposes per-node result, error, attempt and timing metadata through the mission read API.

Run #520 (`44197777681`) passed all three jobs on this candidate: Rust formatting, strict Clippy, workspace tests including priority-root, dependency, auto-advance and late-approval regressions, release build, installer syntax, portable migration/rollback, Python bridge/importer regressions, GPU single-owner guard, React production build, package checksum/content validation, disposable Ubuntu 24.04 package install, API/UI/authentication and host-profile smoke tests.

**Still open in phase 3:** resource-class-aware scheduling, retry/backoff, explicit per-node timeout and provider cancellation propagation, mission-level aggregation policy, and interrupted mission recovery. Release remains blocked by the other outstanding phases and live Ubuntu acceptance.


## Latest PR-head CI verification — 2026-11-18

**PR:** #12  
**PR head:** `5a494c87592d14cd057cc37d2df4642c92d85e7c`  
**GitHub Actions run:** #527 (`44198019930`) — all three jobs completed successfully.

- Rust / Ubuntu 24.04: formatting, strict Clippy, workspace tests, optimized release build, installer validation, portable migration/rollback, Python bridge/importer regressions and GPU single-owner guard passed.
- Web console / Ubuntu 24.04: locked dependency validation and production build passed.
- Ubuntu packages / smoke test: package assembly, checksum/content validation, disposable Ubuntu 24.04 installation, API health, production UI, unauthenticated 401 and authenticated host-profile smoke test passed.

Run #527 validates the current PR HEAD, whose commit updates acceptance documentation after the Run #520 implementation candidate. It does not change the implementation candidate's identity: mission scheduler code was validated by Run #520 on `c41ea4b4424c30f1da536305dc0fef7e8f3f4129`. The newer head CI confirms the complete current PR tree still builds and passes the same three-job workflow.

**Acceptance boundary unchanged:** this is repository CI and disposable-runner package evidence only. It is not a live install or acceptance on the physical Ubuntu workstation. PR #12 remains open and unmerged. Do not stop, replace or reconfigure the legacy Python Hub, ComfyUI, llama.cpp or Python GPU Supervisor as part of this documentation update.


## Latest implementation evidence — 2026-11-18

Candidate `4cfc38b955cc93d6b4550b402a2dab9db14f30a4` adds capped cancellable retry backoff to the execution primitive and corrects the GNU mounted-filesystem probe to use the supported `df --output` interface. GitHub Actions run #565 (`44203367682`) passed all three Ubuntu 24.04 CI jobs. Retry remains disabled by default (one attempt); per-node retry policy, retryability/idempotency classification, resource-aware mission scheduling, provider cancellation propagation and interrupted-mission recovery remain incomplete. Physical workstation acceptance remains outstanding. See WORK-LOG.md for the dated evidence entry.


## Latest discovery implementation evidence — 2026-11-18

Candidate `13e400c96ebd0434f9c48b1da5532a8b20a79de9` adds a complete distinct `nvcc` compiler inventory (version plus compiler path) while preserving the PATH-first compatibility field. The UI lists all discovered toolkits; a regression covers CUDA 12.8 and 13.0. CI run #571 (`44204021257`) passed all three Ubuntu 24.04 jobs. Physical workstation confirmation is still outstanding. See WORK-LOG.md for the dated entry.


## Latest execution-safety evidence — 2026-11-18

Candidate `9e9a7502c5a149214749599483bf17b28ee412cd` adds explicit retryable/permanent error classification. Ordinary runner string errors and timeouts are not automatically replayed; only explicitly retryable provider failures can enter capped, cancellable backoff. CI run #578 (`44204756118`) passed all three Ubuntu 24.04 jobs. Mission-specific retry policy, provider idempotency wiring and recovery remain incomplete. See WORK-LOG.md.


## Latest CPU discovery and legacy Hub gate evidence — 2026-11-18

Candidate `f3fa453044b37ab304478fb15145eb3a3d233be8` adds logical CPU count, sockets, cores/socket, threads/core and NUMA discovery while keeping process-available parallelism distinct. The baseline capture now records CPU topology. CI run #583 (`44205536123`) passed all three Ubuntu 24.04 jobs. A separate `LEGACY-HUB-COMPATIBILITY.md` audit records the reference Python Hub route families and migration requirements. Gate 1 remains open pending a hash of the actual installed Hub, route-by-route parity and safe side-by-side acceptance. See WORK-LOG.md.


## Hermes and DeerFlow adapter implementation — 2026-11-18

Candidate `36e09c1ab59c4955518379166785ccd732277059` adds two provider-specific HTTP adapters to `dread-agents`:

- **Hermes Runs API:** loopback-IP endpoint restriction, bearer API key, redirects disabled, bounded input/output, durable `Idempotency-Key`, local/provider task identity separation, status polling, SSE event parsing, stop and result retrieval. Execution remains disabled until an operator explicitly attests a supported isolated terminal backend and per-task workspace. DREAD task IDs are used as task-scoped Hermes session IDs; caller-selected shared sessions and prior-response chains are rejected.
- **DeerFlow Gateway API:** loopback endpoint, PAT bearer authentication, supported API prefix, bounded requests/responses, stable per-task thread identity, idempotent run creation, durable status/events, cancellation and result retrieval. `LocalSandboxProvider` is denied; only AIO/BoxLite class names can be attested, and a configured remote/Kubernetes provisioner is rejected. Each DREAD task receives its own thread/workspace.
- Both adapters are library-only. They are not yet registered in `dread-api`, exposed as executable capabilities, or dispatched through persisted DREAD tasks/missions. No provider service has been contacted on the user's workstation.
- Regression tests cover endpoint/auth constraints, disabled-by-default sandbox gates, task identity mapping, supported/unsupported input, status mapping, SSE sequence handling, PAT/API-key use, idempotency headers and mock HTTP start/status/events/cancel/result lifecycles.
- GitHub Actions PR run #605 (`44208381646`) passed all three jobs on this candidate: Rust / Ubuntu 24.04, Web console / Ubuntu 24.04 and Ubuntu packages / smoke test.

**Phase 4 remains partial / not accepted.** Next implementation is safe DREAD task-registry/dispatch wiring, persistent provider-event mapping and cancellation propagation; then a separate OpenHands Agent Server adapter based on a pinned server contract. Live Hermes/DeerFlow/OpenHands sandbox verification and physical host acceptance remain required. The passing mock lifecycle and package CI are not live agent acceptance.


## Agent registry and cancellation handoff — 2026-11-18

Candidate `c930fcd2d72433f7f43b772ab154ba11037f4175` adds:
- Environment-driven, fail-closed adapter construction. Hermes requires `DREAD_HERMES_API_URL`, `DREAD_HERMES_API_KEY`, `DREAD_HERMES_SANDBOX_BACKEND`, and `DREAD_HERMES_ISOLATED_WORKSPACE_PER_TASK=true`. DeerFlow requires `DREAD_DEERFLOW_API_URL`, `DREAD_DEERFLOW_PAT`, `DREAD_DEERFLOW_SANDBOX_PROVIDER`, and `DREAD_DEERFLOW_ISOLATED_WORKSPACE_PER_TASK=true`; optional `DREAD_DEERFLOW_API_PREFIX` defaults to `/api/langgraph`; a non-empty `DREAD_DEERFLOW_PROVISIONER_URL` is rejected.
- The registry only includes adapters whose explicit sandbox attestations succeed. Credentials are read from environment and never returned by the inventory API.
- Protected `GET /api/agent-adapters` reports only configured provider descriptors and enabled state. It does not invoke provider endpoints or expose secrets.
- The orchestrator now allows a bounded five-second cooperative cleanup window after active cancellation so a runner can request provider-side cancellation before the execution future is dropped.
- Registry and cancellation regression tests are included. CI push run #625 (`44209673688`) passed Rust, Web and Ubuntu package smoke jobs on this candidate.

**Still incomplete:** the API has not yet dispatched agent-run tasks to the registry. The inventory route is discovery only. Provider task creation, persisted provider events, remote cancellation and recovery are the next required implementation, followed by OpenHands task support and live sandbox acceptance.


## Provider execution and recovery status — 2026-11-18 (PR #12)

This addendum supersedes earlier statements in this document and the work log that Hermes and DeerFlow were library-only, that OpenHands had health discovery only, or that provider execution/recovery was not connected. It records repository and CI evidence only. It does not mark Phase 4 or the full product complete.

- Repository: `dreaddo151-sys/koko`; PR #12 remains open and unmerged.
- Branch: `feature/universal-capability-discovery-clean-2026-11-17`.
- Verified source head: `976880a5cf6ffc587fb420d4458a29ede0d7410c`.
- GitHub Actions run #690 (`44220143462`, push) and run #691 (`44220150439`, PR) both passed all three jobs at this exact head: Rust / Ubuntu 24.04, Web console / Ubuntu 24.04, and Ubuntu packages / smoke test.
- Rust gates passed: `cargo fmt --all -- --check`, strict Clippy with warnings denied, `cargo test --workspace --locked` (111 passed, 0 failed across the workspace), and optimized release build. Installer syntax, portable installation migration/rollback, Python bridge/importer regressions, and the single-owner GPU boundary check also passed.
- The Web job verified the committed npm lockfile, installed locked dependencies and built the production React console.
- The package job assembled the Ubuntu `.deb` and portable bundle, verified package checksums and contents, installed the package in a disposable Ubuntu 24.04 runner, and smoke-tested the packaged API and production console. Artifacts were uploaded by the workflow.
- The Rust tests include authenticated/task-scoped OpenHands Agent Server lifecycle coverage, provider registration/attestation checks, provider task persistence, task event replay, queue recovery, and mission dispatch regressions.

### Implemented provider execution slice

- `HermesApiAdapter`, `DeerFlowApiAdapter` and `OpenHandsAgentServerAdapter` are concrete implementations registered through fail-closed environment configuration. Provider credentials are never returned by the protected adapter inventory.
- The task API routes approved provider-backed tasks through the worker. A provider capability must still be enabled and mapped to the selected adapter. A health probe alone never authorizes task execution.
- Workers persist the provider task identity, provider event cursor, task event audit history, result and error. Event records are committed before they are published to stream subscribers.
- Startup recovery requeues previously running provider tasks only when approval is recorded, the matching capability remains enabled and the configured adapter remains enabled. It resumes from the persisted event cursor and relies on deterministic provider identities/idempotent start behavior to reattach rather than create duplicate remote work. Invalid recovery candidates fail closed and block their mission.
- Provider configuration is unset by default. Hermes requires an explicitly attested isolated terminal backend and workspace per task. DeerFlow requires AIO or BoxLite and a per-task isolated thread/workspace; local sandbox and remote provisioner configurations are rejected. OpenHands requires an explicit container identity, per-task workspace isolation and operator attestation that the Agent Server's stable `OH_SECRET_KEY` is securely configured.
- The OpenHands integration uses the Agent Server conversation API, paginated event search and pause operation. A provider pause is resumable at the OpenHands side; DREAD records its task as cancelled, but this is not permanent conversation deletion.

### Updated acceptance ledger

| Phase | Current status | Remaining acceptance |
|---|---|---|
| 3 Task kernel and missions | Durable task lifecycle, approval-gated dispatch, bounded execution, persisted provider identity/events/cursor, provider recovery and dependency-aware mission dispatch are implemented and CI-tested | Resource-aware scheduling, explicit mission result/aggregation policy, live provider cancellation and restart drills |
| 4 Agent adapters | Hermes, DeerFlow and OpenHands adapters are registered and connected to the approved task worker; mock lifecycle and recovery tests pass | Verify actual provider API versions, credentials, isolated sandboxes/workspaces, tool permissions, cancellation and restart recovery on the Ubuntu workstation |
| 10 Ubuntu packaging | CI builds packages and validates checksums, contents, portable migration/rollback and disposable `.deb` API/UI smoke tests | Clean physical workstation install/upgrade/reboot, service-preservation and failed-`.deb`-upgrade rollback |
| 11 Release | Not accepted | Complete remaining phases, dual-RTX-3090 acceptance, SBOM/license review, signed release metadata and workstation recovery evidence |

### Safety and evidence boundary

- These CI runs did not contact the user's live Hermes, DeerFlow or OpenHands endpoint, deploy the candidate to the workstation, execute a real provider task, restart a real provider, or validate container isolation and tool policy.
- They did not install, restart or replace the legacy Python Hub, ComfyUI, Ollama, GPU Supervisor or other workstation services. The existing Python GPU Supervisor remains the single GPU lease authority.
- Provider environment flags are operator attestations. They are not an automated proof of a secure container, per-task filesystem boundary or secret store. Keep providers disabled until those conditions have been independently verified.
- The API and production console package are CI-verified candidates, not a completed Master Hub migration or production release. PR #12 must remain open/unmerged until the physical Ubuntu workstation and remaining product acceptance gates are complete.


## Checkpoint refresh — 2026-11-18, PR #12 head 573627c

- Rechecked PR #12 and verified commit `573627c204891e85d7a5bb96ecaacb8db52df995` exists on `feature/universal-capability-discovery-clean-2026-11-17`. PR remains open/unmerged; no merge or deployment was performed.
- Verified GitHub Actions PR run #699 (`44221090674`) and push run #698 (`44221083709`) completed successfully. Both expose three successful jobs: Rust / Ubuntu 24.04, Web console / Ubuntu 24.04, and Ubuntu packages / smoke test. The PR run's job steps include Rust formatting, strict lint, workspace tests, release build, installer validation, portable migration/rollback, Python bridge/importer regression tests, GPU single-owner guard, locked npm install/production build, package checksum/content verification, disposable Ubuntu 24.04 package installation, and packaged API/production-console smoke test. The 111-test count is recorded in the PR head work-log entry; no CI was rerun in this checkpoint.
- The latest PR description and work-log record all three provider adapters (Hermes, DeerFlow, OpenHands) registered and wired to approval-gated persisted task execution/recovery. Their lifecycle tests use mocks. Provider config remains opt-in; explicit isolation flags are attestations, not proof of container, filesystem, secret-store or tool-policy enforcement. No real provider task is accepted.
- **Workstation acceptance not performed in this checkpoint.** No connected SSH or workstation terminal session was available in this chat, so no current process/port/unit/config/secret/sandbox/GPU/media state was read. No workstation command was claimed, and no service, file, database, model or user data was changed. In particular, the Python Master Hub, ComfyUI and Python GPU Supervisor were not stopped, restarted, replaced or deployed.
- Therefore live OpenHands/Hermes/DeerFlow configuration and per-task tool/sandbox permissions remain **unknown/unverified**, not absent and not accepted. Do not submit a real provider task until endpoint/API versions, credentials handling, container identity, per-task workspace boundaries, tool allowlists and provider cancellation/recovery behavior have been independently checked on the host.
- Next acceptance action: obtain an authorized read-only workstation terminal session and capture a compact baseline for the existing Python Hub, ComfyUI, Python GPU Supervisor, Rust API, provider processes/endpoints and redacted configuration-presence/permission metadata. Preserve all running services. Then verify provider sandbox/tool policy before a controlled non-destructive provider test. Follow with the already-listed Hub parity, dual-3090/media, resource-aware mission policy, signed extension/updater and physical Ubuntu install/upgrade/reboot/rollback gates.


## Recovered Ubuntu workstation test evidence — 2026-11-17 23:33

A saved terminal capture, `Pasted text(20261117-233320).txt`, was recovered from the user's project files and inspected during the 2026-11-18 checkpoint. It records commands executed in the Ubuntu workspace `/mnt/sabrent/dread_workspace/dread_universal_upstream/dread_universal`, on branch `feature/universal-capability-discovery-clean-2026-11-17` tracking its origin branch:

- `cargo fmt --all -- --check`: exit status 0.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: exit status 0.
- `cargo test --workspace --locked`: exit status 0; 87 unit tests passed, zero failures (11 agents, 35 API, 11 discovery, 6 GPU, 9 orchestrator, 8 platform, 7 skills).
- `cargo build --workspace --release --locked`: exit status 0.

This is **actual prior Ubuntu workstation execution**, not GitHub CI. It should be credited and not repeated merely as a generic Rust regression. It predates the final PR #12 head and does not prove that the final provider-adapter changes were installed or that live provider endpoints were contacted. The Hermes HTTP lifecycle in this run is a test; it is not a live Hermes task. This capture does not establish live OpenHands/DeerFlow sandbox isolation, tool allowlists, current service status, dual-GPU concurrency or media artifact acceptance.

Separate previously recorded live evidence remains valid at its own timestamps: 2026-11-16 Rust API health/service discovery and the 2026-11-17 Supervisor-bridged llama.cpp inference returning `DREAD GPU INFERENCE OK`. These are historical live proofs, not a fresh current-candidate integration test. Preserve the Python Master Hub, ComfyUI and Python GPU Supervisor; do not repeat broad checks without a specific candidate-integration reason.

## Phase 5 status clarification — 2026-11-18

The current candidate now supports a limited, audited registry-removal operation: only disabled/rejected metadata can be removed; records referenced by durable tasks are protected; the prior record and operator reason are retained in a separate SQLite audit table; an authenticated history endpoint and console control expose that record lifecycle. This was verified by the new API regression test and GitHub Actions Ubuntu 24.04 workflow run #755 (44301310766).

This is not completion of Phase 5. Signed catalog/publisher verification, controlled remote source acquisition, transactional installation/update, atomic activation/rollback, executable tool/MCP manifests and their permission-review lifecycle remain unimplemented or unaccepted. Pstack source content was not deleted or changed. See WORK-LOG.md for the dated evidence and limits.


## Checkpoint refresh — 2026-11-19 (source head before this plan update: 71bdf4f23f46be877795f01718dbae1a18a1bacc)

- PR #12 is still open and unmerged. The previously recorded \`573627c204891e85d7a5bb96ecaacb8db52df995\` checkpoint is historical; the live branch advanced to \`d22d57b9282fc245f0f6525e9b544a5d8ac606ba\` before this run.
- Current-head CI #856 (\`44317429157\`) failed only at the signed-release verifier test fixture after Rust format, strict Clippy, 124 workspace tests, release build, shell syntax and Web build passed. The package job was skipped. The fake-cosign heredoc was malformed; the production verifier was not implicated. The fixture was repaired in commit \`12d86f505c6c6e85bf2b1181c372f7d3fd03be09\`; the repaired script passed a disposable local-container test. A new GitHub CI result is required before marking this gate passed.
- Phase 4 source correction: OpenHands must advertise authenticated \`/server_info.execution_runtime == "docker"\`; DREAD now requires \`usable_tools\` to include \`terminal\`, \`file_editor\`, and \`task_tracker\`, requests \`DockerExecutionWorkspace\` at \`/workspace\`, disables host worktree setup, and retains \`AlwaysConfirm\`. This aligns the adapter with merged OpenHands PR #17177 / the v1.21.0 client contract. It is still not live proof of container creation, mount isolation, egress policy or per-task tool enforcement. \`AlwaysConfirm\` is not a command allowlist.
- Hermes and DeerFlow live configuration remains unknown. Official Hermes docs state the Docker terminal backend is persistent/shared for a Hermes process and local is the default; a per-DREAD-task boundary must be demonstrated, not inferred from a generic Docker flag. DeerFlow's LocalSandboxProvider is host execution; AIO is Docker; BoxLite provides thread-scoped micro-VMs; AIO network mode can be open, isolated or allowlist. DREAD's provider flags are attestations, not actual configuration reads.
- **Physical workstation acceptance remains blocked/not refreshed.** No SSH/workstation terminal was available. Latest evidence is still the 2026-11-18 10:30 UTC user-supplied baseline (ComfyUI :8188, Rust API :8765, Supervisor gateway :11500, authenticated bridge :11502 HTTP 200; Python Hub :8080 HTTP 000; \`dread-tensor.service\` inactive). Do not represent it as current.
- No provider task, package install, service stop/restart, deployment, GPU/media workload, merge or data mutation was performed. Preserve Python Master Hub, ComfyUI, Python GPU Supervisor, models, databases and existing worktrees.
- Acceptance priority: (1) read-only workstation baseline plus real provider config/version/sandbox/tool checks; (2) confirm CI on the repaired current head; (3) live Python Hub route/payload/auth/stream/cancel/upload/output parity; (4) resource-class mission policies, timeout/cancel propagation, idempotency/retry and result aggregation; (5) signed Pstack catalog and transactional executable extension lifecycle; (6) dual-3090 contention and valid image/H3/FlashVSR artifacts; (7) signed updater, SBOM/license review and physical Ubuntu install/upgrade/reboot/rollback.


## Current acceptance checkpoint — 2026-11-19 09:28 Bahrain

### Repository and CI evidence

- PR #12 remains open/unmerged on `feature/universal-capability-discovery-clean-2026-11-17`. The last fully tested implementation head is `8d8863c7393176432d04def410639514414b6d2c`; PR run #885 (`44473460017`) and push run #884 (`44473452327`) both passed all three Ubuntu 24.04 jobs. The Rust job passed formatting, strict Clippy, 124 workspace tests, release build, installer/release verifier tests, portable migration/rollback, Python bridge/importer tests and the GPU single-owner guard. Web production build and disposable Ubuntu package/API/UI/auth smoke tests passed. The earlier #699/#698 results belong to the older `573627c...` checkpoint.
- Current code fixes in the passing head corrected two Rust formatting errors, completed the new provider policy fields in positive registry fixtures, and staged `verify-release.sh` beside `SHA256SUMS` so package assembly can complete. Later commits in this checkpoint only update this plan and work log.

### Live workstation and provider acceptance

- No authorized SSH/workstation shell was available in this run. The live workstation was not inspected or modified. The latest dated physical baseline remains 2026-11-18 10:30 UTC: ComfyUI `:8188`, Rust API `:8765`, Supervisor gateway `:11500` and authenticated bridge `:11502` returned HTTP 200; Python Master Hub `:8080` returned HTTP 000; `dread-tensor.service` was inactive. This is historical evidence only, not current status. Preserve the Python Master Hub, ComfyUI and Python GPU Supervisor.
- `config/agents.toml` is empty (`agents = []`) and `.env.example` leaves provider groups commented. Task adapters are enabled from the running Rust API process environment, not from `agents.toml`. Actual environment/configuration and credentials-presence metadata remain unknown because the running service environment could not be read.
- OpenHands source requires authenticated `/server_info.execution_runtime == "docker"`, the `terminal`, `file_editor` and `task_tracker` tools, a `DockerExecutionWorkspace` at `/workspace`, `worktree=false` and `AlwaysConfirm`. This is still not proof of per-conversation container creation, safe mounts, restricted egress or a command allowlist; the terminal tool is powerful.
- Hermes source currently accepts `modal`, `daytona`, `vercel_sandbox` and `singularity`; stock Docker is rejected. Upstream Hermes documentation describes `container_persistent: false` for per-session Docker isolation, but DREAD does not yet model or verify that configuration. DeerFlow source permits AIO or BoxLite and rejects local/remote provisioners; upstream AIO's compatibility network default is `open`, while `isolated` and `allowlist` provide stricter egress. BoxLite scopes by `(user_id, thread_id)` and can reuse a warm VM for the same thread. Actual versions, mounts, network settings and secret handling must be checked on the host.
- Every `DREAD_*_SANDBOX_POLICY_VERIFIED` value is an operator attestation, not runtime proof. Provider lifecycle and recovery tests are mock-based. Do not submit a real provider task until live versions, sandbox/workspace isolation, mount policy, egress, tool permissions, secrets, cancellation and restart recovery have been verified.

### Updated phase ledger

| Phase | Current state | Remaining acceptance |
|---|---|---|
| 0 Preservation baseline | Historical baseline exists | Fresh read-only host/service/GPU/storage capture; preserve all existing services |
| 1 Hub continuity/parity | Rust API and React console exist; legacy compatibility audit exists | Hash exact installed Python Hub; route/payload/auth/stream/cancel/upload/output parity; side-by-side and rollback |
| 2 Discovery | CPU/RAM/storage/GPU/driver/CUDA discovery and tests exist | Compare authenticated live API profile and full physical inventory on workstation |
| 3 Tasks/missions | Durable approval-gated DAG, priority dispatch, bounded global admission, retry/cancel/recovery primitives | Resource-class admission, per-node policies, provider idempotency/retry, mission aggregation and interrupted recovery |
| 4 Agent adapters | Hermes, DeerFlow and OpenHands registered and wired; mock lifecycle/recovery tests pass | Live provider version/config, real sandbox/tool/mount/egress/secret verification, cancellation and restart drills |
| 5 Extensions | Pinned Pstack metadata import/preview/enable/disable/refresh and audited registry removal | Signed catalog/publisher, source acquisition, executable tool/MCP permission review, transactional install/update/remove and atomic rollback |
| 6 GPU runtime | Rust uses authenticated Python Supervisor bridge; single-owner CI guard passes | Physical dual-3090 contention/fairness/lease recovery with ComfyUI and video; preserve Supervisor as sole GPU owner |
| 7 Media | ComfyUI present historically; media integration not accepted | Valid image, playable MiniMax H3 MP4 and FlashVSR outputs; progress, cancel, download, hashes and concurrency evidence |
| 8 Security/repair | Loopback/auth, approvals, audit and provider gates exist | Full PASS/DENY/ASK, least privilege, safe repair simulation/postconditions/rollback and restore drills |
| 9 Unified UI | Production React console and mission/capability controls exist | Full Master Hub workflow parity, sessions/checkpoints, telemetry, media and browser acceptance |
| 10 Installer/updater | `.deb`/portable package, checksum manifest, verifier and disposable package smoke tests pass in CI | Actual cosign signing/bundle publication; physical install/upgrade/reboot/service-preservation/failed-upgrade rollback |
| 11 Release | Not accepted | Signed release, SBOM/license review, live provider/GPU/media acceptance and physical Ubuntu recovery evidence |

### Next acceptance order

1. Obtain a read-only workstation baseline and inspect live provider config without exposing secret values or changing services.
2. Complete exact legacy Hub route parity and safe side-by-side tests.
3. Implement resource-class mission admission, per-node timeout/cancel, provider-specific idempotency/retry and mission aggregation.
4. Complete signed Pstack publisher/catalog and transactional executable extension lifecycle; add cosign signing and Sigstore bundle publication to the release workflow.
5. Run approved dual-3090/ComfyUI contention and validate image, MiniMax H3 MP4 and FlashVSR artifacts.
6. Complete physical Ubuntu install/upgrade/reboot/rollback, SBOM/license review and release acceptance.

CI success is not live provider, workstation, GPU/media or release acceptance. Keep PR #12 open and unmerged until every remaining gate has direct evidence.

## Provider isolation acceptance detail — 2026-11-19

- **Hermes:** upstream Docker defaults to a shared persistent container; `container_persistent: false` selects one container per session. Network egress defaults on unless `docker_network: false`; host mounts, `docker_extra_args` and forwarded env can weaken the boundary. DREAD currently rejects Docker and must not accept it until task-to-session/container identity, mount restrictions, network policy and secret forwarding are verified. [Upstream configuration](https://github.com/NousResearch/hermes-agent/blob/main/website/docs/user-guide/configuration.md).
- **DeerFlow:** AIO DooD may mount the host Docker socket into the Gateway; the socket is root-equivalent. AIO network defaults to `open`; strict mode is `isolated` or a reviewed `allowlist`, and the current AIO image retains compatibility capabilities and defaults to `seccomp=unconfined`. The Kubernetes provisioner avoids the host socket but is currently rejected by DREAD's non-empty-provisioner gate. BoxLite is scoped by `(user_id, thread_id)` and can reuse a warm VM. Verify socket, bind host, mounts, egress, image, seccomp, capabilities and resource limits before use. [Upstream configuration](https://github.com/bytedance/deer-flow/blob/main/backend/docs/CONFIGURATION.md).
- **OpenHands:** inspect authenticated `/server_info`, `X-Session-API-Key`, stable `OH_SECRET_KEY`, loopback binding, actual per-conversation Docker containers, mounts, egress and requested tools. A tool-confirmation policy is not a command allowlist. [Agent Server security/API guide](https://github.com/OpenHands/docs/blob/main/sdk/arch/agent-server.mdx).
- These are source-level requirements only. No live provider config, container, mount, key or permission was inspected in this checkpoint; no provider task is approved.


## 2026-11-21 — Universal continuation and capability-discovery reconciliation

The 20 November offline continuation archive is treated as a feature source derived from Run 81, not as a replacement for the later Phase 6/PR #13 source. Its safe, additive capabilities have been selectively integrated on stacked draft PR #14. PR #12 and #13 remain separate and unmerged.

Implemented in this continuation:
- Reframed “Universal” as hardware-/capability-adaptive dual-/multi-GPU PCs rather than a DREAD installation tailored to the original RTX 3090 workstation or its /mnt/sabrent path. Ubuntu 24.04 remains the current validated CI/package target. Other OS/accelerator combinations are not advertised as supported until their adapters and acceptance tests pass.
- Added configurable and portable discovery roots for local model files, ComfyUI workflow exports, plugin manifests and agent adapter manifests. Environment/config, install/current-relative and per-user data roots replace the original fixed workstation path as defaults.
- Added local model-file inventory, separate from gateway-advertised models; discovered files are never reported as loaded or executable.
- Added bounded read-only ComfyUI monitor and workflow metadata discovery. It reports queue/history/output metadata without synthetic progress, does not submit/cancel jobs, and never echoes workflow widget values or payloads.
- Added Cursor/Claude/generic plugin manifest catalogue. Results remain untrusted and disabled; no hooks, scripts, commands or MCP servers are started.
- Added local agent adapter manifest catalogue. It validates loopback health URL declarations but does not probe those URLs, start agents, activate adapters or trust declared capabilities.
- Added proposal-only repair diagnostics: every finding is proposed_not_applied, operator approval is required and auto-apply is ineligible. This is a first diagnostic layer, not the complete self-repair lifecycle.
- Added authenticated API routes and a separate operator inventory workspace. Disk inventories are not run on the five-second general dashboard polling loop.
- Added bounded scan, symlink refusal, malformed manifest, loopback validation, disabled-default and route-auth regression tests.
- Added docs/IMPLEMENTATION-GAP-MATRIX.md and updated this controlling plan/README to require every future change to be checked against the master-plan rows.

New protected routes:
- GET /api/models/local
- GET /api/media/comfyui/monitor
- GET /api/media/workflows/discover
- GET /api/extensions/discover
- GET /api/agents/manifests/discover
- GET /api/repair/plan

Safety and release boundary:
- No live workstation was accessed, and no local package was installed.
- No service was stopped/restarted; no ComfyUI prompt, provider task or GPU workload was submitted.
- No model was loaded/unloaded and no GPU lease was acquired.
- The Python Master Hub, ComfyUI and Python GPU Supervisor were not changed.
- Rust CI run #972 passed the Web console build but failed the Rust formatting gate, so Clippy, Rust tests, release build and package smoke test were skipped. The reported rustfmt changes and a crate-level doc-comment ordering compile issue were corrected in subsequent commits. Those corrections have not yet been validated by a fresh green current-head CI run.
- Latest inspected PR #14 source head at this log entry: 30dba903ac2b4e588a96f3778cfb92b74b6fa15b. PR #14 remains a draft stacked continuation, not merged. Do not claim its new Rust code compiled or passed tests until a later CI run confirms it.
- Still incomplete: full GPU stable identity/heterogeneous scheduling, GGUF/projector/context metadata, live agent adapter enablement/verification, executable extension transaction/signatures, WebSocket media progress and artifact pipelines, resource-aware mission scheduling, repair simulation/apply/postcondition/backup/rollback, Python Hub route parity, signed updater and physical host acceptance.


### Multi-GPU identity slice — 2026-11-21

- `dread-platform::GpuDevice` now carries `device_id`, `device_index` and `pci_bus_id` as separate optional values. NVIDIA discovery requests UUID and PCI bus address with the telemetry query; legacy query fallback still reports a device by name/VRAM with identity fields unknown.
- AMD `rocm-smi --showproductname` records its reported GPU index where present; the current adapter does not invent UUID or PCI address. Apple `system_profiler` records the ordinal it reports through enumeration; it does not claim a stable hardware identifier.
- The host profile and console display index, provider ID and PCI bus address separately. The index is explicitly an enumeration/display index, not a stable key.
- Unit tests cover two RTX devices with distinct UUIDs/PCI addresses and a driver response that lacks compute capability. More vendor-specific identity coverage remains required.
- This closes only the stable identity slice for NVIDIA reporting. Intel enumeration, AMD stable IDs/PCI address, persistent identity across hardware reordering, heterogeneous scheduling and actual multi-GPU contention remain open.


### Bounded GGUF metadata inventory — 2026-11-21

- Local `.gguf` files are now inspected through a bounded metadata-only parser that validates GGUF magic/version and reads the selected metadata keys. It does not read tensor records or weight payloads and does not load the model.
- Header parsing has a 1 MiB per-file scan cap, 16,384 metadata-entry cap, 1,000,000 array-element cap, 64 KiB string-value cap and 64 GGUF files per inventory scan. Oversized, malformed or truncated headers remain visible as local candidates with `metadata_status=unavailable` and a bounded error rather than being represented as usable models.
- Exposed metadata includes the raw GGUF file-type code and quantization version; the file-type code is deliberately not translated into an inferred quantization label. Compatibility, tokenizer/projector dependency checks, VRAM fit, residency and gateway-load acceptance remain separate requirements.


### Host-root and model-cache portability slice — 2026-11-21

- Model, workflow and plugin root discovery now consults `USERPROFILE` in addition to `HOME`; Windows-style per-user data roots (`LOCALAPPDATA`/`APPDATA`) are included where present. Explicit `DREAD_*_ROOTS` configuration remains authoritative. These discovery conventions do not constitute Windows runtime/installer support.
- Local model inventory now resolves symlink targets and follows only canonical targets that remain under the configured model root. Canonical directories/files are deduplicated, so symlink cycles/duplicate snapshot aliases do not cause repeated scans. External symlink targets and broken links are ignored.
- Added a Hugging Face cache regression test for `snapshots/<revision>/model.safetensors` pointing to an in-root `blobs/<hash>` file. The scanner preserves the visible snapshot filename for format classification and API display while using the canonical file for containment, size and metadata.
- The existing external-link rejection test remains in place. Workflow/plugin manifest scanners continue to reject symlinks entirely because their manifests are not needed through cache-style aliases.
