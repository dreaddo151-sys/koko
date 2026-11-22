pub mod launch;
pub mod models;
pub mod token_booster;
pub mod supervisor;
pub use supervisor::GpuSupervisorClient;

use anyhow::{Context, Result, ensure};
use reqwest::{Response, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

const DEFAULT_GATEWAY: &str = "http://127.0.0.1:11500";
const HEALTH_TIMEOUT: Duration = Duration::from_secs(3);
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(5);
const INFERENCE_TIMEOUT: Duration = Duration::from_secs(1805);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayHealth {
    pub status: String,
    pub gpu: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelList {
    pub object: String,
    pub data: Vec<ModelInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub object: String,
    pub owned_by: Option<String>,
}

#[derive(Clone)]
pub struct GpuGatewayClient {
    base_url: String,
    http: reqwest::Client,
}

impl GpuGatewayClient {
    pub fn new(base_url: impl AsRef<str>) -> Result<Self> {
        let url = Url::parse(base_url.as_ref()).context("Invalid GPU gateway URL")?;

        ensure!(
            url.scheme() == "http",
            "GPU gateway must use HTTP on loopback"
        );
        ensure!(
            matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "::1")),
            "GPU gateway must use a loopback address"
        );
        ensure!(url.port().is_some(), "GPU gateway port is required");
        ensure!(
            url.username().is_empty()
                && url.password().is_none()
                && url.path() == "/"
                && url.query().is_none()
                && url.fragment().is_none(),
            "GPU gateway URL must contain only scheme, loopback host and port"
        );

        let base_url = base_url.as_ref().trim_end_matches('/').to_string();

        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(3))
            .build()
            .context("Could not create GPU gateway HTTP client")?;

        Ok(Self { base_url, http })
    }

    /// Construct the loopback gateway client from DREAD_GPU_GATEWAY_URL.
    /// The default remains the local Supervisor gateway used by the workstation build.
    pub fn from_env() -> Result<Self> {
        let endpoint =
            std::env::var("DREAD_GPU_GATEWAY_URL").unwrap_or_else(|_| DEFAULT_GATEWAY.to_string());
        Self::new(endpoint)
    }

    pub fn local() -> Result<Self> {
        Self::new(DEFAULT_GATEWAY)
    }

    pub async fn health(&self) -> Result<GatewayHealth> {
        let response = self
            .http
            .get(format!("{}/health", self.base_url))
            .timeout(HEALTH_TIMEOUT)
            .send()
            .await
            .context("GPU gateway health request failed")?;

        response
            .error_for_status()
            .context("GPU gateway health endpoint returned an error")?
            .json::<GatewayHealth>()
            .await
            .context("Invalid GPU gateway health response")
    }

    pub async fn models(&self) -> Result<ModelList> {
        let response = self
            .http
            .get(format!("{}/v1/models", self.base_url))
            .timeout(DISCOVERY_TIMEOUT)
            .send()
            .await
            .context("GPU gateway model discovery failed")?;

        response
            .error_for_status()
            .context("GPU gateway model discovery returned an error")?
            .json::<ModelList>()
            .await
            .context("Invalid GPU gateway model response")
    }

    pub async fn tags(&self) -> Result<Value> {
        let response = self
            .http
            .get(format!("{}/api/tags", self.base_url))
            .timeout(DISCOVERY_TIMEOUT)
            .send()
            .await
            .context("GPU gateway tags request failed")?;

        response
            .error_for_status()
            .context("GPU gateway tags endpoint returned an error")?
            .json::<Value>()
            .await
            .context("Invalid GPU gateway tags response")
    }

    // Returns the upstream response without consuming its body.
    // This preserves HTTP errors and permits callers to consume SSE streams.
    // GPU ownership remains entirely inside the Python gateway/Supervisor.
    pub async fn post_v1(&self, path: &str, payload: &Value) -> Result<Response> {
        ensure!(
            path.starts_with("/v1/")
                && !path.contains("..")
                && !path.contains('?')
                && !path.contains('#'),
            "Only safe /v1/ gateway paths are permitted"
        );

        let response = self
            .http
            .post(format!("{}{}", self.base_url, path))
            .header("Content-Type", "application/json")
            .timeout(INFERENCE_TIMEOUT)
            .json(payload)
            .send()
            .await
            .context("GPU gateway inference request failed")?;

        // Do not call error_for_status here. The caller needs the original
        // status and body for OpenAI-compatible error handling.
        Ok(response)
    }
}

impl Default for GpuGatewayClient {
    fn default() -> Self {
        Self::local().expect("Default loopback gateway URL is valid")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Json, Router,
        body::Body,
        http::StatusCode,
        response::Response,
        routing::{get, post},
    };
    use serde_json::json;
    use tokio::net::TcpListener;

    async fn health_handler() -> Json<Value> {
        Json(json!({
            "status": "ok",
            "gpu": "on-demand"
        }))
    }

    async fn models_handler() -> Json<Value> {
        Json(json!({
            "object": "list",
            "data": [{
                "id": "qwen3.8:27b",
                "object": "model",
                "owned_by": "DREAD"
            }]
        }))
    }

    async fn completion_handler(Json(_payload): Json<Value>) -> Response {
        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/event-stream")
            .body(Body::from(
                "data: {\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\n\
                 data: [DONE]\n\n",
            ))
            .unwrap()
    }

    async fn mock_gateway() -> Result<(String, tokio::task::JoinHandle<()>)> {
        let app = Router::new()
            .route("/health", get(health_handler))
            .route("/v1/models", get(models_handler))
            .route("/v1/chat/completions", post(completion_handler));

        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;

        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        Ok((format!("http://{address}"), task))
    }

    #[test]
    fn rejects_non_loopback_gateway() {
        assert!(GpuGatewayClient::new("http://192.168.1.10:11500").is_err());
        assert!(GpuGatewayClient::new("https://127.0.0.1:11500").is_err());
        assert!(GpuGatewayClient::new("http://127.0.0.1:11500@evil.test").is_err());
        assert!(GpuGatewayClient::new("http://127.0.0.1.evil.test:11500").is_err());
        assert!(GpuGatewayClient::new("http://user@127.0.0.1:11500").is_err());
    }

    #[test]
    fn accepts_local_gateway() {
        assert!(GpuGatewayClient::local().is_ok());
    }

    #[test]
    fn rejects_paths_outside_v1() {
        let client = GpuGatewayClient::local().unwrap();

        assert!(client.base_url.starts_with("http://127.0.0.1:11500"));
    }

    #[tokio::test]
    async fn health_models_and_sse_are_supported() -> Result<()> {
        let (url, server) = mock_gateway().await?;
        let client = GpuGatewayClient::new(url)?;

        let health = client.health().await?;
        assert_eq!(health.status, "ok");
        assert_eq!(health.gpu, "on-demand");

        let models = client.models().await?;
        assert_eq!(models.data.len(), 1);
        assert_eq!(models.data[0].id, "qwen3.8:27b");

        let response = client
            .post_v1(
                "/v1/chat/completions",
                &json!({
                    "model": "qwen3.8:27b",
                    "messages": [{"role": "user", "content": "hello"}],
                    "stream": true
                }),
            )
            .await?;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-type"], "text/event-stream");

        let body = response.text().await?;
        assert!(body.contains("hello"));
        assert!(body.contains("[DONE]"));

        server.abort();
        Ok(())
    }
}
