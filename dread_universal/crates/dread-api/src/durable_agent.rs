//! Durable provider dispatch and evidence-backed lifecycle worker.
//!
//! Provider start is issued only after the canonical Phase 6 admission and
//! attempt have been persisted. Replays never call start again. An uncertain
//! transport outcome remains nonterminal and is left for reconciliation.

use crate::{
    AgentTaskExecution, AppState, TaskEvent,
    execution_authority::{
        agent_execution_identity, durable_agent_start_request, provider_event_from_adapter_receipt,
        provider_evidence_from_adapter_receipt,
    },
};
use dread_agents::{AgentAdapter, AgentTaskState};
use dread_orchestrator::persistence::{
    self, DispatchPreparation, DispatchRequest, EventAppendResult, ProviderTerminalPayload,
};
use serde_json::{Value, json};
use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::watch;
use tracing::{info, warn};
use uuid::Uuid;

struct PreparedDurableDispatch {
    request_id: String,
    identity: dread_agents::AgentExecutionIdentity,
    start_request: dread_agents::DurableAgentStartRequest,
}

fn now_epoch_i64() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .try_into()
        .unwrap_or(i64::MAX)
}

fn mark_reconciliation_required(state: &AppState, request_id: &str, reason: &str) {
    let now = now_epoch_i64();
    let reason_json = json!({
        "reason": reason,
        "source": "durable-agent-worker",
        "request_id": request_id
    })
    .to_string();
    let Ok(mut connection) = state.db.lock() else {
        warn!(
            request_id,
            "Could not acquire the database lock to record reconciliation"
        );
        return;
    };
    if let Err(error) =
        persistence::mark_reconciliation_required(&mut connection, request_id, &reason_json, now)
    {
        warn!(request_id, error = %error, "Could not persist reconciliation-required state");
    }
}

/// Admission and attempt creation happen after the shared ExecutionGate grants
/// this worker a slot, and before the provider side effect. Only Created may
/// submit a remote start; Existing is deliberately a no-start replay.
fn prepare_new_attempt(
    state: &AppState,
    execution: &AgentTaskExecution,
) -> Result<Option<PreparedDurableDispatch>, String> {
    let authority = state
        .execution_authority
        .as_ref()
        .ok_or_else(|| "signed Phase 6 execution authority is not configured".to_string())?;
    let mut connection = state
        .db
        .lock()
        .map_err(|_| "database lock is unavailable".to_string())?;

    let (status, requester_id): (String, Option<String>) = connection
        .query_row(
            "SELECT status, approved_by FROM tasks WHERE id = ?1",
            [&execution.task_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|error| format!("cannot read canonical agent task: {error}"))?;
    if status != "queued" {
        return Err(format!(
            "durable dispatch requires a queued task, found '{status}'"
        ));
    }
    let requester_id = requester_id
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "canonical task has no recorded approval identity".to_string())?;
    let attempt_number: u32 = connection
        .query_row(
            "SELECT CAST(COALESCE(MAX(attempt_number), 0) + 1 AS INTEGER)
             FROM dread_task_attempts WHERE task_id = ?1",
            [&execution.task_id],
            |row| row.get(0),
        )
        .map_err(|error| format!("cannot determine durable attempt number: {error}"))?;

    let request = DispatchRequest {
        request_id: Uuid::new_v4().to_string(),
        task_id: execution.task_id.clone(),
        requester_id,
        worker_id: format!("dread-api-worker-{}", Uuid::new_v4()),
        attempt_number,
        permission_scope: "agent:execute".into(),
    };
    let now = now_epoch_i64();
    let admission =
        persistence::authorize_and_persist(&mut connection, &request, authority.as_ref(), now)
            .map_err(|error| format!("durable provider admission was rejected: {error}"))?;
    let preparation =
        persistence::prepare_dispatch(&mut connection, &request, authority.as_ref(), now)
            .map_err(|error| format!("durable provider attempt preparation failed: {error}"))?;

    let attempt = match preparation {
        DispatchPreparation::Created(attempt) => attempt,
        DispatchPreparation::Existing(attempt) => {
            info!(
                task_id = %attempt.task_id,
                request_id = %attempt.request_id,
                attempt_id = %attempt.attempt_id,
                "Durable dispatch replay found an existing attempt; provider start will not be repeated"
            );
            return Ok(None);
        }
    };
    let identity = agent_execution_identity(&admission, &attempt)?;
    if identity.provider_id != execution.provider
        || identity.capability_id != execution.capability_id
    {
        return Err(
            "selected API adapter does not match the canonical durable provider identity".into(),
        );
    }
    let start_request =
        durable_agent_start_request(&admission, &attempt, execution.payload.clone())?;
    Ok(Some(PreparedDurableDispatch {
        request_id: request.request_id,
        identity,
        start_request,
    }))
}

fn persist_provider_event(
    state: &AppState,
    request_id: &str,
    event_receipt: &dread_agents::AgentOperationReceipt<dread_agents::AgentEvent>,
) -> Result<(), String> {
    let owned = provider_event_from_adapter_receipt(event_receipt)?;
    let now = now_epoch_i64();
    let event: dread_agents::AgentEvent = serde_json::from_str(&owned.payload_json)
        .map_err(|error| format!("converted provider event JSON is invalid: {error}"))?;
    if owned.request_id != request_id {
        return Err("provider event receipt belongs to another durable request".into());
    }
    let authority = state
        .execution_authority
        .as_ref()
        .ok_or_else(|| "signed Phase 6 execution authority is unavailable".to_string())?;
    let mut connection = state
        .db
        .lock()
        .map_err(|_| "database lock is unavailable".to_string())?;
    let appended = persistence::append_provider_event_with_receipt(
        &mut connection,
        owned.persistence_context(now),
        authority.as_ref(),
    )
    .map_err(|error| format!("provider event was rejected by durable persistence: {error}"))?;
    drop(connection);

    if let EventAppendResult::Inserted { sequence } = appended {
        let local = TaskEvent {
            task_id: event.task_id,
            sequence: u64::try_from(sequence)
                .map_err(|_| "durable task event sequence is negative".to_string())?,
            event_type: event.event_type,
            payload: event.payload,
            actor: "provider-adapter".into(),
            created_at: event.occurred_at,
        };
        let _ = state.events.send(local);
    }
    Ok(())
}

fn persist_active_status(
    state: &AppState,
    request_id: &str,
    evidence: &dread_orchestrator::persistence::ProviderEvidence,
) -> Result<(), String> {
    let authority = state
        .execution_authority
        .as_ref()
        .ok_or_else(|| "signed Phase 6 execution authority is unavailable".to_string())?;
    let now = now_epoch_i64();
    let mut connection = state
        .db
        .lock()
        .map_err(|_| "database lock is unavailable".to_string())?;
    persistence::record_provider_active(
        &mut connection,
        request_id,
        evidence,
        authority.as_ref(),
        now,
    )
    .map_err(|error| format!("authenticated provider status was rejected: {error}"))?;
    persistence::mark_recovery_worker_attached(&mut connection, request_id, now)
        .map_err(|error| format!("provider worker attachment was not persisted: {error}"))?;
    Ok(())
}

fn persist_provider_terminal(
    state: &AppState,
    request_id: &str,
    response: &dread_agents::AgentTask,
    evidence: &dread_orchestrator::persistence::ProviderEvidence,
) -> Result<(), String> {
    let authority = state
        .execution_authority
        .as_ref()
        .ok_or_else(|| "signed Phase 6 execution authority is unavailable".to_string())?;
    let outcome = match response.state {
        AgentTaskState::Succeeded => "succeeded",
        AgentTaskState::Failed => "failed",
        AgentTaskState::Cancelled => "cancelled",
        _ => return Err("provider task result is not terminal".into()),
    };
    let result_json = response.result.as_ref().map(Value::to_string);
    let provider_error = response.error.as_deref();
    let now = now_epoch_i64();
    let mut connection = state
        .db
        .lock()
        .map_err(|_| "database lock is unavailable".to_string())?;
    persistence::record_provider_terminal(
        &mut connection,
        request_id,
        ProviderTerminalPayload {
            outcome,
            result_json: result_json.as_deref(),
            provider_error,
        },
        evidence,
        authority.as_ref(),
        now,
    )
    .map_err(|error| format!("provider terminal result was rejected: {error}"))?;

    if outcome == "succeeded"
        && let Err(error) = persistence::record_independent_verification(
            &mut connection,
            request_id,
            result_json.as_deref(),
            provider_error,
            authority.as_ref(),
            now,
        )
    {
        // The current authority is intentionally fail-closed until a
        // separately trusted result verifier is configured. Do not turn
        // provider success into DREAD task success or advance missions.
        warn!(
            request_id,
            error = %error,
            "Provider succeeded but independent result verification is still pending"
        );
    }
    Ok(())
}

async fn cancel_with_evidence(
    state: &AppState,
    adapter: &dyn AgentAdapter,
    identity: &dread_agents::AgentExecutionIdentity,
    provider_execution_id: &str,
) -> Result<bool, String> {
    let authority = state
        .execution_authority
        .as_ref()
        .ok_or_else(|| "signed Phase 6 execution authority is unavailable".to_string())?;
    let now = now_epoch_i64();
    {
        let mut connection = state
            .db
            .lock()
            .map_err(|_| "database lock is unavailable".to_string())?;
        persistence::request_cancellation(&mut connection, &identity.request_id, now)
            .map_err(|error| format!("durable cancellation request failed: {error}"))?;
    }
    let receipt = adapter
        .cancel_task_with_evidence(identity, provider_execution_id)
        .await
        .map_err(|error| format!("provider cancellation transport failed: {error}"))?;
    if receipt.response.task_id != identity.task_id
        || receipt.response.provider_task_id.as_deref() != Some(provider_execution_id)
    {
        return Err("provider cancellation response identity does not match the attempt".into());
    }
    let evidence = provider_evidence_from_adapter_receipt(
        &identity.provider_id,
        &identity.adapter_id,
        &identity.adapter_version,
        identity.provider_registration_revision,
        &receipt,
    )?;
    match receipt.response.state {
        AgentTaskState::Cancelled | AgentTaskState::Succeeded | AgentTaskState::Failed => {
            persist_provider_terminal(state, &identity.request_id, &receipt.response, &evidence)?;
            Ok(true)
        }
        AgentTaskState::Queued
        | AgentTaskState::Running
        | AgentTaskState::WaitingForConfirmation => {
            let mut connection = state
                .db
                .lock()
                .map_err(|_| "database lock is unavailable".to_string())?;
            persistence::record_cancellation_progress(
                &mut connection,
                &identity.request_id,
                "accepted",
                &evidence,
                authority.as_ref(),
                now,
            )
            .map_err(|error| {
                format!("provider cancellation acknowledgement was rejected: {error}")
            })?;
            Ok(false)
        }
        _ => {
            let mut connection = state
                .db
                .lock()
                .map_err(|_| "database lock is unavailable".to_string())?;
            persistence::record_cancellation_progress(
                &mut connection,
                &identity.request_id,
                "unknown",
                &evidence,
                authority.as_ref(),
                now,
            )
            .map_err(|error| format!("provider cancellation uncertainty was rejected: {error}"))?;
            Ok(false)
        }
    }
}

pub(super) async fn execute_durable_agent_task_inner(
    state: AppState,
    adapter: Arc<dyn AgentAdapter>,
    execution: AgentTaskExecution,
    cancellation: watch::Receiver<bool>,
) {
    let prepared = match prepare_new_attempt(&state, &execution) {
        Ok(Some(prepared)) => prepared,
        Ok(None) => return,
        Err(error) => {
            warn!(task_id = %execution.task_id, error = %error, "Durable provider dispatch preparation failed; no provider start was sent");
            return;
        }
    };
    let PreparedDurableDispatch {
        request_id,
        identity,
        start_request,
    } = prepared;
    if *cancellation.borrow() {
        mark_reconciliation_required(
            &state,
            &request_id,
            "task was cancelled before durable provider submission completed",
        );
        return;
    }

    // This is the sole provider-start call in the durable path. The attempt
    // already exists durably, and only the Created branch can reach this line.
    let started = match adapter.start_task_with_evidence(start_request).await {
        Ok(receipt) => receipt,
        Err(error) => {
            mark_reconciliation_required(
                &state,
                &request_id,
                &format!("provider start outcome is uncertain: {error}"),
            );
            return;
        }
    };
    if started.response.task_id != identity.task_id {
        mark_reconciliation_required(
            &state,
            &request_id,
            "provider start acknowledgement returned a different local task ID",
        );
        return;
    }
    let Some(provider_execution_id) = started.response.provider_task_id.clone() else {
        mark_reconciliation_required(
            &state,
            &request_id,
            "provider start acknowledgement contained no provider execution ID",
        );
        return;
    };
    let ack = match provider_evidence_from_adapter_receipt(
        &identity.provider_id,
        &identity.adapter_id,
        &identity.adapter_version,
        identity.provider_registration_revision,
        &started,
    ) {
        Ok(evidence) => evidence,
        Err(error) => {
            mark_reconciliation_required(&state, &request_id, &error);
            return;
        }
    };
    let authority = match state.execution_authority.as_ref() {
        Some(authority) => Arc::clone(authority),
        None => {
            mark_reconciliation_required(
                &state,
                &request_id,
                "execution authority disappeared before acknowledgement persistence",
            );
            return;
        }
    };
    {
        let mut connection = match state.db.lock() {
            Ok(connection) => connection,
            Err(_) => {
                mark_reconciliation_required(
                    &state,
                    &request_id,
                    "database lock was unavailable after provider acknowledgement",
                );
                return;
            }
        };
        if let Err(error) = persistence::record_provider_ack(
            &mut connection,
            &request_id,
            &provider_execution_id,
            &ack,
            authority.as_ref(),
            now_epoch_i64(),
        ) {
            drop(connection);
            mark_reconciliation_required(
                &state,
                &request_id,
                &format!("provider acknowledgement could not be persisted: {error}"),
            );
            return;
        }
    }

    let mut cursor = execution.initial_event_cursor;
    let mut cancellation_submitted = false;
    loop {
        if *cancellation.borrow() && !cancellation_submitted {
            cancellation_submitted = true;
            match cancel_with_evidence(&state, adapter.as_ref(), &identity, &provider_execution_id)
                .await
            {
                Ok(true) => return,
                Ok(false) => {}
                Err(error) => {
                    mark_reconciliation_required(&state, &request_id, &error);
                    return;
                }
            }
        }

        let events = match adapter
            .events_with_evidence(&identity, &provider_execution_id, cursor)
            .await
        {
            Ok(events) => events,
            Err(error) => {
                mark_reconciliation_required(
                    &state,
                    &request_id,
                    &format!("authenticated provider event polling failed: {error}"),
                );
                return;
            }
        };
        for receipt in events {
            cursor = cursor.max(receipt.response.sequence);
            if let Err(error) = persist_provider_event(&state, &request_id, &receipt) {
                mark_reconciliation_required(&state, &request_id, &error);
                return;
            }
        }

        let status = match adapter
            .task_status_with_evidence(&identity, &provider_execution_id)
            .await
        {
            Ok(status) => status,
            Err(error) => {
                mark_reconciliation_required(
                    &state,
                    &request_id,
                    &format!("authenticated provider status polling failed: {error}"),
                );
                return;
            }
        };
        if status.response.task_id != identity.task_id
            || status.response.provider_task_id.as_deref() != Some(provider_execution_id.as_str())
        {
            mark_reconciliation_required(
                &state,
                &request_id,
                "provider status response identity does not match the durable attempt",
            );
            return;
        }

        if matches!(
            status.response.state,
            AgentTaskState::Succeeded | AgentTaskState::Failed | AgentTaskState::Cancelled
        ) {
            let result = match adapter
                .task_result_with_evidence(&identity, &provider_execution_id)
                .await
            {
                Ok(result) => result,
                Err(error) => {
                    mark_reconciliation_required(
                        &state,
                        &request_id,
                        &format!("provider terminal result retrieval failed: {error}"),
                    );
                    return;
                }
            };
            if result.response.task_id != identity.task_id
                || result.response.provider_task_id.as_deref()
                    != Some(provider_execution_id.as_str())
                || result.response.state != status.response.state
            {
                mark_reconciliation_required(
                    &state,
                    &request_id,
                    "provider result response does not match its terminal status observation",
                );
                return;
            }
            let terminal = match provider_evidence_from_adapter_receipt(
                &identity.provider_id,
                &identity.adapter_id,
                &identity.adapter_version,
                identity.provider_registration_revision,
                &result,
            ) {
                Ok(evidence) => evidence,
                Err(error) => {
                    mark_reconciliation_required(&state, &request_id, &error);
                    return;
                }
            };
            if let Err(error) =
                persist_provider_terminal(&state, &request_id, &result.response, &terminal)
            {
                mark_reconciliation_required(&state, &request_id, &error);
            }
            return;
        }

        let active = match provider_evidence_from_adapter_receipt(
            &identity.provider_id,
            &identity.adapter_id,
            &identity.adapter_version,
            identity.provider_registration_revision,
            &status,
        ) {
            Ok(evidence) => evidence,
            Err(error) => {
                mark_reconciliation_required(&state, &request_id, &error);
                return;
            }
        };
        if active.evidence_kind != "active" {
            mark_reconciliation_required(
                &state,
                &request_id,
                "nonterminal provider status did not produce active evidence",
            );
            return;
        }
        if let Err(error) = persist_active_status(&state, &request_id, &active) {
            mark_reconciliation_required(&state, &request_id, &error);
            return;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}
