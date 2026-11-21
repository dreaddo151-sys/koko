-- Additive trusted retry-contract provenance and immutable attempt identity.
-- This migration belongs to DREAD's private execution schema version space;
-- it does not consume or rewrite the API's existing migration numbers.

CREATE TABLE IF NOT EXISTS dread_trusted_retry_contract_evidence (
    evidence_id TEXT PRIMARY KEY NOT NULL,
    contract_id TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    provider_registration_revision INTEGER NOT NULL,
    capability_id TEXT NOT NULL,
    capability_revision INTEGER NOT NULL,
    adapter_id TEXT NOT NULL,
    adapter_version TEXT NOT NULL,
    operation TEXT NOT NULL,
    contract_revision INTEGER NOT NULL,
    idempotent_operation INTEGER NOT NULL CHECK (idempotent_operation IN (0, 1)),
    provider_keys_supported INTEGER NOT NULL CHECK (provider_keys_supported IN (0, 1)),
    contract_sha256 TEXT NOT NULL CHECK (length(contract_sha256) = 64),
    verifier_id TEXT NOT NULL,
    source_reference TEXT NOT NULL,
    authenticated INTEGER NOT NULL CHECK (authenticated IN (0, 1)),
    observed_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    CHECK (expires_at > observed_at)
);

CREATE INDEX IF NOT EXISTS idx_dread_retry_contract_evidence_scope
    ON dread_trusted_retry_contract_evidence(
        provider_id, capability_id, operation, contract_revision, expires_at
    );

CREATE TRIGGER IF NOT EXISTS dread_trusted_retry_contract_evidence_no_update
BEFORE UPDATE ON dread_trusted_retry_contract_evidence
BEGIN
    SELECT RAISE(ABORT, 'trusted retry-contract evidence is append-only');
END;

CREATE TRIGGER IF NOT EXISTS dread_trusted_retry_contract_evidence_no_delete
BEFORE DELETE ON dread_trusted_retry_contract_evidence
BEGIN
    SELECT RAISE(ABORT, 'trusted retry-contract evidence is append-only');
END;

-- The attempt identity is fixed at creation. Only lifecycle/result columns
-- may change through the dedicated persistence functions.
CREATE TRIGGER IF NOT EXISTS dread_task_attempts_identity_immutable
BEFORE UPDATE ON dread_task_attempts
WHEN
    OLD.attempt_id IS NOT NEW.attempt_id OR
    OLD.task_id IS NOT NEW.task_id OR
    OLD.mission_id IS NOT NEW.mission_id OR
    OLD.attempt_number IS NOT NEW.attempt_number OR
    OLD.request_id IS NOT NEW.request_id OR
    OLD.admission_id IS NOT NEW.admission_id OR
    OLD.provider_id IS NOT NEW.provider_id OR
    OLD.capability_id IS NOT NEW.capability_id OR
    OLD.operation IS NOT NEW.operation OR
    OLD.idempotency_key IS NOT NEW.idempotency_key OR
    OLD.dispatched_at IS NOT NEW.dispatched_at OR
    (OLD.provider_execution_id IS NOT NULL AND
     OLD.provider_execution_id IS NOT NEW.provider_execution_id)
BEGIN
    SELECT RAISE(ABORT, 'execution attempt identity is immutable');
END;