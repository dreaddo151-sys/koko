# PROJECT DREAD UNIVERSAL — Work Log and Handoff

**Updated:** 2026-11-18  
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
- Last checked head: `36e09c1ab59c4955518379166785ccd732277059` (adapter implementation candidate; documentation commits may advance PR HEAD without changing that tested implementation).
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


## Pstack provenance and Hermes adapter hardening — 2026-11-18

- Corrected local Pstack import provenance. A local directory is now recorded as `Pstack local filesystem copy`; no official GitHub URL, default version, or synthetic revision is asserted. Optional operator-supplied version/revision values remain explicit claims, not verified source provenance. The manifest says `source_verification=unverified_local_copy` and `execution=never`; license is not invented.
- Added a unit regression for local provenance with and without operator-supplied claims.
- Added a concrete, library-only `HermesApiAdapter` using the documented Hermes Runs API. It supports authenticated health/capability discovery, run creation with idempotency, status/result, SSE events and stop. Endpoint construction rejects hostnames/non-loopback addresses and credential-bearing URLs; request IDs and idempotency keys are bounded; JSON responses and event streams have explicit byte limits; run input, conversation history and response IDs are validated.
- Added adapter unit tests for loopback restrictions (including IPv6), terminal status mapping, LF/CRLF SSE parsing and replay sequence filtering.
- Important boundary: the adapter is not registered into the API's task/mission execution path. It must not be called until a DREAD-approved task lifecycle and provider sandbox/tool permission policy are connected. Hermes server health is not agent execution proof.
- CI run #389 (`44191004156`) passed all three jobs for candidate `bfa432d51e103e08bb36c77e61695459330c40d0`: Rust formatting, strict Clippy, workspace tests, release build, installer validation, migration/rollback regression, Python bridge/importer tests, single-owner GPU guard, React build, package checksum/content validation and disposable Ubuntu 24.04 package/API/UI/auth smoke tests.
- No live Hermes server was contacted, no agent task was executed, and no workstation process or file was changed.


## Mounted storage discovery — 2026-11-18

- Added a read-only GNU `df` probe to the host profile, with the existing bounded probe timeout and no shell invocation.
- Added structured mount inventory for persistent and network filesystems: source, filesystem type, mount point, total, used and available MiB. Common pseudo-filesystems (tmpfs, proc, sysfs, overlay, cgroup, squashfs and others) are excluded.
- Exposed the inventory in the authenticated host-profile API and rendered it in the React host overview.
- Added a parser regression covering an ext4 root filesystem, an NFS mount, and pseudo-filesystem exclusion.
- Explicit limitation: this is mounted-filesystem capacity, not physical disk/partition topology, SMART health, RAID state, NVMe diagnostics or storage-controller inventory.
- CI run #403 (`44192143563`) passed all three jobs for code candidate `78c8d4db95686edfe8fbb5c0bc9b86de42da3d4b`: Rust formatting, strict Clippy, workspace tests (including the storage parser), release build, installer/migration/rollback tests, Python bridge/importer tests, single-owner GPU guard, React production build, package checksum/content verification and disposable Ubuntu 24.04 package/API/UI/auth smoke test.
- No workstation files or services were read or changed by this CI run.


## Mission result visibility, Hermes adapter contract and Pstack hardening — 2026-11-18

- Mission GET now returns each node's persisted result JSON, error, attempt count, elapsed milliseconds and completion timestamp. This is result visibility/aggregation data, not an autonomous DAG scheduler; task dispatch remains approval-gated and dependency-checked.
- AgentAdapter lifecycle calls now carry both the stable local DREAD task ID and provider task ID. Hermes status/events/stop/result use the provider run ID for remote paths while preserving the local task ID in returned DREAD events/results.
- Hermes input accepts only declared run fields; undeclared tool/configuration fields are rejected. A loopback HTTP regression test exercises capability discovery, idempotent submission, status, SSE events, stop and result retrieval. Hermes remains a library adapter and is not yet wired as a DREAD execution runner; host-tool sandbox/isolation must be verified before that is enabled.
- Pstack runtime import now hard-pins SHA-256 for the catalog, import marker, upstream manifest and LICENSE in addition to repository, commit, tree, version and license metadata. It rejects symlink escapes and changed source files. Tests use the actual pinned snapshot rather than synthetic metadata.
- Added authenticated `POST /api/capabilities/import-batch`: bounded to 128 records, validates every record, inserts in one SQLite transaction, skips only exact matching disabled records (ignoring import timestamp only), and rolls back the complete batch on conflict. The Python Pstack importer now uses this single batch request and validates the complete response; it does not issue per-record partial writes.
- Regression tests cover batch idempotency, metadata conflict rollback (no partial row remains), no execution, pinned marker/catalog tampering, changed upstream content, symlink/path traversal rejection and single-request batch submission.
- Commit `b23c1feba39a95e687f552a0746094766e20545f` exposed one rustfmt-only failure in the new batch importer; the formatting was corrected in commit `aa78e66c2c9228fa14416de7df0d84aeacf41127`.
- GitHub Actions PR run #472 (`44195876703`) passed all three jobs on corrected head `aa78e66c2c9228fa14416de7df0d84aeacf41127`: Rust formatting, strict Clippy, workspace tests, optimized release build, installer syntax, portable migration/rollback, Python bridge/importer tests, single-owner GPU guard, React production build, package checksum/content validation, disposable Ubuntu 24.04 `.deb` installation, API health, production UI, unauthenticated 401 and authenticated host-profile smoke test.
- CI proves repository/package behavior in a disposable runner. No current-candidate workstation install, ComfyUI/GPU workload, Hermes tool execution, media artifact or reboot acceptance was performed.


## Approval-gated mission DAG auto-advance — 2026-11-18

- Added a mission scheduler that finds approved nodes whose full dependency set is completed and queues them through the existing task queue and shared execution gate.
- The first root task still requires explicit operator approval and an explicit start action. Once the mission is running, successful node completion advances all newly ready, already-approved nodes. Approving a dependent node after its prerequisites finish also dispatches it automatically.
- Nodes that are still pending approval are never dispatched. Task failure or cancellation blocks the mission and prevents further automatic dispatch. Concurrent scheduler attempts are safe against duplicate queueing because the task transition is conditional and conflicts are ignored as races.
- Added integration regressions for dependency-gated dispatch, auto-advancing a pre-approved dependent node, and late approval after prerequisite completion (including the database-lock/deadlock path).
- CI run #509 (`44197296018`) passed all three jobs on code candidate `8c19a5b904fc32c0e811c55ed0d96bb5cd15cc1a`: Rust formatting, strict Clippy, workspace tests including the mission scheduler regressions, release build, installer syntax, portable migration/rollback, Python bridge/importer tests, GPU single-owner guard, React production build, package checksum/content checks, disposable Ubuntu 24.04 `.deb` install, API health, UI, unauthenticated 401 and authenticated host-profile smoke test.
- Scheduler scope remains deliberately bounded: no priority/resource-aware queue policy, retry/backoff, mission-level aggregation strategy or interrupted-mission recovery has been claimed.
- No live workstation deployment, service restart, GPU job or media generation was performed.


## Priority-ordered independent mission roots — 2026-11-18

- Mission start now queues all other approved, dependency-free root nodes after the operator explicitly starts an approved root. The graph's deterministic priority-aware execution order is preserved for ready-node dispatch.
- Successful node completion dispatches newly ready approved dependents. Approval arriving after prerequisite completion triggers dispatch. Pending-approval nodes remain held; failures/cancellations block further dispatch. The shared execution semaphore bounds concurrent runners.
- Added integration coverage proving priority ordering of independent roots and automatic dispatch of all approved roots, in addition to the existing dependency and late-approval tests.
- Run #520 (`44197777681`) passed all three CI jobs on code candidate `c41ea4b4424c30f1da536305dc0fef7e8f3f4129`: formatting, strict Clippy, Rust tests, release build, installer/migration/rollback tests, Python bridge/importer regressions, GPU single-owner guard, React build and disposable Ubuntu 24.04 package/API/UI/auth/host-profile smoke tests.
- Still incomplete in mission orchestration: resource-class-aware scheduling, retry/backoff, per-node timeout and provider cancellation propagation, mission-level aggregation policy, and interrupted mission recovery.


## Latest PR-head CI verification — 2026-11-18

- Verified PR #12 head `5a494c87592d14cd057cc37d2df4642c92d85e7c` against GitHub Actions run #527 (`44198019930`).
- All three jobs passed: Rust / Ubuntu 24.04, Web console / Ubuntu 24.04, and Ubuntu packages / smoke test.
- Rust checks included formatting, strict Clippy, workspace tests, release build, installer validation, migration/rollback, Python bridge/importer regressions and GPU single-owner guard.
- Web checks included lockfile validation and production build.
- Package checks included assembly, checksums/content, disposable Ubuntu 24.04 installation, API health, production UI, unauthenticated 401 and authenticated host-profile smoke test.
- Run #527 validates the latest PR documentation head. The mission scheduler implementation remains the Run #520 candidate `c41ea4b4424c30f1da536305dc0fef7e8f3f4129`; its scheduler-specific tests passed in that run.
- No live workstation installation, service restart, GPU workload, media generation or reboot acceptance is implied. PR #12 remains open and unmerged.


## Mission retry control and host discovery probe correction — 2026-11-18

- Added bounded exponential retry backoff to the shared execution gate. The initial delay and maximum delay are explicit configuration fields; invalid bounds are rejected. Cancellation is observed while waiting, so a cancelled task does not launch another attempt.
- Added regression coverage for invalid backoff configuration and cancellation during retry wait. Existing runner retry, timeout, active cancellation, concurrency and admission tests remain in the workspace suite.
- **Important limitation:** the production default remains one attempt, and the new delay is an execution primitive. No mission-node-specific retry policy, retryable-error classification, or provider idempotency contract has been enabled. Non-idempotent work must not be blindly retried.
- Corrected mounted filesystem discovery to use GNU `df -B1 --output=...` without the incompatible `-P` option. Added a live probe regression requiring the root filesystem to be reported; parser-only fixtures alone would not have caught the invalid command combination.
- Candidate commit: `4cfc38b955cc93d6b4550b402a2dab9db14f30a4`.
- GitHub Actions run #565 (`44203367682`) passed all three jobs: Rust / Ubuntu 24.04, Web console / Ubuntu 24.04, and Ubuntu packages / smoke test. Rust checks included formatting, strict Clippy, workspace tests, release build, installer syntax, portable migration/rollback, Python bridge/importer regressions and GPU single-owner guard. Package checks included checksum/content validation and disposable Ubuntu 24.04 API/UI/authenticated host-profile smoke tests.
- Run #565 is CI evidence only. It does not prove the current candidate is installed on the physical workstation or that dual-GPU inference, ComfyUI concurrency, real media artifacts, agent execution, or release rollback have passed there.


## Multi-version CUDA Toolkit inventory — 2026-11-18

- Expanded read-only CUDA compiler discovery from one selected `nvcc` to a full distinct compiler inventory. Discovery checks the PATH-selected compiler, `/usr/local/cuda/bin/nvcc`, and versioned `/usr/local/cuda-*/bin/nvcc` locations; canonical paths are deduplicated.
- Each discovered toolkit reports its parsed compiler version and exact compiler path. The existing `cuda_toolkit_version` field remains as the PATH-first preferred version for compatibility; the additive `cuda_toolkits` array exposes every discovered compiler to API clients and the React host profile.
- Added regression coverage for simultaneous CUDA 12.8 and 13.0 compiler installations. The UI now lists all discovered toolkits rather than implying only one is installed.
- The candidate initially failed only the Rust formatting check in run #570; the exact rustfmt diff was applied in commit `13e400c96ebd0434f9c48b1da5532a8b20a79de9`.
- GitHub Actions run #571 (`44204021257`) passed all three jobs on that candidate: Rust / Ubuntu 24.04, Web console / Ubuntu 24.04, and Ubuntu packages / smoke test. Rust formatting, strict Clippy, workspace tests, release build, installer/rollback tests, Python bridge/importer tests and GPU single-owner guard passed. Web production build and disposable Ubuntu package/API/UI/authenticated profile smoke tests passed.
- This remains CI evidence. It does not prove both compilers are discovered on the physical workstation until the current candidate's host profile is run there. The known workstation had CUDA 12.8 and 13.0 compiler files in the earlier inventory; this new array is not yet physically accepted on that host.


## Retry safety and idempotency boundary — 2026-11-18

- Added a typed `RunnerError` with explicit permanent/retryable classification. The compatibility `ExecutionGate::run` path maps ordinary string errors to permanent failures, so existing API runners cannot be retried accidentally.
- Added `run_with_retryable_errors` for future providers that can prove a failure is safe to retry (for example, by a provider idempotency key). Only explicitly retryable failures enter the capped exponential backoff. A timeout is not retried because the remote side-effect outcome may be unknown. Cancellation remains observable during backoff.
- Regression tests cover retry of explicitly retryable transport errors, ordinary string/permanent errors not being replayed, cancellation during backoff, and timeout not being replayed even when the configured maximum is three attempts.
- Candidate `9e9a7502c5a149214749599483bf17b28ee412cd`; GitHub Actions run #578 (`44204756118`) passed all three jobs. Rust formatting, strict Clippy, workspace tests, release build, installer/rollback, Python bridge/importer and GPU single-owner checks passed; Web production build and Ubuntu package/API/UI/authenticated profile smoke passed.
- **Remaining limitation:** this is a safe retry primitive, not a complete mission retry policy. The production API still uses the permanent-error compatibility path and default one attempt. Mission-node retry settings, provider-specific idempotency integration, resource-class scheduling, and retry/recovery aggregation remain outstanding.


## CPU topology and legacy Master Hub compatibility audit — 2026-11-18

- Added read-only `lscpu --json` discovery for logical processors, sockets, cores per socket, threads per core and NUMA nodes. Kept `cpu_threads` as process-available parallelism and added `cpu_logical_threads` so CPU quotas/cpusets are not mislabeled as physical hardware topology. Falls back to `/proc/cpuinfo` for logical processor count and existing physical-core detection; topology UI displays both.
- Added parser tests for a 1-socket / 6-core / 12-logical-thread / 2-threads-per-core / 1-NUMA fixture and malformed JSON. Updated the read-only workstation baseline script to capture `lscpu --json` (or `/proc/cpuinfo` fallback).
- CI run #583 (`44205536123`) passed all three jobs on candidate `f3fa453044b37ab304478fb15145eb3a3d233be8`: Rust formatting, strict Clippy, workspace tests, release build, installer syntax, portable migration/rollback, Python bridge/importer tests, GPU single-owner guard, React build, package checksums and disposable Ubuntu package/API/UI/auth smoke tests.
- Audited the repository's 5,915-line `legacy-reference/dread_web_hub.py` and recorded its route families, Rust API overlap, reference-source security observations and safe side-by-side migration sequence in `LEGACY-HUB-COMPATIBILITY.md`.
- **Gate 1 remains blocked:** the reference snapshot is not verified against the actual live `/mnt/sabrent/dread_workspace/dread_web_hub.py`; the Rust API does not provide full route/payload parity for chat, upload/media, workspace/visual tools, agent sessions/approvals or all legacy build routes. Do not switch the live hub based on the reference snapshot.
- **Physical acceptance remains outstanding:** lscpu parser and package CI pass do not prove the current candidate's reported CPU topology, CUDA inventory or physical disks on the user's workstation.


## Hermes and DeerFlow adapter implementation — 2026-11-18

- Added `dread-agents::hermes::HermesApiAdapter` for the documented Hermes Runs API. It uses loopback-IP HTTP only, explicit bearer API key, no redirects, bounded payloads/responses, idempotency keys, run status/events/stop/result methods and local/provider task ID separation.
- Hermes is disabled by default. A caller must explicitly attest a supported isolated terminal backend and per-task workspace before capabilities or task submission are enabled. DREAD task ID determines the Hermes session ID; caller-selected shared session IDs and prior-response chains are rejected to avoid cross-task conversation/workspace reuse.
- Added `dread-agents::deerflow::DeerFlowApiAdapter` for the thread-scoped Gateway API. It uses loopback endpoint restriction, PAT bearer auth, strict API prefix, bounded payloads, deterministic per-task thread IDs, idempotent run creation, status/events/cancel/result and local/provider identity separation.
- DeerFlow is disabled until the operator attests an isolated container-backed provider and one thread/workspace per DREAD task. LocalSandboxProvider is denied. The attestation rejects a configured remote/Kubernetes provisioner; AIO and BoxLite are the only supported provider classes.
- Added mock HTTP lifecycle tests for both adapters plus endpoint, auth, sandbox denial, status mapping, idempotency, sequence replay, input bounds and task identity regressions.
- CI run #605 (`44208381646`) passed all three jobs on implementation candidate `36e09c1ab59c4955518379166785ccd732277059`: Rust / Ubuntu 24.04, Web console / Ubuntu 24.04 and Ubuntu packages / smoke test.
- Follow-up documentation commit `a8ccca71fe5342daf1e709399be65a7a945ec963` records the acceptance boundary. The PR HEAD may be newer than the implementation candidate because the docs are committed separately.

**Not completed:** adapters are still library-only and are not wired into `dread-api` task/mission registration or execution dispatch. OpenHands has discovery/health probing only; its live task adapter remains outstanding. No Hermes, DeerFlow or OpenHands service was contacted on the physical workstation. Mock HTTP tests do not prove live provider behavior, sandbox isolation, cancellation of real work or recovery after provider/server restart. Phase 4 remains partial.


## Environment-driven agent registry and cancellation cleanup — 2026-11-18

- Added `AgentAdapterConfig` and `build_agent_adapters` to construct Hermes and DeerFlow clients only when their required endpoint, credentials, sandbox backend/provider and explicit per-task isolation attestations are supplied.
- Added fail-closed environment loading. Partial configuration fails startup instead of silently registering an unsafe/incomplete provider. Hermes uses `DREAD_HERMES_API_URL`, `DREAD_HERMES_API_KEY`, `DREAD_HERMES_SANDBOX_BACKEND`, `DREAD_HERMES_ISOLATED_WORKSPACE_PER_TASK=true`. DeerFlow uses `DREAD_DEERFLOW_API_URL`, `DREAD_DEERFLOW_PAT`, `DREAD_DEERFLOW_SANDBOX_PROVIDER`, `DREAD_DEERFLOW_ISOLATED_WORKSPACE_PER_TASK=true`, optional `DREAD_DEERFLOW_API_PREFIX` (default `/api/langgraph`), and optional `DREAD_DEERFLOW_PROVISIONER_URL` (non-empty remote provisioners rejected).
- Added protected `GET /api/agent-adapters` inventory returning configured provider descriptors only; no credentials or live provider requests are exposed.
- Added a bounded 5-second cooperative cleanup grace in `ExecutionGate` after active cancellation, so an execution runner can react to the cancellation watch and request provider-side cancellation before the future is dropped.
- Added tests for explicit isolation requirements, remote provisioner rejection, valid provider registration, empty registry inventory, and active-runner cleanup. The latest push CI run #625 (`44209673688`) passed all three jobs: Rust / Ubuntu 24.04, Web console / Ubuntu 24.04 and Ubuntu packages / smoke test.
- The subsequent plan-documentation commit `8edd98ead22ed096a66b1757c14d6d51c50ba201` records these boundaries.

**Not completed:** the new registry is not yet connected to persisted task creation/approval/dispatch. `GET /api/agent-adapters` is inventory only. The API still does not start/poll/cancel Hermes or DeerFlow tasks, persist provider events, or recover provider runs. OpenHands remains discovery/health-only. No live agent provider or physical workstation service was contacted. The 5-second cleanup grace is a scheduler primitive; it does not itself prove a real provider cancellation.


## OpenHands task adapter, provider recovery and final CI verification — 2026-11-18

- Added and registered `OpenHandsAgentServerAdapter`. It authenticates with the Agent Server session key, binds each conversation ID to the DREAD task ID, creates a task-specific LocalWorkspace directory, retrieves conversation status, pages the event-search API, extracts final agent messages and calls the documented conversation pause endpoint for cancellation.
- OpenHands remains disabled unless the operator provides the Agent Server endpoint/key, LLM configuration, workspace root, container identity, per-task workspace isolation attestation and secret-store attestation. The real stable `OH_SECRET_KEY` must be configured on the OpenHands Agent Server. These flags are explicit attestations and do not automatically prove a secure container or secret store.
- Wired provider-backed task dispatch into the approved DREAD task worker. The worker persists provider identity, event cursor, event history, result and error. It publishes provider events only after the SQLite transaction commits.
- Added startup recovery for running provider tasks. Recovery requires the task's recorded approval, a still-enabled capability mapped to the same provider and an enabled adapter. It requeues the task and reattaches through deterministic/idempotent provider identity, resuming event polling from the stored cursor. Candidates failing those checks are failed closed and block their mission.
- Added/updated provider registration, OpenHands authenticated lifecycle, task execution persistence, queue recovery and mission dispatch regression coverage. The OpenHands lifecycle test runs against a mock Agent Server; it is not a live provider test.
- Repaired the final rustfmt differences and a missing test-only `Arc` import revealed by CI. The source candidate passed `cargo fmt --all -- --check`, strict Clippy, optimized release build and all **111 workspace tests** with zero failures.
- GitHub Actions run #690 (`44220143462`) and PR run #691 (`44220150439`) passed all three jobs on source head `976880a5cf6ffc587fb420d4458a29ede0d7410c`. The jobs covered Rust quality/tests/release, locked npm install and production console build, installer syntax, portable migration/rollback, Python bridge/importer regressions, GPU single-owner guard, Ubuntu package checksum/content verification, disposable `.deb` installation, and packaged API/console smoke tests.
- Uploaded CI artifacts include the API build, web build and `dread-universal-ubuntu-packages` package bundle. These are workflow artifacts, not a signed public release.
- Updated `.env.example`, the root README, and the controlling master-plan phase ledger to document all three provider groups, the protected adapter inventory, approval-gated worker dispatch, recovery semantics and current acceptance limits. Earlier claims that the adapters are library-only or that OpenHands is health-probe-only are superseded by this entry and the latest plan addendum.
- PR #12 remains open and unmerged. No live provider was contacted, no real agent task was executed, and no current workstation install/service restart/GPU workload/media generation was performed. The Python Hub, ComfyUI, Ollama and GPU Supervisor were not changed by this GitHub work.
- Remaining Phase 4 acceptance: verify the configured provider versions, secrets, isolated sandbox/workspace, tool policy, real cancellation and restart recovery on the Ubuntu workstation. The CI tests validate adapter contracts and integration code, not real provider isolation. Resource-aware scheduling, full legacy Hub parity, dual-3090/media acceptance, transactional signed extension management, signed updater and workstation install/upgrade/reboot/rollback remain open.


## PR #12 and workstation acceptance checkpoint — 2026-11-18

- Verified PR #12 head commit `573627c204891e85d7a5bb96ecaacb8db52df995` and rechecked GitHub Actions run #699 (`44221090674`, PR) plus #698 (`44221083709`, push). Both runs completed successfully with all three jobs successful: Rust / Ubuntu 24.04, Web console / Ubuntu 24.04, Ubuntu packages / smoke test.
- Inspected the #699 job steps: formatting, lint, workspace tests, release build, installer validation, portable migration/rollback, Python bridge/importer tests, GPU single-owner guard, locked npm dependency install and production build, Ubuntu package assembly/checksum/content validation, disposable Ubuntu 24.04 installation and packaged API/console smoke tests. The PR-head work-log records 111 workspace tests passing. This checkpoint did not rerun CI.
- Re-read the PR description and controlling master-plan/work-log addenda. Hermes, DeerFlow and OpenHands are registered and wired to approval-gated provider task execution, persisted provider identity/events/cursors/results and guarded recovery. Provider lifecycle tests are mock-based; explicit sandbox flags remain operator attestations. Live provider isolation and tool permissions are not proven.
- **Workstation status: blocked / not inspected.** This chat had no authorized SSH/workstation terminal session. No live service status, process, port, environment configuration, credential-presence metadata, provider endpoint or sandbox permission was read. No commands were run on the user's Ubuntu host.
- No workstation files or services were modified. The Python Master Hub, ComfyUI and Python GPU Supervisor were left untouched; no stop/restart/replacement/deployment occurred. PR remains unmerged.
- Next step is a read-only workstation baseline with redacted configuration metadata, followed by independent verification of live provider API versions, per-task container/workspace isolation, tool allowlists, secret handling, cancellation and restart recovery. Only after those checks should a controlled real-provider task be considered. Other outstanding gates remain full legacy Hub parity, dual-3090 concurrent inference/media artifacts, resource-aware mission policy, signed transactional extension management/updater, and physical Ubuntu install/upgrade/reboot/rollback acceptance.


## Recovered Ubuntu workstation test evidence — 2026-11-17 23:33

A saved terminal capture, `Pasted text(20261117-233320).txt`, was recovered from the user's project files and inspected during the 2026-11-18 checkpoint. It records commands executed in the Ubuntu workspace `/mnt/sabrent/dread_workspace/dread_universal_upstream/dread_universal`, on branch `feature/universal-capability-discovery-clean-2026-11-17` tracking its origin branch:

- `cargo fmt --all -- --check`: exit status 0.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: exit status 0.
- `cargo test --workspace --locked`: exit status 0; 87 unit tests passed, zero failures (11 agents, 35 API, 11 discovery, 6 GPU, 9 orchestrator, 8 platform, 7 skills).
- `cargo build --workspace --release --locked`: exit status 0.

This is **actual prior Ubuntu workstation execution**, not GitHub CI. It should be credited and not repeated merely as a generic Rust regression. It predates the final PR #12 head and does not prove that the final provider-adapter changes were installed or that live provider endpoints were contacted. The Hermes HTTP lifecycle in this run is a test; it is not a live Hermes task. This capture does not establish live OpenHands/DeerFlow sandbox isolation, tool allowlists, current service status, dual-GPU concurrency or media artifact acceptance.

Separate previously recorded live evidence remains valid at its own timestamps: 2026-11-16 Rust API health/service discovery and the 2026-11-17 Supervisor-bridged llama.cpp inference returning `DREAD GPU INFERENCE OK`. These are historical live proofs, not a fresh current-candidate integration test. Preserve the Python Master Hub, ComfyUI and Python GPU Supervisor; do not repeat broad checks without a specific candidate-integration reason.


## Live Ubuntu baseline and service-discovery checkpoint — 2026-11-18 10:30 UTC

- Received the user's read-only workstation baseline captured on Ubuntu 24.04.5 LTS, host `draead-MS-7D68`, kernel `7.0.0-43-generic`, workspace `/mnt/sabrent/dread_workspace`. The source baseline was saved on the host at `/home/draead/.local/share/dread-universal/baselines/workstation-baseline-20261118T103028Z.txt`.
- Current baseline reports ComfyUI HTTP 200 on loopback :8188, Rust API HTTP 200 on :8765, GPU Supervisor gateway HTTP 200 on :11500, and authenticated Supervisor bridge HTTP 200 on :11502. Both RTX 3090 devices are detected. `dread-gpu-supervisor-bridge.service` and `dread-universal-api.service` are active; `dread-tensor.service` is inactive as required.
- Legacy Python Hub and telemetry checks return HTTP 000 on :8080; no :8080 listener appears in the captured listening-port excerpt and no matching legacy Hub unit appears in the listed user services. This establishes endpoint unavailability at capture time, not whether the old Hub source is retired, moved, or intentionally stopped. Do not restart or deploy it as part of this checkpoint.
- The baseline excerpt reports current PCIe link values below device maxima (GPU 0 Gen1 x8 vs Gen4 x16 maximum; GPU 1 Gen4 x8 vs Gen4 x16 maximum). The capture script explicitly warns that idle power-saving downshift is possible. Do not diagnose a hardware fault from this idle snapshot; verify under a controlled sustained workload only during the GPU acceptance stage.
- The baseline text supplied in chat does not include the script's CPU topology, CUDA Toolkit, or physical block-device sections. The script itself includes read-only `lsblk --json` inventory for unmounted disks; those details remain unverified from the excerpt and must not be inferred.
- Read the current candidate's platform implementation: `dread-platform` already reports both mounted filesystem capacity and a separate `physical_storage_devices` inventory populated by read-only `lsblk --json`, including whole disks, partitions and mount points. Its parser has a regression test for an unmounted disk. This corrects the earlier assumption that candidate storage discovery was only `df`-based; the API's `/api/system/profile` exposes the host profile behind bearer authentication.
- Read the current service discovery configuration and confirmed it monitored the legacy Hub, ComfyUI and llama.cpp gateway, but omitted the now-running authenticated GPU Supervisor bridge. Added a loopback-only read-only health entry for `http://127.0.0.1:11502/health` with ID `gpu-supervisor-bridge` and role `gpu-lifecycle-control`. The legacy Hub entry is intentionally retained so its unavailable state remains visible rather than being hidden.
- Source/config commit: `cfac424cb5d3fa9e16889f5a13d4c306fd45c8a4`. GitHub commit inspection confirms this is a six-line additive change to `dread_universal/config/services.toml`; it does not change running services or workstation files. No workflow run was returned for this commit at the time of inspection; CI acceptance is pending.
- Stage 1 remains open: retrieve the omitted physical block-device and CPU/CUDA sections from the saved baseline; obtain an authenticated, redacted `/api/system/profile` response to compare live GPU/storage discovery against the API; establish the current legacy Hub source/process/route inventory before declaring parity. Do not expose API tokens. Preserve ComfyUI, both Supervisor services, the Rust API, provider services and user data.

## Registry lifecycle hardening — audited removal — 2026-11-18

- Added authenticated DELETE /api/capabilities/{id} for registry metadata only. The endpoint requires an operator reason, permits only disabled or rejected records, and refuses removal while any durable task references the capability.
- The removal and its audit event commit in one SQLite transaction. The audit row preserves the full pre-removal capability JSON, reason, operator ID and timestamp independently of the live capability row; GET /api/capabilities/{id}/history reads the retained event history.
- Added an operator-console REMOVE control only for disabled/rejected records. It explicitly discloses that removal affects DREAD registry metadata only and does not delete files from the pinned Pstack checkout.
- Added regression coverage for referenced-record rejection, unreferenced metadata removal, durable audit/history retrieval and 404 after removal. The test initially failed with HTTP 405 before implementation, then passed after the endpoint and route were added.
- GitHub Actions run #755 (44301310766) passed all three Ubuntu 24.04 jobs on source head 978461f0ab932872656cba94ae915d32495683fc: Rust format, strict Clippy, workspace tests (the API binary test group reported 41 passed, 0 failed), optimized release build, installer validation, portable migration/rollback, Python bridge/importer tests, GPU ownership boundary, locked web install/production build, package/checksum verification, and disposable Ubuntu .deb install plus packaged API/UI/auth smoke tests. Artifacts include the API, web and Ubuntu packages.
- This completes only the registry metadata removal path. Phase 5 remains partial: no signed publisher/catalog verification, remote source download/installation, transactional update/replace, atomic activation/rollback, or reviewed executable tool/MCP lifecycle is implemented by this change. Source files in Pstack were not modified.
- No workstation command ran; no local package was installed; no service was stopped/restarted; no provider or media job was executed. PR #12 remains open and unmerged.


## OpenHands Docker-runtime preflight — 2026-11-18

- Added a fail-closed runtime check to the OpenHands Agent Server adapter. It performs an authenticated `GET /server_info` and accepts Docker only when `conversation_runtime` is `docker` or `available_conversation_runtimes` explicitly contains `docker`.
- The check is applied before advertising the OpenHands execution capability, reported in adapter health, and repeated before creating a new conversation. A task cannot fall through to a local workspace if Docker is unavailable. Existing task conversations can still be reattached without requiring the server to advertise new Docker starts.
- DREAD sends `conversation_runtime: "docker"`, uses the Docker conversation workspace at `/workspace`, routes the LLM endpoint through `http://host.docker.internal:11500/v1`, and keeps `AlwaysConfirm` so tool actions require an operator decision through DREAD.
- Added a real adapter-level mock HTTP regression. The test first failed on CI run #772 (`44304521168`): the adapter created a running conversation against a server whose `/server_info` advertised only `local`. The implementation then passed all three Ubuntu 24.04 jobs on run #776 (`44305025586`): Rust format/Clippy/workspace tests/release build, web production build, and Ubuntu package plus API/UI/auth smoke tests.
- Updated `.env.example` and the README to use the current configuration keys: `DREAD_OPENHANDS_LLM_BASE_URL`, `DREAD_OPENHANDS_DOCKER_RUNTIME_ENABLED`, and `DREAD_OPENHANDS_SECRET_STORE_CONFIGURED`. The example now documents the Agent Server's `OH_CONVERSATION_RUNTIME=docker` or `OH_CONVERSATION_RUNTIME_SELECTABLE=1` setup. Removed stale workspace-root/container-ID fields from the example because the current adapter no longer reads them.
- This verifies the adapter contract against a mock Agent Server and official `server_info` contract; it does not prove Docker Engine availability, host-gateway reachability, runtime container creation, or live tool execution on the user's workstation. The earlier workstation evidence still identifies Agent Server 1.62.0 running on the host; current provider configuration and runtime capability have not been independently inspected.
- No workstation command or service action was performed. No real agent task was submitted. The Python Master Hub, ComfyUI, GPU Supervisor and user data remain untouched. PR #12 remains open and unmerged.


## Agent cancellation acknowledgement and extension audit history — 2026-11-18

- OpenHands cancellation now requires the authenticated pause endpoint's JSON response to contain `success: true`. An HTTP-success response with `success: false`, a missing flag, or a malformed response is not recorded as a completed cancellation; the adapter returns `provider_cancel_not_confirmed` and leaves the caller to reconcile provider status.
- Added a mock HTTP regression for the unsuccessful pause acknowledgement. The test first failed on CI run #802 (`44307932138`): the adapter returned `AgentTaskState::Cancelled` for `{"success":false}`. The acknowledgement check was added in commit `e5366e867192c38010ba631280a1953838bd5ea5` and passed all three Ubuntu 24.04 CI jobs in run #804 (`44308155855`).
- Capability enable/approval and disable/revocation now write their prior record, operator, reason and timestamp to `capability_events` in the same SQLite transaction as the state change. Enable re-checks the current source SHA-256 fingerprint before committing approval; rejected capabilities and changed content remain blocked. The console displays audit event IDs for both operations.
- Added regression coverage for approval fingerprint mismatch, rejected capability denial, approval metadata, disabled state, retained prior approval record and readable history. The API/UI changes passed all three Ubuntu 24.04 CI jobs in run #798 (`44307389259`) and run #790 (`44306575501`), respectively.
- Updated the README API table to document `GET /api/capabilities/{id}/history` and the reasoned enable/disable lifecycle.
- Hermes cancellation already performs a subsequent provider run-status read after its stop request; DREAD maps the returned provider state instead of blindly marking the task cancelled. DeerFlow likewise reads the provider run after its interrupt request. These are source-level contract observations backed by mock lifecycle tests, not live-provider cancellation acceptance.
- CI proves the adapter/API logic against mock servers and validates the Ubuntu package in a disposable runner. It does not prove actual OpenHands Docker availability, provider cancellation under network failure, container tool restrictions, host-gateway reachability or the user's running Agent Server behavior.
- No workstation commands, service changes, provider task starts, media jobs, deployment or merge were performed. PR #12 remains open and unmerged; preserve the Python Master Hub, ComfyUI and Python GPU Supervisor.


## Mission DAG console, dispatch validation and local capability refresh — 2026-11-18

- Repaired mission-to-agent instantiation: mission nodes of type `agent_task` now resolve an explicitly registered, enabled `Agent` capability and a matching reviewed adapter. The capability ID and normalized input are persisted with the task. Mission instantiation still creates approval-gated tasks only; it does not approve or dispatch them automatically.
- Added shared final-queue validation so an agent capability cannot be attached to a built-in task type such as `gpu_chat`, and an `agent_task` cannot run without a reviewed provider capability. Provider IDs are constrained to `hermes.run`, `deerflow.run` and `openhands.run`; the capability kind, status, adapter registration, enabled state and adapter identity are checked before queueing.
- Added regression coverage for the cross-runner mismatch. The new test first reproduced the defect on CI: a `gpu_chat` task carrying `hermes.run` was accepted with HTTP 202 and reached the mock provider, instead of being rejected with HTTP 409. After the shared validation fix, the test passes and the adapter start count remains unchanged.
- Added read-only `GET /api/missions`, returning at most the latest 100 mission summaries after validating stored graph identity. Added the operator console Mission DAG page: save a plan, inspect execution order and node/task state, and separately instantiate the graph. Task approval, run and cancellation remain in the task queue.
- Added an explicit authenticated local capability refresh action for the local Pstack Markdown source. The old import path silently ignored an existing ID even when the source fingerprint had changed; preview then rejected the stale fingerprint and repeating import could not repair it. Ordinary import now reports changed content rather than silently treating it as current.
- The new refresh is a single SQLite transaction. New records are imported disabled; identical records remain unchanged; changed records may be replaced only when the prior record is also an explicitly unverified local Pstack context record, is disabled, and has no task-history references. A changed enabled record or task-referenced record aborts the entire refresh. The prior record snapshot, reason, actor and timestamp are retained in `capability_events` before replacement. No imported content is executed and refreshed records remain disabled for review.
- Regression tests cover disabled-record refresh and old-snapshot audit retention, plus rejection of changed enabled and task-referenced records without replacing the stored fingerprint. The refreshed frontend production bundle builds successfully.
- GitHub Actions PR run **#840** (`44315586513`) passed all three Ubuntu 24.04 jobs on code head `844e7d3df48dac69622f3e707502a293cac70beb`: Rust format, strict Clippy, workspace tests, release build, installer syntax, portable migration/rollback, Python bridge/importer tests and GPU single-owner guard; React/TypeScript production build; and Ubuntu package checksum/content validation plus disposable `.deb` install, API health, packaged UI, unauthenticated 401 and authenticated host-profile smoke test. The Rust API test group reported 47 passed, 0 failed; the other reported crate test groups passed (11, 6, 12, 15 and 7).
- Scope limitation: this refresh updates registry metadata for the already configured local Pstack copy; it does not download or install an upstream Pstack release, establish publisher identity, verify a signed catalog, install executable plugins/MCP tools, or implement atomic extension-code activation/rollback. SHA-256 here detects local content changes; it is not a publisher signature.
- This CI evidence does not prove the changes are installed on the user's workstation or that live providers, Docker isolation, dual-3090 contention, ComfyUI, MiniMax H3 MP4 or FlashVSR outputs pass physical acceptance. No workstation command, package install, service restart, provider task or media job was performed. Preserve the Python Master Hub, ComfyUI, Python GPU Supervisor and user data. PR #12 remains open and unmerged.


## Current PR refresh, signed-verifier repair and OpenHands workspace contract — 2026-11-19 09:00 Bahrain

- Re-inspected PR #12, its live branch ref, latest commit, changed-file inventory, workflow runs and this work log before doing more work. The user-supplied checkpoint SHA \`573627c204891e85d7a5bb96ecaacb8db52df995\` is historical, not the current branch head. The actual branch/PR head at inspection was \`d22d57b9282fc245f0f6525e9b544a5d8ac606ba\`, commit message \`test(ci): validate signed release verification and packaging\`; PR #12 remained open and unmerged.
- Current-head PR workflow run #856 (\`44317429157\`) did **not** pass: Rust formatting, strict Clippy, all 124 workspace tests, optimized release build, shell syntax and Web production build passed; \`scripts/test_verify_release.sh\` failed at its fake-cosign fixture; the package job was skipped. This supersedes the earlier successful #699/#698 checkpoint for current-head status. The #699/#698 results remain valid for their older head only.
- Inspected the failing fixture. Its first fake-cosign heredoc contained an unterminated single-quoted identity-regex assignment, then duplicate fixture/command content remained outside the heredoc. The production \`verify-release.sh\` itself was not the cause. Replaced the fixture in commit \`12d86f505c6c6e85bf2b1181c372f7d3fd03be09\`. The repaired test checks the expected Sigstore OIDC issuer and GitHub workflow identity, rejects a failed signature, a missing bundle, a symlinked manifest and a checksum mismatch. A disposable local container run of the test against a copy of the current verifier passed; this is not Ubuntu workstation evidence. CI rerun is pending.
- A separate security-contract review found a substantive OpenHands adapter mismatch. The DREAD adapter previously treated \`conversation_runtime\` / \`available_conversation_runtimes\` as Docker proof and always sent \`workspace.kind="LocalWorkspace"\`. OpenHands' merged upstream PR #17177 and v1.21.0 release contract use authenticated \`/server_info.execution_runtime\` and require \`workspace.kind="DockerExecutionWorkspace"\` with \`working_dir="/workspace"\` when the server advertises Docker execution; Docker workspaces must also disable host worktree setup. Updated DREAD in commit \`282e36c4f9714c44d618079fcc3bf248e62c4558\` to fail closed unless \`execution_runtime == "docker"\`, require \`usable_tools\` to include \`terminal\`, \`file_editor\`, and \`task_tracker\`, request \`DockerExecutionWorkspace\`, set \`worktree=false\`, and retain \`AlwaysConfirm\`. Tests now reject the legacy runtime fields and missing tool inventory. This is a source/mock-contract correction, not proof that the live Agent Server creates isolated containers.
- Updated \`.env.example\` and README in commits \`1878aec7d32bed1b6263967c7d4c0dd7e0933938\` and \`67444ffd427b086b0dc0a5590c2e5f72e521bcd0\` to document the execution-runtime contract and the distinction between confirmation and command allowlisting. The OpenHands tool set still includes a powerful terminal tool; \`AlwaysConfirm\` is an approval gate, not a command-level sandbox or allowlist. Do not run real tasks until actual per-conversation container creation, mounts, network restrictions, secret handling and tool boundaries are checked.
- Official provider documentation reviewed for acceptance design: OpenHands Docker sandbox/workspace contract and merged PR #17177; Hermes terminal backend documentation (Docker is a persistent container shared across a Hermes process, and local is the default); DeerFlow configuration documentation (LocalSandboxProvider is host execution, AIO is Docker, BoxLite is per-thread micro-VM, and AIO network mode can be open/isolated/allowlist). DREAD's current environment flags remain attestations; they do not inspect the actual provider configuration or enforce per-task freshness/egress by themselves.
- **Live workstation/provider state remains not inspected.** No authorized SSH or workstation terminal connector was available in this run. The latest physical baseline is still the user's 2026-11-18 10:30 UTC capture: ComfyUI :8188, Rust API :8765, Supervisor gateway :11500 and authenticated bridge :11502 returned HTTP 200; Python Master Hub :8080 returned HTTP 000; \`dread-tensor.service\` was inactive. That is a dated snapshot, not a current status. Current Hermes, DeerFlow and OpenHands configuration, credentials-presence metadata, Docker containers/mounts, tool permissions, API versions and cancellation/recovery behavior remain unknown/unverified.
- No live provider task, package install, service stop/restart, deployment, GPU workload or media job was run. The Python Master Hub, ComfyUI, Python GPU Supervisor, databases, models and user data were not changed. PR #12 remains unmerged.
- Remaining work, in priority order: (1) obtain a read-only workstation capture and verify all three live provider sandboxes/tool permissions before any task; (2) rerun CI on the fixed/current branch head; (3) complete exact live Python Hub route/payload/auth/stream/cancel/upload/output parity; (4) add resource-class admission, timeout/cancel propagation, safe retry/idempotency and mission result aggregation; (5) signed extension catalog/publisher verification and transactional executable extension lifecycle; (6) dual-RTX-3090 concurrency with ComfyUI, valid image/MiniMax H3 MP4/FlashVSR artifacts; (7) signed updater/SBOM/license review and physical Ubuntu install/upgrade/reboot/rollback acceptance.


## Resume checkpoint — 2026-11-19 09:28 Bahrain

- Re-inspected PR #12, the live branch ref, latest commit, current workflow runs, the provider adapter/configuration source, mission scheduler, legacy Hub compatibility audit, installer/package scripts and the current master plan before continuing. PR #12 remains open, unmerged and mergeable; branch `feature/universal-capability-discovery-clean-2026-11-17`; current source head at this checkpoint `8d8863c7393176432d04def410639514414b6d2c`; 64 changed files. The supplied `573627c204891e85d7a5bb96ecaacb8db52df995` checkpoint and runs #699/#698 are historical.
- Current-head GitHub Actions PR run #885 (`44473460017`) and push run #884 (`44473452327`) both passed all three Ubuntu 24.04 jobs at `8d8863c7393176432d04def410639514414b6d2c`: Rust, Web console and Ubuntu packages/smoke test. The Rust job passed `cargo fmt --all -- --check`, strict Clippy, `cargo test --workspace --locked` (**124 passed, 0 failed**), optimized release build, installer/release-script syntax, signed-verifier regression tests, portable migration/rollback tests, Python bridge/importer tests and the single-owner GPU boundary check. The Web job passed locked npm installation and production build. The package job passed bundle/.deb assembly, checksums/content validation, disposable Ubuntu 24.04 install, packaged API health, production UI and auth smoke tests.
- Three current-branch CI defects were diagnosed and repaired, rather than rerunning completed work blindly: (1) #879/#878 failed formatting on two misindented provider test fields; fixed in `fdad0edbfdcb6c3e1559687ce8a0a222d65121b8`; (2) #881/#880 then passed formatting but failed Rust test compilation because the positive Hermes/DeerFlow registry fixture omitted the new `sandbox_policy_verified` fields; fixed in `6c5282bcdc9cca2c3cd51b1334290b29783f141c`; (3) #883/#882 passed Rust and Web but package assembly failed because `SHA256SUMS` named `verify-release.sh` without staging that file in `dist/release`; fixed in `8d8863c7393176432d04def410639514414b6d2c`. Final #885/#884 passed; do not repeat these gates unless source changes.
- Provider configuration clarification: `dread_universal/config/agents.toml` contains `agents = []` and is for read-only health probes, not task-adapter credentials. All provider environment groups in `.env.example` remain commented. `load_agent_adapters_from_env()` reads the `DREAD_HERMES_*`, `DREAD_DEERFLOW_*` and `DREAD_OPENHANDS_*` variables from the running API process; any partially supplied provider group fails closed. No actual service environment was readable in this run, so the live provider configuration is unknown, not absent.
- Hermes source gate: current DREAD adapter accepts only `modal`, `daytona`, `vercel_sandbox` or `singularity`, requires per-task workspace and explicit sandbox-policy attestations, and rejects the stock Docker backend. Current upstream Hermes docs now describe a Docker per-session mode using `container_persistent: false`; the DREAD adapter does not currently model or verify that option, so do not assume this mode is supported. Verify actual Hermes version/config, container-per-session behavior, host mounts, network egress and forwarded secrets before any task. Reference: https://github.com/NousResearch/hermes-agent/blob/main/website/docs/user-guide/configuration.md
- DeerFlow source gate: current DREAD adapter permits only `AioSandboxProvider` or `BoxliteProvider`, requires per-task thread/workspace and explicit policy attestations, and rejects `LocalSandboxProvider` and non-empty remote provisioners. Current DeerFlow docs state AIO's compatibility default network mode is `open`; `isolated` denies outbound traffic and `allowlist` uses a policy sidecar. BoxLite scopes VMs by `(user_id, thread_id)` and may reuse a warm VM for the same thread; DREAD uses task-scoped thread IDs, but actual network policy, mounts and deployed provider version still need inspection. Reference: https://github.com/bytedance/deer-flow/blob/main/backend/docs/CONFIGURATION.md
- OpenHands source gate: DREAD requires authenticated `/server_info.execution_runtime == "docker"` and `usable_tools` containing `terminal`, `file_editor` and `task_tracker`; its conversation request names only those three tools, selects `DockerExecutionWorkspace` at `/workspace`, sets `worktree=false`, and uses `AlwaysConfirm`. `AlwaysConfirm` is an action-confirmation gate, not a command allowlist; the terminal remains a powerful shell inside the selected workspace. Inspect the actual Agent Server version, stable `OH_SECRET_KEY` storage, per-conversation container creation, mounts, network egress and tool policy before a real task. Reference: https://github.com/OpenHands/docs/blob/main/sdk/arch/agent-server.mdx
- All `DREAD_*_SANDBOX_POLICY_VERIFIED` values are operator attestations. They do not independently prove a container, fresh workspace, mount restrictions, egress controls or secret-store permissions. Provider lifecycle tests remain mock/API-contract tests; no live provider task, real container isolation test, provider cancellation or provider restart/recovery drill is accepted.
- Workstation status was **not inspected in this run**: the available tools expose GitHub and file/skill connectors but no authorized SSH or workstation shell. No command was run on Ubuntu and no local secret was read. The latest physical capture remains the historical 2026-11-18 10:30 UTC baseline: ComfyUI `:8188`, Rust API `:8765`, Supervisor gateway `:11500` and authenticated bridge `:11502` returned HTTP 200; Python Master Hub `:8080` returned HTTP 000; `dread-tensor.service` was inactive. This is not a current status claim. Preserve the Python Master Hub, ComfyUI, Python GPU Supervisor, llama.cpp, models, databases and existing worktrees.
- Existing read-only host capture script: `dread_universal/scripts/capture-workstation-baseline.sh`. On the workstation, run it from the active upstream checkout to save a private baseline under `$HOME/.local/share/dread-universal/baselines/`; it does not start/stop/restart services. It records host, user services, ports, HTTP status, CPU/GPU, CUDA, physical disks and filesystem capacity, but it does not inspect provider secrets/configuration or Docker mounts. Provider review must separately read only variable names/presence and safe config values; never print token/key values.
- Resource-aware mission scheduling remains unimplemented: `dread-orchestrator::ExecutionGate` currently applies one global semaphore/admission limit (default concurrency 2, admitted 8) rather than resource-class quotas. Mission DAG validation, priority ordering, approval-gated dispatch and dependent-node advancement exist; per-resource admission, per-node resource/timeout/retry policy, mission result aggregation and interrupted mission recovery remain open.
- Legacy Hub parity remains open. `LEGACY-HUB-COMPATIBILITY.md` is still an audit of the 5,915-line repository reference snapshot, not the exact installed `/mnt/sabrent/dread_workspace/dread_web_hub.py`. The route/payload/auth/stream/cancel/upload/output matrix and side-by-side tests are not complete. Do not replace or restart the live Hub.
- Signed extension/updater work remains partial. Registry metadata import/refresh/audit/removal exists, but signed publisher/catalog verification, source acquisition, executable tool/MCP permission review, transactional install/update/remove and atomic activation/rollback are not accepted. The release verifier now ships beside the checksum manifest, but the inspected release workflow still has no `cosign sign-blob` step or Sigstore bundle generation; a verifier and failure-path tests do not constitute a signed release.
- Dual-RTX-3090 concurrency and media remain unaccepted: no current-candidate contention run alongside ComfyUI, no validated image, playable MiniMax H3 MP4 or FlashVSR artifact, and no progress/cancel/download/concurrency evidence was produced. Physical Ubuntu install/upgrade/reboot/service-preservation/failed-upgrade rollback is also not accepted; current package tests ran only in disposable GitHub Ubuntu runners.
- No live provider task, GPU/media workload, workstation package install, service action, deployment, merge or data mutation occurred. PR #12 remains open/unmerged.

### Next acceptance order

1. Obtain a read-only workstation capture; hash the installed Python Hub and identify current Hub/ComfyUI/Supervisor/Rust API/provider processes and units without changing them.
2. Read the running API's provider environment safely (names/presence only for secrets), inspect provider configs and versions, OpenHands `/server_info`, Docker containers/mounts/networks and exact tool permissions. Keep all providers disabled until verified; do not submit a real task yet.
3. Complete the live Hub route/payload/auth/stream/cancel/upload/output parity matrix and alternate-loopback side-by-side tests.
4. Implement resource-class admission and explicit per-node timeout, cancellation, retry/idempotency and mission aggregation policies.
5. Complete signed Pstack catalog/publisher verification and transactional extension lifecycle; add actual cosign signing and bundle publication to the release workflow.
6. Run approved dual-3090/ComfyUI contention tests and produce/hash valid image, MiniMax H3 MP4 and FlashVSR artifacts.
7. Complete physical Ubuntu install/upgrade/reboot/service-preservation/rollback, SBOM/license review and signed release acceptance.

## Provider runtime security detail — 2026-11-19

- Rechecked the current upstream configuration guides directly from the official repositories. This refines the adapter-level review above; it is still documentation/source evidence, not evidence about the live workstation.
- Hermes official `main` documents the default Docker backend as one persistent container shared across sessions and subagents. `terminal.container_persistent: false` selects a fresh container per session; `terminal.docker_network` defaults to enabled egress and `false` uses `--network=none`; `docker_mount_cwd_to_workspace`, `docker_volumes`, `docker_extra_args` and `docker_forward_env` can change mounts, runtime flags and forwarded credentials. DREAD currently rejects Docker and does not parse/verify these controls. Candidate future support must prove one DREAD task maps to one Hermes session/container, no unintended host workspace/data mounts, no Docker socket, no extra args weakening isolation, and no forwarded host secrets; network must be disabled or governed by an explicitly tested allowlist. Subagents share the parent session container, which is acceptable only within the same DREAD task. Source: https://github.com/NousResearch/hermes-agent/blob/main/website/docs/user-guide/configuration.md
- DeerFlow official `main` adds a critical AIO trust-boundary detail: the `AioSandboxProvider` DooD mode optionally mounts the host `/var/run/docker.sock` into the Gateway; that socket is root-equivalent host control. The default Compose stack does not mount it, but the AIO overlay does. The stronger Kubernetes provisioner avoids the host Docker socket, but the current DREAD adapter rejects non-empty `provisioner_url`, so that safer mode is not presently supported. AIO's network `open` mode is the compatibility default; `isolated` denies outbound traffic and `allowlist` uses a sidecar. The shipped AIO image retains a five-capability compatibility set and defaults to `seccomp=unconfined` for Chromium; CPU/memory/PID limits and `no-new-privileges` are present but do not erase the retained-capability risk. The sandbox HTTP API has no authentication and must remain on loopback/host-gateway, never `0.0.0.0`. `sandbox.mounts` can expose host paths. BoxLite uses `(user_id, thread_id)` scope and can warm-reuse a VM for the same thread. Verify the exact deployed mode, socket mounts, image digest/version, seccomp/capabilities, network mode, bind host, resource caps, user-data/skills mounts and thread IDs. Do not treat AIO as a secure boundary merely because it uses Docker. Source: https://github.com/bytedance/deer-flow/blob/main/backend/docs/CONFIGURATION.md
- OpenHands official Agent Server documentation confirms `/server_info` reports version, uptime and available tools; authenticated API requests use `X-Session-API-Key`; `OH_SECRET_KEY` must remain stable across restarts to decrypt stored conversation secrets. The docs warn that an unauthenticated Agent Server can execute commands and read/write its workspace. Verify loopback binding, key presence/rotation/storage, server version, per-conversation Docker workspace IDs, mounts, network and exact conversation tool set. Source: https://github.com/OpenHands/docs/blob/main/sdk/arch/agent-server.mdx
- These provider-specific controls are not represented by the current DREAD environment attestation booleans. Before enabling a live provider, extend the adapter/config contract and tests to capture the required settings as structured, validated values, then compare them with the live provider API/container evidence. Until that work is complete, keep live provider execution disabled.


## Universal continuation, safe discovery and master-plan review — 2026-11-21

- Continued from the 20 November chat handoff and verified that the latest supplied 44 MB offline continuation was a Run 81-derived branch, not the later Phase 6 canonical tree. The archive was inspected and its ZIP integrity verified. Its manifest warned against replacing Phase 6 with the older source. Kept the newer PR #12/#13 tree as the base and selectively integrated additive work on draft PR #14.
- Created PR #14, branch work/offline-continuation-reconcile-2026-11-21, stacked on PR #13 (work/shared-agent-execution-admission-2026-11-21), which is itself stacked on PR #12. PR #12/#13/main were not changed by the PR #14 feature branch. No merge occurred.
- Re-read and revised the controlling MASTER-PLAN.md to restore the user's product requirement: DREAD must adapt to other dual-/multi-GPU PCs and their models, agents, Pstack/plugins, media/runtime installations and host layouts. The original workstation and /mnt/sabrent path are reference evidence only, not universal defaults. Ubuntu 24.04 remains the current CI/package baseline; unvalidated OS/accelerator combinations are not advertised as supported.
- Added docs/IMPLEMENTATION-GAP-MATRIX.md as a row-by-row acceptance review. It distinguishes implemented source, tests/CI evidence and remaining live acceptance. Future source work must cite the master-plan row it advances and update the matrix after review.
- Added dread-media: read-only ComfyUI system stats/queue/recent history monitor and bounded local workflow graph/API-prompt JSON metadata discovery. URL restrictions are loopback-only; request and file scan budgets are bounded; workflow payload/widget values are not returned; no prompt is submitted.
- Added bounded local model-file inventory. Files are reported as discovered_not_loaded; this does not replace gateway-advertised model inventory and does not load/unload or start any model.
- Added read-only Cursor, Claude and generic plugin manifest discovery. Findings are untrusted and disabled; plugin hooks, commands, scripts, tools and MCP servers are never launched.
- Added read-only agent adapter manifest discovery for agent.toml and *.agent.toml files under configured/install-relative/per-user roots. It validates local loopback URL declarations, but does not probe endpoints, execute agent code, or trust manifest capability claims. All findings are disabled/review-required.
- Added dread-platform repair planner and protected GET /api/repair/plan. It probes only configured/known local components and returns bounded recommendations. All findings are proposed_not_applied, require operator approval and are never eligible for automatic application. This is a first diagnostic layer, not a complete apply/rollback engine.
- Added protected API routes: /api/models/local, /api/media/comfyui/monitor, /api/media/workflows/discover, /api/extensions/discover, /api/agents/manifests/discover, /api/repair/plan. All discovery routes sit behind the existing Bearer authentication middleware.
- Extended the React operator console with a separate Local Inventory page. It displays gateway models separately from local model files, ComfyUI queue/history, workflows, plugin manifests, agent manifests and repair proposals. Large directory scans are rate-limited to the page-level 30-second refresh and serialized so overlapping scans do not pile up.
- Added test coverage for bounded scans, symlink refusal, invalid/remote manifest rejection, disabled defaults, ComfyUI response parsing and API authentication. Added managed execution cancellation regressions from PR #13.
- Current CI evidence: run #972 (44830974105) built the React/TypeScript console successfully but failed cargo fmt --all -- --check. The Rust test and Clippy/release stages were skipped, and the package job did not run. The log also exposed a Rust compile blocker from placing pub mod repair; above the crate-level inner docs; that declaration has been moved below the docs. Reported rustfmt differences were corrected in later commits. A fresh CI run for the latest head 30dba903ac2b4e588a96f3778cfb92b74b6fa15b had not yet been reported at the time of this entry. Do not describe the latest Rust source as compiled/tested until CI proves it.
- No workstation shell/SSH connector was available. No local command, package install, API call to the workstation, service action, provider task, ComfyUI render, GPU lease or model load/unload was performed. ComfyUI, Python GPU Supervisor, legacy Python Master Hub, llama.cpp processes, databases, models and output files were not modified.
- Remaining master-plan gates remain explicit: multi-vendor stable GPU identity and resource scheduling; GGUF/projector/context metadata; live provider sandbox verification; signed extension publisher provenance and executable extension update/rollback; WebSocket render progress and image/MiniMax H3/FlashVSR artifact acceptance; resource-class mission scheduling and interrupted-run recovery; full repair simulation/approval/apply/postcondition/backup/rollback; exact Python Hub parity; signed updater and physical install/upgrade/reboot/service-preservation acceptance.


## Multi-GPU identity and installer portability review — 2026-11-21

- Rechecked current PR #14 head, installer migration, agent manifest search roots, every GpuDevice constructor and the host console against MASTER-PLAN.md and IMPLEMENTATION-GAP-MATRIX.md.
- Installer fix: removed the unconditional systemd `DREAD_GPU_GATEWAY_URL=http://127.0.0.1:11500` override. The configured gateway in the per-user environment now remains effective; if absent, the Rust client default still applies.
- Installer migration now allowlists the new discovery settings (ComfyUI URL/root, workspace, local model roots, ComfyUI user/workflow roots, plugin roots and agent manifest roots). It merges only missing recognized keys from a legacy app-local `.env`; an existing per-user setting always wins. Regression fixtures cover alternate gateway/ComfyUI ports, non-default discovery roots and conflicting legacy values.
- Agent manifest discovery now searches the actual installed `~/.config/dread-universal/agents.d` root as well as XDG/config and data roots. The scanner still does not probe or launch a discovered endpoint.
- GPU identity: `GpuDevice` now carries `device_id`, `device_index` and `pci_bus_id` separately. NVIDIA discovery requests `index,uuid,pci.bus_id` plus telemetry; it preserves the older query without compute-capability support, a lower-feature identity query, and finally the legacy name/VRAM-only fallback. When old utilities cannot report identity, the fields remain `None`. AMD currently reports its parsed `GPU[n]` enumeration index only; Apple reports an enumerated index from `system_profiler`; no stable ID is invented for either.
- Added regression coverage for two NVIDIA devices with distinct UUID and PCI IDs, plus an NVIDIA record without compute capability. The console now renders GPU enumeration index, provider ID and PCI bus address on the host inventory card.
- CI run #1008 (`44831820907`) passed the Web console job but failed Rust formatting/early parsing because `dread-agents/src/lib.rs` had `pub mod extensions;` before crate-level inner documentation. This is corrected in commit `ab0c9f4c5c8b45f92ea06cab8dc0e1609a0234e1`. Do not call the latest Rust source compiled or tested until a new current-head CI run passes `cargo fmt`, Clippy, workspace tests and release build.
- Latest source commits after run #1008 include stable GPU identity and test changes (`bc39370a...`, `a660850c...`, `4343bd38...`, `adba78d2...`, `70e65240...`), console display (`46608414...`, `88cf01c0...`) and master plan/README/matrix updates (`6469596b...`, `e5036696...`, `07db5d6a...`). Run status for the current head must be rechecked.
- Remaining acceptance: Intel device enumeration, stable AMD/Apple identity, device identity across reordering/hot-plug, heterogeneous GPU scheduling, non-NVIDIA hardware parser acceptance and live multi-GPU contention. The GPU Supervisor remains sole authority for all inference/process leases.
- No workstation service, package, model, database, output or config was modified. No agent task, ComfyUI render or GPU workload was submitted.


## GPU identity and signed release-path continuation — 2026-11-21

- Multi-GPU master-plan slice: added optional `device_id`, `device_index` and `pci_bus_id` to the host `GpuDevice` model. NVIDIA `nvidia-smi` inventory now requests index, UUID and PCI bus ID along with telemetry; older driver paths are preserved, including a final `name,memory.total` legacy fallback that reports missing identity as unknown. AMD `rocm-smi --showproductname` records a parsed enumeration index where exposed; Apple `system_profiler` records an enumeration ordinal. Neither AMD nor Apple is assigned a fabricated stable UUID/PCI address.
- Added tests for two NVIDIA devices with unique UUIDs and PCI bus addresses, and for a driver response that has UUID/PCI identity but does not return compute capability. The API serializes the identity fields and the host console displays GPU index, provider ID and PCI address separately.
- CI run #1008 (`44831820907`) had a successful Web console job but the Rust job stopped during rustfmt/check parsing on `dread-agents/src/lib.rs`: `pub mod extensions;` preceded the crate-level inner docs. The module export was moved after the `//!` docs in `ab0c9f4c5c8b45f92ea06cab8dc0e1609a0234e1`. This CI result is not a compile/test pass for the subsequent GPU identity changes.
- Upgrade path review found an unconditional systemd GPU gateway override in `install-user.sh`; it was removed so a custom loopback GPU gateway configured in the user env file is not overwritten. Installer migration now carries the new ComfyUI/workspace/model/plugin/agent discovery variables and merges only recognized missing keys from an app-local legacy `.env`, never overwriting a per-user setting. `test_install_user.sh` now covers the alternate gateway/ComfyUI endpoints and discovery roots, merge conflict resolution and backup/rollback preservation.
- Release authenticity review found that `verify-release.sh` expected a Sigstore bundle while the release workflow did not create or publish one. `.github/workflows/dread-universal-release.yml` now requests `id-token: write`, installs `sigstore/cosign-installer@v4.1.0`, signs `dist/release/SHA256SUMS` with `cosign sign-blob --yes --bundle`, runs the identity-pinned verifier in the release job and uploads the adjacent `.sigstore.json` bundle. README install instructions now require verifying the downloaded release manifest before unpacking or installing a `.deb`.
- Sigstore's current official CI quickstart documents the OIDC `id-token: write` requirement and Cosign's keyless `sign-blob --bundle`/`verify-blob` flow; the official installer README currently shows cosign-installer v4.1.0. Sources: https://docs.sigstore.dev/quickstart/quickstart-ci/ and https://github.com/sigstore/cosign-installer.
- These are source-level changes only. No tagged release signing workflow has been executed yet, no signed public release has been produced, no download verification has been run on a clean user machine, and no workstation install/upgrade/reboot or failed-upgrade rollback has been accepted.
- New GPU identity and release-signing changes are committed on PR #14 branch `work/offline-continuation-reconcile-2026-11-21`. Latest inspected head at this log entry is `93cabe79b8a15964fa7295d12f7241a5270aa23a`; fetch a new Actions run for that exact SHA before claiming Rust build/test or release validation.
- Remaining: Intel device enumeration, stable AMD/Apple identity and cross-boot/reorder identity, heterogeneous scheduling, actual dual-/multi-GPU contention tests on more than the reference host, release workflow OIDC signing run and external signature verification, Windows/additional Linux platform adapters, full repair apply/postcondition/backup/rollback, real WebSocket/media artifacts and Hub parity.


## GGUF compile corrections, cache symlinks and host-root portability — 2026-11-21

- Hosted CI run #1062 (`44833669242`) reached Rust formatting successfully but was cancelled when a later branch commit arrived; it did not provide Clippy/test/build evidence. The Web job had installed locked dependencies but its build step was cancelled.
- Hosted CI run #1067 (`44833946946`) for head `89fb6cb2cea576ca454e81125a7a8c9a9ab3ca74` passed `cargo fmt --all -- --check` and the React/TypeScript build. Clippy compile found three source errors: ambiguous `serde_json::from_slice` types in media workflow and plugin parsing, and the fallback model-kind function's incorrect `&'static str` lifetime. The two JSON calls now use `from_slice::<Value>`, and `infer_kind_from_path` returns the caller-provided fallback lifetime.
- Hosted CI run #1072 (`44834108528`) for head `2edf26387e1e5625061d8ef2359cb099bab8d89a` again passed formatting and Web build; strict Clippy found only four `manual_range_patterns` warnings in GGUF value-type width tables. They were changed to the equivalent inclusive ranges `4..=6` and `10..=12`.
- Added bounded Hugging Face model-cache symlink support: the scanner canonicalizes each entry, follows only targets still under the configured model root, deduplicates canonical directories/files, and rejects broken, cyclic or outside-root targets. It preserves the visible `snapshots/<revision>/model.safetensors` path for classification while reading file metadata from the in-root blob target. The existing external-symlink test remains and a new in-root snapshot-to-blob symlink regression test was added.
- Model/workflow/plugin discovery now consults `USERPROFILE` as a fallback to `HOME`, and `LOCALAPPDATA`/`APPDATA` roots where present. Explicit discovery-root environment variables remain preferred. This improves cross-host discovery but is not an assertion that Windows installation, process management, service control or packaging is accepted.
- Updated the controlling master plan and implementation matrix with GGUF header parsing, safe in-root model-cache symlinks and environment-based user roots. Workflow and plugin manifest discovery still rejects symlinks; they do not need Hugging Face cache aliases.
- Latest source head after these changes and docs: see current PR #14 head. No fresh complete green CI run has yet been confirmed for that latest head. Do not mark strict Clippy, Rust tests, release build, installer tests or signed release workflow as passing until current-head Actions confirms them.
- All changes remain isolated to draft PR #14 stacked on PR #13/#12. No workstation was accessed or changed; no package install, service action, model load/unload, GPU lease, agent execution or ComfyUI render occurred.


## Full Ubuntu CI green after installer-test cleanup — 2026-11-21

- Current PR #14 head `977024fc5d05f5799e74e3604a404bfb0dbd5442` passed GitHub Actions run #1149 (`44840250093`) across every job.
- Rust / Ubuntu 24.04: `cargo fmt --all -- --check` passed; strict `cargo clippy --workspace --all-targets --locked -- -D warnings` passed; `cargo test --workspace --locked` passed for the full workspace; `cargo build --workspace --release --locked` passed; API release binary artifact uploaded.
- Web / Ubuntu 24.04: committed npm lockfile verified; `npm ci --ignore-scripts --no-audit --no-fund` passed; production `npm run build` passed and web artifact uploaded.
- Rust-job integration checks passed: shell syntax for installer/package/release scripts, signed-release verifier failure-path tests, portable install migration/upgrade/rollback including watchdog-unit preservation, Python bridge/importer tests (the new watchdog tests run under this test discovery), and the static single-owner GPU boundary guard.
- Ubuntu package job passed: assembled `.deb` and portable tarball, verified package checksums and contents, then installed and smoke-tested packaged API plus production console. All three jobs concluded success.
- The previous installer failure was a test-fixture defect, not an installer defect. `scripts/test_install_user.sh` had accumulated 23 duplicate copies of the upgrade/rollback tail and an accidental stray command after the first full test flow. Replaced it with one clean 213-line regression flow, made the mock systemctl logger optional for direct calls, and extended failed-upgrade rollback checks to include both watchdog service and timer files. Run #1149 passed the corrected test.
- Evidence boundary: this validates Ubuntu 24.04 CI and a disposable package smoke-test environment only. It does not prove install/upgrade on the user's physical workstation, Windows/macOS/other distro support, real dual-GPU contention, signed tagged release publication, or live media/provider task acceptance.
- PR #14 remains draft/open and unmerged. No local workstation service, ComfyUI instance, Python GPU Supervisor, model file, database or user output was modified.


## PR #10 Supervisor telemetry restored — 2026-11-21

- The older PR #10 branch contained an authenticated `GpuSupervisorClient::telemetry()` call and `/api/gpu/telemetry` route that were absent from the later PR #12/#13 Phase 6 base. Restored them additively on PR #14.
- `GpuSupervisorClient::telemetry()` calls only `GET /v1/telemetry` on the already configured loopback bridge with its existing Bearer token, 4-second timeout and 1 MiB response limit (both declared content length and streamed chunks). Non-success HTTP and invalid JSON fail closed. It does not acquire, release or change a GPU lease.
- Added protected `GET /api/gpu/telemetry`; no configured Supervisor returns 503. Mock tests assert the Bearer token on the bridge request and return a clearly `read_only` sample. API test checks the forwarded payload; `/api/gpu/telemetry` is also in the protected-route auth regression.
- The React GPU page now has a distinct Python GPU Supervisor telemetry panel showing the actual bridge sample source/time and utilization, VRAM, temperature, power and PCIe fields. It refreshes every 30 seconds only while the GPU page is open and clears on disconnect. Host `/api/system/profile` remains separately rendered; the bridge feed is not conflated with host discovery.
- README, master plan and implementation matrix were updated to distinguish host discovery and Supervisor runtime telemetry and to preserve the single Supervisor GPU-lease authority.
- Latest full green CI baseline before this telemetry change: run #1149 (`44840250093`) passed all three jobs on commit `977024fc5d05f5799e74e3604a404bfb0dbd5442`, including Rust fmt/Clippy/workspace tests/release build, Web build, installer rollback/watchdog tests, release-verifier tests, GPU single-owner guard and Ubuntu package smoke test. The subsequent telemetry code/docs are newer and must be revalidated by a new current-head run before claiming those changes pass.
- No workstation services, GPU leases, models, configs, databases or output files were accessed or modified. PR #14 remains draft/open/unmerged.


## Supervisor telemetry full Ubuntu CI validation — 2026-11-21

- Current PR #14 head `96e0e28a0285594b3d1e5565aed66ab67e7142fc` passed GitHub Actions run #1168 (`44840949476`) across all three Ubuntu 24.04 jobs.
- Rust job passed: `cargo fmt --all -- --check`; strict `cargo clippy --workspace --all-targets --locked -- -D warnings`; `cargo test --workspace --locked`; optimized `cargo build --workspace --release --locked`; shell syntax; release-verifier negative-path tests; portable installer migration/rollback (including watchdog-unit preservation); Python bridge/importer/watchdog tests; and the static single-owner GPU boundary guard.
- Web job passed: lockfile verification, locked dependency install and production Vite/TypeScript build.
- Ubuntu package job passed: `.deb`/portable bundle assembly, checksum/content verification, install and packaged API + production console smoke test.
- This validates the Supervisor telemetry changes in CI: authenticated `GET /v1/telemetry` bridge call with 4-second timeout and 1 MiB response cap; protected Rust `GET /api/gpu/telemetry`; Bearer-auth mock test; separate GPU console panel refreshing only while the GPU page is open.
- The frontend build initially failed on the telemetry panel's escaped JSX template literals. Those were corrected; a later Rust Clippy run also caught a production-scope `HeaderMap` import used only by tests. The import was moved into the test module. Run #1168 passed the corrected current head.
- Remaining evidence boundary: this is an Ubuntu 24.04 CI and disposable package install/smoke pass. Actual connection to the user's existing Python Supervisor, NVIDIA/AMD/Intel/Apple host runtime, multi-GPU contention, Windows/other-distro installation and real watchdog opt-in timer are still live host acceptance items. No workstation/service/model/GPU lease was touched.


## Intel XPU inventory adapter — full CI validation — 2026-11-21

- Current PR #14 source head `8efecbe99375d7270205a53ef19a8a0ea691db0a` passed GitHub Actions run #1196 (`44842488739`) with all three Ubuntu 24.04 jobs successful.
- Rust job passed `cargo fmt --all -- --check`, strict Clippy (`--workspace --all-targets --locked -- -D warnings`), the complete locked workspace test suite, optimized release build, installer/release shell validation, signed-release verifier negative-path tests, portable installer migration/rollback, Python bridge/importer/watchdog regression tests and the static single-owner GPU boundary guard.
- Web job passed lockfile verification, locked `npm ci` and the production TypeScript/Vite build.
- Ubuntu package job assembled the `.deb` and portable tarball, verified package checksums/content, then installed and smoke-tested the packaged API and production console.
- Intel inventory implementation on this head: preferred `xpu-smi --timeout=8 query --json` maps per-device name, UUID, index, PCI BDF, FB memory total/used/free, UMD version, PCIe current/max, GPU utilization, temperature and average power. If unavailable, `xpu-smi --timeout=6 discovery --json` is static inventory only; if JSON parsing is unavailable, `--query-gpu=index,name,uuid,pci.bus_id` CSV is identity-only. All commands are read-only; no XPU config/reset/firmware operation is used.
- Tests cover two query JSON devices, identity and telemetry mapping, explicit absence values, discovery memory unit conversion, static discovery fallback and identity-only CSV fallback. The previous CI failures were limited to one strict-Clippy range idiom, a test-only `json!` macro import, then rustfmt's preferred wrapping. They are corrected on the passing head.
- Evidence boundary: no Intel GPU or Intel driver was present in the CI runner, so this validates parsing/build/packaging, not live Intel hardware behavior. Physical Intel XPU acceptance, driver-version compatibility, AMD/Apple stable identity and real heterogeneous multi-GPU contention remain open. No workstation service, ComfyUI, GPU Supervisor, model, database or output was touched.


## AMD SMI inventory, telemetry and CUID privacy — full CI validation — 2026-11-21

- Current PR #14 head `a744380500758298b1cc7be8c6fa563535e50908` passed GitHub Actions run #1216 (`44844349840`) with all three Ubuntu 24.04 jobs successful.
- Rust job passed `cargo fmt --all -- --check`, strict workspace Clippy with warnings denied, the full locked workspace test suite, optimized release build, installer/release shell validation, signed-release verifier failure-path tests, portable installer migration/rollback, Python bridge/importer/watchdog regressions and the static single-owner GPU boundary guard.
- Web job passed npm lockfile verification, locked dependency installation and production TypeScript/Vite build.
- Ubuntu package job assembled `.deb` and portable tarball, verified package checksums/content, then installed and smoke-tested the packaged API and production console.
- AMD host discovery now prefers `amd-smi list --json`, `static --json`, `metric --json`, joining per-GPU reports by the AMD tool's enumeration index. It maps BDF, provider CUID/UUID alias, product/ASIC name, driver, VRAM, GFX usage, temperature, socket power and current/max PCIe where available. Units are preserved and normalized; missing values remain unknown. Older installations fall back to `rocm-smi --showproductname` for identity only.
- Identity privacy: the adapter prefers the list's derived `cuid`, falls back to the CUID-first `uuid` compatibility alias and does not read/export static `ASIC_SERIAL` or privileged `PRIMARY_CUID`. If neither CUID nor UUID alias is present, `device_id` remains `None`. Enumeration index is not treated as a persistent identity.
- Regression tests include multi-GPU report joining, unit-bearing object/string parsing (`MB`, `GB`, `W`, `mW`, `GT/s`), N/A handling, static vs runtime values and a raw-ASIC-serial exclusion case. A CI failure exposed that parsing at the first alphabetic character truncated `MB` to `B`; the parser now splits at whitespace and the direct string-unit regressions cover it.
- Evidence boundary: the CI runner did not have physical AMD GPUs or AMD drivers installed; this validates parser/build/package behavior, not actual AMD hardware. Real AMD SMI versions, vendor/driver compatibility, CUID persistence semantics, Intel/AMD multi-GPU scheduling and live GPU contention require host acceptance. PR #14 remains draft/open/unmerged; no local workstation, ComfyUI, Supervisor, model, database or output was touched.
- Official AMD CLI references used for the field/command contract: https://rocmdocs.amd.com/projects/amdsmi/en/develop/how-to/amdsmi-cli-tool.html and https://rocmdocs.amd.com/projects/amdsmi/en/develop/reference/changelog.html.


## Portable bounded host-probe hardening — 2026-11-21

- Replaced the optional GNU `timeout` wrapper/fallback with an in-process Rust deadline around allowlisted host utility execution. The runner captures stdout only, discards stderr, enforces a 4 MiB retained-output ceiling, kills and reaps the direct child on timeout, and rejects oversized output.
- Added Unix regression tests for a stalled process without GNU `timeout` and output exceeding the capture budget. The timeout remains a defensive process bound in addition to Intel XPU Manager's documented per-command no-progress timeout.
- Formatting, strict Clippy, workspace tests, optimized release build and package smoke tests passed on the preceding commit `447cf43e34e0d423f17e6a387181cdf5cc83ab92` (PR run #1222, `44846314057`). The additional output-budget regression test is on `90ee726a5def06c843219b1be6937afb96545aac`; its push and PR CI runs #1234 (`44851596135`) and #1235 (`44851601283`) were still running when this entry was written. Do not treat the new test as validated until those exact-head runs pass.
- The web production build has passed on the new head's push run; Rust gates and the full PR run remain pending.
- Safety boundary: no host utility is run against the user's workstation by this code change. The test validates the Rust process wrapper in Ubuntu CI only; it does not establish process-tree termination on all platforms or live GPU-driver behavior. No workstation services, ComfyUI, Python GPU Supervisor, model, database or user output was touched.


## Provider cancellation truthfulness — 2026-11-21

- Active agent-provider tasks now transition to `cancelling` when the operator requests cancellation. The API no longer reports them as `cancelled` before the remote provider has confirmed a terminal cancelled state.
- The worker submits the provider cancellation request once, persists an explicit `task_cancellation_unconfirmed` event if the provider refuses, returns a non-terminal state, or is temporarily unreachable, and continues polling the provider's durable status. A provider race that completes successfully is recorded as completed rather than being overwritten as cancelled.
- Provider start requests are awaited to a definitive response before attempting cancellation, avoiding a dropped HTTP start future that could leave an accepted remote task without a recorded provider ID.
- The console now counts `cancelling` tasks as active and displays a pending-confirmation notice instead of inviting duplicate cancellation.
- Added a focused state-transition unit test. Exact-head Rust formatting, strict Clippy, workspace tests, release build and package smoke tests must pass before this change is considered CI-verified.
- Scope boundary: this change covers agent-provider tasks with a registered provider identity. Built-in GPU inference cancellation remains best-effort because the current Python Supervisor bridge cannot interrupt an already-running upstream generation; do not represent that path as provider-confirmed cancellation.
- No workstation services, ComfyUI, Python GPU Supervisor, models, database or user outputs were accessed or modified.
