//! OpenHands Agent Server adapter.
//!
//! Every DREAD conversation requests DockerExecutionWorkspace. The adapter only
//! enables execution when authenticated /server_info advertises execution_runtime
//! = "docker" and exposes the exact tools requested by DREAD. AlwaysConfirm is
//! retained. These server claims still require live host/container validation.

use crate::{
    signed_agent_operation_receipt, durable_evidence_error, AdapterFuture, AgentAdapter,
    AgentAdapterError, AgentCapability, AgentDescriptor, AgentEvent, AgentExecutionIdentity,
    AgentHealth, AgentHealthState, AgentOperationEvidenceKind, AgentOperationReceipt,
    AgentReceiptSigner, AgentTask, AgentTaskState, DurableAgentStartRequest, StartTaskRequest,
};
use reqwest::{Client, Method, StatusCode, Url};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    net::IpAddr,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const MAX_OBJECTIVE_BYTES: usize = 20_000;
const MAX_INSTRUCTIONS_BYTES: usize = 20_000;
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const MAX_EVENT_PAGES: usize = 100;
const EVENTS_PER_PAGE: &str = "100";

#[derive(Clone)]
pub struct OpenHandsAgentServerAdapter {
    descriptor: AgentDescriptor,
    client: Client,
    base_url: Url,
    session_api_key: String,
    llm_model: String,
    llm_api_key: String,
    llm_base_url: String,
    sandbox_attested: bool,
    secret_store_attested: bool,
    receipt_signer: Option<AgentReceiptSigner>,
}

impl OpenHandsAgentServerAdapter {
    /// Construct a disabled, loopback-only adapter.
    ///
    /// DockerExecutionWorkspace selects the Agent Server's per-conversation
    /// execution workspace. Never substitute LocalWorkspace: it may execute in
    /// the Agent Server process. /server_info must advertise execution_runtime=docker.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        base_url: &str,
        session_api_key: impl Into<String>,
        llm_model: impl Into<String>,
        llm_api_key: impl Into<String>,
        llm_base_url: impl Into<String>,
        display_name: impl Into<String>,
    ) -> Result<Self, AgentAdapterError> {
        let mut url = Url::parse(base_url).map_err(|error| {
            adapter_error(
                "invalid_endpoint",
                format!("Invalid OpenHands URL: {error}"),
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
                "OpenHands Agent Server requires an explicit-port loopback HTTP origin without credentials, path, query or fragment".into(),
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
                "OpenHands Agent Server must be addressed through a loopback IP".into(),
                false,
            ));
        }

        let session_api_key = session_api_key.into();
        let llm_model = llm_model.into();
        let llm_api_key = llm_api_key.into();
        let llm_base_url = llm_base_url.into();
        if session_api_key.trim().is_empty() || session_api_key.len() > 4096 {
            return Err(adapter_error(
                "missing_session_key",
                "A non-empty OpenHands session API key is required".into(),
                false,
            ));
        }
        if llm_model.trim().is_empty() || llm_model.len() > 256 {
            return Err(adapter_error(
                "invalid_model",
                "OpenHands LLM model must contain 1-256 characters".into(),
                false,
            ));
        }
        if llm_api_key.trim().is_empty() || llm_api_key.len() > 4096 {
            return Err(adapter_error(
                "missing_llm_key",
                "A non-empty OpenHands LLM API key is required".into(),
                false,
            ));
        }
        validate_llm_base_url(&llm_base_url)?;

        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| adapter_error("client_error", error.to_string(), false))?;
        let endpoint = url.as_str().trim_end_matches('/').to_string();
        url.set_path("");
        Ok(Self {
            descriptor: AgentDescriptor {
                id: "openhands.agent_server".into(),
                display_name: display_name.into(),
                adapter: "openhands.agent_server".into(),
                version: None,
                endpoint: Some(endpoint),
                enabled: false,
            },
            client,
            base_url: url,
            session_api_key,
            llm_model,
            llm_api_key,
            llm_base_url,
            sandbox_attested: false,
            secret_store_attested: false,
            receipt_signer: None,
        })
    }

    /// Attach the trusted local receipt signer. Durable lifecycle operations
    /// remain unavailable until a key anchored by the signed provider registry
    /// is configured by the caller.
    pub fn with_receipt_signer(mut self, signer: AgentReceiptSigner) -> Self {
        self.receipt_signer = Some(signer);
        self
    }

    /// Enable only after Docker execution and stable OH_SECRET_KEY storage have
    /// been configured on the Agent Server. Before advertising or starting a task,
    /// DREAD reads authoritative /server_info execution_runtime and usable_tools.
    pub fn attest_docker_runtime(
        mut self,
        docker_runtime_enabled: bool,
        secret_store_configured: bool,
    ) -> Result<Self, AgentAdapterError> {
        if !docker_runtime_enabled {
            return Err(adapter_error(
                "docker_runtime_required",
                "OpenHands requires Docker execution runtime selection on the Agent Server".into(),
                false,
            ));
        }
        if !secret_store_configured {
            return Err(adapter_error(
                "secret_store_required",
                "OpenHands OH_SECRET_KEY must be configured and stable across restarts".into(),
                false,
            ));
        }
        self.sandbox_attested = true;
        self.secret_store_attested = true;
        self.descriptor.enabled = true;
        Ok(self)
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{}{}", self.base_url.as_str().trim_end_matches('/'), path)
    }

    fn require_attested(&self) -> Result<(), AgentAdapterError> {
        if self.sandbox_attested && self.secret_store_attested && self.descriptor.enabled {
            Ok(())
        } else {
            Err(adapter_error(
                "sandbox_not_attested",
                "OpenHands execution is disabled until Docker execution runtime and stable OH_SECRET_KEY storage are configured".into(),
                false,
            ))
        }
    }

    async fn require_docker_runtime(&self) -> Result<(), AgentAdapterError> {
        let server_info = match self
            .json_request(Method::GET, "/server_info", None, &[])
            .await
        {
            Ok(value) => value,
            Err(error) if error.code == "not_found" => {
                return Err(adapter_error(
                    "docker_runtime_unavailable",
                    "OpenHands does not expose /server_info; refusing to assume Docker runtime availability".into(),
                    false,
                ));
            }
            Err(error) => return Err(error),
        };

        if !server_supports_docker_runtime(&server_info) {
            return Err(adapter_error(
                "docker_runtime_unavailable",
                "OpenHands /server_info does not advertise execution_runtime=docker".into(),
                false,
            ));
        }
        if !server_has_required_tools(&server_info) {
            return Err(adapter_error(
                "required_tools_unavailable",
                "OpenHands /server_info does not expose all DREAD-required tools: terminal, file_editor, task_tracker".into(),
                false,
            ));
        }
        Ok(())
    }

    async fn json_request(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
        query: &[(&str, String)],
    ) -> Result<Value, AgentAdapterError> {
        let mut request = self
            .client
            .request(method, self.endpoint(path))
            .header("X-Session-API-Key", &self.session_api_key)
            .timeout(Duration::from_secs(30));
        if !query.is_empty() {
            request = request.query(query);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.map_err(|error| {
            adapter_error(
                "transport_error",
                error.to_string(),
                error.is_timeout() || error.is_connect(),
            )
        })?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|error| adapter_error("response_error", error.to_string(), true))?;
        if bytes.len() > MAX_RESPONSE_BYTES {
            return Err(adapter_error(
                "response_too_large",
                "OpenHands response exceeded the 2 MiB limit".into(),
                false,
            ));
        }
        if status == StatusCode::NOT_FOUND {
            return Err(adapter_error(
                "not_found",
                "OpenHands resource was not found".into(),
                false,
            ));
        }
        if status == StatusCode::CONFLICT {
            return Err(adapter_error(
                "conflict",
                "OpenHands conversation already exists or is running".into(),
                false,
            ));
        }
        if !status.is_success() {
            return Err(adapter_error(
                "http_error",
                format!(
                    "OpenHands returned HTTP {}: {}",
                    status.as_u16(),
                    String::from_utf8_lossy(&bytes)
                ),
                status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS,
            ));
        }
        serde_json::from_slice(&bytes).map_err(|error| {
            adapter_error(
                "invalid_response",
                format!("OpenHands returned invalid JSON: {error}"),
                false,
            )
        })
    }

    async fn conversation(
        &self,
        conversation_id: &str,
    ) -> Result<Option<Value>, AgentAdapterError> {
        validate_task_id(conversation_id)?;
        match self
            .json_request(
                Method::GET,
                &format!("/api/conversations/{conversation_id}"),
                None,
                &[],
            )
            .await
        {
            Ok(value) => {
                if value.get("id").and_then(Value::as_str) != Some(conversation_id) {
                    return Err(adapter_error(
                        "provider_id_mismatch",
                        "OpenHands returned a conversation ID different from the DREAD task ID"
                            .into(),
                        false,
                    ));
                }
                Ok(Some(value))
            }
            Err(error) if error.code == "not_found" => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn to_task(&self, task_id: &str, conversation: &Value) -> AgentTask {
        let status = conversation
            .get("execution_status")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let state = match status {
            "idle" => AgentTaskState::Queued,
            "running" => AgentTaskState::Running,
            "waiting_for_confirmation" => AgentTaskState::WaitingForConfirmation,
            "finished" => AgentTaskState::Succeeded,
            "error" | "stuck" => AgentTaskState::Failed,
            "paused" | "deleting" => AgentTaskState::Cancelled,
            _ => AgentTaskState::Unknown,
        };
        AgentTask {
            task_id: task_id.to_string(),
            state,
            provider_task_id: Some(task_id.to_string()),
            result: None,
            error: conversation
                .get("error")
                .and_then(Value::as_str)
                .map(str::to_string),
            updated_at: now_epoch(),
        }
    }

    fn start_payload(&self, request: &StartTaskRequest) -> Result<Value, AgentAdapterError> {
        if request.capability_id != "openhands.run" {
            return Err(adapter_error(
                "unsupported_capability",
                "OpenHands adapter supports only openhands.run".into(),
                false,
            ));
        }
        if request.idempotency_key != format!("project-dread:{}", request.task_id) {
            return Err(adapter_error(
                "invalid_idempotency_key",
                "OpenHands idempotency key must be derived from the DREAD task ID".into(),
                false,
            ));
        }
        let object = request.input.as_object().ok_or_else(|| {
            adapter_error(
                "invalid_input",
                "OpenHands input must be a JSON object".into(),
                false,
            )
        })?;
        if object
            .keys()
            .any(|key| !["objective", "instructions", "max_iterations"].contains(&key.as_str()))
        {
            return Err(adapter_error(
                "invalid_input",
                "OpenHands input contains unsupported fields".into(),
                false,
            ));
        }
        let objective = request
            .input
            .get("objective")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                adapter_error(
                    "invalid_input",
                    "OpenHands input requires a text objective".into(),
                    false,
                )
            })?;
        if objective.trim().is_empty() || objective.len() > MAX_OBJECTIVE_BYTES {
            return Err(adapter_error(
                "invalid_input",
                "OpenHands objective must contain 1-20000 bytes".into(),
                false,
            ));
        }
        let instructions = request
            .input
            .get("instructions")
            .and_then(Value::as_str)
            .unwrap_or("");
        if instructions.len() > MAX_INSTRUCTIONS_BYTES {
            return Err(adapter_error(
                "invalid_input",
                "OpenHands instructions exceed 20000 bytes".into(),
                false,
            ));
        }
        let max_iterations = request
            .input
            .get("max_iterations")
            .and_then(Value::as_u64)
            .unwrap_or(100);
        if !(1..=200).contains(&max_iterations) {
            return Err(adapter_error(
                "invalid_input",
                "OpenHands max_iterations must be between 1 and 200".into(),
                false,
            ));
        }
        let message = if instructions.trim().is_empty() {
            objective.to_string()
        } else {
            format!("{instructions}\n\nObjective:\n{objective}")
        };
        let llm = json!({
            "model": self.llm_model,
            "api_key": self.llm_api_key,
            "base_url": self.llm_base_url,
            "temperature": 0,
            "stream": false
        });
        Ok(json!({
            "conversation_id": request.task_id,
            "agent": {
                "kind": "Agent",
                "llm": llm,
                "tools": [
                    {"name": "terminal"},
                    {"name": "file_editor"},
                    {"name": "task_tracker"}
                ]
            },
            "workspace": {
                "kind": "DockerExecutionWorkspace",
                "working_dir": "/workspace"
            },
            "worktree": false,
            "confirmation_policy": {"kind": "AlwaysConfirm"},
            "initial_message": {
                "role": "user",
                "content": [{"type": "text", "text": message}],
                "run": true
            },
            "max_iterations": max_iterations,
            "stuck_detection": true
        }))
    }
}

impl AgentAdapter for OpenHandsAgentServerAdapter {
    fn descriptor(&self) -> &AgentDescriptor {
        &self.descriptor
    }

    fn health<'a>(&'a self) -> AdapterFuture<'a, AgentHealth> {
        Box::pin(async move {
            let response = self
                .client
                .get(self.endpoint("/ready"))
                .header("X-Session-API-Key", &self.session_api_key)
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
            let docker_runtime_available =
                status.is_success() && self.require_docker_runtime().await.is_ok();
            Ok(AgentHealth {
                state: if docker_runtime_available {
                    AgentHealthState::Online
                } else if status.is_success()
                    || status.is_server_error()
                    || status == StatusCode::SERVICE_UNAVAILABLE
                {
                    AgentHealthState::Degraded
                } else {
                    AgentHealthState::Unknown
                },
                checked_at: now_epoch(),
                detail: Some(if docker_runtime_available {
                    "OpenHands /ready succeeded and Docker runtime is available".into()
                } else if status.is_success() {
                    "OpenHands is ready but Docker runtime availability could not be verified"
                        .into()
                } else {
                    format!("OpenHands /ready returned HTTP {}", status.as_u16())
                }),
            })
        })
    }

    fn capabilities<'a>(&'a self) -> AdapterFuture<'a, Vec<AgentCapability>> {
        Box::pin(async move {
            if !self.sandbox_attested || !self.secret_store_attested || !self.descriptor.enabled {
                return Ok(Vec::new());
            }
            if self.require_docker_runtime().await.is_err() {
                return Ok(Vec::new());
            }
            Ok(vec![AgentCapability {
                id: "openhands.run".into(),
                name: "OpenHands isolated agent run".into(),
                description: "Run an approved objective in an isolated OpenHands Agent Server container workspace; DREAD polls durable conversation events and status and pauses the conversation on cancellation.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "objective": {"type": "string", "minLength": 1, "maxLength": 20000},
                        "instructions": {"type": "string", "maxLength": 20000},
                        "max_iterations": {"type": "integer", "minimum": 1, "maximum": 200}
                    },
                    "required": ["objective"],
                    "additionalProperties": false
                }),
                output_schema: Some(json!({"type": "object", "properties": {"messages": {"type": "array"}, "final_message": {"type": "string"}}})),
            }])
        })
    }

    fn start_task<'a>(&'a self, request: StartTaskRequest) -> AdapterFuture<'a, AgentTask> {
        Box::pin(async move {
            self.require_attested()?;
            validate_task_id(&request.task_id)?;
            if let Some(existing) = self.conversation(&request.task_id).await? {
                return Ok(self.to_task(&request.task_id, &existing));
            }
            let body = self.start_payload(&request)?;
            self.require_docker_runtime().await?;
            match self
                .json_request(Method::POST, "/api/conversations", Some(body), &[])
                .await
            {
                Ok(value) => Ok(self.to_task(&request.task_id, &value)),
                Err(error) if error.code == "conflict" || error.retryable => {
                    if let Some(existing) = self.conversation(&request.task_id).await? {
                        Ok(self.to_task(&request.task_id, &existing))
                    } else {
                        Err(error)
                    }
                }
                Err(error) => Err(error),
            }
        })
    }

    fn task_status<'a>(
        &'a self,
        task_id: &'a str,
        provider_task_id: &'a str,
    ) -> AdapterFuture<'a, AgentTask> {
        Box::pin(async move {
            self.require_attested()?;
            validate_task_id(task_id)?;
            if provider_task_id != task_id {
                return Err(adapter_error(
                    "provider_id_mismatch",
                    "OpenHands conversation ID must match the DREAD task ID".into(),
                    false,
                ));
            }
            let conversation = self.conversation(provider_task_id).await?.ok_or_else(|| {
                adapter_error(
                    "not_found",
                    "OpenHands conversation does not exist".into(),
                    false,
                )
            })?;
            Ok(self.to_task(task_id, &conversation))
        })
    }

    fn events<'a>(
        &'a self,
        task_id: &'a str,
        provider_task_id: &'a str,
        after_sequence: u64,
    ) -> AdapterFuture<'a, Vec<AgentEvent>> {
        Box::pin(async move {
            self.require_attested()?;
            validate_task_id(task_id)?;
            if provider_task_id != task_id {
                return Err(adapter_error(
                    "provider_id_mismatch",
                    "OpenHands conversation ID must match the DREAD task ID".into(),
                    false,
                ));
            }
            let mut page_id: Option<String> = None;
            let mut seen_pages = HashSet::new();
            let mut sequence = 0u64;
            let mut output = Vec::new();
            for _ in 0..MAX_EVENT_PAGES {
                let mut query = vec![
                    ("limit", EVENTS_PER_PAGE.to_string()),
                    ("sort_order", "TIMESTAMP".to_string()),
                ];
                if let Some(page) = page_id.as_ref() {
                    query.push(("page_id", page.clone()));
                }
                let page = self
                    .json_request(
                        Method::GET,
                        &format!("/api/conversations/{provider_task_id}/events/search"),
                        None,
                        &query,
                    )
                    .await?;
                let items = page.get("items").and_then(Value::as_array).ok_or_else(|| {
                    adapter_error(
                        "invalid_response",
                        "OpenHands event search response has no items array".into(),
                        false,
                    )
                })?;
                for item in items {
                    sequence = sequence.saturating_add(1);
                    if sequence > after_sequence {
                        output.push(AgentEvent {
                            task_id: task_id.to_string(),
                            sequence,
                            event_type: item
                                .get("kind")
                                .and_then(Value::as_str)
                                .unwrap_or("unknown")
                                .to_string(),
                            payload: item.clone(),
                            occurred_at: now_epoch(),
                        });
                    }
                }
                let next = page.get("next_page_id").and_then(Value::as_str);
                match next {
                    Some(next) if !next.is_empty() => {
                        if next.len() > 512 || !seen_pages.insert(next.to_string()) {
                            return Err(adapter_error(
                                "invalid_pagination",
                                "OpenHands returned an invalid or repeated event page cursor"
                                    .into(),
                                false,
                            ));
                        }
                        page_id = Some(next.to_string());
                    }
                    _ => return Ok(output),
                }
            }
            Err(adapter_error(
                "event_history_limit",
                "OpenHands event history exceeded the 100-page safety limit".into(),
                false,
            ))
        })
    }

    fn cancel_task<'a>(
        &'a self,
        task_id: &'a str,
        provider_task_id: &'a str,
    ) -> AdapterFuture<'a, AgentTask> {
        Box::pin(async move {
            self.require_attested()?;
            validate_task_id(task_id)?;
            if provider_task_id != task_id {
                return Err(adapter_error(
                    "provider_id_mismatch",
                    "OpenHands conversation ID must match the DREAD task ID".into(),
                    false,
                ));
            }
            let response = self
                .json_request(
                    Method::POST,
                    &format!("/api/conversations/{provider_task_id}/pause"),
                    None,
                    &[],
                )
                .await?;
            if response.get("success").and_then(Value::as_bool) != Some(true) {
                return Err(adapter_error(
                    "provider_cancel_not_confirmed",
                    "OpenHands Agent Server did not confirm that the conversation was paused"
                        .into(),
                    false,
                ));
            }
            Ok(AgentTask {
                task_id: task_id.to_string(),
                state: AgentTaskState::Cancelled,
                provider_task_id: Some(provider_task_id.to_string()),
                result: None,
                error: None,
                updated_at: now_epoch(),
            })
        })
    }

    fn respond_to_confirmation<'a>(
        &'a self,
        task_id: &'a str,
        provider_task_id: &'a str,
        accept: bool,
        reason: &'a str,
    ) -> AdapterFuture<'a, ()> {
        Box::pin(async move {
            self.require_attested()?;
            validate_task_id(task_id)?;
            if provider_task_id != task_id {
                return Err(adapter_error(
                    "provider_id_mismatch",
                    "OpenHands conversation ID must match the DREAD task ID".into(),
                    false,
                ));
            }
            let reason = reason.trim();
            if reason.is_empty() || reason.len() > 2000 {
                return Err(adapter_error(
                    "invalid_confirmation_reason",
                    "Confirmation reason must contain 1-2000 characters".into(),
                    false,
                ));
            }
            let conversation = self.conversation(provider_task_id).await?.ok_or_else(|| {
                adapter_error(
                    "not_found",
                    "OpenHands conversation does not exist".into(),
                    false,
                )
            })?;
            if self.to_task(task_id, &conversation).state != AgentTaskState::WaitingForConfirmation
            {
                return Err(adapter_error(
                    "confirmation_not_pending",
                    "OpenHands conversation is not waiting for operator confirmation".into(),
                    false,
                ));
            }
            self.json_request(
                Method::POST,
                &format!("/api/conversations/{provider_task_id}/events/respond_to_confirmation"),
                Some(json!({"accept": accept, "reason": reason})),
                &[],
            )
            .await?;
            Ok(())
        })
    }

    fn task_result<'a>(
        &'a self,
        task_id: &'a str,
        provider_task_id: &'a str,
    ) -> AdapterFuture<'a, AgentTask> {
        Box::pin(async move {
            let mut task = self.task_status(task_id, provider_task_id).await?;
            if task.state != AgentTaskState::Succeeded {
                return Ok(task);
            }
            let events = self.events(task_id, provider_task_id, 0).await?;
            let mut messages = Vec::new();
            for event in events {
                if event.event_type != "MessageEvent"
                    || event.payload.get("source").and_then(Value::as_str) != Some("agent")
                {
                    continue;
                }
                let content = event.payload.pointer("/llm_message/content");
                let text = match content {
                    Some(Value::Array(items)) => items
                        .iter()
                        .filter_map(|item| item.get("text").and_then(Value::as_str))
                        .collect::<Vec<_>>()
                        .join("\n"),
                    Some(Value::String(value)) => value.clone(),
                    _ => String::new(),
                };
                if !text.trim().is_empty() {
                    messages.push(text);
                }
            }
            let final_message = messages.last().cloned().unwrap_or_default();
            task.result = Some(json!({"messages": messages, "final_message": final_message}));
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
            let response = self.start_task(request.request).await?;
            signed_agent_operation_receipt(
                self.receipt_signer
                    .as_ref()
                    .ok_or_else(|| durable_evidence_error("OpenHands receipt signer is not configured"))?,
                &self.descriptor,
                "openhands",
                &identity,
                AgentOperationEvidenceKind::Acknowledged,
                response.clone(),
                response.provider_task_id.clone(),
                None,
                None,
            )
        })
    }

    fn task_status_with_evidence<'a>(
        &'a self,
        identity: &'a AgentExecutionIdentity,
        provider_execution_id: &'a str,
    ) -> AdapterFuture<'a, AgentOperationReceipt<AgentTask>> {
        Box::pin(async move {
            let response = self.task_status(&identity.task_id, provider_execution_id).await?;
            let kind = match response.state {
                AgentTaskState::Succeeded => AgentOperationEvidenceKind::Succeeded,
                AgentTaskState::Failed => AgentOperationEvidenceKind::Failed,
                AgentTaskState::Cancelled => AgentOperationEvidenceKind::Cancelled,
                _ => AgentOperationEvidenceKind::Active,
            };
            signed_agent_operation_receipt(
                self.receipt_signer
                    .as_ref()
                    .ok_or_else(|| durable_evidence_error("OpenHands receipt signer is not configured"))?,
                &self.descriptor,
                "openhands",
                identity,
                kind,
                response,
                Some(provider_execution_id.to_owned()),
                None,
                None,
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
            let events = self.events(&identity.task_id, provider_execution_id, after_sequence).await?;
            let signer = self.receipt_signer
                .as_ref()
                .ok_or_else(|| durable_evidence_error("OpenHands receipt signer is not configured"))?;
            let mut receipts = Vec::with_capacity(events.len());
            for event in events {
                let external_event_id = format!("{provider_execution_id}:{}", event.sequence);
                let event_type = event.event_type.clone();
                receipts.push(signed_agent_operation_receipt(
                    signer,
                    &self.descriptor,
                    "openhands",
                    identity,
                    AgentOperationEvidenceKind::Event,
                    event,
                    Some(provider_execution_id.to_owned()),
                    Some(external_event_id),
                    Some(event_type),
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
            let response = self.cancel_task(&identity.task_id, provider_execution_id).await?;
            let kind = match response.state {
                AgentTaskState::Cancelled => AgentOperationEvidenceKind::Cancelled,
                AgentTaskState::Queued | AgentTaskState::Running | AgentTaskState::WaitingForConfirmation => AgentOperationEvidenceKind::CancellationAccepted,
                _ => AgentOperationEvidenceKind::CancellationUnknown,
            };
            signed_agent_operation_receipt(
                self.receipt_signer
                    .as_ref()
                    .ok_or_else(|| durable_evidence_error("OpenHands receipt signer is not configured"))?,
                &self.descriptor,
                "openhands",
                identity,
                kind,
                response,
                Some(provider_execution_id.to_owned()),
                None,
                None,
            )
        })
    }

    fn task_result_with_evidence<'a>(
        &'a self,
        identity: &'a AgentExecutionIdentity,
        provider_execution_id: &'a str,
    ) -> AdapterFuture<'a, AgentOperationReceipt<AgentTask>> {
        Box::pin(async move {
            let response = self.task_result(&identity.task_id, provider_execution_id).await?;
            let kind = match response.state {
                AgentTaskState::Succeeded => AgentOperationEvidenceKind::Succeeded,
                AgentTaskState::Failed => AgentOperationEvidenceKind::Failed,
                AgentTaskState::Cancelled => AgentOperationEvidenceKind::Cancelled,
                _ => AgentOperationEvidenceKind::Active,
            };
            signed_agent_operation_receipt(
                self.receipt_signer
                    .as_ref()
                    .ok_or_else(|| durable_evidence_error("OpenHands receipt signer is not configured"))?,
                &self.descriptor,
                "openhands",
                identity,
                kind,
                response,
                Some(provider_execution_id.to_owned()),
                None,
                None,
            )
        })
    }
}

fn server_supports_docker_runtime(server_info: &Value) -> bool {
    server_info.get("execution_runtime").and_then(Value::as_str) == Some("docker")
}

fn server_has_required_tools(server_info: &Value) -> bool {
    const REQUIRED_TOOLS: [&str; 3] = ["terminal", "file_editor", "task_tracker"];
    let Some(usable_tools) = server_info.get("usable_tools").and_then(Value::as_array) else {
        return false;
    };
    REQUIRED_TOOLS.iter().all(|required| {
        usable_tools
            .iter()
            .any(|tool| tool.as_str() == Some(required))
    })
}

fn validate_task_id(task_id: &str) -> Result<(), AgentAdapterError> {
    let bytes = task_id.as_bytes();
    if bytes.len() != 36
        || ![8, 13, 18, 23].iter().all(|index| bytes[*index] == b'-')
        || !bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| [8, 13, 18, 23].contains(&index) || byte.is_ascii_hexdigit())
    {
        return Err(adapter_error(
            "invalid_task_id",
            "OpenHands requires a canonical UUID DREAD task ID".into(),
            false,
        ));
    }
    Ok(())
}

fn validate_llm_base_url(value: &str) -> Result<(), AgentAdapterError> {
    let url = Url::parse(value)
        .map_err(|error| adapter_error("invalid_llm_base_url", error.to_string(), false))?;
    if url.scheme() != "http"
        || url.host_str() != Some("host.docker.internal")
        || url.port() != Some(11500)
        || url.path() != "/v1"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(adapter_error(
            "unsafe_llm_base_url",
            "OpenHands must use DREAD's Docker-reachable local GPU gateway at http://host.docker.internal:11500/v1".into(),
            false,
        ));
    }
    Ok(())
}

fn adapter_error(code: &str, message: String, retryable: bool) -> AgentAdapterError {
    AgentAdapterError {
        code: code.into(),
        message,
        retryable,
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
    use axum::{
        Json, Router,
        extract::Path,
        http::HeaderMap,
        routing::{get, post},
    };
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use tokio::net::TcpListener;

    async fn ready() -> &'static str {
        "ready"
    }

    async fn docker_server_info(headers: HeaderMap) -> Json<Value> {
        assert_eq!(
            headers
                .get("x-session-api-key")
                .and_then(|value| value.to_str().ok()),
            Some("session-secret")
        );
        Json(json!({
            "execution_runtime": "docker",
            "usable_tools": ["terminal", "file_editor", "task_tracker"]
        }))
    }

    async fn create_conversation(headers: HeaderMap, Json(body): Json<Value>) -> Json<Value> {
        assert_eq!(
            headers
                .get("x-session-api-key")
                .and_then(|v| v.to_str().ok()),
            Some("session-secret")
        );
        let id = body
            .get("conversation_id")
            .and_then(Value::as_str)
            .unwrap()
            .to_string();
        assert_eq!(
            body.pointer("/workspace/kind").and_then(Value::as_str),
            Some("DockerExecutionWorkspace")
        );
        assert_eq!(
            body.pointer("/workspace/working_dir")
                .and_then(Value::as_str),
            Some("/workspace")
        );
        assert_eq!(body["worktree"], false);
        assert!(body.get("conversation_runtime").is_none());
        assert_eq!(body["confirmation_policy"]["kind"], "AlwaysConfirm");
        assert_eq!(body["agent"]["llm"]["stream"], false);
        assert!(
            body.pointer("/initial_message/run")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        );
        Json(json!({"id": id, "execution_status": "running"}))
    }

    async fn get_conversation(
        Path(id): Path<String>,
        state: axum::extract::State<Arc<AtomicBool>>,
        headers: HeaderMap,
    ) -> Result<Json<Value>, StatusCode> {
        assert_eq!(
            headers
                .get("x-session-api-key")
                .and_then(|v| v.to_str().ok()),
            Some("session-secret")
        );
        if !state.load(Ordering::SeqCst) {
            return Err(StatusCode::NOT_FOUND);
        }
        Ok(Json(json!({"id": id, "execution_status": "finished"})))
    }

    async fn events_search(headers: HeaderMap) -> Json<Value> {
        assert_eq!(
            headers
                .get("x-session-api-key")
                .and_then(|v| v.to_str().ok()),
            Some("session-secret")
        );
        Json(json!({
            "items": [{
                "id": "01J00000000000000000000000",
                "kind": "MessageEvent",
                "source": "agent",
                "llm_message": {"content": [{"type": "text", "text": "done"}]}
            }],
            "next_page_id": null
        }))
    }

    async fn pause() -> Json<Value> {
        Json(json!({"success": true}))
    }

    #[test]
    fn server_runtime_metadata_must_prove_docker_execution_and_required_tools() {
        assert!(server_supports_docker_runtime(
            &json!({"execution_runtime": "docker"})
        ));
        assert!(!server_supports_docker_runtime(
            &json!({"execution_runtime": "local"})
        ));
        assert!(!server_supports_docker_runtime(&json!({
            "conversation_runtime": "docker",
            "available_conversation_runtimes": ["docker"]
        })));
        assert!(!server_supports_docker_runtime(&json!({})));

        assert!(server_has_required_tools(&json!({
            "usable_tools": ["terminal", "file_editor", "task_tracker", "browser"]
        })));
        assert!(!server_has_required_tools(&json!({
            "usable_tools": ["terminal", "file_editor"]
        })));
        assert!(!server_has_required_tools(&json!({})));
    }

    #[test]
    fn start_payload_requires_docker_runtime_and_always_confirm() {
        let adapter = OpenHandsAgentServerAdapter::new(
            "http://127.0.0.1:18000",
            "session-secret",
            "local/model",
            "local-gateway-key",
            "http://host.docker.internal:11500/v1",
            "OpenHands test",
        )
        .unwrap();
        let task_id = "00000000-0000-4000-8000-000000000001";
        let payload = adapter
            .start_payload(&StartTaskRequest {
                task_id: task_id.into(),
                capability_id: "openhands.run".into(),
                input: json!({"objective": "create a file"}),
                idempotency_key: format!("project-dread:{task_id}"),
            })
            .unwrap();

        assert!(payload.get("conversation_runtime").is_none());
        assert_eq!(payload["workspace"]["kind"], "DockerExecutionWorkspace");
        assert_eq!(payload["workspace"]["working_dir"], "/workspace");
        assert_eq!(payload["worktree"], false);
        assert_eq!(payload["confirmation_policy"]["kind"], "AlwaysConfirm");
        assert_eq!(
            payload["agent"]["tools"],
            json!([
                {"name": "terminal"},
                {"name": "file_editor"},
                {"name": "task_tracker"}
            ])
        );
        assert_eq!(payload["agent"]["llm"]["stream"], false);
    }

    async fn waiting_conversation(Path(id): Path<String>, headers: HeaderMap) -> Json<Value> {
        assert_eq!(
            headers
                .get("x-session-api-key")
                .and_then(|v| v.to_str().ok()),
            Some("session-secret")
        );
        Json(json!({"id": id, "execution_status": "waiting_for_confirmation"}))
    }

    async fn respond_confirmation(
        Path(id): Path<String>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> Json<Value> {
        assert_eq!(
            headers
                .get("x-session-api-key")
                .and_then(|v| v.to_str().ok()),
            Some("session-secret")
        );
        assert!(!id.is_empty());
        assert_eq!(body["accept"], true);
        assert_eq!(body["reason"], "Reviewed in DREAD");
        Json(json!({"success": true}))
    }

    #[tokio::test]
    async fn confirmation_response_requires_pending_state_and_uses_authenticated_endpoint() {
        let app = Router::new()
            .route("/api/conversations/{id}", get(waiting_conversation))
            .route(
                "/api/conversations/{id}/events/respond_to_confirmation",
                post(respond_confirmation),
            );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let adapter = OpenHandsAgentServerAdapter::new(
            &format!("http://{address}"),
            "session-secret",
            "test/model",
            "llm-secret",
            "http://host.docker.internal:11500/v1",
            "OpenHands test",
        )
        .unwrap()
        .attest_docker_runtime(true, true)
        .unwrap();
        let task_id = "00000000-0000-4000-8000-000000000001";
        adapter
            .respond_to_confirmation(task_id, task_id, true, "Reviewed in DREAD")
            .await
            .unwrap();
        assert!(
            adapter
                .respond_to_confirmation(
                    "00000000-0000-4000-8000-000000000002",
                    task_id,
                    true,
                    "wrong local identity",
                )
                .await
                .is_err()
        );
        server.abort();
    }

    #[tokio::test]
    async fn agent_server_lifecycle_is_authenticated_and_task_scoped() {
        let created = Arc::new(AtomicBool::new(false));
        let app = Router::new()
            .route("/ready", get(ready))
            .route("/server_info", get(docker_server_info))
            .route("/api/conversations", post(create_conversation))
            .route("/api/conversations/{id}", get(get_conversation))
            .route("/api/conversations/{id}/events/search", get(events_search))
            .route("/api/conversations/{id}/pause", post(pause))
            .with_state(created.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let adapter = OpenHandsAgentServerAdapter::new(
            &format!("http://{address}"),
            "session-secret",
            "test/model",
            "llm-secret",
            "http://host.docker.internal:11500/v1",
            "OpenHands test",
        )
        .unwrap();
        assert!(adapter.capabilities().await.unwrap().is_empty());
        let adapter = adapter.attest_docker_runtime(true, true).unwrap();
        assert!(adapter.descriptor().enabled);
        assert_eq!(adapter.capabilities().await.unwrap()[0].id, "openhands.run");

        let task_id = "00000000-0000-4000-8000-000000000001";
        let started = adapter
            .start_task(StartTaskRequest {
                task_id: task_id.into(),
                capability_id: "openhands.run".into(),
                input: json!({"objective": "create a file"}),
                idempotency_key: format!("project-dread:{task_id}"),
            })
            .await
            .unwrap();
        assert_eq!(started.provider_task_id.as_deref(), Some(task_id));

        created.store(true, Ordering::SeqCst);
        let status = adapter.task_status(task_id, task_id).await.unwrap();
        assert_eq!(status.state, AgentTaskState::Succeeded);

        let events = adapter.events(task_id, task_id, 0).await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].sequence, 1);

        let result = adapter.task_result(task_id, task_id).await.unwrap();
        assert_eq!(
            result
                .result
                .as_ref()
                .and_then(|value| value.get("final_message"))
                .and_then(Value::as_str),
            Some("done")
        );

        let cancelled = adapter.cancel_task(task_id, task_id).await.unwrap();
        assert_eq!(cancelled.state, AgentTaskState::Cancelled);
        server.abort();
    }

    #[tokio::test]
    async fn does_not_report_cancellation_when_openhands_pause_returns_false() {
        async fn pause_refused() -> Json<Value> {
            Json(json!({"success": false}))
        }

        let app = Router::new().route("/api/conversations/{id}/pause", post(pause_refused));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let adapter = OpenHandsAgentServerAdapter::new(
            &format!("http://{address}"),
            "session-secret",
            "test/model",
            "llm-secret",
            "http://host.docker.internal:11500/v1",
            "OpenHands test",
        )
        .unwrap()
        .attest_docker_runtime(true, true)
        .unwrap();

        let task_id = "00000000-0000-4000-8000-000000000002";
        let error = adapter.cancel_task(task_id, task_id).await.unwrap_err();

        assert_eq!(error.code, "provider_cancel_not_confirmed");
        server.abort();
    }

    #[test]
    fn rejects_remote_servers_and_non_gateway_llm_endpoints() {
        assert!(
            OpenHandsAgentServerAdapter::new(
                "http://example.com:8000",
                "session",
                "model",
                "key",
                "http://host.docker.internal:11500/v1",
                "OpenHands"
            )
            .is_err()
        );
        assert!(
            OpenHandsAgentServerAdapter::new(
                "http://127.0.0.1:8000",
                "session",
                "model",
                "key",
                "http://127.0.0.1:11500/v1",
                "OpenHands"
            )
            .is_err()
        );
    }

    #[tokio::test]
    async fn refuses_to_create_a_conversation_when_server_does_not_advertise_docker() {
        async fn server_info(headers: HeaderMap) -> Json<Value> {
            assert_eq!(
                headers
                    .get("x-session-api-key")
                    .and_then(|value| value.to_str().ok()),
                Some("session-secret")
            );
            Json(json!({
                "conversation_runtime": "local",
                "available_conversation_runtimes": ["local"]
            }))
        }

        async fn create_conversation_if_called(
            headers: HeaderMap,
            Json(body): Json<Value>,
        ) -> Json<Value> {
            assert_eq!(
                headers
                    .get("x-session-api-key")
                    .and_then(|value| value.to_str().ok()),
                Some("session-secret")
            );
            Json(json!({
                "id": body["conversation_id"],
                "execution_status": "running"
            }))
        }

        let app = Router::new()
            .route("/server_info", get(server_info))
            .route("/api/conversations", post(create_conversation_if_called));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let adapter = OpenHandsAgentServerAdapter::new(
            &format!("http://{address}"),
            "session-secret",
            "test/model",
            "llm-secret",
            "http://host.docker.internal:11500/v1",
            "OpenHands test",
        )
        .unwrap()
        .attest_docker_runtime(true, true)
        .unwrap();

        let task_id = "00000000-0000-4000-8000-000000000001";
        let error = adapter
            .start_task(StartTaskRequest {
                task_id: task_id.into(),
                capability_id: "openhands.run".into(),
                input: json!({"objective": "create a file"}),
                idempotency_key: format!("project-dread:{task_id}"),
            })
            .await
            .unwrap_err();

        assert_eq!(error.code, "docker_runtime_unavailable");
        server.abort();
    }
}
