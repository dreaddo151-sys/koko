-- Bind independent verification to a verifier distinct from the task requester,
-- the executing worker, and the provider's own outcome attestation identity.
-- This is a forward-only migration in DREAD's private execution-schema space.

ALTER TABLE dread_verification_evidence
    ADD COLUMN worker_id TEXT NOT NULL DEFAULT 'legacy_unbound';

CREATE TRIGGER IF NOT EXISTS dread_verification_evidence_identity_guard
BEFORE INSERT ON dread_verification_evidence
WHEN
    length(trim(NEW.worker_id)) = 0
    OR NEW.verifier_id = NEW.requester_id
    OR NEW.verifier_id = NEW.worker_id
    OR NEW.verifier_id = NEW.provider_verifier_id
BEGIN
    SELECT RAISE(ABORT, 'verification identity must be independent of requester, worker, and provider attestation');
END;

CREATE TRIGGER IF NOT EXISTS dread_verification_evidence_worker_immutable
BEFORE UPDATE ON dread_verification_evidence
WHEN OLD.worker_id IS NOT NEW.worker_id
BEGIN
    SELECT RAISE(ABORT, 'verification worker identity is immutable');
END;