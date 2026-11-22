# PROJECT DREAD UNIVERSAL — Autonomous Engineering Handoff
Date: 2026-11-22
Repository: https://github.com/dreaddo151-sys/koko
Handoff branch: `handoff/project-dread-universal-agent-2026-11-22`
Base checkpoint: `cd7540629d562cef4b3bf3019e1085b2e0edc24a`
Related PR: https://github.com/dreaddo151-sys/koko/pull/16

## Mission
Continue the existing PROJECT DREAD UNIVERSAL implementation. Do not restart, replace the architecture, or treat this as a greenfield project. Prioritize real Rust implementation, integration, tests, and a releasable artifact. Work autonomously in this branch, make small reviewable commits, and report exact commit SHAs and CI runs.

## Preservation and safety rules
- Preserve all existing source history, Kimi 3 Pro code deliveries, corrections, integrations, architecture decisions, and test evidence.
- Do not overwrite existing files without first inspecting them and preserving a recoverable prior version.
- Do not delete models, workflows, databases, sessions, outputs, backups, or project assets.
- Do not replace working runtimes or introduce a second GPU scheduler. The existing DREAD GPU Supervisor remains the sole GPU ownership authority.
- Do not stop or restart the user's existing ComfyUI or legacy DREAD Hub services as part of repository work.
- Keep workstation-specific deployment separate from this portable/universal codebase.
- Mark every claim as confirmed, inferred, or unverified. Never claim a feature is complete based only on design notes.
- Do not open the startup dispatch barrier until all safety conditions below are implemented and tested.

## Source and checkpoint status
The latest recorded GitHub engineering checkpoint before this handoff is `cd7540629d562cef4b3bf3019e1085b2e0edc24a` on branch `work/kimi003-phase6-api-integration-2026-11-22`. GitHub Actions run #1567 (run ID `45002540405`) completed successfully across three jobs:
1. Rust / Ubuntu 24.04: formatting, strict Clippy, workspace tests, release build, installer/release checks, signed verifier failure paths, migration/rollback, Python bridge, and single-GPU-supervisor ownership checks.
2. Web console / Ubuntu 24.04: npm lockfile, dependency install, console build.
3. Ubuntu packages / smoke test: Debian and portable bundle assembly, checksum/content validation, install and packaged API + production console smoke tests.

This confirms the checkpoint commit passed those jobs. It does NOT establish that subsequent changes are verified or that the whole product is complete.

A previously recorded source handoff workspace exists at:
`/mnt/sabrent/dread_workspace/PROJECT-DREAD-SOURCE-HANDOFF/dread_universal`
with branch `workstation-source-handoff-20261122` and base HEAD `7559ac305e116da7a73c24394b1ccf9bb21dae79`. This is a distinct workstation handoff snapshot; do not confuse it with the GitHub branch above.

The user supplied multiple Kimi 3 Pro code deliveries, batch material (including batches 0–4), a ZIP described as “PROJECT DREAD UNIVERSAL — Offline Continuation” (Nov 20, 2026; approximately 44 MB and 3319 archive entries), and a supporting text file. Treat these as upstream implementation material to reconcile and preserve, not as replacements for the canonical project. This handoff records known context; it does not claim that every byte of every earlier attachment has been imported into this branch. Retrieve/reconcile the actual source artifacts from the repository and available project files before making integration claims.

## Confirmed architecture findings at the checkpoint
### Agent evidence and signing
- `dread_universal/crates/dread-agents/src/lib.rs` contains `AgentOperationEvidencePayload`, `SignedAgentOperationEvidence`, Ed25519 `AgentReceiptSigner`, and `signed_agent_operation_receipt`.
- The signed receipt attests to the local adapter's observation after authenticated transport. It is NOT a provider-origin signature and is NOT independent verification of the task result.
- `AgentAdapter` has fail-closed defaults for evidence-aware start, status, events, and cancellation operations.
- Hermes, DeerFlow, and OpenHands adapters have evidence-aware operations and receipt signing.

### Execution authority
- `dread_universal/crates/dread-api/src/execution_authority.rs` defines `Phase6ExecutionAuthority`.
- It checks execution policy, provider registration/capabilities, receipt signatures against registration-pinned keys, admission/attempt identities, expiry, event digest and identities.
- `verify_independent_result` deliberately fails closed when no independent verifier is configured, with the error that independent result verification is not configured and provider success remains unverified.

### Durable persistence
- `dread_universal/crates/dread-orchestrator/src/persistence.rs` contains SQLite durable registry/state machinery including authorization/admission, attempt preparation, provider acknowledgements, status/events/terminal persistence, independent verification records, cancellation progression, and startup recovery.
- Preserve the existing schema, migrations, idempotency rules, and transaction boundaries. Inspect before changing.

### API startup/dispatch barrier
- `dread_universal/crates/dread-api/src/main.rs` has startup recovery hooks and an intentionally closed dispatch barrier.
- The API queue path calls `ensure_durable_agent_dispatch_ready(&tx)`; current comments indicate the barrier remains closed until durable admission/attempt preparation, recovery attachment, and provider lifecycle persistence are integrated.
- Startup currently performs recovery setup and leaves `startup_dispatch_barrier_open = false`, logging that provider dispatch is blocked pending durable dispatch and worker reattachment.
- Existing tests verify legacy dispatch is blocked while the barrier is closed.
- This barrier is a deliberate safety control. Do not bypass it to make a demo appear to work.

## Required implementation sequence
### P0 — Inspect and reconcile before editing
1. Fetch the complete tree and current branch state. Compare this handoff branch to the base checkpoint.
2. Inspect PR #16 and related commits, including closed/open PRs relevant to Phase 6. Preserve useful Kimi-delivered implementations and their history.
3. Identify actual remaining gaps from source and tests. Do not reimplement existing completed modules.
4. Record a compact file-level plan and proceed directly to implementation.

### P1 — Wire durable API dispatch
1. Integrate the Axum API task dispatch handler with the existing durable SQLite admission and attempt-preparation APIs.
2. Ensure the authorization/admission decision and attempt identity are persisted transactionally before any provider side effect.
3. Enforce strict idempotency: only a newly created dispatch preparation may invoke provider start. An existing preparation must never issue a second start.
4. Persist the provider acknowledgement/receipt before polling or advancing state.
5. Never accept caller-supplied evidence as trusted provider evidence; derive evidence through the authenticated adapter and configured signer.

### P2 — Durable provider lifecycle
1. Wire status, events, cancellation, and terminal outcomes through evidence-aware adapter methods.
2. Verify every receipt against the registration-pinned key and exact task/admission/attempt identity.
3. Persist lifecycle transitions and deduplication/event cursors before exposing state changes.
4. Fail closed on missing, expired, mismatched, replayed, malformed, or invalidly signed receipts.
5. Ensure retries cannot duplicate provider work or move a task to success without required evidence.

### P3 — Crash recovery and worker reattachment
1. On startup, enumerate all nonterminal durable attempts and reconcile their exact provider/task identities.
2. Reattach to existing remote work using durable identifiers; do not blindly resubmit start calls for ambiguous attempts.
3. Persist reconciliation outcomes and startup generation/attachment state.
4. Handle ambiguous submission, lost acknowledgement, interrupted polling, cancellation races, and terminal receipt replay.
5. Add fault-injection tests for crashes at each boundary: before provider call, after provider accepts but before ack persistence, after ack persistence, during polling, during cancellation, and during terminal persistence.

### P4 — Independent result verification
1. Implement a distinct independent verifier contract and trusted verifier registration/configuration.
2. Keep verifier identity and trust material separate from requester, worker, and provider adapter receipt signer.
3. Verify result artifacts/content against durable task requirements and provenance; do not equate provider-reported success with verified success.
4. Persist verification evidence and bind it to exact task/attempt/result digest.
5. Keep mission dependency advancement and final success fail-closed until independent verification passes.
6. Add negative tests for verifier unavailable, wrong key, mismatched digest, wrong attempt, stale receipt, and provider-only success.

### P5 — Startup barrier and integration
Only open the dispatch barrier after:
- durable admission and attempt preparation are wired in the production API path;
- provider start is protected by exactly-once/idempotent preparation;
- provider acknowledgement/status/events/cancel/terminal receipts are authenticated and persisted;
- every nonterminal attempt can be safely recovered/reattached;
- independent result verification is configured and enforced for success;
- integration and crash-recovery tests pass.

If any condition is not met, leave the barrier closed and explain why.

### P6 — Full validation and release
Run the repository's canonical checks, including at minimum:
- formatting check;
- strict Clippy with warnings denied (use project CI configuration);
- complete Rust workspace tests;
- release build;
- Python bridge and GPU ownership checks;
- web console build;
- installer/migration/rollback checks;
- Debian and portable package assembly;
- checksum/content validation and packaged smoke tests.

Use online GitHub Actions if the coding environment lacks cargo/rustc. Do not state a check passed until the exact commit's run is completed successfully. Capture exact run IDs, job names, commit SHA, and failures.

## Broader universal project integration context
The intended universal hub includes model discovery/loading controls, agent/plugin/skill discovery and selection, Pstack integration and potato agent, self-repair, TPS/GPU/temperature/load telemetry, media generation section, D-SPINE, OpenHands, token booster, and a reliable chat input/UI. Keep it hardware/provider agnostic and extensible; preserve existing integrations rather than tailoring the product to one workstation. Reconcile this list with the actual repository and Kimi source before treating any item as absent or complete.

Existing workstation environment (deployment context only; do not assume access from this cloud branch):
- Workspace root: `/mnt/sabrent/dread_workspace`
- Existing DREAD Hub: `dread_web_hub.py`, loopback port 8080
- ComfyUI: port 8188
- Ollama: port 11434
- DREAD GPU gateway: port 11500
- llama.cpp target: port 11501
- Existing GPU Supervisor: `dread_gpu_supervisor.py` and its existing lease/ownership model
- Dual RTX 3090 system
Do not make workstation changes from this GitHub task unless explicitly authorized and access is actually available.

## Required working method
- Work in this handoff branch or a clearly named child branch. Do not push directly to main or merge PR #16 without review and green checks.
- Make actual code changes, not only plans or docs.
- Before modifying a file, inspect its current content and relevant tests.
- Prefer small coherent commits; include tests with each behavior change.
- Do not make broad speculative refactors.
- When a test fails, inspect the failing assertion/log and fix the root cause.
- Maintain a checklist in the PR description or a progress file with exact commit references.
- If a tool limitation prevents implementation, state the blocker and the exact next action; do not pretend the work ran.
- Do not ask the user to repeat known project context or upload everything again unless a specific missing source file is genuinely required.

## Definition of done
PROJECT DREAD UNIVERSAL is not complete merely because a branch exists, a plan is written, or one CI run is green. Completion requires:
1. Production API durable dispatch and provider lifecycle integration.
2. Safe crash recovery and worker reattachment.
3. Independent verification before task/mission success.
4. Startup barrier opened only after all safety gates pass.
5. Universal hub and agent/plugin/model/media integrations reconciled against the intended feature set.
6. Full Rust, bridge, UI, packaging, migration, and smoke-test suite green on the final release commit.
7. Documentation and a reproducible release artifact with checksums.
8. Exact final commit and CI evidence.

## First action for the receiving agent
Fetch and inspect this branch and the source tree. Then implement P1 in the actual Rust API/orchestrator path, reusing existing persistence and evidence primitives. Do not spend the first hour writing another audit report. End the first work session with a code commit and tests, or a precise blocking failure with logs.
