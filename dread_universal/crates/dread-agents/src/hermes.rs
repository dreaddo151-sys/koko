//! Hermes Agent API-server adapter.
//!
//! This adapter talks only to the documented HTTP Runs API. The API server has
//! access to Hermes' configured toolset, so callers must keep it on loopback,
//! require a bearer key, and apply DREAD approval before invoking start_task.

use crate::{
    AdapterFuture, AgentAdapter, AgentAdapterError, AgentCapability, AgentDescriptor, AgentEvent,
    AgentHealth, AgentHealthState, AgentTask, AgentTaskState, StartTaskRequest,
};
use reqwest::{Client, StatusCode, Url};
use serde_json::{Value, json};
use std::{
    net::IpAddr,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const MAX_INPUT_BYTES: usize = 1_048_576;
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const MAX_EVENT_STREAM_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone)]
pub struct HermesApiAdapter {
    descriptor: AgentDescriptor,
    client: Client,
    base_url: Url,
    api_key: String,
}

impl HermesApiAdapter {
    /// Construct a loopback-only adapter. Hermes' API server must have
    /// API_SERVER_KEY configured; unauthenticated mode is never accepted.
    pub fn new(
        base_url: &str,
        api_key: impl Into<String>,
        display_name: impl Into<String>,
    ) -> Result<Self, AgentAdapterError> {
        let mut url = Url::parse(base_url).map_err(|error| {
            adapter_error(
                "invalid_endpoint",
                format!("Invalid Hermes URL: {error}"),
                false,
            )
        })?;
        if url.scheme() != "http"
            || url.port().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(adapter_error(
                "unsafe_endpoint",
                "Hermes adapter requires an explicit-port loopback HTTP URL without credentials, query or fragment".into(),
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
                "Hermes API server must be bound to a loopback address".into(),
                false,
            ));
        }
        let api_key = api_key.into();
        if api_key.trim().is_empty() || api_key.len() > 4096 {
            return Err(adapter_error(
                "missing_api_key",
                "A non-empty Hermes API_SERVER_KEY is required".into(),
                false,
            ));
        }
        let normalized_path = url.path().trim_end_matches('/').to_string();
        url.set_path(&normalized_path);
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| adapter_error("client_error", error.to_string(), false))?;
        let endpoint = url.as_str().trim_end_matches('/').to_string();
        Ok(Self {
            descriptor: AgentDescriptor {
                id: "hermes.api_server".into(),
                display_name: display_name.into(),
                adapter: "hermes.api_server".into(),
                version: None,
                endpoint: Some(endpoint),
                enabled: true,
            },
            client,
            base_url: url,
            api_key,
        })
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{}{}", self.base_url.as_str().trim_end_matches('/'), path)
    }

    async fn json_request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
        idempotency_key: Option<&str>,
    ) -> Result<Value, AgentAdapterError> {
        let mut request = self
            .client
            .request(method, self.endpoint(path))
            .bearer_auth(&self.api_key)
            .timeout(Duration::from_secs(30));
        if let Some(body) = body {
            request = request.json(&body);
        }
        if let Some(key) = idempotency_key {
            if key.is_empty()
                || key.len() > 255
                || !key.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
            {
                return Err(adapter_error(
                    "invalid_idempotency_key",
                    "Hermes Idempotency-Key must be 1-255 visible ASCII characters".into(),
                    false,
                ));
            }
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

    async fn read_run(&self, run_id: &str) -> Result<Value, AgentAdapterError> {
        validate_run_id(run_id)?;
        self.json_request(
            reqwest::Method::GET,
            &format!("/v1/runs/{run_id}"),
            None,
            None,
        )
        .await
    }

    fn to_task(&self, local_task_id: &str, run: &Value) -> AgentTask {
        let run_id = run
            .get("run_id")
            .and_then(Value::as_str)
            .map(str::to_string);
        let status = run
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let state = run_state(status);
        let result = (state == AgentTaskState::Succeeded).then(|| {
            json!({
                "output": run.get("output").cloned().unwrap_or(Value::Null),
                "usage": run.get("usage").cloned().unwrap_or(Value::Null),
                "runtime": run.get("runtime").cloned().unwrap_or(Value::Null)
            })
        });
        AgentTask {
            task_id: local_task_id.to_string(),
            state,
            provider_task_id: run_id,
            result,
            error: run.get("error").and_then(Value::as_str).map(str::to_string),
            updated_at: now_epoch(),
        }
    }
}

impl AgentAdapter for HermesApiAdapter {
    fn descriptor(&self) -> &AgentDescriptor {
        &self.descriptor
    }

    fn health<'a>(&'a self) -> AdapterFuture<'a, AgentHealth> {
        Box::pin(async move {
            let response = self
                .client
                .get(self.endpoint("/health"))
                .bearer_auth(&self.api_key)
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
                } else if status.is_server_error() {
                    AgentHealthState::Degraded
                } else {
                    AgentHealthState::Unknown
                },
                checked_at: now_epoch(),
                detail: Some(format!("Hermes /health returned HTTP {}", status.as_u16())),
            })
        })
    }

    fn capabilities<'a>(&'a self) -> AdapterFuture<'a, Vec<AgentCapability>> {
        Box::pin(async move {
            let value = self
                .json_request(reqwest::Method::GET, "/v1/capabilities", None, None)
                .await?;
            let features = value.get("features").and_then(Value::as_object);
            let supports_runs = ["run_submission", "run_status", "run_events_sse", "run_stop"]
                .iter()
                .all(|feature| {
                    features
                        .and_then(|items| items.get(*feature))
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                });
            if !supports_runs {
                return Ok(Vec::new());
            }
            Ok(vec![AgentCapability {
                id: "hermes.run".into(),
                name: "Hermes agent run".into(),
                description: "Submit a bounded text task to the authenticated Hermes Runs API; Hermes tools execute under the configured Hermes policy.".into(),
                input_schema: json!({
                    "type":"object",
                    "properties":{
                        "input":{"type":"string","maxLength":1048576},
                        "session_id":{"type":"string","maxLength":160},
                        "instructions":{"type":"string","maxLength":65536}
                    },
                    "required":["input"],
                    "additionalProperties":false
                }),
                output_schema: Some(json!({"type":"object","properties":{"output":{"type":"string"}}})),
            }])
        })
    }

    fn start_task<'a>(&'a self, request: StartTaskRequest) -> AdapterFuture<'a, AgentTask> {
        Box::pin(async move {
            if request.task_id.trim().is_empty() || request.task_id.len() > 160 {
                return Err(adapter_error(
                    "invalid_task_id",
                    "DREAD task id must contain 1-160 characters".into(),
                    false,
                ));
            }
            if request.capability_id != "hermes.run" {
                return Err(adapter_error(
                    "unsupported_capability",
                    "Hermes adapter supports only hermes.run".into(),
                    false,
                ));
            }
            let input = request
                .input
                .get("input")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    adapter_error(
                        "invalid_input",
                        "Hermes run input must contain a text 'input' field".into(),
                        false,
                    )
                })?;
            if input.trim().is_empty() || input.len() > MAX_INPUT_BYTES {
                return Err(adapter_error(
                    "invalid_input",
                    "Hermes run input must contain 1-1048576 bytes".into(),
                    false,
                ));
            }
            let mut body = json!({"input": input});
            for field in [
                "session_id",
                "instructions",
                "conversation_history",
                "previous_response_id",
            ] {
                if let Some(value) = request.input.get(field) {
                    if field == "session_id"
                        && (!value.is_string() || value.as_str().unwrap_or_default().len() > 160)
                    {
                        return Err(adapter_error(
                            "invalid_input",
                            "Hermes session_id must be a string of at most 160 characters".into(),
                            false,
                        ));
                    }
                    if field == "instructions"
                        && (!value.is_string() || value.as_str().unwrap_or_default().len() > 65536)
                    {
                        return Err(adapter_error(
                            "invalid_input",
                            "Hermes instructions must be text of at most 65536 bytes".into(),
                            false,
                        ));
                    }
                    if field == "previous_response_id"
                        && (!value.is_string() || value.as_str().unwrap_or_default().len() > 160)
                    {
                        return Err(adapter_error(
                            "invalid_input",
                            "Hermes previous_response_id must be text of at most 160 bytes".into(),
                            false,
                        ));
                    }
                    if field == "conversation_history" {
                        let history_size = serde_json::to_vec(value)
                            .map(|encoded| encoded.len())
                            .unwrap_or(usize::MAX);
                        if !value.is_array() || history_size > 512 * 1024 {
                            return Err(adapter_error(
                                "invalid_input",
                                "Hermes conversation_history must be an array no larger than 512 KiB".into(),
                                false,
                            ));
                        }
                    }
                    body[field] = value.clone();
                }
            }
            let body_size = serde_json::to_vec(&body)
                .map(|encoded| encoded.len())
                .unwrap_or(usize::MAX);
            if body_size > MAX_INPUT_BYTES + 65536 {
                return Err(adapter_error(
                    "invalid_input",
                    "Hermes run request body exceeded the 1.0625 MiB adapter limit".into(),
                    false,
                ));
            }
            let value = self
                .json_request(
                    reqwest::Method::POST,
                    "/v1/runs",
                    Some(body),
                    Some(&request.idempotency_key),
                )
                .await?;
            let run_id = value.get("run_id").and_then(Value::as_str).ok_or_else(|| {
                adapter_error(
                    "invalid_provider_response",
                    "Hermes did not return a run_id".into(),
                    false,
                )
            })?;
            validate_run_id(run_id)?;
            Ok(AgentTask {
                task_id: request.task_id,
                state: run_state(
                    value
                        .get("status")
                        .and_then(Value::as_str)
                        .unwrap_or("started"),
                ),
                provider_task_id: Some(run_id.to_string()),
                result: None,
                error: None,
                updated_at: now_epoch(),
            })
        })
    }

    fn task_status<'a>(
        &'a self,
        task_id: &'a str,
        provider_task_id: &'a str,
    ) -> AdapterFuture<'a, AgentTask> {
        Box::pin(async move {
            let run = self.read_run(provider_task_id).await?;
            Ok(self.to_task(task_id, &run))
        })
    }

    fn events<'a>(
        &'a self,
        task_id: &'a str,
        provider_task_id: &'a str,
        after_sequence: u64,
    ) -> AdapterFuture<'a, Vec<AgentEvent>> {
        Box::pin(async move {
            validate_run_id(provider_task_id)?;
            let response = self
                .client
                .get(self.endpoint(&format!("/v1/runs/{provider_task_id}/events")))
                .bearer_auth(&self.api_key)
                .header("Accept", "text/event-stream")
                .timeout(Duration::from_secs(30))
                .send()
                .await
                .map_err(|error| {
                    adapter_error(
                        "transport_error",
                        error.to_string(),
                        error.is_timeout() || error.is_connect(),
                    )
                })?;
            if !response.status().is_success() {
                return Err(http_error(
                    response.status(),
                    "Hermes run events request failed",
                ));
            }
            if response
                .content_length()
                .is_some_and(|length| length > MAX_EVENT_STREAM_BYTES as u64)
            {
                return Err(adapter_error(
                    "event_stream_too_large",
                    "Hermes event stream exceeded the 8 MiB adapter limit".into(),
                    false,
                ));
            }
            let mut response = response;
            let mut body = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|error| adapter_error("stream_error", error.to_string(), true))?
            {
                if body.len().saturating_add(chunk.len()) > MAX_EVENT_STREAM_BYTES {
                    return Err(adapter_error(
                        "event_stream_too_large",
                        "Hermes event stream exceeded the 8 MiB adapter limit".into(),
                        false,
                    ));
                }
                body.extend_from_slice(&chunk);
            }
            let body = String::from_utf8_lossy(&body);
            Ok(parse_sse_events(task_id, &body, after_sequence))
        })
    }

    fn cancel_task<'a>(
        &'a self,
        task_id: &'a str,
        provider_task_id: &'a str,
    ) -> AdapterFuture<'a, AgentTask> {
        Box::pin(async move {
            validate_run_id(provider_task_id)?;
            let _ack = self
                .json_request(
                    reqwest::Method::POST,
                    &format!("/v1/runs/{provider_task_id}/stop"),
                    Some(json!({})),
                    None,
                )
                .await?;
            let run = self.read_run(provider_task_id).await?;
            Ok(self.to_task(task_id, &run))
        })
    }

    fn task_result<'a>(
        &'a self,
        task_id: &'a str,
        provider_task_id: &'a str,
    ) -> AdapterFuture<'a, AgentTask> {
        Box::pin(async move {
            let run = self.read_run(provider_task_id).await?;
            Ok(self.to_task(task_id, &run))
        })
    }
}

async fn parse_json_response(response: reqwest::Response) -> Result<Value, AgentAdapterError> {
    let status = response.status();
    if !status.is_success() {
        return Err(http_error(status, "Hermes API request failed"));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(adapter_error(
            "response_too_large",
            "Hermes JSON response exceeded the 2 MiB adapter limit".into(),
            false,
        ));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| adapter_error("transport_error", error.to_string(), error.is_timeout()))?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(adapter_error(
            "response_too_large",
            "Hermes JSON response exceeded the 2 MiB adapter limit".into(),
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

fn validate_run_id(run_id: &str) -> Result<(), AgentAdapterError> {
    if run_id.is_empty()
        || run_id.len() > 160
        || !run_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-.".contains(&byte))
    {
        return Err(adapter_error(
            "invalid_run_id",
            "Hermes run id contains invalid characters".into(),
            false,
        ));
    }
    Ok(())
}

fn run_state(status: &str) -> AgentTaskState {
    match status {
        "queued" | "started" => AgentTaskState::Queued,
        "running" | "stopping" => AgentTaskState::Running,
        "completed" => AgentTaskState::Succeeded,
        "cancelled" => AgentTaskState::Cancelled,
        "failed" | "interrupted" => AgentTaskState::Failed,
        _ => AgentTaskState::Unknown,
    }
}

fn parse_sse_events(task_id: &str, body: &str, after_sequence: u64) -> Vec<AgentEvent> {
    let normalized_body = body.replace("\r\n", "\n");
    let mut events = Vec::new();
    let mut fallback_sequence = 0_u64;
    for block in normalized_body.split("\n\n") {
        let mut event_type = "message".to_string();
        let mut data = String::new();
        let mut event_id = None;
        for line in block.lines() {
            if let Some(value) = line.strip_prefix("event:") {
                event_type = value.trim().to_string();
            } else if let Some(value) = line.strip_prefix("id:") {
                event_id = Some(value.trim().to_string());
            } else if let Some(value) = line.strip_prefix("data:") {
                if !data.is_empty() {
                    data.push('\n');
                }
                data.push_str(value.trim_start());
            }
        }
        if data.is_empty() {
            continue;
        }
        fallback_sequence += 1;
        let payload =
            serde_json::from_str::<Value>(&data).unwrap_or_else(|_| json!({"data": data}));
        let sequence = payload
            .get("sequence")
            .or_else(|| payload.get("sequence_number"))
            .and_then(Value::as_u64)
            .or_else(|| event_id.as_deref().and_then(|value| value.parse().ok()))
            .unwrap_or(fallback_sequence);
        fallback_sequence = fallback_sequence.max(sequence);
        if sequence <= after_sequence {
            continue;
        }
        events.push(AgentEvent {
            task_id: task_id.to_string(),
            sequence,
            event_type,
            payload,
            occurred_at: now_epoch(),
        });
    }
    events
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
    fn rejects_non_loopback_and_credential_bearing_endpoints() {
        assert!(HermesApiAdapter::new("http://example.com:8642", "key", "Hermes").is_err());
        assert!(HermesApiAdapter::new("http://localhost:8642", "key", "Hermes").is_err());
        assert!(HermesApiAdapter::new("http://[::1]:8642", "key", "Hermes").is_ok());
        assert!(HermesApiAdapter::new("http://user:pass@127.0.0.1:8642", "key", "Hermes").is_err());
        assert!(HermesApiAdapter::new("http://127.0.0.1:8642", "", "Hermes").is_err());
        assert!(HermesApiAdapter::new("http://127.0.0.1:8642", "key", "Hermes").is_ok());
    }

    #[test]
    fn maps_documented_hermes_terminal_statuses_without_success_bias() {
        assert_eq!(run_state("completed"), AgentTaskState::Succeeded);
        assert_eq!(run_state("cancelled"), AgentTaskState::Cancelled);
        assert_eq!(run_state("failed"), AgentTaskState::Failed);
        assert_eq!(run_state("interrupted"), AgentTaskState::Failed);
        assert_eq!(run_state("stopping"), AgentTaskState::Running);
        assert_eq!(run_state("unknown"), AgentTaskState::Unknown);
    }

    #[test]
    fn parses_crlf_streams_and_advances_fallback_sequence_after_filtering() {
        let body = "event: tool.started\r\ndata: {\"tool\":\"terminal\"}\r\n\r\nevent: run.completed\r\ndata: {\"output\":\"done\"}\r\n\r\n";
        let parsed = parse_sse_events("run_abc123", body, 1);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].sequence, 2);
        assert_eq!(parsed[0].event_type, "run.completed");
    }

    #[test]
    fn parses_sse_lifecycle_and_filters_replayed_sequences() {
        let body = "event: tool.started\ndata: {\"sequence\":1,\"tool\":\"terminal\"}\n\nevent: run.completed\ndata: {\"sequence\":2,\"output\":\"done\"}\n\n";
        let parsed = parse_sse_events("run_abc123", body, 1);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].sequence, 2);
        assert_eq!(parsed[0].event_type, "run.completed");
    }
}
