use anyhow::{Context, Result, ensure};
use reqwest::Url;
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;

const POLL_INTERVAL: Duration = Duration::from_secs(1);
const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(3600);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const LEASE_TTL_SECONDS: u64 = 2400;

#[derive(Debug, Deserialize)]
struct LeaseResponse {
    id: String,
    status: String,
    error: Option<String>,
}

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
            .timeout(REQUEST_TIMEOUT)
            .build()
            .context("Could not create GPU Supervisor HTTP client")?;
        Ok(Self {
            base_url: base_url.as_ref().trim_end_matches('/').to_string(),
            token,
            http,
        })
    }

    pub async fn acquire_tensor_lease(&self) -> Result<GpuLeaseGuard> {
        let response = self
            .http
            .post(format!("{}/v1/leases", self.base_url))
            .bearer_auth(&self.token)
            .json(&json!({
                "mode": "tensor",
                "ttl_seconds": LEASE_TTL_SECONDS
            }))
            .send()
            .await
            .context("GPU Supervisor lease request failed")?
            .error_for_status()
            .context("GPU Supervisor rejected the lease request")?;
        let mut lease = response
            .json::<LeaseResponse>()
            .await
            .context("GPU Supervisor returned an invalid lease response")?;
        let deadline = tokio::time::Instant::now() + ACQUIRE_TIMEOUT;
        loop {
            match lease.status.as_str() {
                "active" => {
                    return Ok(GpuLeaseGuard {
                        client: self.clone(),
                        id: lease.id,
                        released: false,
                    });
                }
                "failed" | "expired" | "released" => {
                    return Err(anyhow::anyhow!(
                        "GPU Supervisor lease {} ended before activation: {}",
                        lease.id,
                        lease.error.unwrap_or(lease.status)
                    ));
                }
                "pending" | "releasing" => {}
                other => {
                    return Err(anyhow::anyhow!(
                        "GPU Supervisor returned unknown lease state '{other}'"
                    ));
                }
            }
            ensure!(
                tokio::time::Instant::now() < deadline,
                "Timed out waiting for the Python GPU Supervisor lease"
            );
            tokio::time::sleep(POLL_INTERVAL).await;
            lease = self.lease_status(&lease.id).await?;
        }
    }

    async fn lease_status(&self, id: &str) -> Result<LeaseResponse> {
        let response = self
            .http
            .get(format!("{}/v1/leases/{id}", self.base_url))
            .bearer_auth(&self.token)
            .send()
            .await
            .context("GPU Supervisor lease status request failed")?
            .error_for_status()
            .context("GPU Supervisor lease status returned an error")?;
        response
            .json::<LeaseResponse>()
            .await
            .context("GPU Supervisor returned invalid lease status")
    }

    async fn release_lease(&self, id: &str) -> Result<()> {
        let response = self
            .http
            .delete(format!("{}/v1/leases/{id}", self.base_url))
            .bearer_auth(&self.token)
            .send()
            .await
            .context("GPU Supervisor lease release request failed")?
            .error_for_status()
            .context("GPU Supervisor rejected the lease release")?;
        let lease = response
            .json::<LeaseResponse>()
            .await
            .context("GPU Supervisor returned invalid release response")?;
        ensure!(
            matches!(lease.status.as_str(), "released" | "expired"),
            "GPU Supervisor did not confirm lease release: {}",
            lease.error.unwrap_or(lease.status)
        );
        Ok(())
    }
}

pub struct GpuLeaseGuard {
    client: GpuSupervisorClient,
    id: String,
    released: bool,
}

impl GpuLeaseGuard {
    pub async fn release(mut self) -> Result<()> {
        self.client.release_lease(&self.id).await?;
        self.released = true;
        Ok(())
    }

    pub fn id(&self) -> &str {
        &self.id
    }
}

impl Drop for GpuLeaseGuard {
    fn drop(&mut self) {
        if self.released {
            return;
        }
        let client = self.client.clone();
        let id = self.id.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                if let Err(error) = client.release_lease(&id).await {
                    eprintln!(
                        "[DREAD GPU] Supervisor lease {id} release failed; bridge TTL remains the final safety bound: {error}"
                    );
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Json, Router,
        extract::Path,
        routing::{delete, get, post},
    };
    use serde_json::Value;
    use tokio::net::TcpListener;

    async fn create_lease() -> Json<Value> {
        Json(json!({"id":"lease-test","status":"pending","error":null}))
    }

    async fn lease_status(Path(id): Path<String>) -> Json<Value> {
        Json(json!({"id":id,"status":"active","error":null}))
    }

    async fn release_lease(Path(id): Path<String>) -> Json<Value> {
        Json(json!({"id":id,"status":"released","error":null}))
    }

    #[tokio::test]
    async fn supervisor_client_waits_for_active_lease_and_confirms_release() {
        let app = Router::new()
            .route("/v1/leases", post(create_lease))
            .route("/v1/leases/{id}", get(lease_status).delete(release_lease));
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
        let lease = client.acquire_tensor_lease().await.unwrap();
        assert_eq!(lease.id(), "lease-test");
        lease.release().await.unwrap();
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
