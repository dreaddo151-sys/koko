//! Separate, read-only verifier process for Phase 6 provider results.
//!
//! The process never writes to the DREAD SQLite database. It reads only
//! provider_succeeded attempts, evaluates an explicit capability policy,
//! and atomically publishes independently signed receipts and a short-lived
//! heartbeat into the API's owner-only receipt directory.

use ring::{
    digest,
    rand::SystemRandom,
    signature::{ED25519, Ed25519KeyPair, KeyPair, UnparsedPublicKey},
};
use rusqlite::{Connection, OpenFlags, OptionalExtension, Row};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    env, fs,
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

const HEARTBEAT_TTL_SECONDS: i64 = 60;
const RESULT_RECEIPT_TTL_SECONDS: i64 = 300;
const MAX_POLICY_BYTES: u64 = 1024 * 1024;
const MAX_PRIVATE_KEY_BYTES: u64 = 4096;
const MAX_RESULT_BYTES: usize = 4 * 1024 * 1024;
const RECEIPT_REFRESH_MARGIN_SECONDS: i64 = 30;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SignedEnvelope<T> {
    key_id: String,
    payload: T,
    signature_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct VerifierHeartbeat {
    schema_version: u32,
    verifier_id: String,
    observed_at: i64,
    expires_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct VerificationPolicy {
    schema_version: u32,
    policy_id: String,
    rules: Vec<CapabilityRule>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct CapabilityRule {
    capability_id: String,
    rule_id: String,
    max_result_bytes: usize,
    required_json_pointers: Vec<String>,
    #[serde(default)]
    forbidden_json_pointers: Vec<String>,
}

#[derive(Debug, Clone)]
struct PendingAttempt {
    request_id: String,
    attempt_id: String,
    task_id: String,
    worker_id: String,
    provider_id: String,
    capability_id: String,
    requester_id: String,
    provider_verifier_id: String,
    result_json: Option<String>,
    provider_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResultDecision {
    passed: bool,
    rule_id: String,
    reason_code: &'static str,
}

struct VerifierConfig {
    database_path: PathBuf,
    receipt_dir: PathBuf,
    verifier_id: String,
    key_id: String,
    private_key_path: PathBuf,
    policy_path: PathBuf,
    receipt_uid: u32,
    receipt_gid: u32,
    poll_interval: Duration,
}

struct VerifierOutput<'a> {
    receipt_dir: &'a Path,
    verifier_id: &'a str,
    key_id: &'a str,
    keypair: &'a Ed25519KeyPair,
    receipt_uid: u32,
    receipt_gid: u32,
}

struct ProcessLock {
    path: PathBuf,
    _file: File,
}

impl Drop for ProcessLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn now_epoch_seconds() -> Result<i64, String> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock is before the Unix epoch: {error}"))?;
    i64::try_from(duration.as_secs())
        .map_err(|_| "system clock timestamp is out of range".to_string())
}

fn validate_text(name: &str, value: &str, max_len: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max_len || value.chars().any(char::is_control) {
        return Err(format!(
            "{name} is empty, too long or contains control characters"
        ));
    }
    Ok(())
}

fn validate_safe_component(name: &str, value: &str, max_len: usize) -> Result<(), String> {
    if value.is_empty()
        || value.len() > max_len
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(format!("{name} is invalid for a receipt filename"));
    }
    Ok(())
}

fn validate_safe_label(name: &str, value: &str, max_len: usize) -> Result<(), String> {
    if value.is_empty()
        || value.len() > max_len
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(format!(
            "{name} must contain only ASCII letters, digits, dot, dash or underscore"
        ));
    }
    Ok(())
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
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

fn sha256_hex(bytes: &[u8]) -> String {
    let value = digest::digest(&digest::SHA256, bytes);
    hex_encode(value.as_ref())
}

fn sign_payload<T: Serialize>(
    keypair: &Ed25519KeyPair,
    key_id: &str,
    payload: T,
) -> Result<Vec<u8>, String> {
    let payload_bytes = serde_json::to_vec(&payload)
        .map_err(|error| format!("cannot serialize signed verifier payload: {error}"))?;
    let envelope = SignedEnvelope {
        key_id: key_id.to_owned(),
        payload,
        signature_hex: hex_encode(keypair.sign(&payload_bytes).as_ref()),
    };
    serde_json::to_vec(&envelope)
        .map_err(|error| format!("cannot serialize signed verifier envelope: {error}"))
}

fn verify_envelope<T>(encoded: &[u8], key_id: &str, public_key: &[u8]) -> Result<T, String>
where
    T: Serialize + DeserializeOwned,
{
    let envelope: SignedEnvelope<T> = serde_json::from_slice(encoded)
        .map_err(|error| format!("signed verifier envelope is invalid JSON: {error}"))?;
    if envelope.key_id != key_id {
        return Err("signed verifier envelope uses an unexpected key ID".into());
    }
    let signature_bytes = decode_hex(&envelope.signature_hex)?;
    if signature_bytes.len() != 64 {
        return Err("Ed25519 signature must contain exactly 64 bytes".into());
    }
    let payload_bytes = serde_json::to_vec(&envelope.payload)
        .map_err(|error| format!("cannot canonicalize signed verifier payload: {error}"))?;
    UnparsedPublicKey::new(&ED25519, public_key)
        .verify(&payload_bytes, &signature_bytes)
        .map_err(|_| "signed verifier payload signature is invalid".to_string())?;
    Ok(envelope.payload)
}

fn has_exact_unix_permission_mode(mode: u32, expected: u32) -> bool {
    mode & 0o7777 == expected
}

fn read_nofollow_regular_file(
    path: &Path,
    max_bytes: u64,
    name: &str,
) -> Result<(Vec<u8>, fs::Metadata), String> {
    use std::os::unix::fs::OpenOptionsExt;

    if max_bytes == 0 {
        return Err(format!("{name} size limit must be nonzero"));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK);
    let file = options.open(path).map_err(|error| {
        format!("cannot safely open {name} without following symlinks: {error}")
    })?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("cannot inspect opened {name}: {error}"))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > max_bytes {
        return Err(format!(
            "{name} must be a regular nonempty file no larger than {max_bytes} bytes"
        ));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read opened {name}: {error}"))?;
    if bytes.is_empty() || bytes.len() as u64 > max_bytes {
        return Err(format!("{name} is empty or exceeds its size limit"));
    }
    Ok((bytes, metadata))
}

fn read_owner_private_file(
    path: &Path,
    expected_uid: u32,
    max_bytes: u64,
    name: &str,
) -> Result<Vec<u8>, String> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let (bytes, metadata) = read_nofollow_regular_file(path, max_bytes, name)?;
    if metadata.uid() != expected_uid
        || !has_exact_unix_permission_mode(metadata.permissions().mode(), 0o600)
    {
        return Err(format!(
            "{name} must be owned by the verifier UID and have exact mode 0600"
        ));
    }
    Ok(bytes)
}

fn read_shared_output_file(
    path: &Path,
    expected_uid: u32,
    expected_gid: u32,
) -> Result<Vec<u8>, String> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let (bytes, metadata) =
        read_nofollow_regular_file(path, 64 * 1024, "existing verifier receipt")?;
    if metadata.uid() != expected_uid
        || metadata.gid() != expected_gid
        || !has_exact_unix_permission_mode(metadata.permissions().mode(), 0o640)
    {
        return Err("existing verifier receipt has an unexpected owner, group or mode".into());
    }
    Ok(bytes)
}

fn secure_directory(
    path: &Path,
    name: &str,
    expected_uid: u32,
    expected_gid: u32,
) -> Result<(), String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("cannot inspect {name}: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!(
            "{name} must be an existing regular non-symlink directory"
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.uid() != expected_uid
            || metadata.gid() != expected_gid
            || metadata.permissions().mode() & 0o7777 != 0o2750
        {
            return Err(format!(
                "{name} must be owned by the configured verifier UID/GID and use mode 2750"
            ));
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (expected_uid, expected_gid);
        return Err("separate verifier receipt ownership is supported only on Unix hosts".into());
    }
    Ok(())
}

fn open_create_new(path: &Path, mode: u32) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode);
    }
    #[cfg(not(unix))]
    let _ = mode;
    options
        .open(path)
        .map_err(|error| format!("cannot create protected file {}: {error}", path.display()))
}

fn open_secure_create_new(path: &Path) -> Result<File, String> {
    open_create_new(path, 0o600)
}

fn write_sync_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = open_secure_create_new(path)?;
    file.write_all(bytes)
        .map_err(|error| format!("cannot write protected file {}: {error}", path.display()))?;
    file.sync_all()
        .map_err(|error| format!("cannot sync protected file {}: {error}", path.display()))
}

fn write_sync_shared_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = open_create_new(path, 0o640)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o640))
            .map_err(|error| format!("cannot set verifier receipt permissions: {error}"))?;
    }
    file.write_all(bytes)
        .map_err(|error| format!("cannot write verifier receipt {}: {error}", path.display()))?;
    file.sync_all()
        .map_err(|error| format!("cannot sync verifier receipt {}: {error}", path.display()))
}

fn sync_directory(path: &Path) -> Result<(), String> {
    let directory = File::open(path)
        .map_err(|error| format!("cannot open receipt directory for sync: {error}"))?;
    directory
        .sync_all()
        .map_err(|error| format!("cannot sync receipt directory: {error}"))
}

/// Publish complete bytes atomically. Existing symlinks and non-regular files
/// are never followed or replaced. The process lock prevents competing
/// verifier instances from racing this writer.
fn validate_safe_filename(filename: &str) -> Result<(), String> {
    if filename.is_empty()
        || filename.len() > 200
        || filename == "."
        || filename == ".."
        || !filename
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
    {
        return Err("verifier output filename contains invalid characters".into());
    }
    Ok(())
}

fn validate_shared_output_file(
    path: &Path,
    expected_uid: u32,
    expected_gid: u32,
) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect verifier output {}: {error}", path.display()))?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() == 0
        || metadata.len() > 64 * 1024
    {
        return Err(
            "existing verifier output is not a regular file within the 64 KiB limit".into(),
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.uid() != expected_uid
            || metadata.gid() != expected_gid
            || !has_exact_unix_permission_mode(metadata.permissions().mode(), 0o640)
        {
            return Err("existing verifier output has an unexpected owner, group or mode".into());
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (expected_uid, expected_gid);
        return Err("shared verifier receipts are supported only on Unix hosts".into());
    }
    Ok(())
}

fn atomic_publish(
    directory: &Path,
    filename: &str,
    bytes: &[u8],
    allow_replace: bool,
    expected_uid: u32,
    expected_gid: u32,
) -> Result<(), String> {
    validate_safe_filename(filename)?;
    let destination = directory.join(filename);
    let temporary = directory.join(format!(
        ".{filename}.tmp.{}.{}",
        std::process::id(),
        Uuid::new_v4()
    ));
    write_sync_shared_file(&temporary, bytes)?;

    let result = match fs::symlink_metadata(&destination) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                Err(format!(
                    "refusing to replace non-regular verifier output {}",
                    destination.display()
                ))
            } else if !allow_replace {
                Err(format!(
                    "verifier output already exists and replacement is disabled: {}",
                    destination.display()
                ))
            } else {
                validate_shared_output_file(&destination, expected_uid, expected_gid)?;
                fs::rename(&temporary, &destination)
                    .map_err(|error| format!("cannot atomically replace verifier output: {error}"))
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::hard_link(&temporary, &destination)
                .map_err(|error| format!("cannot atomically publish verifier output: {error}"))
        }
        Err(error) => Err(format!("cannot inspect verifier output path: {error}")),
    };

    let cleanup_result = fs::remove_file(&temporary)
        .map_err(|error| format!("cannot remove verifier temporary file: {error}"));
    result?;
    validate_shared_output_file(&destination, expected_uid, expected_gid)?;
    cleanup_result?;
    sync_directory(directory)
}

fn acquire_process_lock(receipt_dir: &Path) -> Result<ProcessLock, String> {
    let path = receipt_dir.join(".dread-verifier.lock");
    let file = open_secure_create_new(&path).map_err(|error| {
        format!(
            "another verifier may already be running, or a stale lock remains at {}: {error}",
            path.display()
        )
    })?;
    Ok(ProcessLock { path, _file: file })
}

fn load_private_key(path: &Path, expected_uid: u32) -> Result<Ed25519KeyPair, String> {
    let encoded = read_owner_private_file(
        path,
        expected_uid,
        MAX_PRIVATE_KEY_BYTES,
        "independent verifier private key",
    )?;
    Ed25519KeyPair::from_pkcs8(&encoded).map_err(|_| {
        "independent verifier private key is not a valid Ed25519 PKCS#8 document".into()
    })
}

fn load_policy(path: &Path, expected_uid: u32) -> Result<(VerificationPolicy, String), String> {
    let encoded = read_owner_private_file(
        path,
        expected_uid,
        MAX_POLICY_BYTES,
        "independent verifier policy",
    )?;
    let policy: VerificationPolicy = serde_json::from_slice(&encoded)
        .map_err(|error| format!("independent verifier policy JSON is invalid: {error}"))?;
    if policy.schema_version != 1 {
        return Err("unsupported independent verifier policy schema version".into());
    }
    validate_safe_label("policy_id", &policy.policy_id, 128)?;
    if policy.rules.is_empty() {
        return Err("independent verifier policy must contain at least one capability rule".into());
    }
    let mut seen = BTreeMap::new();
    for rule in &policy.rules {
        validate_text("capability_id", &rule.capability_id, 128)?;
        validate_safe_label("rule_id", &rule.rule_id, 128)?;
        if rule.max_result_bytes == 0 || rule.max_result_bytes > MAX_RESULT_BYTES {
            return Err("capability rule max_result_bytes must be between 1 byte and 4 MiB".into());
        }
        if rule.required_json_pointers.is_empty() {
            return Err("every capability rule must require at least one JSON Pointer".into());
        }
        for pointer in rule
            .required_json_pointers
            .iter()
            .chain(rule.forbidden_json_pointers.iter())
        {
            if !valid_json_pointer(pointer) {
                return Err(format!("invalid JSON Pointer in rule {}", rule.rule_id));
            }
        }
        if seen.insert(rule.capability_id.as_str(), ()).is_some() {
            return Err(format!(
                "policy contains duplicate capability rule for {}",
                rule.capability_id
            ));
        }
    }
    Ok((policy, sha256_hex(&encoded)))
}

fn valid_json_pointer(pointer: &str) -> bool {
    if pointer.is_empty() || !pointer.starts_with('/') {
        return false;
    }
    let bytes = pointer.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'~' {
            if index + 1 >= bytes.len() || !matches!(bytes[index + 1], b'0' | b'1') {
                return false;
            }
            index += 2;
        } else {
            index += 1;
        }
    }
    true
}

fn evaluate_result(attempt: &PendingAttempt, policy: &VerificationPolicy) -> ResultDecision {
    let Some(rule) = policy
        .rules
        .iter()
        .find(|rule| rule.capability_id == attempt.capability_id)
    else {
        return ResultDecision {
            passed: false,
            rule_id: "no-matching-rule".into(),
            reason_code: "capability_not_covered",
        };
    };

    if attempt.provider_error.is_some() {
        return ResultDecision {
            passed: false,
            rule_id: rule.rule_id.clone(),
            reason_code: "provider_error_present",
        };
    }
    let Some(result_json) = attempt.result_json.as_deref() else {
        return ResultDecision {
            passed: false,
            rule_id: rule.rule_id.clone(),
            reason_code: "result_missing",
        };
    };
    if result_json.len() > rule.max_result_bytes {
        return ResultDecision {
            passed: false,
            rule_id: rule.rule_id.clone(),
            reason_code: "result_exceeds_policy_size",
        };
    }
    let result: Value = match serde_json::from_str(result_json) {
        Ok(result) => result,
        Err(_) => {
            return ResultDecision {
                passed: false,
                rule_id: rule.rule_id.clone(),
                reason_code: "result_invalid_json",
            };
        }
    };
    let Some(object) = result.as_object() else {
        return ResultDecision {
            passed: false,
            rule_id: rule.rule_id.clone(),
            reason_code: "result_not_object",
        };
    };
    if object.get("success") == Some(&Value::Bool(false))
        || object.get("ok") == Some(&Value::Bool(false))
        || object.get("error").is_some_and(|value| !value.is_null())
    {
        return ResultDecision {
            passed: false,
            rule_id: rule.rule_id.clone(),
            reason_code: "result_reports_failure",
        };
    }
    if rule
        .required_json_pointers
        .iter()
        .any(|pointer| result.pointer(pointer).is_none_or(Value::is_null))
    {
        return ResultDecision {
            passed: false,
            rule_id: rule.rule_id.clone(),
            reason_code: "required_result_field_missing",
        };
    }
    if rule
        .forbidden_json_pointers
        .iter()
        .any(|pointer| result.pointer(pointer).is_some())
    {
        return ResultDecision {
            passed: false,
            rule_id: rule.rule_id.clone(),
            reason_code: "forbidden_result_field_present",
        };
    }
    ResultDecision {
        passed: true,
        rule_id: rule.rule_id.clone(),
        reason_code: "policy_passed",
    }
}

fn validate_database_path(path: &Path) -> Result<PathBuf, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect canonical DREAD database: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("canonical DREAD database path must be a regular non-symlink file".into());
    }
    fs::canonicalize(path)
        .map_err(|error| format!("cannot resolve canonical DREAD database path: {error}"))
}

fn open_read_only_database(path: &Path) -> Result<Connection, String> {
    let canonical_path = validate_database_path(path)?;
    let connection = Connection::open_with_flags(
        canonical_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| format!("cannot open DREAD database read-only: {error}"))?;
    connection
        .busy_timeout(Duration::from_secs(3))
        .map_err(|error| format!("cannot configure SQLite busy timeout: {error}"))?;
    connection
        .execute_batch("PRAGMA query_only = ON; PRAGMA trusted_schema = OFF;")
        .map_err(|error| format!("cannot enforce SQLite read-only mode: {error}"))?;
    let migration_version: Option<i64> = connection
        .query_row(
            "SELECT MAX(version) FROM dread_execution_schema_migrations",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| format!("cannot inspect DREAD execution schema version: {error}"))?
        .flatten();
    if migration_version.is_none_or(|version| version < 7) {
        return Err(
            "canonical DREAD database must have durable execution schema version 7 or newer".into(),
        );
    }
    // Prepare-only schema validation ensures the verifier is reading the
    // canonical Phase 6 columns and not a similarly named unrelated database.
    connection
        .prepare(
            "SELECT request_id, attempt_id, task_id, worker_id, provider_id,
                    capability_id, provider_result_json, provider_error, dispatch_state
             FROM dread_task_attempts LIMIT 0",
        )
        .map_err(|error| format!("canonical attempt schema is unavailable: {error}"))?;
    connection
        .prepare("SELECT request_id, requester_id FROM dread_dispatch_admissions LIMIT 0")
        .map_err(|error| format!("canonical admission schema is unavailable: {error}"))?;
    connection
        .prepare(
            "SELECT attempt_id, evidence_id, evidence_kind, verifier_id, observed_at
                  FROM dread_execution_evidence LIMIT 0",
        )
        .map_err(|error| format!("canonical provider evidence schema is unavailable: {error}"))?;
    Ok(connection)
}

fn row_to_pending(row: &Row<'_>) -> rusqlite::Result<PendingAttempt> {
    Ok(PendingAttempt {
        request_id: row.get(0)?,
        attempt_id: row.get(1)?,
        task_id: row.get(2)?,
        worker_id: row.get(3)?,
        provider_id: row.get(4)?,
        capability_id: row.get(5)?,
        requester_id: row.get(6)?,
        provider_verifier_id: row.get(7)?,
        result_json: row.get(8)?,
        provider_error: row.get(9)?,
    })
}

fn load_pending_attempts(connection: &Connection) -> Result<Vec<PendingAttempt>, String> {
    let mut statement = connection
        .prepare(
            "SELECT a.request_id, a.attempt_id, a.task_id, a.worker_id, a.provider_id,
                    a.capability_id, d.requester_id,
                    (SELECT e.verifier_id
                     FROM dread_execution_evidence e
                     WHERE e.attempt_id = a.attempt_id AND e.evidence_kind = 'succeeded'
                     ORDER BY e.observed_at DESC, e.evidence_id ASC LIMIT 1),
                    a.provider_result_json, a.provider_error
             FROM dread_task_attempts a
             INNER JOIN dread_dispatch_admissions d ON d.request_id = a.request_id
             WHERE a.dispatch_state = 'provider_succeeded'
             ORDER BY a.dispatched_at, a.request_id",
        )
        .map_err(|error| format!("cannot prepare pending verifier query: {error}"))?;
    let pending = statement
        .query_map([], row_to_pending)
        .map_err(|error| format!("cannot query pending verifier results: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("cannot read pending verifier result row: {error}"))?;
    Ok(pending)
}

fn build_result_receipt(
    attempt: &PendingAttempt,
    policy: &VerificationPolicy,
    policy_digest: &str,
    verifier_id: &str,
    now: i64,
) -> Result<IndependentResultReceipt, String> {
    validate_safe_component("request_id", &attempt.request_id, 128)?;
    validate_text("attempt_id", &attempt.attempt_id, 160)?;
    validate_text("task_id", &attempt.task_id, 160)?;
    validate_text("worker_id", &attempt.worker_id, 512)?;
    validate_text("provider_id", &attempt.provider_id, 128)?;
    validate_text("capability_id", &attempt.capability_id, 128)?;
    validate_text("requester_id", &attempt.requester_id, 512)?;
    validate_text("provider_verifier_id", &attempt.provider_verifier_id, 512)?;
    if verifier_id == attempt.requester_id
        || verifier_id == attempt.worker_id
        || verifier_id == attempt.provider_verifier_id
    {
        return Err("independent verifier identity must be distinct from requester, worker and provider attestor".into());
    }
    let decision = evaluate_result(attempt, policy);
    let result_sha256 = attempt
        .result_json
        .as_deref()
        .map(|value| sha256_hex(value.as_bytes()));
    let error_sha256 = attempt
        .provider_error
        .as_deref()
        .map(|value| sha256_hex(value.as_bytes()));
    let policy_rule = if decision.rule_id == "no-matching-rule" {
        "no-matching-rule"
    } else {
        decision.rule_id.as_str()
    };
    let source_reference = format!(
        "verifier://{verifier_id}/policy/{policy_digest}/capability/{}/rule/{policy_rule}/decision/{}",
        attempt.capability_id, decision.reason_code
    );
    validate_text("verification source reference", &source_reference, 2048)?;
    Ok(IndependentResultReceipt {
        schema_version: 1,
        evidence_id: format!("independent-result:{}", Uuid::new_v4()),
        verifier_id: verifier_id.to_owned(),
        request_id: attempt.request_id.clone(),
        attempt_id: attempt.attempt_id.clone(),
        task_id: attempt.task_id.clone(),
        requester_id: attempt.requester_id.clone(),
        worker_id: attempt.worker_id.clone(),
        provider_verifier_id: attempt.provider_verifier_id.clone(),
        result_sha256,
        error_sha256,
        passed: decision.passed,
        source_reference,
        observed_at: now,
        expires_at: now.saturating_add(RESULT_RECEIPT_TTL_SECONDS),
    })
}

fn existing_receipt_is_current(
    path: &Path,
    key_id: &str,
    public_key: &[u8],
    expected: &IndependentResultReceipt,
    now: i64,
    expected_uid: u32,
    expected_gid: u32,
) -> bool {
    let Ok(bytes) = read_shared_output_file(path, expected_uid, expected_gid) else {
        return false;
    };
    let Ok(receipt) = verify_envelope::<IndependentResultReceipt>(&bytes, key_id, public_key)
    else {
        return false;
    };
    receipt.schema_version == 1
        && receipt.verifier_id == expected.verifier_id
        && receipt.request_id == expected.request_id
        && receipt.attempt_id == expected.attempt_id
        && receipt.task_id == expected.task_id
        && receipt.requester_id == expected.requester_id
        && receipt.worker_id == expected.worker_id
        && receipt.provider_verifier_id == expected.provider_verifier_id
        && receipt.result_sha256 == expected.result_sha256
        && receipt.error_sha256 == expected.error_sha256
        && receipt.passed == expected.passed
        && receipt.source_reference == expected.source_reference
        && receipt.expires_at > now.saturating_add(RECEIPT_REFRESH_MARGIN_SECONDS)
}

fn publish_result_receipt(
    receipt_dir: &Path,
    attempt: &PendingAttempt,
    receipt: &IndependentResultReceipt,
    key_id: &str,
    keypair: &Ed25519KeyPair,
    receipt_uid: u32,
    receipt_gid: u32,
) -> Result<bool, String> {
    let filename = format!("{}.json", attempt.request_id);
    let destination = receipt_dir.join(&filename);
    let now = now_epoch_seconds()?;
    if existing_receipt_is_current(
        &destination,
        key_id,
        keypair.public_key().as_ref(),
        receipt,
        now,
        receipt_uid,
        receipt_gid,
    ) {
        return Ok(false);
    }
    let bytes = sign_payload(keypair, key_id, receipt)?;
    atomic_publish(
        receipt_dir,
        &filename,
        &bytes,
        true,
        receipt_uid,
        receipt_gid,
    )?;
    Ok(true)
}

fn write_heartbeat(
    receipt_dir: &Path,
    verifier_id: &str,
    key_id: &str,
    keypair: &Ed25519KeyPair,
    now: i64,
    receipt_uid: u32,
    receipt_gid: u32,
) -> Result<(), String> {
    let heartbeat = VerifierHeartbeat {
        schema_version: 1,
        verifier_id: verifier_id.to_owned(),
        observed_at: now,
        expires_at: now.saturating_add(HEARTBEAT_TTL_SECONDS),
    };
    let encoded = sign_payload(keypair, key_id, heartbeat)?;
    atomic_publish(
        receipt_dir,
        ".dread-verifier-ready.json",
        &encoded,
        true,
        receipt_uid,
        receipt_gid,
    )
}

fn run_cycle(
    connection: &Connection,
    output: &VerifierOutput<'_>,
    policy: &VerificationPolicy,
    policy_digest: &str,
) -> Result<(usize, usize), String> {
    let pending = load_pending_attempts(connection)?;
    let mut created_or_refreshed = 0usize;
    for attempt in &pending {
        let now = now_epoch_seconds()?;
        let receipt =
            build_result_receipt(attempt, policy, policy_digest, output.verifier_id, now)?;
        if publish_result_receipt(
            output.receipt_dir,
            attempt,
            &receipt,
            output.key_id,
            output.keypair,
            output.receipt_uid,
            output.receipt_gid,
        )? {
            created_or_refreshed = created_or_refreshed.saturating_add(1);
        }
    }
    let now = now_epoch_seconds()?;
    write_heartbeat(
        output.receipt_dir,
        output.verifier_id,
        output.key_id,
        output.keypair,
        now,
        output.receipt_uid,
        output.receipt_gid,
    )?;
    Ok((pending.len(), created_or_refreshed))
}

fn parse_config() -> Result<VerifierConfig, String> {
    let database_path = env::var_os("DREAD_VERIFIER_DB_PATH")
        .map(PathBuf::from)
        .ok_or_else(|| {
            "DREAD_VERIFIER_DB_PATH is required (use the canonical DREAD_DB_PATH)".to_string()
        })?;
    let receipt_dir = env::var_os("DREAD_INDEPENDENT_RESULT_VERIFIER_RECEIPT_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| "DREAD_INDEPENDENT_RESULT_VERIFIER_RECEIPT_DIR is required".to_string())?;
    let verifier_id = env::var("DREAD_INDEPENDENT_RESULT_VERIFIER_ID")
        .map_err(|_| "DREAD_INDEPENDENT_RESULT_VERIFIER_ID is required".to_string())?;
    let key_id = env::var("DREAD_INDEPENDENT_RESULT_VERIFIER_KEY_ID")
        .map_err(|_| "DREAD_INDEPENDENT_RESULT_VERIFIER_KEY_ID is required".to_string())?;
    let private_key_path = env::var_os("DREAD_INDEPENDENT_RESULT_VERIFIER_PRIVATE_KEY_FILE")
        .map(PathBuf::from)
        .ok_or_else(|| {
            "DREAD_INDEPENDENT_RESULT_VERIFIER_PRIVATE_KEY_FILE is required".to_string()
        })?;
    let policy_path = env::var_os("DREAD_INDEPENDENT_RESULT_VERIFIER_POLICY_FILE")
        .map(PathBuf::from)
        .ok_or_else(|| "DREAD_INDEPENDENT_RESULT_VERIFIER_POLICY_FILE is required".to_string())?;
    let receipt_uid = env::var("DREAD_INDEPENDENT_RESULT_VERIFIER_RECEIPT_UID")
        .map_err(|_| "DREAD_INDEPENDENT_RESULT_VERIFIER_RECEIPT_UID is required".to_string())?
        .parse::<u32>()
        .map_err(|_| "receipt UID must be numeric".to_string())?;
    let receipt_gid = env::var("DREAD_INDEPENDENT_RESULT_VERIFIER_RECEIPT_GID")
        .map_err(|_| "DREAD_INDEPENDENT_RESULT_VERIFIER_RECEIPT_GID is required".to_string())?
        .parse::<u32>()
        .map_err(|_| "receipt GID must be numeric".to_string())?;
    let poll_seconds = env::var("DREAD_VERIFIER_POLL_INTERVAL_SECONDS")
        .ok()
        .map(|value| value.parse::<u64>())
        .transpose()
        .map_err(|_| "DREAD_VERIFIER_POLL_INTERVAL_SECONDS must be an integer".to_string())?
        .unwrap_or(5);
    if !(1..=60).contains(&poll_seconds) {
        return Err("DREAD_VERIFIER_POLL_INTERVAL_SECONDS must be between 1 and 60".into());
    }
    validate_text("independent verifier ID", &verifier_id, 160)?;
    validate_safe_label("independent verifier ID", &verifier_id, 160)?;
    validate_safe_label("independent verifier key ID", &key_id, 128)?;
    secure_directory(
        &receipt_dir,
        "independent verifier receipt directory",
        receipt_uid,
        receipt_gid,
    )?;
    Ok(VerifierConfig {
        database_path,
        receipt_dir,
        verifier_id,
        key_id,
        private_key_path,
        policy_path,
        receipt_uid,
        receipt_gid,
        poll_interval: Duration::from_secs(poll_seconds),
    })
}

fn check_public_key_environment(keypair: &Ed25519KeyPair) -> Result<(), String> {
    let configured = env::var("DREAD_INDEPENDENT_RESULT_VERIFIER_PUBLIC_KEY_HEX")
        .map_err(|_| "DREAD_INDEPENDENT_RESULT_VERIFIER_PUBLIC_KEY_HEX is required".to_string())?;
    let decoded = decode_hex(&configured)?;
    if decoded.as_slice() != keypair.public_key().as_ref() {
        return Err("configured verifier public key does not match the private PKCS#8 key".into());
    }
    Ok(())
}

fn run_daemon(config: VerifierConfig, once: bool) -> Result<(), String> {
    let _lock = acquire_process_lock(&config.receipt_dir)?;
    let keypair = load_private_key(&config.private_key_path, config.receipt_uid)?;
    check_public_key_environment(&keypair)?;
    let (policy, policy_digest) = load_policy(&config.policy_path, config.receipt_uid)?;
    let connection = open_read_only_database(&config.database_path)?;
    let output = VerifierOutput {
        receipt_dir: &config.receipt_dir,
        verifier_id: &config.verifier_id,
        key_id: &config.key_id,
        keypair: &keypair,
        receipt_uid: config.receipt_uid,
        receipt_gid: config.receipt_gid,
    };
    eprintln!(
        "DREAD independent verifier ready: id={} policy={} database={} poll={}s",
        config.verifier_id,
        policy.policy_id,
        config.database_path.display(),
        config.poll_interval.as_secs()
    );
    loop {
        match run_cycle(&connection, &output, &policy, &policy_digest) {
            Ok((pending, receipts)) => {
                eprintln!(
                    "verifier cycle complete: pending={} receipts_created_or_refreshed={}",
                    pending, receipts
                );
            }
            Err(error) => {
                eprintln!("verifier cycle failed; readiness heartbeat not refreshed: {error}");
            }
        }
        if once {
            return Ok(());
        }
        thread::sleep(config.poll_interval);
    }
}

fn generate_key(path: &Path) -> Result<(), String> {
    if path.exists() {
        return Err(format!(
            "refusing to overwrite existing key file {}",
            path.display()
        ));
    }
    let document = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).map_err(|_| {
        "operating-system random source could not generate an Ed25519 key".to_string()
    })?;
    let keypair = Ed25519KeyPair::from_pkcs8(document.as_ref())
        .map_err(|_| "generated Ed25519 PKCS#8 key could not be loaded".to_string())?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if !parent.is_dir() {
        return Err(format!(
            "key parent directory does not exist: {}",
            parent.display()
        ));
    }
    write_sync_file(path, document.as_ref())?;
    sync_directory(parent)?;
    println!(
        "DREAD_INDEPENDENT_RESULT_VERIFIER_PUBLIC_KEY_HEX={}",
        hex_encode(keypair.public_key().as_ref())
    );
    Ok(())
}

fn current_effective_uid() -> Result<u32, String> {
    #[cfg(unix)]
    {
        // SAFETY: geteuid has no pointer arguments or side effects and returns
        // the effective UID of the current process.
        Ok(unsafe { libc::geteuid() })
    }
    #[cfg(not(unix))]
    {
        Err("verifier private-key ownership is supported only on Unix hosts".into())
    }
}

fn print_public_key(path: &Path) -> Result<(), String> {
    let keypair = load_private_key(path, current_effective_uid()?)?;
    println!("{}", hex_encode(keypair.public_key().as_ref()));
    Ok(())
}

fn usage() -> &'static str {
    "Usage:
  dread-result-verifier keygen <private-key.pk8>
  dread-result-verifier public-key <private-key.pk8>
  dread-result-verifier run [--once]

Run requires:
  DREAD_VERIFIER_DB_PATH
  DREAD_INDEPENDENT_RESULT_VERIFIER_RECEIPT_DIR
  DREAD_INDEPENDENT_RESULT_VERIFIER_ID
  DREAD_INDEPENDENT_RESULT_VERIFIER_KEY_ID
  DREAD_INDEPENDENT_RESULT_VERIFIER_PRIVATE_KEY_FILE
  DREAD_INDEPENDENT_RESULT_VERIFIER_PUBLIC_KEY_HEX
  DREAD_INDEPENDENT_RESULT_VERIFIER_POLICY_FILE
  DREAD_VERIFIER_POLL_INTERVAL_SECONDS (optional, default 5)"
}

fn main() {
    if let Err(error) = run_cli() {
        eprintln!("dread-result-verifier: {error}");
        std::process::exit(1);
    }
}

fn run_cli() -> Result<(), String> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("keygen") => {
            let path = args.next().ok_or_else(|| usage().to_string())?;
            if args.next().is_some() {
                return Err(usage().into());
            }
            generate_key(Path::new(&path))
        }
        Some("public-key") => {
            let path = args.next().ok_or_else(|| usage().to_string())?;
            if args.next().is_some() {
                return Err(usage().into());
            }
            print_public_key(Path::new(&path))
        }
        Some("run") => {
            let once = match args.next().as_deref() {
                None => false,
                Some("--once") => true,
                Some(_) => return Err(usage().into()),
            };
            if args.next().is_some() {
                return Err(usage().into());
            }
            let config = parse_config()?;
            run_daemon(config, once)
        }
        _ => Err(usage().into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::KeyPair;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    fn keypair() -> Ed25519KeyPair {
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap()
    }

    fn temp_dir() -> PathBuf {
        let path = env::temp_dir().join(format!(
            "dread-result-verifier-test-{}-{}",
            std::process::id(),
            Uuid::new_v4()
        ));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    fn policy() -> VerificationPolicy {
        VerificationPolicy {
            schema_version: 1,
            policy_id: "test-policy-v1".into(),
            rules: vec![CapabilityRule {
                capability_id: "hermes.run".into(),
                rule_id: "hermes-run-v1".into(),
                max_result_bytes: 64 * 1024,
                required_json_pointers: vec!["/result".into()],
                forbidden_json_pointers: vec!["/debug/secret".into()],
            }],
        }
    }

    fn pending(result: Option<&str>, error: Option<&str>) -> PendingAttempt {
        PendingAttempt {
            request_id: "request-1".into(),
            attempt_id: "attempt-1".into(),
            task_id: "task-1".into(),
            worker_id: "worker-1".into(),
            provider_id: "hermes".into(),
            capability_id: "hermes.run".into(),
            requester_id: "operator-1".into(),
            provider_verifier_id: "dread-adapter-receipt:hermes".into(),
            result_json: result.map(str::to_owned),
            provider_error: error.map(str::to_owned),
        }
    }

    #[test]
    fn exact_permission_check_rejects_special_mode_bits() {
        assert!(has_exact_unix_permission_mode(0o600, 0o600));
        assert!(!has_exact_unix_permission_mode(0o4600, 0o600));
        assert!(!has_exact_unix_permission_mode(0o2600, 0o600));
        assert!(!has_exact_unix_permission_mode(0o1600, 0o600));
        assert!(!has_exact_unix_permission_mode(0o640, 0o600));
        assert!(has_exact_unix_permission_mode(0o640, 0o640));
        assert!(!has_exact_unix_permission_mode(0o4640, 0o640));
        assert!(!has_exact_unix_permission_mode(0o2640, 0o640));
        assert!(!has_exact_unix_permission_mode(0o1640, 0o640));
    }

    #[test]
    fn capability_policy_requires_declared_non_null_result_paths() {
        let policy = policy();
        assert!(
            evaluate_result(&pending(Some(r#"{"result":{"answer":42}}"#), None), &policy).passed
        );
        assert_eq!(
            evaluate_result(&pending(Some(r#"{"other":42}"#), None), &policy).reason_code,
            "required_result_field_missing"
        );
        assert_eq!(
            evaluate_result(&pending(Some(r#"{"result":null}"#), None), &policy).reason_code,
            "required_result_field_missing"
        );
    }

    #[test]
    fn unknown_capability_and_provider_error_are_negative_decisions() {
        let mut unknown = pending(Some(r#"{"result":"ok"}"#), None);
        unknown.capability_id = "unregistered.capability".into();
        assert!(!evaluate_result(&unknown, &policy()).passed);
        assert!(
            !evaluate_result(
                &pending(Some(r#"{"result":"ok"}"#), Some("oops")),
                &policy()
            )
            .passed
        );
    }

    #[test]
    fn generic_failure_shapes_cannot_pass_even_when_required_fields_exist() {
        let policy = policy();
        assert!(
            !evaluate_result(
                &pending(Some(r#"{"result":"ok","success":false}"#), None),
                &policy
            )
            .passed
        );
        assert!(
            !evaluate_result(
                &pending(Some(r#"{"result":"ok","error":"failed"}"#), None),
                &policy
            )
            .passed
        );
    }

    #[test]
    fn signed_heartbeat_matches_api_payload_contract() {
        let key = keypair();
        let now = 1_800_000_000;
        let payload = VerifierHeartbeat {
            schema_version: 1,
            verifier_id: "dread-independent-result-verifier".into(),
            observed_at: now,
            expires_at: now + HEARTBEAT_TTL_SECONDS,
        };
        let bytes = sign_payload(&key, "verifier-key-1", payload.clone()).unwrap();
        let verified: VerifierHeartbeat =
            verify_envelope(&bytes, "verifier-key-1", key.public_key().as_ref()).unwrap();
        assert_eq!(verified, payload);
    }

    #[test]
    fn signed_result_receipt_matches_api_payload_contract_and_exact_digests() {
        let key = keypair();
        let attempt = pending(Some(r#"{"result":{"answer":42}}"#), None);
        let receipt = build_result_receipt(
            &attempt,
            &policy(),
            "abcdef",
            "dread-independent-result-verifier",
            1_800_000_000,
        )
        .unwrap();
        assert!(receipt.passed);
        assert_eq!(
            receipt.result_sha256.as_deref(),
            Some(sha256_hex(attempt.result_json.as_deref().unwrap().as_bytes()).as_str())
        );
        let encoded = sign_payload(&key, "verifier-key-1", receipt.clone()).unwrap();
        let decoded: IndependentResultReceipt =
            verify_envelope(&encoded, "verifier-key-1", key.public_key().as_ref()).unwrap();
        assert_eq!(decoded, receipt);
    }

    #[test]
    fn pending_query_reads_only_provider_succeeded_rows_and_canonical_identity() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(
            r#"CREATE TABLE dread_task_attempts (
                request_id TEXT, attempt_id TEXT, task_id TEXT, worker_id TEXT,
                provider_id TEXT, capability_id TEXT, provider_result_json TEXT,
                provider_error TEXT, dispatch_state TEXT, dispatched_at INTEGER
             );
             CREATE TABLE dread_dispatch_admissions(request_id TEXT, requester_id TEXT);
             CREATE TABLE dread_execution_evidence(
                evidence_id TEXT, attempt_id TEXT, evidence_kind TEXT,
                verifier_id TEXT, observed_at INTEGER
             );
             INSERT INTO dread_task_attempts VALUES
                ('request-1','attempt-1','task-1','worker-1','hermes','hermes.run','{"result":42}',NULL,'provider_succeeded',10),
                ('request-2','attempt-2','task-2','worker-2','hermes','hermes.run','{"result":42}',NULL,'running',11);
             INSERT INTO dread_dispatch_admissions VALUES ('request-1','operator-1'),('request-2','operator-2');
             INSERT INTO dread_execution_evidence VALUES ('evidence-1','attempt-1','succeeded','dread-adapter-receipt:hermes',10);"#
        ).unwrap();
        let pending = load_pending_attempts(&connection).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].request_id, "request-1");
        assert_eq!(pending[0].requester_id, "operator-1");
        assert_eq!(
            pending[0].provider_verifier_id,
            "dread-adapter-receipt:hermes"
        );
    }

    #[test]
    fn canonical_database_is_opened_read_only_and_query_only() {
        let directory = temp_dir();
        let database_path = directory.join("canonical-phase6.sqlite");
        let writable = Connection::open(&database_path).unwrap();
        writable
            .execute_batch(
                "CREATE TABLE dread_execution_schema_migrations(version INTEGER NOT NULL);
                 INSERT INTO dread_execution_schema_migrations(version) VALUES (7);
                 CREATE TABLE dread_task_attempts(
                    request_id TEXT, attempt_id TEXT, task_id TEXT, worker_id TEXT,
                    provider_id TEXT, capability_id TEXT, provider_result_json TEXT,
                    provider_error TEXT, dispatch_state TEXT
                 );
                 CREATE TABLE dread_dispatch_admissions(request_id TEXT, requester_id TEXT);
                 CREATE TABLE dread_execution_evidence(
                    attempt_id TEXT, evidence_id TEXT, evidence_kind TEXT,
                    verifier_id TEXT, observed_at INTEGER
                 );",
            )
            .unwrap();
        drop(writable);

        let readonly = open_read_only_database(&database_path).unwrap();
        let query_only: i64 = readonly
            .query_row("PRAGMA query_only", [], |row| row.get(0))
            .unwrap();
        assert_eq!(query_only, 1);
        assert!(
            readonly
                .execute(
                    "INSERT INTO dread_task_attempts(request_id) VALUES ('must-not-write')",
                    []
                )
                .is_err()
        );
        assert!(
            readonly
                .execute_batch("CREATE TABLE must_not_be_created(id INTEGER)")
                .is_err()
        );
        drop(readonly);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn atomic_publication_refuses_symlink_and_preserves_existing_file_without_replace() {
        let directory = temp_dir();
        let metadata = fs::metadata(&directory).unwrap();
        let uid = metadata.uid();
        let gid = metadata.gid();
        atomic_publish(&directory, "sample.json", b"first", false, uid, gid).unwrap();
        assert!(atomic_publish(&directory, "sample.json", b"second", false, uid, gid).is_err());
        assert_eq!(fs::read(directory.join("sample.json")).unwrap(), b"first");
        let output = fs::metadata(directory.join("sample.json")).unwrap();
        assert_eq!(output.uid(), uid);
        assert_eq!(output.gid(), gid);
        assert_eq!(output.permissions().mode() & 0o7777, 0o640);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn private_key_generation_is_create_once_and_owner_only() {
        let directory = temp_dir();
        let path = directory.join("verifier.pk8");
        generate_key(&path).unwrap();
        let owner_uid = fs::metadata(&directory).unwrap().uid();
        assert!(load_private_key(&path, owner_uid).is_ok());
        assert!(generate_key(&path).is_err());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn private_key_reader_rejects_symlink_and_checks_opened_owner() {
        use std::os::unix::fs::symlink;

        let directory = temp_dir();
        let owner_uid = fs::metadata(&directory).unwrap().uid();
        let target = directory.join("private-key.pk8");
        let link = directory.join("private-key-link.pk8");
        write_sync_file(&target, b"not-a-real-pkcs8-key").unwrap();
        assert!(read_owner_private_file(&target, owner_uid, 4096, "test private key").is_ok());
        symlink(&target, &link).unwrap();
        assert!(read_owner_private_file(&link, owner_uid, 4096, "test private key").is_err());
        assert!(
            read_owner_private_file(
                &target,
                owner_uid.saturating_add(1),
                4096,
                "test private key"
            )
            .is_err()
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn existing_receipt_reader_rejects_symlink_and_checks_opened_metadata() {
        use std::os::unix::fs::symlink;

        let directory = temp_dir();
        let directory_metadata = fs::metadata(&directory).unwrap();
        let expected_uid = directory_metadata.uid();
        let expected_gid = directory_metadata.gid();
        let target = directory.join("receipt-target.json");
        let link = directory.join("receipt-link.json");
        write_sync_shared_file(&target, b"not-a-signed-receipt").unwrap();
        assert!(read_shared_output_file(&target, expected_uid, expected_gid).is_ok());
        symlink(&target, &link).unwrap();
        assert!(read_shared_output_file(&link, expected_uid, expected_gid).is_err());
        assert!(
            read_shared_output_file(&target, expected_uid.saturating_add(1), expected_gid).is_err()
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn json_pointer_validation_rejects_invalid_escape_sequences() {
        assert!(valid_json_pointer("/result/value"));
        assert!(valid_json_pointer("/a~0b/~1c"));
        assert!(!valid_json_pointer("result"));
        assert!(!valid_json_pointer("/bad~2escape"));
    }

    #[test]
    fn verifier_identity_must_differ_from_all_execution_actors() {
        let mut attempt = pending(Some(r#"{"result":"ok"}"#), None);
        attempt.worker_id = "dread-independent-result-verifier".into();
        assert!(
            build_result_receipt(
                &attempt,
                &policy(),
                "abcdef",
                "dread-independent-result-verifier",
                1_800_000_000
            )
            .is_err()
        );
    }
}
