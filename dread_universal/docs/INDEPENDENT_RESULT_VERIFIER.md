# PROJECT DREAD Universal — Independent Result Verifier Contract

**Status:** implementation contract in Phase 6. The API accepts no provider-success result as a verified DREAD success unless an independent verifier has a fresh signed heartbeat and produces the exact per-request receipt described below.

The verifier is a separate process and must run as a distinct Unix service identity from the API. Its private Ed25519 key is not available to the API account, and the API account has read-only group access to the receipt spool. Re-signing or forwarding a provider receipt is not independent verification. A process launched under the same UID as the API is not an acceptable production trust boundary.

## Configuration

Configure all six API-side values together. Partial configuration is a startup error; no configuration remains fail-closed.

- `DREAD_INDEPENDENT_RESULT_VERIFIER_RECEIPT_DIR`: shared receipt directory owned by the dedicated verifier account, mode `2750`, setgid enabled; the configured shared group has read/execute but no write access.
- `DREAD_INDEPENDENT_RESULT_VERIFIER_ID`: stable verifier identity, distinct from requester, API worker, and provider adapter receipt identity.
- `DREAD_INDEPENDENT_RESULT_VERIFIER_KEY_ID`: stable Ed25519 public-key identifier.
- `DREAD_INDEPENDENT_RESULT_VERIFIER_PUBLIC_KEY_HEX`: 32-byte Ed25519 public key, lower- or uppercase hexadecimal.

The API must additionally set:
- `DREAD_INDEPENDENT_RESULT_VERIFIER_RECEIPT_UID`: numeric Unix UID owning the receipt directory and output files.
- `DREAD_INDEPENDENT_RESULT_VERIFIER_RECEIPT_GID`: numeric shared Unix GID used by the receipt directory and output files.

The verifier process uses the same receipt UID/GID settings. The directory must be owned by that UID/GID and have exact mode `2750`; the setgid bit causes newly created outputs to inherit the shared GID. Heartbeat and result files must be regular, non-symlink files owned by the verifier UID/GID with exact mode `0640` (verifier read/write, API group read-only, no other access). The API checks owner, group and mode on every receipt read. The API process must actually belong to the configured shared group; lack of filesystem read access causes verification to remain unavailable. The verifier owns the directory; the API cannot replace or write receipts.

## Signed envelope

Each file is a JSON object with exactly these fields:

```json
{
  "key_id": "result-verifier-key-1",
  "payload": {},
  "signature_hex": "<128 lowercase hex characters>"
}
```

The signature is Ed25519 over the UTF-8 bytes of compact JSON serialization of the `payload` object only. The field order below is the Rust struct serialization order and is part of this contract. Unknown fields are rejected.

## Readiness heartbeat payload

Write `.dread-verifier-ready.json` with this payload shape:

```json
{
  "schema_version": 1,
  "verifier_id": "dread-independent-result-verifier",
  "observed_at": 1800000000,
  "expires_at": 1800000060
}
```

The heartbeat is accepted only when its signature/key ID is valid, the verifier ID matches configuration, the observation is not future-dated, the expiry is in the future, and the lifetime is at most 120 seconds. Refresh it periodically using an atomic temporary-file-and-rename write. A missing, invalid, or stale heartbeat means verifier status is `configured_unavailable` and startup dispatch remains blocked.

## Per-request verification payload

The verifier process should monitor the canonical Phase 6 SQLite database using a read-only connection and select only attempts in `dispatch_state = 'provider_succeeded'`. It needs the attempt's `request_id`, `attempt_id`, `task_id`, `worker_id`, exact `provider_result_json` and `provider_error` bytes, the matching admission's `requester_id`, and the verifier ID from the attempt's authenticated `succeeded` provider evidence. The canonical tables are `dread_task_attempts`, `dread_dispatch_admissions`, and `dread_execution_evidence`; do not create a parallel task or result database. The verifier must apply its own result policy for the task/capability before signing a receipt.

For each durable request, write `<request_id>.json` with this payload shape:

```json
{
  "schema_version": 1,
  "evidence_id": "independent-result-<unique-id>",
  "verifier_id": "dread-independent-result-verifier",
  "request_id": "the-durable-request-id",
  "attempt_id": "the-durable-attempt-id",
  "task_id": "the-canonical-task-id",
  "requester_id": "the-canonical-approved-requester",
  "worker_id": "the-durable-execution-worker",
  "provider_verifier_id": "dread-adapter-receipt:hermes",
  "result_sha256": "<sha256 of the exact stored result_json bytes, or null>",
  "error_sha256": "<sha256 of the exact stored provider_error bytes, or null>",
  "passed": true,
  "source_reference": "verifier://dread-independent-result-verifier/<record>",
  "observed_at": 1800000000,
  "expires_at": 1800000300
}
```

The API checks every identity against canonical SQLite admission/attempt state and the provider-success receipt. It recomputes SHA-256 over the exact UTF-8 bytes of the stored `result_json` and `provider_error` strings; it does not parse and reserialize them. The verifier must therefore hash the exact bytes it was given. A missing value is represented by JSON `null`, not the hash of an empty string.

The result receipt is accepted only with a valid signature, matching key ID/verifier ID, exact request/attempt/task/requester/worker/provider-verifier identities, exact result/error digests, a nonempty evidence ID/source reference, an observation no later than API time, and an expiry within 300 seconds. The verifier identity must be distinct from the requester, execution worker and provider receipt verifier. `passed=false` is a valid negative verification decision; it finalizes the task as failed rather than completed.

Use a unique evidence ID and a signed receipt file for each pending request. The producer writes a group-readable temporary file (mode `0640`) inside the verifier-owned setgid directory, syncs it, and atomically publishes it. It refreshes receipts only while the canonical attempt remains in `provider_succeeded`; finalized attempts are no longer queried. Retain receipts according to the deployment's audit-retention policy.

## Release behavior

- The startup dispatch barrier requires a fresh signed heartbeat in addition to signed execution policy, enabled provider registration, granular isolation evidence, and pinned adapter receipt keys.
- A provider's signed `succeeded` observation first enters `provider_succeeded`; it does not complete the canonical task or advance mission dependencies.
- The API polls pending `provider_succeeded` attempts in-process every five seconds; a late receipt can finalize a task without restarting the API.
- Fresh provider admissions and attempt revalidation require a live signed verifier heartbeat, not merely configured environment variables.
- The API finalizes the task and mission only after the exact independent verification receipt is accepted and stored transactionally.
- Missing, stale, mismatched, replayed, or tampered receipts leave provider success pending and keep the startup barrier closed.
- This contract does not claim that a verifier process, its signing key, or a production verification policy has been deployed. That is a separate acceptance requirement.

## Standalone producer binary

The producer is shipped as `dread-result-verifier` alongside the API in the portable and Debian packages. It is a separate executable, not a thread inside the API.

Commands:

```bash
dread-result-verifier keygen /secure/verifier/private-key.pk8
dread-result-verifier public-key /secure/verifier/private-key.pk8
dread-result-verifier run --once
dread-result-verifier run
```

The key generator refuses to overwrite an existing file. The private PKCS#8 file is mode `0600`; it must be owned by the verifier account. Do not put the private key in the repository, API environment, or API service account's readable files.

The producer requires these environment variables:

- `DREAD_VERIFIER_DB_PATH`: path to the existing canonical Phase 6 SQLite database. It opens this database read-only, enables SQLite query-only mode, verifies schema version 7 or newer, and queries only `provider_succeeded` attempts.
- `DREAD_INDEPENDENT_RESULT_VERIFIER_RECEIPT_DIR`: verifier-owned receipt directory described above.
- `DREAD_INDEPENDENT_RESULT_VERIFIER_ID`: verifier identity.
- `DREAD_INDEPENDENT_RESULT_VERIFIER_KEY_ID`: receipt signing key identifier.
- `DREAD_INDEPENDENT_RESULT_VERIFIER_PRIVATE_KEY_FILE`: verifier-owned Ed25519 PKCS#8 private key, mode `0600`.
- `DREAD_INDEPENDENT_RESULT_VERIFIER_PUBLIC_KEY_HEX`: public key matching that private key.
- `DREAD_INDEPENDENT_RESULT_VERIFIER_POLICY_FILE`: verifier-owned policy JSON, mode `0600`.
- `DREAD_INDEPENDENT_RESULT_VERIFIER_RECEIPT_UID` and `DREAD_INDEPENDENT_RESULT_VERIFIER_RECEIPT_GID`: numeric owner UID and shared group GID used for the receipt directory/output files.
- `DREAD_VERIFIER_POLL_INTERVAL_SECONDS`: optional polling interval from 1 through 60 seconds (default 5).

The API service separately configures the receipt directory, verifier ID, key ID, matching public key, receipt UID and receipt GID. The API never receives the private key or policy file.

### Policy template and decision scope

The packaged example is `config/independent-result-verifier.policy.example.json`. Copy it into the verifier account's protected configuration directory, change ownership to the verifier account, set mode `0600`, and replace every `/__REPLACE_WITH_REQUIRED_VERIFIED_RESULT_FIELD__` pointer with a required non-null JSON Pointer from the real output schema for that capability. Configure an explicit rule for each capability. Unknown capabilities, provider errors, missing/invalid/non-object results, generic failure fields, oversized results, missing required fields, or forbidden fields produce a signed negative decision.

The current producer performs deterministic structural JSON checks (size, object shape, failure markers, required/forbidden JSON Pointers) and hashes the exact stored result/error bytes. It does **not** execute the agent's code, validate external side effects, prove that a requested operation happened, or semantically judge arbitrary prose. Capability owners must define result schemas and domain-specific evidence requirements before enabling a rule. A structurally valid JSON object is not by itself proof of real-world completion.

### Separate-account deployment requirements

The API must not share the verifier UID. Create a dedicated verifier user and a read-only shared group. The verifier owns the receipt directory and files; the API user is a member of the shared group with no write permission. Configure the receipt directory as owner UID/shared GID, mode `2750`. The producer writes mode `0640` output files, and the API validates exact owner UID, group GID and mode before reading them.

The verifier needs read-only access to the canonical SQLite database **and its required WAL/SHM files and parent directories**. Do not grant it write access to the database, journals, task records, or result tables. Configure filesystem ACLs/group traversal narrowly and verify that the API account cannot modify receipts and the verifier account cannot modify SQLite. The producer itself uses SQLite read-only open flags and `PRAGMA query_only=ON`, but filesystem permissions must independently enforce that boundary.

The producer uses `.dread-verifier.lock` to prevent duplicate instances. If the process is killed and leaves a stale lock, first verify that no verifier process is running; only then remove that lock file. Do not routinely delete locks or receipts.

No verifier process, service account, signing key, receipt directory, policy, heartbeat, or database ACL is deployed by this source change. These steps are an operator deployment and acceptance gate. Keep provider dispatch blocked until the separate account, database read-only access, API group-only receipt access, live heartbeat, and capability policy are tested on the target workstation.
