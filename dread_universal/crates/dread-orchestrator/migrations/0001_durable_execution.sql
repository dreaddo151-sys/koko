-- Additive, Phase-6-compatible execution facts.
-- Canonical entities remain in tasks, task_events, task_results, missions and
-- mission_tasks. This migration deliberately creates no replacement entities.

CREATE TABLE IF NOT EXISTS dread_execution_schema_migrations (
    version INTEGER PRIMARY KEY NOT NULL,
    applied_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS dread_task_execution_contracts (
    task_id TEXT PRIMARY KEY NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    provider_id TEXT NOT NULL,
    capability_id TEXT NOT NULL,
    operation TEXT NOT NULL,
    idempotency_class TEXT NOT NULL CHECK (
        idempotency_class IN ('idempotent', 'non_idempotent', 'provider_key_required', 'unknown')
    ),
    idempotency_key TEXT NOT NULL,
    retry_contract_id TEXT NOT NULL,
    retry_contract_evidence_id TEXT NOT NULL,
    retry_contract_revision INTEGER NOT NULL,
    retry_contract_expires_at INTEGER NOT NULL,
    provider_registration_revision INTEGER NOT NULL,
    capability_revision INTEGER NOT NULL,
    adapter_id TEXT NOT NULL,
    adapter_version TEXT NOT NULL,
    idempotent_operation INTEGER NOT NULL CHECK (idempotent_operation IN (0, 1)),
    provider_keys_supported INTEGER NOT NULL CHECK (provider_keys_supported IN (0, 1)),
    max_attempts INTEGER NOT NULL CHECK (max_attempts > 0),
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS dread_dispatch_admissions (
    admission_id TEXT PRIMARY KEY NOT NULL,
    request_id TEXT NOT NULL UNIQUE,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    mission_id TEXT REFERENCES missions(id) ON DELETE CASCADE,
    attempt_number INTEGER NOT NULL CHECK (attempt_number > 0),
    requester_id TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    capability_id TEXT NOT NULL,
    operation TEXT NOT NULL,
    permission_scope TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    evidence_id TEXT NOT NULL,
    policy_decision_id TEXT NOT NULL,
    adapter_id TEXT NOT NULL,
    adapter_version TEXT NOT NULL,
    provider_registration_revision INTEGER NOT NULL,
    capability_revision INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    consumed_at INTEGER,
    created_at INTEGER NOT NULL
);

-- Multiple short-lived admissions may exist for the same upcoming attempt.
-- Only one can ever be consumed because dread_task_attempts has UNIQUE(task_id, attempt_number).
CREATE INDEX IF NOT EXISTS idx_dread_dispatch_admissions_task
    ON dread_dispatch_admissions(task_id, attempt_number);

CREATE TABLE IF NOT EXISTS dread_task_attempts (
    attempt_id TEXT PRIMARY KEY NOT NULL,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    mission_id TEXT REFERENCES missions(id) ON DELETE CASCADE,
    attempt_number INTEGER NOT NULL CHECK (attempt_number > 0),
    request_id TEXT NOT NULL UNIQUE,
    admission_id TEXT NOT NULL UNIQUE REFERENCES dread_dispatch_admissions(admission_id),
    provider_id TEXT NOT NULL,
    capability_id TEXT NOT NULL,
    operation TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    provider_execution_id TEXT,
    provider_result_json TEXT,
    provider_error TEXT,
    dispatch_state TEXT NOT NULL CHECK (
        dispatch_state IN ('dispatching', 'acknowledged', 'running', 'provider_succeeded',
                           'reconciliation_required', 'succeeded', 'failed', 'cancelled')
    ),
    cancellation_state TEXT NOT NULL DEFAULT 'none' CHECK (
        cancellation_state IN ('none', 'requested', 'request_sent', 'accepted', 'cancelling',
                               'confirmed_cancelled', 'completed_successfully', 'failed', 'unknown')
    ),
    outcome_json TEXT,
    dispatched_at INTEGER NOT NULL,
    acknowledged_at INTEGER,
    terminal_at INTEGER,
    recovery_attached_at INTEGER,
    UNIQUE(task_id, attempt_number),
    UNIQUE(provider_id, provider_execution_id)
);

CREATE INDEX IF NOT EXISTS idx_dread_task_attempts_unresolved
    ON dread_task_attempts(dispatch_state, dispatched_at);

CREATE TABLE IF NOT EXISTS dread_execution_runtime_state (
    state_key TEXT PRIMARY KEY NOT NULL,
    state_value TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_dread_task_attempts_task
    ON dread_task_attempts(task_id, attempt_number);

-- Provider event identity is scoped to an exact attempt. A repeated identity
-- with different type/payload is treated as a conflict by the adapter.
CREATE TABLE IF NOT EXISTS dread_provider_event_keys (
    provider_id TEXT NOT NULL,
    attempt_id TEXT NOT NULL REFERENCES dread_task_attempts(attempt_id) ON DELETE CASCADE,
    external_event_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    task_event_sequence INTEGER NOT NULL,
    recorded_at INTEGER NOT NULL,
    PRIMARY KEY(provider_id, attempt_id, external_event_id)
);

-- These rows are written only after trusted server-side adapter/verifier checks.
-- An authenticated boolean or a caller-supplied digest alone is not proof.
CREATE TABLE IF NOT EXISTS dread_execution_evidence (
    evidence_id TEXT PRIMARY KEY NOT NULL,
    attempt_id TEXT NOT NULL REFERENCES dread_task_attempts(attempt_id) ON DELETE CASCADE,
    request_id TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    adapter_id TEXT NOT NULL,
    adapter_version TEXT NOT NULL,
    provider_execution_id TEXT,
    provider_registration_revision INTEGER NOT NULL,
    evidence_kind TEXT NOT NULL CHECK (
        evidence_kind IN ('acknowledged', 'succeeded', 'failed', 'cancelled',
                          'pre_side_effect_failure', 'active', 'cancel_request_sent',
                          'cancel_accepted', 'cancellation_in_progress')
    ),
    verifier_id TEXT NOT NULL,
    source_reference TEXT NOT NULL,
    observed_at INTEGER NOT NULL,
    expires_at INTEGER
);

CREATE TABLE IF NOT EXISTS dread_verification_evidence (
    evidence_id TEXT PRIMARY KEY NOT NULL,
    attempt_id TEXT NOT NULL REFERENCES dread_task_attempts(attempt_id) ON DELETE CASCADE,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    verifier_id TEXT NOT NULL,
    requester_id TEXT NOT NULL,
    provider_verifier_id TEXT NOT NULL,
    passed INTEGER NOT NULL CHECK (passed IN (0, 1)),
    source_reference TEXT NOT NULL,
    observed_at INTEGER NOT NULL,
    expires_at INTEGER,
    CHECK (verifier_id <> requester_id)
);

CREATE TRIGGER IF NOT EXISTS dread_dispatch_admissions_immutable
BEFORE UPDATE ON dread_dispatch_admissions
WHEN
    OLD.admission_id IS NOT NEW.admission_id OR
    OLD.request_id IS NOT NEW.request_id OR
    OLD.task_id IS NOT NEW.task_id OR
    OLD.mission_id IS NOT NEW.mission_id OR
    OLD.attempt_number IS NOT NEW.attempt_number OR
    OLD.requester_id IS NOT NEW.requester_id OR
    OLD.provider_id IS NOT NEW.provider_id OR
    OLD.capability_id IS NOT NEW.capability_id OR
    OLD.operation IS NOT NEW.operation OR
    OLD.permission_scope IS NOT NEW.permission_scope OR
    OLD.idempotency_key IS NOT NEW.idempotency_key OR
    OLD.evidence_id IS NOT NEW.evidence_id OR
    OLD.policy_decision_id IS NOT NEW.policy_decision_id OR
    OLD.adapter_id IS NOT NEW.adapter_id OR
    OLD.adapter_version IS NOT NEW.adapter_version OR
    OLD.provider_registration_revision IS NOT NEW.provider_registration_revision OR
    OLD.capability_revision IS NOT NEW.capability_revision OR
    OLD.expires_at IS NOT NEW.expires_at OR
    OLD.created_at IS NOT NEW.created_at OR
    OLD.consumed_at IS NOT NULL OR
    NEW.consumed_at IS NULL
BEGIN
    SELECT RAISE(ABORT, 'admission scope is immutable and consumption is single-use');
END;

CREATE TRIGGER IF NOT EXISTS dread_dispatch_admissions_no_delete
BEFORE DELETE ON dread_dispatch_admissions
BEGIN
    SELECT RAISE(ABORT, 'dispatch admissions are append-only');
END;

CREATE TRIGGER IF NOT EXISTS dread_execution_evidence_no_update
BEFORE UPDATE ON dread_execution_evidence
BEGIN
    SELECT RAISE(ABORT, 'execution evidence is append-only');
END;
CREATE TRIGGER IF NOT EXISTS dread_execution_evidence_no_delete
BEFORE DELETE ON dread_execution_evidence
BEGIN
    SELECT RAISE(ABORT, 'execution evidence is append-only');
END;
CREATE TRIGGER IF NOT EXISTS dread_verification_evidence_no_update
BEFORE UPDATE ON dread_verification_evidence
BEGIN
    SELECT RAISE(ABORT, 'verification evidence is append-only');
END;
CREATE TRIGGER IF NOT EXISTS dread_verification_evidence_no_delete
BEFORE DELETE ON dread_verification_evidence
BEGIN
    SELECT RAISE(ABORT, 'verification evidence is append-only');
END;


-- Execution contracts, provider event identities and attempt/evidence history
-- are audit records. They may be advanced only through the dedicated lifecycle
-- methods and must not disappear when a parent row is deleted.
CREATE TRIGGER IF NOT EXISTS dread_task_execution_contracts_no_update
BEFORE UPDATE ON dread_task_execution_contracts
BEGIN
    SELECT RAISE(ABORT, 'task execution contract is immutable');
END;
CREATE TRIGGER IF NOT EXISTS dread_task_execution_contracts_no_delete
BEFORE DELETE ON dread_task_execution_contracts
BEGIN
    SELECT RAISE(ABORT, 'task execution contract is append-only');
END;
CREATE TRIGGER IF NOT EXISTS dread_task_attempts_no_delete
BEFORE DELETE ON dread_task_attempts
BEGIN
    SELECT RAISE(ABORT, 'execution attempts are append-only');
END;
CREATE TRIGGER IF NOT EXISTS dread_provider_event_keys_no_update
BEFORE UPDATE ON dread_provider_event_keys
BEGIN
    SELECT RAISE(ABORT, 'provider event identities are append-only');
END;
CREATE TRIGGER IF NOT EXISTS dread_provider_event_keys_no_delete
BEFORE DELETE ON dread_provider_event_keys
BEGIN
    SELECT RAISE(ABORT, 'provider event identities are append-only');
END;
CREATE TRIGGER IF NOT EXISTS dread_execution_schema_migrations_no_update
BEFORE UPDATE ON dread_execution_schema_migrations
BEGIN
    SELECT RAISE(ABORT, 'execution migration history is immutable');
END;
CREATE TRIGGER IF NOT EXISTS dread_execution_schema_migrations_no_delete
BEFORE DELETE ON dread_execution_schema_migrations
BEGIN
    SELECT RAISE(ABORT, 'execution migration history is append-only');
END;