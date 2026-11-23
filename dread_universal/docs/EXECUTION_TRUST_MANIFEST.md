# DREAD Phase 6 — Signed Execution Trust Manifest

**Status:** implemented as a verification/configuration boundary; provider dispatch remains gated pending durable API worker integration and independent result verification.

## Purpose

The API must not treat environment booleans such as `*_SANDBOX_POLICY_VERIFIED=true` as proof of isolation. The signed execution trust manifest provides a server-verified statement from a provisioner/trust authority about a provider registration, its adapter identity, capability revision, isolation controls, and retry contract.

This manifest is not a capability approval and does not grant task permission. The canonical capability record and the authenticated task approval event are checked separately by `Phase6ExecutionAuthority`.

## Configuration

Configure both values together:

- `DREAD_EXECUTION_TRUST_MANIFEST`: path to the signed JSON envelope.
- `DREAD_EXECUTION_TRUST_PUBLIC_KEY_HEX`: 32-byte Ed25519 public key encoded as 64 hexadecimal characters.

If neither is configured, the API starts with provider trust unavailable and reports `not_configured` at the protected `GET /api/execution/trust` endpoint. If only one is configured or the manifest is invalid, API startup fails closed.

The manifest file must be a regular, non-symlink file no larger than 1 MiB.

## Envelope and payload

The envelope has exactly these fields:

```json
{
  "key_id": "dread-provisioner-2026",
  "payload": {
    "schema_version": 1,
    "manifest_id": "provider-trust-2026-11-22-001",
    "verifier_id": "dread-provisioner",
    "source_reference": "provisioner://attestations/bundle-001",
    "observed_at": 1795300000,
    "expires_at": 1795386400,
    "providers": []
  },
  "signature_hex": "<128 hexadecimal characters>"
}
```

Each provider entry must include:

- `provider_id`: one of `hermes`, `deerflow`, or `openhands`.
- `adapter_id`, `adapter_version`, and `endpoint`: must exactly match the configured live adapter.
- `registration_revision`: positive monotonically managed revision.
- `capability_id`: respectively `hermes.run`, `deerflow.run`, or `openhands.run`.
- `capability_revision`: the current `capability_events.id` for the latest `enabled` approval event.
- `operation`: currently `agent.execute`.
- `enabled`, `revoked`, evidence identity/source, and observation/expiry timestamps.
- A nested `isolation` evidence record bound to the provider ID, adapter ID/version, provider-registration revision, and runtime identity.
- Authenticated provenance: source observation ID, recording adapter ID/version, evidence SHA-256, observation time, and an `authenticated` marker covered by the Ed25519 signature.
- Exactly eight individually evidenced controls: `runtime_identity`, `task_workspace_boundary`, `mount_policy`, `egress_policy`, `tool_permissions`, `stable_secret_handling`, `cancellation_propagation`, and `restart_reconciliation`. Each control needs `verified: true` and its own non-empty durable observation ID.
- Isolation evidence must be observed no later than now, remain unexpired, and have a lifetime no longer than 300 seconds.
- A retry contract with contract/evidence IDs, revision, expiry, SHA-256 digest, verifier/source, observation time, idempotency class, idempotency/provider-key support, and a bounded maximum attempt count.

Provider endpoints must use HTTPS or an approved local endpoint. The configured endpoint must match the signed value exactly.

## Signature format

The signature is Ed25519 over the compact UTF-8 JSON serialization of the typed `ExecutionTrustManifest` payload, using the Rust field order and `serde_json::to_vec` serialization used by the API. The signature is not over the outer envelope. The raw 32-byte public key and 64-byte signature are represented as hexadecimal strings.

The verifier rejects invalid signatures, unknown JSON fields, unsupported providers, duplicate provider entries, missing per-control observations, unauthenticated provenance, adapter/revision mismatches, malformed digests, future observations, expired evidence, and lifetimes outside the configured bounds. The API re-reads and re-verifies the signed manifest at each trust preflight, so the provisioner must refresh the short-lived isolation evidence in the file at least every five minutes. Updates should be written atomically; a stale or invalid manifest blocks new provider dispatch.

Do not place private signing keys, provider API tokens, or secret values in the manifest. The manifest contains public attestation and registration metadata only.

## Authorization behavior

The API authority also checks, against the canonical Phase 6 database:

1. The task is an `agent_task`, is queued, and has a current approval by the dispatch requester.
2. A matching `task_approved` event exists and its actor/payload agree with the canonical task approval.
3. The capability record is valid, enabled, of kind Agent, and bound to the same provider.
4. The latest capability audit event is `enabled` and agrees with the capability approval record.
5. The capability audit revision matches the signed provider registration.
6. The live adapter identity, version, endpoint, provider registration revision, operation, and retry contract match the signed manifest.
7. The admission expires no later than the manifest or retry contract.

Provider event/evidence verification is only called from the internal authenticated adapter path. HTTP callers cannot submit these trust facts directly.

## Current limitations — do not bypass

- This module loads and verifies signed trust material and implements the server-side `ExecutionAuthority` checks. The API's legacy direct provider worker has **not yet** been replaced by durable `authorize_and_persist` / `prepare_dispatch` / acknowledgement / event / terminal calls.
- The durable startup dispatch barrier remains closed by default. Do not open it merely because a manifest is present.
- The independent result verifier is not configured. Provider success must remain nonterminal and unverified until a separate verifier produces scoped evidence.
- The existing adapter sandbox environment booleans are not accepted as signed proof by this module. They must not be represented as sufficient isolation verification.
- The manifest is validated at startup and re-read on every trust preflight. Revocation/update takes effect on the next preflight after an atomically replaced, newly signed manifest is written.

This document describes the current implementation boundary, not a declaration of provider dispatch readiness or release acceptance.
