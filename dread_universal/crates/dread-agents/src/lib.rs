//! Provider-neutral contracts for PROJECT DREAD agent adapters and capabilities.
//!
//! Provider adapters are explicit, loopback-restricted clients and are not
//! auto-discovered or registered by this crate. Callers must enforce DREAD
//! approval before task submission; imported capabilities start disabled.

pub mod deerflow;
pub mod hermes;
pub mod openhands;

use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentTaskState {
    Queued,
    Running,
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
