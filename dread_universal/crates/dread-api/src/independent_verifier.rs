//! Independent result verification through a separately keyed local receipt spool.
//!
//! The verifier process is outside the API worker and provider adapter. It
//! writes a short-lived signed heartbeat plus one signed result receipt per
//! durable request. The API trusts only the pinned verifier public key and
//! exact durable task/attempt/result identities; absence, expiry or tampering
//! remains fail-closed.

use dread_orchestrator::persistence::{AttemptRecord, VerificationFacts};
use ring::{digest, signature};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

const MAX_RECEIPT_BYTES: u64 = 64 * 1024;
const MAX_RESULT_RECEIPT_LIFETIME_SECONDS: i64 = 300;
const MAX_HEARTBEAT_LIFETIME_SECONDS: i64 = 120;

#[derive(Debug, Clone)]
pub struct IndependentResultVerifier {
    receipt_dir: PathBuf,
    verifier_id: String,
    key_id: String,
    public_key: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SignedEnvelope<T> {
    key_id: String,
    payload: T,
    signature_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct VerifierHeartbeat {
    schema_version: u32,
    verifier_id: String,
    observed_at: i64,
    expires_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IndependentResultReceipt {
    schema_version: u32,
    evidence_id: String,
    verifier_id: String,
    request_id: String,
    attempt_id: String,
    task_id: String,
    requester_id: String,
    worker_id: String,
    provider_verifier_id: String,
    result_sha256: Option<String>,
    error_sha256: Option<String>,
    passed: bool,
    source_reference: String,
    observed_at: i64,
    expires_at: i64,
}

impl IndependentResultVerifier {
    pub fn load_from_env() -> Result<Option<Self>, String> {
        let receipt_dir = env::var_os("DREAD_INDEPENDENT_RESULT_VERIFIER_RECEIPT_DIR");
        let verifier_id = env::var("DREAD_INDEPENDENT_RESULT_VERIFIER_ID").ok();
        let key_id = env::var("DREAD_INDEPENDENT_RESULT_VERIFIER_KEY_ID").ok();
        let public_key_hex = env::var("DREAD_INDEPENDENT_RESULT_VERIFIER_PUBLIC_KEY_HEX").ok();

        let configured = [
            receipt_dir.is_some(),
            verifier_id.is_some(),
            key_id.is_some(),
            public_key_hex.is_some(),
        ];
        if configured.iter().all(|present| !present) {
            return Ok(None);
        }
        if configured.iter().any(|present| !present) {
            return Err(
                "independent result verifier receipt directory, verifier ID, key ID and public key must be configured together"
                    .into(),
            );
        }

        let receipt_dir = PathBuf::from(receipt_dir.expect("presence checked"));
        validate_secure_directory(&receipt_dir)?;
        let verifier_id = verifier_id.expect("presence checked");
        let key_id = key_id.expect("presence checked");
        validate_text("independent verifier ID", &verifier_id, 160)?;
        validate_text("independent verifier key ID", &key_id, 128)?;
        let public_key_hex = public_key_hex.expect("presence checked");
        let public_key = decode_hex(&public_key_hex)?;
        if public_key.len() != 32 {
            return Err("independent verifier Ed25519 public key must be exactly 32 bytes".into());
        }

        Ok(Some(Self {
            receipt_dir,
            verifier_id,
            key_id,
            public_key,
        }))
    }

    pub fn status(&self, now: i64) -> serde_json::Value {
        match self.read_heartbeat(now) {
            Ok(heartbeat) => serde_json::json!({
                "status": "verified",
                "verifier_id": self.verifier_id,
                "key_id": self.key_id,
                "observed_at": heartbeat.observed_at,
                "expires_at": heartbeat.expires_at
            }),
            Err(error) => serde_json::json!({
                "status": "configured_unavailable",
                "verifier_id": self.verifier_id,
                "key_id": self.key_id,
                "dispatch": "blocked",
                "reason": error
            }),
        }
    }

    pub fn verify_result(
        &self,
        connection: &Connection,
        attempt: &AttemptRecord,
        result_json: Option<&str>,
        provider_error: Option<&str>,
        now: i64,
    ) -> Result<VerificationFacts, String> {
        self.read_heartbeat(now)?;
        validate_safe_id("request_id", &attempt.request_id)?;
        let receipt_path = self
            .receipt_dir
            .join(format!("{}.json", attempt.request_id));
        let receipt: IndependentResultReceipt = self.read_signed_payload(&receipt_path)?;
        let requester_id: String = connection
            .query_row(
                "SELECT requester_id FROM dread_dispatch_admissions WHERE request_id = ?1",
                [&attempt.request_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| {
                "durable admission requester is unavailable for verification".to_string()
            })?;
        let provider_verifier_id: String = connection
            .query_row(
                "SELECT verifier_id FROM dread_execution_evidence
                 WHERE attempt_id = ?1 AND evidence_kind = 'succeeded'
                 ORDER BY observed_at DESC, evidence_id ASC LIMIT 1",
                [&attempt.attempt_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "provider success receipt identity is missing".to_string())?;

        if receipt.schema_version != 1
            || receipt.verifier_id != self.verifier_id
            || receipt.request_id != attempt.request_id
            || receipt.attempt_id != attempt.attempt_id
            || receipt.task_id != attempt.task_id
            || receipt.requester_id != requester_id
            || receipt.worker_id != attempt.worker_id
            || receipt.provider_verifier_id != provider_verifier_id
            || receipt.verifier_id == requester_id
            || receipt.verifier_id == attempt.worker_id
            || receipt.verifier_id == provider_verifier_id
        {
            return Err(
                "independent verifier receipt does not match the canonical request, attempt, requester, worker or provider identity"
                    .into(),
            );
        }

        let expected_result_digest = result_json.map(sha256_hex);
        let expected_error_digest = provider_error.map(sha256_hex);
        if receipt.result_sha256 != expected_result_digest
            || receipt.error_sha256 != expected_error_digest
        {
            return Err(
                "independent verifier receipt does not cover the exact durable result and error bytes"
                    .into(),
            );
        }
        validate_text(
            "independent verification evidence ID",
            &receipt.evidence_id,
            1024,
        )?;
        validate_text(
            "independent verifier source",
            &receipt.source_reference,
            2048,
        )?;
        if receipt.observed_at <= 0
            || receipt.observed_at > now
            || receipt.expires_at <= now
            || receipt.expires_at <= receipt.observed_at
            || receipt.expires_at
                > receipt
                    .observed_at
                    .saturating_add(MAX_RESULT_RECEIPT_LIFETIME_SECONDS)
        {
            return Err("independent verification receipt is future-dated, expired or outside its lifetime bound".into());
        }

        Ok(VerificationFacts {
            evidence_id: receipt.evidence_id,
            verifier_id: receipt.verifier_id,
            requester_id: receipt.requester_id,
            worker_id: receipt.worker_id,
            passed: receipt.passed,
            source_reference: receipt.source_reference,
            result_json: result_json.map(str::to_owned),
            error: provider_error.map(str::to_owned),
            observed_at: receipt.observed_at,
            expires_at: Some(receipt.expires_at),
        })
    }

    fn read_heartbeat(&self, now: i64) -> Result<VerifierHeartbeat, String> {
        let path = self.receipt_dir.join(".dread-verifier-ready.json");
        let heartbeat: VerifierHeartbeat = self.read_signed_payload(&path)?;
        if heartbeat.schema_version != 1
            || heartbeat.verifier_id != self.verifier_id
            || heartbeat.observed_at <= 0
            || heartbeat.observed_at > now
            || heartbeat.expires_at <= now
            || heartbeat.expires_at <= heartbeat.observed_at
            || heartbeat.expires_at
                > heartbeat
                    .observed_at
                    .saturating_add(MAX_HEARTBEAT_LIFETIME_SECONDS)
        {
            return Err(
                "independent verifier heartbeat is expired, future-dated or invalid".into(),
            );
        }
        Ok(heartbeat)
    }

    fn read_signed_payload<T: Serialize + DeserializeOwned>(
        &self,
        path: &Path,
    ) -> Result<T, String> {
        validate_secure_receipt_file(path)?;
        let encoded = fs::read(path)
            .map_err(|error| format!("cannot read independent verifier receipt: {error}"))?;
        let envelope: SignedEnvelope<T> = serde_json::from_slice(&encoded)
            .map_err(|error| format!("independent verifier receipt JSON is invalid: {error}"))?;
        if envelope.key_id != self.key_id {
            return Err(
                "independent verifier receipt key ID does not match the configured trust anchor"
                    .into(),
            );
        }
        let signature_bytes = decode_hex(&envelope.signature_hex)?;
        if signature_bytes.len() != 64 {
            return Err("independent verifier Ed25519 signature must be exactly 64 bytes".into());
        }
        let payload_bytes = serde_json::to_vec(&envelope.payload).map_err(|error| {
            format!("cannot canonicalize independent verifier payload: {error}")
        })?;
        signature::UnparsedPublicKey::new(&signature::ED25519, &self.public_key)
            .verify(&payload_bytes, &signature_bytes)
            .map_err(|_| "independent verifier receipt signature is invalid".to_string())?;
        Ok(envelope.payload)
    }
}

fn validate_secure_directory(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        format!("cannot inspect independent verifier receipt directory: {error}")
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(
            "independent verifier receipt path must be a regular non-symlink directory".into(),
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(
                "independent verifier receipt directory must be owner-only (0700 or stricter)"
                    .into(),
            );
        }
    }
    Ok(())
}

fn validate_secure_receipt_file(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect independent verifier receipt: {error}"))?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() == 0
        || metadata.len() > MAX_RECEIPT_BYTES
    {
        return Err(
            "independent verifier receipt must be a regular non-symlink file no larger than 64 KiB"
                .into(),
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(
                "independent verifier receipts must be owner-only (0600 or stricter)".into(),
            );
        }
    }
    Ok(())
}

fn validate_safe_id(name: &str, value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(format!("{name} is invalid for a verifier receipt filename"));
    }
    Ok(())
}

fn validate_text(name: &str, value: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(format!(
            "{name} is empty, too long or contains control characters"
        ));
    }
    Ok(())
}

fn sha256_hex(value: &str) -> String {
    let digest = digest::digest(&digest::SHA256, value.as_bytes());
    hex_encode(digest.as_ref())
}

fn decode_hex(value: &str) -> Result<Vec<u8>, String> {
    let (pairs, remainder) = value.as_bytes().as_chunks::<2>();
    if value.is_empty() || !remainder.is_empty() {
        return Err("hex value is empty or has an odd length".into());
    }
    pairs
        .iter()
        .map(|pair| {
            let digit = |byte: u8| match byte {
                b'0'..=b'9' => Some(byte - b'0'),
                b'a'..=b'f' => Some(byte - b'a' + 10),
                b'A'..=b'F' => Some(byte - b'A' + 10),
                _ => None,
            };
            let high =
                digit(pair[0]).ok_or_else(|| "hex value contains a non-hex digit".to_string())?;
            let low =
                digit(pair[1]).ok_or_else(|| "hex value contains a non-hex digit".to_string())?;
            Ok((high << 4) | low)
        })
        .collect()
}

fn hex_encode(value: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(value.len() * 2);
    for byte in value {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::{Ed25519KeyPair, KeyPair};
    use std::{
        os::unix::fs::PermissionsExt,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn signed<T: Serialize>(key: &Ed25519KeyPair, key_id: &str, payload: &T) -> String {
        let bytes = serde_json::to_vec(payload).unwrap();
        let signature_hex = hex_encode(key.sign(&bytes).as_ref());
        serde_json::to_string(&SignedEnvelope {
            key_id: key_id.to_owned(),
            payload,
            signature_hex,
        })
        .unwrap()
    }

    fn temp_dir() -> PathBuf {
        let id = format!(
            "dread-verifier-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let path = env::temp_dir().join(id);
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    fn write_secure(path: &Path, content: &str) {
        fs::write(path, content).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }

    #[test]
    fn independently_signed_verifier_heartbeat_is_required() {
        let seed = [31_u8; 32];
        let key = Ed25519KeyPair::from_seed_unchecked(&seed).unwrap();
        let dir = temp_dir();
        let verifier = IndependentResultVerifier {
            receipt_dir: dir.clone(),
            verifier_id: "dread-independent-result-verifier".into(),
            key_id: "result-verifier-key-1".into(),
            public_key: key.public_key().as_ref().to_vec(),
        };
        let absent = verifier.status(1_800_000_000);
        assert_eq!(absent["status"], "configured_unavailable");

        let heartbeat = VerifierHeartbeat {
            schema_version: 1,
            verifier_id: verifier.verifier_id.clone(),
            observed_at: 1_800_000_000,
            expires_at: 1_800_000_060,
        };
        write_secure(
            &dir.join(".dread-verifier-ready.json"),
            &signed(&key, &verifier.key_id, &heartbeat),
        );
        assert_eq!(verifier.status(1_800_000_020)["status"], "verified");
        assert_eq!(
            verifier.status(1_800_000_061)["status"],
            "configured_unavailable"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn signed_result_receipt_binds_exact_attempt_and_result_bytes() {
        let seed = [32_u8; 32];
        let key = Ed25519KeyPair::from_seed_unchecked(&seed).unwrap();
        let dir = temp_dir();
        let verifier = IndependentResultVerifier {
            receipt_dir: dir.clone(),
            verifier_id: "dread-independent-result-verifier".into(),
            key_id: "result-verifier-key-1".into(),
            public_key: key.public_key().as_ref().to_vec(),
        };
        let now = 1_800_000_000_i64;
        let heartbeat = VerifierHeartbeat {
            schema_version: 1,
            verifier_id: verifier.verifier_id.clone(),
            observed_at: now,
            expires_at: now + 60,
        };
        write_secure(
            &dir.join(".dread-verifier-ready.json"),
            &signed(&key, &verifier.key_id, &heartbeat),
        );

        let result = r#"{"ok":true}"#;
        let payload = IndependentResultReceipt {
            schema_version: 1,
            evidence_id: "independent-result-1".into(),
            verifier_id: verifier.verifier_id.clone(),
            request_id: "request-1".into(),
            attempt_id: "attempt-1".into(),
            task_id: "task-1".into(),
            requester_id: "operator-1".into(),
            worker_id: "worker-1".into(),
            provider_verifier_id: "dread-adapter-receipt:hermes".into(),
            result_sha256: Some(sha256_hex(result)),
            error_sha256: None,
            passed: true,
            source_reference: "verifier://dread-independent-result-verifier/1".into(),
            observed_at: now,
            expires_at: now + 120,
        };
        write_secure(
            &dir.join("request-1.json"),
            &signed(&key, &verifier.key_id, &payload),
        );

        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(
            "CREATE TABLE dread_dispatch_admissions(request_id TEXT, requester_id TEXT);
             INSERT INTO dread_dispatch_admissions VALUES ('request-1', 'operator-1');
             CREATE TABLE dread_execution_evidence(attempt_id TEXT, evidence_kind TEXT, verifier_id TEXT);
             INSERT INTO dread_execution_evidence VALUES ('attempt-1', 'succeeded', 'dread-adapter-receipt:hermes');"
        ).unwrap();
        let attempt = AttemptRecord {
            attempt_id: "attempt-1".into(),
            task_id: "task-1".into(),
            mission_id: None,
            attempt_number: 1,
            request_id: "request-1".into(),
            provider_id: "hermes".into(),
            capability_id: "hermes.run".into(),
            operation: "agent.execute".into(),
            idempotency_key: "project-dread:task-1".into(),
            worker_id: "worker-1".into(),
            provider_execution_id: Some("remote-1".into()),
            provider_result_json: Some(result.into()),
            provider_error: None,
            outcome_json: None,
            dispatch_state: "provider_succeeded".into(),
            cancellation_state: "none".into(),
            dispatched_at: now - 10,
            acknowledged_at: Some(now - 9),
            terminal_at: None,
            recovery_attached_at: None,
        };
        let facts = verifier
            .verify_result(&connection, &attempt, Some(result), None, now + 1)
            .unwrap();
        assert!(facts.passed);
        assert_eq!(facts.verifier_id, verifier.verifier_id);
        assert_eq!(facts.result_json.as_deref(), Some(result));
        assert!(
            verifier
                .verify_result(
                    &connection,
                    &attempt,
                    Some(r#"{"ok":false}"#),
                    None,
                    now + 1
                )
                .is_err()
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
