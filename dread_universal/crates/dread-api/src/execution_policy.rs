//! Signed, request-scoped execution policy for provider dispatch.
//!
//! This policy is independent from task approval and provider registration.
//! A signed policy bundle defines who may execute which capability, operation,
//! task/mission and scope. Every admission re-evaluates the current bundle;
//! absent, expired, ambiguous, revoked-by-replacement, or denied policy fails closed.

use ring::{digest, signature};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::HashSet, env, fs, path::PathBuf};

const MAX_POLICY_BYTES: u64 = 1_048_576;
const MAX_POLICY_LIFETIME_SECONDS: i64 = 30 * 24 * 60 * 60;
const MAX_RULES: usize = 512;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedExecutionPolicyEnvelope {
    pub key_id: String,
    pub payload: ExecutionPolicyManifest,
    pub signature_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionPolicyManifest {
    pub schema_version: u32,
    pub policy_id: String,
    pub revision: i64,
    pub verifier_id: String,
    pub source_reference: String,
    pub observed_at: i64,
    pub expires_at: i64,
    pub rules: Vec<ExecutionPolicyRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionPolicyRule {
    pub rule_id: String,
    pub requester_id: String,
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub mission_id: Option<String>,
    pub capability_id: String,
    pub operation: String,
    pub permission_scope: String,
    pub effect: ExecutionPolicyEffect,
    pub issued_at: i64,
    pub expires_at: i64,
    pub max_attempts: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionPolicyEffect {
    Allow,
    Deny,
}

#[derive(Debug, Clone, Copy)]
pub struct ExecutionPolicyInput<'a> {
    pub request_id: &'a str,
    pub requester_id: &'a str,
    pub task_id: &'a str,
    pub mission_id: Option<&'a str>,
    pub capability_id: &'a str,
    pub operation: &'a str,
    pub permission_scope: &'a str,
    pub attempt_number: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionPolicyDecision {
    pub policy_id: String,
    pub policy_revision: i64,
    pub rule_id: String,
    pub decision_id: String,
    pub expires_at: i64,
}

#[derive(Debug, Clone)]
struct VerifiedExecutionPolicy {
    envelope: SignedExecutionPolicyEnvelope,
    payload_sha256: String,
}

impl VerifiedExecutionPolicy {
    fn verify(encoded: &[u8], public_key_hex: &str, now: i64) -> Result<Self, String> {
        if encoded.is_empty() || encoded.len() as u64 > MAX_POLICY_BYTES {
            return Err("execution policy manifest is empty or exceeds 1 MiB".into());
        }
        let envelope: SignedExecutionPolicyEnvelope = serde_json::from_slice(encoded)
            .map_err(|error| format!("execution policy manifest JSON is invalid: {error}"))?;
        validate_text("policy key_id", &envelope.key_id, 128)?;
        let public_key = decode_hex(public_key_hex)?;
        let signature_bytes = decode_hex(&envelope.signature_hex)?;
        if public_key.len() != 32 || signature_bytes.len() != 64 {
            return Err("execution policy Ed25519 key or signature has an invalid length".into());
        }

        let payload_bytes = serde_json::to_vec(&envelope.payload)
            .map_err(|error| format!("cannot canonicalize execution policy: {error}"))?;
        signature::UnparsedPublicKey::new(&signature::ED25519, &public_key)
            .verify(&payload_bytes, &signature_bytes)
            .map_err(|_| "execution policy signature is invalid".to_string())?;

        validate_policy_payload(&envelope.payload, now)?;
        let payload_sha256 = hex_encode(digest::digest(&digest::SHA256, &payload_bytes).as_ref());
        Ok(Self {
            envelope,
            payload_sha256,
        })
    }

    fn authorize(
        &self,
        input: ExecutionPolicyInput<'_>,
        now: i64,
    ) -> Result<ExecutionPolicyDecision, String> {
        validate_input(&input)?;
        let manifest = &self.envelope.payload;
        if manifest.expires_at <= now {
            return Err("execution policy manifest has expired".into());
        }

        let active_rules = manifest
            .rules
            .iter()
            .filter(|rule| rule.issued_at <= now && rule.expires_at > now)
            .filter(|rule| rule_matches_scope(rule, &input));
        let applicable: Vec<&ExecutionPolicyRule> = active_rules.collect();

        // Deny rules always win, including when an allow rule also matches.
        if applicable
            .iter()
            .any(|rule| rule.effect == ExecutionPolicyEffect::Deny)
        {
            return Err("execution policy contains a matching deny rule".into());
        }

        let allows: Vec<&ExecutionPolicyRule> = applicable
            .into_iter()
            .filter(|rule| {
                rule.effect == ExecutionPolicyEffect::Allow
                    && input.attempt_number <= rule.max_attempts
            })
            .collect();
        if allows.is_empty() {
            return Err("no active execution policy allow rule matches this request".into());
        }
        if allows.len() != 1 {
            return Err("execution policy has ambiguous matching allow rules".into());
        }

        let rule = allows[0];
        let binding = json!({
            "policy_id": manifest.policy_id,
            "policy_revision": manifest.revision,
            "policy_sha256": self.payload_sha256,
            "rule_id": rule.rule_id,
            "request_id": input.request_id,
            "requester_id": input.requester_id,
            "task_id": input.task_id,
            "mission_id": input.mission_id,
            "capability_id": input.capability_id,
            "operation": input.operation,
            "permission_scope": input.permission_scope,
            "attempt_number": input.attempt_number
        });
        let binding_bytes = serde_json::to_vec(&binding)
            .map_err(|error| format!("cannot bind execution policy decision: {error}"))?;
        let binding_sha256 = hex_encode(digest::digest(&digest::SHA256, &binding_bytes).as_ref());
        let decision_id = format!(
            "policy:{}:{}:{}",
            manifest.policy_id, manifest.revision, binding_sha256
        );

        Ok(ExecutionPolicyDecision {
            policy_id: manifest.policy_id.clone(),
            policy_revision: manifest.revision,
            rule_id: rule.rule_id.clone(),
            decision_id,
            expires_at: manifest.expires_at.min(rule.expires_at),
        })
    }

    fn public_status(&self, now: i64) -> Value {
        let manifest = &self.envelope.payload;
        json!({
            "status": if manifest.expires_at > now { "verified" } else { "expired" },
            "policy_id": manifest.policy_id,
            "revision": manifest.revision,
            "verifier_id": manifest.verifier_id,
            "source_reference": manifest.source_reference,
            "observed_at": manifest.observed_at,
            "expires_at": manifest.expires_at,
            "payload_sha256": self.payload_sha256,
            "rule_count": manifest.rules.len(),
            "decision_binding": "requester_task_mission_capability_operation_scope_attempt"
        })
    }
}

/// Reloads and re-verifies the policy file for every decision. Replacing the
/// file with a new signed revision therefore takes effect without an API restart.
pub struct ExecutionPolicyAuthority {
    manifest_path: PathBuf,
    public_key_hex: String,
}

impl ExecutionPolicyAuthority {
    pub fn load_from_env() -> Result<Option<Self>, String> {
        let path = env::var_os("DREAD_EXECUTION_POLICY_MANIFEST");
        let key = env::var("DREAD_EXECUTION_POLICY_PUBLIC_KEY_HEX").ok();
        match (path, key) {
            (None, None) => Ok(None),
            (Some(_), None) | (None, Some(_)) => Err(
                "DREAD_EXECUTION_POLICY_MANIFEST and DREAD_EXECUTION_POLICY_PUBLIC_KEY_HEX must be configured together"
                    .into(),
            ),
            (Some(path), Some(public_key_hex)) => {
                let authority = Self {
                    manifest_path: PathBuf::from(path),
                    public_key_hex,
                };
                let _ = authority.refresh(now_epoch())?;
                Ok(Some(authority))
            }
        }
    }

    pub fn authorize(
        &self,
        input: ExecutionPolicyInput<'_>,
        now: i64,
    ) -> Result<ExecutionPolicyDecision, String> {
        self.refresh(now)?.authorize(input, now)
    }

    pub fn status(&self, now: i64) -> Value {
        match self.refresh(now) {
            Ok(policy) => policy.public_status(now),
            Err(error) => json!({
                "status": "invalid_or_expired",
                "dispatch": "blocked",
                "error": error
            }),
        }
    }

    fn refresh(&self, now: i64) -> Result<VerifiedExecutionPolicy, String> {
        let metadata = fs::symlink_metadata(&self.manifest_path)
            .map_err(|error| format!("cannot inspect execution policy manifest: {error}"))?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() == 0
            || metadata.len() > MAX_POLICY_BYTES
        {
            return Err(
                "execution policy manifest must be a regular non-symlink file no larger than 1 MiB"
                    .into(),
            );
        }
        let encoded = fs::read(&self.manifest_path)
            .map_err(|error| format!("cannot read execution policy manifest: {error}"))?;
        VerifiedExecutionPolicy::verify(&encoded, &self.public_key_hex, now)
    }
}

fn validate_policy_payload(manifest: &ExecutionPolicyManifest, now: i64) -> Result<(), String> {
    if manifest.schema_version != 1
        || manifest.revision <= 0
        || manifest.observed_at < 0
        || manifest.observed_at > now
        || manifest.expires_at <= now
        || manifest.expires_at <= manifest.observed_at
        || manifest.expires_at - manifest.observed_at > MAX_POLICY_LIFETIME_SECONDS
        || manifest.rules.is_empty()
        || manifest.rules.len() > MAX_RULES
    {
        return Err("execution policy is expired, future-dated, or outside bounds".into());
    }
    validate_text("policy_id", &manifest.policy_id, 128)?;
    validate_text("policy_verifier_id", &manifest.verifier_id, 160)?;
    validate_text("policy_source_reference", &manifest.source_reference, 2048)?;

    let mut rule_ids = HashSet::new();
    for rule in &manifest.rules {
        validate_text("policy_rule_id", &rule.rule_id, 160)?;
        validate_text("policy_requester_id", &rule.requester_id, 160)?;
        validate_text("policy_capability_id", &rule.capability_id, 160)?;
        validate_text("policy_operation", &rule.operation, 128)?;
        validate_text("policy_permission_scope", &rule.permission_scope, 128)?;
        if let Some(task_id) = rule.task_id.as_deref() {
            validate_text("policy_task_id", task_id, 512)?;
        }
        if let Some(mission_id) = rule.mission_id.as_deref() {
            validate_text("policy_mission_id", mission_id, 512)?;
        }
        if !rule_ids.insert(rule.rule_id.as_str())
            || !matches!(
                rule.capability_id.as_str(),
                "hermes.run" | "deerflow.run" | "openhands.run"
            )
            || rule.operation != "agent.execute"
            || rule.permission_scope != "agent:execute"
            || rule.issued_at < 0
            || rule.issued_at > manifest.observed_at
            || rule.expires_at <= now
            || rule.expires_at > manifest.expires_at
            || rule.expires_at <= rule.issued_at
            || rule.max_attempts == 0
            || rule.max_attempts > 8
        {
            return Err("execution policy contains a duplicate or invalid rule".into());
        }
    }
    Ok(())
}

fn validate_input(input: &ExecutionPolicyInput<'_>) -> Result<(), String> {
    for (name, value, limit) in [
        ("request_id", input.request_id, 160),
        ("requester_id", input.requester_id, 160),
        ("task_id", input.task_id, 512),
        ("capability_id", input.capability_id, 160),
        ("operation", input.operation, 128),
        ("permission_scope", input.permission_scope, 128),
    ] {
        validate_text(name, value, limit)?;
    }
    if let Some(mission_id) = input.mission_id {
        validate_text("mission_id", mission_id, 512)?;
    }
    if input.attempt_number == 0
        || input.operation != "agent.execute"
        || input.permission_scope != "agent:execute"
    {
        return Err(
            "execution policy input has an unsupported operation, scope, or attempt".into(),
        );
    }
    Ok(())
}

fn rule_matches_scope(rule: &ExecutionPolicyRule, input: &ExecutionPolicyInput<'_>) -> bool {
    rule.requester_id == input.requester_id
        && rule
            .task_id
            .as_deref()
            .is_none_or(|task_id| task_id == input.task_id)
        && rule
            .mission_id
            .as_deref()
            .is_none_or(|mission_id| Some(mission_id) == input.mission_id)
        && rule.capability_id == input.capability_id
        && rule.operation == input.operation
        && rule.permission_scope == input.permission_scope
}

fn validate_text(name: &str, value: &str, limit: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > limit || value.contains('\0') {
        Err(format!("{name} is empty, too long, or contains NUL"))
    } else {
        Ok(())
    }
}

fn decode_hex(value: &str) -> Result<Vec<u8>, String> {
    if value.is_empty() || !value.len().is_multiple_of(2) {
        return Err("hex value has an invalid length".into());
    }
    let mut output = Vec::with_capacity(value.len() / 2);
    let (pairs, remainder) = value.as_bytes().as_chunks::<2>();
    debug_assert!(remainder.is_empty());
    for pair in pairs {
        let high = hex_nibble(pair[0])?;
        let low = hex_nibble(pair[1])?;
        output.push((high << 4) | low);
    }
    Ok(output)
}

fn hex_nibble(value: u8) -> Result<u8, String> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err("hex value contains a non-hexadecimal character".into()),
    }
}

fn hex_encode(value: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(value.len() * 2);
    for byte in value {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    output
}

fn now_epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .try_into()
        .unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    fn signed_policy(now: i64, rules: Vec<ExecutionPolicyRule>) -> (Vec<u8>, String) {
        let key = Ed25519KeyPair::from_seed_unchecked(&[37_u8; 32]).unwrap();
        let payload = ExecutionPolicyManifest {
            schema_version: 1,
            policy_id: "dread-agent-policy".into(),
            revision: 4,
            verifier_id: "dread-policy-authority".into(),
            source_reference: "policy://project-dread/agent-execution/4".into(),
            observed_at: now - 10,
            expires_at: now + 1800,
            rules,
        };
        let payload_bytes = serde_json::to_vec(&payload).unwrap();
        let envelope = SignedExecutionPolicyEnvelope {
            key_id: "policy-test-key".into(),
            payload,
            signature_hex: hex_encode(key.sign(&payload_bytes).as_ref()),
        };
        (
            serde_json::to_vec(&envelope).unwrap(),
            hex_encode(key.public_key().as_ref()),
        )
    }

    fn allow_rule() -> ExecutionPolicyRule {
        ExecutionPolicyRule {
            rule_id: "allow-hermes-test".into(),
            requester_id: "test-operator".into(),
            task_id: Some("task-7".into()),
            mission_id: Some("mission-2".into()),
            capability_id: "hermes.run".into(),
            operation: "agent.execute".into(),
            permission_scope: "agent:execute".into(),
            effect: ExecutionPolicyEffect::Allow,
            issued_at: 1_799_999_990,
            expires_at: 1_800_001_700,
            max_attempts: 3,
        }
    }

    fn input<'a>(request_id: &'a str) -> ExecutionPolicyInput<'a> {
        ExecutionPolicyInput {
            request_id,
            requester_id: "test-operator",
            task_id: "task-7",
            mission_id: Some("mission-2"),
            capability_id: "hermes.run",
            operation: "agent.execute",
            permission_scope: "agent:execute",
            attempt_number: 1,
        }
    }

    #[test]
    fn policy_decision_is_signed_scoped_and_request_bound() {
        let now = 1_800_000_000;
        let (encoded, key) = signed_policy(now, vec![allow_rule()]);
        let verified = VerifiedExecutionPolicy::verify(&encoded, &key, now).unwrap();
        let first = verified.authorize(input("request-a"), now).unwrap();
        let replay = verified.authorize(input("request-a"), now).unwrap();
        let other_request = verified.authorize(input("request-b"), now).unwrap();
        assert_eq!(first, replay);
        assert_ne!(first.decision_id, other_request.decision_id);
        assert_eq!(first.rule_id, "allow-hermes-test");
        assert_eq!(first.expires_at, 1_800_001_700);
    }

    #[test]
    fn explicit_deny_overrides_matching_allow() {
        let now = 1_800_000_000;
        let mut deny = allow_rule();
        deny.rule_id = "deny-task-7".into();
        deny.effect = ExecutionPolicyEffect::Deny;
        let (encoded, key) = signed_policy(now, vec![allow_rule(), deny]);
        let verified = VerifiedExecutionPolicy::verify(&encoded, &key, now).unwrap();
        assert!(verified.authorize(input("request-a"), now).is_err());
    }

    #[test]
    fn ambiguous_allow_rules_fail_closed() {
        let now = 1_800_000_000;
        let mut second = allow_rule();
        second.rule_id = "allow-hermes-duplicate".into();
        let (encoded, key) = signed_policy(now, vec![allow_rule(), second]);
        let verified = VerifiedExecutionPolicy::verify(&encoded, &key, now).unwrap();
        assert!(verified.authorize(input("request-a"), now).is_err());
    }

    #[test]
    fn changed_requester_task_mission_scope_and_attempt_are_rejected() {
        let now = 1_800_000_000;
        let (encoded, key) = signed_policy(now, vec![allow_rule()]);
        let verified = VerifiedExecutionPolicy::verify(&encoded, &key, now).unwrap();

        let mut wrong_requester = input("request-a");
        wrong_requester.requester_id = "other-operator";
        assert!(verified.authorize(wrong_requester, now).is_err());

        let mut wrong_task = input("request-a");
        wrong_task.task_id = "task-other";
        assert!(verified.authorize(wrong_task, now).is_err());

        let mut wrong_mission = input("request-a");
        wrong_mission.mission_id = Some("mission-other");
        assert!(verified.authorize(wrong_mission, now).is_err());

        let mut over_attempt = input("request-a");
        over_attempt.attempt_number = 4;
        assert!(verified.authorize(over_attempt, now).is_err());
    }

    #[test]
    fn rejects_expired_policy_and_modified_signature() {
        let now = 1_800_000_000;
        let (encoded, key) = signed_policy(now, vec![allow_rule()]);
        assert!(VerifiedExecutionPolicy::verify(&encoded, &key, now + 3600).is_err());

        let mut envelope: SignedExecutionPolicyEnvelope = serde_json::from_slice(&encoded).unwrap();
        envelope.payload.rules[0].requester_id = "attacker".into();
        let tampered = serde_json::to_vec(&envelope).unwrap();
        assert!(VerifiedExecutionPolicy::verify(&tampered, &key, now).is_err());
    }
}
