use crate::runtime_attestation::{
    RuntimeIsolationAttestationSummary, RuntimeIsolationControl,
    VerifiedRuntimeIsolationAttestation,
};
//! DeerFlow Gateway API adapter.
//!
//! This adapter uses the documented thread-scoped API, PAT bearer auth,
//! run idempotency keys, durable run status/events and provider cancellation.
//! It is library-only: this crate does not auto-register it. A caller must
//! verify the exact Gateway sandbox configuration and attest that each
//! DREAD task receives an isolated container-backed thread workspace before
//! calling `attest_container_sandbox`. LocalSandboxProvider is rejected.

use crate::{
    AdapterFuture, AgentAdapter, AgentAdapterError, AgentCapability, AgentDescriptor, AgentEvent,
    AgentExecutionIdentity, AgentHealth, AgentHealthState, AgentOperationEvidenceKind,
    AgentOperationReceipt, AgentOperationReceiptDetails, AgentReceiptSigner, AgentTask,
    AgentTaskState, DurableAgentStartRequest, StartTaskRequest, durable_evidence_error,
    signed_agent_operation_receipt,
};
use reqwest::{Client, StatusCode, Url};
use serde_json::{Value, json};
use std::{
    net::IpAddr,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const MAX_OBJECTIVE_BYTES: usize = 20_000;
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const MAX_TASK_ID_BYTES: usize = 160;
const MAX_ASSISTANT_ID_BYTES: usize = 128;
const MAX_MODEL_NAME_BYTES: usize = 200;

#[derive(Clone)]
pub struct DeerFlowApiAdapter {
    descriptor: AgentDescriptor,
    client: Client,
    base_url: Url,
    api_prefix: String,
    pat: String,
    sandbox_attested: bool,
    receipt_signer: Option<AgentReceiptSigner>,
    runtime_attestation: Option<RuntimeIsolationAttestationSummary>,
}

impl DeerFlowApiAdapter {
    /// Construct a disabled, loopback-only adapter.
    ///
    /// `api_prefix` is `/api` for a direct Gateway listener or
    /// `/api/langgraph` for the documented Nginx LangGraph route.
    /// The adapter stays disabled until an operator attests a container
    /// sandbox provider and per-task workspace isolation.
    pub fn new(
        base_url: &str,
        api_prefix: &str,
        pat: impl Into<String>,
        display_name: impl Into<String>,
    ) -> Result<Self, AgentAdapterError> {
        let mut url = Url::parse(base_url).map_err(|error| {
            adapter_error(
                "invalid_endpoint",
                format!("Invalid DeerFlow URL: {error}"),
                false,
            )
        })?;
        if url.scheme() != "http"
            || url.port().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || !url.path().trim_matches('/').is_empty()
        {
            return Err(adapter_error(
                "unsafe_endpoint",
                "DeerFlow requires an explicit-port loopback HTTP origin without credentials, path, query or fragment".into(),
                false,
            ));
        }
        let loopback = url
            .host_str()
            .and_then(|host| {
                host.trim_start_matches('[')
                    .trim_end_matches(']')
                    .parse::<IpAddr>()
                    .ok()
            })
            .is_some_and(|ip| ip.is_loopback());
        if !loopback {
            return Err(adapter_error(
                "unsafe_endpoint",
                "DeerFlow Gateway must be addressed through a loopback IP".into(),
                false,
            ));
        }
        if !matches!(api_prefix, "/api" | "/api/langgraph") {
            return Err(adapter_error(
                "invalid_api_prefix",
                "DeerFlow API prefix must be /api or /api/langgraph".into(),
                false,
            ));
        }
        let pat = pat.into();
        if pat.trim().is_empty() || pat.len() > 4096 {
            return Err(adapter_error(
                "missing_pat",
                "A non-empty DeerFlow personal access token is required".into(),
                false,
            ));
        }
        url.set_path("");
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| adapter_error("client_error", error.to_string(), false))?;
        let endpoint = url.as_str().trim_end_matches('/').to_string();
        Ok(Self {
            descriptor: AgentDescriptor {
                id: "deerflow.gateway".into(),
                display_name: display_name.into(),
                adapter: "deerflow.gateway".into(),
                version: Some(env!("CARGO_PKG_VERSION").to_owned()),
                endpoint: Some(format!("{endpoint}{api_prefix}")),
                enabled: false,
            },
            client,
            base_url: url,
            api_prefix: api_prefix.to_string(),
            pat,
            sandbox_attested: false,
            receipt_signer: None,
            runtime_attestation: None,
        })
    }

    /// Attach the trusted local receipt signer. Durable lifecycle operations
    /// remain unavailable until a key anchored by the signed provider registry
    /// is configured by the caller.
    pub fn with_receipt_signer(mut self, signer: AgentReceiptSigner) -> Self {
        self.receipt_signer = Some(signer);
        self
    }

    /// Enable task submission only after the operator verifies the deployed
    /// DeerFlow config uses a container-backed provider and one isolated
    /// thread workspace per DREAD task. This is an explicit attestation, not
    /// an automatic remote config probe.
    pub(crate) fn attest_container_sandbox(
        mut self,
        provider_class: &str,
        isolated_workspace_per_task: bool,
        provisioner_url: Option<&str>,
    ) -> Result<Self, AgentAdapterError> {
        let supported = ["AioSandboxProvider", "BoxliteProvider"];
        if !supported
            .iter()
            .any(|provider| provider_class.ends_with(provider))
        {
            return Err(adapter_error(
                "unsafe_sandbox_provider",
                "DeerFlow must use the local AioSandboxProvider or BoxliteProvider; LocalSandboxProvider and remote/unknown providers are denied".into(),
                false,
            ));
        }
        if !isolated_workspace_per_task {
            return Err(adapter_error(
                "workspace_isolation_required",
                "DeerFlow must provide an isolated thread workspace for each DREAD task".into(),
                false,
            ));
        }
        if provisioner_url.is_some_and(|url| !url.trim().is_empty()) {
            return Err(adapter_error(
                "remote_provisioner_not_supported",
                "DeerFlow remote/Kubernetes provisioners are not accepted by this local sandbox attestation".into(),
                false,
            ));
        }
        self.sandbox_attested = true;
        self.descriptor.enabled = true;
        Ok(self)
    }

    fn endpoint(&self, path: &str) -> String {
        format!(
            "{}{}{}",
            self.base_url.as_str().trim_end_matches('/'),
            self.api_prefix,
            path
        )
    }

    async fn json_request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
        idempotency_key: Option<&str>,
        query: &[(&str, String)],
    ) -> Result<Value, AgentAdapterError> {
        let mut request = self
            .client
            .request(method, self.endpoint(path))
            .bearer_auth(&self.pat)
            .timeout(Duration::from_secs(30));
        if !query.is_empty() {
            request = request.query(query);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        if let Some(key) = idempotency_key {
            validate_idempotency_key(key)?;
            request = request.header("Idempotency-Key", key);
        }
        let response = request.send().await.map_err(|error| {
            adapter_error(
                "transport_error",
                error.to_string(),
                error.is_timeout() || error.is_connect(),
            )
        })?;
        parse_json_response(response).await
    }

    async fn read_run(&self, thread_id: &str, run_id: &str) -> Result<Value, AgentAdapterError> {
        validate_thread_id(thread_id)?;
        validate_run_id(run_id)?;
        self.json_request(
            reqwest::Method::GET,
            &format!("/threads/{thread_id}/runs/{run_id}"),
            None,
            None,
            &[],
        )
        .await
    }

    fn split_provider_id(provider_task_id: &str) -> Result<(&str, &str), AgentAdapterError> {
        let (thread_id, run_id) = provider_task_id.split_once(':').ok_or_else(|| {
            adapter_error(
                "invalid_provider_task_id",
                "DeerFlow provider task ID must contain thread_id:run_id".into(),
                false,
            )
        })?;
        validate_thread_id(thread_id)?;
        validate_run_id(run_id)?;
        Ok((thread_id, run_id))
    }

    fn to_task(&self, task_id: &str, provider_task_id: &str, run: &Value) -> AgentTask {
        let state = run_state(
            run.get("status")
                .and_then(Value::as_str)
                .unwrap_or("unknown"),
        );
        AgentTask {
            task_id: task_id.to_string(),
            state,
            provider_task_id: Some(provider_task_id.to_string()),
            result: run.get("result").cloned(),
            error: run
                .get("error")
                .and_then(Value::as_str)
                .or_else(|| run.get("detail").and_then(Value::as_str))
                .map(str::to_string),
            updated_at: now_epoch(),
        }
    }

    fn require_attested(&self) -> Result<(), AgentAdapterError> {
        if self.sandbox_attested && self.descriptor.enabled {
            Ok(())
        } else {
            Err(adapter_error(
                "sandbox_not_attested",
                "DeerFlow task submission is disabled until a container sandbox and per-task workspace isolation are attested".into(),
                false,
            ))
        }
    }
}

impl AgentAdapter for DeerFlowApiAdapter {
    fn descriptor(&self) -> &AgentDescriptor {
        &self.descriptor
    }

    fn runtime_isolation_attestation(&self) -> Option<RuntimeIsolationAttestationSummary> {
        self.runtime_attestation.clone()
    }

    fn health<'a>(&'a self) -> AdapterFuture<'a, AgentHealth> {
        Box::pin(async move {
            let response = self
                .client
                .get(format!(
                    "{}{}",
                    self.base_url.as_str().trim_end_matches('/'),
                    "/health/ready"
                ))
                .bearer_auth(&self.pat)
                .timeout(Duration::from_secs(10))
                .send()
                .await
                .map_err(|error| {
                    adapter_error(
                        "transport_error",
                        error.to_string(),
                        error.is_timeout() || error.is_connect(),
                    )
                })?;
            let status = response.status();
            Ok(AgentHealth {
                state: if status.is_success() {
                    AgentHealthState::Online
                } else if status.is_server_error() || status == StatusCode::SERVICE_UNAVAILABLE {
                    AgentHealthState::Degraded
                } else {
                    AgentHealthState::Unknown
                },
                checked_at: now_epoch(),
                detail: Some(format!(
                    "DeerFlow /health/ready returned HTTP {}",
                    status.as_u16()
                )),
            })
        })
    }

    fn capabilities<'a>(&'a self) -> AdapterFuture<'a, Vec<AgentCapability>> {
        Box::pin(async move {
            if !self.sandbox_attested || !self.descriptor.enabled {
                return Ok(Vec::new());
            }
            Ok(vec![AgentCapability {
                id: "deerflow.run".into(),
                name: "DeerFlow isolated agent run".into(),
                description: "Submit an approved objective to a container-backed DeerFlow thread with an idempotency key; poll durable status/events and cancel the provider run.".into(),
                input_schema: json!({
                    "type":"object",
                    "properties":{
                        "objective":{"type":"string","minLength":1,"maxLength":20000},
                        "assistant_id":{"type":"string","maxLength":128},
                        "model_name":{"type":"string","maxLength":200},
                        "thinking_enabled":{"type":"boolean"}
                    },
                    "required":["objective"],
                    "additionalProperties":false
                }),
                output_schema: Some(json!({"type":"object","properties":{"messages":{"type":"array"}}})),
            }])
        })
    }

    fn start_task<'a>(&'a self, request: StartTaskRequest) -> AdapterFuture<'a, AgentTask> {
        Box::pin(async move {
            self.require_attested()?;
            if request.task_id.trim().is_empty() || request.task_id.len() > MAX_TASK_ID_BYTES {
                return Err(adapter_error(
                    "invalid_task_id",
                    "DREAD task ID must contain 1-160 characters".into(),
                    false,
                ));
            }
            if request.capability_id != "deerflow.run" {
                return Err(adapter_error(
                    "unsupported_capability",
                    "DeerFlow adapter supports only deerflow.run".into(),
                    false,
                ));
            }
            let object = request.input.as_object().ok_or_else(|| {
                adapter_error(
                    "invalid_input",
                    "DeerFlow input must be an object".into(),
                    false,
                )
            })?;
            if object.keys().any(|key| {
                ![
                    "objective",
                    "assistant_id",
                    "model_name",
                    "thinking_enabled",
                ]
                .contains(&key.as_str())
            }) {
                return Err(adapter_error(
                    "invalid_input",
                    "DeerFlow input contains an unsupported field".into(),
                    false,
                ));
            }
            let objective = object
                .get("objective")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty() && value.len() <= MAX_OBJECTIVE_BYTES)
                .ok_or_else(|| {
                    adapter_error(
                        "invalid_input",
                        "DeerFlow objective must contain 1-20,000 UTF-8 bytes".into(),
                        false,
                    )
                })?;
            if object
                .get("assistant_id")
                .is_some_and(|value| !value.is_string())
                || object
                    .get("model_name")
                    .is_some_and(|value| !value.is_string())
                || object
                    .get("thinking_enabled")
                    .is_some_and(|value| !value.is_boolean())
            {
                return Err(adapter_error(
                    "invalid_input",
                    "DeerFlow assistant_id/model_name/thinking_enabled fields have invalid types"
                        .into(),
                    false,
                ));
            }
            let assistant_id = object.get("assistant_id").and_then(Value::as_str);
            if assistant_id
                .is_some_and(|value| value.is_empty() || value.len() > MAX_ASSISTANT_ID_BYTES)
            {
                return Err(adapter_error(
                    "invalid_input",
                    "DeerFlow assistant_id must contain 1-128 characters".into(),
                    false,
                ));
            }
            let model_name = object
                .get("model_name")
                .and_then(Value::as_str)
                .unwrap_or("dread-local-qwen");
            if model_name.is_empty() || model_name.len() > MAX_MODEL_NAME_BYTES {
                return Err(adapter_error(
                    "invalid_input",
                    "DeerFlow model_name must contain 1-200 characters".into(),
                    false,
                ));
            }
            let thinking_enabled = object
                .get("thinking_enabled")
                .and_then(Value::as_bool)
                .unwrap_or(false);

            let thread_id = thread_id_for_task(&request.task_id);
            let thread_body = json!({
                "thread_id": thread_id,
                "metadata": {
                    "source": "project-dread",
                    "dread_task_id": request.task_id
                }
            });
            let created = self
                .json_request(
                    reqwest::Method::POST,
                    "/threads",
                    Some(thread_body),
                    None,
                    &[],
                )
                .await?;
            let returned_thread = created
                .get("thread_id")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    adapter_error(
                        "invalid_provider_response",
                        "DeerFlow thread creation returned no thread_id".into(),
                        false,
                    )
                })?;
            validate_thread_id(returned_thread)?;
            if returned_thread != thread_id {
                return Err(adapter_error(
                    "thread_id_mismatch",
                    "DeerFlow returned a different thread ID than the idempotent DREAD task mapping".into(),
                    false,
                ));
            }

            let mut run_body = json!({
                "input": {
                    "messages": [{"role": "user", "content": objective}]
                },
                "context": {
                    "model_name": model_name,
                    "thinking_enabled": thinking_enabled
                },
                "on_disconnect": "continue",
                "multitask_strategy": "reject"
            });
            if let Some(assistant_id) = assistant_id {
                run_body["assistant_id"] = Value::String(assistant_id.to_string());
            }
            let run = self
                .json_request(
                    reqwest::Method::POST,
                    &format!("/threads/{thread_id}/runs"),
                    Some(run_body),
                    Some(&request.idempotency_key),
                    &[],
                )
                .await?;
            let run_id = run.get("run_id").and_then(Value::as_str).ok_or_else(|| {
                adapter_error(
                    "invalid_provider_response",
                    "DeerFlow run creation returned no run_id".into(),
                    false,
                )
            })?;
            validate_run_id(run_id)?;
            let provider_task_id = format!("{thread_id}:{run_id}");
            Ok(self.to_task(&request.task_id, &provider_task_id, &run))
        })
    }

    fn task_status<'a>(
        &'a self,
        task_id: &'a str,
        provider_task_id: &'a str,
    ) -> AdapterFuture<'a, AgentTask> {
        Box::pin(async move {
            let (thread_id, run_id) = Self::split_provider_id(provider_task_id)?;
            let run = self.read_run(thread_id, run_id).await?;
            Ok(self.to_task(task_id, provider_task_id, &run))
        })
    }

    fn events<'a>(
        &'a self,
        task_id: &'a str,
        provider_task_id: &'a str,
        after_sequence: u64,
    ) -> AdapterFuture<'a, Vec<AgentEvent>> {
        Box::pin(async move {
            let (thread_id, run_id) = Self::split_provider_id(provider_task_id)?;
            let query = if after_sequence == 0 {
                Vec::new()
            } else {
                vec![("after_seq", after_sequence.to_string())]
            };
            let events = self
                .json_request(
                    reqwest::Method::GET,
                    &format!("/threads/{thread_id}/runs/{run_id}/events"),
                    None,
                    None,
                    &query,
                )
                .await?;
            let rows = events
                .as_array()
                .or_else(|| events.get("data").and_then(Value::as_array))
                .ok_or_else(|| {
                    adapter_error(
                        "invalid_provider_response",
                        "DeerFlow events endpoint did not return an array".into(),
                        false,
                    )
                })?;
            Ok(rows
                .iter()
                .filter_map(|row| {
                    let sequence = row
                        .get("seq")
                        .or_else(|| row.get("sequence"))
                        .or_else(|| row.get("sequence_number"))
                        .and_then(Value::as_u64)?;
                    if sequence <= after_sequence {
                        return None;
                    }
                    Some(AgentEvent {
                        task_id: task_id.to_string(),
                        sequence,
                        event_type: row
                            .get("event")
                            .or_else(|| row.get("event_type"))
                            .or_else(|| row.get("type"))
                            .and_then(Value::as_str)
                            .unwrap_or("message")
                            .to_string(),
                        payload: row
                            .get("data")
                            .or_else(|| row.get("payload"))
                            .cloned()
                            .unwrap_or_else(|| row.clone()),
                        occurred_at: now_epoch(),
                    })
                })
                .collect())
        })
    }

    fn cancel_task<'a>(
        &'a self,
        task_id: &'a str,
        provider_task_id: &'a str,
    ) -> AdapterFuture<'a, AgentTask> {
        Box::pin(async move {
            let (thread_id, run_id) = Self::split_provider_id(provider_task_id)?;
            let _ = self
                .json_request(
                    reqwest::Method::POST,
                    &format!("/threads/{thread_id}/runs/{run_id}/cancel"),
                    None,
                    None,
                    &[("action", "interrupt".into()), ("wait", "true".into())],
                )
                .await?;
            let run = self.read_run(thread_id, run_id).await?;
            Ok(self.to_task(task_id, provider_task_id, &run))
        })
    }

    fn task_result<'a>(
        &'a self,
        task_id: &'a str,
        provider_task_id: &'a str,
    ) -> AdapterFuture<'a, AgentTask> {
        Box::pin(async move {
            let (thread_id, run_id) = Self::split_provider_id(provider_task_id)?;
            let run = self.read_run(thread_id, run_id).await?;
            let mut task = self.to_task(task_id, provider_task_id, &run);
            let messages = self
                .json_request(
                    reqwest::Method::GET,
                    &format!("/threads/{thread_id}/runs/{run_id}/messages"),
                    None,
                    None,
                    &[],
                )
                .await?;
            let rows = messages
                .get("data")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if task.state == AgentTaskState::Succeeded {
                task.result = Some(json!({"messages": rows, "run": run}));
            }
            Ok(task)
        })
    }

    fn start_task_with_evidence<'a>(
        &'a self,
        request: DurableAgentStartRequest,
    ) -> AdapterFuture<'a, AgentOperationReceipt<AgentTask>> {
        Box::pin(async move {
            request.validate()?;
            let identity = request.identity;
            let signer = self.receipt_signer.as_ref().ok_or_else(|| {
                durable_evidence_error("DeerFlow receipt signer is not configured")
            })?;
            let response = self.start_task(request.request).await?;
            signed_agent_operation_receipt(
                signer,
                &self.descriptor,
                "deerflow",
                &identity,
                AgentOperationEvidenceKind::Acknowledged,
                response.clone(),
                AgentOperationReceiptDetails {
                    provider_execution_id: response.provider_task_id.clone(),
                    external_event_id: None,
                    event_type: None,
                },
            )
        })
    }

    fn task_status_with_evidence<'a>(
        &'a self,
        identity: &'a AgentExecutionIdentity,
        provider_execution_id: &'a str,
    ) -> AdapterFuture<'a, AgentOperationReceipt<AgentTask>> {
        Box::pin(async move {
            let response = self
                .task_status(&identity.task_id, provider_execution_id)
                .await?;
            let kind = match response.state {
                AgentTaskState::Succeeded => AgentOperationEvidenceKind::Succeeded,
                AgentTaskState::Failed => AgentOperationEvidenceKind::Failed,
                AgentTaskState::Cancelled => AgentOperationEvidenceKind::Cancelled,
                _ => AgentOperationEvidenceKind::Active,
            };
            signed_agent_operation_receipt(
                self.receipt_signer.as_ref().ok_or_else(|| {
                    durable_evidence_error("DeerFlow receipt signer is not configured")
                })?,
                &self.descriptor,
                "deerflow",
                identity,
                kind,
                response,
                AgentOperationReceiptDetails {
                    provider_execution_id: Some(provider_execution_id.to_owned()),
                    external_event_id: None,
                    event_type: None,
                },
            )
        })
    }

    fn events_with_evidence<'a>(
        &'a self,
        identity: &'a AgentExecutionIdentity,
        provider_execution_id: &'a str,
        after_sequence: u64,
    ) -> AdapterFuture<'a, Vec<AgentOperationReceipt<AgentEvent>>> {
        Box::pin(async move {
            let events = self
                .events(&identity.task_id, provider_execution_id, after_sequence)
                .await?;
            let signer = self.receipt_signer.as_ref().ok_or_else(|| {
                durable_evidence_error("DeerFlow receipt signer is not configured")
            })?;
            let mut receipts = Vec::with_capacity(events.len());
            for event in events {
                let external_event_id = format!("{provider_execution_id}:{}", event.sequence);
                let event_type = event.event_type.clone();
                receipts.push(signed_agent_operation_receipt(
                    signer,
                    &self.descriptor,
                    "deerflow",
                    identity,
                    AgentOperationEvidenceKind::Event,
                    event,
                    AgentOperationReceiptDetails {
                        provider_execution_id: Some(provider_execution_id.to_owned()),
                        external_event_id: Some(external_event_id),
                        event_type: Some(event_type),
                    },
                )?);
            }
            Ok(receipts)
        })
    }

    fn cancel_task_with_evidence<'a>(
        &'a self,
        identity: &'a AgentExecutionIdentity,
        provider_execution_id: &'a str,
    ) -> AdapterFuture<'a, AgentOperationReceipt<AgentTask>> {
        Box::pin(async move {
            let response = self
                .cancel_task(&identity.task_id, provider_execution_id)
                .await?;
            let kind = match response.state {
                AgentTaskState::Cancelled => AgentOperationEvidenceKind::Cancelled,
                AgentTaskState::Queued
                | AgentTaskState::Running
                | AgentTaskState::WaitingForConfirmation => {
                    AgentOperationEvidenceKind::CancellationAccepted
                }
                _ => AgentOperationEvidenceKind::CancellationUnknown,
            };
            signed_agent_operation_receipt(
                self.receipt_signer.as_ref().ok_or_else(|| {
                    durable_evidence_error("DeerFlow receipt signer is not configured")
                })?,
                &self.descriptor,
                "deerflow",
                identity,
                kind,
                response,
                AgentOperationReceiptDetails {
                    provider_execution_id: Some(provider_execution_id.to_owned()),
                    external_event_id: None,
                    event_type: None,
                },
            )
        })
    }

    fn task_result_with_evidence<'a>(
        &'a self,
        identity: &'a AgentExecutionIdentity,
        provider_execution_id: &'a str,
    ) -> AdapterFuture<'a, AgentOperationReceipt<AgentTask>> {
        Box::pin(async move {
            let response = self
                .task_result(&identity.task_id, provider_execution_id)
                .await?;
            let kind = match response.state {
                AgentTaskState::Succeeded => AgentOperationEvidenceKind::Succeeded,
                AgentTaskState::Failed => AgentOperationEvidenceKind::Failed,
                AgentTaskState::Cancelled => AgentOperationEvidenceKind::Cancelled,
                _ => AgentOperationEvidenceKind::Active,
            };
            signed_agent_operation_receipt(
                self.receipt_signer.as_ref().ok_or_else(|| {
                    durable_evidence_error("DeerFlow receipt signer is not configured")
                })?,
                &self.descriptor,
                "deerflow",
                identity,
                kind,
                response,
                AgentOperationReceiptDetails {
                    provider_execution_id: Some(provider_execution_id.to_owned()),
                    external_event_id: None,
                    event_type: None,
                },
            )
        })
    }
}

async fn parse_json_response(response: reqwest::Response) -> Result<Value, AgentAdapterError> {
    let status = response.status();
    if !status.is_success() {
        return Err(http_error(status, "DeerFlow API request failed"));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(adapter_error(
            "response_too_large",
            "DeerFlow JSON response exceeded the 2 MiB adapter limit".into(),
            false,
        ));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| adapter_error("transport_error", error.to_string(), error.is_timeout()))?;
    if bytes.is_empty() {
        return Ok(Value::Null);
    }
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(adapter_error(
            "response_too_large",
            "DeerFlow JSON response exceeded the 2 MiB adapter limit".into(),
            false,
        ));
    }
    serde_json::from_slice::<Value>(&bytes)
        .map_err(|error| adapter_error("invalid_provider_response", error.to_string(), false))
}

fn http_error(status: StatusCode, context: &str) -> AgentAdapterError {
    let retryable = status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS;
    adapter_error(
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            "authentication_error"
        } else if status == StatusCode::NOT_FOUND {
            "not_found"
        } else if status == StatusCode::CONFLICT {
            "conflict"
        } else {
            "http_error"
        },
        format!("{context}: HTTP {}", status.as_u16()),
        retryable,
    )
}

fn adapter_error(code: &str, message: String, retryable: bool) -> AgentAdapterError {
    AgentAdapterError {
        code: code.into(),
        message,
        retryable,
    }
}

fn validate_idempotency_key(key: &str) -> Result<(), AgentAdapterError> {
    if key.is_empty() || key.len() > 255 || !key.bytes().all(|byte| (0x21..=0x7e).contains(&byte)) {
        return Err(adapter_error(
            "invalid_idempotency_key",
            "DeerFlow Idempotency-Key must be 1-255 visible ASCII characters".into(),
            false,
        ));
    }
    Ok(())
}

fn validate_thread_id(thread_id: &str) -> Result<(), AgentAdapterError> {
    if thread_id.is_empty()
        || thread_id.len() > 64
        || !thread_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
    {
        return Err(adapter_error(
            "invalid_thread_id",
            "DeerFlow thread ID contains invalid characters".into(),
            false,
        ));
    }
    Ok(())
}

fn validate_run_id(run_id: &str) -> Result<(), AgentAdapterError> {
    if run_id.is_empty()
        || run_id.len() > 128
        || !run_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-.".contains(&byte))
    {
        return Err(adapter_error(
            "invalid_run_id",
            "DeerFlow run ID contains invalid characters".into(),
            false,
        ));
    }
    Ok(())
}

fn thread_id_for_task(task_id: &str) -> String {
    // Stable FNV-1a mapping makes thread creation idempotent across a retry
    // after an uncertain HTTP result without exposing arbitrary task text.
    let hash = task_id.bytes().fold(0xcbf29ce484222325_u64, |state, byte| {
        (state ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    });
    format!("dread-{hash:016x}")
}

fn run_state(status: &str) -> AgentTaskState {
    match status {
        "pending" | "queued" => AgentTaskState::Queued,
        "running" | "stopping" => AgentTaskState::Running,
        "success" | "completed" => AgentTaskState::Succeeded,
        "cancelled" | "interrupted" => AgentTaskState::Cancelled,
        "error" | "failed" => AgentTaskState::Failed,
        _ => AgentTaskState::Unknown,
    }
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapter_stays_disabled_until_container_sandbox_is_attested() {
        let adapter = DeerFlowApiAdapter::new(
            "http://127.0.0.1:2026",
            "/api/langgraph",
            "dfp_test-token",
            "DeerFlow",
        )
        .unwrap();
        assert!(!adapter.descriptor().enabled);
        assert!(
            adapter
                .clone()
                .attest_container_sandbox("LocalSandboxProvider", true, None)
                .is_err()
        );
        assert!(
            adapter
                .clone()
                .attest_container_sandbox("AioSandboxProvider", false, None)
                .is_err()
        );
        assert!(
            adapter
                .clone()
                .attest_container_sandbox(
                    "deerflow.community.aio_sandbox:AioSandboxProvider",
                    true,
                    Some("http://provisioner:8002"),
                )
                .is_err()
        );
        let enabled = adapter
            .attest_container_sandbox("deerflow.sandbox.aio:AioSandboxProvider", true, None)
            .unwrap();
        assert!(enabled.descriptor().enabled);
    }

    #[test]
    fn rejects_remote_endpoints_and_unknown_api_prefixes() {
        assert!(
            DeerFlowApiAdapter::new("http://example.com:2026", "/api", "dfp_token", "DeerFlow")
                .is_err()
        );
        assert!(
            DeerFlowApiAdapter::new("http://localhost:2026", "/api", "dfp_token", "DeerFlow")
                .is_err()
        );
        assert!(
            DeerFlowApiAdapter::new("http://127.0.0.1:2026", "/other", "dfp_token", "DeerFlow")
                .is_err()
        );
        assert!(
            DeerFlowApiAdapter::new(
                "http://user:pass@127.0.0.1:2026",
                "/api",
                "dfp_token",
                "DeerFlow"
            )
            .is_err()
        );
    }

    #[test]
    fn maps_deerflow_terminal_statuses_without_success_bias() {
        assert_eq!(run_state("success"), AgentTaskState::Succeeded);
        assert_eq!(run_state("completed"), AgentTaskState::Succeeded);
        assert_eq!(run_state("error"), AgentTaskState::Failed);
        assert_eq!(run_state("failed"), AgentTaskState::Failed);
        assert_eq!(run_state("cancelled"), AgentTaskState::Cancelled);
        assert_eq!(run_state("interrupted"), AgentTaskState::Cancelled);
        assert_eq!(run_state("running"), AgentTaskState::Running);
        assert_eq!(run_state("future-status"), AgentTaskState::Unknown);
    }

    #[test]
    fn thread_id_is_stable_and_provider_id_validation_is_strict() {
        assert_eq!(
            thread_id_for_task("task_123"),
            thread_id_for_task("task_123")
        );
        assert!(validate_thread_id("dread-123").is_ok());
        assert!(validate_thread_id("../etc").is_err());
        assert!(validate_run_id("run-123").is_ok());
        assert!(validate_run_id("../run").is_err());
        assert!(validate_idempotency_key("dread-task-123").is_ok());
        assert!(validate_idempotency_key("bad\nkey").is_err());
    }

    #[tokio::test]
    async fn deerflow_http_lifecycle_uses_pat_idempotency_and_cancel() {
        use axum::{
            Json, Router,
            extract::{Path, Query, State},
            http::{HeaderMap, StatusCode, header},
            response::IntoResponse,
            routing::{get, post},
        };
        use serde_json::{Value, json};
        use std::sync::{Arc, Mutex};
        use tokio::net::TcpListener;

        #[derive(Clone)]
        struct MockState {
            requests: Arc<Mutex<Vec<Value>>>,
            status: Arc<Mutex<String>>,
        }

        async fn create_thread(
            State(state): State<MockState>,
            headers: HeaderMap,
            Json(body): Json<Value>,
        ) -> Json<Value> {
            state.requests.lock().unwrap().push(json!({
                "path":"/api/threads",
                "authorization":headers.get(header::AUTHORIZATION).and_then(|v|v.to_str().ok()),
                "body":body
            }));
            Json(json!({"thread_id":body["thread_id"]}))
        }

        async fn create_run(
            State(state): State<MockState>,
            Path(thread_id): Path<String>,
            headers: HeaderMap,
            Json(body): Json<Value>,
        ) -> (StatusCode, Json<Value>) {
            state.requests.lock().unwrap().push(json!({
                "path":format!("/api/threads/{thread_id}/runs"),
                "authorization":headers.get(header::AUTHORIZATION).and_then(|v|v.to_str().ok()),
                "idempotency_key":headers.get("idempotency-key").and_then(|v|v.to_str().ok()),
                "body":body
            }));
            (
                StatusCode::ACCEPTED,
                Json(json!({"thread_id":thread_id,"run_id":"run-123","status":"pending"})),
            )
        }

        async fn get_run(
            State(state): State<MockState>,
            Path((_thread_id, run_id)): Path<(String, String)>,
        ) -> Json<Value> {
            let status = state.status.lock().unwrap().clone();
            Json(json!({"run_id":run_id,"status":status,"result":{"answer":"done"}}))
        }

        async fn get_events(
            Path((_thread_id, _run_id)): Path<(String, String)>,
            Query(query): Query<std::collections::HashMap<String, String>>,
        ) -> Json<Value> {
            assert!(query.is_empty());
            Json(json!([{"seq":1,"event":"run.started","data":{"status":"running"}}]))
        }

        async fn cancel_run(
            State(state): State<MockState>,
            Path((_thread_id, run_id)): Path<(String, String)>,
            Query(query): Query<std::collections::HashMap<String, String>>,
        ) -> impl IntoResponse {
            state.requests.lock().unwrap().push(json!({
                "path":format!("/cancel/{run_id}"),
                "action":query.get("action"),
                "wait":query.get("wait")
            }));
            *state.status.lock().unwrap() = "cancelled".into();
            StatusCode::NO_CONTENT
        }

        async fn get_messages(Path((_thread_id, _run_id)): Path<(String, String)>) -> Json<Value> {
            Json(
                json!({"data":[{"role":"assistant","content":"verified result"}],"has_more":false}),
            )
        }

        let state = MockState {
            requests: Arc::new(Mutex::new(Vec::new())),
            status: Arc::new(Mutex::new("success".into())),
        };
        let app = Router::new()
            .route(
                "/health/ready",
                get(|| async { Json(json!({"status":"ready"})) }),
            )
            .route("/api/threads", post(create_thread))
            .route("/api/threads/{thread_id}/runs", post(create_run))
            .route("/api/threads/{thread_id}/runs/{run_id}", get(get_run))
            .route(
                "/api/threads/{thread_id}/runs/{run_id}/events",
                get(get_events),
            )
            .route(
                "/api/threads/{thread_id}/runs/{run_id}/cancel",
                post(cancel_run),
            )
            .route(
                "/api/threads/{thread_id}/runs/{run_id}/messages",
                get(get_messages),
            )
            .with_state(state.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let adapter = DeerFlowApiAdapter::new(
            &format!("http://{address}"),
            "/api",
            "dfp_test-token",
            "DeerFlow test",
        )
        .unwrap()
        .attest_container_sandbox("AioSandboxProvider", true, None)
        .unwrap();

        assert_eq!(
            adapter.health().await.unwrap().state,
            AgentHealthState::Online
        );
        assert_eq!(adapter.capabilities().await.unwrap().len(), 1);
        let started = adapter
            .start_task(StartTaskRequest {
                task_id: "dread-task-123".into(),
                capability_id: "deerflow.run".into(),
                input: json!({"objective":"inspect approved workspace"}),
                idempotency_key: "dread-task-123".into(),
            })
            .await
            .unwrap();
        assert_eq!(started.task_id, "dread-task-123");
        assert_eq!(started.state, AgentTaskState::Queued);
        let provider_id = started.provider_task_id.clone().unwrap();

        let status = adapter
            .task_status("dread-task-123", &provider_id)
            .await
            .unwrap();
        assert_eq!(status.state, AgentTaskState::Succeeded);
        let events = adapter
            .events("dread-task-123", &provider_id, 0)
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].sequence, 1);
        let result = adapter
            .task_result("dread-task-123", &provider_id)
            .await
            .unwrap();
        assert_eq!(
            result.result.unwrap()["messages"][0]["content"],
            "verified result"
        );

        let cancelled = adapter
            .cancel_task("dread-task-123", &provider_id)
            .await
            .unwrap();
        assert_eq!(cancelled.state, AgentTaskState::Cancelled);

        let requests = state.requests.lock().unwrap();
        let run_request = requests
            .iter()
            .find(|row| row["path"].as_str().unwrap_or("").ends_with("/runs"))
            .unwrap();
        assert_eq!(run_request["authorization"], "Bearer dfp_test-token");
        assert_eq!(run_request["idempotency_key"], "dread-task-123");
        assert_eq!(run_request["body"]["on_disconnect"], "continue");
        assert_eq!(run_request["body"]["multitask_strategy"], "reject");
        assert!(
            requests
                .iter()
                .any(|row| row["action"] == "interrupt" && row["wait"] == "true")
        );
    }
}
