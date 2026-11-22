//! Independently signed provider-runtime isolation attestations.
//!
//! This contract is deliberately separate from adapter receipt signing and
//! from the Phase 6 trust manifest. It binds a trusted runtime observer's
//! eight concrete control observations to one exact provider registration.
//! The API must cross-check the returned summary against the signed manifest;
//! an environment boolean or an unsigned local assertion is never accepted.

use ring::signature::{ED25519, UnparsedPublicKey};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const MAX_ATTESTATION_BYTES: usize = 64 * 1024;
const MAX_LIFETIME_SECONDS: i64 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeIsolationControl {
    RuntimeIdentity,
    TaskWorkspaceBoundary,
    MountPolicy,
    EgressPolicy,
    ToolPermissions,
    StableSecretHandling,
    CancellationPropagation,
    RestartReconciliation,
}

impl RuntimeIsolationControl {
    pub const ALL: [Self; 8] = [
        Self::RuntimeIdentity,
        Self::TaskWorkspaceBoundary,
        Self::MountPolicy,
        Self::EgressPolicy,
        Self::ToolPermissions,
        Self::StableSecretHandling,
        Self::CancellationPropagation,
        Self::RestartReconciliation,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RuntimeIdentity => "runtime_identity",
            Self::TaskWorkspaceBoundary => "task_workspace_boundary",
            Self::MountPolicy => "mount_policy",
            Self::EgressPolicy => "egress_policy",
            Self::ToolPermissions => "tool_permissions",
            Self::StableSecretHandling => "stable_secret_handling",
            Self::CancellationPropagation => "cancellation_propagation",
            Self::RestartReconciliation => "restart_reconciliation",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeIsolationCheck {
    pub control: RuntimeIsolationControl,
    pub verified: bool,
    pub observation_id: String,
    pub evidence_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeIsolationAttestationPayload {
    pub schema_version: u32,
    pub attestation_id: String,
    pub attestor_id: String,
    pub provider_id: String,
    pub provider_registration_revision: i64,
    pub adapter_id: String,
    pub adapter_version: String,
    pub endpoint: String,
    pub runtime_identity: String,
    pub sandbox_backend: String,
    pub source_observation_id: String,
    pub evidence_bundle_sha256: String,
    pub observed_at: i64,
    pub expires_at: i64,
    pub checks: Vec<RuntimeIsolationCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SignedRuntimeIsolationEnvelope {
    key_id: String,
    payload: RuntimeIsolationAttestationPayload,
    signature_hex: String,
}

/// Public metadata only. It contains no attestor private key or mutable state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeIsolationAttestationSummary {
    pub attestation_id: String,
    pub attestor_id: String,
    pub provider_id: String,
    pub provider_registration_revision: i64,
    pub adapter_id: String,
    pub adapter_version: String,
    pub endpoint: String,
    pub runtime_identity: String,
    pub sandbox_backend: String,
    pub source_observation_id: String,
    pub evidence_bundle_sha256: String,
    pub observed_at: i64,
    pub expires_at: i64,
    pub checks: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy)]
pub struct RuntimeAttestationTrustAnchor<'a> {
    pub key_id: &'a str,
    pub public_key_hex: &'a str,
}

#[derive(Debug, Clone, Copy)]
pub struct RuntimeAttestationAdapterBinding<'a> {
    pub provider_id: &'a str,
    pub adapter_id: &'a str,
    pub adapter_version: &'a str,
    pub endpoint: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedRuntimeIsolationAttestation {
    payload: RuntimeIsolationAttestationPayload,
    key_id: String,
}

impl VerifiedRuntimeIsolationAttestation {
    /// Verify a signed envelope and bind it to the configured live adapter.
    /// Registration revision and manifest observation IDs are cross-checked
    /// separately by Phase6ExecutionAuthority at every admission.
    pub fn verify(
        encoded: &[u8],
        trust_anchor: RuntimeAttestationTrustAnchor<'_>,
        expected: RuntimeAttestationAdapterBinding<'_>,
        now: i64,
    ) -> Result<Self, String> {
        if encoded.is_empty() || encoded.len() > MAX_ATTESTATION_BYTES {
            return Err("runtime isolation attestation is empty or exceeds 64 KiB".into());
        }
        validate_text("runtime attestation key ID", trust_anchor.key_id, 128)?;
        let envelope: SignedRuntimeIsolationEnvelope = serde_json::from_slice(encoded)
            .map_err(|error| format!("runtime isolation attestation JSON is invalid: {error}"))?;
        if envelope.key_id != trust_anchor.key_id {
            return Err(
                "runtime isolation attestation key ID does not match the configured trust anchor"
                    .into(),
            );
        }

        let public_key = decode_hex(trust_anchor.public_key_hex)?;
        let signature = decode_hex(&envelope.signature_hex)?;
        if public_key.len() != 32 || signature.len() != 64 {
            return Err(
                "runtime attestation Ed25519 public key/signature length is invalid".into(),
            );
        }
        let payload_bytes = serde_json::to_vec(&envelope.payload)
            .map_err(|error| format!("cannot canonicalize runtime attestation payload: {error}"))?;
        UnparsedPublicKey::new(&ED25519, &public_key)
            .verify(&payload_bytes, &signature)
            .map_err(|_| "runtime isolation attestation signature is invalid".to_string())?;

        let payload = &envelope.payload;
        if payload.schema_version != 1 {
            return Err("unsupported runtime isolation attestation schema version".into());
        }
        validate_text("runtime attestation ID", &payload.attestation_id, 160)?;
        validate_text("runtime attestor ID", &payload.attestor_id, 160)?;
        validate_text("runtime identity", &payload.runtime_identity, 512)?;
        validate_text("sandbox backend", &payload.sandbox_backend, 160)?;
        validate_text(
            "runtime source observation ID",
            &payload.source_observation_id,
            256,
        )?;
        if payload.provider_id != expected.provider_id
            || payload.adapter_id != expected.adapter_id
            || payload.adapter_version != expected.adapter_version
            || payload.endpoint != expected.endpoint
        {
            return Err(
                "runtime isolation attestation does not match the configured provider, adapter build or endpoint"
                    .into(),
            );
        }
        if payload.provider_registration_revision <= 0 {
            return Err("runtime attestation provider registration revision is invalid".into());
        }
        validate_sha256(
            "runtime attestation evidence bundle digest",
            &payload.evidence_bundle_sha256,
        )?;
        if payload.observed_at <= 0
            || payload.observed_at > now
            || payload.expires_at <= now
            || payload.expires_at <= payload.observed_at
            || payload.expires_at > payload.observed_at.saturating_add(MAX_LIFETIME_SECONDS)
        {
            return Err("runtime isolation attestation is future-dated, expired or exceeds its lifetime bound".into());
        }

        let expected: BTreeSet<_> = RuntimeIsolationControl::ALL.into_iter().collect();
        let mut actual = BTreeSet::new();
        let mut observation_ids = BTreeSet::new();
        for check in &payload.checks {
            if !actual.insert(check.control) {
                return Err("runtime isolation attestation contains a duplicate control".into());
            }
            if !check.verified {
                return Err(format!(
                    "runtime isolation control '{}' is not verified",
                    check.control.as_str()
                ));
            }
            validate_text("runtime control observation ID", &check.observation_id, 256)?;
            if !observation_ids.insert(check.observation_id.as_str()) {
                return Err("runtime isolation controls must have distinct observation IDs".into());
            }
            validate_sha256("runtime control evidence digest", &check.evidence_sha256)?;
        }
        if observation_ids.contains(payload.source_observation_id.as_str()) {
            return Err(
                "runtime source observation ID must be distinct from control observation IDs"
                    .into(),
            );
        }
        if actual != expected || payload.checks.len() != RuntimeIsolationControl::ALL.len() {
            return Err(
                "runtime isolation attestation must contain exactly the eight required controls"
                    .into(),
            );
        }

        Ok(Self {
            payload: envelope.payload,
            key_id: envelope.key_id,
        })
    }

    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    pub fn provider_id(&self) -> &str {
        &self.payload.provider_id
    }

    pub fn adapter_id(&self) -> &str {
        &self.payload.adapter_id
    }

    pub fn adapter_version(&self) -> &str {
        &self.payload.adapter_version
    }

    pub fn endpoint(&self) -> &str {
        &self.payload.endpoint
    }

    pub fn sandbox_backend(&self) -> &str {
        &self.payload.sandbox_backend
    }

    pub fn control_verified(&self, control: RuntimeIsolationControl) -> bool {
        self.payload
            .checks
            .iter()
            .any(|check| check.control == control && check.verified)
    }

    pub fn summary(&self) -> RuntimeIsolationAttestationSummary {
        RuntimeIsolationAttestationSummary {
            attestation_id: self.payload.attestation_id.clone(),
            attestor_id: self.payload.attestor_id.clone(),
            provider_id: self.payload.provider_id.clone(),
            provider_registration_revision: self.payload.provider_registration_revision,
            adapter_id: self.payload.adapter_id.clone(),
            adapter_version: self.payload.adapter_version.clone(),
            endpoint: self.payload.endpoint.clone(),
            runtime_identity: self.payload.runtime_identity.clone(),
            sandbox_backend: self.payload.sandbox_backend.clone(),
            source_observation_id: self.payload.source_observation_id.clone(),
            evidence_bundle_sha256: self.payload.evidence_bundle_sha256.clone(),
            observed_at: self.payload.observed_at,
            expires_at: self.payload.expires_at,
            checks: self
                .payload
                .checks
                .iter()
                .map(|check| {
                    (
                        check.control.as_str().to_owned(),
                        check.observation_id.clone(),
                    )
                })
                .collect(),
        }
    }
}

fn validate_text(name: &str, value: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(format!(
            "{name} is empty, too long or contains control characters"
        ));
    }
    Ok(())
}

fn validate_sha256(name: &str, value: &str) -> Result<(), String> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!(
            "{name} must contain exactly 64 hexadecimal characters"
        ));
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    fn signed_attestation(
        key: &Ed25519KeyPair,
        payload: RuntimeIsolationAttestationPayload,
    ) -> Vec<u8> {
        let payload_bytes = serde_json::to_vec(&payload).unwrap();
        let signature_hex = encode_hex(key.sign(&payload_bytes).as_ref());
        serde_json::to_vec(&SignedRuntimeIsolationEnvelope {
            key_id: "runtime-attestor-key-1".into(),
            payload,
            signature_hex,
        })
        .unwrap()
    }

    fn encode_hex(bytes: &[u8]) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut output = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            output.push(HEX[(byte >> 4) as usize] as char);
            output.push(HEX[(byte & 0x0f) as usize] as char);
        }
        output
    }

    fn payload() -> RuntimeIsolationAttestationPayload {
        RuntimeIsolationAttestationPayload {
            schema_version: 1,
            attestation_id: "isolation-evidence-hermes-7".into(),
            attestor_id: "dread-runtime-observer".into(),
            provider_id: "hermes".into(),
            provider_registration_revision: 7,
            adapter_id: "hermes".into(),
            adapter_version: "0.4.0".into(),
            endpoint: "http://127.0.0.1:8642".into(),
            runtime_identity: "hermes-sandbox-pool/blue".into(),
            sandbox_backend: "modal".into(),
            source_observation_id: "runtime-observation-20261122-1".into(),
            evidence_bundle_sha256: "ab".repeat(32),
            observed_at: 1_800_000_000,
            expires_at: 1_800_000_180,
            checks: RuntimeIsolationControl::ALL
                .into_iter()
                .map(|control| RuntimeIsolationCheck {
                    control,
                    verified: true,
                    observation_id: format!("{}-observation-1", control.as_str()),
                    evidence_sha256: "cd".repeat(32),
                })
                .collect(),
        }
    }

    #[test]
    fn verifies_signed_runtime_attestation_and_returns_manifest_cross_binding() {
        let key = Ed25519KeyPair::from_seed_unchecked(&[21_u8; 32]).unwrap();
        let encoded = signed_attestation(&key, payload());
        let public_key = encode_hex(key.public_key().as_ref());
        let verified = VerifiedRuntimeIsolationAttestation::verify(
            &encoded,
            RuntimeAttestationTrustAnchor {
                key_id: "runtime-attestor-key-1",
                public_key_hex: &public_key,
            },
            RuntimeAttestationAdapterBinding {
                provider_id: "hermes",
                adapter_id: "hermes",
                adapter_version: "0.4.0",
                endpoint: "http://127.0.0.1:8642",
            },
            1_800_000_010,
        )
        .unwrap();
        let summary = verified.summary();
        assert_eq!(summary.attestation_id, "isolation-evidence-hermes-7");
        assert_eq!(summary.provider_registration_revision, 7);
        assert_eq!(
            summary.checks.get("egress_policy").map(String::as_str),
            Some("egress_policy-observation-1")
        );
    }

    #[test]
    fn rejects_invalid_signatures_expired_reports_and_registration_mismatches() {
        let key = Ed25519KeyPair::from_seed_unchecked(&[22_u8; 32]).unwrap();
        let public_key = encode_hex(key.public_key().as_ref());
        let encoded = signed_attestation(&key, payload());
        assert!(
            VerifiedRuntimeIsolationAttestation::verify(
                &encoded,
                RuntimeAttestationTrustAnchor {
                    key_id: "wrong-key-id",
                    public_key_hex: &public_key,
                },
                RuntimeAttestationAdapterBinding {
                    provider_id: "hermes",
                    adapter_id: "hermes",
                    adapter_version: "0.4.0",
                    endpoint: "http://127.0.0.1:8642",
                },
                1_800_000_010,
            )
            .is_err()
        );
        assert!(
            VerifiedRuntimeIsolationAttestation::verify(
                &encoded,
                RuntimeAttestationTrustAnchor {
                    key_id: "runtime-attestor-key-1",
                    public_key_hex: &public_key,
                },
                RuntimeAttestationAdapterBinding {
                    provider_id: "hermes",
                    adapter_id: "hermes",
                    adapter_version: "0.5.0",
                    endpoint: "http://127.0.0.1:8642",
                },
                1_800_000_010,
            )
            .is_err()
        );
        assert!(
            VerifiedRuntimeIsolationAttestation::verify(
                &encoded,
                RuntimeAttestationTrustAnchor {
                    key_id: "runtime-attestor-key-1",
                    public_key_hex: &public_key,
                },
                RuntimeAttestationAdapterBinding {
                    provider_id: "hermes",
                    adapter_id: "hermes",
                    adapter_version: "0.4.0",
                    endpoint: "http://127.0.0.1:8642",
                },
                1_800_000_181,
            )
            .is_err()
        );
    }

    #[test]
    fn every_required_control_must_be_present_verified_and_unique() {
        let key = Ed25519KeyPair::from_seed_unchecked(&[23_u8; 32]).unwrap();
        let public_key = encode_hex(key.public_key().as_ref());
        let mut missing = payload();
        missing.checks.pop();
        assert!(
            VerifiedRuntimeIsolationAttestation::verify(
                &signed_attestation(&key, missing),
                RuntimeAttestationTrustAnchor {
                    key_id: "runtime-attestor-key-1",
                    public_key_hex: &public_key,
                },
                RuntimeAttestationAdapterBinding {
                    provider_id: "hermes",
                    adapter_id: "hermes",
                    adapter_version: "0.4.0",
                    endpoint: "http://127.0.0.1:8642",
                },
                1_800_000_010,
            )
            .is_err()
        );

        let mut unverified = payload();
        unverified.checks[0].verified = false;
        assert!(
            VerifiedRuntimeIsolationAttestation::verify(
                &signed_attestation(&key, unverified),
                RuntimeAttestationTrustAnchor {
                    key_id: "runtime-attestor-key-1",
                    public_key_hex: &public_key,
                },
                RuntimeAttestationAdapterBinding {
                    provider_id: "hermes",
                    adapter_id: "hermes",
                    adapter_version: "0.4.0",
                    endpoint: "http://127.0.0.1:8642",
                },
                1_800_000_010,
            )
            .is_err()
        );

        let mut duplicate = payload();
        duplicate.checks[1].control = duplicate.checks[0].control;
        assert!(
            VerifiedRuntimeIsolationAttestation::verify(
                &signed_attestation(&key, duplicate),
                RuntimeAttestationTrustAnchor {
                    key_id: "runtime-attestor-key-1",
                    public_key_hex: &public_key,
                },
                RuntimeAttestationAdapterBinding {
                    provider_id: "hermes",
                    adapter_id: "hermes",
                    adapter_version: "0.4.0",
                    endpoint: "http://127.0.0.1:8642",
                },
                1_800_000_010,
            )
            .is_err()
        );

        let mut reused_observation = payload();
        reused_observation.checks[1].observation_id =
            reused_observation.checks[0].observation_id.clone();
        assert!(
            VerifiedRuntimeIsolationAttestation::verify(
                &signed_attestation(&key, reused_observation),
                RuntimeAttestationTrustAnchor {
                    key_id: "runtime-attestor-key-1",
                    public_key_hex: &public_key,
                },
                RuntimeAttestationAdapterBinding {
                    provider_id: "hermes",
                    adapter_id: "hermes",
                    adapter_version: "0.4.0",
                    endpoint: "http://127.0.0.1:8642",
                },
                1_800_000_010,
            )
            .is_err()
        );

        let mut reused_source_observation = payload();
        reused_source_observation.checks[0].observation_id =
            reused_source_observation.source_observation_id.clone();
        assert!(
            VerifiedRuntimeIsolationAttestation::verify(
                &signed_attestation(&key, reused_source_observation),
                RuntimeAttestationTrustAnchor {
                    key_id: "runtime-attestor-key-1",
                    public_key_hex: &public_key,
                },
                RuntimeAttestationAdapterBinding {
                    provider_id: "hermes",
                    adapter_id: "hermes",
                    adapter_version: "0.4.0",
                    endpoint: "http://127.0.0.1:8642",
                },
                1_800_000_010,
            )
            .is_err()
        );
    }
}
