-- Extend the immutable provider evidence kind allow-list for the exact
-- signed adapter cancellation receipt variants. Rebuild only the evidence
-- table, copying all historical rows and receipt bytes without reinterpretation.

DROP TRIGGER IF EXISTS dread_execution_evidence_no_update;
DROP TRIGGER IF EXISTS dread_execution_evidence_no_delete;

CREATE TABLE dread_execution_evidence_v7 (
    evidence_id TEXT PRIMARY KEY NOT NULL,
    attempt_id TEXT NOT NULL REFERENCES dread_task_attempts(attempt_id) ON DELETE CASCADE,
    request_id TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    adapter_id TEXT NOT NULL,
    adapter_version TEXT NOT NULL,
    provider_execution_id TEXT,
    provider_registration_revision INTEGER NOT NULL,
    evidence_kind TEXT NOT NULL CHECK (
        evidence_kind IN (
            'acknowledged',
            'succeeded',
            'failed',
            'cancelled',
            'pre_side_effect_failure',
            'active',
            'cancel_request_sent',
            'cancel_accepted',
            'cancellation_in_progress',
            'cancellation_accepted',
            'cancellation_unknown'
        )
    ),
    verifier_id TEXT NOT NULL,
    source_reference TEXT NOT NULL,
    observed_at INTEGER NOT NULL,
    expires_at INTEGER,
    receipt_key_id TEXT,
    receipt_payload_json TEXT,
    receipt_signature_hex TEXT,
    receipt_response_sha256 TEXT
);

INSERT INTO dread_execution_evidence_v7 (
    evidence_id,
    attempt_id,
    request_id,
    provider_id,
    adapter_id,
    adapter_version,
    provider_execution_id,
    provider_registration_revision,
    evidence_kind,
    verifier_id,
    source_reference,
    observed_at,
    expires_at,
    receipt_key_id,
    receipt_payload_json,
    receipt_signature_hex,
    receipt_response_sha256
)
SELECT
    evidence_id,
    attempt_id,
    request_id,
    provider_id,
    adapter_id,
    adapter_version,
    provider_execution_id,
    provider_registration_revision,
    evidence_kind,
    verifier_id,
    source_reference,
    observed_at,
    expires_at,
    receipt_key_id,
    receipt_payload_json,
    receipt_signature_hex,
    receipt_response_sha256
FROM dread_execution_evidence;

DROP TABLE dread_execution_evidence;
ALTER TABLE dread_execution_evidence_v7 RENAME TO dread_execution_evidence;

CREATE TRIGGER dread_execution_evidence_no_update
BEFORE UPDATE ON dread_execution_evidence
BEGIN
    SELECT RAISE(ABORT, 'execution evidence is append-only');
END;

CREATE TRIGGER dread_execution_evidence_no_delete
BEFORE DELETE ON dread_execution_evidence
BEGIN
    SELECT RAISE(ABORT, 'execution evidence is append-only');
END;
