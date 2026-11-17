use axum::{Json, http::StatusCode};
use serde_json::{Value, json};
use std::time::Duration;

const MAX_QUEUE_BYTES: usize = 2 * 1024 * 1024;

fn comfyui_base_url() -> Result<String, String> {
    let raw = std::env::var("DREAD_COMFYUI_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8188".to_string());
    let url = reqwest::Url::parse(&raw).map_err(|_| "DREAD_COMFYUI_URL is invalid".to_string())?;
    let loopback = url.host_str().is_some_and(|host| {
        host == "localhost"
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    if url.scheme() != "http" || !loopback || url.port_or_known_default().is_none() {
        return Err("ComfyUI monitor accepts only an explicit loopback HTTP endpoint".into());
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

pub async fn monitor() -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let base = comfyui_base_url()
        .map_err(|message| (StatusCode::BAD_REQUEST, Json(json!({"error": message}))))?;
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error":"ComfyUI HTTP client unavailable"})),
            )
        })?
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
    let bytes = response.bytes().await.map_err(|_| {
        (
            StatusCode::BAD_GATEWAY,
            Json(json!({"error":"ComfyUI queue response could not be read"})),
        )
    })?;
    if bytes.len() > MAX_QUEUE_BYTES {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(json!({"error":"ComfyUI queue response exceeded the 2 MiB safety limit"})),
        ));
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| {
        (
            StatusCode::BAD_GATEWAY,
            Json(json!({"error":"ComfyUI queue returned invalid JSON"})),
        )
    })?;
    Ok(Json(summarize_queue(&value)))
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
}
