# PROJECT DREAD UNIVERSAL — Work Log and Handoff

**Updated:** 2026-11-17  
**Read together with:** `MASTER-PLAN.md`

## 1. Product constraints
- Ubuntu 24.04 LTS and later Ubuntu releases only.
- `dread_web_hub.py` remains the Master Hub during incremental migration; do not replace it with a disconnected product.
- Preserve ComfyUI, Python GPU Supervisor, authenticated bridge, models, configs, databases and user data.
- llama.cpp is active inference runtime; Ollama must not be started as active server.
- `dread_gpu_supervisor.py` is sole GPU lease/process authority.
- Backed-up additive changes only; verify real behavior.

## 2. Repository and PR state
- Repository: `dreaddo151-sys/koko`.
- PR #12: https://github.com/dreaddo151-sys/koko/pull/12
- Branch: `feature/universal-capability-discovery-clean-2026-11-17`.
- Base: `work/futuristic-command-center-2026-11-17`.
- Last checked: open, not merged, mergeable true; 37 commits, 19 files, 1,822 additions, 112 deletions.
- Last checked head: `2168dc5366b9edc161e4940193751528b084e69e`.
- PR adds read-only host discovery, llama.cpp gateway config, OpenHands/LangGraph health probes, Pstack skill/profile discovery/import/preview/enable/disable and UI controls.
- No PR #12 workflow run/status was reported at that check. Do not claim its new source passed CI.
- GitHub branch edits are not a workstation install.

## 3. Workstation paths and services (historical; recheck before action)
- Workspace: `/mnt/sabrent/dread_workspace`.
- Rust development checkout: `/home/draead/dread_rust_redesign/dread_universal`.
- Live API checkout: `/home/draead/dread_rust_stage/dread_universal`.
- Legacy Hub `dread_web_hub.py`, historically port 8080; later observed offline, so do not report it online without a current check.
- ComfyUI `127.0.0.1:8188`; preserve.
- GPU Supervisor gateway `127.0.0.1:11500`.
- Authenticated bridge `127.0.0.1:11502`, systemd user service `dread-gpu-supervisor-bridge.service`.
- Rust API `127.0.0.1:8765), systemd user service `dread-universal-api.service`.
- Managed llama.cpp tensor server `127.0.0.1:11501), on demand through `dread-tensor.service`.
- Port 8000 was associated with OpenHands/agent-canvas ingress; health response does not prove task adapter.
- Dual NVIDIA RTX 3090 detected. Historical PCIe links were below capability; do not change BIOS blindly.
- Pstack local source previously at `/mnt/sabrent/dread_workspace/integrations/cursor-plugins/pstack`.

## 4. GPU inference proof
At a prior workstation checkpoint, Rust API inference through authenticated bridge returned HTTP 200 and exact content `DREAD GPU INFERENCE OK`, finish reason `stop`. Path: Rust API → authenticated bridge → Python GPU Supervisor lease → managed llama.cpp tensor server. This proves that specific path at that checkpoint, not the whole product. Never launch llama-server manually, call tensor upstream directly from Rust or bypass Supervisor.

## 5. Prior automated verification (before current PR #12)
Earlier development checkout:
- `cargo fmt --all -- --check`: PASS.
- `cargo check --workspace --locked`: PASS.
- `cargo test --workspace --locked`: PASS, 50 tests.
- `cargo build --workspace --release --locked`: PASS.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: PASS.
- Frontend `npm run build`: PASS.
These predate PR #12 and do not validate its new source.

Earlier canonical-source GitHub Actions run `44077407357`:
- Pstack provenance, 11 importer tests, Rust format, Clippy, 50 Rust tests, release build, React/TypeScript build, four Python Supervisor bridge tests, GPU single-owner guard, authenticated live Pstack API import/readback/idempotency and source inventory hashing passed.
- All 60 Pstack records remained disabled.
- Browser-level live API acceptance and workstation deployment remained outstanding at that checkpoint.

Earlier WebSocket network test `44025427057`: authenticated handshake, replay, live delivery and reconnect with sequence cursor passed.

## 6. Pstack provenance
- Version 0.15.43; commit `2f87c854fe3e2fcda44ca86b46ede35aad3c46b7`; subtree `71d8e1d1d718e2149dc43b2535618064f29cba6b`; MIT.
- Inventory: 58 skills and 2 Markdown agent prompt profiles.
- Markdown profiles are context profiles, not live agents. Current PR loader attaches approved profiles as bounded untrusted context; it does not execute scripts or launch agents.
- The initial PR #12 implementation used FNV-1a. This has now been replaced in the current branch with SHA-256 fingerprints using `ring` 0.17. SHA-256 detects content changes; it is not a publisher signature or proof of trusted provenance. Signed catalog provenance remains future work.

## 7. Legacy Hub regression findings (2026-11-08 audit)
- 44-route API inventory.
- Build missions SQLite-persistent; some interactive jobs in-memory.
- `/api/task/*` and `/api/approval/*` empty-reply defects.
- A permission path involving `/etc/shadow` resolved to ALLOW; must fail closed.
- Live PASS/DENY/ASK evidence and `DREAD_CURRENT_CHECKPOINT.md` were absent at that audit.
- MiniMax H3 assets/custom nodes existed but valid MP4 not verified.
- LTX disabled at that audit.
These are historical findings; retest to establish current state.

## 8. Current candidate limitations
- Host profile has OS, architecture, CPU threads, GPU metadata and llama-server executable discovery; RAM/disk/driver inventory incomplete.
- Agent discovery is read-only health probes, not task dispatch.
- Pstack prompt profiles are not live AgentAdapters.
- SHA-256 fingerprints are now implemented for Pstack skill and agent-profile content. They are integrity fingerprints only; trusted publisher signatures and signed catalog verification remain incomplete.
- UI bundle is not embedded/served by Rust API; production must not require Vite dev server.
- Transactional installer/updater/rollback incomplete.
- Hermes/OpenHands/DeerFlow task adapters not all implemented/verified.
- Mission planning/delegation, resource-aware scheduling, self-repair, full legacy Hub parity and real H3/FlashVSR artifact acceptance remain incomplete.
- Cross-platform workflow targets are out of scope; Ubuntu-only CI/release is required.

## 9. Immediate execution order
1. Change CI/release to Ubuntu 24.04 only; keep PR #12 unmerged until Ubuntu CI passes.
2. Reconcile canonical source with live/development checkouts; record exact commit and dirty state.
3. Fix legacy Hub empty replies and permission fail-open behavior with regression tests.
4. Integrate Rust task/event/registry APIs into existing Master Hub without breaking routes.
5. Complete host/model/service discovery and persistent registry.
6. Implement Hermes/OpenHands/DeerFlow AgentAdapters and mission delegation.
7. Complete Pstack/Cursor extension manager with SHA-256 provenance and safe transactional install/update/rollback.
8. Prove GPU single-owner behavior under concurrent chat, ComfyUI and video.
9. Produce real image, MiniMax H3 video and FlashVSR outputs.
10. Complete Ubuntu installer, systemd, packaged UI, updater and rollback.
11. Run clean Ubuntu 24.04 install/upgrade tests and dual-3090 workstation acceptance.

## 10. Reporting rules
Every handoff separates:
- **Verified complete** — actual command output, CI run or artifact.
- **Implemented, unverified** — source exists but functional acceptance is missing.
- **Blocked** — exact blocker and evidence.
- **Next action** — one concrete implementation step.

Do not describe the platform as complete until Master Plan acceptance gates pass. Do not claim Codex, another model or an external agent was used unless its execution tool actually ran.


## 11. Live continuation — 2026-11-17

- Current source branch head after fixes: `0a2553bfd778424c268cb9fd8a9bb278d32f4037`.
- PR #12 remains open and unmerged. CI run #131 (`44144854217`) validated the merge commit for PR #12:
  - Web console build: PASS.
  - Rust formatting: PASS.
  - Rust Clippy with warnings denied: PASS.
  - Rust tests: FAIL in `dread-skills::tests::loads_only_exact_catalog_ids`.
- Failure root cause: `load_skill_context` joined a safe relative catalog path to the canonical absolute root and then rejected the resulting absolute path because its components necessarily include `RootDir`. This made every legitimate skill load fail with `UnsafePath`.
- Corrected in commit `0a2553bfd778424c268cb9fd8a9bb278d32f4037`: validate the descriptor path while it is still relative, then re-check the joined file with `symlink_metadata`, size bound, canonical path containment and UTF-8 validation. The existing regression test covers the successful exact-ID load; symlink rejection remains covered separately.
- New CI run #132: `44145187972`, queued at last check. Its result is not yet known.
- OpenHands Agent Server contract was rechecked against the current upstream Agent SDK OpenAPI schema. Local API surface includes `POST /api/conversations`, `GET /api/conversations/{id}`, `POST /api/conversations/{id}/events`, event search/count, run, pause, delete and WebSocket events. Local conversation creation requires an Agent configuration and a workspace; LLM and tool definitions are part of that request.
- Security/design gate: OpenHands `LocalWorkspace` executes tools against a host working directory. Enabling TerminalTool/FileEditorTool without a constrained workspace and explicit DREAD policy would grant direct host filesystem/command access. Also, allowing OpenHands to configure its own LLM endpoint could bypass the Python GPU Supervisor. Therefore do not mark the OpenHands adapter enabled or dispatch host-mutating tasks until the workspace isolation, confirmation policy, credentials and Supervisor-routed LLM path are proven. Health-only probes remain discovery evidence, not execution acceptance.
- The current branch has not been installed on the workstation. No local Ubuntu acceptance, real agent task, H3 MP4 or FlashVSR artifact is claimed by this continuation.


### CI gate closed — 2026-11-17 (run #135)

- GitHub Actions run `44145235574` completed successfully for the PR validation merge commit.
- Rust / Ubuntu 24.04 job `134258451360`: formatting PASS, strict Clippy PASS, complete Rust test step PASS, release build PASS.
- Web console / Ubuntu 24.04 job `134258452211`: production console build PASS.
- This validates the tested PR merge-commit snapshot. It does not prove local workstation installation, browser-to-API integration, real external agent execution, media output, or clean-machine packaging.

### Task execution audit — source inspection after CI

- The Rust API has durable task records, approval, queued/running/completed/failed/cancelled states, event persistence/streaming, result persistence, bounded concurrency/admission and restart recovery logic.
- Currently reviewed built-in runners are only `service_discovery` and `gpu_chat`. Capability-backed tasks are explicitly rejected until a reviewed adapter is registered. Unknown task types fail closed. GPU chat validates payloads and calls the authenticated Python GPU Supervisor client; no direct llama.cpp endpoint is used in this path.
- Recovery marks interrupted running tasks failed without replay. It only considers approved queued `service_discovery` tasks eligible for recovery; other queued tasks are rejected for operator review. Verify the continuation/requeue behavior in the remaining startup code before claiming recovery execution.
- No concrete Hermes/OpenHands/DeerFlow adapter is present yet. Agent discovery remains health-only. OpenHands remains gated on constrained workspace execution, tool policy and Supervisor-routed LLM configuration.
- Immediate next engineering gate: implement a reviewed, testable provider-neutral agent execution adapter and API task lifecycle integration behind explicit opt-in, while preserving the fail-closed default. Start with a mock HTTP contract test; do not enable host-mutating tools or point an agent at an unrestricted local workspace.


### SHA-256 provenance fingerprint implementation — 2026-11-17

- Replaced the FNV-1a fingerprint in `dread-skills` with SHA-256 via the already locked `ring 0.17.14` crate. Added a known-answer unit test for SHA-256(`abc`) and updated the discovery assertion to require a 64-hex-character digest.
- Added `ring` as a workspace dependency and locked dependency for `dread-skills`; no new crate version was introduced into `Cargo.lock` because `ring 0.17.14` was already present transitively.
- Commits: `ddb79ea` (workspace dependency), `6858aec` (crate dependency), `0913242` (implementation and regression test), `b214ed4` (lockfile).
- CI run #143 (`44146062714`) was triggered for SHA `b214ed450b62406561bf1d050e835c8c68b091bb` and was pending at the time of this log update. Do not call the SHA-256 change validated until this run completes.
- This is a content fingerprint, not a digital signature. Extension installation still needs signed/pinned catalog verification, staged activation, rollback and audit logging.


### Exhaustive phase-gap audit — 2026-11-17 (candidate source review)

The current candidate was reviewed against all phases in `MASTER-PLAN.md`. This is a source audit, not a claim of workstation acceptance.

#### Newly found and corrected
- The Python GPU Supervisor bridge used the single-threaded `HTTPServer`. A long-running inference request could block `/health` and every other bridge request for the full generation duration. Replaced it with `ThreadingHTTPServer`; the Python GPU lease remains the sole serialization/ownership mechanism.
- Added an HTTP-level regression test that holds a mocked inference open and proves `GET /health` still returns while that request is active.
- Added Python bridge/importer regression tests and the static single-owner GPU boundary check to Ubuntu CI. These checks were present in the repository but were not previously part of the main CI gate.
- Removed 22 accidental tracked `*.backup_*` / `*.orig` working copies from the active source tree and added `dread_universal/.gitignore` for local backup snapshots, secrets, runtime data, Rust output and web build output. Git history remains available for recovery.

#### Phase-by-phase findings

- **Phase 0 — Preservation:** historical service evidence exists, but there is no fresh, machine-readable baseline generated from the live workstation in this candidate. Must verify current Hub, ComfyUI, Supervisor, bridge, Rust API, model paths, user data and systemd state before installation.
- **Phase 1 — Hub continuity:** Rust API and React console exist, but the React console is not yet embedded into the existing Python Master Hub. No full legacy feature-parity matrix or migration/rollback rehearsal exists.
- **Phase 2 — Discovery:** host profile currently reports OS/architecture, CPU thread count, selected GPU metadata/utilities and llama-server path. RAM, storage capacity/free space, kernel, driver/CUDA versions, PCIe link state, GPU utilization/temperature/power and model-file inventory are not yet implemented. Host profile is process-cached and therefore will not reflect hardware changes until restart.
- **Phase 3 — Task kernel:** SQLite tasks, approval, events, bounded execution, cancellation signalling and conservative recovery exist. Mission dependency DAGs, priorities, durable schedules, operator identity/auth integration with the legacy Hub, and long-payload round-trip tests remain incomplete. Cancellation of GPU chat cancels the Rust request, but the synchronous Python bridge/upstream may continue holding the GPU lease until its request completes or times out; this must be resolved or explicitly exposed as cancellation-pending.
- **Phase 4 — Agent adapters:** only provider-neutral traits and health-only discovery exist. No Hermes/OpenHands/DeerFlow start/status/events/cancel/result adapter is registered. OpenHands must use an isolated workspace, strict tool allowlist, session-key authentication, stable secret handling and DREAD-controlled LLM routing before activation. Current official Agent Server guidance confirms the session-key header and warns that workspace tools can read/write files and execute commands; see official OpenHands Agent Server guide: https://github.com/OpenHands/docs/blob/main/sdk/arch/agent-server.mdx
- **Phase 5 — Extensions:** Pstack profiles are discovered/imported, previewed, enabled/disabled and loaded as bounded untrusted model context; SHA-256 fingerprints now detect source changes. There is no transactional install/update/remove lifecycle, atomic activation/rollback, signed publisher/catalog verification, or reviewed executable tool/MCP manifest runtime. SHA-256 content hashing is not a publisher signature.
- **Phase 6 — GPU:** Rust submits chat through the authenticated bridge; Python Supervisor owns the lease and managed tensor process. Bridge now serves requests concurrently while the existing lease controls GPU access. Actual dual-3090 concurrent workload, fairness, lease timeout/recovery and current workstation inference must be re-tested. The Rust gateway health/models path and bridge upstream must be kept distinct to avoid nested lease acquisition.
- **Phase 7 — Media:** ComfyUI discovery exists, but no integrated DREAD image/video job adapter, MiniMax H3 validation, FlashVSR integration, progress/cancel path, or real playable MP4 artifact acceptance is present in this candidate.
- **Phase 8 — Security/self-repair:** loopback-only API, bearer auth, explicit approval and disabled-by-default imported Markdown exist. No complete PASS/DENY/ASK permission engine, least-privilege sandbox, repair simulation/postcondition engine, signed update trust chain or restore/rollback drill is implemented.
- **Phase 9 — Unified UI:** the React console exposes system/task/GPU/model/agent/capability views. The Rust API now serves the compiled React console on the same origin via a bounded static-file fallback with traversal and symlink rejection. Legacy Hub parity, multi-session/project/checkpoint experience, agent execution UX, media output controls and complete telemetry remain incomplete.
- **Phase 10 — Installer/updater:** added Ubuntu `.deb` and portable `.tar.gz` packaging, SHA-256 manifest, hardened per-user systemd unit, first-run configuration helper, and a portable installer/updater that migrates legacy app-local config/SQLite data, backs up the previous application tree, health-checks and rolls back on failure. CI now assembles both packages, checks the manifest and archive layout, installs the `.deb` on a disposable Ubuntu runner, and smoke-tests the API, static console and auth boundary. These are implementation changes awaiting the new CI run; no package is yet installed on the user's workstation. `.deb` upgrade rollback and signed release provenance remain incomplete. `web/package-lock.json` is absent, so frontend transitive dependency resolution is not yet locked/reproducible.
- **Phase 11 — Acceptance:** current CI previously passed Rust formatting, strict Clippy, Rust tests, release build and web build on run #147 at SHA `d51f488`. The newer CI now additionally runs Python bridge/importer tests and the GPU ownership boundary guard; its result must be checked before considering that expanded gate green. No current workstation acceptance or release artifacts have been produced.

#### Additional regression/acceptance cases required before release

1. Clean Ubuntu 24.04 install and in-place upgrade with existing Hub/ComfyUI/Supervisor left running.
2. Reboot persistence for all systemd user services; no duplicate llama-server or second GPU lease owner.
3. API auth missing/invalid/valid, WebSocket auth/reconnect, loopback bind rejection, oversized bodies and malformed JSON.
4. Task approval single-use, cancellation at each state, process restart during inference, duplicate dispatch prevention and recovery audit.
5. 5 KB / 10 KB / 20 KB prompt round trips and response truncation/size boundaries.
6. Two concurrent inference requests with lease fairness, ComfyUI image generation during inference, video generation while the Supervisor manages model lifecycle, and bridge health responsiveness.
7. Imported skill symlink/path traversal, TOCTOU, oversize, invalid UTF-8, tampered source, stale revision, malicious prompt injection, disabled/enable/disable and restart persistence.
8. OpenHands adapter mock contract tests for auth, timeout, unknown event kinds, idempotency, cancel, result retrieval, workspace escape attempts and forbidden tools before any live endpoint is enabled.
9. Real image file validation, playable MP4 decode/metadata validation, interrupted render cleanup and FlashVSR output comparison.
10. Installer signature/checksum verification, interrupted install/update, DB migration failure, rollback, uninstall preserving user data, and no Vite server in production.

#### Current implementation activity after this audit
- Bridge threading fix: commit `72c05aaf85e1e5d45c132bb9bdb5764607a4e458`.
- Health-during-inference integration regression test: commit `2546afb723475a6375b1525c6410ae90a98f9e7f`.
- CI expansion (Python tests + GPU ownership boundary): commit `52b770c8f83a7f21f313c20764a9df1c5c0e7b32`.
- Source-tree cleanup and ignore rules: commits `41dba3afaf406b4c986382567a06d22f7a9339ba` and subsequent cleanup commits through `fc811b466dfa847776eebd5c8ecf657eddd22935`.
- Latest test run for bridge concurrency and expanded CI is pending; do not report the new test as passed until CI completes.

### Production console and Ubuntu packaging implementation — 2026-11-17

- Added same-origin production React console serving from `DREAD_WEB_ROOT` (default `web/dist`). The fallback returns static assets only from a canonical configured root, rejects parent/root components, symlinks and files larger than 16 MiB, sets explicit content types and refuses unknown `/api/*` paths. Added unit coverage for index, JS, traversal, API fallback and symlink rejection.
- Added per-user systemd service unit at `systemd/dread-universal.service`; it binds loopback, uses a private per-user env file, stores SQLite under the user's data directory, serves static files from `/usr/share/dread-universal/web/dist`, and applies service hardening without blocking read-only NVIDIA discovery.
- Added `scripts/package-ubuntu.sh` to assemble an Ubuntu `.deb`, portable `.tar.gz` and SHA-256 manifest from prebuilt API/web artifacts. The Debian package installs the binary, web assets, config templates, bridge helper and user unit; it does not auto-start or stop any existing DREAD/ComfyUI/Supervisor service.
- Added `scripts/configure-user.sh` for first-run token/config creation and user-service restart after package upgrades. Added `scripts/install-user.sh` for portable installation/upgrades: it stops only its own DREAD API service before touching SQLite, refuses to overwrite an unmanaged API on port 8765, migrates app-local config/database and known Supervisor credentials, keeps user config/data outside the release tree, backs up the prior app directory, health-checks and restores the previous app on failure.
- Added `scripts/test_install_user.sh` with mocked systemd/HTTP controls to exercise legacy config/database/credential migration, restrictive env-file permissions, successful update backup and forced-health-failure rollback.
- Expanded CI with Python bridge/importer tests, GPU ownership boundary guard, shell syntax validation, portable installer migration/rollback test, artifact transfer from Rust/web jobs, Ubuntu package assembly, SHA-256 verification, archive checks, disposable `.deb` installation and live API/static UI/auth smoke tests.
- Changed CI concurrency groups to isolate workflow event/PR branch runs rather than cancelling unrelated branches.
- Updated release workflow to create `.deb`, `.tar.gz` and checksum assets from a selected Ubuntu 24.04 commit. It is not a signed release and has not yet been run to publish a tag.
- Commits for this workstream include `6460d7a`, `84e8f3c`, `52fc561`, `35e47a1`, `53d422c`, `8848381`, `074e784`, `a40037a`, `8c48fb8`, `d701116`, `79d000b`, `ca42feb`, `f40a16d`, `0fefc1e`, `217fb69`, `e0af073`, `1340206`, `d60711a`, `0c1126a`, `9f59e9f`, `14d5ffc`.
- Current CI validation run #243 (`44155642033`) is still in progress. Do not report packaging, installer rollback, or bridge concurrency tests as passed until this run completes.
- Remaining packaging gaps: no committed npm lockfile; `.deb` upgrade rollback is not transactional; release checksums are not publisher signatures; clean Ubuntu 24.04 install/upgrade/reboot and user's dual-3090 workstation acceptance are still outstanding.

## 2026-11-17 — PR #12 remaining-phase audit

Reviewed the controlling master plan, agent contract, skills importer, host discovery, task kernel, installer/package scripts, web console and PR metadata against the current feature branch.

- Candidate HEAD: 2418155aaf268b136158c7dd8008aff79efe3aac (after workflow fixes).
- Corrected CI and release workflow package-script invocation to use Bash explicitly. Commit sequence: CI workflow fix a78c4ff954dd611729c363c0807d336367088319; release workflow fix and current HEAD 2418155aaf268b136158c7dd8008aff79efe3aac.
- Run #306 (44160690656): web build, formatting, Clippy and Rust tests passed at last poll; release build still running. Remaining steps were not yet evidenced as passed.
- Concrete provider agent adapters are still absent. The Rust trait is a contract only. Do not claim Hermes/OpenHands/DeerFlow execution.
- Mission DAG/priority/resource scheduling, complete extension transactions/signatures, full Hub continuity, media artifact pipelines, complete repair policy, signed updater and workstation acceptance remain outstanding.
- Current audit details and updated phase ledger are appended to MASTER-PLAN.md. It explicitly supersedes stale branch/run/count statements earlier in that document.
- No Ubuntu workstation commands were run and no live services or user data were changed.

Next safe engineering sequence: wait for run #306 and repair any concrete CI failure; then implement a single bounded provider adapter against a verified local agent API contract, with mocked contract tests and an opt-in live test. In parallel, design Hub integration around the existing Python route inventory rather than replacing it. Do not fabricate agent endpoints or enable imported Pstack code.


### CI evidence refresh — 2026-11-18

- Candidate HEAD `116406e7eb99dc89c3da90985e6bf9b1248b9960` was checked against GitHub Actions run #316 (`44169900862`).
- **All three jobs completed successfully:** Web console / Ubuntu 24.04; Rust / Ubuntu 24.04; Ubuntu packages / smoke test.
- Rust job passed formatting, strict Clippy, workspace tests, optimized release build, installer shell syntax, portable migration/rollback tests, Python bridge/importer regression tests, and the single-owner GPU boundary check.
- Web job passed committed npm lockfile verification, `npm ci`, and production console build.
- Package job assembled the `.deb` and portable archive, verified checksums and archive contents, installed the package in a disposable Ubuntu 24.04 runner, started the packaged API, checked the production UI, verified unauthenticated API rejection (401), and verified authenticated host-profile access.
- This supersedes earlier statements in this log and the master-plan addendum that run #306 or expanded packaging/installer checks were still pending. Earlier historical run notes remain historical.
- **Scope of evidence:** this is GitHub-hosted Ubuntu CI only. It does not prove installation, upgrade, reboot persistence, preservation of live DREAD/ComfyUI/Supervisor services, dual-RTX-3090 operation, real inference, agent task execution, image/video output, or workstation rollback.
- Remaining product gaps are unchanged: live Python Hub parity/integration, concrete provider task adapters, mission DAG/delegation, transactional signed extensions, full inventory, live GPU concurrency, ComfyUI/H3/FlashVSR artifacts, complete permission/sandbox/self-repair lifecycle, signed updater and workstation acceptance.
- PR #12 remains open and unmerged. No Ubuntu workstation services or user data were touched by this GitHub-only evidence refresh.


## Workstation evidence reconciliation — 2026-11-18

Recovered prior terminal/API evidence and corrected the acceptance accounting. This records historical checkpoints; it does not claim the current PR candidate is deployed.

- **2026-11-08:** DREAD Hub :8080, ComfyUI :8188, Ollama API :11434 and OpenCode :4096 were exercised. Recorded endpoints included ComfyUI `/system_stats`, Ollama `/api/tags` and `/api/ps`, DREAD `/api/hub/vram` and `/api/telemetry`. `qwen3.8:27b` was resident (about 29.3 GB VRAM reported); two RTX 3090s were detected. This is historical API/hardware evidence; final architecture still requires llama.cpp, not Ollama as active inference server.
- **2026-11-16:** Rust API `127.0.0.1:8765/health` returned HTTP 200, version 0.1.0; `/api/services` returned nine discovered services. ComfyUI, GPU Gateway, Hermes and OpenHands ingress/server were reported reachable. DREAD Hub :8080, Ollama :11434, llama.cpp :11501 and OpenCode :4096 were unreachable at that checkpoint. Health/reachability is not agent task-execution proof.
- **2026-11-17:** dual-RTX-3090 inference through the Supervisor was previously proven. Rust API → authenticated bridge → Python GPU Supervisor lease → managed llama.cpp returned HTTP 200 and exact response `DREAD GPU INFERENCE OK`.
- ComfyUI `/system_stats` and model/node presence were checked. A valid integrated image artifact, playable MiniMax H3 MP4 and FlashVSR output were not established by those checks.

### Revised retest policy

Do not repeat basic GPU enumeration, generic API health or standalone llama.cpp inference as if no prior tests existed. Before workstation deployment, capture a fresh preservation baseline. For the new candidate, perform a narrow integration regression through the existing authenticated bridge and Supervisor-owned inference path, including health responsiveness during inference. Test concurrent dual-GPU chat/ComfyUI/video only when the integrated media jobs are implemented. Preserve all existing service and GPU ownership boundaries.


## Mission planning API vertical slice — 2026-11-18

- Added SQLite persistence for mission graphs and deterministic execution order.
- Added authenticated `POST /api/missions` and `GET /api/missions/{id}` endpoints.
- The planning endpoint rejects empty/oversized graphs, oversized node fields, missing dependencies, self-dependencies, duplicate node IDs and dependency cycles through the existing graph validator.
- Added API regression tests for ordering, persistence/readback, duplicate mission IDs, cycle rejection and the 128-node limit.
- Safety boundary: plans are stored with status `planned`; API explicitly returns `execution_started: false`. No task dispatch, agent invocation, tool execution or GPU activity is triggered by mission planning.
- Source commit: `1d04452c05739a759988bedb40ee638debc32909`.
- CI runs #336/#337 were queued for this commit at the time of this entry. Do not mark this vertical slice CI-verified until the run for this exact SHA completes.
- This is a mission-planning API slice, not mission orchestration completion. Dependency-aware scheduling, task creation/aggregation, failure propagation, retries, pause/resume and provider delegation remain outstanding.


## Expanded CI acceptance — 2026-11-18 (run #343)

- Candidate HEAD tested: `924cda887479741b67168f20524863b84e29bf49`.
- GitHub Actions run #343 (`44184194589`) completed with **all three jobs successful**: Web console / Ubuntu 24.04; Rust / Ubuntu 24.04; Ubuntu packages / smoke test.
- Rust job passed `cargo fmt --all -- --check`, strict workspace Clippy, workspace tests (including mission planning persistence/order/validation cases), optimized locked release build, installer shell syntax, portable installer migration/rollback regression tests, Python bridge/importer tests, and the single-owner GPU boundary guard.
- Web job passed committed lockfile verification, `npm ci`, and production console build.
- Package job assembled the Ubuntu `.deb` and portable `.tar.gz`, verified SHA-256 manifest and archive contents, installed the `.deb` in a disposable Ubuntu 24.04 runner, started the packaged API, checked production UI serving, verified unauthenticated API rejection (401), and authenticated host-profile access.
- The earlier formatting-only failure during this implementation was corrected; the final run passed formatting and all downstream gates.
- This proves CI behavior for the exact candidate SHA above. It does not prove clean physical workstation install/upgrade/reboot, preservation of the running legacy Hub/ComfyUI/Supervisor, live dual-3090 operation, provider task execution, real image/video artifacts, or rollback on the user's machine.
- PR #12 remains open and unmerged. No live workstation service or user data was modified by this GitHub-only work.
- Mission execution/provider delegation, concrete Hermes/OpenHands/DeerFlow adapters, full legacy Hub parity, signed/transactional Pstack extension management, media pipelines, complete security/repair lifecycle, signed updater, and live workstation acceptance remain open.


## Installer rollback and GPU telemetry UI hardening — 2026-11-18

- Fixed portable installer rollback so a failed update restores the prior application tree, credential file, systemd unit file, and prior enabled/active service state.
- Fixed failed first-install cleanup: disable the newly enabled unit before removing its unit file, so a systemd enablement symlink is not left behind.
- Added regression tests for both failed upgrade (byte-for-byte restoration of unit and credential file, previous binary restored, service remains enabled/active) and failed first install (no app tree or unit left; service disabled).
- Corrected the React GPU cards to display the host telemetry already returned by `/api/system/profile`: utilization, VRAM used/total, temperature, power, driver and PCIe current/max. Read-only host sampling is no longer incorrectly gated by the Python Supervisor; the UI now reports Supervisor status separately because it owns inference admission and process lifecycle.
- Latest candidate HEAD: `482a1bc1526a64160e2047600cea2c3214365512`.
- GitHub Actions PR run #359 (`44186835645`) passed all three jobs: Rust / Ubuntu 24.04, Web console / Ubuntu 24.04, and Ubuntu packages / smoke test. This includes Rust formatting, strict Clippy, workspace tests, release build, installer syntax, portable migration/rollback regression (including first-install failure), Python bridge/importer regressions, single-owner GPU static guard, TypeScript type-check and Vite production build, package checksum/content verification, disposable Ubuntu 24.04 `.deb` install, API health, production UI, unauthenticated 401 and authenticated host-profile smoke test.
- No live workstation deployment, service restart, GPU workload or media artifact was performed. The run proves CI/package behavior only.
