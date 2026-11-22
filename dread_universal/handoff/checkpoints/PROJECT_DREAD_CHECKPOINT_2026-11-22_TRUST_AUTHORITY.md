# PROJECT DREAD UNIVERSAL — Continuation Checkpoint

**Checkpoint timestamp:** 2026-11-22 (UTC)  
**Purpose:** resume the active engineering work from this exact state. Do not restart the project review or reconstruct the source history from zero.

## 1. Canonical source and Git state

- **Repository:** `dreaddo151-sys/koko`
- **Active PR:** [#16 — Durable execution integration](https://github.com/dreaddo151-sys/koko/pull/16)
- **PR status:** Open, draft. Do not merge or mark complete.
- **Working branch:** `work/kimi003-phase6-api-integration-2026-11-22`
- **PR base:** `07051e097ebb0daeb1c1e62e4ad557c81d088439` (PR #15 durable-execution candidate)
- **Exact checkpoint HEAD:** `756a3e712d2c74cba7c417a7eefdaee6064b83c9`

This is the eight-crate Phase 6 GitHub source. Keep it distinct from the historical seven-crate workstation snapshot at:

`/mnt/sabrent/dread_workspace/PROJECT-DREAD-SOURCE-HANDOFF/dread_universal`

Do not replace the Phase 6 source with that older checkout. Preserve PRs #12–#16 and their commit history; do not squash or rewrite the earlier deliveries.

### Current source fingerprints

| Path | SHA at checkpoint |
|---|---|
| `dread_universal/crates/dread-api/src/main.rs` | `9c9514c1ecb318d11bef0bde0950a901aed95e48` |
| `dread_universal/crates/dread-api/src/execution_authority.rs` | `05b3eb9d07fdbeba60c3406bc671afd2603a9c7f` |
| `dread_universal/crates/dread-orchestrator/src/persistence.rs` | `8378f972712f34ca1e01babdfd58c0c3bffce305` |
| `dread_universal/crates/dread-orchestrator/migrations/0005_startup_recovery_generation.sql` | `e40291c6ea32cdbf71aa040c89cdc6bbd0e34b5b` |
| `dread_universal/docs/EXECUTION_TRUST_MANIFEST.md` | `253df8e5e69749538a4b8057998e4f6d08736dc4` |

Always fetch the current branch file/SHA before editing. GitHub file updates replace full file contents, so preserve the complete fetched source and current SHA on every update.

## 2. Historical material already located — do not ask the user to resend

- Master engineering record: `PROJECT_DREAD_UNIVERSAL_MASTER_ENGINEERING_RECORD.md`
- Phase 6 baseline audit: `PROJECT_DREAD_UNIVERSAL_PHASE6_BASELINE_AUDIT_20261122.md`
- Phase 6 continuation / integration notes: `PROJECT_DREAD_PHASE6_CHAT_WORK`
- Kimi 3 Pro archive branch: `handoff-kimi3-pro-all-chat`
- Kimi transcript: `dread_universal/handoff/kimi3/KIMI3_COMPLETE_TRANSCRIPT.md`
- Kimi continuation correction notes: `KIMI3_CONTINUATION_REQUIREMENTS_003A_003B.md`
- The transcript archive contains Batches 0–4 and Correction Cycles 003A/003B, plus multiple versions of preflight/execution files. Compare the versions and use the corrected/latest requirements as design input; do not blindly copy an older draft over integrated Phase 6 files.
- User's PC/hardware, API, GitHub PR, source bundle and integration context is in the project records and prior conversation. Do not repeat broad discovery or ask the user to provide it again unless a genuinely missing file/source is identified.

## 3. What was implemented in this session

### A. Signed provider execution trust authority

Added `crates/dread-api/src/execution_authority.rs` and wired it into `main.rs`.

The implementation includes:

- Ed25519 verification of a bounded JSON execution-trust envelope using a server-configured public-key trust anchor.
- Signed provider identity and revision, adapter ID/version, endpoint, capability ID/revision, operation, enabled/revoked state, expiry and retry-contract metadata.
- Exact live adapter descriptor matching against the signed registration.
- Provider IDs constrained to the reviewed known agents: Hermes, DeerFlow and OpenHands.
- Canonical capability checks against the existing `capabilities` record and most-recent `capability_events` approval event.
- Task approval checks against the canonical `tasks` row and `task_events` approval actor/payload; revalidation also checks the canonical mission mapping.
- A protected `GET /api/execution/trust` status endpoint.
- Manifest reload/reverification from the configured non-symlink regular file on each authority check, so expiry, replacement and revocation are not merely cached until restart.
- Fail-closed independent result verification: it returns an error because no independent verifier integration exists yet.
- Signed retry-contract metadata, including digest, verifier/source references, idempotency class and maximum attempt count.

Configuration names:
- `DREAD_EXECUTION_TRUST_MANIFEST`
- `DREAD_EXECUTION_TRUST_PUBLIC_KEY_HEX`

Both must be provided together. If neither is present, provider execution trust is unavailable. If only one is present or the manifest cannot be verified, API startup fails closed.

### B. Granular isolation evidence (latest changes; CI pending)

The trust manifest was strengthened from eight boolean flags to the Kimi-style per-control evidence representation:

- `IsolationControl`: runtime identity, task workspace boundary, mount policy, egress policy, tool permissions, stable secret handling, cancellation propagation, restart reconciliation.
- `ControlEvidence`: verified state plus durable observation identity for each control.
- `IsolationEvidenceRecord`: contract version, evidence ID, provider registration revision, adapter identity/version, runtime identity, provenance, control map and checked/expiry times.
- `EvidenceProvenance`: source observation ID, recording adapter identity/version, SHA-256 evidence digest, timestamp and authenticated flag. This entire record is covered by the outer Ed25519 signature.
- All eight controls must be present, verified and have durable observation IDs.
- Evidence must match the signed provider registration and adapter and remain within the 300-second preflight maximum lifetime.

This is a signed provisioner/trust-authority assertion, not proof produced by an already-integrated runtime verifier. The next stage must connect the trust issuer to real adapter/runtime observations. Do not claim that the current workstation has passed these isolation controls.

### C. Canonical execution schema / per-boot recovery epoch

Updated the existing `dread-orchestrator::persistence` implementation and added migration:

`crates/dread-orchestrator/migrations/0005_startup_recovery_generation.sql`

- Migration v5 adds `recovery_generation` to the canonical durable attempt table and initializes `startup_recovery_generation`.
- The installer now supports contiguous versions 1–5 and still validates schema history.
- `begin_startup_recovery` increments a generation on each API boot and explicitly closes the startup dispatch barrier.
- Recovery candidates include acknowledged/running/reconciliation-required attempts with an existing provider execution ID that are not attached for the current generation.
- Worker attachment records the current generation and is idempotent for the current generation.
- Barrier release requires all relevant nonterminal dispatch and cancellation attempts to be attached for the current generation.
- A timestamp from a previous process can no longer satisfy the next process's startup reattachment barrier.

**Important:** the API startup path intentionally does not open the barrier yet. It advances the recovery generation and remains closed. Actual provider worker enumeration/reattachment has not been integrated into the API startup sequence.

### D. API queue/preflight boundary

- Signed provider trust is checked at the task queue/dispatch approval boundary, not while merely planning or instantiating a mission. This preserves the distinction between planning, capability registration and execution authorization.
- The legacy agent queue still calls the durable-dispatch readiness guard.
- The startup barrier remains closed, so no provider may slip into the legacy direct-start path.
- Built-in tasks retain their existing execution paths and shared `ExecutionGate` ownership.

## 4. Verification status — exact distinction matters

### Previously green, but before the latest granular-isolation changes

CI run **44973104140**, run #1420, on commit:

`ef631bd94786797b1b750c7f244b1b612dd5b495`

All three jobs passed:
1. Rust / Ubuntu 24.04 — format, strict Clippy, tests, release build, installer/release verification, signed-verifier failure-path checks, portable migration/rollback tests, Python bridge/importer regression, single-owner GPU boundary.
2. Web console — lockfile verification and production build.
3. Ubuntu package/smoke test — deb and portable bundle assembly, checksums/content validation, packaged API/console smoke test.

This successful run predates the subsequent granular isolation evidence structure and must not be represented as verification of the checkpoint HEAD.

### Most recent run before the final formatting correction

CI run **44973316104**, run #1426, on commit `9520900f44ef9f97c85841035f7913e80495f237`:
- Web console passed.
- Rust job stopped at `cargo fmt --all -- --check`.
- The reported diffs were formatting-only in `execution_authority.rs`.
- The exact reported formatting was corrected in the next commit.

### Checkpoint HEAD verification

Checkpoint HEAD is now `756a3e712d2c74cba7c417a7eefdaee6064b83c9`.

**Fresh CI for this exact HEAD was not yet visible when this checkpoint was written.** Do not mark this HEAD green until a workflow run for this exact SHA completes. First action tomorrow: check the PR head and fetch workflow runs. Then resolve any format, compile, Clippy or test defects from logs and rerun the exact head.

## 5. Work still required — do not declare completion

1. **Trusted evidence issuer / repository.** Connect signed isolation evidence to real, durable adapter/runtime observations and trust-key lifecycle. Current `authenticated` is a field inside a signed manifest; it must be produced only by the trusted issuer and never from API input or environment booleans.
2. **Production admission policy.** Replace the current task-approval-derived policy decision placeholder with the actual trusted policy decision source bound to authenticated operator identity, requester, task, mission, capability, operation, scope, decision ID and expiry. Capability approval text alone is not cryptographic authorization.
3. **Durable API dispatch integration.** Replace the legacy provider worker start path with:
   - trusted server-generated `DispatchRequest`;
   - `authorize_and_persist`;
   - atomic `prepare_dispatch`;
   - provider start only for `DispatchPreparation::Created`;
   - never start again for `Existing` replay;
   - durable acknowledgement before polling/monitoring.
4. **Adapter transport/evidence contract.** Extend the AgentAdapter boundary to produce authenticated provider acknowledgement, active status, event, terminal and cancellation evidence bound to request ID, attempt ID, idempotency key, provider execution ID, adapter revision and signed provider registration. Existing bearer-authenticated polling alone is not an independent signature or proof of every event/result.
5. **Independent result verifier.** Implement a separately trusted verifier with result digest/identity binding and distinct worker/verifier identities. Until present, provider success must not become a verified terminal success and mission dependencies must not advance.
6. **Durable event/result/cancellation lifecycle.** Use the persistence methods for events, terminal provider result, independent verification, cancellation request/progress and canonical task/mission/result updates in the correct transaction order.
7. **Startup reconciliation and reattachment.** Query each provider by the exact durable idempotency/request identity, safely reattach existing workers, persist active/terminal observations, handle unknown/not-found without redispatch, and only then conditionally release the current-boot barrier.
8. **Crash/replay/security tests.** Include crash windows before/after provider acceptance, lost acknowledgement, duplicate start replay, provider-ID conflict, event-ID payload tamper, revocation/expiry between admission and attempt, restart generation, worker reattachment, cancellation races and independently verified result replay.
9. **Full CI on exact final head**, including packaging smoke tests. Keep PR #16 draft until all of this is genuinely integrated and validated.

## 6. Architecture and workstation boundaries to preserve

- Existing Phase 6 SQLite tables `missions`, `mission_tasks`, `tasks`, `task_events`, `task_results` are canonical. Do not create parallel mission/task databases.
- Existing `ExecutionGate` and Python GPU Supervisor remain the only scheduling/ownership authority. Do not add a second GPU scheduler.
- Preserve the existing Axum API, legacy Hub, ComfyUI, GPU gateway, llama.cpp service, models, workflows, databases, sessions, outputs and backups until replacement parity is demonstrated and explicitly authorized.
- No workstation deployment or service action is part of this checkpoint. All changes so far are GitHub source changes only.
- Existing service context remains: Hub 8080, ComfyUI 8188, GPU gateway 11500, llama.cpp target 11501, Hermes observed 48547, OpenHands 8000/18000, OpenCode 4096, embeddings 8128. These are reference facts, not an instruction to probe or restart them.
- No destructive operations; no overwrite without preserving the old source/version; do not remove historical handoffs or Kimi deliveries.

## 7. First actions on resumption

1. Fetch PR #16 and confirm the branch head is still `756a3e712d2c74cba7c417a7eefdaee6064b83c9`; if newer, use the newer exact head and update this record.
2. Fetch CI for that exact head. If it is still pending/not started, trigger only through a new meaningful code/test commit, not a blind no-op.
3. Finish verification of the granular `IsolationEvidenceRecord` change; capture compiler/Clippy/test failures from the job logs and correct them.
4. Continue source implementation in the current branch. Prioritize the canonical API dispatch bridge and adapter evidence contracts; keep the startup barrier closed until worker reattachment and independent verification are complete.
5. Update this checkpoint with each accepted milestone and exact commit/CI run. Do not ask the user to resend the PC/API/Kimi/GitHub source context.

**Resume instruction:** Continue PROJECT DREAD UNIVERSAL from this checkpoint and the exact active GitHub branch. Preserve every earlier source delivery and correction. Do not restart from zero, do not merge PR #16 prematurely, and do not claim completion while the listed execution, verification and recovery work remains.


## 8. Resumption update — 2026-11-22

This section supersedes the earlier pending-verification note in Section 4 while preserving its historical CI chronology.

### Current verified baseline

- **Source commit:** `5f1999cfaeb5f1be35a17796ea207d745464be0e`
- **Workflow:** DREAD Universal CI, run **#1434**, ID **44997828065**
- **Result:** all three jobs passed on the exact source commit.
  - Rust / Ubuntu 24.04: `cargo fmt --all -- --check`, strict Clippy, workspace tests, release build, installer/release verification, migration/rollback checks, Python bridge/importer tests and the single-owner GPU boundary.
  - Web console / Ubuntu 24.04: locked dependency verification and production build.
  - Ubuntu packages / smoke test: deb and portable bundle assembly, checksums/content validation, packaged API and production console smoke test.
- **CI corrections applied after the prior checkpoint:**
  1. Updated the isolation regression test to mutate `IsolationControl::EgressPolicy` in the granular evidence map, then re-sign the changed payload with the fixture's trusted Ed25519 key. This now exercises rejection of a trusted-but-unsafe evidence record rather than merely testing a signature mismatch.
  2. Removed the unused `Phase6ExecutionAuthority::new` constructor. The production authority remains created only through the environment loader after the configured manifest and server public key are verified.
- **Web console and packaging stayed green** during both correction cycles. No workstation service, file, model, database, or workflow was touched.

### Active implementation boundary

The API still does **not** start providers through the new durable lifecycle. The current queue path remains protected by the closed startup-dispatch barrier. The durable persistence layer exposes admission, atomic attempt preparation, provider acknowledgement/active/event/terminal persistence, independent-verification recording, cancellation, and per-boot recovery primitives; however:

- the API's current approval-derived `policy_decision_id` remains a placeholder, not a production policy decision;
- `AgentAdapter` still exposes legacy task/status/event structures rather than attempt-bound authenticated operation receipts;
- actual startup provider-worker reconciliation/reattachment is not wired;
- independent result verification remains fail-closed.

Next implementation must establish trusted policy-decision provenance and a request/attempt/idempotency-key-bound adapter evidence contract before replacing the guarded legacy provider worker. Keep the startup dispatch barrier closed until durable provider reattachment and independent verification satisfy the release invariants. PR #16 remains draft and unmerged.

## 9. Autonomous continuation update — 2026-11-22

### Source changes committed on the active PR #16 branch

- Commit `6305e3ee5273a4b6a28bb200a0c4aa65848b0166` adds a focused regression test for the lost-provider-acknowledgement crash window. It creates a durable `dispatching` attempt, advances the startup recovery generation, confirms the attempt remains in `unresolved_attempts`, confirms it is not yet eligible for worker attachment because no provider execution ID is known, and asserts that the startup dispatch barrier cannot reopen.
- Commit `6f82f79dc07fb72b37f9fb421c1ab1c5626c21d3` adds `provider_evidence_from_adapter_receipt`, a conversion boundary from the existing signed `AgentOperationReceipt<T>` into durable `ProviderEvidence`. It binds provider/adapter/version/registration revision, preserves the exact signed payload JSON/key/signature/response digest, rejects event receipts from the lifecycle-evidence path, and includes a focused mapping/scope regression test.
- Commit `07aff333148106da2e18838aa26550be58e7d6f0` records the helper as an intentionally staged integration boundary until the API worker is wired to consume it. This does not activate provider dispatch.

### Verification status

- The two changed source files were read back from GitHub after their commits; both new tests and the receipt-conversion helper are present.
- No GitHub Actions run was returned for the new commits at the time of this update. Do not mark these commits compiled, Clippy-clean, tested, packaged or green until a run for the exact final SHA completes.
- Latest code change in this continuation: `07aff333148106da2e18838aa26550be58e7d6f0`.

### Remaining implementation boundary

The API does not yet call `authorize_and_persist`, `prepare_dispatch`, or the receipt conversion helper from the production provider worker. The existing direct-start worker remains behind the closed startup dispatch barrier. The new test strengthens the lost-ack crash invariant; it does not complete startup reconciliation. Keep the barrier closed and PR #16 draft. Continue with production policy binding, durable API admission/attempt preparation, receipt-backed provider lifecycle persistence, startup reattachment, and an independently trusted result verifier.
