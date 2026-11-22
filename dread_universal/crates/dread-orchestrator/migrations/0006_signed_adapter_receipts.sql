-- Preserve the exact Ed25519 adapter receipt material with provider evidence.
-- Existing historical rows stay nullable and are not retroactively reclassified.
ALTER TABLE dread_execution_evidence
    ADD COLUMN receipt_key_id TEXT;
ALTER TABLE dread_execution_evidence
    ADD COLUMN receipt_payload_json TEXT;
ALTER TABLE dread_execution_evidence
    ADD COLUMN receipt_signature_hex TEXT;

ALTER TABLE dread_provider_event_keys
    ADD COLUMN receipt_key_id TEXT;
ALTER TABLE dread_provider_event_keys
    ADD COLUMN receipt_payload_json TEXT;
ALTER TABLE dread_provider_event_keys
    ADD COLUMN receipt_signature_hex TEXT;
