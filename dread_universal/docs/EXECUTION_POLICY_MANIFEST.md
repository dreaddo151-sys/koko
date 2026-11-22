# DREAD Phase 6 — Signed Execution Policy Manifest

**Status:** integrated into provider admission/revalidation; this policy is an independent authorization layer and does not enable provider execution by itself.

## Purpose

The signed provider trust manifest establishes what provider, adapter and capability registration DREAD is willing to trust. It does not decide whether a particular requester may execute a particular task.

The signed execution policy is a separate Ed25519 trust artifact. It binds an authorization decision to the exact request, requester, task, mission, capability, operation, permission scope and attempt number. The API requires both provider trust and a matching policy allow rule before a durable admission can be created.

Canonical task approval remains a separate condition. It is not converted into a policy decision and cannot substitute for the policy signing authority.

## Configuration

Configure both values together:

- `DREAD_EXECUTION_POLICY_MANIFEST`: path to the signed execution policy JSON envelope.
- `DREAD_EXECUTION_POLICY_PUBLIC_KEY_HEX`: 32-byte Ed25519 public key encoded as 64 hexadecimal characters.

These values use a separate signing key from `DREAD_EXECUTION_TRUST_PUBLIC_KEY_HEX`. Do not reuse the provider-provisioning key for policy authorization.

If neither value is configured, the API can start but provider dispatch admission fails closed. If only one is configured, or the envelope is malformed, expired, or has an invalid signature, API startup fails closed. A policy file without provider trust configuration is rejected.

The policy file must be a regular, non-symlink file no larger than 1 MiB. It is re-read and re-verified whenever an admission or admission revalidation checks policy.

## Envelope and payload

The outer envelope has exactly these fields:

```json
{
  "key_id": "dread-policy-key-2026",
  "payload": {
    "schema_version": 1,
    "policy_id": "dread-agent-execution",
    "revision": 4,
    "verifier_id": "dread-policy-authority",
    "source_reference": "policy://project-dread/agent-execution/4",
    "observed_at": 1795300000,
    "expires_at": 1795386400,
    "rules": [
      {
        "rule_id": "allow-hermes-mission-42",
        "requester_id": "operator-identity",
        "task_id": "task-42",
        "mission_id": "mission-7",
        "capability_id": "hermes.run",
        "operation": "agent.execute",
        "permission_scope": "agent:execute",
        "effect": "allow",
        "issued_at": 1795300000,
        "expires_at": 1795386400,
        "max_attempts": 2
      }
    ]
  },
  "signature_hex": "<128 hexadecimal characters>"
}
```

Rule fields:

- `rule_id`: unique within this policy manifest.
- `requester_id`: required exact requester identity; there is no requester wildcard.
- `task_id` and `mission_id`: optional. Omitting either makes that field a wildcard; use exact IDs for narrow, high-assurance grants.
- `capability_id`: one of `hermes.run`, `deerflow.run`, or `openhands.run`.
- `operation`: currently `agent.execute`.
- `permission_scope`: currently `agent:execute`.
- `effect`: `allow` or `deny`. A matching deny always overrides an allow.
- `issued_at` and `expires_at`: bounded Unix timestamps; rules cannot outlive the manifest.
- `max_attempts`: 1–8; an attempt above the rule's limit is denied.

The manifest supports at most 512 rules and a maximum lifetime of 30 days. It must be current, not future-dated, contain unique rule IDs and have at least one rule. A request is authorized only when exactly one applicable allow rule matches and no deny rule matches. No match or multiple matching allow rules are denied.

## Decision identity and revalidation

After verifying the signature, the API derives a deterministic decision ID from the policy ID and revision, the SHA-256 digest of the signed payload, the selected rule ID, and all request scope fields:

- request ID and requester ID;
- task ID and canonical mission ID;
- capability ID and operation;
- permission scope and attempt number.

The derived decision ID, not an operator approval sequence, is persisted as the admission's `policy_decision_id`. The admission expiry is capped by both provider-trust expiry and policy/rule expiry.

Before an admission is consumed into a durable attempt, the API re-reads the policy and recomputes the decision. If the rule was removed, denied, expired, or changed in any way that affects the signed payload or request scope, the persisted decision ID no longer matches and dispatch preparation fails closed.

The policy issuer should monotonically increase `revision` for each policy update and retain its signing-key audit history. Automated policy-revision rollback detection and signing-key rotation workflows are not yet implemented; until then, the Ed25519 policy key remains the root of trust for policy issuance.

## Signature format and handling

The signature is Ed25519 over the compact UTF-8 JSON serialization of the typed `ExecutionPolicyManifest` payload, using the Rust field order and `serde_json::to_vec` serialization implemented by the API. The signature is not over the outer envelope. The raw 32-byte public key and 64-byte signature are encoded as hexadecimal strings.

Never store a private signing key, provider API token or secret value in the policy file. Keep private keys in the trusted policy issuer's protected key store. Publish policy files atomically; a missing, invalid, expired, or replaced policy must never be treated as an allow decision.

## Release boundary

This policy module does not enable provider starts. Provider dispatch remains blocked until the following independent controls are implemented and verified:

- attempt-bound authenticated provider acknowledgement, active status, event, terminal and cancellation evidence;
- durable API integration that starts a provider only for `DispatchPreparation::Created` and never starts again for `Existing` replay;
- startup reconciliation and worker reattachment for the current recovery generation;
- independent result verification before task success or mission dependency release.

The API currently keeps the startup dispatch barrier closed. A valid policy manifest is necessary for admission, but it is not a release or provider-dispatch readiness signal.
