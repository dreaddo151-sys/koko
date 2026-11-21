//! Durable provider-task lifecycle rules for PROJECT DREAD.
//!
//! This module is intentionally independent of the HTTP/API crate. It does not
//! own dispatch, provider connections, SQLite, or GPU resources. The API and
//! persistence adapter must obtain provider evidence from trusted adapters.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissionState {
    Ready,
    Running,
    ReconciliationRequired,
    Succeeded,
    Failed,
    Cancelled,
    Blocked,
}

impl MissionState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Running => "running",
            Self::ReconciliationRequired => "reconciliation_required",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Blocked => "blocked",
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled | Self::Blocked)
    }

    pub fn can_transition_to(self, next: Self) -> bool {
        use MissionState::*;
        if self == next {
            return true;
        }
        matches!(
            (self, next),
            (Ready, Running | ReconciliationRequired | Failed | Cancelled | Blocked)
                | (Running, ReconciliationRequired | Succeeded | Failed | Cancelled | Blocked)
                | (ReconciliationRequired, Running | Succeeded | Failed | Cancelled | Blocked)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Pending,
    Ready,
    Dispatching,
    Running,
    RetryScheduled,
    CancelRequested,
    Cancelling,
    ReconciliationRequired,
    ProviderSucceeded,
    Verifying,
    Succeeded,
    Failed,
    Cancelled,
    Blocked,
}

impl TaskState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Ready => "ready",
            Self::Dispatching => "dispatching",
            Self::Running => "running",
            Self::RetryScheduled => "retry_scheduled",
            Self::CancelRequested => "cancel_requested",
            Self::Cancelling => "cancelling",
            Self::ReconciliationRequired => "reconciliation_required",
            Self::ProviderSucceeded => "provider_succeeded",
            Self::Verifying => "verifying",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Blocked => "blocked",
        }
    }

    pub fn parse(value: &str) -> Result<Self, LifecycleError> {
        match value {
            "pending" => Ok(Self::Pending),
            "ready" => Ok(Self::Ready),
            "dispatching" => Ok(Self::Dispatching),
            "running" => Ok(Self::Running),
            "retry_scheduled" => Ok(Self::RetryScheduled),
            "cancel_requested" => Ok(Self::CancelRequested),
            "cancelling" => Ok(Self::Cancelling),
            "reconciliation_required" => Ok(Self::ReconciliationRequired),
            "provider_succeeded" => Ok(Self::ProviderSucceeded),
            "verifying" => Ok(Self::Verifying),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            "blocked" => Ok(Self::Blocked),
            other => Err(LifecycleError::InvalidStoredState(other.to_owned())),
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled | Self::Blocked)
    }

    /// True while the remote provider outcome is not yet known. Provider
    /// success is a known remote outcome even though DREAD still needs local
    /// independent verification; keep that separate from remote reattachment.
    pub const fn has_unresolved_provider_outcome(self) -> bool {
        matches!(
            self,
            Self::Dispatching
                | Self::Running
                | Self::CancelRequested
                | Self::Cancelling
                | Self::ReconciliationRequired
        )
    }

    /// A provider-success result is durable but not yet a terminal DREAD
    /// success. These states require the independent verification pass.
    pub const fn requires_independent_verification(self) -> bool {
        matches!(self, Self::ProviderSucceeded | Self::Verifying)
    }

    /// Explicit forward-only transition table. Reconciliation never moves a
    /// task backwards to Ready/Pending or directly to a new dispatch attempt.
    pub fn can_transition_to(self, next: Self) -> bool {
        use TaskState::*;
        if self == next {
            return true;
        }
        matches!(
            (self, next),
            (Pending, Ready | Cancelled | Blocked)
                | (Ready, Dispatching | CancelRequested | Cancelled | Blocked)
                | (RetryScheduled, Ready | Dispatching | CancelRequested | Cancelled | Blocked)
                | (Dispatching, Running | CancelRequested | ReconciliationRequired | RetryScheduled | ProviderSucceeded | Failed)
                | (Running, CancelRequested | Cancelling | ReconciliationRequired | ProviderSucceeded | RetryScheduled | Failed)
                | (CancelRequested, Cancelling | ReconciliationRequired | ProviderSucceeded | Failed | Cancelled)
                | (Cancelling, ReconciliationRequired | ProviderSucceeded | Failed | Cancelled)
                | (ReconciliationRequired, Running | CancelRequested | Cancelling | ProviderSucceeded | Failed | Cancelled)
                | (ProviderSucceeded, Verifying | ReconciliationRequired)
                | (Verifying, Succeeded | Failed | ReconciliationRequired)
        )
    }

    pub fn transition(self, next: Self) -> Result<Self, LifecycleError> {
        if self.can_transition_to(next) {
            Ok(next)
        } else {
            Err(LifecycleError::InvalidTransition { from: self, to: next })
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdempotencyClass {
    Idempotent,
    NonIdempotent,
    ProviderKeyRequired,
    Unknown,
}

impl IdempotencyClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Idempotent => "idempotent",
            Self::NonIdempotent => "non_idempotent",
            Self::ProviderKeyRequired => "provider_key_required",
            Self::Unknown => "unknown",
        }
    }

    pub fn parse(value: &str) -> Result<Self, LifecycleError> {
        match value {
            "idempotent" => Ok(Self::Idempotent),
            "non_idempotent" => Ok(Self::NonIdempotent),
            "provider_key_required" => Ok(Self::ProviderKeyRequired),
            "unknown" => Ok(Self::Unknown),
            other => Err(LifecycleError::InvalidStoredState(other.to_owned())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteOutcome {
    /// Trusted provider evidence establishes that no side effect occurred.
    DefiniteFailureBeforeSideEffects,
    /// A timeout, transport error, NotFound ambiguity, or partial response.
    Unknown,
    /// Provider still reports the operation as active.
    StillActive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryDecision {
    Allowed,
    Exhausted,
    ReconciliationRequired,
    UnsafeContract,
}

/// Facts returned by a trusted server-side retry-contract verifier.
/// A boolean/digest supplied by an API request is not this verified record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifiedRetryContract {
    pub idempotent_operation: bool,
    pub provider_keys_supported: bool,
    pub revoked: bool,
    pub valid_at_observation: bool,
}

pub fn classify_retry(
    class: IdempotencyClass,
    outcome: RemoteOutcome,
    attempt: u32,
    max_attempts: u32,
    contract: VerifiedRetryContract,
) -> RetryDecision {
    match outcome {
        RemoteOutcome::Unknown | RemoteOutcome::StillActive => {
            return RetryDecision::ReconciliationRequired;
        }
        RemoteOutcome::DefiniteFailureBeforeSideEffects => {}
    }

    if !contract.valid_at_observation || contract.revoked {
        return RetryDecision::UnsafeContract;
    }
    if max_attempts == 0 || attempt >= max_attempts {
        return RetryDecision::Exhausted;
    }

    let permitted = match class {
        IdempotencyClass::Idempotent => contract.idempotent_operation,
        IdempotencyClass::NonIdempotent => true,
        IdempotencyClass::ProviderKeyRequired => {
            contract.idempotent_operation && contract.provider_keys_supported
        }
        IdempotencyClass::Unknown => false,
    };
    if permitted { RetryDecision::Allowed } else { RetryDecision::UnsafeContract }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifecycleError {
    InvalidTransition { from: TaskState, to: TaskState },
    InvalidStoredState(String),
}

impl fmt::Display for LifecycleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTransition { from, to } => {
                write!(f, "invalid task transition from {} to {}", from.as_str(), to.as_str())
            }
            Self::InvalidStoredState(value) => write!(f, "invalid stored lifecycle state: {value}"),
        }
    }
}

impl std::error::Error for LifecycleError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_unknown_and_still_active_never_become_retryable() {
        let verified = VerifiedRetryContract {
            idempotent_operation: true,
            provider_keys_supported: true,
            revoked: false,
            valid_at_observation: true,
        };
        for outcome in [RemoteOutcome::Unknown, RemoteOutcome::StillActive] {
            assert_eq!(
                classify_retry(IdempotencyClass::Idempotent, outcome, 1, 3, verified),
                RetryDecision::ReconciliationRequired
            );
        }
    }

    #[test]
    fn provider_key_required_needs_trusted_key_support() {
        let mut verified = VerifiedRetryContract {
            idempotent_operation: true,
            provider_keys_supported: false,
            revoked: false,
            valid_at_observation: true,
        };
        assert_eq!(
            classify_retry(
                IdempotencyClass::ProviderKeyRequired,
                RemoteOutcome::DefiniteFailureBeforeSideEffects,
                1,
                3,
                verified,
            ),
            RetryDecision::UnsafeContract
        );
        verified.provider_keys_supported = true;
        assert_eq!(
            classify_retry(
                IdempotencyClass::ProviderKeyRequired,
                RemoteOutcome::DefiniteFailureBeforeSideEffects,
                1,
                3,
                verified,
            ),
            RetryDecision::Allowed
        );
    }

    #[test]
    fn terminal_states_cannot_reopen() {
        for terminal in [TaskState::Succeeded, TaskState::Failed, TaskState::Cancelled, TaskState::Blocked] {
            assert!(terminal.is_terminal());
            assert!(!terminal.can_transition_to(TaskState::Running));
            assert!(!terminal.can_transition_to(TaskState::Ready));
        }
    }

    #[test]
    fn reconciliation_cannot_redispatch_or_move_back_to_pending() {
        assert!(!TaskState::ReconciliationRequired.can_transition_to(TaskState::Pending));
        assert!(!TaskState::ReconciliationRequired.can_transition_to(TaskState::Ready));
        assert!(!TaskState::ReconciliationRequired.can_transition_to(TaskState::Dispatching));
        assert!(!TaskState::ReconciliationRequired.can_transition_to(TaskState::RetryScheduled));
        assert!(TaskState::ReconciliationRequired.can_transition_to(TaskState::Running));
    }

    #[test]
    fn provider_success_during_cancellation_still_requires_verification() {
        assert!(TaskState::Cancelling.can_transition_to(TaskState::ProviderSucceeded));
        assert!(!TaskState::ProviderSucceeded.is_terminal());
        assert!(TaskState::ProviderSucceeded.can_transition_to(TaskState::Verifying));
        assert!(TaskState::Verifying.can_transition_to(TaskState::Succeeded));
    }

    #[test]
    fn mission_reconciliation_is_not_terminal() {
        assert!(!MissionState::ReconciliationRequired.is_terminal());
        assert!(MissionState::Running.can_transition_to(MissionState::ReconciliationRequired));
    }

    #[test]
    fn provider_success_is_not_mistaken_for_remote_inflight_work() {
        assert!(!TaskState::ProviderSucceeded.has_unresolved_provider_outcome());
        assert!(!TaskState::Verifying.has_unresolved_provider_outcome());
        assert!(TaskState::ProviderSucceeded.requires_independent_verification());
        assert!(TaskState::Verifying.requires_independent_verification());
        assert!(TaskState::ReconciliationRequired.has_unresolved_provider_outcome());
    }
}