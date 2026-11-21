//! Restart reconciliation for remote provider executions.
//!
//! The provider source is strictly read-only: `inspect` may query a known
//! provider execution ID or look up the original idempotency key, but it must
//! never submit, restart, or duplicate remote work. All SQLite operations are
//! short and occur after network inspection has completed; no transaction is
//! held across an `.await`.

use std::{future::Future, pin::Pin};

use rusqlite::Connection;
use serde_json::json;

use crate::persistence::{
    mark_reconciliation_required, pending_verification_results,
    record_independent_verification, record_provider_ack, record_provider_active,
    record_provider_terminal, release_startup_dispatch_barrier,
    schedule_retry_after_definite_failure, unresolved_attempts, AttemptRecord,
    ExecutionAuthority, PersistenceError, ProviderEvidence,
};
use crate::lifecycle::RetryDecision;

pub type ReconciliationFuture<'a> =
    Pin<Box<dyn Future<Output = Result<ProviderObservation, String>> + Send + 'a>>;

#[derive(Debug, Clone)]
pub struct ReconciliationQuery {
    pub request_id: String,
    pub task_id: String,
    pub provider_id: String,
    pub capability_id: String,
    pub operation: String,
    pub idempotency_key: String,
    pub provider_execution_id: Option<String>,
    pub cancellation_requested: bool,
}

/// Facts returned by a trusted, configured provider adapter after a read-only
/// status lookup. The adapter must authenticate the response and construct
/// evidence bound to this exact request/attempt. An HTTP 404 or timeout is not
/// a definite failure unless the provider contract proves no side effect.
#[derive(Debug, Clone)]
pub enum ProviderObservation {
    Active {
        acknowledgement: Option<ProviderEvidence>,
        evidence: ProviderEvidence,
    },
    Succeeded {
        acknowledgement: Option<ProviderEvidence>,
        evidence: ProviderEvidence,
        result_json: Option<String>,
        provider_error: Option<String>,
    },
    Failed {
        acknowledgement: Option<ProviderEvidence>,
        evidence: ProviderEvidence,
        result_json: Option<String>,
        provider_error: Option<String>,
    },
    Cancelled {
        acknowledgement: Option<ProviderEvidence>,
        evidence: ProviderEvidence,
        result_json: Option<String>,
        provider_error: Option<String>,
    },
    DefiniteFailureBeforeSideEffects {
        evidence: ProviderEvidence,
    },
    Unknown {
        reason: String,
    },
}

/// Implementations must perform only provider-side lookup/polling. In
/// particular, this trait has no `start_task`/`submit` method by design.
pub trait ProviderReconciliationSource: Send + Sync {
    fn inspect<'a>(&'a self, query: ReconciliationQuery) -> ReconciliationFuture<'a>;
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ReconciliationReport {
    pub inspected: usize,
    /// Provider status was confirmed active against its known remote ID.
    /// The API still must register the local polling/cancellation worker.
    pub provider_active_confirmed: usize,
    /// Active provider attempts still waiting for the API to spawn and record
    /// their local polling/cancellation workers.
    pub worker_attachment_pending: usize,
    pub provider_succeeded: usize,
    pub provider_failed: usize,
    pub provider_cancelled: usize,
    pub retries_scheduled: usize,
    pub retries_denied: usize,
    pub unresolved: usize,
    pub verification_completed: usize,
    pub verification_pending: usize,
    pub startup_dispatch_barrier_open: bool,
}

fn query_for(attempt: &AttemptRecord) -> ReconciliationQuery {
    ReconciliationQuery {
        request_id: attempt.request_id.clone(),
        task_id: attempt.task_id.clone(),
        provider_id: attempt.provider_id.clone(),
        capability_id: attempt.capability_id.clone(),
        operation: attempt.operation.clone(),
        idempotency_key: attempt.idempotency_key.clone(),
        provider_execution_id: attempt.provider_execution_id.clone(),
        cancellation_requested: attempt.cancellation_state != "none",
    }
}

fn persist_optional_acknowledgement(
    connection: &mut Connection,
    attempt: &AttemptRecord,
    acknowledgement: Option<ProviderEvidence>,
    authority: &dyn ExecutionAuthority,
    now: i64,
) -> Result<Option<String>, PersistenceError> {
    match acknowledgement {
        Some(evidence) => {
            let Some(provider_execution_id) = evidence.provider_execution_id.as_deref() else {
                return Err(PersistenceError::Invalid(
                    "provider acknowledgement evidence has no execution ID".into(),
                ));
            };
            record_provider_ack(
                connection,
                &attempt.request_id,
                provider_execution_id,
                &evidence,
                authority,
                now,
            )?;
            Ok(Some(provider_execution_id.to_owned()))
        }
        None => Ok(attempt.provider_execution_id.clone()),
    }
}

/// Reconcile every durable remote attempt before allowing new dispatch.
///
/// Provider lookups happen without a SQLite transaction. Every resulting state
/// change is then applied atomically through the persistence adapter. Unknown
/// outcomes remain nonterminal and keep the startup barrier closed. A provider
/// success is independently verified before the canonical task becomes
/// `completed`.
pub async fn reconcile_startup<S: ProviderReconciliationSource>(
    connection: &mut Connection,
    source: &S,
    authority: &dyn ExecutionAuthority,
    now: i64,
) -> Result<ReconciliationReport, PersistenceError> {
    let mut report = ReconciliationReport::default();
    let attempts = unresolved_attempts(connection)?;

    for attempt in attempts {
        // The remote operation is already terminal-successful. Its durable
        // result is handled by the independent-verification pass below; do not
        // ask the provider to restart or re-report work unnecessarily.
        if attempt.dispatch_state == "provider_succeeded" {
            continue;
        }

        report.inspected += 1;
        let observation = match source.inspect(query_for(&attempt)).await {
            Ok(observation) => observation,
            Err(error) => ProviderObservation::Unknown {
                reason: format!("provider reconciliation transport failure: {error}"),
            },
        };

        match observation {
            ProviderObservation::Active {
                acknowledgement,
                evidence,
            } => {
                let recovered_id = persist_optional_acknowledgement(
                    connection,
                    &attempt,
                    acknowledgement,
                    authority,
                    now,
                )?;
                if recovered_id.is_none() && evidence.provider_execution_id.is_none() {
                    mark_reconciliation_required(
                        connection,
                        &attempt.request_id,
                        &json!({
                            "reason": "provider reports active work but no durable execution ID could be recovered",
                            "attempt_id": attempt.attempt_id,
                        }).to_string(),
                        now,
                    )?;
                    report.unresolved += 1;
                    continue;
                }
                record_provider_active(
                    connection,
                    &attempt.request_id,
                    &evidence,
                    authority,
                    now,
                )?;
                report.provider_active_confirmed += 1;
            }
            ProviderObservation::Succeeded {
                acknowledgement,
                evidence,
                result_json,
                provider_error,
            } => {
                let _ = persist_optional_acknowledgement(
                    connection,
                    &attempt,
                    acknowledgement,
                    authority,
                    now,
                )?;
                record_provider_terminal(
                    connection,
                    &attempt.request_id,
                    "succeeded",
                    result_json.as_deref(),
                    provider_error.as_deref(),
                    &evidence,
                    authority,
                    now,
                )?;
                report.provider_succeeded += 1;
            }
            ProviderObservation::Failed {
                acknowledgement,
                evidence,
                result_json,
                provider_error,
            } => {
                let _ = persist_optional_acknowledgement(
                    connection,
                    &attempt,
                    acknowledgement,
                    authority,
                    now,
                )?;
                record_provider_terminal(
                    connection,
                    &attempt.request_id,
                    "failed",
                    result_json.as_deref(),
                    provider_error.as_deref(),
                    &evidence,
                    authority,
                    now,
                )?;
                report.provider_failed += 1;
            }
            ProviderObservation::Cancelled {
                acknowledgement,
                evidence,
                result_json,
                provider_error,
            } => {
                let _ = persist_optional_acknowledgement(
                    connection,
                    &attempt,
                    acknowledgement,
                    authority,
                    now,
                )?;
                record_provider_terminal(
                    connection,
                    &attempt.request_id,
                    "cancelled",
                    result_json.as_deref(),
                    provider_error.as_deref(),
                    &evidence,
                    authority,
                    now,
                )?;
                report.provider_cancelled += 1;
            }
            ProviderObservation::DefiniteFailureBeforeSideEffects { evidence } => {
                match schedule_retry_after_definite_failure(
                    connection,
                    &attempt.request_id,
                    &evidence,
                    authority,
                    now,
                ) {
                    Ok(RetryDecision::Allowed) => report.retries_scheduled += 1,
                    Ok(RetryDecision::Exhausted | RetryDecision::UnsafeContract) => {
                        report.retries_denied += 1;
                    }
                    Ok(RetryDecision::ReconciliationRequired) => report.unresolved += 1,
                    Err(error) => {
                        // A stale/revoked contract, cancellation race, or
                        // canonical-state conflict must not abort startup or
                        // convert a known remote result into an unsafe retry.
                        // Preserve a nonterminal hold for explicit resolution.
                        mark_reconciliation_required(
                            connection,
                            &attempt.request_id,
                            &json!({
                                "reason": "retry was not authorized after a definite pre-side-effect failure",
                                "detail": error.to_string(),
                                "attempt_id": attempt.attempt_id,
                            }).to_string(),
                            now,
                        )?;
                        report.retries_denied += 1;
                        report.unresolved += 1;
                    }
                }
            }
            ProviderObservation::Unknown { reason } => {
                mark_reconciliation_required(
                    connection,
                    &attempt.request_id,
                    &json!({
                        "reason": reason,
                        "attempt_id": attempt.attempt_id,
                        "provider_id": attempt.provider_id,
                        "provider_execution_id": attempt.provider_execution_id,
                    }).to_string(),
                    now,
                )?;
                report.unresolved += 1;
            }
        }
    }

    // Provider success and result bytes are already durable. Independent
    // verification may be retried locally without resubmitting remote work.
    for (request_id, result_json, provider_error) in pending_verification_results(connection)? {
        match record_independent_verification(
            connection,
            &request_id,
            result_json.as_deref(),
            provider_error.as_deref(),
            authority,
            now,
        ) {
            Ok(()) => report.verification_completed += 1,
            Err(_) => report.verification_pending += 1,
        }
    }

    report.worker_attachment_pending =
        crate::persistence::recovery_worker_attachment_candidates(connection)?.len();
    // This opens only when no unresolved remote operation remains OR every
    // active one has had its local worker registered by the API. Reconciliation
    // itself never claims that spawning a worker has already happened.
    report.startup_dispatch_barrier_open =
        release_startup_dispatch_barrier(connection, now)?;
    if !report.startup_dispatch_barrier_open {
        report.unresolved = report.unresolved.max(
            unresolved_attempts(connection)?.iter()
                .filter(|attempt| attempt.recovery_attached_at.is_none()
                    && attempt.dispatch_state != "provider_succeeded")
                .count(),
        );
    }
    Ok(report)
}