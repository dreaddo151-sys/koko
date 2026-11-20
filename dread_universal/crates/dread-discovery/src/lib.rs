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


/// Untrusted, disabled-by-default local agent adapter manifest.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveredAgentManifest {
    pub id: String,
    pub name: String,
    pub adapter: String,
    pub health_url: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub capabilities: Vec<String>,
    pub manifest_path: String,
    pub enabled: bool,
    pub status: String,
    pub execution: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AgentManifestInventory {
    pub roots_scanned: Vec<String>,
    pub scanned_entries: usize,
    pub truncated: bool,
    pub manifests: Vec<DiscoveredAgentManifest>,
    pub warnings: Vec<String>,
    pub note: String,
}

#[derive(Debug, Deserialize)]
struct AgentManifestFile {
    id: String,
    name: String,
    adapter: String,
    health_url: String,
    version: Option<String>,
    description: Option<String>,
    #[serde(default)]
    capabilities: Vec<String>,
}

const AGENT_MANIFEST_MAX_ROOTS: usize = 16;
const AGENT_MANIFEST_MAX_DEPTH: usize = 4;
const AGENT_MANIFEST_MAX_ENTRIES: usize = 4_000;
const AGENT_MANIFEST_MAX_FILES: usize = 256;
const AGENT_MANIFEST_MAX_BYTES: u64 = 64 * 1024;

fn configured_agent_manifest_roots() -> Vec<std::path::PathBuf> {
    use std::path::PathBuf;

    if let Some(value) = std::env::var_os("DREAD_AGENT_MANIFEST_ROOTS") {
        return std::env::split_paths(&value)
            .filter(|path| !path.as_os_str().is_empty())
            .take(AGENT_MANIFEST_MAX_ROOTS + 1)
            .collect();
    }

    let mut roots = Vec::new();
    if let Ok(current) = std::env::current_dir() {
        roots.push(current.join("config/agents.d"));
        if let Some(parent) = current.parent() {
            roots.push(parent.join("config/agents.d"));
        }
    }
    if let Some(config_home) = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from) {
        roots.push(config_home.join("dread/agents.d"));
    } else if let Some(home) = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
    {
        roots.push(home.join(".config/dread/agents.d"));
    }
    if let Some(data_home) = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from) {
        roots.push(data_home.join("dread/agents.d"));
    }
    if let Some(home) = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
    {
        roots.push(home.join(".local/share/dread/agents.d"));
    }

    let mut seen = HashSet::new();
    roots.into_iter().filter(|path| seen.insert(path.clone())).collect()
}

/// Discover local adapter manifests only. This never probes their endpoint or
/// executes an adapter; every record is returned disabled for operator review.
pub fn discover_agent_manifests() -> AgentManifestInventory {
    discover_agent_manifests_in(&configured_agent_manifest_roots())
}

pub fn discover_agent_manifests_in(roots: &[std::path::PathBuf]) -> AgentManifestInventory {
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    let mut inventory = AgentManifestInventory {
        roots_scanned: Vec::new(),
        scanned_entries: 0,
        truncated: roots.len() > AGENT_MANIFEST_MAX_ROOTS,
        manifests: Vec::new(),
        warnings: Vec::new(),
        note: "Local agent manifests are untrusted metadata and remain disabled. Discovery does not probe health URLs, start providers, execute code, load plugins or grant permissions.".into(),
    };
    let mut seen_roots = BTreeSet::new();
    let mut seen_files = BTreeSet::new();
    if roots.len() > AGENT_MANIFEST_MAX_ROOTS {
        inventory.warnings.push(format!(
            "Agent manifest root list exceeded the {}-root limit",
            AGENT_MANIFEST_MAX_ROOTS
        ));
    }

    'roots: for configured_root in roots.iter().take(AGENT_MANIFEST_MAX_ROOTS) {
        if inventory.scanned_entries >= AGENT_MANIFEST_MAX_ENTRIES
            || inventory.manifests.len() >= AGENT_MANIFEST_MAX_FILES
        {
            inventory.truncated = true;
            break;
        }
        let metadata = match fs::symlink_metadata(configured_root) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                inventory.warnings.push(format!(
                    "Agent manifest root does not exist: {}",
                    configured_root.display()
                ));
                continue;
            }
            Err(error) => {
                inventory.warnings.push(format!(
                    "Could not inspect agent manifest root {}: {error}",
                    configured_root.display()
                ));
                continue;
            }
        };
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            inventory.warnings.push(format!(
                "Agent manifest root is not a regular directory: {}",
                configured_root.display()
            ));
            continue;
        }
        let root = match configured_root.canonicalize() {
            Ok(path) => path,
            Err(error) => {
                inventory.warnings.push(format!(
                    "Could not resolve agent manifest root {}: {error}",
                    configured_root.display()
                ));
                continue;
            }
        };
        if !seen_roots.insert(root.clone()) {
            continue;
        }
        inventory.roots_scanned.push(root.display().to_string());
        let mut pending = vec![(root.clone(), 0usize)];

        while let Some((directory, depth)) = pending.pop() {
            if inventory.scanned_entries >= AGENT_MANIFEST_MAX_ENTRIES
                || inventory.manifests.len() >= AGENT_MANIFEST_MAX_FILES
            {
                inventory.truncated = true;
                break 'roots;
            }
            let entries = match fs::read_dir(&directory) {
                Ok(entries) => entries,
                Err(error) => {
                    inventory.warnings.push(format!(
                        "Could not read agent manifest directory {}: {error}",
                        directory.display()
                    ));
                    continue;
                }
            };
            let mut children = Vec::<PathBuf>::new();
            for entry in entries {
                if inventory.scanned_entries >= AGENT_MANIFEST_MAX_ENTRIES {
                    inventory.truncated = true;
                    break;
                }
                inventory.scanned_entries += 1;
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(error) => {
                        inventory.warnings.push(format!(
                            "Could not read an agent manifest entry: {error}"
                        ));
                        continue;
                    }
                };
                let kind = match entry.file_type() {
                    Ok(kind) => kind,
                    Err(error) => {
                        inventory.warnings.push(format!(
                            "Could not inspect agent manifest entry: {error}"
                        ));
                        continue;
                    }
                };
                if kind.is_symlink() {
                    continue;
                }
                if kind.is_dir() {
                    if depth < AGENT_MANIFEST_MAX_DEPTH {
                        children.push(entry.path());
                    }
                    continue;
                }
                if !kind.is_file()
                    || !entry
                        .path()
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name == "agent.toml" || name.ends_with(".agent.toml"))
                {
                    continue;
                }

                let file_path = entry.path();
                let file_meta = match fs::symlink_metadata(&file_path) {
                    Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => meta,
                    _ => continue,
                };
                if file_meta.len() > AGENT_MANIFEST_MAX_BYTES {
                    inventory.warnings.push(format!(
                        "Agent manifest exceeds 64 KiB and was skipped: {}",
                        file_path.display()
                    ));
                    continue;
                }
                let canonical = match file_path.canonicalize() {
                    Ok(path) if path.starts_with(&root) => path,
                    _ => {
                        inventory.warnings.push(format!(
                            "Agent manifest escaped its configured root and was skipped: {}",
                            file_path.display()
                        ));
                        continue;
                    }
                };
                if !seen_files.insert(canonical.clone()) {
                    continue;
                }
                let bytes = match fs::read(&canonical) {
                    Ok(bytes) if bytes.len() as u64 <= AGENT_MANIFEST_MAX_BYTES => bytes,
                    Ok(_) => continue,
                    Err(error) => {
                        inventory.warnings.push(format!(
                            "Could not read agent manifest {}: {error}",
                            file_path.display()
                        ));
                        continue;
                    }
                };
                let manifest_text = match std::str::from_utf8(&bytes) {
                    Ok(text) => text,
                    Err(error) => {
                        inventory.warnings.push(format!(
                            "Agent manifest is not UTF-8 {}: {error}",
                            file_path.display()
                        ));
                        continue;
                    }
                };
                let manifest: AgentManifestFile = match toml::from_str(manifest_text) {
                    Ok(manifest) => manifest,
                    Err(error) => {
                        inventory.warnings.push(format!(
                            "Invalid agent manifest {}: {error}",
                            file_path.display()
                        ));
                        continue;
                    }
                };
                if !valid_agent_manifest_fields(&manifest) {
                    inventory.warnings.push(format!(
                        "Agent manifest has invalid identifiers or field lengths: {}",
                        file_path.display()
                    ));
                    continue;
                }
                if let Err(error) = validate_local_service_url(&manifest.health_url) {
                    inventory.warnings.push(format!(
                        "Agent manifest health URL must be explicit-port loopback HTTP ({}): {error}",
                        file_path.display()
                    ));
                    continue;
                }
                let relative_path = canonical
                    .strip_prefix(&root)
                    .map(|path| path.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_default();
                inventory.manifests.push(DiscoveredAgentManifest {
                    id: manifest.id,
                    name: manifest.name,
                    adapter: manifest.adapter,
                    health_url: manifest.health_url,
                    version: manifest.version,
                    description: manifest.description,
                    capabilities: manifest.capabilities.into_iter().take(64).collect(),
                    manifest_path: format!("{}/{}", root.display(), relative_path),
                    enabled: false,
                    status: "discovered_review_required".into(),
                    execution: "never_during_discovery".into(),
                });
            }
            children.sort();
            for child in children.into_iter().rev() {
                pending.push((child, depth + 1));
            }
        }
    }

    inventory.manifests.sort_by(|left, right| left.id.cmp(&right.id));
    inventory
}

fn valid_agent_manifest_fields(manifest: &AgentManifestFile) -> bool {
    let identifier = |value: &str| {
        !value.trim().is_empty()
            && value.len() <= 160
            && value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || b"-_.".contains(&byte)
            })
    };
    identifier(&manifest.id)
        && identifier(&manifest.adapter)
        && !manifest.name.trim().is_empty()
        && manifest.name.len() <= 200
        && manifest.version.as_ref().is_none_or(|value| value.len() <= 80)
        && manifest.description.as_ref().is_none_or(|value| value.len() <= 2_000)
        && manifest.capabilities.len() <= 128
        && manifest.capabilities.iter().all(|value| !value.trim().is_empty() && value.len() <= 160)
}

#[cfg(test)]
mod tests {
    use super::{classify_agent_http_status, classify_http_status, validate_local_service_url};

    #[test]
    fn known_agent_catalog_is_loopback_only_and_bounded() {
        let agents = super::known_agent_endpoints();
        assert_eq!(agents.len(), 2);
        assert!(agents.iter().all(|agent| agent.enabled));
        assert!(
            agents
                .iter()
                .all(|agent| { agent.health_url.starts_with("http://127.0.0.1:") })
        );
    }

    #[test]
    fn service_config_rejects_duplicate_ids_and_too_many_endpoints() {
        let service = super::Service {
            id: "llama-cpp".into(),
            name: "llama.cpp".into(),
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
    fn local_inference_adapter_summarizes_model_metadata_without_returning_raw_payload() {
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


#[cfg(test)]
mod agent_manifest_tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("dread-agent-manifests-{label}-{nonce}"));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn discovers_loopback_manifest_disabled_without_probing_or_execution() {
        let root = temp_dir("valid");
        let nested = root.join("openhands");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("agent.toml"), r#"
id = "openhands.local"
name = "OpenHands local adapter"
adapter = "openhands.agent_server"
health_url = "http://127.0.0.1:8000/health"
version = "1.0.0"
description = "Candidate only"
capabilities = ["task.start", "task.status"]
"#).unwrap();

        let inventory = discover_agent_manifests_in(std::slice::from_ref(&root));
        assert_eq!(inventory.manifests.len(), 1);
        let manifest = &inventory.manifests[0];
        assert_eq!(manifest.id, "openhands.local");
        assert!(!manifest.enabled);
        assert_eq!(manifest.status, "discovered_review_required");
        assert_eq!(manifest.execution, "never_during_discovery");
        assert!(inventory.note.contains("does not probe health URLs"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_non_loopback_or_invalid_agent_manifests() {
        let root = temp_dir("invalid");
        fs::write(root.join("remote.agent.toml"), r#"
id = "remote.agent"
name = "Remote"
adapter = "remote"
health_url = "http://192.168.1.10:8000/health"
"#).unwrap();
        fs::write(root.join("bad.agent.toml"), r#"id = "bad""#).unwrap();
        let inventory = discover_agent_manifests_in(std::slice::from_ref(&root));
        assert!(inventory.manifests.is_empty());
        assert_eq!(inventory.warnings.len(), 2);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn skips_symlinked_agent_manifest_files() {
        use std::os::unix::fs::symlink;
        let root = temp_dir("symlink");
        let outside = temp_dir("outside");
        fs::write(outside.join("outside.agent.toml"), r#"
id = "outside"
name = "Outside"
adapter = "outside"
health_url = "http://127.0.0.1:8000/health"
"#).unwrap();
        symlink(outside.join("outside.agent.toml"), root.join("linked.agent.toml")).unwrap();
        let inventory = discover_agent_manifests_in(std::slice::from_ref(&root));
        assert!(inventory.manifests.is_empty());
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }
}
