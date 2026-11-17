use anyhow::{Context, Result, ensure};
use reqwest::Url;
use serde_json::Value;
use std::time::Duration;

#[derive(Clone)]
pub struct GpuSupervisorClient {
    base_url: String,
    token: String,
    http: reqwest::Client,
}

impl GpuSupervisorClient {
    pub fn new(base_url: impl AsRef<str>, token: impl Into<String>) -> Result<Self> {
        let url = Url::parse(base_url.as_ref()).context("Invalid GPU Supervisor bridge URL")?;
        ensure!(
            url.scheme() == "http"
                && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "::1"))
                && url.port().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && url.path() == "/"
                && url.query().is_none()
                && url.fragment().is_none(),
            "GPU Supervisor bridge must be explicit-port loopback HTTP"
        );
        let token = token.into();
        ensure!(
            token.len() >= 32 && token.trim() == token,
            "GPU Supervisor bridge token must be at least 32 characters"
        );
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(10))
            .build()
            .context("Could not create GPU Supervisor HTTP client")?;
        Ok(Self {
            base_url: base_url.as_ref().trim_end_matches('/').to_string(),
            token,
            http,
        })
    }

    // The Python bridge owns the GPU lease. Rust only submits authenticated work.
    pub async fn chat_completion(&self, payload: &Value) -> Result<Value> {
        let body = serde_json::to_vec(payload).context("Could not encode GPU chat payload")?;
        ensure!(
            body.len() <= 1_048_576,
            "GPU chat request exceeds the 1 MiB Supervisor bridge limit"
        );
        let response = self
            .http
            .post(format!("{}/v1/chat/completions", self.base_url))
            .bearer_auth(&self.token)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .timeout(Duration::from_secs(1805))
            .body(body)
            .send()
            .await
            .context("Python GPU Supervisor bridge request failed")?;
        let status = response.status();
        if response
            .content_length()
            .is_some_and(|length| length > 1_048_576)
        {
            anyhow::bail!("GPU Supervisor response exceeds the 1 MiB result limit");
        }
        let response_body = response
            .bytes()
            .await
            .context("Could not read GPU Supervisor response")?;
        ensure!(
            response_body.len() <= 1_048_576,
            "GPU Supervisor response exceeds the 1 MiB result limit"
        );
        if !status.is_success() {
            anyhow::bail!(
                "GPU Supervisor returned HTTP {}: {}",
                status.as_u16(),
                String::from_utf8_lossy(&response_body)
            );
        }
        serde_json::from_slice(&response_body).context("GPU Supervisor returned invalid JSON")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Json, Router,
        http::{HeaderMap, StatusCode, header::AUTHORIZATION},
        routing::post,
    };
    use serde_json::{Value, json};
    use tokio::net::TcpListener;

    async fn completion(headers: HeaderMap, Json(payload): Json<Value>) -> (StatusCode, Json<Value>) {
        assert_eq!(
            headers.get(AUTHORIZATION).and_then(|value| value.to_str().ok()),
            Some("Bearer test-supervisor-token-that-is-long-enough")
        );
        (StatusCode::OK, Json(json!({"received": payload})))
    }

    #[tokio::test]
    async fn bridge_client_sends_authenticated_chat_without_lease_endpoints() {
        let app = Router::new().route("/v1/chat/completions", post(completion));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = GpuSupervisorClient::new(
            format!("http://{address}"),
            "test-supervisor-token-that-is-long-enough",
        )
        .unwrap();
        let result = client
            .chat_completion(&json!({
                "model": "qwen3.8:27b",
                "messages": [{"role": "user", "content": "hello"}],
                "stream": false
            }))
            .await
            .unwrap();
        assert_eq!(result["received"]["model"], "qwen3.8:27b");
        server.abort();
    }

    #[test]
    fn supervisor_client_rejects_remote_urls_and_short_tokens() {
        assert!(
            GpuSupervisorClient::new(
                "http://192.168.1.5:11502",
                "test-supervisor-token-that-is-long-enough"
            )
            .is_err()
        );
        assert!(GpuSupervisorClient::new("http://127.0.0.1:11502", "short").is_err());
    }
}
