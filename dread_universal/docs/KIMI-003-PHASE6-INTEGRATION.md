# Kimi 003A/003B concepts adapted to the Phase 6 execution kernel

**Status:** isolated integration candidate; not production-wired or accepted.

## Provenance

- Original Kimi transcript: [KIMI3_COMPLETE_TRANSCRIPT.md](https://github.com/dreaddo151-sys/koko/blob/handoff-kimi3-pro-all-chat/dread_universal/handoff/kimi3/KIMI3_COMPLETE_TRANSCRIPT.md)
- Transcript blob: `58ffd484238542c8874154aef86fb376f408ba8a`
- Kimi correction-cycle final section begins at transcript line 16950.
- Canonical Phase 6 base: `000f1003c681091c6160d7c49bcf9213d7f9b724` (PR #14 stacked on PR #13 and #12).
- This branch is a separate review candidate. It does not modify PR #12, #13, #14, the handoff archive branch, or the user's workstation checkout.

## What is added

- `dread-orchestrator::lifecycle`: provider outcome, reconciliation, cancellation and independent-verification states.
- `dread-orchestrator::persistence`: additive provider admission, per-attempt identity, event, evidence, retry and verification records.
- `dread-orchestrator::reconcile`: read-only provider status reconciliation. There is deliberately no provider submission method in the reconciliation source contract.
- Private execution migrations `0001`–`0004`. They create `dread_*` metadata tables only and do not rename/create the canonical Phase 6 `tasks`, `task_events`, `task_results`, `missions`, or `mission_tasks` tables.
- The existing shared `ExecutionGate` remains untouched; Python GPU Supervisor remains the sole GPU lease/process authority.

## Why this is an adaptation, not a blind Kimi merge

Kimi's final `CanonicalSchema` expects mission/task columns that do not exist in Phase 6. The adapter in this candidate validates and uses the existing Phase 6 schema, while storing only additional execution facts in the `dread_*` namespace. The incomplete final Kimi `store.rs` and missing final Kimi `reconcile.rs` are not represented as complete original files here.

## Known non-goals for this candidate

This commit does **not** install or wire an `ExecutionAuthority` into `dread-api`. It does not change the current API dispatch worker, recovery worker, cancellation route, mission scheduler, provider adapters, ComfyUI, media routes, or GPU Supervisor. It must not be enabled for real provider dispatch until that integration is separately reviewed and tested.

## Verification

- Candidate SQLite schema tests, executed before this branch commit: 14 passed.
- Rust format, compile, unit tests, Clippy and release build: pending exact-head GitHub Actions for this branch.
- Live provider, browser, GPU, ComfyUI/media and workstation acceptance: not performed by this candidate.


## Phase 6 canonical integration follow-up — 2026-11-22

The original candidate boundary above describes PR #15 at the time it was authored. The stacked PR #16 subsequently wired the adapted durable execution path into the canonical API:

- `dread-api::durable_agent` performs single-use admission and attempt preparation before provider start; only a newly created attempt can call `start_task`.
- `dread-api::execution_authority` supplies signed provider, capability, task-approval, policy, retry-contract, receipt and runtime-attestation checks.
- API startup installs additive execution migrations on the existing Phase 6 SQLite connection, starts a new recovery generation and keeps the startup dispatch barrier closed pending reconciliation and worker attachment.
- The read-only reconciliation source contract has no provider submission method. Unknown remote outcomes remain nonterminal; successful provider results remain `provider_succeeded` until an independent result receipt verifies the exact result/error bytes.
- The standalone `dread-result-verifier` producer is packaged but is not installed or activated on the user's workstation. The API refuses new provider admission without a fresh independently signed verifier heartbeat.
- The Python GPU Supervisor remains the sole hardware lease/process authority; no second GPU scheduler was introduced.

Latest exact-head CI evidence is recorded in checkpoint section 19: PR #16 head `1d3c515133bf9b027e260657688f0b6e74dd9f6b`, run #1968, all Rust, web and package jobs successful. Subsequent checkpoint-only documentation commits do not change the verified code.

The remaining non-CI acceptance gates remain open: independent live runtime-isolation observations, real per-capability domain semantics, separate service-account deployment/ACL tests, provider crash/race/replay tests, and physical workstation/ComfyUI/GPU acceptance. This follow-up does not authorize dispatch activation or merging.


### Mission reconciliation hold follow-up

- Mission dispatch now checks for sibling attempts in `reconciliation_required` before dispatching another task in that mission.
- The check is inside the same SQLite dispatch transaction and therefore cannot race with the mission status update that follows it.
- Exact-head regression and full CI passed in run #1980 on `e783e2e6b3758bc90c622a1062f12a73adce2952`. The test proves that an unresolved sibling prevents the mission from returning to `running`.
