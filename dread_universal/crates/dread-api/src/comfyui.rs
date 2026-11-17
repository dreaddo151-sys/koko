use axum::{Json, http::StatusCode};
use serde_json::{Value, json};
use std::time::Duration;

const MAX_QUEUE_BYTES: usize = 2 * 1024 * 1024;

fn comfyui_base_url() -> Result<String, String> {
    let raw =
        std::env::var("DREAD_COMFYUI_URL").unwrap_or_else(|_| "http://127.0.0.1:8188".to_string());
    let url = reqwest::Url::parse(&raw).map_err(|_| "DREAD_COMFYUI_URL is invalid".to_string())?;
    let loopback = url.host_str().is_some_and(|host| {
        host == "localhost"
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    if url.scheme() != "http"
        || !loopback
        || url.port().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
    {
        return Err("ComfyUI monitor accepts only an explicit-port loopback HTTP endpoint".into());
    }
    Ok(raw.trim_end_matches('/').to_string())
}

fn summarize_queue(value: &Value) -> Value {
    fn jobs(value: &Value, key: &str, status: &str) -> Vec<Value> {
        value
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .take(100)
            .map(|entry| {
                let id = entry.get(1).and_then(Value::as_str).unwrap_or("unknown");
                let queue_position = entry.get(0).and_then(Value::as_u64);
                json!({"prompt_id": id, "queue_position": queue_position, "status": status})
            })
            .collect()
    }
    let running = jobs(value, "queue_running", "running");
    let pending = jobs(value, "queue_pending", "pending");
    json!({
        "source": "ComfyUI /queue",
        "read_only": true,
        "running_count": running.len(),
        "pending_count": pending.len(),
        "running": running,
        "pending": pending,
        "sampled_at_unix_ms": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    })
}

fn summarize_history(value: &Value) -> Vec<Value> {
    fn collect_files(value: &Value, files: &mut Vec<Value>) {
        match value {
            Value::Array(items) => {
                for item in items {
                    collect_files(item, files);
                }
            }
            Value::Object(object) => {
                if let Some(filename) = object.get("filename").and_then(Value::as_str) {
                    if files.len() < 20 {
                        files.push(json!({
                            "filename": filename,
                            "subfolder": object.get("subfolder").and_then(Value::as_str).unwrap_or(""),
                            "type": object.get("type").and_then(Value::as_str).unwrap_or("output")
                        }));
                    }
                } else {
                    for item in object.values() {
                        collect_files(item, files);
                    }
                }
            }
            _ => {}
        }
    }

    value
        .as_object()
        .into_iter()
        .flat_map(|history| history.iter())
        .take(10)
        .map(|(prompt_id, record)| {
            let status = record
                .pointer("/status/status_str")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let mut files = Vec::new();
            if let Some(outputs) = record.get("outputs") {
                collect_files(outputs, &mut files);
            }
            let error = record
                .pointer("/status/messages")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .find(|message| message.get(0).and_then(Value::as_str) == Some("execution_error"))
                .and_then(|message| message.get(1))
                .and_then(|data| {
                    data.get("exception_message")
                        .or_else(|| data.get("exception_type"))
                })
                .and_then(Value::as_str);
            json!({
                "prompt_id": prompt_id,
                "status": status,
                "outputs": files,
                "error": error
            })
        })
        .collect()
}

async fn read_limited(
    mut response: reqwest::Response,
) -> Result<Vec<u8>, (StatusCode, Json<Value>)> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_QUEUE_BYTES as u64)
    {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(json!({"error":"ComfyUI response exceeded the 2 MiB safety limit"})),
        ));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| {
        (
            StatusCode::BAD_GATEWAY,
            Json(json!({"error":"ComfyUI response could not be read"})),
        )
    })? {
        if body.len() + chunk.len() > MAX_QUEUE_BYTES {
            return Err((
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(json!({"error":"ComfyUI response exceeded the 2 MiB safety limit"})),
            ));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

pub async fn monitor() -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let base = comfyui_base_url()
        .map_err(|message| (StatusCode::BAD_REQUEST, Json(json!({"error": message}))))?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error":"ComfyUI HTTP client unavailable"})),
            )
        })?;
    let response = client
        .get(format!("{base}/queue"))
        .send()
        .await
        .map_err(|_| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error":"ComfyUI queue is unreachable","source":"127.0.0.1:8188"})),
            )
        })?;
    if !response.status().is_success() {
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(
                json!({"error":"ComfyUI queue returned a non-success status","upstream_status":response.status().as_u16()}),
            ),
        ));
    }
    let body = read_limited(response).await?;
    let value: Value = serde_json::from_slice(&body).map_err(|_| {
        (
            StatusCode::BAD_GATEWAY,
            Json(json!({"error":"ComfyUI queue returned invalid JSON"})),
        )
    })?;
    let mut summary = summarize_queue(&value);

    // History is supplemental: an unavailable history endpoint does not hide live queue state.
    let history = match client
        .get(format!("{base}/history?max_items=10"))
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => match read_limited(response).await {
            Ok(body) => serde_json::from_slice::<Value>(&body)
                .map(|value| summarize_history(&value))
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        },
        _ => Vec::new(),
    };
    summary["history_count"] = json!(history.len());
    summary["history"] = json!(history);
    summary["history_available"] = json!(!history.is_empty());
    Ok(Json(summary))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_summary_reports_only_real_running_and_pending_entries() {
        let value = json!({
            "queue_running": [[3, "running-id", {}, {}, []]],
            "queue_pending": [[4, "pending-id", {}, {}, []]]
        });
        let summary = summarize_queue(&value);
        assert_eq!(summary["running_count"], 1);
        assert_eq!(summary["pending_count"], 1);
        assert_eq!(summary["running"][0]["prompt_id"], "running-id");
        assert_eq!(summary["pending"][0]["status"], "pending");
        assert_eq!(summary["read_only"], true);
    }

    #[test]
    fn queue_summary_handles_empty_queue() {
        let summary = summarize_queue(&json!({"queue_running": [], "queue_pending": []}));
        assert_eq!(summary["running_count"], 0);
        assert_eq!(summary["pending_count"], 0);
    }

    #[test]
    fn history_summary_reports_real_status_outputs_and_errors() {
        let value = json!({
            "completed-id": {
                "status": {"status_str": "success", "messages": []},
                "outputs": {"1": {"images": [{"filename": "frame.png", "subfolder": "renders", "type": "output"}]}}
            },
            "failed-id": {
                "status": {"status_str": "error", "messages": [["execution_error", {"exception_message": "node failed"}]]},
                "outputs": {}
            }
        });
        let history = summarize_history(&value);
        assert_eq!(history[0]["status"], "success");
        assert_eq!(history[0]["outputs"][0]["filename"], "frame.png");
        assert_eq!(history[1]["error"], "node failed");
    }
}
