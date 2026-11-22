-- Persist the trusted identity of the local worker that initiated each attempt.
-- A later recovery worker does not replace this original execution identity.

ALTER TABLE dread_task_attempts
    ADD COLUMN worker_id TEXT NOT NULL DEFAULT 'legacy_unbound';

CREATE TRIGGER IF NOT EXISTS dread_task_attempts_worker_identity_immutable
BEFORE UPDATE ON dread_task_attempts
WHEN OLD.worker_id IS NOT NEW.worker_id
BEGIN
    SELECT RAISE(ABORT, 'original attempt worker identity is immutable');
END;

CREATE TRIGGER IF NOT EXISTS dread_verification_evidence_attempt_worker_match
BEFORE INSERT ON dread_verification_evidence
WHEN NEW.worker_id IS NOT (
    SELECT worker_id FROM dread_task_attempts WHERE attempt_id = NEW.attempt_id
)
BEGIN
    SELECT RAISE(ABORT, 'verification worker identity does not match the original attempt worker');
END;