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

## 10. Durable API execution + independent verification milestone — 2026-11-22

This addendum supersedes earlier statements that durable API dispatch, startup reattachment, and independent result verification had not yet been integrated. It does **not** supersede the remaining physical workstation and trusted-runtime acceptance gates below.

### Exact source and CI evidence

- **Active PR:** #16, still open and draft.
- **Branch:** `work/kimi003-phase6-api-integration-2026-11-22`
- **Verified source HEAD:** `a6c48814056d7bd1b9afa892f9dcbe385547a842`
- **GitHub Actions:** DREAD Universal CI run **#1684**, ID **45033888462**.
- **Result:** all three jobs passed on that exact SHA:
  1. Rust / Ubuntu 24.04 — formatting, strict Clippy, workspace tests, release build, installer/release verification, signed-release verifier failure paths, portable migration/rollback, Python bridge/importer regression, and the single-owner GPU boundary.
  2. Web console / Ubuntu 24.04 — locked dependency install and production build.
  3. Ubuntu packages / smoke test — Debian and portable package creation, checksums/content verification, and packaged API/console smoke tests.

Run URL: https://github.com/dreaddo151-sys/koko/actions/runs/45033888462  
Source commit: https://github.com/dreaddo151-sys/koko/commit/a6c48814056d7bd1b9afa892f9dcbe385547a842

The Rust suite reported 87 API tests passing, with the orchestrator, agents, discovery, GPU, media and other workspace crate tests also green. The OpenHands test suite includes authenticated lifecycle tests; the new verifier tests cover heartbeat signature/freshness and exact per-attempt result digest binding. These are CI/mock contract tests, not live provider execution.

### Implemented in this continuation

1. **Durable API dispatch path** — `dread-api/src/durable_agent.rs` is wired into the existing Axum worker and shared `ExecutionGate`. Admission and atomic attempt preparation are persisted before the remote side effect. Only `DispatchPreparation::Created` can call `start_task_with_evidence`; `Existing` never starts a second request. The provider acknowledgement is verified and persisted before polling begins.

2. **Authenticated lifecycle** — adapter-receipt conversion validates response digests against the actual returned object. Provider events carry signed receipt context into canonical durable event persistence. Active status, provider terminal outcomes, and cancellation progression use the attempt/request/adapter/registration identity. A provider success remains `provider_succeeded` until independent verification accepts the exact stored result/error.

3. **Ambiguous start and crash recovery** — `AgentAdapter::reconcile_start_with_evidence` defaults to a fail-closed unsupported result; OpenHands implements read-only GET reconciliation through its deterministic conversation ID. Hermes and DeerFlow do not blindly replay ambiguous starts. Startup enumerates unresolved attempts, queries known IDs or the provider's read-only reconciliation hook, persists status/result observations, and attaches known-active workers inside the shared `ExecutionGate`. Unknown outcomes without IDs remain nonterminal; no start is replayed. Legacy cancellation recovery skips tasks with durable attempts.

4. **Startup barrier and late-verifier worker** — the release predicate requires a valid signed policy, a fresh signed independent-verifier heartbeat, at least one enabled provider, and valid non-revoked isolation evidence plus a manifest-pinned adapter receipt key for each enabled provider. The independent-verification background worker retries only durable `provider_succeeded` attempts every five seconds; if a receipt arrives late and is accepted transactionally, it can then reevaluate the gate and resume queued durable tasks. This worker never sends provider starts itself.

5. **Independent result verifier contract** — `dread-api/src/independent_verifier.rs` verifies a separately keyed Ed25519 heartbeat and per-request signed receipt from an owner-only, non-symlink receipt directory. The receipt binds canonical request, attempt, task, requester, worker and provider-receipt-verifier identities, plus SHA-256 of the exact persisted result JSON and provider-error bytes. A negative verifier decision is stored as a failed task; missing, stale, mismatched or tampered receipts leave provider success pending. Both new admission and attempt revalidation require a live heartbeat, while active provider lifecycle reconciliation may continue when the verifier is temporarily unavailable.

6. **Schema migration 0007** — `0007_extended_cancellation_evidence.sql` expands the append-only evidence table's allowed signed cancellation receipt kinds while copying all existing audit/receipt rows and restoring append-only triggers. Schema installer and version regression tests now support schema version 7.

7. **Adapter identity pinning** — Hermes, DeerFlow and OpenHands expose `CARGO_PKG_VERSION` as the adapter version. OpenHands' descriptor adapter key now matches the canonical provider ID `openhands` while retaining its descriptive descriptor ID.

### Operational status and hard boundaries

- **The startup dispatch barrier is still expected to remain closed on the user's actual machine** until the signed execution trust manifest, signed policy, provider receipt public keys, and independent verifier process/key/heartbeat are installed and verified there. The CI uses fixtures and does not deploy or configure these services.
- No independent verifier process was deployed or launched as part of this GitHub source work. The new receipt directory protocol is documented in `docs/INDEPENDENT_RESULT_VERIFIER.md`; the operator must run a separate verifier that reads the canonical Phase 6 DB read-only, applies an independent result policy, and writes signed receipts/heartbeats.
- Granular isolation remains signed provisioner/trust-authority evidence. The production issuer is not yet bound to live runtime observations of mounts, egress, tool permissions, secret handling, cancellation and restart reconciliation. Do not describe those flags as independently verified physical isolation.
- The CI is repository code/package proof only. No provider task, live Hermes/DeerFlow/OpenHands session, actual ComfyUI/media generation, dual-3090 contention, physical installation, reboot, or workstation Hub parity test was performed.
- No workstation service, file, database, model, workflow, output, session, backup, ComfyUI instance, legacy DREAD Hub, or Python GPU Supervisor was touched. Existing `ExecutionGate` and the Python GPU Supervisor ownership boundary remain unchanged.
- PR #16 remains draft and unmerged. Do not open the dispatch barrier or mark production activation accepted until the independent verifier and live runtime evidence issuer are deployed/configured and physical acceptance checks pass.

### Next engineering steps

- Add an executable independent-verifier producer/tool or reference implementation that reads canonical SQLite read-only, applies a defined result policy, and atomically writes heartbeat and result receipt files without sharing API/provider secrets.
- Implement the trusted runtime isolation evidence issuer and key lifecycle; replace operator-asserted isolation booleans/evidence records with durable observations from the actual provider/container/runtime.
- Add direct API integration tests for durable start/replay/ack-lost/reattach/cancellation/terminal-verification and provider mock servers, including races between verifier expiry, provider completion and cancellation.
- Verify real adapter API behavior and run physical Ubuntu acceptance without stopping or replacing ComfyUI, the legacy Hub, or the single Python GPU Supervisor.
- Re-run full CI on every resulting code head and keep the startup barrier fail-closed until all required controls are actually deployed and validated.

## 11. Standalone independent verifier producer and separate-user packaging — 2026-11-22

### Exact verified source and CI

- **PR:** #16 — open, draft, unmerged.
- **Branch:** `work/kimi003-phase6-api-integration-2026-11-22`
- **Exact verified HEAD:** `e390cc688bbf59108d1f8f58bb0037d20ab72f05`
- **GitHub Actions run:** #1757, ID `45038700204`.
- **Result:** all three jobs passed on that exact SHA:
  1. Rust / Ubuntu 24.04 — fmt, strict Clippy, workspace tests, release build, installer/release checks, portable migration/rollback tests, Python bridge/importer tests, single-owner GPU boundary.
  2. Web console / Ubuntu 24.04 — locked install and production build.
  3. Ubuntu packages / smoke test — archive and Debian creation, checksums/content verification, packaged API and production-console smoke test.

Run: https://github.com/dreaddo151-sys/koko/actions/runs/45038700204  
Commit: https://github.com/dreaddo151-sys/koko/commit/e390cc688bbf59108d1f8f58bb0037d20ab72f05

### New executable producer

Workspace crate `crates/dread-result-verifier` builds the standalone `dread-result-verifier` binary. It is deliberately a separate process rather than an API thread.

- Opens the canonical Phase 6 SQLite database using SQLite read-only open flags, sets `PRAGMA query_only=ON` and `trusted_schema=OFF`, checks schema version 7+, and validates the canonical attempt/admission/evidence tables.
- Selects only `provider_succeeded` attempts and reads requester, worker, provider attestor and exact result/error bytes from the canonical records.
- Evaluates explicit per-capability rules: max result bytes, required JSON Pointers, forbidden JSON Pointers, object shape and generic failure markers. Unknown capability, provider error, missing/invalid/non-object result, failure marker, missing required field, forbidden field or oversize result becomes a signed negative decision.
- Hashes exact persisted result/error bytes; signs the API-compatible heartbeat/result payloads with Ed25519 using a verifier-owned PKCS#8 key. Key generation is create-once and refuses overwrites.
- Publishes heartbeat and per-request receipts atomically; verifies safe filenames, symlink/file types, size, owner/group/mode, and uses a process lock to avoid concurrent producers.
- Uses a distinct verifier identity and checks it differs from requester, worker and provider receipt verifier.
- Added a file-backed test which opens a canonical schema-v7 SQLite file via the real read-only open path, confirms `PRAGMA query_only=1`, and proves both row writes and DDL are rejected.

**Verification from run #1757:** the full workspace test suite passed, including 11 standalone producer tests, 87 API tests, 34 agent tests, 36 orchestrator tests and all other workspace crate tests. Strict Clippy and release build passed.

### Separate Unix identity and receipt spool

The original owner-only receipt-dir design was corrected because it would have prevented a separately privileged verifier account from publishing files that the API can read. The verified contract is:

- Dedicated verifier UID owns the receipt directory/files.
- Shared API receipt group is separate from the verifier's primary group.
- Receipt directory exact mode `2750` (owner rwx, group r-x, setgid, no other access).
- Heartbeat/result files exact mode `0640` (verifier owner read/write, API group read-only, no other access).
- API checks configured receipt UID/GID plus exact ownership/mode on every read; API is not allowed to replace or create receipts.
- Verifier private key and capability policy are verifier-owned, mode `0600`, and are never provided to the API account.
- New API environment values include numeric `DREAD_INDEPENDENT_RESULT_VERIFIER_RECEIPT_UID` and `..._GID` in addition to receipt path, verifier ID, key ID and pinned public key.

### Deployment examples now shipped, not activated

The portable and Debian package builders now include:

- `bin/dread-result-verifier`
- `config/independent-result-verifier.policy.example.json`
- `config/independent-result-verifier.env.example`
- `systemd/dread-result-verifier.service.example`
- `systemd/dread-result-verifier.tmpfiles.example`

The portable installer preflights and preserves these examples during upgrades. CI asserts that the archive and Debian package contain them. The installer does **not** create verifier users/groups, create the receipt directory, copy the system unit into `/etc/systemd/system`, enable/start the verifier, or alter API group membership. The verifier service remains a manual operator action.

### Important acceptance boundary

The producer's current policy performs deterministic **structural JSON checks** and exact digest binding. It does not execute agent code, validate external side effects, establish that a real-world operation occurred, or semantically judge arbitrary prose. Owners must replace every placeholder JSON Pointer in the example policy with a real per-capability result schema and add domain-specific evidence requirements before enabling any rule.

No verifier process, Unix service account, signing key, receipt directory, capability policy, heartbeat, API shared-group membership, SQLite ACL, or service unit has been deployed on the workstation. The service/env/tmpfiles files are examples only. The actual DREAD DB path must be discovered and mapped read-only, including its required WAL/SHM and parent-directory permissions. PR #16 remains draft/unmerged and provider dispatch remains blocked until this is installed and tested with two distinct Unix identities on the target system.

The existing API integration, durable startup recovery, Python GPU Supervisor ownership boundary, ComfyUI, legacy DREAD Hub, prior Kimi deliveries, PRs #12–#15, canonical SQLite data and backups remain preserved. No workstation service was touched.

## 12. Runtime-attestation adapter binding — 2026-11-22

**Verified code head:** `0577ca2a247c16799db5a49cd1c2817efff18a6e`  
**CI run:** [#1849 — all jobs passed](https://github.com/dreaddo151-sys/koko/actions/runs/45041780145)

The runtime-attestation verifier now uses typed trust and provider-binding records rather than a positional list of eight arguments. The independent runtime observer signature is cross-bound to the signed provider registration: provider identity, registration revision, adapter identity/version, exact effective endpoint, runtime identity, source observation, evidence bundle digest, observation/expiry times, and all eight required unique control observations must match. The API authority retains this fail-closed cross-check at admission.

### Canonical adapter identities and endpoints

A runtime attestation must identify the adapter the API actually constructs, not merely the friendly provider name:

- Hermes adapter identity is `hermes.api_server`; the attested endpoint follows the adapter's URL path and trailing-slash normalization.
- DeerFlow adapter identity is `deerflow.gateway`; the attested endpoint includes the selected effective route, `/api` or `/api/langgraph`, appended to the normalized origin.
- OpenHands adapter identity is `openhands`; the attested endpoint is the normalized origin.

The production environment loader now derives these same identities/endpoints before verifying each signed report. It rejects a DeerFlow API prefix outside the two supported values and rejects an origin helper input that contains a nested path. This closes a genuine integration mismatch where the configuration loader was previously verifying friendly provider IDs/raw origins that would not match the live adapter descriptors.

A new API regression test, `runtime_attestation_endpoint_normalizers_match_provider_descriptors`, covers Hermes trailing-slash/path normalization, both DeerFlow route forms, OpenHands origin normalization, and rejection of a path-bearing origin. The provider-registry test now builds each adapter using its exact descriptor identity and effective endpoint. The OpenHands confirmation test also uses the public signed-runtime-attestation path rather than a private boolean helper.

### Exact-head verification

Run #1849 was triggered for the exact code head above. All three GitHub Actions jobs completed successfully:

1. **Rust / Ubuntu 24.04:** formatting, strict workspace Clippy, workspace tests, optimized release build, installer and signed-release checks, portable migration/rollback, Python bridge/importer regressions, and single-owner GPU boundary.
2. **Web console / Ubuntu 24.04:** locked dependency installation and production console build.
3. **Ubuntu packages / smoke test:** portable and Debian package assembly, checksums/content validation, packaged API installation and production-console smoke test.

The workspace test output included 37 agent tests, 89 API tests, 14 discovery tests, 15 GPU tests, 6 media tests, 36 orchestrator tests, 31 platform tests, 11 standalone verifier tests, and 7 remaining workspace tests; all reported zero failures. The dedicated endpoint-normalization regression and signed-provider-registry test both passed.

**Run:** https://github.com/dreaddo151-sys/koko/actions/runs/45041780145  
**Commit:** https://github.com/dreaddo151-sys/koko/commit/0577ca2a247c16799db5a49cd1c2817efff18a6e

### Remaining acceptance gates — unchanged

Passing CI proves source/build/package correctness only. It does not prove that a trusted runtime observer has deployed live evidence for mount, egress, tool permission, secret handling, cancellation or restart controls. The target workstation still needs a separately operated trusted attestor, matching live observations and pinned keys. The independent verifier also remains an undeployed manual separate-UID service with unconfigured real capability policies, receipt directory/ACL and canonical database/WAL/SHM access. Two-UID receipt access, live heartbeat, real provider operation, crash/race recovery, physical Ubuntu install/reboot, actual GPU/ComfyUI contention and legacy Hub parity are still not accepted.

Provider dispatch remains fail-closed. PR #16 remains draft and unmerged. No workstation service or user data was changed by these repository operations.

## 13. Durable pre-start failure outcome — 2026-11-22

**Verified code head:** `7eaa03f914b4a685da9c9d902f39a13e7731bccc`  
**CI run:** [#1865 — all jobs passed](https://github.com/dreaddo151-sys/koko/actions/runs/45042978730)

### Change

The durable worker previously logged and returned when admission/attempt preparation failed before an attempt had been persisted. The API had already acknowledged the task as queued, so that left a queued task with no running worker and no terminal task result.

The worker now distinguishes a local pre-start failure from a request whose provider side effect may already exist:

- It looks up the task's durable attempts in canonical SQLite and treats dispatching, acknowledged, running, provider_succeeded, reconciliation_required, and unresolved cancellation states as possible remote-side-effect blockers.
- If an unresolved attempt exists, it preserves the nonterminal state and records/retains reconciliation_required. It never writes a local terminal failure over an ambiguous provider outcome.
- If no unresolved attempt exists, the provider start endpoint has not been reached by this worker. The worker records a canonical failed task result and task_failed event with the durable preparation reason instead of silently stranding the task in queued.
- If the database cannot be inspected, task state is left unchanged. The worker does not assume that an unknown remote outcome is safe to finalize.

The startup barrier and provider start contract are unchanged: a new provider start still occurs only after current-boot reconciliation, signed policy/registration checks and atomic attempt preparation; ambiguous starts are never blindly replayed.

### Regression coverage

Added worker tests for:
- Terminal failed attempts do not block classification as a pre-start local failure.
- Dispatching and cancellation-unknown attempts are treated as unresolved blockers.
- A queued task with no durable attempt transitions to failed, receives a task_results error and publishes task_failed.
- The existing API barrier assertion now checks the current-boot reconciliation and signed activation reason instead of stale wording that claimed the durable worker integration was not yet present.

### Exact-head verification

Run #1865 completed successfully on the exact code head above. All three CI jobs passed:

1. **Rust / Ubuntu 24.04:** rustfmt, strict workspace Clippy, full workspace tests, optimized release build, installer and signed-release verification checks, portable install migration/rollback, Python bridge/importer tests, and the single-owner GPU boundary.
2. **Web console / Ubuntu 24.04:** locked dependencies and production build.
3. **Ubuntu packages / smoke test:** portable and Debian package assembly, checksums/content validation, packaged API install and production-console smoke test.

Workspace test output included 37 agent tests, 91 API tests, 14 discovery tests, 15 GPU tests, 6 media tests, 36 orchestrator tests, 31 platform tests, 11 standalone verifier tests, and 7 remaining workspace tests; all had zero failures. Both new durable-worker tests and the startup barrier test passed.

**Run:** https://github.com/dreaddo151-sys/koko/actions/runs/45042978730  
**Commit:** https://github.com/dreaddo151-sys/koko/commit/7eaa03f914b4a685da9c9d902f39a13e7731bccc

### Acceptance boundary

This is a source and CI milestone, not a live provider deployment. Independent result verification, live runtime-isolation attestation issuance, separate verifier UID/GID service deployment, capability-specific semantic/result evidence policies, real provider behavior, crash/race acceptance, physical workstation deployment and GPU/ComfyUI contention tests remain outstanding. Keep provider dispatch fail-closed until those gates are met. PR #16 remains draft and unmerged.

## 14. Direct no-provider-start proof for durable preparation failure — 2026-11-22

**Verified code head:** `0a8ece2cbe884ad458d30f7aca39e65105a2c31f`  
**CI run:** [#1870 — all jobs passed](https://github.com/dreaddo151-sys/koko/actions/runs/45043437675)

The previous durable pre-start failure handling was tested at the task-state helper boundary. This update adds a stronger worker-level regression that invokes `execute_durable_agent_task_inner` with a queued canonical agent task, a configured mock Hermes adapter, and no Phase 6 execution authority.

The test proves both required effects:
- The durable preparation path fails before a provider side effect; the adapter's recorded `start_task` calls remain empty.
- The accepted queued task is persisted as `failed` with a `task_results.error` identifying the durable preparation failure before start.

The complementary durable-worker tests remain:
- A task with no unresolved durable attempt transitions to failed and emits `task_failed`.
- Existing terminal failed attempts do not block pre-start local failure classification.
- Dispatching and cancellation-unknown attempts are treated as unresolved remote-side-effect blockers and retain reconciliation rather than being finalized locally.
- If the database cannot be inspected, no local terminal assumption is made.

### Exact-head verification

Run #1870 completed successfully on the exact source SHA above. All three jobs passed:

1. **Rust / Ubuntu 24.04:** formatting, strict workspace Clippy, workspace tests, optimized release build, installer and signed-release checks, portable migration/rollback, Python bridge/importer regressions, and single-owner GPU boundary.
2. **Web console / Ubuntu 24.04:** locked dependency install and production build.
3. **Ubuntu packages / smoke test:** portable and Debian package assembly, checksums/content verification, packaged API installation and production-console smoke test.

The test output includes the direct `durable_worker_does_not_start_provider_when_preparation_fails` regression, the task-state and unresolved-attempt tests, and 92 API tests with zero failures. Agent tests: 37; discovery: 14; GPU: 15; media: 6; orchestrator: 36; platform: 31; standalone verifier: 11; skills: 7. All reported zero failures.

**Run:** https://github.com/dreaddo151-sys/koko/actions/runs/45043437675  
**Commit:** https://github.com/dreaddo151-sys/koko/commit/0a8ece2cbe884ad458d30f7aca39e65105a2c31f

### Unchanged deployment and safety boundary

This is repository-level source and CI verification only. No provider start was sent during these tests; no real provider, independent verifier service, runtime attestor, API shared-group/receipt ACL or canonical database/WAL/SHM permissions were deployed on the workstation. Live provider crash/race handling, separate-UID receipt read/write denial, real capability semantic evidence, physical Ubuntu install/reboot, and actual dual-GPU/ComfyUI contention acceptance remain outstanding. Dispatch remains fail-closed until those gates pass. PR #16 remains draft and unmerged.
