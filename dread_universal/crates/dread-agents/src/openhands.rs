//! OpenHands Agent Server adapter.
//!
//! The Agent Server is treated as an execution boundary, not as a trusted
//! local process. The adapter is disabled until an operator attests that the
//! server runs in a container with a distinct workspace per DREAD task and
//! that OH_SECRET_KEY is configured for encrypted conversation secrets.

use crate::{
    AdapterFuture, AgentAdapter, AgentAdapterError, AgentCapability, AgentDescriptor, AgentEvent,
    AgentHealth, AgentHealthState, AgentTask, AgentTaskState, StartTaskRequest,
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
    llm_base_url: Option<String>,
    workspace_root: String,
    sandbox_attested: bool,
    secret_store_attested: bool,
}

impl OpenHandsAgentServerAdapter {
    /// Construct a disabled, loopback-only adapter.
    ///
    /// The server's LocalWorkspace runs inside the Agent Server process. DREAD
    /// therefore requires the Agent Server itself to be isolated in a container
    /// and creates one deterministic working directory per task.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        base_url: &str,
        session_api_key: impl Into<String>,
        llm_model: impl Into<String>,
        llm_api_key: impl Into<String>,
        llm_base_url: Option<String>,
        workspace_root: impl Into<String>,
        display_name: impl Into<String>,
    ) -> Result<Self, AgentAdapterError> {
        let mut url = Url::parse(base_url).map_err(|error| {
            adapter_error("invalid_endpoint", format!("Invalid OpenHands URL: {error}"), false)
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
            .and_then(|host| host.trim_start_matches('[').trim_end_matches(']').parse::<IpAddr>().ok())
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
        let workspace_root = workspace_root.into();
        if session_api_key.trim().is_empty() || session_api_key.len() > 4096 {
            return Err(adapter_error("missing_session_key", "A non-empty OpenHands session API key is required".into(), false));
        }
        if llm_model.trim().is_empty() || llm_model.len() > 256 {
            return Err(adapter_error("invalid_model", "OpenHands LLM model must contain 1-256 characters".into(), false));
        }
        if llm_api_key.trim().is_empty() || llm_api_key.len() > 4096 {
            return Err(adapter_error("missing_llm_key", "A non-empty OpenHands LLM API key is required".into(), false));
        }
        validate_workspace_root(&workspace_root)?;
        if let Some(base) = llm_base_url.as_deref() {
            validate_llm_base_url(base)?;
        }

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
            workspace_root,
            sandbox_attested: false,
            secret_store_attested: false,
        })
    }

    /// Enable execution only after explicit container/workspace and secret
    /// storage attestations. LocalWorkspace is safe here only because it is
    /// scoped to the isolated Agent Server container.
    pub fn attest_container_sandbox(
        mut self,
        container_id: &str,
        isolated_workspace_per_task: bool,
        secret_store_configured: bool,
    ) -> Result<Self, AgentAdapterError> {
        if container_id.trim().is_empty()
            || container_id.len() > 128
            || !container_id.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        {
            return Err(adapter_error("invalid_container_attestation", "A valid container identifier is required".into(), false));
        }
        if !isolated_workspace_per_task {
            return Err(adapter_error("workspace_isolation_required", "OpenHands must use a distinct container workspace directory for every DREAD task".into(), false));
        }
        if !secret_store_configured {
            return Err(adapter_error("secret_store_required", "OpenHands OH_SECRET_KEY must be configured and stable across restarts".into(), false));
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
                "OpenHands execution is disabled until its container, per-task workspace isolation, and OH_SECRET_KEY storage are attested".into(),
                false,
            ))
        }
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
        let bytes = response.bytes().await.map_err(|error| {
            adapter_error("response_error", error.to_string(), true)
        })?;
        if bytes.len() > MAX_RESPONSE_BYTES {
            return Err(adapter_error("response_too_large", "OpenHands response exceeded the 2 MiB limit".into(), false));
        }
        if status == StatusCode::NOT_FOUND {
            return Err(adapter_error("not_found", "OpenHands resource was not found".into(), false));
        }
        if status == StatusCode::CONFLICT {
            return Err(adapter_error("conflict", "OpenHands conversation already exists or is running".into(), false));
        }
        if !status.is_success() {
            return Err(adapter_error(
                "http_error",
                format!("OpenHands returned HTTP {}: {}", status.as_u16(), String::from_utf8_lossy(&bytes)),
                status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS,
            ));
        }
        serde_json::from_slice(&bytes).map_err(|error| {
            adapter_error("invalid_response", format!("OpenHands returned invalid JSON: {error}"), false)
        })
    }

    async fn conversation(&self, conversation_id: &str) -> Result<Option<Value>, AgentAdapterError> {
        validate_task_id(conversation_id)?;
        match self.json_request(
            Method::GET,
            &format!("/api/conversations/{conversation_id}"),
            None,
            &[],
        ).await {
            Ok(value) => {
                if value.get("id").and_then(Value::as_str) != Some(conversation_id) {
                    return Err(adapter_error(
                        "provider_id_mismatch",
                        "OpenHands returned a conversation ID different from the DREAD task ID".into(),
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
            "running" | "waiting_for_confirmation" => AgentTaskState::Running,
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
            error: conversation.get("error").and_then(Value::as_str).map(str::to_string),
            updated_at: now_epoch(),
        }
    }

    fn start_payload(&self, request: &StartTaskRequest) -> Result<Value, AgentAdapterError> {
        if request.capability_id != "openhands.run" {
            return Err(adapter_error("unsupported_capability", "OpenHands adapter supports only openhands.run".into(), false));
        }
        if request.idempotency_key != format!("project-dread:{}", request.task_id) {
            return Err(adapter_error("invalid_idempotency_key", "OpenHands idempotency key must be derived from the DREAD task ID".into(), false));
        }
        let object = request.input.as_object().ok_or_else(|| {
            adapter_error("invalid_input", "OpenHands input must be a JSON object".into(), false)
        })?;
        if object.keys().any(|key| !["objective", "instructions", "max_iterations"].contains(&key.as_str())) {
            return Err(adapter_error("invalid_input", "OpenHands input contains unsupported fields".into(), false));
        }
        let objective = request.input.get("objective").and_then(Value::as_str).ok_or_else(|| {
            adapter_error("invalid_input", "OpenHands input requires a text objective".into(), false)
        })?;
        if objective.trim().is_empty() || objective.len() > MAX_OBJECTIVE_BYTES {
            return Err(adapter_error("invalid_input", "OpenHands objective must contain 1-20000 bytes".into(), false));
        }
        let instructions = request.input.get("instructions").and_then(Value::as_str).unwrap_or("");
        if instructions.len() > MAX_INSTRUCTIONS_BYTES {
            return Err(adapter_error("invalid_input", "OpenHands instructions exceed 20000 bytes".into(), false));
        }
        let max_iterations = request.input.get("max_iterations").and_then(Value::as_u64).unwrap_or(100);
        if !(1..=200).contains(&max_iterations) {
            return Err(adapter_error("invalid_input", "OpenHands max_iterations must be between 1 and 200".into(), false));
        }
        let message = if instructions.trim().is_empty() {
            objective.to_string()
        } else {
            format!("{instructions}\n\nObjective:\n{objective}")
        };
        let mut llm = json!({
            "model": self.llm_model,
            "api_key": self.llm_api_key,
            "temperature": 0
        });
        if let Some(base_url) = self.llm_base_url.as_deref() {
            llm["base_url"] = Value::String(base_url.to_string());
        }
        let working_dir = format!("{}/{}", self.workspace_root.trim_end_matches('/'), request.task_id);
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
                "kind": "LocalWorkspace",
                "working_dir": working_dir
            },
            "confirmation_policy": {"kind": "NeverConfirm"},
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
            let response = self.client
                .get(self.endpoint("/ready"))
                .header("X-Session-API-Key", &self.session_api_key)
                .timeout(Duration::from_secs(10))
                .send()
                .await
                .map_err(|error| adapter_error("transport_error", error.to_string(), error.is_timeout() || error.is_connect()))?;
            let status = response.status();
            Ok(AgentHealth {
                state: if status.is_success() { AgentHealthState::Online }
                    else if status.is_server_error() || status == StatusCode::SERVICE_UNAVAILABLE { AgentHealthState::Degraded }
                    else { AgentHealthState::Unknown },
                checked_at: now_epoch(),
                detail: Some(format!("OpenHands /ready returned HTTP {}", status.as_u16())),
            })
        })
    }

    fn capabilities<'a>(&'a self) -> AdapterFuture<'a, Vec<AgentCapability>> {
        Box::pin(async move {
            if !self.sandbox_attested || !self.secret_store_attested || !self.descriptor.enabled {
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
            match self.json_request(Method::POST, "/api/conversations", Some(body), &[]).await {
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

    fn task_status<'a>(&'a self, task_id: &'a str, provider_task_id: &'a str) -> AdapterFuture<'a, AgentTask> {
        Box::pin(async move {
            self.require_attested()?;
            validate_task_id(task_id)?;
            if provider_task_id != task_id {
                return Err(adapter_error("provider_id_mismatch", "OpenHands conversation ID must match the DREAD task ID".into(), false));
            }
            let conversation = self.conversation(provider_task_id).await?
                .ok_or_else(|| adapter_error("not_found", "OpenHands conversation does not exist".into(), false))?;
            Ok(self.to_task(task_id, &conversation))
        })
    }

    fn events<'a>(&'a self, task_id: &'a str, provider_task_id: &'a str, after_sequence: u64) -> AdapterFuture<'a, Vec<AgentEvent>> {
        Box::pin(async move {
            self.require_attested()?;
            validate_task_id(task_id)?;
            if provider_task_id != task_id {
                return Err(adapter_error("provider_id_mismatch", "OpenHands conversation ID must match the DREAD task ID".into(), false));
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
                let page = self.json_request(
                    Method::GET,
                    &format!("/api/conversations/{provider_task_id}/events/search"),
                    None,
                    &query,
                ).await?;
                let items = page.get("items").and_then(Value::as_array).ok_or_else(|| {
                    adapter_error("invalid_response", "OpenHands event search response has no items array".into(), false)
                })?;
                for item in items {
                    sequence = sequence.saturating_add(1);
                    if sequence > after_sequence {
                        output.push(AgentEvent {
                            task_id: task_id.to_string(),
                            sequence,
                            event_type: item.get("kind").and_then(Value::as_str).unwrap_or("unknown").to_string(),
                            payload: item.clone(),
                            occurred_at: now_epoch(),
                        });
                    }
                }
                let next = page.get("next_page_id").and_then(Value::as_str);
                match next {
                    Some(next) if !next.is_empty() => {
                        if next.len() > 512 || !seen_pages.insert(next.to_string()) {
                            return Err(adapter_error("invalid_pagination", "OpenHands returned an invalid or repeated event page cursor".into(), false));
                        }
                        page_id = Some(next.to_string());
                    }
                    _ => return Ok(output),
                }
            }
            Err(adapter_error("event_history_limit", "OpenHands event history exceeded the 100-page safety limit".into(), false))
        })
    }

    fn cancel_task<'a>(&'a self, task_id: &'a str, provider_task_id: &'a str) -> AdapterFuture<'a, AgentTask> {
        Box::pin(async move {
            self.require_attested()?;
            validate_task_id(task_id)?;
            if provider_task_id != task_id {
                return Err(adapter_error("provider_id_mismatch", "OpenHands conversation ID must match the DREAD task ID".into(), false));
            }
            let _ = self.json_request(
                Method::POST,
                &format!("/api/conversations/{provider_task_id}/pause"),
                None,
                &[],
            ).await?;
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

    fn task_result<'a>(&'a self, task_id: &'a str, provider_task_id: &'a str) -> AdapterFuture<'a, AgentTask> {
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
                    Some(Value::Array(items)) => items.iter().filter_map(|item| item.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join("\n"),
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
}

fn validate_task_id(task_id: &str) -> Result<(), AgentAdapterError> {
    let bytes = task_id.as_bytes();
    if bytes.len() != 36
        || ![8, 13, 18, 23].iter().all(|index| bytes[*index] == b'-')
        || !bytes.iter().enumerate().all(|(index, byte)| {
            [8, 13, 18, 23].contains(&index) || byte.is_ascii_hexdigit()
        })
    {
        return Err(adapter_error("invalid_task_id", "OpenHands requires a canonical UUID DREAD task ID".into(), false));
    }
    Ok(())
}

fn validate_workspace_root(root: &str) -> Result<(), AgentAdapterError> {
    if root.trim().is_empty()
        || root.len() > 512
        || root.starts_with('/')
        || root.contains('\\')
        || !root.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"._-/".contains(&byte))
        || root.split('/').any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(adapter_error("unsafe_workspace_root", "OpenHands workspace root must be a safe relative path without traversal components".into(), false));
    }
    Ok(())
}

fn validate_llm_base_url(value: &str) -> Result<(), AgentAdapterError> {
    let url = Url::parse(value).map_err(|error| adapter_error("invalid_llm_base_url", error.to_string(), false))?;
    if !matches!(url.scheme(), "https" | "http")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || (url.scheme() == "http" && !url.host_str().and_then(|host| host.parse::<IpAddr>().ok()).is_some_and(|ip| ip.is_loopback()))
    {
        return Err(adapter_error("unsafe_llm_base_url", "OpenHands LLM base URL must use HTTPS or loopback HTTP without embedded credentials, query or fragment".into(), false));
    }
    Ok(())
}

fn adapter_error(code: &str, message: String, retryable: bool) -> AgentAdapterError {
    AgentAdapterError { code: code.into(), message, retryable }
}

fn now_epoch() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
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
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::net::TcpListener;

    async fn ready() -> &'static str {
        "ready"
    }

    async fn create_conversation(
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> Json<Value> {
        assert_eq!(headers.get("x-session-api-key").and_then(|v| v.to_str().ok()), Some("session-secret"));
        let id = body.get("conversation_id").and_then(Value::as_str).unwrap().to_string();
        assert_eq!(body.pointer("/workspace/kind").and_then(Value::as_str), Some("LocalWorkspace"));
        assert!(body.pointer("/initial_message/run").and_then(Value::as_bool).unwrap_or(false));
        Json(json!({"id": id, "execution_status": "running"}))
    }

    async fn get_conversation(
        Path(id): Path<String>,
        state: axum::extract::State<Arc<AtomicBool>>,
        headers: HeaderMap,
    ) -> Result<Json<Value>, StatusCode> {
        assert_eq!(headers.get("x-session-api-key").and_then(|v| v.to_str().ok()), Some("session-secret"));
        if !state.load(Ordering::SeqCst) {
            return Err(StatusCode::NOT_FOUND);
        }
        Ok(Json(json!({"id": id, "execution_status": "finished"})))
    }

    async fn events_search(
        headers: HeaderMap,
    ) -> Json<Value> {
        assert_eq!(headers.get("x-session-api-key").and_then(|v| v.to_str().ok()), Some("session-secret"));
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

    #[tokio::test]
    async fn agent_server_lifecycle_is_authenticated_and_task_scoped() {
        let created = Arc::new(AtomicBool::new(false));
        let app = Router::new()
            .route("/ready", get(ready))
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
            None,
            "workspace/dread-tasks",
            "OpenHands test",
        )
        .unwrap();
        assert!(adapter.capabilities().await.unwrap().is_empty());
        let adapter = adapter.attest_container_sandbox("dread-agent-server-test", true, true).unwrap();
        assert!(adapter.descriptor().enabled);
        assert_eq!(adapter.capabilities().await.unwrap()[0].id, "openhands.run");

        let task_id = "00000000-0000-4000-8000-000000000001";
        let started = adapter.start_task(StartTaskRequest {
            task_id: task_id.into(),
            capability_id: "openhands.run".into(),
            input: json!({"objective": "create a file"}),
            idempotency_key: format!("project-dread:{task_id}"),
        }).await.unwrap();
        assert_eq!(started.provider_task_id.as_deref(), Some(task_id));

        created.store(true, Ordering::SeqCst);
        let status = adapter.task_status(task_id, task_id).await.unwrap();
        assert_eq!(status.state, AgentTaskState::Succeeded);

        let events = adapter.events(task_id, task_id, 0).await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].sequence, 1);

        let result = adapter.task_result(task_id, task_id).await.unwrap();
        assert_eq!(result.result.as_ref().and_then(|value| value.get("final_message")).and_then(Value::as_str), Some("done"));

        let cancelled = adapter.cancel_task(task_id, task_id).await.unwrap();
        assert_eq!(cancelled.state, AgentTaskState::Cancelled);
        server.abort();
    }

    #[test]
    fn rejects_unsafe_workspace_and_non_loopback_servers() {
        assert!(OpenHandsAgentServerAdapter::new(
            "http://example.com:8000", "session", "model", "key", None,
            "workspace/tasks", "OpenHands"
        ).is_err());
        assert!(OpenHandsAgentServerAdapter::new(
            "http://127.0.0.1:8000", "session", "model", "key", None,
            "../outside", "OpenHands"
        ).is_err());
    }
}
