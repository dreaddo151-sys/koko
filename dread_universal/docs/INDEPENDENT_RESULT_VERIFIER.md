# PROJECT DREAD Universal — Independent Result Verifier Contract

**Status:** implementation contract in Phase 6. The API accepts no provider-success result as a verified DREAD success unless an independent verifier has a fresh signed heartbeat and produces the exact per-request receipt described below.

The verifier is a separate process and trust domain from the API worker, the provider adapter, and the provider's own receipt signer. It must inspect the result using its own validation policy. Re-signing or forwarding a provider receipt is not independent verification.

## Configuration

Configure all four values together. Partial configuration is a startup error; no configuration remains fail-closed.

- `DREAD_INDEPENDENT_RESULT_VERIFIER_RECEIPT_DIR`: existing directory owned by the DREAD service account, mode `0700`.
- `DREAD_INDEPENDENT_RESULT_VERIFIER_ID`: stable verifier identity, distinct from requester, API worker, and provider adapter receipt identity.
- `DREAD_INDEPENDENT_RESULT_VERIFIER_KEY_ID`: stable Ed25519 public-key identifier.
- `DREAD_INDEPENDENT_RESULT_VERIFIER_PUBLIC_KEY_HEX`: 32-byte Ed25519 public key, lower- or uppercase hexadecimal.

The directory contains a signed heartbeat named `.dread-verifier-ready.json` and one signed result receipt named `<request_id>.json`. Files must be regular, non-symlink files, no larger than 64 KiB, with owner-only permissions (`0600` or stricter). The API reads the files; it does not execute verifier code or write verifier results.

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

Use a unique evidence ID and immutable, create-once receipt file for each request. Write to a temporary owner-only file, fsync as required by the verifier's durability policy, then atomically rename into place. Do not replace a receipt for an already-finalized request. Retain receipts according to the deployment's audit-retention policy.

## Release behavior

- The startup dispatch barrier requires a fresh signed heartbeat in addition to signed execution policy, enabled provider registration, granular isolation evidence, and pinned adapter receipt keys.
- A provider's signed `succeeded` observation first enters `provider_succeeded`; it does not complete the canonical task or advance mission dependencies.
- The API polls pending `provider_succeeded` attempts in-process every five seconds; a late receipt can finalize a task without restarting the API.
- Fresh provider admissions and attempt revalidation require a live signed verifier heartbeat, not merely configured environment variables.
- The API finalizes the task and mission only after the exact independent verification receipt is accepted and stored transactionally.
- Missing, stale, mismatched, replayed, or tampered receipts leave provider success pending and keep the startup barrier closed.
- This contract does not claim that a verifier process, its signing key, or a production verification policy has been deployed. That is a separate acceptance requirement.
