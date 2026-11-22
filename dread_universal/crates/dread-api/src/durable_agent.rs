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

    monitor_durable_attempt(
        state,
        adapter,
        request_id,
        identity,
        provider_execution_id,
        execution.initial_event_cursor,
        cancellation,
    )
    .await;
}

/// Reattach the persisted provider execution ID to an already-created local
/// worker. This function is invoked only inside the shared ExecutionGate slot.
/// Provider dispatch stays closed until the signed policy and a distinct
/// independent-result verifier are both available and currently valid.
fn durable_dispatch_activation_ready(state: &AppState, now: i64) -> bool {
    let Some(authority) = state.execution_authority.as_ref() else {
        return false;
    };
    let status = authority.status(now);
    if status["execution_policy"]["status"] != "verified"
        || status["independent_result_verification"] != "verified"
    {
        return false;
    }
    let Some(providers) = status["providers"].as_array() else {
        return false;
    };
    !providers.is_empty()
        && providers.iter().all(|provider| {
            provider["enabled"] == true
                && provider["isolation_verified"] == true
                && provider["revoked"] != true
        })
}

fn attach_recovered_worker(state: &AppState, request_id: &str) -> Result<bool, String> {
    let now = now_epoch_i64();
    let mut connection = state
        .db
        .lock()
        .map_err(|_| "database lock is unavailable during worker reattachment".to_string())?;
    persistence::mark_recovery_worker_attached(&mut connection, request_id, now)
        .map_err(|error| format!("durable recovery attachment failed: {error}"))?;
    if !durable_dispatch_activation_ready(state, now) {
        return Ok(false);
    }
    persistence::release_startup_dispatch_barrier(&mut connection, now)
        .map_err(|error| format!("startup dispatch barrier evaluation failed: {error}"))
}

/// Monitor an existing provider execution. This path never issues a start
/// request and is shared by first-run dispatch and startup reattachment.
async fn monitor_durable_attempt(
    state: AppState,
    adapter: Arc<dyn AgentAdapter>,
    request_id: String,
    identity: dread_agents::AgentExecutionIdentity,
    provider_execution_id: String,
    mut cursor: u64,
    cancellation: watch::Receiver<bool>,
) {
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

/// Reconcile existing attempts on boot using authenticated status reads only.
/// No branch in this function can call the provider start endpoint.
pub(super) async fn recover_startup_durable_attempts(state: &AppState) -> Result<usize, String> {
    if state.execution_authority.is_none() {
        return Ok(0);
    }
    let attempts = {
        let connection = state.db.lock().map_err(|_| {
            "database lock is unavailable during durable startup recovery".to_string()
        })?;
        persistence::unresolved_attempts(&connection)
            .map_err(|error| format!("cannot enumerate durable attempts: {error}"))?
    };
    let mut workers_scheduled = 0usize;

    for attempt in attempts {
        if attempt.dispatch_state == "provider_succeeded" {
            let admission = {
                let connection = state.db.lock().map_err(|_| {
                    "database lock is unavailable during verification recovery".to_string()
                })?;
                persistence::admission_for_request(&connection, &attempt.request_id)
                    .map_err(|error| format!("cannot read durable admission: {error}"))?
            };
            let authority = state.execution_authority.as_ref().ok_or_else(|| {
                "signed Phase 6 authority disappeared during recovery".to_string()
            })?;
            let result = {
                let mut connection = state.db.lock().map_err(|_| {
                    "database lock is unavailable during independent verification".to_string()
                })?;
                persistence::record_independent_verification(
                    &mut connection,
                    &attempt.request_id,
                    attempt.provider_result_json.as_deref(),
                    attempt.provider_error.as_deref(),
                    authority.as_ref(),
                    now_epoch_i64(),
                )
            };
            if let Err(error) = result {
                warn!(
                    request_id = %attempt.request_id,
                    verifier = %authority.status(now_epoch_i64())["independent_result_verification"],
                    requester = %admission.requester_id,
                    error = %error,
                    "Persisted provider success remains pending independent verification"
                );
            }
            continue;
        }

        let Some(provider_execution_id) = attempt.provider_execution_id.clone() else {
            warn!(
                request_id = %attempt.request_id,
                task_id = %attempt.task_id,
                dispatch_state = %attempt.dispatch_state,
                "Provider outcome is ambiguous and has no persisted execution ID; startup barrier remains closed and no start is replayed"
            );
            continue;
        };
        let admission = {
            let connection = state.db.lock().map_err(|_| {
                "database lock is unavailable while reading provider admission".to_string()
            })?;
            persistence::admission_for_request(&connection, &attempt.request_id)
                .map_err(|error| format!("cannot read durable admission: {error}"))?
        };
        let identity = agent_execution_identity(&admission, &attempt)?;
        let adapter = state
            .agent_adapters
            .get(&identity.provider_id)
            .cloned()
            .ok_or_else(|| {
                format!(
                    "provider adapter '{}' is not configured",
                    identity.provider_id
                )
            })?;
        if !adapter.descriptor().enabled
            || adapter.descriptor().adapter != identity.adapter_id
            || adapter.descriptor().version.as_deref() != Some(identity.adapter_version.as_str())
        {
            warn!(
                request_id = %attempt.request_id,
                provider = %identity.provider_id,
                "Provider adapter descriptor differs from the durable registration; attempt remains unresolved"
            );
            continue;
        }

        let status = match adapter
            .task_status_with_evidence(&identity, &provider_execution_id)
            .await
        {
            Ok(status) => status,
            Err(error) => {
                warn!(
                    request_id = %attempt.request_id,
                    provider = %identity.provider_id,
                    error = %error,
                    "Authenticated provider status recovery failed; attempt remains unresolved"
                );
                continue;
            }
        };
        if status.response.task_id != identity.task_id
            || status.response.provider_task_id.as_deref() != Some(provider_execution_id.as_str())
        {
            warn!(
                request_id = %attempt.request_id,
                "Provider recovery status returned a conflicting local/provider task identity"
            );
            continue;
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
                    warn!(
                        request_id = %attempt.request_id,
                        error = %error,
                        "Terminal provider result recovery failed; attempt remains unresolved"
                    );
                    continue;
                }
            };
            if result.response.task_id != identity.task_id
                || result.response.provider_task_id.as_deref()
                    != Some(provider_execution_id.as_str())
                || result.response.state != status.response.state
            {
                warn!(
                    request_id = %attempt.request_id,
                    "Recovered provider result conflicts with authenticated status"
                );
                continue;
            }
            let evidence = match provider_evidence_from_adapter_receipt(
                &identity.provider_id,
                &identity.adapter_id,
                &identity.adapter_version,
                identity.provider_registration_revision,
                &result,
            ) {
                Ok(evidence) => evidence,
                Err(error) => {
                    warn!(request_id = %attempt.request_id, error = %error, "Recovered terminal receipt failed scope or digest validation");
                    continue;
                }
            };
            if let Err(error) =
                persist_provider_terminal(state, &attempt.request_id, &result.response, &evidence)
            {
                warn!(
                    request_id = %attempt.request_id,
                    error = %error,
                    "Recovered provider terminal result was rejected by durable persistence"
                );
            }
            continue;
        }

        let active = match provider_evidence_from_adapter_receipt(
            &identity.provider_id,
            &identity.adapter_id,
            &identity.adapter_version,
            identity.provider_registration_revision,
            &status,
        ) {
            Ok(evidence) if evidence.evidence_kind == "active" => evidence,
            Ok(_) => {
                warn!(request_id = %attempt.request_id, "Provider returned a nonterminal state without active evidence");
                continue;
            }
            Err(error) => {
                warn!(request_id = %attempt.request_id, error = %error, "Recovered provider status receipt failed validation");
                continue;
            }
        };
        let authority = state
            .execution_authority
            .as_ref()
            .ok_or_else(|| "signed Phase 6 execution authority is unavailable".to_string())?;
        {
            let mut connection = state.db.lock().map_err(|_| {
                "database lock is unavailable while persisting recovered provider status"
                    .to_string()
            })?;
            if let Err(error) = persistence::record_provider_active(
                &mut connection,
                &attempt.request_id,
                &active,
                authority.as_ref(),
                now_epoch_i64(),
            ) {
                warn!(
                    request_id = %attempt.request_id,
                    error = %error,
                    "Recovered active provider observation could not be persisted"
                );
                continue;
            }
        }

        let cursor: u64 = {
            let connection = state.db.lock().map_err(|_| {
                "database lock is unavailable while reading the recovered event cursor".to_string()
            })?;
            connection
                .query_row(
                    "SELECT provider_event_cursor FROM tasks WHERE id = ?1",
                    [&attempt.task_id],
                    |row| row.get(0),
                )
                .map_err(|error| format!("cannot read recovered event cursor: {error}"))?
        };
        let (cancel_sender, cancel_receiver) = watch::channel(attempt.cancellation_state != "none");
        state
            .cancellations
            .lock()
            .map_err(|_| "task cancellation registry is unavailable".to_string())?
            .insert(attempt.task_id.clone(), cancel_sender);

        let worker_state = state.clone();
        let task_id = attempt.task_id.clone();
        let request_id = attempt.request_id.clone();
        let recovered_state = attempt.cancellation_state != "none";
        tokio::spawn(async move {
            let gate_state = worker_state.clone();
            let gate_task_id = task_id.clone();
            let run = worker_state
                .execution
                .run_managed_for_resources(
                    dread_orchestrator::ResourceRequest::AGENT,
                    gate_task_id.clone(),
                    cancel_receiver,
                    move |cancellation| async move {
                        match attach_recovered_worker(&gate_state, &request_id) {
                            Ok(opened) => info!(
                                request_id = %request_id,
                                task_id = %gate_task_id,
                                barrier_open = opened,
                                "Recovered provider worker attached without resubmitting its start"
                            ),
                            Err(error) => {
                                warn!(
                                    request_id = %request_id,
                                    error = %error,
                                    "Could not attach recovered provider worker; dispatch barrier remains closed"
                                );
                                mark_reconciliation_required(&gate_state, &request_id, &error);
                                return Ok(());
                            }
                        }
                        monitor_durable_attempt(
                            gate_state,
                            adapter,
                            request_id,
                            identity,
                            provider_execution_id,
                            cursor,
                            cancellation,
                        )
                        .await;
                        Ok(())
                    },
                )
                .await;
            if let Err(error) = run {
                warn!(
                    task_id = %task_id,
                    error = ?error,
                    cancellation_pending = recovered_state,
                    "Recovered provider worker could not enter the shared ExecutionGate"
                );
                mark_reconciliation_required(
                    &worker_state,
                    &attempt.request_id,
                    "recovered provider worker could not acquire the shared execution gate",
                );
            }
        });
        workers_scheduled = workers_scheduled.saturating_add(1);
    }

    // This succeeds only when every nonterminal attempt has been reconciled or
    // a worker has attached for this boot. Unknown submissions and unverified
    // provider successes remain blockers.
    let opened = if durable_dispatch_activation_ready(state, now_epoch_i64()) {
        let mut connection = state.db.lock().map_err(|_| {
            "database lock is unavailable while evaluating startup dispatch barrier".to_string()
        })?;
        persistence::release_startup_dispatch_barrier(&mut connection, now_epoch_i64())
            .map_err(|error| format!("startup dispatch barrier evaluation failed: {error}"))?
    } else {
        false
    };
    info!(
        workers_scheduled,
        barrier_open = opened,
        "Durable startup provider reconciliation completed"
    );
    Ok(workers_scheduled)
}
