//! Provider-neutral contracts for PROJECT DREAD agent adapters and capabilities.
//!
//! Provider adapters are explicit, loopback-restricted clients and are not
//! auto-discovered or registered by this crate. Callers must enforce DREAD
//! approval before task submission; imported capabilities start disabled.

pub mod extensions;

pub mod deerflow;
pub mod hermes;
pub mod openhands;

use futures::future::BoxFuture;
use ring::signature::KeyPair;
use ring::{digest, signature};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fmt,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentHealthState {
    Online,
    Degraded,
    Offline,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentHealth {
    pub state: AgentHealthState,
    pub checked_at: u64,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentDescriptor {
    pub id: String,
    pub display_name: String,
    pub adapter: String,
    pub version: Option<String>,
    pub endpoint: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentCapability {
    pub id: String,
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub output_schema: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartTaskRequest {
    pub task_id: String,
    pub capability_id: String,
    pub input: Value,
    pub idempotency_key: String,
}

/// Immutable identity supplied by DREAD for one durable provider attempt.
/// Adapters must copy these exact fields into every signed operation receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentExecutionIdentity {
    pub request_id: String,
    pub attempt_id: String,
    pub task_id: String,
    pub worker_id: String,
    pub provider_id: String,
    pub capability_id: String,
    pub adapter_id: String,
    pub adapter_version: String,
    pub provider_registration_revision: i64,
    pub idempotency_key: String,
}

/// Durable start request. The embedded legacy request must agree with identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DurableAgentStartRequest {
    pub identity: AgentExecutionIdentity,
    pub request: StartTaskRequest,
}

impl DurableAgentStartRequest {
    pub fn validate(&self) -> AdapterResult<()> {
        validate_execution_identity(&self.identity)?;
        if self.request.task_id != self.identity.task_id
            || self.request.capability_id != self.identity.capability_id
            || self.request.idempotency_key != self.identity.idempotency_key
        {
            return Err(durable_evidence_error(
                "durable start request does not match its immutable execution identity",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentOperationEvidenceKind {
    Acknowledged,
    Active,
    Event,
    Succeeded,
    Failed,
    Cancelled,
    CancellationAccepted,
    CancellationUnknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentOperationEvidencePayload {
    pub evidence_id: String,
    pub evidence_kind: AgentOperationEvidenceKind,
    pub identity: AgentExecutionIdentity,
    pub provider_execution_id: Option<String>,
    pub external_event_id: Option<String>,
    pub event_type: Option<String>,
    /// SHA-256 of the exact serialized response/event covered by this receipt.
    pub response_sha256: String,
    pub verifier_id: String,
    pub source_reference: String,
    pub observed_at: u64,
    pub expires_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedAgentOperationEvidence {
    pub key_id: String,
    pub payload: AgentOperationEvidencePayload,
    pub signature_hex: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentOperationReceipt<T> {
    pub response: T,
    pub evidence: SignedAgentOperationEvidence,
}

/// Optional provider identity fields associated with one signed adapter operation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentOperationReceiptDetails {
    pub provider_execution_id: Option<String>,
    pub external_event_id: Option<String>,
    pub event_type: Option<String>,
}

/// Per-adapter Ed25519 signer. Its private seed is never serialized or exposed
/// through Debug; adapter receipts contain only the public key ID and signature.
#[derive(Clone)]
pub struct AgentReceiptSigner {
    key_id: String,
    seed: [u8; 32],
}

impl AgentReceiptSigner {
    pub fn from_seed_hex(key_id: &str, seed_hex: &str) -> AdapterResult<Self> {
        validate_receipt_text("receipt key_id", key_id, 128)?;
        let decoded = decode_receipt_hex(seed_hex)
            .map_err(|_| durable_evidence_error("receipt signing seed is not valid hexadecimal"))?;
        let seed: [u8; 32] = decoded
            .try_into()
            .map_err(|_| durable_evidence_error("receipt signing seed must be exactly 32 bytes"))?;
        Ok(Self {
            key_id: key_id.to_owned(),
            seed,
        })
    }

    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    pub fn public_key_hex(&self) -> AdapterResult<String> {
        let keypair = signature::Ed25519KeyPair::from_seed_unchecked(&self.seed)
            .map_err(|_| durable_evidence_error("receipt signing seed is invalid"))?;
        Ok(encode_receipt_hex(keypair.public_key().as_ref()))
    }

    pub fn sign(
        &self,
        payload: AgentOperationEvidencePayload,
    ) -> AdapterResult<SignedAgentOperationEvidence> {
        validate_operation_evidence_payload(&payload)?;
        let payload_bytes = serde_json::to_vec(&payload)
            .map_err(|_| durable_evidence_error("cannot serialize receipt evidence payload"))?;
        let keypair = signature::Ed25519KeyPair::from_seed_unchecked(&self.seed)
            .map_err(|_| durable_evidence_error("receipt signing seed is invalid"))?;
        let signature_hex = encode_receipt_hex(keypair.sign(&payload_bytes).as_ref());
        Ok(SignedAgentOperationEvidence {
            key_id: self.key_id.clone(),
            payload,
            signature_hex,
        })
    }
}

impl Drop for AgentReceiptSigner {
    fn drop(&mut self) {
        self.seed.fill(0);
    }
}

pub fn response_sha256<T: Serialize>(response: &T) -> AdapterResult<String> {
    let encoded = serde_json::to_vec(response)
        .map_err(|_| durable_evidence_error("cannot serialize adapter response for receipt"))?;
    Ok(encode_receipt_hex(
        digest::digest(&digest::SHA256, &encoded).as_ref(),
    ))
}

/// Signs a receipt after a provider adapter has completed its authenticated
/// transport operation. This attests what the local adapter observed; it is
/// not a provider-origin signature or an independent result verification.
pub fn signed_agent_operation_receipt<T: Serialize>(
    signer: &AgentReceiptSigner,
    descriptor: &AgentDescriptor,
    expected_provider_id: &str,
    identity: &AgentExecutionIdentity,
    evidence_kind: AgentOperationEvidenceKind,
    response: T,
    details: AgentOperationReceiptDetails,
) -> AdapterResult<AgentOperationReceipt<T>> {
    validate_execution_identity(identity)?;
    if !descriptor.enabled
        || descriptor.adapter != identity.adapter_id
        || expected_provider_id != identity.provider_id
    {
        return Err(durable_evidence_error(
            "durable receipt identity does not match the enabled provider adapter",
        ));
    }
    let source_reference = descriptor
        .endpoint
        .as_deref()
        .ok_or_else(|| durable_evidence_error("adapter endpoint is unavailable for receipt"))?;
    let observed_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let expires_at = observed_at.saturating_add(300);
    let response_sha256 = response_sha256(&response)?;
    let event_suffix = external_event_id
        .as_deref()
        .unwrap_or(evidence_kind_name(evidence_kind));
    let evidence_id = format!(
        "{}:{}:{}:{}",
        identity.request_id, identity.attempt_id, event_suffix, observed_at
    );
    let payload = AgentOperationEvidencePayload {
        evidence_id,
        evidence_kind,
        identity: identity.clone(),
        provider_execution_id: details.provider_execution_id,
        external_event_id: details.external_event_id,
        event_type: details.event_type,
        response_sha256,
        verifier_id: format!("dread-adapter-receipt:{}", identity.adapter_id),
        source_reference: source_reference.to_owned(),
        observed_at,
        expires_at: Some(expires_at),
    };
    let evidence = signer.sign(payload)?;
    Ok(AgentOperationReceipt { response, evidence })
}

fn evidence_kind_name(kind: AgentOperationEvidenceKind) -> &'static str {
    match kind {
        AgentOperationEvidenceKind::Acknowledged => "acknowledged",
        AgentOperationEvidenceKind::Active => "active",
        AgentOperationEvidenceKind::Event => "event",
        AgentOperationEvidenceKind::Succeeded => "succeeded",
        AgentOperationEvidenceKind::Failed => "failed",
        AgentOperationEvidenceKind::Cancelled => "cancelled",
        AgentOperationEvidenceKind::CancellationAccepted => "cancellation_accepted",
        AgentOperationEvidenceKind::CancellationUnknown => "cancellation_unknown",
    }
}

fn validate_operation_evidence_payload(
    payload: &AgentOperationEvidencePayload,
) -> AdapterResult<()> {
    validate_execution_identity(&payload.identity)?;
    validate_receipt_text("evidence_id", &payload.evidence_id, 1024)?;
    validate_receipt_text("verifier_id", &payload.verifier_id, 512)?;
    validate_receipt_text("source_reference", &payload.source_reference, 2048)?;
    if payload.response_sha256.len() != 64
        || decode_receipt_hex(&payload.response_sha256).is_err()
        || payload.observed_at == 0
        || payload
            .expires_at
            .is_some_and(|expiry| expiry <= payload.observed_at)
    {
        return Err(durable_evidence_error(
            "receipt evidence digest or timestamp is invalid",
        ));
    }
    if matches!(
        payload.evidence_kind,
        AgentOperationEvidenceKind::Acknowledged
            | AgentOperationEvidenceKind::Active
            | AgentOperationEvidenceKind::Succeeded
            | AgentOperationEvidenceKind::Failed
            | AgentOperationEvidenceKind::Cancelled
            | AgentOperationEvidenceKind::CancellationAccepted
            | AgentOperationEvidenceKind::CancellationUnknown
    ) && payload
        .provider_execution_id
        .as_deref()
        .is_none_or(str::is_empty)
    {
        return Err(durable_evidence_error(
            "provider lifecycle evidence requires a provider execution ID",
        ));
    }
    if payload.evidence_kind == AgentOperationEvidenceKind::Event {
        validate_receipt_text(
            "external_event_id",
            payload.external_event_id.as_deref().unwrap_or_default(),
            1024,
        )?;
        validate_receipt_text(
            "event_type",
            payload.event_type.as_deref().unwrap_or_default(),
            256,
        )?;
        if payload
            .provider_execution_id
            .as_deref()
            .is_none_or(str::is_empty)
        {
            return Err(durable_evidence_error(
                "provider event evidence requires a provider execution ID",
            ));
        }
    }
    Ok(())
}

fn validate_execution_identity(identity: &AgentExecutionIdentity) -> AdapterResult<()> {
    for (name, value, limit) in [
        ("request_id", identity.request_id.as_str(), 160),
        ("attempt_id", identity.attempt_id.as_str(), 160),
        ("task_id", identity.task_id.as_str(), 512),
        ("worker_id", identity.worker_id.as_str(), 512),
        ("provider_id", identity.provider_id.as_str(), 64),
        ("capability_id", identity.capability_id.as_str(), 160),
        ("adapter_id", identity.adapter_id.as_str(), 128),
        ("adapter_version", identity.adapter_version.as_str(), 128),
        ("idempotency_key", identity.idempotency_key.as_str(), 1024),
    ] {
        validate_receipt_text(name, value, limit)?;
    }
    if identity.provider_registration_revision <= 0 {
        return Err(durable_evidence_error(
            "provider registration revision must be positive",
        ));
    }
    Ok(())
}

fn validate_receipt_text(name: &str, value: &str, limit: usize) -> AdapterResult<()> {
    if value.trim().is_empty() || value.len() > limit || value.contains('\0') {
        Err(durable_evidence_error(&format!(
            "{name} is empty, too long, or contains NUL"
        )))
    } else {
        Ok(())
    }
}

fn decode_receipt_hex(value: &str) -> Result<Vec<u8>, ()> {
    if value.is_empty() || !value.len().is_multiple_of(2) {
        return Err(());
    }
    let mut bytes = Vec::with_capacity(value.len() / 2);
    let (pairs, remainder) = value.as_bytes().as_chunks::<2>();
    if !remainder.is_empty() {
        return Err(());
    }
    for pair in pairs {
        let high = receipt_hex_nibble(pair[0]).ok_or(())?;
        let low = receipt_hex_nibble(pair[1]).ok_or(())?;
        bytes.push((high << 4) | low);
    }
    Ok(bytes)
}

fn receipt_hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn encode_receipt_hex(value: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(value.len() * 2);
    for byte in value {
        encoded.push(DIGITS[(byte >> 4) as usize] as char);
        encoded.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    encoded
}

pub fn durable_evidence_error(message: &str) -> AgentAdapterError {
    AgentAdapterError {
        code: "durable_evidence_invalid".into(),
        message: message.into(),
        retryable: false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentTaskState {
    Queued,
    Running,
    WaitingForConfirmation,
    Succeeded,
    Failed,
    Cancelled,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentTask {
    pub task_id: String,
    pub state: AgentTaskState,
    pub provider_task_id: Option<String>,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentEvent {
    pub task_id: String,
    pub sequence: u64,
    pub event_type: String,
    pub payload: Value,
    pub occurred_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentAdapterError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

impl fmt::Display for AgentAdapterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for AgentAdapterError {}

pub type AdapterResult<T> = Result<T, AgentAdapterError>;
pub type AdapterFuture<'a, T> = BoxFuture<'a, AdapterResult<T>>;

/// Contract implemented by reviewed, provider-specific agent adapters.
///
/// Implementations must honor cancellation and timeouts, avoid implicit retries
/// for non-idempotent operations, and report task state without hiding failures.
pub trait AgentAdapter: Send + Sync {
    fn descriptor(&self) -> &AgentDescriptor;
    fn health<'a>(&'a self) -> AdapterFuture<'a, AgentHealth>;
    fn capabilities<'a>(&'a self) -> AdapterFuture<'a, Vec<AgentCapability>>;
    fn start_task<'a>(&'a self, request: StartTaskRequest) -> AdapterFuture<'a, AgentTask>;

    /// Durable dispatch must use this method; adapters without a signed receipt
    /// implementation fail closed instead of falling back to start_task.
    fn start_task_with_evidence<'a>(
        &'a self,
        _request: DurableAgentStartRequest,
    ) -> AdapterFuture<'a, AgentOperationReceipt<AgentTask>> {
        Box::pin(async {
            Err(AgentAdapterError {
                code: "durable_evidence_unsupported".into(),
                message: "adapter has not implemented signed durable start receipts".into(),
                retryable: false,
            })
        })
    }

    fn task_status_with_evidence<'a>(
        &'a self,
        _identity: &'a AgentExecutionIdentity,
        _provider_execution_id: &'a str,
    ) -> AdapterFuture<'a, AgentOperationReceipt<AgentTask>> {
        Box::pin(async {
            Err(AgentAdapterError {
                code: "durable_evidence_unsupported".into(),
                message: "adapter has not implemented signed durable status receipts".into(),
                retryable: false,
            })
        })
    }

    fn events_with_evidence<'a>(
        &'a self,
        _identity: &'a AgentExecutionIdentity,
        _provider_execution_id: &'a str,
        _after_sequence: u64,
    ) -> AdapterFuture<'a, Vec<AgentOperationReceipt<AgentEvent>>> {
        Box::pin(async {
            Err(AgentAdapterError {
                code: "durable_evidence_unsupported".into(),
                message: "adapter has not implemented signed durable event receipts".into(),
                retryable: false,
            })
        })
    }

    fn cancel_task_with_evidence<'a>(
        &'a self,
        _identity: &'a AgentExecutionIdentity,
        _provider_execution_id: &'a str,
    ) -> AdapterFuture<'a, AgentOperationReceipt<AgentTask>> {
        Box::pin(async {
            Err(AgentAdapterError {
                code: "durable_evidence_unsupported".into(),
                message: "adapter has not implemented signed durable cancellation receipts".into(),
                retryable: false,
            })
        })
    }

    fn task_result_with_evidence<'a>(
        &'a self,
        _identity: &'a AgentExecutionIdentity,
        _provider_execution_id: &'a str,
    ) -> AdapterFuture<'a, AgentOperationReceipt<AgentTask>> {
        Box::pin(async {
            Err(AgentAdapterError {
                code: "durable_evidence_unsupported".into(),
                message: "adapter has not implemented signed durable result receipts".into(),
                retryable: false,
            })
        })
    }

    /// Provider lifecycle methods receive both stable identities. The local
    /// task ID is returned to DREAD callers; the provider ID is used only for
    /// the remote API path and must never be substituted for the local ID.
    fn task_status<'a>(
        &'a self,
        task_id: &'a str,
        provider_task_id: &'a str,
    ) -> AdapterFuture<'a, AgentTask>;
    fn events<'a>(
        &'a self,
        task_id: &'a str,
        provider_task_id: &'a str,
        after_sequence: u64,
    ) -> AdapterFuture<'a, Vec<AgentEvent>>;
    fn cancel_task<'a>(
        &'a self,
        task_id: &'a str,
        provider_task_id: &'a str,
    ) -> AdapterFuture<'a, AgentTask>;
    /// Accept or reject a provider action that is currently waiting for
    /// operator confirmation. Providers without interactive approval support
    /// fail closed by default.
    fn respond_to_confirmation<'a>(
        &'a self,
        _task_id: &'a str,
        _provider_task_id: &'a str,
        _accept: bool,
        _reason: &'a str,
    ) -> AdapterFuture<'a, ()> {
        Box::pin(async {
            Err(AgentAdapterError {
                code: "unsupported_confirmation".into(),
                message: "This provider does not support interactive confirmations".into(),
                retryable: false,
            })
        })
    }
    fn task_result<'a>(
        &'a self,
        task_id: &'a str,
        provider_task_id: &'a str,
    ) -> AdapterFuture<'a, AgentTask>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityKind {
    Agent,
    Skill,
    McpTool,
    OpenApiTool,
    CliTool,
    Workflow,
    ModelProvider,
    Service,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityStatus {
    Discovered,
    ReviewRequired,
    #[default]
    Disabled,
    Approved,
    Enabled,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityProvenance {
    pub source_name: String,
    pub source_uri: Option<String>,
    pub source_version: Option<String>,
    pub source_revision: Option<String>,
    pub imported_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityRecord {
    pub id: String,
    pub name: String,
    pub kind: CapabilityKind,
    pub description: String,
    pub version: Option<String>,
    pub license: Option<String>,
    pub provenance: CapabilityProvenance,
    pub compatibility: String,
    pub dependencies: Vec<String>,
    pub adapter: Option<String>,
    pub status: CapabilityStatus,
    pub manifest: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    Empty(&'static str),
    TooLong(&'static str),
    InvalidIdentifier,
    MissingProvenance,
    EnabledWithoutApproval,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty(field) => write!(f, "{field} must not be empty"),
            Self::TooLong(field) => write!(f, "{field} exceeds its maximum length"),
            Self::InvalidIdentifier => write!(
                f,
                "id must contain only lowercase letters, digits, '-', '_' or '.'"
            ),
            Self::MissingProvenance => write!(f, "source_name provenance is required"),
            Self::EnabledWithoutApproval => {
                write!(f, "capability cannot be enabled without an approval record")
            }
        }
    }
}

impl std::error::Error for ValidationError {}

impl CapabilityRecord {
    /// Validate metadata and enforce the safe-import default.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.id.trim().is_empty() || self.name.trim().is_empty() {
            return Err(ValidationError::Empty("id/name"));
        }
        if self.id.len() > 160 || self.name.len() > 200 || self.description.len() > 8_000 {
            return Err(ValidationError::TooLong("capability field"));
        }
        if !self
            .id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-_.".contains(&b))
        {
            return Err(ValidationError::InvalidIdentifier);
        }
        if self.provenance.source_name.trim().is_empty() {
            return Err(ValidationError::MissingProvenance);
        }
        if self.status == CapabilityStatus::Enabled {
            let approved = self
                .manifest
                .get("approval")
                .and_then(Value::as_object)
                .is_some_and(|a| {
                    a.get("approved_by")
                        .and_then(Value::as_str)
                        .is_some_and(|s| !s.trim().is_empty())
                        && a.get("approved_at").and_then(Value::as_u64).is_some()
                });
            if !approved {
                return Err(ValidationError::EnabledWithoutApproval);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> CapabilityRecord {
        CapabilityRecord {
            id: "pstack.skill.example".into(),
            name: "Example skill".into(),
            kind: CapabilityKind::Skill,
            description: "Imported for review".into(),
            version: Some("1.0.0".into()),
            license: Some("MIT".into()),
            provenance: CapabilityProvenance {
                source_name: "Pstack".into(),
                source_uri: Some("https://example.invalid".into()),
                source_version: Some("1.0.0".into()),
                source_revision: Some("abc123".into()),
                imported_at: 1,
            },
            compatibility: "review_required".into(),
            dependencies: vec![],
            adapter: None,
            status: CapabilityStatus::Disabled,
            manifest: json!({}),
        }
    }

    #[test]
    fn signed_adapter_receipt_binds_request_attempt_idempotency_and_response() {
        let signer =
            AgentReceiptSigner::from_seed_hex("hermes-receipt-key", &"2a".repeat(32)).unwrap();
        let identity = AgentExecutionIdentity {
            request_id: "request-1".into(),
            attempt_id: "attempt-1".into(),
            task_id: "task-1".into(),
            worker_id: "worker-1".into(),
            provider_id: "hermes".into(),
            capability_id: "hermes.run".into(),
            adapter_id: "hermes.api_server".into(),
            adapter_version: "1.0.0".into(),
            provider_registration_revision: 4,
            idempotency_key: "dread:task-1:attempt-1".into(),
        };
        let response = AgentTask {
            task_id: identity.task_id.clone(),
            state: AgentTaskState::Running,
            provider_task_id: Some("provider-run-7".into()),
            result: None,
            error: None,
            updated_at: 1_800_000_000,
        };
        let payload = AgentOperationEvidencePayload {
            evidence_id: "evidence-1".into(),
            evidence_kind: AgentOperationEvidenceKind::Acknowledged,
            identity: identity.clone(),
            provider_execution_id: response.provider_task_id.clone(),
            external_event_id: None,
            event_type: None,
            response_sha256: response_sha256(&response).unwrap(),
            verifier_id: "adapter:hermes.api_server".into(),
            source_reference: "http://127.0.0.1:8765".into(),
            observed_at: 1_800_000_000,
            expires_at: Some(1_800_000_300),
        };
        let signed = signer.sign(payload).unwrap();
        assert_eq!(signed.key_id, "hermes-receipt-key");
        assert_eq!(
            signed.payload.response_sha256,
            response_sha256(&response).unwrap()
        );

        let public_key = decode_receipt_hex(&signer.public_key_hex().unwrap()).unwrap();
        let signature_bytes = decode_receipt_hex(&signed.signature_hex).unwrap();
        let encoded = serde_json::to_vec(&signed.payload).unwrap();
        signature::UnparsedPublicKey::new(&signature::ED25519, &public_key)
            .verify(&encoded, &signature_bytes)
            .unwrap();

        let mut changed = signed.payload;
        changed.identity.attempt_id = "attempt-other".into();
        let changed_bytes = serde_json::to_vec(&changed).unwrap();
        assert!(
            signature::UnparsedPublicKey::new(&signature::ED25519, &public_key)
                .verify(&changed_bytes, &signature_bytes)
                .is_err()
        );
    }

    #[test]
    fn adapter_acknowledgement_receipt_requires_provider_execution_identity() {
        let signer = AgentReceiptSigner::from_seed_hex("receipt-key", &"31".repeat(32)).unwrap();
        let payload = AgentOperationEvidencePayload {
            evidence_id: "evidence-2".into(),
            evidence_kind: AgentOperationEvidenceKind::Acknowledged,
            identity: AgentExecutionIdentity {
                request_id: "request-1".into(),
                attempt_id: "attempt-1".into(),
                task_id: "task-1".into(),
                worker_id: "worker-1".into(),
                provider_id: "hermes".into(),
                capability_id: "hermes.run".into(),
                adapter_id: "hermes.api_server".into(),
                adapter_version: "1.0.0".into(),
                provider_registration_revision: 1,
                idempotency_key: "dread:task-1:attempt-1".into(),
            },
            provider_execution_id: None,
            external_event_id: None,
            event_type: None,
            response_sha256: "a".repeat(64),
            verifier_id: "adapter:hermes.api_server".into(),
            source_reference: "http://127.0.0.1:8765".into(),
            observed_at: 1_800_000_000,
            expires_at: None,
        };
        assert!(signer.sign(payload).is_err());
    }

    #[test]
    fn durable_start_request_must_match_execution_identity() {
        let request = DurableAgentStartRequest {
            identity: AgentExecutionIdentity {
                request_id: "request-1".into(),
                attempt_id: "attempt-1".into(),
                task_id: "task-1".into(),
                worker_id: "worker-1".into(),
                provider_id: "hermes".into(),
                capability_id: "hermes.run".into(),
                adapter_id: "hermes.api_server".into(),
                adapter_version: "1.0.0".into(),
                provider_registration_revision: 1,
                idempotency_key: "dread:task-1:attempt-1".into(),
            },
            request: StartTaskRequest {
                task_id: "task-else".into(),
                capability_id: "hermes.run".into(),
                input: Value::Null,
                idempotency_key: "dread:task-1:attempt-1".into(),
            },
        };
        assert!(request.validate().is_err());
    }

    #[test]
    fn imported_capabilities_validate_disabled_by_default() {
        assert!(sample().validate().is_ok());
    }

    #[test]
    fn rejects_path_like_or_uppercase_ids() {
        let mut item = sample();
        item.id = "../RunMe".into();
        assert_eq!(item.validate(), Err(ValidationError::InvalidIdentifier));
    }

    #[test]
    fn enabled_requires_a_recorded_approval() {
        let mut item = sample();
        item.status = CapabilityStatus::Enabled;
        assert_eq!(
            item.validate(),
            Err(ValidationError::EnabledWithoutApproval)
        );
        item.manifest =
            serde_json::json!({"approval": {"approved_by": "operator", "approved_at": 123}});
        assert!(item.validate().is_ok());
    }

    #[test]
    fn provenance_is_mandatory() {
        let mut item = sample();
        item.provenance.source_name.clear();
        assert_eq!(item.validate(), Err(ValidationError::MissingProvenance));
    }
}
