use futures::{StreamExt, future::join_all};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::net::IpAddr;
use std::{fs, time::Duration, time::Instant};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Service {
    pub id: String,
    pub name: String,
    pub url: String,
    pub role: String,
}

#[derive(Debug, Deserialize)]
pub struct ServiceConfig {
    pub services: Vec<Service>,
}

#[derive(Debug, Serialize)]
pub struct ServiceResult {
    pub id: String,
    pub name: String,
    pub url: String,
    pub role: String,
    pub status: String,
    pub http_status: Option<u16>,
    pub latency_ms: Option<u128>,
    pub detail: String,
    pub summary: Option<String>,
}

pub fn load_services(path: &str) -> Result<Vec<Service>, String> {
    let contents = fs::read_to_string(path).map_err(|e| format!("Cannot read {path}: {e}"))?;
    let config: ServiceConfig =
        toml::from_str(&contents).map_err(|e| format!("Invalid service configuration: {e}"))?;

    validate_services(&config.services)?;
    Ok(config.services)
}

fn validate_services(services: &[Service]) -> Result<(), String> {
    if services.len() > 32 {
        return Err("Service configuration exceeds the 32 endpoint safety limit".into());
    }
    let mut ids = HashSet::new();
    for service in services {
        if service.id.trim().is_empty()
            || service.name.trim().is_empty()
            || service.role.trim().is_empty()
            || service.id.len() > 128
            || service.name.len() > 200
            || service.role.len() > 128
            || !ids.insert(service.id.as_str())
        {
            return Err(format!(
                "Invalid or duplicate local service definition: {}",
                service.id
            ));
        }
        validate_local_service_url(&service.url)
            .map_err(|_| format!("Invalid local service definition: {}", service.id))?;
    }
    Ok(())
}

fn validate_local_service_url(raw: &str) -> Result<(), String> {
    let url = Url::parse(raw).map_err(|_| "invalid URL".to_string())?;
    if url.scheme() != "http" || url.port().is_none() {
        return Err("only explicit-port loopback HTTP URLs are allowed".into());
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("userinfo, query strings and fragments are not allowed".into());
    }
    let loopback = match url.host_str() {
        Some("localhost") => true,
        Some(host) => host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false),
        None => false,
    };
    if !loopback {
        return Err("service host must be loopback".into());
    }
    Ok(())
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentEndpoint {
    pub id: String,
    pub name: String,
    pub adapter: String,
    pub health_url: String,
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
pub struct AgentConfig {
    #[serde(default)]
    pub agents: Vec<AgentEndpoint>,
}

#[derive(Debug, Serialize)]
pub struct AgentProbeResult {
    pub id: String,
    pub name: String,
    pub adapter: String,
    pub health_url: String,
    pub enabled: bool,
    pub status: String,
    pub http_status: Option<u16>,
    pub latency_ms: Option<u128>,
    pub detail: String,
}

/// Small allowlist of known loopback agent runtimes. This is not a port sweep.
pub fn known_agent_endpoints() -> Vec<AgentEndpoint> {
    vec![
        AgentEndpoint {
            id: "openhands.agent-server".into(),
            name: "OpenHands Agent Server".into(),
            adapter: "openhands.agent_server".into(),
            health_url: "http://127.0.0.1:8000/health".into(),
            enabled: true,
        },
        AgentEndpoint {
            id: "langgraph.agent-server".into(),
            name: "LangGraph Agent Server".into(),
            adapter: "langgraph.agent_server".into(),
            health_url: "http://127.0.0.1:2024/ok".into(),
            enabled: true,
        },
    ]
}

pub fn load_agents(path: &str) -> Result<Vec<AgentEndpoint>, String> {
    let contents = fs::read_to_string(path).map_err(|e| format!("Cannot read {path}: {e}"))?;
    let config: AgentConfig =
        toml::from_str(&contents).map_err(|e| format!("Invalid agent configuration: {e}"))?;
    if config.agents.len() > 32 {
        return Err("Agent configuration exceeds the 32 endpoint safety limit".into());
    }
    let mut ids = std::collections::HashSet::new();
    for agent in &config.agents {
        if agent.id.trim().is_empty()
            || agent.name.trim().is_empty()
            || agent.adapter.trim().is_empty()
            || agent.id.len() > 128
            || agent.name.len() > 200
            || agent.adapter.len() > 128
            || !ids.insert(agent.id.as_str())
        {
            return Err(format!(
                "Invalid or duplicate local agent definition: {}",
                agent.id
            ));
        }
        validate_local_service_url(&agent.health_url).map_err(|_| {
            format!(
                "Agent {} must use an explicit-port loopback HTTP health URL",
                agent.id
            )
        })?;
    }
    Ok(config.agents)
}

async fn probe_agent(client: Client, agent: AgentEndpoint) -> AgentProbeResult {
    if !agent.enabled {
        return AgentProbeResult {
            id: agent.id,
            name: agent.name,
            adapter: agent.adapter,
            health_url: agent.health_url,
            enabled: false,
            status: "DISABLED".into(),
            http_status: None,
            latency_ms: None,
            detail: "Configured but disabled; no network request was made".into(),
        };
    }
    let started = Instant::now();
    match client.get(&agent.health_url).send().await {
        Ok(response) => {
            let code = response.status().as_u16();
            AgentProbeResult {
                id: agent.id,
                name: agent.name,
                adapter: agent.adapter,
                health_url: agent.health_url,
                enabled: true,
                status: classify_agent_http_status(code).into(),
                http_status: Some(code),
                latency_ms: Some(started.elapsed().as_millis()),
                detail: agent_status_detail(code).into(),
            }
        }
        Err(error) => AgentProbeResult {
            id: agent.id,
            name: agent.name,
            adapter: agent.adapter,
            health_url: agent.health_url,
            enabled: true,
            status: if error.is_timeout() {
                "UNKNOWN"
            } else {
                "OFFLINE"
            }
            .into(),
            http_status: None,
            latency_ms: Some(started.elapsed().as_millis()),
            detail: if error.is_timeout() {
                "Connection timed out"
            } else {
                "Endpoint could not be reached"
            }
            .into(),
        },
    }
}

pub async fn discover_agents(agents: Vec<AgentEndpoint>) -> Vec<AgentProbeResult> {
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(2))
        .build()
        .expect("HTTP client configuration should be valid");
    join_all(
        agents
            .into_iter()
            .map(|agent| probe_agent(client.clone(), agent)),
    )
    .await
}

pub fn classify_http_status(code: u16) -> &'static str {
    match code {
        200..=399 => "ONLINE",
        400..=499 => "REACHABLE",
        500..=599 => "DEGRADED",
        _ => "UNKNOWN",
    }
}

fn classify_agent_http_status(code: u16) -> &'static str {
    match code {
        200..=299 => "ONLINE",
        300..=399 => "REDIRECTED",
        400..=499 => "REACHABLE",
        500..=599 => "DEGRADED",
        _ => "UNKNOWN",
    }
}

fn agent_status_detail(code: u16) -> &'static str {
    match code {
        200..=299 => "Agent health endpoint responded successfully",
        300..=399 => "Agent endpoint redirected; redirects are not followed",
        400..=499 => "Agent host is reachable, but the health path or request was not accepted",
        500..=599 => "Agent host is reachable, but the service returned a server error",
        _ => "HTTP response received; agent health classification is unknown",
    }
}

fn status_detail(code: u16) -> &'static str {
    match code {
        200..=399 => "HTTP endpoint responded successfully",
        400..=499 => "Host is reachable, but this path or request is not accepted",
        500..=599 => "Host is reachable, but the service returned a server error",
        _ => "HTTP response received; health classification is unknown",
    }
}

const MAX_ADAPTER_BODY_BYTES: usize = 128 * 1024;

fn service_summary(role: &str, body: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(body).ok()?;
    match role {
        "local-inference" => {
            let models = value.get("models")?.as_array()?;
            let names = models
                .iter()
                .filter_map(|model| model.get("name").and_then(Value::as_str))
                .take(3)
                .collect::<Vec<_>>();
            let suffix = if models.len() > names.len() {
                ", …"
            } else {
                ""
            };
            Some(format!(
                "{} model(s) advertised{}{}",
                models.len(),
                if names.is_empty() { "" } else { ": " },
                if names.is_empty() {
                    String::new()
                } else {
                    format!("{}{}", names.join(", "), suffix)
                }
            ))
        }
        "image-video-runtime" => {
            let system = value.get("system")?;
            let devices = value.get("devices")?.as_array()?;
            let version = system
                .get("comfyui_version")
                .and_then(Value::as_str)
                .unwrap_or("version unknown");
            let names = devices
                .iter()
                .filter_map(|device| device.get("name").and_then(Value::as_str))
                .take(2)
                .collect::<Vec<_>>();
            let suffix = if devices.len() > names.len() {
                ", …"
            } else {
                ""
            };
            Some(format!(
                "ComfyUI {version}; {} device(s){}{}",
                devices.len(),
                if names.is_empty() { "" } else { ": " },
                if names.is_empty() {
                    String::new()
                } else {
                    format!("{}{}", names.join(", "), suffix)
                }
            ))
        }
        _ => None,
    }
}

async fn read_adapter_body(response: reqwest::Response) -> Result<Vec<u8>, String> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_ADAPTER_BODY_BYTES as u64)
    {
        return Err("Adapter response exceeded the 128 KiB metadata limit".into());
    }
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| "Could not read adapter response body".to_string())?;
        if body.len().saturating_add(chunk.len()) > MAX_ADAPTER_BODY_BYTES {
            return Err("Adapter response exceeded the 128 KiB metadata limit".into());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

async fn check_one(client: Client, service: Service) -> ServiceResult {
    let started = Instant::now();
    match client.get(&service.url).send().await {
        Ok(response) => {
            let code = response.status().as_u16();
            let mut status = classify_http_status(code).to_string();
            let mut detail = status_detail(code).to_string();
            let mut summary = None;
            if (200..300).contains(&code)
                && matches!(
                    service.role.as_str(),
                    "local-inference" | "image-video-runtime"
                )
            {
                match read_adapter_body(response).await {
                    Ok(body) => match service_summary(&service.role, &body) {
                        Some(value) => {
                            summary = Some(value);
                            detail = "HTTP endpoint and service-specific metadata are valid".into();
                        }
                        None => {
                            status = "DEGRADED".into();
                            detail =
                                "HTTP endpoint responded, but its adapter metadata was invalid"
                                    .into();
                        }
                    },
                    Err(error) => {
                        status = "DEGRADED".into();
                        detail = error;
                    }
                }
            }
            ServiceResult {
                id: service.id,
                name: service.name,
                url: service.url,
                role: service.role,
                status,
                http_status: Some(code),
                latency_ms: Some(started.elapsed().as_millis()),
                detail,
                summary,
            }
        }
        Err(error) => ServiceResult {
            id: service.id,
            name: service.name,
            url: service.url,
            role: service.role,
            status: if error.is_timeout() {
                "UNKNOWN"
            } else {
                "OFFLINE"
            }
            .into(),
            http_status: None,
            latency_ms: Some(started.elapsed().as_millis()),
            detail: if error.is_timeout() {
                "Connection timed out"
            } else {
                "Endpoint could not be reached"
            }
            .into(),
            summary: None,
        },
    }
}

pub async fn discover(services: Vec<Service>) -> Vec<ServiceResult> {
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(2))
        .build()
        .expect("HTTP client configuration should be valid");
    join_all(
        services
            .into_iter()
            .map(|service| check_one(client.clone(), service)),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::{classify_agent_http_status, classify_http_status, validate_local_service_url};

    #[test]
    fn known_agent_catalog_is_loopback_only_and_bounded() {
        let agents = super::known_agent_endpoints();
        assert_eq!(agents.len(), 2);
        assert!(agents.iter().all(|agent| agent.enabled));
        assert!(agents.iter().all(|agent| {
            agent.health_url.starts_with("http://127.0.0.1:")
        }));
    }

    #[test]
    fn service_config_rejects_duplicate_ids_and_too_many_endpoints() {
        let service = super::Service {
            id: "ollama".into(),
            name: "Ollama".into(),
            url: "http://127.0.0.1:11434/api/tags".into(),
            role: "local-inference".into(),
        };
        assert!(super::validate_services(&[service.clone(), service.clone()]).is_err());
        let many = (0..33)
            .map(|index| super::Service {
                id: format!("service-{index}"),
                name: format!("Service {index}"),
                url: "http://127.0.0.1:8000/health".into(),
                role: "generic".into(),
            })
            .collect::<Vec<_>>();
        assert!(super::validate_services(&many).is_err());
    }

    #[test]
    fn ollama_adapter_summarizes_model_metadata_without_returning_raw_payload() {
        let body = br#"{"models":[{"name":"qwen3.8:27b"},{"name":"llama3:8b"}]}"#;
        assert_eq!(
            super::service_summary("local-inference", body).as_deref(),
            Some("2 model(s) advertised: qwen3.8:27b, llama3:8b")
        );
    }

    #[test]
    fn comfyui_adapter_summarizes_version_and_gpu_devices() {
        let body = br#"{
            "system":{"comfyui_version":"0.43.0"},
            "devices":[
                {"name":"cuda:0 NVIDIA GeForce RTX 3090"},
                {"name":"cuda:1 NVIDIA GeForce RTX 3090"}
            ]
        }"#;
        assert_eq!(
            super::service_summary("image-video-runtime", body).as_deref(),
            Some(
                "ComfyUI 0.43.0; 2 device(s): cuda:0 NVIDIA GeForce RTX 3090, cuda:1 NVIDIA GeForce RTX 3090"
            )
        );
    }

    #[test]
    fn specialized_service_adapters_fail_closed_on_invalid_json() {
        assert!(super::service_summary("local-inference", b"<html>not json</html>").is_none());
        assert!(super::service_summary("image-video-runtime", br#"{"system":{}}"#).is_none());
        assert!(super::service_summary("control-plane", br#"{"models":[]}"#).is_none());
    }

    #[test]
    fn accepts_only_explicit_loopback_http_urls() {
        assert!(validate_local_service_url("http://127.0.0.1:8080/health").is_ok());
        assert!(validate_local_service_url("http://[::1]:8080/health").is_ok());
        assert!(validate_local_service_url("http://localhost:8080/health").is_ok());
        assert!(validate_local_service_url("http://127.0.0.1.evil.test:8080/health").is_err());
        assert!(validate_local_service_url("http://127.0.0.1:8080@evil.test/health").is_err());
        assert!(
            validate_local_service_url("http://127.0.0.1:8080/health?next=http://evil.test")
                .is_err()
        );
        assert!(validate_local_service_url("https://127.0.0.1:8080/health").is_err());
        assert!(validate_local_service_url("http://192.168.1.5:8080/health").is_err());
    }

    #[test]
    fn successful_responses_are_online() {
        assert_eq!(classify_http_status(200), "ONLINE");
        assert_eq!(classify_http_status(302), "ONLINE");
    }

    #[test]
    fn client_errors_mean_reachable_not_online() {
        assert_eq!(classify_http_status(404), "REACHABLE");
        assert_eq!(classify_http_status(401), "REACHABLE");
    }

    #[test]
    fn server_errors_are_degraded() {
        assert_eq!(classify_http_status(500), "DEGRADED");
        assert_eq!(classify_http_status(503), "DEGRADED");
    }

    #[test]
    fn agent_health_urls_must_be_loopback_and_explicit_port() {
        assert!(validate_local_service_url("http://127.0.0.1:8642/health").is_ok());
        assert!(validate_local_service_url("http://agent.example:8642/health").is_err());
        assert!(validate_local_service_url("http://127.0.0.1:8642/health?token=x").is_err());
    }

    #[test]
    fn agent_redirects_are_not_reported_as_online() {
        assert_eq!(classify_agent_http_status(200), "ONLINE");
        assert_eq!(classify_agent_http_status(302), "REDIRECTED");
        assert_eq!(classify_agent_http_status(401), "REACHABLE");
        assert_eq!(classify_agent_http_status(503), "DEGRADED");
    }
}
