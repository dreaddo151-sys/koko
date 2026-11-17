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
- PR #12 uses FNV-1a as local change detector, not cryptographic signature. Replace with SHA-256 for production provenance/update integrity.

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
- FNV fingerprint is not cryptographic.
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
