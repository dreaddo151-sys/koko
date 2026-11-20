//! Read-only local media discovery and ComfyUI monitoring.
//!
//! This crate never submits workflows, interrupts renders, loads models or
//! acquires GPU leases. Workflow execution belongs behind reviewed manifests
//! and the Python GPU Supervisor's single-owner resource boundary.

pub mod workflows;
pub use workflows::{WorkflowDescriptor, WorkflowInventory, discover_local_workflows};

use anyhow::{Context, Result, ensure};
use reqwest::{Client, Url};
use serde::Serialize;
use serde_json::Value;
use std::{net::IpAddr, time::Duration};

const DEFAULT_COMFYUI_URL: &str = "http://127.0.0.1:8188";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(4);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const MAX_HISTORY_ITEMS: usize = 10;
const MAX_OUTPUT_ITEMS_PER_JOB: usize = 64;
const MAX_ERROR_MESSAGE_CHARS: usize = 2_048;

#[derive(Clone)]
pub struct ComfyUiClient {
    base_url: Url,
    http: Client,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct EndpointProbe<T> {
    pub status: String,
    pub data: Option<T>,
    pub error: Option<String>,
}

impl<T> EndpointProbe<T> {
    fn available(data: T) -> Self {
        Self {
            status: "available".into(),
            data: Some(data),
            error: None,
        }
    }

    fn unavailable(error: impl Into<String>) -> Self {
        Self {
            status: "unavailable".into(),
            data: None,
            error: Some(error.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct QueueJob {
    pub queue_position: Option<u64>,
    pub prompt_id: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct QueueSnapshot {
    pub running: Vec<QueueJob>,
    pub pending: Vec<QueueJob>,
    pub running_count: usize,
    pub pending_count: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MediaOutput {
    pub node_id: String,
    pub media_type: String,
    pub filename: String,
    pub subfolder: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MediaError {
    pub event_type: String,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct HistoryJob {
    pub prompt_id: String,
    pub status: String,
    pub completed: bool,
    pub outputs: Vec<MediaOutput>,
    pub errors: Vec<MediaError>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ComfyUiMonitor {
    pub status: String,
    pub endpoint: String,
    pub system: EndpointProbe<Value>,
    pub queue: EndpointProbe<QueueSnapshot>,
    pub history: EndpointProbe<Vec<HistoryJob>>,
    pub progress_note: String,
}

impl ComfyUiClient {
    pub fn new(raw_url: impl AsRef<str>) -> Result<Self> {
        let base_url = Url::parse(raw_url.as_ref()).context("Invalid ComfyUI URL")?;
        ensure!(
            base_url.scheme() == "http",
            "ComfyUI must use loopback HTTP"
        );
        ensure!(base_url.port().is_some(), "ComfyUI URL must specify a port");
        ensure!(
            base_url.username().is_empty()
                && base_url.password().is_none()
                && base_url.path() == "/"
                && base_url.query().is_none()
                && base_url.fragment().is_none(),
            "ComfyUI URL must contain only scheme, loopback IP and port"
        );
        let loopback = base_url
            .host_str()
            .and_then(|host| {
                host.trim_start_matches('[')
                    .trim_end_matches(']')
                    .parse::<IpAddr>()
                    .ok()
            })
            .is_some_and(|ip| ip.is_loopback());
        ensure!(loopback, "ComfyUI endpoint must use a loopback IP address");

        let http = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .context("Could not create ComfyUI HTTP client")?;
        Ok(Self { base_url, http })
    }

    pub fn from_env() -> Result<Self> {
        let url =
            std::env::var("DREAD_COMFYUI_URL").unwrap_or_else(|_| DEFAULT_COMFYUI_URL.to_string());
        Self::new(url)
    }

    pub fn local() -> Result<Self> {
        Self::new(DEFAULT_COMFYUI_URL)
    }

    async fn get_json(&self, path_and_query: &str) -> Result<Value> {
        let url = self
            .base_url
            .join(path_and_query)
            .context("Invalid ComfyUI API path")?;
        let mut response = self
            .http
            .get(url)
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .context("ComfyUI request failed")?;
        let status = response.status();
        ensure!(
            status.is_success(),
            "ComfyUI returned HTTP {}",
            status.as_u16()
        );
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
        {
            anyhow::bail!("ComfyUI response exceeded the 2 MiB limit");
        }
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .context("Could not read ComfyUI response")?
        {
            ensure!(
                body.len().saturating_add(chunk.len()) <= MAX_RESPONSE_BYTES,
                "ComfyUI response exceeded the 2 MiB limit"
            );
            body.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&body).context("ComfyUI returned invalid JSON")
    }

    /// Read system metadata, the live queue and up to ten recent history jobs.
    pub async fn monitor(&self) -> ComfyUiMonitor {
        let (system_result, queue_result, history_result) = tokio::join!(
            self.get_json("system_stats"),
            self.get_json("queue"),
            self.get_json("history?max_items=10"),
        );
        let system = match system_result {
            Ok(value) => EndpointProbe::available(value),
            Err(error) => EndpointProbe::unavailable(error.to_string()),
        };
        let queue = match queue_result {
            Ok(value) => match parse_queue(&value) {
                Some(snapshot) => EndpointProbe::available(snapshot),
                None => {
                    EndpointProbe::unavailable("ComfyUI queue response has an unexpected shape")
                }
            },
            Err(error) => EndpointProbe::unavailable(error.to_string()),
        };
        let history = match history_result {
            Ok(value) => EndpointProbe::available(parse_history(&value)),
            Err(error) => EndpointProbe::unavailable(error.to_string()),
        };
        let available = [
            system.status == "available",
            queue.status == "available",
            history.status == "available",
        ]
        .into_iter()
        .filter(|value| *value)
        .count();
        let status = match available {
            3 => "online",
            0 => "offline",
            _ => "degraded",
        };
        ComfyUiMonitor {
            status: status.into(),
            endpoint: self.base_url.as_str().trim_end_matches('/').to_string(),
            system,
            queue,
            history,
            progress_note: "ComfyUI queue and history do not expose a reliable percentage here; no synthetic progress is reported".into(),
        }
    }
}

fn parse_queue(value: &Value) -> Option<QueueSnapshot> {
    let running = parse_queue_items(value.get("queue_running")?)?;
    let pending = parse_queue_items(value.get("queue_pending")?)?;
    Some(QueueSnapshot {
        running_count: running.len(),
        pending_count: pending.len(),
        running,
        pending,
    })
}

fn parse_queue_items(value: &Value) -> Option<Vec<QueueJob>> {
    let items = value.as_array()?;
    Some(
        items
            .iter()
            .take(256)
            .filter_map(|item| {
                let fields = item.as_array()?;
                let prompt_id = fields.iter().find_map(Value::as_str)?;
                if prompt_id.is_empty() || prompt_id.len() > 256 {
                    return None;
                }
                let queue_position = fields.iter().find_map(Value::as_u64);
                Some(QueueJob {
                    queue_position,
                    prompt_id: prompt_id.to_string(),
                })
            })
            .collect(),
    )
}

fn parse_history(value: &Value) -> Vec<HistoryJob> {
    let Some(object) = value.as_object() else {
        return Vec::new();
    };
    let mut jobs = object
        .iter()
        .take(MAX_HISTORY_ITEMS)
        .filter_map(|(prompt_id, job)| {
            if prompt_id.is_empty() || prompt_id.len() > 256 || !job.is_object() {
                return None;
            }
            let status_obj = job.get("status").unwrap_or(&Value::Null);
            let completed = status_obj
                .get("completed")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let status = status_obj
                .get("status_str")
                .and_then(Value::as_str)
                .or_else(|| status_obj.get("status").and_then(Value::as_str))
                .unwrap_or(if completed { "completed" } else { "unknown" });
            let mut outputs = Vec::new();
            if let Some(nodes) = job.get("outputs").and_then(Value::as_object) {
                'nodes: for (node_id, node) in nodes {
                    for (key, media_type) in [
                        ("images", "image"),
                        ("videos", "video"),
                        ("gifs", "gif"),
                        ("audio", "audio"),
                    ] {
                        let Some(files) = node.get(key).and_then(Value::as_array) else {
                            continue;
                        };
                        for file in files {
                            if outputs.len() >= MAX_OUTPUT_ITEMS_PER_JOB {
                                break 'nodes;
                            }
                            let Some(filename) = file.get("filename").and_then(Value::as_str)
                            else {
                                continue;
                            };
                            if filename.is_empty() || filename.len() > 512 {
                                continue;
                            }
                            let subfolder =
                                file.get("subfolder").and_then(Value::as_str).unwrap_or("");
                            if subfolder.len() > 512 {
                                continue;
                            }
                            outputs.push(MediaOutput {
                                node_id: node_id.chars().take(64).collect(),
                                media_type: media_type.into(),
                                filename: filename.to_string(),
                                subfolder: subfolder.to_string(),
                            });
                        }
                    }
                }
            }
            let mut errors = Vec::new();
            if let Some(messages) = status_obj.get("messages").and_then(Value::as_array) {
                for message in messages.iter().take(64) {
                    let Some(fields) = message.as_array() else {
                        continue;
                    };
                    let event_type = fields.first().and_then(Value::as_str).unwrap_or("unknown");
                    if event_type.len() > 128 {
                        continue;
                    }
                    let message = fields
                        .get(1)
                        .and_then(Value::as_object)
                        .and_then(|data| {
                            data.get("exception_message")
                                .or_else(|| data.get("message"))
                        })
                        .and_then(Value::as_str)
                        .map(|text| {
                            text.chars()
                                .take(MAX_ERROR_MESSAGE_CHARS)
                                .collect::<String>()
                        });
                    errors.push(MediaError {
                        event_type: event_type.into(),
                        message,
                    });
                }
            }
            Some(HistoryJob {
                prompt_id: prompt_id.to_string(),
                status: status.chars().take(64).collect(),
                completed,
                outputs,
                errors,
            })
        })
        .collect::<Vec<_>>();
    jobs.sort_by(|left, right| left.prompt_id.cmp(&right.prompt_id));
    jobs
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn comfyui_url_must_be_loopback_and_have_no_extra_path() {
        assert!(ComfyUiClient::new("http://127.0.0.1:8188").is_ok());
        assert!(ComfyUiClient::new("http://[::1]:8188").is_ok());
        assert!(ComfyUiClient::new("http://192.168.1.10:8188").is_err());
        assert!(ComfyUiClient::new("https://127.0.0.1:8188").is_err());
        assert!(ComfyUiClient::new("http://127.0.0.1:8188/evil").is_err());
        assert!(ComfyUiClient::new("http://user@127.0.0.1:8188").is_err());
    }

    #[test]
    fn parses_queue_ids_and_history_metadata_without_payloads() {
        let queue = parse_queue(&json!({
            "queue_running": [[1, "prompt-running", {}, {}]],
            "queue_pending": [[2, "prompt-pending", {}, {}]]
        }))
        .unwrap();
        assert_eq!(queue.running_count, 1);
        assert_eq!(queue.running[0].prompt_id, "prompt-running");

        let history = parse_history(&json!({
            "prompt-1": {
                "status": {"completed": true, "status_str": "success", "messages": []},
                "outputs": {"9": {"videos": [{"filename":"render.mp4","subfolder":"video"}]}}
            }
        }));
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].status, "success");
        assert_eq!(history[0].outputs[0].filename, "render.mp4");
    }
}
