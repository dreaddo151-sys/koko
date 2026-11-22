//! Signed provider trust and Phase 6 execution authority.
//!
//! Provider execution is authorized only by a signed, time-bounded trust
//! manifest plus the current canonical capability approval and task approval.
//! Environment booleans are configuration hints, never isolation evidence.
//!
//! The independent-result verifier is deliberately fail-closed until a
//! separately authenticated verifier integration is configured.

use crate::execution_policy::{ExecutionPolicyAuthority, ExecutionPolicyInput};
use crate::independent_verifier::IndependentResultVerifier;
use dread_agents::{
    AgentAdapter, AgentEvent, AgentExecutionIdentity, AgentOperationEvidenceKind,
    AgentOperationEvidencePayload, AgentOperationReceipt, CapabilityKind, CapabilityRecord,
    CapabilityStatus, DurableAgentStartRequest, RuntimeIsolationAttestationSummary,
    StartTaskRequest, response_sha256,
};
use dread_orchestrator::persistence::{
    AdmissionRecord, AttemptRecord, AuthorizationFacts, CurrentRetryContractFacts, DispatchRequest,
    ExecutionAuthority, ProviderEventReceiptContext, ProviderEvidence, SignedProviderReceipt,
    VerificationFacts,
};
use ring::{digest, signature};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    env, fs,
    path::{Path, PathBuf},
    sync::Arc,
};

const MAX_MANIFEST_BYTES: u64 = 1_048_576;
const MAX_MANIFEST_LIFETIME: i64 = 30 * 24 * 60 * 60;
const ADMISSION_LIFETIME: i64 = 5 * 60;
const AGENT_OPERATION: &str = "agent.execute";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedExecutionTrustEnvelope {
    pub key_id: String,
    pub payload: ExecutionTrustManifest,
    pub signature_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionTrustManifest {
    pub schema_version: u32,
    pub manifest_id: String,
    pub verifier_id: String,
    pub source_reference: String,
    pub observed_at: i64,
    pub expires_at: i64,
    pub providers: Vec<ProviderTrustRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderTrustRecord {
    pub provider_id: String,
    pub adapter_id: String,
    pub adapter_version: String,
    pub endpoint: String,
    pub registration_revision: i64,
    pub capability_id: String,
    pub capability_revision: i64,
    pub operation: String,
    /// Optional for legacy registrations; durable receipt verification refuses
    /// to proceed unless both values are present and valid.
    #[serde(default)]
    pub receipt_key_id: Option<String>,
    #[serde(default)]
    pub receipt_public_key_hex: Option<String>,
    pub enabled: bool,
    pub revoked: bool,
    pub evidence_id: String,
    pub source_reference: String,
    pub observed_at: i64,
    pub expires_at: i64,
    pub isolation: IsolationEvidenceRecord,
    pub retry_contract: RetryContractTrust,
}

pub const PREFLIGHT_CONTRACT_VERSION: &str = "provider-isolation-preflight-v1";
pub const MAX_PREFLIGHT_LIFETIME_SECONDS: i64 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IsolationControl {
    RuntimeIdentity,
    TaskWorkspaceBoundary,
    MountPolicy,
    EgressPolicy,
    ToolPermissions,
    StableSecretHandling,
    CancellationPropagation,
    RestartReconciliation,
}

impl IsolationControl {
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlEvidence {
    pub verified: bool,
    pub durable_observation_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceProvenance {
    pub source_observation_id: String,
    pub recorded_by_adapter_id: String,
    pub recorded_by_adapter_version: String,
    pub evidence_sha256: String,
    pub recorded_at: i64,
    pub authenticated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IsolationEvidenceRecord {
    pub contract_version: String,
    pub evidence_id: String,
    pub provider_id: String,
    pub provider_registration_revision: i64,
    pub adapter_id: String,
    pub adapter_version: String,
    pub runtime_identity: String,
    pub provenance: EvidenceProvenance,
    pub checks: BTreeMap<IsolationControl, ControlEvidence>,
    pub checked_at: i64,
    pub expires_at: i64,
}

impl IsolationEvidenceRecord {
    fn all_verified(&self) -> bool {
        let expected: BTreeSet<_> = IsolationControl::ALL.into_iter().collect();
        let actual: BTreeSet<_> = self.checks.keys().copied().collect();
        expected == actual
            && self.checks.values().all(|evidence| {
                evidence.verified && !evidence.durable_observation_id.trim().is_empty()
            })
    }
}

fn isolation_control_name(control: IsolationControl) -> &'static str {
    match control {
        IsolationControl::RuntimeIdentity => "runtime_identity",
        IsolationControl::TaskWorkspaceBoundary => "task_workspace_boundary",
        IsolationControl::MountPolicy => "mount_policy",
        IsolationControl::EgressPolicy => "egress_policy",
        IsolationControl::ToolPermissions => "tool_permissions",
        IsolationControl::StableSecretHandling => "stable_secret_handling",
        IsolationControl::CancellationPropagation => "cancellation_propagation",
        IsolationControl::RestartReconciliation => "restart_reconciliation",
    }
}

/// Cross-bind the separate runtime observer signature to the already signed
/// provider trust manifest. No operator boolean or partial control report can
/// satisfy this comparison.
fn verify_runtime_isolation_binding(
    provider_id: &str,
    registration: &ProviderTrustRecord,
    runtime: &RuntimeIsolationAttestationSummary,
    now: i64,
) -> Result<(), String> {
    let isolation = &registration.isolation;
    if !isolation.provenance.authenticated || !isolation.all_verified() {
        return Err("provider trust manifest does not contain authenticated eight-control isolation evidence".into());
    }
    if runtime.attestor_id == provider_id
        || runtime.attestor_id == registration.adapter_id
        || runtime.attestor_id == isolation.provenance.recorded_by_adapter_id
    {
        return Err(
            "runtime isolation attestor identity is not independent from the provider/adapter"
                .into(),
        );
    }
    if runtime.attestation_id != isolation.evidence_id
        || runtime.provider_id != provider_id
        || runtime.provider_registration_revision != registration.registration_revision
        || runtime.provider_registration_revision != isolation.provider_registration_revision
        || runtime.adapter_id != registration.adapter_id
        || runtime.adapter_version != registration.adapter_version
        || runtime.endpoint != registration.endpoint
        || runtime.runtime_identity != isolation.runtime_identity
        || runtime.source_observation_id != isolation.provenance.source_observation_id
        || runtime.evidence_bundle_sha256 != isolation.provenance.evidence_sha256
        || runtime.observed_at != isolation.checked_at
        || runtime.expires_at != isolation.expires_at
        || runtime.expires_at <= now
        || runtime.observed_at > now
    {
        return Err(
            "independent runtime attestation does not exactly match the signed provider isolation record"
                .into(),
        );
    }
    if runtime.checks.len() != IsolationControl::ALL.len() {
        return Err(
            "independent runtime attestation does not contain exactly eight control observations"
                .into(),
        );
    }
    for control in IsolationControl::ALL {
        let expected = isolation.checks.get(&control).ok_or_else(|| {
            "signed provider isolation record is missing a required control".to_string()
        })?;
        let actual = runtime
            .checks
            .get(isolation_control_name(control))
            .ok_or_else(|| {
                "independent runtime attestation is missing a required control observation"
                    .to_string()
            })?;
        if !expected.verified || expected.durable_observation_id != *actual {
            return Err(format!(
                "runtime attestation observation for '{}' does not match the signed provider registration",
                isolation_control_name(control)
            ));
        }
    }
    Ok(())
}

fn validate_provider_receipt_key(record: &ProviderTrustRecord) -> Result<(), String> {
    let key_id = record
        .receipt_key_id
        .as_deref()
        .ok_or_else(|| "provider dispatch requires a manifest-pinned receipt key ID".to_string())?;
    validate_text("receipt_key_id", key_id, 128)?;
    let public_key_hex = record.receipt_public_key_hex.as_deref().ok_or_else(|| {
        "provider dispatch requires a manifest-pinned receipt public key".to_string()
    })?;
    let public_key = decode_hex(public_key_hex)?;
    if public_key.len() != 32 {
        return Err("provider receipt Ed25519 public key must be 32 bytes".into());
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetryContractTrust {
    pub contract_id: String,
    pub evidence_id: String,
    pub revision: i64,
    pub expires_at: i64,
    pub sha256: String,
    pub verifier_id: String,
    pub source_reference: String,
    pub observed_at: i64,
    pub idempotency_class: String,
    pub idempotent_operation: bool,
    pub provider_keys_supported: bool,
    pub max_attempts: u32,
}

#[derive(Debug, Clone)]
pub struct VerifiedExecutionTrustManifest {
    envelope: SignedExecutionTrustEnvelope,
    payload_sha256: String,
}

impl VerifiedExecutionTrustManifest {
    pub fn verify(encoded: &[u8], public_key_hex: &str, now: i64) -> Result<Self, String> {
        if encoded.is_empty() || encoded.len() as u64 > MAX_MANIFEST_BYTES {
            return Err("execution trust manifest is empty or exceeds 1 MiB".into());
        }
        let envelope: SignedExecutionTrustEnvelope = serde_json::from_slice(encoded)
            .map_err(|error| format!("execution trust manifest JSON is invalid: {error}"))?;
        validate_text("key_id", &envelope.key_id, 128)?;
        let public_key = decode_hex(public_key_hex)?;
        let signature_bytes = decode_hex(&envelope.signature_hex)?;
        if public_key.len() != 32 || signature_bytes.len() != 64 {
            return Err("Ed25519 public key or signature has an invalid length".into());
        }

        let payload_bytes = serde_json::to_vec(&envelope.payload)
            .map_err(|error| format!("cannot canonicalize trust manifest: {error}"))?;
        signature::UnparsedPublicKey::new(&signature::ED25519, &public_key)
            .verify(&payload_bytes, &signature_bytes)
            .map_err(|_| "execution trust manifest signature is invalid".to_string())?;

        validate_manifest_payload(&envelope.payload, now)?;
        let payload_sha256 = hex_encode(digest::digest(&digest::SHA256, &payload_bytes).as_ref());
        Ok(Self {
            envelope,
            payload_sha256,
        })
    }

    pub fn manifest_id(&self) -> &str {
        &self.envelope.payload.manifest_id
    }

    pub fn verifier_id(&self) -> &str {
        &self.envelope.payload.verifier_id
    }

    pub fn expires_at(&self) -> i64 {
        self.envelope.payload.expires_at
    }

    pub fn payload_sha256(&self) -> &str {
        &self.payload_sha256
    }

    pub fn providers(&self) -> &[ProviderTrustRecord] {
        &self.envelope.payload.providers
    }

    fn provider(&self, provider_id: &str, now: i64) -> Result<&ProviderTrustRecord, String> {
        let record = self
            .providers()
            .iter()
            .find(|record| record.provider_id == provider_id)
            .ok_or_else(|| format!("provider '{provider_id}' has no signed trust registration"))?;
        if !record.enabled || record.revoked {
            return Err(format!("provider '{provider_id}' is disabled or revoked"));
        }
        if record.expires_at <= now || self.expires_at() <= now {
            return Err(format!(
                "provider '{provider_id}' trust evidence has expired"
            ));
        }
        Ok(record)
    }

    pub fn public_status(&self, now: i64) -> Value {
        let providers = self
            .providers()
            .iter()
            .map(|provider| {
                json!({
                    "provider_id": provider.provider_id,
                    "adapter_id": provider.adapter_id,
                    "adapter_version": provider.adapter_version,
                    "capability_id": provider.capability_id,
                    "registration_revision": provider.registration_revision,
                    "capability_revision": provider.capability_revision,
                    "enabled": provider.enabled && !provider.revoked && provider.expires_at > now,
                    "revoked": provider.revoked,
                    "expires_at": provider.expires_at,
                    "isolation_verified": provider.isolation.all_verified(),
                    "receipt_key_pinned": provider.receipt_key_id.is_some()
                        && provider.receipt_public_key_hex.is_some(),
                    "isolation_evidence_id": provider.isolation.evidence_id,
                    "runtime_identity": provider.isolation.runtime_identity,
                    "retry_contract_id": provider.retry_contract.contract_id,
                    "retry_contract_expires_at": provider.retry_contract.expires_at
                })
            })
            .collect::<Vec<_>>();
        json!({
            "status": if self.expires_at() > now { "verified" } else { "expired" },
            "manifest_id": self.manifest_id(),
            "verifier_id": self.verifier_id(),
            "source_reference": self.envelope.payload.source_reference,
            "observed_at": self.envelope.payload.observed_at,
            "expires_at": self.expires_at(),
            "payload_sha256": self.payload_sha256(),
            "key_id": self.envelope.key_id,
            "providers": providers,
            "independent_result_verification": "not_configured_fail_closed"
        })
    }
}

pub struct Phase6ExecutionAuthority {
    manifest: VerifiedExecutionTrustManifest,
    manifest_path: Option<PathBuf>,
    public_key_hex: Option<String>,
    adapters: Arc<HashMap<String, Arc<dyn AgentAdapter>>>,
    policy: Option<ExecutionPolicyAuthority>,
    independent_verifier: Option<IndependentResultVerifier>,
}

impl Phase6ExecutionAuthority {
    fn with_source(
        manifest: VerifiedExecutionTrustManifest,
        adapters: Arc<HashMap<String, Arc<dyn AgentAdapter>>>,
        manifest_path: PathBuf,
        public_key_hex: String,
        policy: Option<ExecutionPolicyAuthority>,
        independent_verifier: Option<IndependentResultVerifier>,
    ) -> Self {
        Self {
            manifest,
            manifest_path: Some(manifest_path),
            public_key_hex: Some(public_key_hex),
            adapters,
            policy,
            independent_verifier,
        }
    }

    fn require_independent_verifier_ready(&self, now: i64) -> Result<(), String> {
        self.independent_verifier
            .as_ref()
            .ok_or_else(|| {
                "independent result verifier is not configured; new provider admission is blocked"
                    .to_string()
            })?
            .ensure_ready(now)
    }

    fn refresh_manifest(&self, now: i64) -> Result<VerifiedExecutionTrustManifest, String> {
        match (&self.manifest_path, &self.public_key_hex) {
            (Some(path), Some(public_key)) => read_and_verify_manifest(path, public_key, now),
            _ if self.manifest.expires_at() > now => Ok(self.manifest.clone()),
            _ => Err("execution trust manifest has expired".into()),
        }
    }

    fn registration(
        &self,
        provider_id: &str,
        now: i64,
    ) -> Result<(VerifiedExecutionTrustManifest, ProviderTrustRecord, String), String> {
        let manifest = self.refresh_manifest(now)?;
        let record = manifest.provider(provider_id, now)?.clone();
        let adapter = self
            .adapters
            .get(provider_id)
            .ok_or_else(|| format!("provider adapter '{provider_id}' is not configured"))?;
        let descriptor = adapter.descriptor();
        let version = descriptor.version.as_deref().unwrap_or_default();
        if record.isolation.expires_at <= now {
            return Err(format!(
                "provider '{provider_id}' isolation evidence has expired"
            ));
        }
        if !descriptor.enabled
            || descriptor.adapter != record.adapter_id
            || version != record.adapter_version
            || descriptor.endpoint.as_deref() != Some(record.endpoint.as_str())
        {
            return Err(format!(
                "live adapter '{provider_id}' does not match its signed registration"
            ));
        }
        let runtime_attestation = adapter.runtime_isolation_attestation().ok_or_else(|| {
            format!(
                "provider '{provider_id}' has no independently signed runtime isolation attestation"
            )
        })?;
        verify_runtime_isolation_binding(provider_id, &record, &runtime_attestation, now)?;
        Ok((manifest, record, version.to_owned()))
    }

    fn capability_revision(
        connection: &Connection,
        capability_id: &str,
        provider_id: &str,
    ) -> Result<i64, String> {
        let encoded: String = connection
            .query_row(
                "SELECT record_json FROM capabilities WHERE id = ?1",
                [capability_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("capability '{capability_id}' is not registered"))?;
        let capability: CapabilityRecord =
            serde_json::from_str(&encoded).map_err(|error| error.to_string())?;
        capability.validate().map_err(|error| error.to_string())?;
        if capability.id != capability_id
            || capability.kind != CapabilityKind::Agent
            || capability.status != CapabilityStatus::Enabled
            || capability.adapter.as_deref() != Some(provider_id)
        {
            return Err("canonical capability is not an enabled matching agent".into());
        }
        let approval = capability
            .manifest
            .get("approval")
            .ok_or_else(|| "enabled capability has no approval record".to_string())?;
        let approved_by = approval
            .get("approved_by")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "capability approval has no operator identity".to_string())?;
        let approved_at = approval
            .get("approved_at")
            .and_then(Value::as_u64)
            .ok_or_else(|| "capability approval has no timestamp".to_string())?;
        if approved_at == 0 {
            return Err("capability approval timestamp is invalid".into());
        }

        let (revision, event_type, actor): (i64, String, String) = connection
            .query_row(
                "SELECT id, event_type, actor FROM capability_events
                 WHERE capability_id = ?1 ORDER BY id DESC LIMIT 1",
                [capability_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "capability has no durable approval event".to_string())?;
        if event_type != "enabled" || actor != approved_by || revision <= 0 {
            return Err("latest capability audit event is not the current approval".into());
        }
        Ok(revision)
    }

    fn approved_task(
        connection: &Connection,
        task_id: &str,
        requester_id: &str,
        allow_active: bool,
    ) -> Result<(Option<String>, i64), String> {
        let (status, task_type, approved_by, capability_id): (
            String,
            String,
            Option<String>,
            Option<String>,
        ) = connection
            .query_row(
                "SELECT status, task_type, approved_by, capability_id
                 FROM tasks WHERE id = ?1",
                [task_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("task '{task_id}' does not exist"))?;
        if !(status == "queued"
            || (allow_active && matches!(status.as_str(), "running" | "cancelling")))
            || task_type != "agent_task"
            || approved_by.as_deref() != Some(requester_id)
            || capability_id.is_none()
        {
            return Err("task is not queued with the matching operator approval".into());
        }
        let (sequence, actor, payload): (i64, String, String) = connection
            .query_row(
                "SELECT sequence, actor, payload_json FROM task_events
                 WHERE task_id = ?1 AND event_type = 'task_approved'
                 ORDER BY sequence DESC LIMIT 1",
                [task_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "task has no durable approval event".to_string())?;
        let payload: Value = serde_json::from_str(&payload).map_err(|error| error.to_string())?;
        if actor != requester_id
            || payload.get("approved_by").and_then(Value::as_str) != Some(requester_id)
        {
            return Err("task approval event does not match the dispatch requester".into());
        }
        let mission_id = connection
            .query_row(
                "SELECT mission_id FROM mission_tasks WHERE task_id = ?1",
                [task_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        Ok((mission_id, sequence))
    }

    fn verify_admission_scope(
        &self,
        connection: &Connection,
        admission: &AdmissionRecord,
        now: i64,
    ) -> Result<(), String> {
        let (_, record, _) = self.registration(&admission.provider_id, now)?;
        validate_provider_receipt_key(&record)?;
        if record.capability_id != admission.capability_id
            || record.operation != admission.operation
            || record.adapter_id != admission.adapter_id
            || record.adapter_version != admission.adapter_version
            || record.registration_revision != admission.provider_registration_revision
            || record.capability_revision != admission.capability_revision
            || record.isolation.evidence_id != admission.evidence_id
        {
            return Err("signed provider registration no longer matches the admission".into());
        }
        let revision = Self::capability_revision(
            connection,
            &admission.capability_id,
            &admission.provider_id,
        )?;
        if revision != admission.capability_revision {
            return Err("capability approval revision changed after admission".into());
        }
        let (mission_id, _) = Self::approved_task(
            connection,
            &admission.task_id,
            &admission.requester_id,
            true,
        )?;
        if mission_id != admission.mission_id {
            return Err("canonical mission mapping changed after admission".into());
        }

        let policy = self.policy.as_ref().ok_or_else(|| {
            "signed execution policy is not configured; provider dispatch is blocked".to_string()
        })?;
        let decision = policy.authorize(
            ExecutionPolicyInput {
                request_id: &admission.request_id,
                requester_id: &admission.requester_id,
                task_id: &admission.task_id,
                mission_id: admission.mission_id.as_deref(),
                capability_id: &admission.capability_id,
                operation: &admission.operation,
                permission_scope: &admission.permission_scope,
                attempt_number: admission.attempt_number,
            },
            now,
        )?;
        if decision.decision_id != admission.policy_decision_id {
            return Err(
                "current signed policy decision differs from the persisted admission".into(),
            );
        }
        Ok(())
    }

    pub fn validate_provider_capability(
        &self,
        connection: &Connection,
        capability_id: &str,
        provider_id: &str,
        now: i64,
    ) -> Result<(), String> {
        let (_, registration, _) = self.registration(provider_id, now)?;
        validate_provider_receipt_key(&registration)?;
        let revision = Self::capability_revision(connection, capability_id, provider_id)?;
        if registration.capability_id != capability_id
            || registration.capability_revision != revision
            || registration.operation != AGENT_OPERATION
        {
            return Err(
                "signed provider trust does not match the current enabled capability".into(),
            );
        }
        Ok(())
    }

    pub fn status(&self, now: i64) -> Value {
        let mut status = match self.refresh_manifest(now) {
            Ok(manifest) => manifest.public_status(now),
            Err(_) => json!({
                "status": "invalid_or_expired",
                "dispatch": "blocked",
                "independent_result_verification": "not_configured_fail_closed"
            }),
        };
        status["execution_policy"] = match self.policy.as_ref() {
            Some(policy) => policy.status(now),
            None => json!({
                "status": "not_configured",
                "dispatch": "blocked",
                "required_environment": [
                    "DREAD_EXECUTION_POLICY_MANIFEST",
                    "DREAD_EXECUTION_POLICY_PUBLIC_KEY_HEX"
                ]
            }),
        };
        let verifier_status = match self.independent_verifier.as_ref() {
            Some(verifier) => verifier.status(now),
            None => json!({
                "status": "not_configured_fail_closed",
                "dispatch": "blocked",
                "required_environment": [
                    "DREAD_INDEPENDENT_RESULT_VERIFIER_RECEIPT_DIR",
                    "DREAD_INDEPENDENT_RESULT_VERIFIER_ID",
                    "DREAD_INDEPENDENT_RESULT_VERIFIER_KEY_ID",
                    "DREAD_INDEPENDENT_RESULT_VERIFIER_PUBLIC_KEY_HEX",
                    "DREAD_INDEPENDENT_RESULT_VERIFIER_RECEIPT_UID",
                    "DREAD_INDEPENDENT_RESULT_VERIFIER_RECEIPT_GID"
                ]
            }),
        };
        status["independent_result_verification"] = verifier_status["status"].clone();
        status["independent_verifier"] = verifier_status;
        // Policy admission alone cannot enable dispatch: startup reattachment,
        // authenticated provider operation evidence and a live independent
        // result-verifier heartbeat are separate release conditions.
        status["dispatch"] = json!("blocked");
        status
    }
}

fn read_and_verify_manifest(
    path: &Path,
    public_key_hex: &str,
    now: i64,
) -> Result<VerifiedExecutionTrustManifest, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect execution trust manifest: {error}"))?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() == 0
        || metadata.len() > MAX_MANIFEST_BYTES
    {
        return Err(
            "execution trust manifest must be a regular non-symlink file no larger than 1 MiB"
                .into(),
        );
    }
    let encoded =
        fs::read(path).map_err(|error| format!("cannot read execution trust manifest: {error}"))?;
    VerifiedExecutionTrustManifest::verify(&encoded, public_key_hex, now)
}

pub fn load_from_env(
    adapters: Arc<HashMap<String, Arc<dyn AgentAdapter>>>,
    now: i64,
) -> Result<Option<Arc<Phase6ExecutionAuthority>>, String> {
    let policy = ExecutionPolicyAuthority::load_from_env()?;
    let independent_verifier = IndependentResultVerifier::load_from_env()?;
    let path = env::var_os("DREAD_EXECUTION_TRUST_MANIFEST");
    let key = env::var("DREAD_EXECUTION_TRUST_PUBLIC_KEY_HEX").ok();
    match (path, key) {
        (None, None) if policy.is_none() => Ok(None),
        (None, None) => Err(
            "a signed provider trust manifest is required when execution policy is configured"
                .into(),
        ),
        (Some(_), None) | (None, Some(_)) => Err(
            "DREAD_EXECUTION_TRUST_MANIFEST and DREAD_EXECUTION_TRUST_PUBLIC_KEY_HEX must be configured together"
                .into(),
        ),
        (Some(path), Some(key)) => {
            let path = PathBuf::from(path);
            let manifest = read_and_verify_manifest(&path, &key, now)?;
            Ok(Some(Arc::new(Phase6ExecutionAuthority::with_source(
                manifest,
                adapters,
                path,
                key,
                policy,
                independent_verifier,
            ))))
        }
    }
}

/// Convert a provider adapter's signed operation receipt into the durable
/// persistence representation. This function only transports the receipt;
/// signature, registration, request and attempt verification remains the
/// responsibility of `Phase6ExecutionAuthority`.
/// Build the immutable adapter identity from the persisted admission and
/// attempt. Mismatched rows are rejected before any provider lifecycle call.
pub fn agent_execution_identity(
    admission: &AdmissionRecord,
    attempt: &AttemptRecord,
) -> Result<AgentExecutionIdentity, String> {
    if admission.request_id != attempt.request_id
        || admission.task_id != attempt.task_id
        || admission.attempt_number != attempt.attempt_number
        || admission.provider_id != attempt.provider_id
        || admission.capability_id != attempt.capability_id
        || admission.operation != attempt.operation
        || admission.idempotency_key != attempt.idempotency_key
        || admission.adapter_id.is_empty()
        || admission.adapter_version.is_empty()
        || attempt.worker_id.is_empty()
    {
        return Err(
            "durable admission and attempt do not describe the same provider execution".into(),
        );
    }
    Ok(AgentExecutionIdentity {
        request_id: attempt.request_id.clone(),
        attempt_id: attempt.attempt_id.clone(),
        task_id: attempt.task_id.clone(),
        worker_id: attempt.worker_id.clone(),
        provider_id: attempt.provider_id.clone(),
        capability_id: attempt.capability_id.clone(),
        adapter_id: admission.adapter_id.clone(),
        adapter_version: admission.adapter_version.clone(),
        provider_registration_revision: admission.provider_registration_revision,
        idempotency_key: attempt.idempotency_key.clone(),
    })
}

/// Construct the adapter request only after the durable attempt exists.
/// The idempotency key and local task identity are taken from persisted state.
pub fn durable_agent_start_request(
    admission: &AdmissionRecord,
    attempt: &AttemptRecord,
    input: Value,
) -> Result<DurableAgentStartRequest, String> {
    let identity = agent_execution_identity(admission, attempt)?;
    let request = StartTaskRequest {
        task_id: attempt.task_id.clone(),
        capability_id: attempt.capability_id.clone(),
        input,
        idempotency_key: attempt.idempotency_key.clone(),
    };
    let durable = DurableAgentStartRequest { identity, request };
    durable.validate().map_err(|error| error.to_string())?;
    Ok(durable)
}

/// Convert a signed, adapter-observed lifecycle response to the persistence
/// type. Recompute the digest from the exact response object before retaining
/// the signature, so the returned object cannot be swapped independently of
/// its signed receipt.
pub fn provider_evidence_from_adapter_receipt<T: serde::Serialize>(
    provider_id: &str,
    adapter_id: &str,
    adapter_version: &str,
    registration_revision: i64,
    receipt: &AgentOperationReceipt<T>,
) -> Result<ProviderEvidence, String> {
    let signed = &receipt.evidence;
    let payload = &signed.payload;
    let identity = &payload.identity;
    if identity.provider_id != provider_id
        || identity.adapter_id != adapter_id
        || identity.adapter_version != adapter_version
        || identity.provider_registration_revision != registration_revision
    {
        return Err(
            "adapter receipt identity does not match the selected provider registration".into(),
        );
    }
    let recomputed_digest =
        response_sha256(&receipt.response).map_err(|error| error.to_string())?;
    if recomputed_digest != payload.response_sha256 {
        return Err("adapter receipt response digest does not match the returned response".into());
    }
    let evidence_kind = match payload.evidence_kind {
        AgentOperationEvidenceKind::Acknowledged => "acknowledged",
        AgentOperationEvidenceKind::Active => "active",
        AgentOperationEvidenceKind::Event => {
            return Err("provider event receipts must use the event persistence path".into());
        }
        AgentOperationEvidenceKind::Succeeded => "succeeded",
        AgentOperationEvidenceKind::Failed => "failed",
        AgentOperationEvidenceKind::Cancelled => "cancelled",
        AgentOperationEvidenceKind::CancellationAccepted => "cancellation_accepted",
        AgentOperationEvidenceKind::CancellationUnknown => "cancellation_unknown",
    };
    let observed_at = i64::try_from(payload.observed_at)
        .map_err(|_| "adapter receipt observation timestamp is out of range".to_string())?;
    let expires_at = payload
        .expires_at
        .map(i64::try_from)
        .transpose()
        .map_err(|_| "adapter receipt expiry timestamp is out of range".to_string())?;
    let payload_json = serde_json::to_string(payload)
        .map_err(|error| format!("cannot serialize signed adapter receipt: {error}"))?;

    Ok(ProviderEvidence {
        evidence_id: payload.evidence_id.clone(),
        evidence_kind: evidence_kind.into(),
        provider_id: provider_id.into(),
        adapter_id: adapter_id.into(),
        adapter_version: adapter_version.into(),
        provider_registration_revision: registration_revision,
        provider_execution_id: payload.provider_execution_id.clone(),
        verifier_id: payload.verifier_id.clone(),
        source_reference: payload.source_reference.clone(),
        observed_at,
        expires_at,
        receipt: Some(SignedProviderReceipt {
            key_id: signed.key_id.clone(),
            payload_json,
            signature_hex: signed.signature_hex.clone(),
            response_sha256: recomputed_digest,
        }),
    })
}

/// Owned event data ready for the canonical durable event transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableProviderEventEvidence {
    pub request_id: String,
    pub external_event_id: String,
    pub event_type: String,
    pub payload_json: String,
    pub receipt: SignedProviderReceipt,
}

impl DurableProviderEventEvidence {
    pub fn persistence_context(&self, now: i64) -> ProviderEventReceiptContext<'_> {
        ProviderEventReceiptContext {
            request_id: &self.request_id,
            external_event_id: &self.external_event_id,
            event_type: &self.event_type,
            payload_json: &self.payload_json,
            receipt: Some(&self.receipt),
            now,
        }
    }
}

/// Convert a typed adapter event receipt without accepting caller-supplied
/// event IDs, event types, payloads or digests.
pub fn provider_event_from_adapter_receipt(
    receipt: &AgentOperationReceipt<AgentEvent>,
) -> Result<DurableProviderEventEvidence, String> {
    let signed = &receipt.evidence;
    let payload = &signed.payload;
    let identity = &payload.identity;
    if payload.evidence_kind != AgentOperationEvidenceKind::Event
        || receipt.response.task_id != identity.task_id
        || receipt.response.sequence == 0
    {
        return Err("adapter event receipt has an invalid event kind or execution identity".into());
    }
    let provider_execution_id = payload
        .provider_execution_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "adapter event receipt has no provider execution ID".to_string())?;
    let external_event_id = payload
        .external_event_id
        .as_deref()
        .ok_or_else(|| "adapter event receipt has no external event ID".to_string())?;
    let event_type = payload
        .event_type
        .as_deref()
        .ok_or_else(|| "adapter event receipt has no event type".to_string())?;
    let expected_event_id = format!("{provider_execution_id}:{}", receipt.response.sequence);
    if external_event_id != expected_event_id || event_type != receipt.response.event_type {
        return Err("adapter event identity does not match its signed event response".into());
    }
    let recomputed_digest =
        response_sha256(&receipt.response).map_err(|error| error.to_string())?;
    if recomputed_digest != payload.response_sha256 {
        return Err("adapter event digest does not match the returned event".into());
    }
    let observed_at = i64::try_from(payload.observed_at)
        .map_err(|_| "adapter event timestamp is out of range".to_string())?;
    let expires_at = payload
        .expires_at
        .map(i64::try_from)
        .transpose()
        .map_err(|_| "adapter event expiry is out of range".to_string())?;
    let payload_json = serde_json::to_string(payload)
        .map_err(|error| format!("cannot serialize signed event receipt: {error}"))?;
    let _ = (observed_at, expires_at);
    Ok(DurableProviderEventEvidence {
        request_id: identity.request_id.clone(),
        external_event_id: external_event_id.to_owned(),
        event_type: event_type.to_owned(),
        payload_json: serde_json::to_string(&receipt.response)
            .map_err(|error| format!("cannot serialize provider event: {error}"))?,
        receipt: SignedProviderReceipt {
            key_id: signed.key_id.clone(),
            payload_json,
            signature_hex: signed.signature_hex.clone(),
            response_sha256: recomputed_digest,
        },
    })
}

impl ExecutionAuthority for Phase6ExecutionAuthority {
    fn authorize_dispatch(
        &self,
        connection: &Connection,
        request: &DispatchRequest,
        now: i64,
    ) -> Result<AuthorizationFacts, String> {
        if request.permission_scope != "agent:execute" {
            return Err("unsupported permission scope".into());
        }
        self.require_independent_verifier_ready(now)?;
        let capability_id: String = connection
            .query_row(
                "SELECT capability_id FROM tasks WHERE id = ?1",
                [&request.task_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "task not found".to_string())?;
        let (mission_id, _approval_sequence) =
            Self::approved_task(connection, &request.task_id, &request.requester_id, false)?;

        let capability_json: String = connection
            .query_row(
                "SELECT record_json FROM capabilities WHERE id = ?1",
                [&capability_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        let capability: CapabilityRecord =
            serde_json::from_str(&capability_json).map_err(|error| error.to_string())?;
        let provider_id = capability
            .adapter
            .as_deref()
            .ok_or_else(|| "agent capability has no provider adapter".to_string())?;
        let (manifest, registration, adapter_version) = self.registration(provider_id, now)?;
        validate_provider_receipt_key(&registration)?;
        let actual_revision = Self::capability_revision(connection, &capability_id, provider_id)?;
        if registration.capability_id != capability_id
            || registration.capability_revision != actual_revision
            || registration.operation != AGENT_OPERATION
        {
            return Err(
                "signed provider registration does not match current capability state".into(),
            );
        }
        if request.attempt_number > registration.retry_contract.max_attempts {
            return Err("verified retry contract attempt limit is exhausted".into());
        }
        let policy = self.policy.as_ref().ok_or_else(|| {
            "signed execution policy is not configured; provider dispatch is blocked".to_string()
        })?;
        let policy_decision = policy.authorize(
            ExecutionPolicyInput {
                request_id: &request.request_id,
                requester_id: &request.requester_id,
                task_id: &request.task_id,
                mission_id: mission_id.as_deref(),
                capability_id: &capability_id,
                operation: &registration.operation,
                permission_scope: &request.permission_scope,
                attempt_number: request.attempt_number,
            },
            now,
        )?;

        let expiry = now
            .saturating_add(ADMISSION_LIFETIME)
            .min(manifest.expires_at())
            .min(registration.expires_at)
            .min(registration.isolation.expires_at)
            .min(registration.retry_contract.expires_at)
            .min(policy_decision.expires_at);
        if expiry <= now {
            return Err("provider trust or retry contract expires before admission".into());
        }

        Ok(AuthorizationFacts {
            admission_id: uuid::Uuid::new_v4().to_string(),
            mission_id,
            provider_id: provider_id.to_owned(),
            capability_id,
            operation: registration.operation.clone(),
            idempotency_class: registration.retry_contract.idempotency_class.clone(),
            idempotency_key: format!("project-dread:{}", request.task_id),
            retry_contract_id: registration.retry_contract.contract_id.clone(),
            retry_contract_evidence_id: registration.retry_contract.evidence_id.clone(),
            retry_contract_revision: registration.retry_contract.revision,
            retry_contract_expires_at: registration.retry_contract.expires_at,
            retry_contract_sha256: registration.retry_contract.sha256.clone(),
            retry_contract_verifier_id: registration.retry_contract.verifier_id.clone(),
            retry_contract_source_reference: registration.retry_contract.source_reference.clone(),
            retry_contract_observed_at: registration.retry_contract.observed_at,
            idempotent_operation: registration.retry_contract.idempotent_operation,
            provider_keys_supported: registration.retry_contract.provider_keys_supported,
            max_attempts: registration.retry_contract.max_attempts,
            evidence_id: registration.isolation.evidence_id.clone(),
            policy_decision_id: policy_decision.decision_id,
            adapter_id: registration.adapter_id.clone(),
            adapter_version,
            provider_registration_revision: registration.registration_revision,
            capability_revision: actual_revision,
            expires_at: expiry,
        })
    }

    fn revalidate_dispatch(
        &self,
        connection: &Connection,
        admission: &AdmissionRecord,
        now: i64,
    ) -> Result<(), String> {
        if admission.expires_at <= now {
            return Err("dispatch admission expired".into());
        }
        self.require_independent_verifier_ready(now)?;
        self.verify_admission_scope(connection, admission, now)
    }

    fn verify_provider_evidence(
        &self,
        connection: &Connection,
        attempt: &AttemptRecord,
        evidence: &ProviderEvidence,
        now: i64,
    ) -> Result<(), String> {
        let admission = load_admission_for_authority(connection, &attempt.request_id)?;
        self.verify_admission_scope(connection, &admission, now)?;
        let (_, registration, _) = self.registration(&attempt.provider_id, now)?;
        let receipt = evidence.receipt.as_ref().ok_or_else(|| {
            "provider lifecycle evidence has no signed adapter receipt".to_string()
        })?;
        let payload =
            verify_signed_adapter_receipt(receipt, &registration, &admission, attempt, now)?;
        let expected_kind = evidence_kind_from_name(&evidence.evidence_kind)
            .ok_or_else(|| "provider evidence has an unsupported operation kind".to_string())?;
        let signed_observed_at = i64::try_from(payload.observed_at)
            .map_err(|_| "adapter receipt observation timestamp is out of range".to_string())?;
        let signed_expires_at = i64::try_from(
            payload
                .expires_at
                .ok_or_else(|| "adapter receipt has no expiry".to_string())?,
        )
        .map_err(|_| "adapter receipt expiry timestamp is out of range".to_string())?;
        if expected_kind == AgentOperationEvidenceKind::Event
            || payload.evidence_kind != expected_kind
            || payload.evidence_id != evidence.evidence_id
            || payload.provider_execution_id != evidence.provider_execution_id
            || payload.external_event_id.is_some()
            || payload.event_type.is_some()
            || evidence.provider_id != attempt.provider_id
            || evidence.adapter_id != registration.adapter_id
            || evidence.adapter_version != registration.adapter_version
            || evidence.provider_registration_revision != registration.registration_revision
            || evidence.verifier_id != payload.verifier_id
            || evidence.source_reference != payload.source_reference
            || evidence.observed_at != signed_observed_at
            || evidence.expires_at != Some(signed_expires_at)
        {
            return Err(
                "provider evidence does not match its signed operation receipt and durable attempt"
                    .into(),
            );
        }
        if let Some(execution_id) = evidence.provider_execution_id.as_deref()
            && attempt
                .provider_execution_id
                .as_deref()
                .is_some_and(|existing| existing != execution_id)
        {
            return Err(
                "provider evidence execution identity conflicts with the durable attempt".into(),
            );
        }
        if evidence.evidence_kind == "acknowledged"
            && evidence
                .provider_execution_id
                .as_deref()
                .is_none_or(str::is_empty)
        {
            return Err("provider acknowledgement evidence has no execution identity".into());
        }
        Ok(())
    }

    fn verify_provider_event(
        &self,
        connection: &Connection,
        attempt: &AttemptRecord,
        external_event_id: &str,
        event_type: &str,
        payload_json: &str,
        now: i64,
    ) -> Result<(), String> {
        let admission = load_admission_for_authority(connection, &attempt.request_id)?;
        self.verify_admission_scope(connection, &admission, now)?;
        if external_event_id.trim().is_empty()
            || external_event_id.len() > 1024
            || event_type.trim().is_empty()
            || event_type.len() > 256
            || payload_json.len() > 1_048_576
            || serde_json::from_str::<Value>(payload_json).is_err()
            || attempt.provider_execution_id.is_none()
        {
            return Err(
                "provider event is malformed or not attached to an acknowledged attempt".into(),
            );
        }
        let _ = self.registration(&attempt.provider_id, now)?;
        // This method is internal to the authenticated AgentAdapter polling path.
        // It does not accept HTTP-supplied evidence; the adapter's configured
        // bearer-authenticated transport is the provenance boundary.
        Ok(())
    }

    fn verify_provider_event_receipt(
        &self,
        connection: &Connection,
        attempt: &AttemptRecord,
        event: &ProviderEventReceiptContext<'_>,
    ) -> Result<(), String> {
        if event.request_id != attempt.request_id {
            return Err("provider event request ID does not match the durable attempt".into());
        }
        self.verify_provider_event(
            connection,
            attempt,
            event.external_event_id,
            event.event_type,
            event.payload_json,
            event.now,
        )?;
        let receipt = event
            .receipt
            .ok_or_else(|| "provider event has no signed adapter receipt".to_string())?;
        let admission = load_admission_for_authority(connection, &attempt.request_id)?;
        let (_, registration, _) = self.registration(&attempt.provider_id, event.now)?;
        let provider_event: AgentEvent = serde_json::from_str(event.payload_json)
            .map_err(|error| format!("signed provider event payload is invalid: {error}"))?;
        let provider_execution_id = attempt
            .provider_execution_id
            .as_deref()
            .ok_or_else(|| "provider event has no acknowledged execution identity".to_string())?;
        let expected_event_id = format!("{provider_execution_id}:{}", provider_event.sequence);
        if provider_event.task_id != attempt.task_id
            || provider_event.event_type != event.event_type
            || provider_event.sequence == 0
            || provider_event.occurred_at > u64::try_from(event.now).unwrap_or_default()
            || event.external_event_id != expected_event_id
        {
            return Err("provider event body does not match its durable event identity".into());
        }
        let response_bytes = serde_json::to_vec(&provider_event)
            .map_err(|error| format!("cannot serialize provider event for receipt: {error}"))?;
        let response_sha256 = hex_encode(digest::digest(&digest::SHA256, &response_bytes).as_ref());
        let payload =
            verify_signed_adapter_receipt(receipt, &registration, &admission, attempt, event.now)?;
        if payload.evidence_kind != AgentOperationEvidenceKind::Event
            || payload.provider_execution_id.as_deref() != Some(provider_execution_id)
            || payload.external_event_id.as_deref() != Some(event.external_event_id)
            || payload.event_type.as_deref() != Some(event.event_type)
            || payload.response_sha256 != response_sha256
        {
            return Err("provider event does not match its signed adapter receipt".into());
        }
        Ok(())
    }

    fn verify_independent_result(
        &self,
        connection: &Connection,
        attempt: &AttemptRecord,
        result_json: Option<&str>,
        provider_error: Option<&str>,
        now: i64,
    ) -> Result<VerificationFacts, String> {
        self.independent_verifier
            .as_ref()
            .ok_or_else(|| {
                "independent result verifier is not configured; provider success remains unverified"
                    .to_string()
            })?
            .verify_result(connection, attempt, result_json, provider_error, now)
    }

    fn verify_retry_contract_for_retry(
        &self,
        connection: &Connection,
        attempt: &AttemptRecord,
        now: i64,
    ) -> Result<CurrentRetryContractFacts, String> {
        let admission = load_admission_for_authority(connection, &attempt.request_id)?;
        self.verify_admission_scope(connection, &admission, now)?;
        let (_, registration, _) = self.registration(&attempt.provider_id, now)?;
        let contract = &registration.retry_contract;
        Ok(CurrentRetryContractFacts {
            contract_id: contract.contract_id.clone(),
            evidence_id: contract.evidence_id.clone(),
            provider_id: registration.provider_id.clone(),
            provider_registration_revision: registration.registration_revision,
            capability_id: registration.capability_id.clone(),
            capability_revision: registration.capability_revision,
            adapter_id: registration.adapter_id.clone(),
            adapter_version: registration.adapter_version.clone(),
            operation: registration.operation.clone(),
            contract_revision: contract.revision,
            expires_at: contract.expires_at,
            contract_sha256: contract.sha256.clone(),
            verifier_id: contract.verifier_id.clone(),
            source_reference: contract.source_reference.clone(),
            observed_at: contract.observed_at,
            idempotent_operation: contract.idempotent_operation,
            provider_keys_supported: contract.provider_keys_supported,
            revoked: registration.revoked || !registration.enabled,
            valid_at_observation: contract.observed_at <= now && contract.expires_at > now,
        })
    }
}

fn load_admission_for_authority(
    connection: &Connection,
    request_id: &str,
) -> Result<AdmissionRecord, String> {
    connection
        .query_row(
            "SELECT admission_id, request_id, task_id, mission_id, attempt_number,
                    requester_id, provider_id, capability_id, operation, permission_scope,
                    idempotency_key, evidence_id, policy_decision_id, adapter_id,
                    adapter_version, provider_registration_revision, capability_revision,
                    expires_at, consumed_at
             FROM dread_dispatch_admissions WHERE request_id = ?1",
            [request_id],
            |row| {
                Ok(AdmissionRecord {
                    admission_id: row.get(0)?,
                    request_id: row.get(1)?,
                    task_id: row.get(2)?,
                    mission_id: row.get(3)?,
                    attempt_number: row.get(4)?,
                    requester_id: row.get(5)?,
                    provider_id: row.get(6)?,
                    capability_id: row.get(7)?,
                    operation: row.get(8)?,
                    permission_scope: row.get(9)?,
                    idempotency_key: row.get(10)?,
                    evidence_id: row.get(11)?,
                    policy_decision_id: row.get(12)?,
                    adapter_id: row.get(13)?,
                    adapter_version: row.get(14)?,
                    provider_registration_revision: row.get(15)?,
                    capability_revision: row.get(16)?,
                    expires_at: row.get(17)?,
                    consumed_at: row.get(18)?,
                })
            },
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("dispatch admission '{request_id}' does not exist"))
}

/// Verify a DREAD adapter observation receipt against the public key pinned in
/// the currently signed provider registration and the exact durable attempt.
fn verify_signed_adapter_receipt(
    receipt: &SignedProviderReceipt,
    registration: &ProviderTrustRecord,
    admission: &AdmissionRecord,
    attempt: &AttemptRecord,
    now: i64,
) -> Result<AgentOperationEvidencePayload, String> {
    let trusted_key_id = registration
        .receipt_key_id
        .as_deref()
        .ok_or_else(|| "provider registration has no trusted adapter receipt key ID".to_string())?;
    let trusted_public_key_hex =
        registration
            .receipt_public_key_hex
            .as_deref()
            .ok_or_else(|| {
                "provider registration has no trusted adapter receipt public key".to_string()
            })?;
    if receipt.key_id != trusted_key_id {
        return Err(
            "adapter receipt key ID does not match the signed provider registration".into(),
        );
    }

    let public_key = decode_hex(trusted_public_key_hex)?;
    let signature_bytes = decode_hex(&receipt.signature_hex)?;
    if public_key.len() != 32 || signature_bytes.len() != 64 {
        return Err("adapter receipt Ed25519 key or signature has an invalid length".into());
    }
    signature::UnparsedPublicKey::new(&signature::ED25519, &public_key)
        .verify(receipt.payload_json.as_bytes(), &signature_bytes)
        .map_err(|_| "adapter receipt signature is invalid".to_string())?;

    let payload: AgentOperationEvidencePayload = serde_json::from_str(&receipt.payload_json)
        .map_err(|error| format!("adapter receipt payload is invalid: {error}"))?;
    let identity = &payload.identity;
    if attempt.request_id != admission.request_id
        || attempt.task_id != admission.task_id
        || attempt.provider_id != admission.provider_id
        || attempt.capability_id != admission.capability_id
        || attempt.operation != admission.operation
        || attempt.idempotency_key != admission.idempotency_key
        || admission.provider_id != registration.provider_id
        || admission.adapter_id != registration.adapter_id
        || admission.adapter_version != registration.adapter_version
        || admission.provider_registration_revision != registration.registration_revision
        || identity.request_id != admission.request_id
        || identity.attempt_id != attempt.attempt_id
        || identity.task_id != attempt.task_id
        || identity.worker_id != attempt.worker_id
        || identity.provider_id != admission.provider_id
        || identity.capability_id != admission.capability_id
        || identity.adapter_id != registration.adapter_id
        || identity.adapter_version != registration.adapter_version
        || identity.provider_registration_revision != registration.registration_revision
        || identity.idempotency_key != admission.idempotency_key
    {
        return Err(
            "adapter receipt identity does not match the durable admission and attempt".into(),
        );
    }

    let observed_at = i64::try_from(payload.observed_at)
        .map_err(|_| "adapter receipt observation timestamp is out of range".to_string())?;
    let expires_at = i64::try_from(
        payload
            .expires_at
            .ok_or_else(|| "adapter receipt has no expiry".to_string())?,
    )
    .map_err(|_| "adapter receipt expiry timestamp is out of range".to_string())?;
    if observed_at <= 0
        || observed_at > now
        || expires_at <= now
        || expires_at <= observed_at
        || expires_at > observed_at.saturating_add(MAX_PREFLIGHT_LIFETIME_SECONDS)
        || expires_at > registration.expires_at
    {
        return Err(
            "adapter receipt is future-dated, expired, or outside its lifetime bound".into(),
        );
    }

    let response_digest = decode_hex(&payload.response_sha256)?;
    if response_digest.len() != 32
        || hex_encode(&response_digest) != payload.response_sha256
        || receipt.response_sha256 != payload.response_sha256
    {
        return Err("adapter receipt response digest is invalid or inconsistent".into());
    }
    if payload.verifier_id != format!("dread-adapter-receipt:{}", registration.adapter_id)
        || payload.source_reference != registration.endpoint
    {
        return Err(
            "adapter receipt provenance does not match the signed adapter registration".into(),
        );
    }
    Ok(payload)
}

fn evidence_kind_from_name(value: &str) -> Option<AgentOperationEvidenceKind> {
    match value {
        "acknowledged" => Some(AgentOperationEvidenceKind::Acknowledged),
        "active" => Some(AgentOperationEvidenceKind::Active),
        "event" => Some(AgentOperationEvidenceKind::Event),
        "succeeded" => Some(AgentOperationEvidenceKind::Succeeded),
        "failed" => Some(AgentOperationEvidenceKind::Failed),
        "cancelled" => Some(AgentOperationEvidenceKind::Cancelled),
        "cancellation_accepted" => Some(AgentOperationEvidenceKind::CancellationAccepted),
        "cancellation_unknown" => Some(AgentOperationEvidenceKind::CancellationUnknown),
        _ => None,
    }
}

fn validate_manifest_payload(manifest: &ExecutionTrustManifest, now: i64) -> Result<(), String> {
    if manifest.schema_version != 1 {
        return Err("unsupported execution trust manifest schema version".into());
    }
    for (name, value, limit) in [
        ("manifest_id", manifest.manifest_id.as_str(), 160),
        ("verifier_id", manifest.verifier_id.as_str(), 160),
        ("source_reference", manifest.source_reference.as_str(), 2048),
    ] {
        validate_text(name, value, limit)?;
    }
    if manifest.observed_at < 0
        || manifest.observed_at > now
        || manifest.expires_at <= now
        || manifest.expires_at <= manifest.observed_at
        || manifest.expires_at > now.saturating_add(MAX_MANIFEST_LIFETIME)
        || manifest.providers.is_empty()
        || manifest.providers.len() > 16
    {
        return Err("execution trust manifest is expired, future-dated, or outside bounds".into());
    }
    let mut providers = std::collections::HashSet::new();
    for provider in &manifest.providers {
        match (
            provider.receipt_key_id.as_deref(),
            provider.receipt_public_key_hex.as_deref(),
        ) {
            (None, None) => {}
            (Some(key_id), Some(public_key_hex)) => {
                validate_text("receipt_key_id", key_id, 128)?;
                let public_key = decode_hex(public_key_hex)?;
                if public_key.len() != 32 {
                    return Err("provider receipt Ed25519 public key must be 32 bytes".into());
                }
            }
            _ => {
                return Err(
                    "provider receipt key ID and public key must be configured together".into(),
                );
            }
        }
        for (name, value, limit) in [
            ("provider_id", provider.provider_id.as_str(), 64),
            ("adapter_id", provider.adapter_id.as_str(), 128),
            ("adapter_version", provider.adapter_version.as_str(), 128),
            ("endpoint", provider.endpoint.as_str(), 2048),
            ("capability_id", provider.capability_id.as_str(), 160),
            ("operation", provider.operation.as_str(), 128),
            ("evidence_id", provider.evidence_id.as_str(), 160),
            ("source_reference", provider.source_reference.as_str(), 2048),
            (
                "isolation_evidence_id",
                provider.isolation.evidence_id.as_str(),
                160,
            ),
            (
                "runtime_identity",
                provider.isolation.runtime_identity.as_str(),
                512,
            ),
            (
                "isolation_source_observation_id",
                provider.isolation.provenance.source_observation_id.as_str(),
                1024,
            ),
            (
                "isolation_adapter_id",
                provider
                    .isolation
                    .provenance
                    .recorded_by_adapter_id
                    .as_str(),
                128,
            ),
            (
                "isolation_adapter_version",
                provider
                    .isolation
                    .provenance
                    .recorded_by_adapter_version
                    .as_str(),
                128,
            ),
            (
                "isolation_evidence_sha256",
                provider.isolation.provenance.evidence_sha256.as_str(),
                64,
            ),
            (
                "retry_contract_id",
                provider.retry_contract.contract_id.as_str(),
                160,
            ),
            (
                "retry_evidence_id",
                provider.retry_contract.evidence_id.as_str(),
                160,
            ),
            (
                "retry_verifier_id",
                provider.retry_contract.verifier_id.as_str(),
                160,
            ),
            (
                "retry_source_reference",
                provider.retry_contract.source_reference.as_str(),
                2048,
            ),
            ("retry_sha256", provider.retry_contract.sha256.as_str(), 64),
        ] {
            validate_text(name, value, limit)?;
        }
        let expected_capability = match provider.provider_id.as_str() {
            "hermes" => "hermes.run",
            "deerflow" => "deerflow.run",
            "openhands" => "openhands.run",
            _ => return Err("manifest contains an unsupported provider identifier".into()),
        };
        if !providers.insert(provider.provider_id.as_str())
            || provider.adapter_id != provider.provider_id
            || provider.capability_id != expected_capability
            || provider.operation != AGENT_OPERATION
            || provider.registration_revision <= 0
            || provider.capability_revision <= 0
            || provider.observed_at < 0
            || provider.observed_at > now
            || provider.expires_at <= now
            || provider.expires_at > manifest.expires_at
            || provider.isolation.contract_version != PREFLIGHT_CONTRACT_VERSION
            || provider.isolation.provider_id != provider.provider_id
            || provider.isolation.provider_registration_revision != provider.registration_revision
            || provider.isolation.adapter_id != provider.adapter_id
            || provider.isolation.adapter_version != provider.adapter_version
            || provider.isolation.provenance.recorded_by_adapter_id != provider.adapter_id
            || provider.isolation.provenance.recorded_by_adapter_version != provider.adapter_version
            || !provider.isolation.provenance.authenticated
            || provider.isolation.provenance.recorded_at < provider.isolation.checked_at
            || provider.isolation.provenance.recorded_at > now
            || provider.isolation.provenance.evidence_sha256.len() != 64
            || !provider
                .isolation
                .provenance
                .evidence_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || provider.isolation.checked_at < 0
            || provider.isolation.checked_at > now
            || provider.isolation.expires_at <= now
            || provider.isolation.expires_at <= provider.isolation.checked_at
            || provider.isolation.expires_at > provider.expires_at
            || provider.isolation.expires_at - provider.isolation.checked_at
                > MAX_PREFLIGHT_LIFETIME_SECONDS
            || !provider.isolation.all_verified()
        {
            return Err("provider registration scope or isolation evidence is invalid".into());
        }
        if !provider.endpoint.starts_with("https://")
            && !provider.endpoint.starts_with("http://127.0.0.1:")
            && !provider.endpoint.starts_with("http://localhost:")
            && !provider
                .endpoint
                .starts_with("http://host.docker.internal:")
        {
            return Err("provider endpoint must use HTTPS or an approved local endpoint".into());
        }
        let retry = &provider.retry_contract;
        if retry.revision <= 0
            || retry.observed_at < 0
            || retry.observed_at > now
            || retry.expires_at <= now
            || retry.expires_at > provider.expires_at
            || retry.max_attempts == 0
            || retry.max_attempts > 8
            || !matches!(
                retry.idempotency_class.as_str(),
                "idempotent" | "non_idempotent" | "provider_key_required" | "unknown"
            )
            || retry.sha256.len() != 64
            || !retry.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("provider retry contract is expired or invalid".into());
        }
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    fn signed_manifest(now: i64) -> (Vec<u8>, String) {
        let seed = [19_u8; 32];
        let key = Ed25519KeyPair::from_seed_unchecked(&seed).unwrap();
        let payload = ExecutionTrustManifest {
            schema_version: 1,
            manifest_id: "trust-manifest-test".into(),
            verifier_id: "dread-provisioner-test".into(),
            source_reference: "file:///test/execution-trust.json".into(),
            observed_at: now - 10,
            expires_at: now + 3600,
            providers: vec![ProviderTrustRecord {
                provider_id: "hermes".into(),
                adapter_id: "hermes".into(),
                adapter_version: "1.0.0".into(),
                endpoint: "http://127.0.0.1:8642".into(),
                registration_revision: 1,
                capability_id: "hermes.run".into(),
                capability_revision: 1,
                operation: AGENT_OPERATION.into(),
                receipt_key_id: None,
                receipt_public_key_hex: None,
                enabled: true,
                revoked: false,
                evidence_id: "isolation-evidence-1".into(),
                source_reference: "provisioner://hermes/isolation/1".into(),
                observed_at: now - 10,
                expires_at: now + 3600,
                isolation: IsolationEvidenceRecord {
                    contract_version: PREFLIGHT_CONTRACT_VERSION.into(),
                    evidence_id: "isolation-evidence-1".into(),
                    provider_id: "hermes".into(),
                    provider_registration_revision: 1,
                    adapter_id: "hermes".into(),
                    adapter_version: "1.0.0".into(),
                    runtime_identity: "hermes-runtime-1".into(),
                    provenance: EvidenceProvenance {
                        source_observation_id: "hermes-observation-1".into(),
                        recorded_by_adapter_id: "hermes".into(),
                        recorded_by_adapter_version: "1.0.0".into(),
                        evidence_sha256: "b".repeat(64),
                        recorded_at: now - 9,
                        authenticated: true,
                    },
                    checks: IsolationControl::ALL
                        .into_iter()
                        .map(|control| {
                            (
                                control,
                                ControlEvidence {
                                    verified: true,
                                    durable_observation_id: format!("{control:?}-observation"),
                                },
                            )
                        })
                        .collect(),
                    checked_at: now - 10,
                    expires_at: now + 290,
                },
                retry_contract: RetryContractTrust {
                    contract_id: "retry-hermes-agent-execute".into(),
                    evidence_id: "retry-evidence-1".into(),
                    revision: 1,
                    expires_at: now + 3600,
                    sha256: "a".repeat(64),
                    verifier_id: "dread-provisioner-test".into(),
                    source_reference: "provisioner://hermes/retry/1".into(),
                    observed_at: now - 10,
                    idempotency_class: "provider_key_required".into(),
                    idempotent_operation: true,
                    provider_keys_supported: true,
                    max_attempts: 3,
                },
            }],
        };
        let payload_bytes = serde_json::to_vec(&payload).unwrap();
        let signature_hex = hex_encode(key.sign(&payload_bytes).as_ref());
        let envelope = SignedExecutionTrustEnvelope {
            key_id: "test-key-1".into(),
            payload,
            signature_hex,
        };
        (
            serde_json::to_vec(&envelope).unwrap(),
            hex_encode(key.public_key().as_ref()),
        )
    }

    fn runtime_summary_for(
        registration: &ProviderTrustRecord,
    ) -> RuntimeIsolationAttestationSummary {
        RuntimeIsolationAttestationSummary {
            attestation_id: registration.isolation.evidence_id.clone(),
            attestor_id: "dread-runtime-observer-test".into(),
            provider_id: registration.provider_id.clone(),
            provider_registration_revision: registration.registration_revision,
            adapter_id: registration.adapter_id.clone(),
            adapter_version: registration.adapter_version.clone(),
            endpoint: registration.endpoint.clone(),
            runtime_identity: registration.isolation.runtime_identity.clone(),
            sandbox_backend: "modal".into(),
            source_observation_id: registration
                .isolation
                .provenance
                .source_observation_id
                .clone(),
            evidence_bundle_sha256: registration.isolation.provenance.evidence_sha256.clone(),
            observed_at: registration.isolation.checked_at,
            expires_at: registration.isolation.expires_at,
            checks: registration
                .isolation
                .checks
                .iter()
                .map(|(control, evidence)| {
                    (
                        isolation_control_name(*control).to_owned(),
                        evidence.durable_observation_id.clone(),
                    )
                })
                .collect(),
        }
    }

    #[test]
    fn runtime_observer_must_match_every_signed_manifest_observation() {
        let now = 1_800_000_000;
        let (encoded, public_key) = signed_manifest(now);
        let manifest = VerifiedExecutionTrustManifest::verify(&encoded, &public_key, now).unwrap();
        let registration = manifest.provider("hermes", now).unwrap();
        let summary = runtime_summary_for(registration);
        assert!(verify_runtime_isolation_binding("hermes", registration, &summary, now).is_ok());

        let mut changed_observation = summary.clone();
        changed_observation
            .checks
            .insert("egress_policy".into(), "different-observation".into());
        assert!(
            verify_runtime_isolation_binding("hermes", registration, &changed_observation, now)
                .is_err()
        );

        let mut changed_bundle = summary.clone();
        changed_bundle.evidence_bundle_sha256 = "c".repeat(64);
        assert!(
            verify_runtime_isolation_binding("hermes", registration, &changed_bundle, now).is_err()
        );

        let mut changed_revision = summary;
        changed_revision.provider_registration_revision += 1;
        assert!(
            verify_runtime_isolation_binding("hermes", registration, &changed_revision, now)
                .is_err()
        );
    }

    fn test_identity() -> AgentExecutionIdentity {
        AgentExecutionIdentity {
            request_id: "request-1".into(),
            attempt_id: "attempt-1".into(),
            task_id: "task-1".into(),
            worker_id: "worker-1".into(),
            provider_id: "hermes".into(),
            capability_id: "hermes.run".into(),
            adapter_id: "hermes".into(),
            adapter_version: "1.0.0".into(),
            provider_registration_revision: 7,
            idempotency_key: "project-dread:task-1".into(),
        }
    }

    #[test]
    fn durable_identity_and_start_request_are_derived_from_persisted_rows() {
        let admission = AdmissionRecord {
            admission_id: "admission-1".into(),
            request_id: "request-1".into(),
            task_id: "task-1".into(),
            mission_id: None,
            attempt_number: 1,
            requester_id: "operator-1".into(),
            provider_id: "hermes".into(),
            capability_id: "hermes.run".into(),
            operation: "agent.execute".into(),
            permission_scope: "agent:execute".into(),
            idempotency_key: "project-dread:task-1".into(),
            evidence_id: "isolation-1".into(),
            policy_decision_id: "policy-1".into(),
            adapter_id: "hermes".into(),
            adapter_version: "1.0.0".into(),
            provider_registration_revision: 7,
            capability_revision: 3,
            expires_at: 1_800_000_300,
            consumed_at: None,
        };
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
            provider_execution_id: None,
            provider_result_json: None,
            provider_error: None,
            outcome_json: None,
            dispatch_state: "dispatching".into(),
            cancellation_state: "none".into(),
            dispatched_at: 1_800_000_000,
            acknowledged_at: None,
            terminal_at: None,
            recovery_attached_at: None,
        };
        let identity = agent_execution_identity(&admission, &attempt).unwrap();
        assert_eq!(identity, test_identity());
        let request =
            durable_agent_start_request(&admission, &attempt, json!({"input":"hello"})).unwrap();
        assert_eq!(request.identity, identity);
        assert_eq!(request.request.idempotency_key, "project-dread:task-1");

        let mut mismatched = attempt;
        mismatched.operation = "agent.other".into();
        assert!(agent_execution_identity(&admission, &mismatched).is_err());
    }

    #[test]
    fn provider_receipt_conversion_recomputes_response_digest() {
        let response = json!({"provider_task_id":"remote-1","state":"running"});
        let identity = test_identity();
        let digest = response_sha256(&response).unwrap();
        let payload = AgentOperationEvidencePayload {
            evidence_id: "evidence-1".into(),
            evidence_kind: AgentOperationEvidenceKind::Acknowledged,
            identity,
            provider_execution_id: Some("remote-1".into()),
            external_event_id: None,
            event_type: None,
            response_sha256: digest,
            verifier_id: "dread-adapter-receipt:hermes".into(),
            source_reference: "http://127.0.0.1:8642".into(),
            observed_at: 1_800_000_000,
            expires_at: Some(1_800_000_300),
        };
        let receipt = AgentOperationReceipt {
            response: response.clone(),
            evidence: dread_agents::SignedAgentOperationEvidence {
                key_id: "hermes-receipt-key".into(),
                payload,
                signature_hex: "cd".repeat(64),
            },
        };
        assert!(
            provider_evidence_from_adapter_receipt("hermes", "hermes", "1.0.0", 7, &receipt)
                .is_ok()
        );
        let mut altered = receipt;
        altered.response = json!({"provider_task_id":"remote-2","state":"running"});
        assert!(
            provider_evidence_from_adapter_receipt("hermes", "hermes", "1.0.0", 7, &altered)
                .is_err()
        );
    }

    #[test]
    fn event_receipt_conversion_binds_event_id_type_and_response_digest() {
        let identity = test_identity();
        let response = AgentEvent {
            task_id: identity.task_id.clone(),
            sequence: 9,
            event_type: "progress".into(),
            payload: json!({"progress":75}),
            occurred_at: 1_800_000_001,
        };
        let digest = response_sha256(&response).unwrap();
        let payload = AgentOperationEvidencePayload {
            evidence_id: "event-evidence-1".into(),
            evidence_kind: AgentOperationEvidenceKind::Event,
            identity,
            provider_execution_id: Some("remote-1".into()),
            external_event_id: Some("remote-1:9".into()),
            event_type: Some("progress".into()),
            response_sha256: digest,
            verifier_id: "dread-adapter-receipt:hermes".into(),
            source_reference: "http://127.0.0.1:8642".into(),
            observed_at: 1_800_000_001,
            expires_at: Some(1_800_000_301),
        };
        let receipt = AgentOperationReceipt {
            response,
            evidence: dread_agents::SignedAgentOperationEvidence {
                key_id: "hermes-receipt-key".into(),
                payload,
                signature_hex: "ef".repeat(64),
            },
        };
        let owned = provider_event_from_adapter_receipt(&receipt).unwrap();
        assert_eq!(owned.request_id, "request-1");
        assert_eq!(owned.external_event_id, "remote-1:9");
        assert_eq!(owned.event_type, "progress");
        let context = owned.persistence_context(1_800_000_002);
        assert_eq!(context.request_id, "request-1");
        assert!(context.receipt.is_some());

        let mut altered = receipt;
        altered.response.payload = json!({"progress":1});
        assert!(provider_event_from_adapter_receipt(&altered).is_err());
    }

    #[test]
    fn adapter_receipt_conversion_preserves_signed_scope_and_rejects_events() {
        let identity = dread_agents::AgentExecutionIdentity {
            request_id: "request-1".into(),
            attempt_id: "attempt-1".into(),
            task_id: "task-1".into(),
            worker_id: "worker-1".into(),
            provider_id: "hermes".into(),
            capability_id: "hermes.run".into(),
            adapter_id: "hermes".into(),
            adapter_version: "1.0.0".into(),
            provider_registration_revision: 7,
            idempotency_key: "project-dread:task-1".into(),
        };
        let response = json!({"provider_task_id":"remote-1"});
        let payload = AgentOperationEvidencePayload {
            evidence_id: "evidence-1".into(),
            evidence_kind: AgentOperationEvidenceKind::Acknowledged,
            identity: identity.clone(),
            provider_execution_id: Some("remote-1".into()),
            external_event_id: None,
            event_type: None,
            response_sha256: response_sha256(&response).unwrap(),
            verifier_id: "dread-adapter-receipt:hermes".into(),
            source_reference: "http://127.0.0.1:8642".into(),
            observed_at: 1_800_000_000,
            expires_at: Some(1_800_000_300),
        };
        let receipt = AgentOperationReceipt {
            response,
            evidence: dread_agents::SignedAgentOperationEvidence {
                key_id: "hermes-receipt-key".into(),
                payload,
                signature_hex: "cd".repeat(64),
            },
        };
        let converted =
            provider_evidence_from_adapter_receipt("hermes", "hermes", "1.0.0", 7, &receipt)
                .unwrap();
        assert_eq!(converted.evidence_kind, "acknowledged");
        assert_eq!(converted.provider_execution_id.as_deref(), Some("remote-1"));
        assert_eq!(converted.provider_registration_revision, 7);
        assert_eq!(
            converted.receipt.as_ref().unwrap().key_id,
            "hermes-receipt-key"
        );

        let mut event_receipt = receipt;
        event_receipt.evidence.payload.evidence_kind = AgentOperationEvidenceKind::Event;
        assert!(
            provider_evidence_from_adapter_receipt("hermes", "hermes", "1.0.0", 7, &event_receipt)
                .is_err()
        );

        assert!(
            provider_evidence_from_adapter_receipt(
                "deerflow",
                "hermes",
                "1.0.0",
                7,
                &event_receipt
            )
            .is_err()
        );
    }

    #[test]
    fn verifies_signed_manifest_and_reports_only_public_metadata() {
        let now = 1_800_000_000;
        let (encoded, key) = signed_manifest(now);
        let verified = VerifiedExecutionTrustManifest::verify(&encoded, &key, now).unwrap();
        assert_eq!(verified.manifest_id(), "trust-manifest-test");
        assert_eq!(verified.providers().len(), 1);
        assert_eq!(verified.public_status(now)["status"], "verified");
        assert_eq!(
            verified.public_status(now)["independent_result_verification"],
            "not_configured_fail_closed"
        );
    }

    #[test]
    fn rejects_modified_payload_and_bad_signature() {
        let now = 1_800_000_000;
        let (encoded, key) = signed_manifest(now);
        let mut envelope: SignedExecutionTrustEnvelope = serde_json::from_slice(&encoded).unwrap();
        envelope.payload.providers[0].endpoint = "https://attacker.invalid".into();
        let tampered = serde_json::to_vec(&envelope).unwrap();
        assert!(VerifiedExecutionTrustManifest::verify(&tampered, &key, now).is_err());
    }

    fn enabled_test_capability() -> CapabilityRecord {
        CapabilityRecord {
            id: "hermes.run".into(),
            name: "Hermes runner".into(),
            kind: CapabilityKind::Agent,
            description: "Test agent capability".into(),
            version: Some("1.0.0".into()),
            license: Some("MIT".into()),
            provenance: dread_agents::CapabilityProvenance {
                source_name: "test registry".into(),
                source_uri: Some("https://example.invalid/hermes".into()),
                source_version: Some("1.0.0".into()),
                source_revision: Some("test-revision".into()),
                imported_at: 100,
            },
            compatibility: "compatible".into(),
            dependencies: Vec::new(),
            adapter: Some("hermes".into()),
            status: CapabilityStatus::Enabled,
            manifest: json!({
                "approval": {
                    "approved_by": "test-operator",
                    "approved_at": 100,
                    "reason": "verified test fixture"
                }
            }),
        }
    }

    #[test]
    fn capability_revision_requires_latest_enabled_audit_record() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE capabilities (id TEXT PRIMARY KEY, record_json TEXT NOT NULL);
                 CREATE TABLE capability_events (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    capability_id TEXT NOT NULL,
                    event_type TEXT NOT NULL,
                    actor TEXT NOT NULL
                 );",
            )
            .unwrap();
        let encoded = serde_json::to_string(&enabled_test_capability()).unwrap();
        connection
            .execute(
                "INSERT INTO capabilities(id, record_json) VALUES ('hermes.run', ?1)",
                [&encoded],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO capability_events(capability_id, event_type, actor)
                 VALUES ('hermes.run', 'enabled', 'test-operator')",
                [],
            )
            .unwrap();
        assert_eq!(
            Phase6ExecutionAuthority::capability_revision(&connection, "hermes.run", "hermes")
                .unwrap(),
            1
        );
        connection
            .execute(
                "INSERT INTO capability_events(capability_id, event_type, actor)
                 VALUES ('hermes.run', 'disabled', 'test-operator')",
                [],
            )
            .unwrap();
        assert!(
            Phase6ExecutionAuthority::capability_revision(&connection, "hermes.run", "hermes")
                .is_err()
        );
    }

    #[test]
    fn task_approval_is_bound_to_operator_and_current_state() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE tasks (
                    id TEXT PRIMARY KEY,
                    status TEXT NOT NULL,
                    task_type TEXT NOT NULL,
                    approved_by TEXT,
                    capability_id TEXT
                 );
                 CREATE TABLE task_events (
                    sequence INTEGER NOT NULL,
                    task_id TEXT NOT NULL,
                    event_type TEXT NOT NULL,
                    actor TEXT NOT NULL,
                    payload_json TEXT NOT NULL
                 );
                 CREATE TABLE mission_tasks (
                    mission_id TEXT NOT NULL,
                    task_id TEXT NOT NULL
                 );
                 INSERT INTO tasks(id, status, task_type, approved_by, capability_id)
                 VALUES ('task-1', 'queued', 'agent_task', 'test-operator', 'hermes.run');
                 INSERT INTO task_events(sequence, task_id, event_type, actor, payload_json)
                 VALUES (7, 'task-1', 'task_approved', 'test-operator',
                         '{\"approved_by\":\"test-operator\",\"reason\":\"approved\"}');",
            )
            .unwrap();
        assert_eq!(
            Phase6ExecutionAuthority::approved_task(&connection, "task-1", "test-operator", false)
                .unwrap(),
            (None, 7)
        );
        assert!(
            Phase6ExecutionAuthority::approved_task(
                &connection,
                "task-1",
                "different-operator",
                false
            )
            .is_err()
        );
        connection
            .execute(
                "UPDATE tasks SET status = 'running' WHERE id = 'task-1'",
                [],
            )
            .unwrap();
        assert!(
            Phase6ExecutionAuthority::approved_task(&connection, "task-1", "test-operator", false)
                .is_err()
        );
        assert!(
            Phase6ExecutionAuthority::approved_task(&connection, "task-1", "test-operator", true)
                .is_ok()
        );
    }

    #[test]
    fn rejects_expired_or_unsafe_isolation_manifest() {
        let now = 1_800_000_000;
        let (encoded, key) = signed_manifest(now);
        assert!(VerifiedExecutionTrustManifest::verify(&encoded, &key, now + 7200).is_err());

        let mut envelope: SignedExecutionTrustEnvelope = serde_json::from_slice(&encoded).unwrap();
        envelope.payload.providers[0]
            .isolation
            .checks
            .get_mut(&IsolationControl::EgressPolicy)
            .expect("signed test fixture includes egress evidence")
            .verified = false;
        let keypair = Ed25519KeyPair::from_seed_unchecked(&[19_u8; 32]).unwrap();
        let payload_bytes = serde_json::to_vec(&envelope.payload).unwrap();
        envelope.signature_hex = hex_encode(keypair.sign(&payload_bytes).as_ref());
        let unsafe_signed = serde_json::to_vec(&envelope).unwrap();
        assert!(VerifiedExecutionTrustManifest::verify(&unsafe_signed, &key, now).is_err());
    }
    fn signed_adapter_receipt_fixture(
        now: i64,
    ) -> (
        ProviderTrustRecord,
        AdmissionRecord,
        AttemptRecord,
        SignedProviderReceipt,
    ) {
        let (encoded, _) = signed_manifest(now);
        let mut manifest: SignedExecutionTrustEnvelope = serde_json::from_slice(&encoded).unwrap();
        let seed = [41_u8; 32];
        let keypair = Ed25519KeyPair::from_seed_unchecked(&seed).unwrap();
        let registration = &mut manifest.payload.providers[0];
        registration.receipt_key_id = Some("hermes-adapter-key-1".into());
        registration.receipt_public_key_hex = Some(hex_encode(keypair.public_key().as_ref()));
        let registration = registration.clone();

        let admission = AdmissionRecord {
            admission_id: "admission-1".into(),
            request_id: "request-1".into(),
            task_id: "task-1".into(),
            mission_id: None,
            attempt_number: 1,
            requester_id: "test-operator".into(),
            provider_id: "hermes".into(),
            capability_id: "hermes.run".into(),
            operation: AGENT_OPERATION.into(),
            permission_scope: "agent:execute".into(),
            idempotency_key: "project-dread:task-1".into(),
            evidence_id: "isolation-evidence-1".into(),
            policy_decision_id: "policy-decision-1".into(),
            adapter_id: "hermes".into(),
            adapter_version: "1.0.0".into(),
            provider_registration_revision: 1,
            capability_revision: 1,
            expires_at: now + 300,
            consumed_at: None,
        };
        let attempt = AttemptRecord {
            attempt_id: "attempt-1".into(),
            task_id: "task-1".into(),
            mission_id: None,
            attempt_number: 1,
            request_id: "request-1".into(),
            provider_id: "hermes".into(),
            capability_id: "hermes.run".into(),
            operation: AGENT_OPERATION.into(),
            idempotency_key: "project-dread:task-1".into(),
            worker_id: "worker-1".into(),
            provider_execution_id: None,
            provider_result_json: None,
            provider_error: None,
            outcome_json: None,
            dispatch_state: "prepared".into(),
            cancellation_state: "none".into(),
            dispatched_at: now - 1,
            acknowledged_at: None,
            terminal_at: None,
            recovery_attached_at: None,
        };

        let response_sha256 =
            hex_encode(digest::digest(&digest::SHA256, b"{\"accepted\":true}").as_ref());
        let payload = AgentOperationEvidencePayload {
            evidence_id: "request-1:attempt-1:acknowledged:1800000000".into(),
            evidence_kind: AgentOperationEvidenceKind::Acknowledged,
            identity: dread_agents::AgentExecutionIdentity {
                request_id: "request-1".into(),
                attempt_id: "attempt-1".into(),
                task_id: "task-1".into(),
                worker_id: "worker-1".into(),
                provider_id: "hermes".into(),
                capability_id: "hermes.run".into(),
                adapter_id: "hermes".into(),
                adapter_version: "1.0.0".into(),
                provider_registration_revision: 1,
                idempotency_key: "project-dread:task-1".into(),
            },
            provider_execution_id: Some("hermes-run-1".into()),
            external_event_id: None,
            event_type: None,
            response_sha256: response_sha256.clone(),
            verifier_id: "dread-adapter-receipt:hermes".into(),
            source_reference: "http://127.0.0.1:8642".into(),
            observed_at: now as u64,
            expires_at: Some(now.saturating_add(300) as u64),
        };
        let payload_json = serde_json::to_string(&payload).unwrap();
        let signature_hex = hex_encode(keypair.sign(payload_json.as_bytes()).as_ref());
        let receipt = SignedProviderReceipt {
            key_id: "hermes-adapter-key-1".into(),
            payload_json,
            signature_hex,
            response_sha256,
        };
        (registration, admission, attempt, receipt)
    }

    #[test]
    fn verifies_adapter_receipt_with_manifest_pinned_public_key_and_attempt_identity() {
        let now = 1_800_000_000;
        let (registration, admission, attempt, receipt) = signed_adapter_receipt_fixture(now);
        let payload =
            verify_signed_adapter_receipt(&receipt, &registration, &admission, &attempt, now)
                .unwrap();
        assert_eq!(
            payload.evidence_kind,
            AgentOperationEvidenceKind::Acknowledged
        );
        assert_eq!(payload.identity.attempt_id, "attempt-1");
    }

    #[test]
    fn rejects_tampered_or_wrong_identity_adapter_receipts() {
        let now = 1_800_000_000;
        let (registration, admission, attempt, receipt) = signed_adapter_receipt_fixture(now);

        let mut tampered = receipt.clone();
        tampered.payload_json.push(' ');
        assert!(
            verify_signed_adapter_receipt(&tampered, &registration, &admission, &attempt, now)
                .is_err()
        );

        let mut wrong_key = receipt.clone();
        wrong_key.key_id = "unregistered-key".into();
        assert!(
            verify_signed_adapter_receipt(&wrong_key, &registration, &admission, &attempt, now)
                .is_err()
        );

        let mut wrong_attempt = attempt.clone();
        wrong_attempt.worker_id = "different-worker".into();
        assert!(
            verify_signed_adapter_receipt(&receipt, &registration, &admission, &wrong_attempt, now)
                .is_err()
        );
    }
}
