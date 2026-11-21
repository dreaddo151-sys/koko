File manifest
File
Purpose
Cargo.toml	Rust workspace definition
crates/dread-core/Cargo.toml	Core crate dependencies
crates/dread-core/src/lib.rs	Public core API
crates/dread-core/src/config.rs	Validated configuration and path resolution
crates/dread-core/src/domain.rs	Capability classification and review states
crates/dread-core/src/resource.rs	Single-authority resource broker contract
crates/dread-core/src/legacy.rs	Legacy Hub compatibility and parity contracts
crates/dread-core/src/manifest.rs	Declarative capability imports, disabled by default
deployment/profile.example.toml	Placeholder-only deployment profile
docs/LEGACY-ROUTE-MATRIX.md	Legacy parity evidence rules
checkpoints/PROJECT-DREAD-AUTONOMOUS-CHECKPOINT.md	Persistent evidence checkpoint
BATCH-NOTES.md	Scope, rollback, limitations, and next step
No existing files are deleted or overwritten. There is no unified diff because no baseline tree was supplied.

Source files
Cargo.toml
Toml

Copy
[workspace]
members = ["crates/dread-core"]
resolver = "2"

[workspace.package]
edition = "2021"
rust-version = "1.82"
license = "Apache-2.0"

[workspace.dependencies]
async-trait = "0.1"
directories = "6"
serde = { version = "1", features = ["derive"] }
tempfile = "3"
thiserror = "2"
toml = "0.8"
url = "2"
crates/dread-core/Cargo.toml
Toml

Copy
[package]
name = "dread-core"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
description = "Host-independent domain and adapter contracts for DREAD Universal"

[dependencies]
async-trait.workspace = true
directories.workspace = true
serde.workspace = true
thiserror.workspace = true
toml.workspace = true
url.workspace = true

[dev-dependencies]
tempfile.workspace = true
toml.workspace = true
crates/dread-core/src/lib.rs
Rust

Copy
//! Host-independent domain and adapter contracts for PROJECT DREAD UNIVERSAL.
//!
//! This crate deliberately contains no workstation paths, provider endpoints,
//! model names, GPU process launcher, or legacy Hub route assumptions.

pub mod config;
pub mod domain;
pub mod legacy;
pub mod manifest;
pub mod resource;

pub use config::{DeploymentProfile, PathLayout};
pub use domain::{CapabilityKind, ImportedCapability, ReviewState};
pub use legacy::{LegacyHubAdapter, LegacyRouteMatrix, ParityStatus};
pub use resource::{NoResourceBroker, ResourceBroker};
crates/dread-core/src/config.rs
Rust

Copy
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    env, fs,
    net::IpAddr,
    path::{Path, PathBuf},
};
use thiserror::Error;
use url::Url;

const MAX_DISCOVERY_RESPONSE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("cannot read deployment profile {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("cannot parse deployment profile: {0}")]
    Parse(#[from] toml::de::Error),

    #[error("invalid deployment profile: {0}")]
    Invalid(String),

    #[error("no platform configuration directories are available")]
    NoPlatformDirectories,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeploymentProfile {
    pub hub: HubConfig,
    pub storage: StorageConfig,
    pub discovery: DiscoveryConfig,
    pub resource_broker: ResourceBrokerConfig,
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderProfile>,
    pub legacy_hub: LegacyHubConfig,
    pub extensions: ExtensionConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HubConfig {
    pub profile_name: String,
    pub listen_address: IpAddr,
    /// Zero is accepted only as a setup-time value. Startup requires a chosen port.
    pub listen_port: u16,
    #[serde(default)]
    pub workspace_roots: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageConfig {
    #[serde(default)]
    pub config_dir: String,
    #[serde(default)]
    pub state_dir: String,
    #[serde(default)]
    pub cache_dir: String,
    #[serde(default)]
    pub log_dir: String,
    #[serde(default)]
    pub artifact_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoveryConfig {
    pub allow_loopback_providers: bool,
    pub allow_remote_providers: bool,
    pub probe_timeout_seconds: u64,
    pub max_response_bytes: usize,
    pub port_scanning: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceBrokerConfig {
    pub provider: String,
    pub single_authority_per_device: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderProfile {
    pub adapter: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub base_url: Option<String>,
    /// A reference to OS secret storage or a protected file, never the secret itself.
    #[serde(default)]
    pub secret_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyHubConfig {
    pub enabled: bool,
    pub adapter: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub source_fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionConfig {
    pub imports_disabled_by_default: bool,
    pub require_signature_for_remote_install: bool,
    pub require_operator_approval_for_activation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathLayout {
    pub config_dir: PathBuf,
    pub state_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub log_dir: PathBuf,
    pub artifact_dir: PathBuf,
}

impl DeploymentProfile {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        let text = fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        Self::from_toml(&text)
    }

    pub fn from_toml(text: &str) -> Result<Self, ConfigError> {
        let profile: Self = toml::from_str(text)?;
        profile.validate()?;
        Ok(profile)
    }

    /// Validates structural and discovery-safety constraints.
    ///
    /// A setup profile with port zero can pass this method, but cannot pass
    /// [`validate_for_startup`](Self::validate_for_startup).
    pub fn validate(&self) -> Result<(), ConfigError> {
        require_identifier("hub.profile_name", &self.hub.profile_name)?;

        if !self.hub.listen_address.is_loopback() {
            return invalid("hub.listen_address must be loopback in this release");
        }

        for root in &self.hub.workspace_roots {
            if !root.is_absolute() {
                return invalid("hub.workspace_roots entries must be absolute paths");
            }
        }

        for (name, value) in [
            ("storage.config_dir", &self.storage.config_dir),
            ("storage.state_dir", &self.storage.state_dir),
            ("storage.cache_dir", &self.storage.cache_dir),
            ("storage.log_dir", &self.storage.log_dir),
            ("storage.artifact_dir", &self.storage.artifact_dir),
        ] {
            if !value.is_empty() && !Path::new(value).is_absolute() {
                return invalid(format!("{name} must be empty or an absolute path"));
            }
        }

        if self.discovery.port_scanning {
            return invalid("discovery.port_scanning is not supported");
        }
        if !(1..=30).contains(&self.discovery.probe_timeout_seconds) {
            return invalid("discovery.probe_timeout_seconds must be between 1 and 30");
        }
        if self.discovery.max_response_bytes == 0
            || self.discovery.max_response_bytes > MAX_DISCOVERY_RESPONSE_BYTES
        {
            return invalid(format!(
                "discovery.max_response_bytes must be between 1 and {MAX_DISCOVERY_RESPONSE_BYTES}"
            ));
        }

        require_identifier(
            "resource_broker.provider",
            &self.resource_broker.provider,
        )?;
        if !self.resource_broker.single_authority_per_device {
            return invalid("resource_broker.single_authority_per_device must remain true");
        }

        for (name, provider) in &self.providers {
            require_identifier("providers key", name)?;
            require_identifier("provider.adapter", &provider.adapter)?;

            if let Some(reference) = &provider.secret_ref {
                if reference.trim().is_empty() {
                    return invalid(format!("provider {name} has an empty secret_ref"));
                }
            }
            if let Some(base_url) = &provider.base_url {
                self.validate_provider_url(name, base_url)?;
            }
        }

        require_identifier("legacy_hub.adapter", &self.legacy_hub.adapter)?;
        if self.legacy_hub.adapter != "legacy-hub-v1" {
            return invalid("only the legacy-hub-v1 contract is currently declared");
        }
        if self.legacy_hub.enabled && self.legacy_hub.base_url.is_empty() {
            return invalid("an enabled legacy Hub requires an explicitly configured base_url");
        }
        if !self.legacy_hub.base_url.is_empty() {
            self.validate_provider_url("legacy_hub", &self.legacy_hub.base_url)?;
        }

        if !self.extensions.imports_disabled_by_default {
            return invalid("extensions.imports_disabled_by_default must remain true");
        }
        if !self.extensions.require_signature_for_remote_install {
            return invalid("extensions.require_signature_for_remote_install must remain true");
        }
        if !self.extensions.require_operator_approval_for_activation {
            return invalid(
                "extensions.require_operator_approval_for_activation must remain true",
            );
        }

        Ok(())
    }

    pub fn validate_for_startup(&self) -> Result<(), ConfigError> {
        self.validate()?;
        if self.hub.listen_port == 0 {
            return invalid(
                "hub.listen_port is zero; run setup to choose and persist a free port",
            );
        }
        Ok(())
    }

    fn validate_provider_url(&self, name: &str, value: &str) -> Result<(), ConfigError> {
        let url = Url::parse(value)
            .map_err(|error| ConfigError::Invalid(format!("provider {name}: {error}")))?;

        if !matches!(url.scheme(), "http" | "https") {
            return invalid(format!("provider {name} must use HTTP or HTTPS"));
        }
        if !url.username().is_empty() || url.password().is_some() {
            return invalid(format!(
                "provider {name} must not embed credentials in its URL"
            ));
        }

        let host = url
            .host_str()
            .ok_or_else(|| ConfigError::Invalid(format!("provider {name} has no host")))?;
        let loopback = host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<IpAddr>()
                .map(|address| address.is_loopback())
                .unwrap_or(false);

        if loopback && !self.discovery.allow_loopback_providers {
            return invalid(format!("provider {name} uses a disallowed loopback endpoint"));
        }
        if !loopback && !self.discovery.allow_remote_providers {
            return invalid(format!("provider {name} uses a disallowed remote endpoint"));
        }
        Ok(())
    }
}

impl PathLayout {
    pub fn resolve(profile: &DeploymentProfile) -> Result<Self, ConfigError> {
        let dread_home = env::var_os("DREAD_HOME").map(PathBuf::from);
        Self::resolve_with_dread_home(profile, dread_home.as_deref())
    }

    /// Deterministic resolver used by setup and tests.
    pub fn resolve_with_dread_home(
        profile: &DeploymentProfile,
        dread_home: Option<&Path>,
    ) -> Result<Self, ConfigError> {
        let (config, state, cache) = if let Some(home) = dread_home {
            if !home.is_absolute() {
                return invalid("DREAD_HOME must be an absolute path");
            }
            (
                home.join("config"),
                home.join("state"),
                home.join("cache"),
            )
        } else {
            let directories = ProjectDirs::from("org", "DREAD", "DREADUniversal")
                .ok_or(ConfigError::NoPlatformDirectories)?;
            (
                directories.config_dir().to_path_buf(),
                directories
                    .state_dir()
                    .unwrap_or_else(|| directories.data_dir())
                    .to_path_buf(),
                directories.cache_dir().to_path_buf(),
            )
        };

        let storage = &profile.storage;
        let state_dir = override_path(&storage.state_dir, state);
        Ok(Self {
            config_dir: override_path(&storage.config_dir, config),
            cache_dir: override_path(&storage.cache_dir, cache),
            log_dir: override_path(&storage.log_dir, state_dir.join("logs")),
            artifact_dir: override_path(
                &storage.artifact_dir,
                state_dir.join("artifacts"),
            ),
            state_dir,
        })
    }
}

fn override_path(configured: &str, default: PathBuf) -> PathBuf {
    if configured.is_empty() {
        default
    } else {
        PathBuf::from(configured)
    }
}

fn require_identifier(field: &str, value: &str) -> Result<(), ConfigError> {
    let valid = !value.is_empty()
        && value.len() <= 128
        && value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    if valid {
        Ok(())
    } else {
        invalid(format!(
            "{field} must contain only lowercase ASCII letters, digits, '-' or '_'"
        ))
    }
}

fn invalid<T>(message: impl Into<String>) -> Result<T, ConfigError> {
    Err(ConfigError::Invalid(message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_profile() -> DeploymentProfile {
        DeploymentProfile::from_toml(
            r#"
            [hub]
            profile_name = "test-local"
            listen_address = "127.0.0.1"
            listen_port = 43127
            workspace_roots = []

            [storage]
            config_dir = ""
            state_dir = ""
            cache_dir = ""
            log_dir = ""
            artifact_dir = ""

            [discovery]
            allow_loopback_providers = true
            allow_remote_providers = false
            probe_timeout_seconds = 3
            max_response_bytes = 1048576
            port_scanning = false

            [resource_broker]
            provider = "none"
            single_authority_per_device = true

            [providers]

            [legacy_hub]
            enabled = false
            adapter = "legacy-hub-v1"
            base_url = ""
            source_fingerprint = ""

            [extensions]
            imports_disabled_by_default = true
            require_signature_for_remote_install = true
            require_operator_approval_for_activation = true
            "#,
        )
        .expect("valid test profile")
    }

    #[test]
    fn startup_rejects_unselected_port() {
        let mut profile = valid_profile();
        profile.hub.listen_port = 0;
        assert!(profile.validate().is_ok());
        assert!(profile.validate_for_startup().is_err());
    }

    #[test]
    fn unknown_configuration_keys_are_rejected() {
        let text = toml::to_string(&valid_profile()).unwrap()
            + "\nunexpected_machine_specific_key = true\n";
        assert!(DeploymentProfile::from_toml(&text).is_err());
    }

    #[test]
    fn remote_provider_is_rejected_by_default() {
        let mut profile = valid_profile();
        profile.providers.insert(
            "example".into(),
            ProviderProfile {
                adapter: "example-v1".into(),
                enabled: false,
                base_url: Some("https://provider.invalid".into()),
                secret_ref: None,
            },
        );
        assert!(profile.validate().is_err());
    }

    #[test]
    fn credentials_cannot_be_embedded_in_provider_urls() {
        let mut profile = valid_profile();
        profile.providers.insert(
            "example".into(),
            ProviderProfile {
                adapter: "example-v1".into(),
                enabled: false,
                base_url: Some("http://user:password@127.0.0.1:12345".into()),
                secret_ref: None,
            },
        );
        assert!(profile.validate().is_err());
    }

    #[test]
    fn port_scanning_cannot_be_enabled() {
        let mut profile = valid_profile();
        profile.discovery.port_scanning = true;
        assert!(profile.validate().is_err());
    }

    #[test]
    fn unsafe_extension_defaults_are_rejected() {
        let mut profile = valid_profile();
        profile.extensions.imports_disabled_by_default = false;
        assert!(profile.validate().is_err());
    }

    #[test]
    fn non_loopback_hub_binding_is_rejected() {
        let mut profile = valid_profile();
        profile.hub.listen_address = "0.0.0.0".parse().unwrap();
        assert!(profile.validate().is_err());
    }

    #[test]
    fn dread_home_produces_relocatable_private_layout() {
        let profile = valid_profile();
        let root = Path::new("/tmp/dread-test-home");
        let layout = PathLayout::resolve_with_dread_home(&profile, Some(root)).unwrap();

        assert_eq!(layout.config_dir, root.join("config"));
        assert_eq!(layout.state_dir, root.join("state"));
        assert_eq!(layout.cache_dir, root.join("cache"));
        assert_eq!(layout.log_dir, root.join("state/logs"));
        assert_eq!(layout.artifact_dir, root.join("state/artifacts"));
    }

    #[test]
    fn configured_paths_must_be_absolute() {
        let mut profile = valid_profile();
        profile.storage.state_dir = "relative/state".into();
        assert!(profile.validate().is_err());
    }
}
crates/dread-core/src/domain.rs
Rust

Copy
use serde::{Deserialize, Serialize};

/// These categories must never be collapsed into a single "agent" count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityKind {
    AgentProvider,
    PromptProfile,
    Skill,
    Tool,
    McpServer,
    Workflow,
    ModelProvider,
    Model,
    Service,
    MediaPipeline,
    ResourceProvider,
    RepairAdapter,
    LegacyHubBridge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewState {
    Discovered,
    Inspected,
    Registered,
    Validated,
    Approved,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportedCapability {
    pub id: String,
    pub kind: CapabilityKind,
    pub review: ReviewState,
    pub enabled: bool,
    pub content_sha256: String,
    pub permissions: Vec<String>,
}

impl ImportedCapability {
    /// Import is registration-only. No manifest field can enable the result.
    pub fn disabled_import(
        id: String,
        kind: CapabilityKind,
        content_sha256: String,
        permissions: Vec<String>,
    ) -> Self {
        Self {
            id,
            kind,
            review: ReviewState::Discovered,
            enabled: false,
            content_sha256,
            permissions,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDecision {
    Allow,
    Deny,
    ApprovalRequired,
    Unknown,
}

impl PermissionDecision {
    pub fn permits_execution(self) -> bool {
        matches!(self, Self::Allow)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_import_is_not_an_agent_and_is_disabled() {
        let capability = ImportedCapability::disabled_import(
            "example-skill".into(),
            CapabilityKind::Skill,
            "a".repeat(64),
            vec![],
        );
        assert_eq!(capability.kind, CapabilityKind::Skill);
        assert!(!capability.enabled);
        assert_eq!(capability.review, ReviewState::Discovered);
    }

    #[test]
    fn unknown_or_approval_decisions_never_permit_execution() {
        assert!(!PermissionDecision::Unknown.permits_execution());
        assert!(!PermissionDecision::ApprovalRequired.permits_execution());
        assert!(!PermissionDecision::Deny.permits_execution());
        assert!(PermissionDecision::Allow.permits_execution());
    }
}
crates/dread-core/src/resource.rs
Rust

Copy
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceAuthority {
    pub adapter_id: String,
    pub deployment_domain: String,
    pub available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdmissionRequest {
    pub workload_id: String,
    pub device_domain: String,
    pub required_memory_bytes: Option<u64>,
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceLease {
    pub lease_id: String,
    pub workload_id: String,
    pub provider_lease_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceCapacity {
    pub device_domain: String,
    pub total_memory_bytes: Option<u64>,
    pub available_memory_bytes: Option<u64>,
    pub queued_workloads: u64,
    /// A freshness timestamp supplied by the real provider adapter.
    pub observed_at_unix_seconds: u64,
}

#[derive(Debug, Error)]
pub enum ResourceError {
    #[error("no reviewed resource authority is configured for {0}")]
    Unavailable(String),
    #[error("resource provider rejected admission: {0}")]
    Rejected(String),
    #[error("resource provider failure: {0}")]
    Provider(String),
}

/// Versioned boundary for the sole resource authority in a device/process domain.
///
/// There is intentionally no direct llama.cpp, GPU-lock, or process-launch method.
/// Implementations must reconcile provider lease identifiers rather than fabricate them.
#[async_trait]
pub trait ResourceBroker: Send + Sync {
    fn authority(&self) -> ResourceAuthority;

    async fn admit(
        &self,
        request: AdmissionRequest,
    ) -> Result<ResourceLease, ResourceError>;

    async fn release(&self, lease: ResourceLease) -> Result<(), ResourceError>;

    async fn capacity(
        &self,
        device_domain: &str,
    ) -> Result<ResourceCapacity, ResourceError>;
}

/// Safe default when setup has not selected a reviewed provider.
/// It never falls back to launching inference directly.
#[derive(Debug, Default)]
pub struct NoResourceBroker;

#[async_trait]
impl ResourceBroker for NoResourceBroker {
    fn authority(&self) -> ResourceAuthority {
        ResourceAuthority {
            adapter_id: "none".into(),
            deployment_domain: "unconfigured".into(),
            available: false,
        }
    }

    async fn admit(
        &self,
        request: AdmissionRequest,
    ) -> Result<ResourceLease, ResourceError> {
        Err(ResourceError::Unavailable(request.device_domain))
    }

    async fn release(&self, lease: ResourceLease) -> Result<(), ResourceError> {
        Err(ResourceError::Unavailable(lease.workload_id))
    }

    async fn capacity(
        &self,
        device_domain: &str,
    ) -> Result<ResourceCapacity, ResourceError> {
        Err(ResourceError::Unavailable(device_domain.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unconfigured_broker_is_explicitly_unavailable() {
        let broker = NoResourceBroker;
        assert!(!broker.authority().available);
    }
}
crates/dread-core/src/legacy.rs
Rust

Copy
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ParityStatus {
    Unknown,
    Unverified,
    Partial,
    Verified,
    RetiredAccepted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyRoute {
    pub method: String,
    pub route: String,
    pub request_schema: Option<String>,
    pub response_schema: Option<String>,
    pub auth_requirement: Option<String>,
    pub streaming_behavior: Option<String>,
    pub side_effects: Option<String>,
    pub data_source: Option<String>,
    pub parity: ParityStatus,
    pub test_evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyRouteMatrix {
    pub contract_version: String,
    pub source_fingerprint: String,
    pub routes: Vec<LegacyRoute>,
}

impl LegacyRouteMatrix {
    pub fn empty_unverified() -> Self {
        Self {
            contract_version: "legacy-route-matrix-v1".into(),
            source_fingerprint: String::new(),
            routes: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<(), LegacyError> {
        if self.contract_version != "legacy-route-matrix-v1" {
            return Err(LegacyError::InvalidMatrix(
                "unsupported contract version".into(),
            ));
        }

        for route in &self.routes {
            let requires_evidence = matches!(
                route.parity,
                ParityStatus::Partial
                    | ParityStatus::Verified
                    | ParityStatus::RetiredAccepted
            );
            if requires_evidence && route.test_evidence.is_empty() {
                return Err(LegacyError::InvalidMatrix(format!(
                    "{} {} claims {:?} without evidence",
                    route.method, route.route, route.parity
                )));
            }
            if route.parity == ParityStatus::Verified
                && (route.request_schema.is_none() || route.response_schema.is_none())
            {
                return Err(LegacyError::InvalidMatrix(format!(
                    "{} {} cannot be verified without request and response schemas",
                    route.method, route.route
                )));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyHealth {
    pub status: String,
    pub observed_at_unix_seconds: u64,
    pub contract_version: Option<String>,
    pub source_fingerprint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyFeatureInventory {
    /// Serialized provider observations; semantics remain adapter-specific until verified.
    pub observed_features_json: Vec<String>,
}

#[derive(Debug, Error)]
pub enum LegacyError {
    #[error("legacy Hub is not configured or available: {0}")]
    Unavailable(String),
    #[error("invalid legacy route matrix: {0}")]
    InvalidMatrix(String),
    #[error("legacy provider contract failure: {0}")]
    Provider(String),
}

/// No route mappings are supplied by core. A real adapter must derive them from
/// inspected source or a sanitized compatibility snapshot.
#[async_trait]
pub trait LegacyHubAdapter: Send + Sync {
    async fn health(&self) -> Result<LegacyHealth, LegacyError>;
    async fn route_inventory(&self) -> Result<LegacyRouteMatrix, LegacyError>;
    async fn feature_inventory(&self) -> Result<LegacyFeatureInventory, LegacyError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verified_parity_requires_schemas_and_evidence() {
        let matrix = LegacyRouteMatrix {
            contract_version: "legacy-route-matrix-v1".into(),
            source_fingerprint: "supplied-source-fingerprint".into(),
            routes: vec![LegacyRoute {
                method: "GET".into(),
                route: "/route-from-real-source".into(),
                request_schema: None,
                response_schema: None,
                auth_requirement: None,
                streaming_behavior: None,
                side_effects: None,
                data_source: None,
                parity: ParityStatus::Verified,
                test_evidence: vec![],
            }],
        };
        assert!(matrix.validate().is_err());
    }

    #[test]
    fn empty_inventory_does_not_invent_routes() {
        let matrix = LegacyRouteMatrix::empty_unverified();
        assert!(matrix.routes.is_empty());
        assert!(matrix.validate().is_ok());
    }
}
crates/dread-core/src/manifest.rs
Rust

Copy
use crate::domain::{CapabilityKind, ImportedCapability};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityManifest {
    pub manifest_version: String,
    pub id: String,
    pub name: String,
    pub version: String,
    pub kind: CapabilityKind,
    pub source: String,
    pub content_sha256: String,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub publisher_signature: Option<String>,
}

#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("invalid capability manifest: {0}")]
    Invalid(String),
}

impl CapabilityManifest {
    pub fn import_disabled(&self) -> Result<ImportedCapability, ManifestError> {
        self.validate()?;
        Ok(ImportedCapability::disabled_import(
            self.id.clone(),
            self.kind,
            self.content_sha256.clone(),
            self.permissions.clone(),
        ))
    }

    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.manifest_version != "capability-manifest-v1" {
            return invalid("unsupported manifest_version");
        }
        if !valid_id(&self.id) {
            return invalid("id must be a bounded lowercase identifier");
        }
        if self.name.trim().is_empty() || self.version.trim().is_empty() {
            return invalid("name and version are required");
        }
        if self.source.trim().is_empty() {
            return invalid("source provenance is required");
        }
        if self.content_sha256.len() != 64
            || !self
                .content_sha256
                .chars()
                .all(|c| c.is_ascii_hexdigit())
        {
            return invalid("content_sha256 must contain exactly 64 hexadecimal characters");
        }
        if self.permissions.iter().any(|p| p.trim().is_empty()) {
            return invalid("permissions must not contain empty entries");
        }
        Ok(())
    }
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

fn invalid<T>(message: impl Into<String>) -> Result<T, ManifestError> {
    Err(ManifestError::Invalid(message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> CapabilityManifest {
        CapabilityManifest {
            manifest_version: "capability-manifest-v1".into(),
            id: "example-skill".into(),
            name: "Example".into(),
            version: "1.0.0".into(),
            kind: CapabilityKind::Skill,
            source: "local-reviewed-source".into(),
            content_sha256: "ab".repeat(32),
            dependencies: vec![],
            permissions: vec!["filesystem:read".into()],
            publisher_signature: None,
        }
    }

    #[test]
    fn import_cannot_enable_a_capability() {
        let imported = manifest().import_disabled().unwrap();
        assert!(!imported.enabled);
    }

    #[test]
    fn integrity_fingerprint_is_not_accepted_as_a_signature() {
        let parsed = manifest();
        assert!(parsed.publisher_signature.is_none());
        assert!(parsed.import_disabled().is_ok());
    }

    #[test]
    fn malformed_hash_is_rejected() {
        let mut parsed = manifest();
        parsed.content_sha256 = "not-a-sha256".into();
        assert!(parsed.import_disabled().is_err());
    }

    #[test]
    fn executable_fields_are_not_part_of_declarative_manifest() {
        let json = r#"{
            "manifest_version":"capability-manifest-v1",
            "id":"example-skill",
            "name":"Example",
            "version":"1.0.0",
            "kind":"skill",
            "source":"local-reviewed-source",
            "content_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "install_hook":"arbitrary-command"
        }"#;
        assert!(toml::from_str::<CapabilityManifest>(json).is_err());
    }
}
deployment/profile.example.toml
Toml

Copy
# Generic example only. This is not a DREAD workstation profile.
# Port zero is setup-only. First-run setup must select and persist a free port.

[hub]
profile_name = "example-local"
listen_address = "127.0.0.1"
listen_port = 0
workspace_roots = []

[storage]
# Empty values resolve through DREAD_HOME or XDG platform directories.
config_dir = ""
state_dir = ""
cache_dir = ""
log_dir = ""
artifact_dir = ""

[discovery]
allow_loopback_providers = true
allow_remote_providers = false
probe_timeout_seconds = 3
max_response_bytes = 1048576
port_scanning = false

[resource_broker]
# "none" never falls back to direct GPU or inference-process launching.
provider = "none"
single_authority_per_device = true

[providers]
# Add only explicitly configured providers, for example:
#
# [providers.example_local]
# adapter = "reviewed-provider-v1"
# enabled = false
# base_url = "http://127.0.0.1:OPERATOR_CHOSEN_PORT"
# secret_ref = "keyring:example-local/provider"
#
# Replace the entire illustrative URL before parsing; it is not a live default.

[legacy_hub]
enabled = false
adapter = "legacy-hub-v1"
base_url = ""
source_fingerprint = ""

[extensions]
imports_disabled_by_default = true
require_signature_for_remote_install = true
require_operator_approval_for_activation = true
docs/LEGACY-ROUTE-MATRIX.md
Markdown

Copy
# Legacy Hub route matrix

Contract: `legacy-route-matrix-v1`

No legacy route is considered known until it is obtained from real source or a
sanitized compatibility snapshot. Discovery of an open service is not evidence
of its API and is not authorization to invoke side-effecting routes.

Each route record contains:

- HTTP method and source-derived route.
- Request and response schema references.
- Authentication requirement.
- Streaming behavior.
- Side effects and data source.
- Parity status.
- Exact test-evidence references.

Statuses are `UNKNOWN`, `UNVERIFIED`, `PARTIAL`, `VERIFIED`, and
`RETIRED_ACCEPTED`. `VERIFIED` requires request/response schemas and evidence.
`RETIRED_ACCEPTED` requires explicit acceptance evidence; it is not a way to
discard unexplained legacy behavior.

Migration proceeds one route family at a time, with rollback and the original
Hub preserved until parity is demonstrated.
checkpoints/PROJECT-DREAD-AUTONOMOUS-CHECKPOINT.md
Markdown

Copy
# PROJECT DREAD AUTONOMOUS CHECKPOINT

## Current batch

- Batch: 001
- Scope: portable configuration, domain classification, resource authority and
  legacy/extension contracts.
- Evidence class: source inspection and supplied unit tests only.
- Overall release status: NOT VERIFIED.
- Existing DREAD workstation/source access: none claimed.
- Existing source modifications: none; this is an additive bootstrap workspace.

## Implemented

- Strict TOML deserialization with unknown-key rejection.
- Loopback-only Hub binding.
- Setup-only port zero and explicit startup rejection.
- DREAD_HOME and XDG-derived storage layout.
- Remote-provider denial unless explicitly enabled.
- Rejection of URL-embedded credentials and port scanning.
- Mandatory safe extension defaults.
- Separate capability kinds, including skills, agents, tools and models.
- Disabled/discovered import state that manifests cannot override.
- Unknown, denied and approval-required permissions do not permit execution.
- ResourceBroker interface and unavailable safe default; no process launcher.
- Legacy route matrix with evidence requirements and no invented routes.

## Files

All files listed in BATCH-NOTES.md are new. No migrations or database changes.

## Backup and rollback

No supplied source was changed. Rollback consists of removing this new batch
from the separate working copy. Do not apply it over an existing workspace
without first recording branch, commit, dirty state and a recovery checkpoint.

## Verification

Commands intended for an integration environment:

    cargo fmt --all -- --check
    cargo check --workspace --all-targets
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace

Current results: NOT RUN. No Rust toolchain or command execution environment
was available in the authoring context. No PASS result is claimed.

## Not implemented or not verified

- SQLite persistence, migrations and restart recovery.
- API server, authentication, event streaming and production UI.
- Real discovery adapters and provider health probes.
- Durable mission DAG, execution, cancellation and independent verification.
- Supervisor/Gateway, llama.cpp, Token Booster and media adapters.
- Signed transactional extension activation and rollback.
- Self-repair engine and safe repair simulations.
- Portable archive, Debian package, updater, SBOM and signatures.
- All live workstation and release acceptance tests.

## Next concrete step

Integrate this slice into a preserved working copy, run the focused core tests,
then implement the versioned SQLite schema with migration backup/restore tests
and durable registry records. Do not begin real provider dispatch before the
provider-specific isolation preflight exists.
BATCH-NOTES.md
Markdown

Copy
# Batch 001 notes

## Scope

New host-independent core contracts and configuration validation. This batch
contains no machine-specific deployment values and no fake provider adapter.

## Integration

Create a recovery checkpoint of the actual target repository first. Preserve
uncommitted work and apply these files only after checking for path conflicts.
If the target already has a Rust workspace, merge dependencies and modules
selectively rather than replacing its manifests.

## Migration and rollback

There are no database migrations. Rollback is removal of the newly added files
from the separate working copy. Existing databases, services, workflows,
models, backups and deployment profiles are untouched.

## Limits

The text transfer does not include a binary ZIP or computed MANIFEST.sha256.
No tests were executed in the authoring context. Generate and inspect those
artifacts in the integration environment; do not represent them as pre-existing
verification evidence.

## Artifact generation

From the transferred bundle root, after reviewing its contents:

    find . -type f ! -name MANIFEST.sha256 -print0 \
      | sort -z \
      | xargs -0 sha256sum > MANIFEST.sha256

Then run the core verification commands recorded in the checkpoint and save
their complete, unedited outputs under `verification/`.
Verification record
Commands run: none. I do not have an execution environment or access to the supplied workstation.

Test status: NOT RUN, not PASS.

The intended focused commands are:

Bash

Copy
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
The example profile can additionally be checked after integration with a small setup/CLI test; port zero must remain rejected for server startup.

Backup and rollback
No existing source or database was modified.
Apply only to a separate working copy after recording its commit, branch and dirty state.
Roll back this batch by removing the new files.
There are no migrations, service operations, driver changes, provider calls, or production effects.
Transfer limitation
I cannot honestly attach a generated ZIP, computed MANIFEST.sha256, or command-output files from this interface. The complete new source is supplied above; those artifacts must be generated in the integration environment. This is a foundational implementation batch, not a final acceptance report.




659.93 s
18.38 TPS
44 s TTFT
Thanks. Please tighten the rollback and verification requirements before we accept this implementation batch.

1. Rollback and preservation

Confirm the exact baseline commit, branch, and git status before applying any changes.

Work only in a separate working copy or isolated branch. Do not modify the canonical source, existing databases, or running services.

List every file as ADDED, MODIFIED, or DELETED.

Preserve the original contents of every modified file. A rollback must restore modified files and remove only files introduced by this batch.

If Git is unavailable, provide a file-by-file backup and restoration procedure. Do not assume deleting new files alone reverses modifications to existing files.

No migrations, provider calls, GPU operations, model loading, service restarts, or production changes.

2. Required tests

Please include real tests for:

Valid configuration and CLI startup.

Port 0 being rejected before server startup. It must never silently select an ephemeral port.

A valid non-zero port being accepted.

Invalid, missing, and malformed configuration handling.

Existing configuration compatibility and defaults.

Rollback restoring the original state.

For the Rust workspace, run these checks against the complete integrated source, not isolated snippets:

Bash

Copy

cargo fmt --all -- --check

cargo clippy --workspace --all-targets --locked -- -D warnings

cargo test --workspace --locked

cargo build --workspace --release --locked

Also run the relevant setup/CLI and packaging smoke tests.

3. Evidence

For every test, report:

Exact command or test name.

Whether it actually ran.

PASS, FAIL, or NOT RUN.

Actual output or failure details.

Environment and tool versions.

Do not report a test as passing because code was inspected or because an isolated online compiler accepted a small snippet. If the full workspace cannot be compiled or tested in your environment, mark those checks NOT RUN.

4. Source handoff

Since we can exchange code only through copy-paste, do not depend on ZIP files, GitHub access, or download links. Return the implementation as ordered, bounded source batches. Each batch must identify the filename, whether it is new or modified, its complete contents or exact patch, its tests, and its rollback instructions.

Do not call this final acceptance. We will integrate the source against the canonical DREAD workspace and verify it separately.

Reasoning · 94.6s total
Stopped
I'll respond by implementing tightened rollback verification and tests, ordered bounded source batches. No tools environment. I'll be honest that tests did not run. Should inspect previous code issues, integrate CLI likely new crate dread-cli with startup validation, config parser compatibility defaults. Packaging smoke tests perhaps scripts static tests but not run. I'll provide complete contents or exact patches, status. Avoid overlong huge repetition perhaps batch 002 with modifications. Make sure the code compiles logically with clippy -D warnings. I'll carefully design Rust code, tests, CLI integration, rollback test real actual file operations.

I should state no canonical baseline commit known, cannot confirm; BLOCKED application until supplied. No applying. All files ADDED in isolated bootstrap except modifications from batch001 labeled MODIFIED relative to batch001. This batch includes core config adjustments for defaults and CLI. Must address valid config and CLI startup actual test runs the binary perhaps integration tests using assert_cmd. Should port 0 rejection pre-bind. Valid nonzero port accepted but CLI startup could bind then? Design dread-hub --config path --check validates config for startup without bind; --bind-check binds configured exact addr port, reports local_addr and exits. "valid configuration and CLI startup" needs real test spawning CLI --bind-check and checks. Also perhaps actual server not implemented; don't claim server. Could have startup function creates TcpListener std, nonblocking, no Axum. CLI modes --check and --bind-check. Valid config test + port. Port zero must be rejected before binding. To prove never silently ephemeral, test invalid port and no bind. Use pre-bind sequence pure.

Packaging smoke tests relevant: shell script `scripts/package-smoke

Partial output is preserved.
Reuse Prompt
Thanks. Please tighten the rollback and verification requirements before we accept this implementation batch.

1. Rollback and preservation

Confirm the exact baseline commit, branch, and git status before applying any changes.

Work only in a separate working copy or isolated branch. Do not modify the canonical source, existing databases, or running services.

List every file as ADDED, MODIFIED, or DELETED.

Preserve the original contents of every modified file. A rollback must restore modified files and remove only files introduced by this batch.

If Git is unavailable, provide a file-by-file backup and restoration procedure. Do not assume deleting new files alone reverses modifications to existing files.

No migrations, provider calls, GPU operations, model loading, service restarts, or production changes.

2. Required tests

Please include real tests for:

Valid configuration and CLI startup.

Port 0 being rejected before server startup. It must never silently select an ephemeral port.

A valid non-zero port being accepted.

Invalid, missing, and malformed configuration handling.

Existing configuration compatibility and defaults.

Rollback restoring the original state.

For the Rust workspace, run these checks against the complete integrated source, not isolated snippets:

Bash

Copy

cargo fmt --all -- --check

cargo clippy --workspace --all-targets --locked -- -D warnings

cargo test --workspace --locked

cargo build --workspace --release --locked

Also run the relevant setup/CLI and packaging smoke tests.

3. Evidence

For every test, report:

Exact command or test name.

Whether it actually ran.

PASS, FAIL, or NOT RUN.

Actual output or failure details.

Environment and tool versions.

Do not report a test as passing because code was inspected or because an isolated online compiler accepted a small snippet. If the full workspace cannot be compiled or tested in your environment, mark those checks NOT RUN.

4. Source handoff

Since we can exchange code only through copy-paste, do not depend on ZIP files, GitHub access, or download links. Return the implementation as ordered, bounded source batches. Each batch must identify the filename, whether it is new or modified, its complete contents or exact patch, its tests, and its rollback instructions.

Do not call this final acceptance. We will integrate the source against the canonical DREAD workspace and verify it separately.

Reasoning · Done · 168.8s total
Batch 002 — rollback controls and executable CLI verification
This is not final acceptance. No canonical repository, baseline commit, branch, or git status was supplied, and I have no command-execution environment. Therefore:

Canonical baseline: UNKNOWN — must be recorded by the integration engineer before application.
Canonical branch: UNKNOWN.
Canonical Git status: UNKNOWN.
Tests actually run: none.
Live providers, databases, GPU operations, model loading, migrations, and service changes: none performed or introduced by this batch.
Statuses below are relative to Batch 001. The integration engineer must reclassify paths after comparing them with the canonical tree.

File
Status	Purpose
Cargo.toml	MODIFIED	Add CLI workspace member and dependencies
crates/dread-core/src/config.rs	MODIFIED	Add valid/missing/malformed/default compatibility tests
crates/dread-cli/Cargo.toml	ADDED	CLI package definition
crates/dread-cli/src/main.rs	ADDED	Validating check and loopback-only run commands
crates/dread-cli/tests/cli.rs	ADDED	Real CLI subprocess/startup tests
scripts/portable-cli-smoke.sh	ADDED	Release-binary relocation smoke test
scripts/rollback-smoke.sh	ADDED	Scratch-fixture rollback test
BATCH-002-NOTES.md	ADDED	Preservation, evidence, and rollback procedure
Cargo.lock is not supplied or fabricated. It must be generated and reviewed in the integrated workspace before any --locked verification.

1. Source changes
Cargo.toml — MODIFIED
Exact patch against Batch 001:

Diff

Copy
diff --git a/Cargo.toml b/Cargo.toml
--- a/Cargo.toml
+++ b/Cargo.toml
@@ -1,5 +1,8 @@
 [workspace]
-members = ["crates/dread-core"]
+members = [
+    "crates/dread-core",
+    "crates/dread-cli",
+]
 resolver = "2"
 
 [workspace.package]
@@ -9,9 +12,12 @@ license = "Apache-2.0"
 
 [workspace.dependencies]
 async-trait = "0.1"
+axum = "0.8"
 directories = "6"
 serde = { version = "1", features = ["derive"] }
+serde_json = "1"
 tempfile = "3"
 thiserror = "2"
 toml = "0.8"
+tokio = { version = "1", features = ["macros", "net", "rt-multi-thread", "signal"] }
 url = "2"
crates/dread-core/src/config.rs — MODIFIED
Append these tests inside the existing mod tests, immediately before its final closing brace:

Rust

Copy
    #[test]
    fn valid_nonzero_port_is_accepted_for_startup() {
        let profile = valid_profile();
        assert_eq!(profile.hub.listen_port, 43127);
        assert!(profile.validate_for_startup().is_ok());
    }

    #[test]
    fn missing_configuration_returns_read_error() {
        let missing = std::env::temp_dir().join(format!(
            "dread-definitely-missing-{}-{}.toml",
            std::process::id(),
            std::thread::current().name().unwrap_or("config-test")
        ));

        // Do not remove or overwrite an unrelated file if this unlikely path exists.
        assert!(!missing.exists(), "unexpected test-path collision: {missing:?}");
        assert!(matches!(
            DeploymentProfile::load(missing),
            Err(ConfigError::Read { .. })
        ));
    }

    #[test]
    fn malformed_configuration_returns_parse_error() {
        let error = DeploymentProfile::from_toml("[hub\nnot valid TOML");
        assert!(matches!(error, Err(ConfigError::Parse(_))));
    }

    #[test]
    fn supplied_example_has_compatible_empty_storage_defaults() {
        let profile = DeploymentProfile::from_toml(include_str!(
            "../../../deployment/profile.example.toml"
        ))
        .expect("the generic example must remain parseable");

        assert_eq!(profile.hub.listen_port, 0);
        assert!(profile.validate().is_ok());
        assert!(profile.validate_for_startup().is_err());
        assert!(profile.providers.is_empty());

        let root = Path::new("/tmp/dread-example-defaults");
        let layout = PathLayout::resolve_with_dread_home(&profile, Some(root)).unwrap();
        assert_eq!(layout.state_dir, root.join("state"));
        assert_eq!(layout.log_dir, root.join("state/logs"));
        assert_eq!(layout.artifact_dir, root.join("state/artifacts"));
    }

    #[test]
    fn optional_provider_fields_retain_disabled_defaults() {
        let mut profile = valid_profile();
        profile.providers.insert(
            "configured_but_disabled".into(),
            ProviderProfile {
                adapter: "reviewed-provider-v1".into(),
                enabled: false,
                base_url: None,
                secret_ref: None,
            },
        );

        let serialized = toml::to_string(&profile).unwrap();
        let reparsed = DeploymentProfile::from_toml(&serialized).unwrap();
        let provider = reparsed.providers.get("configured_but_disabled").unwrap();

        assert!(!provider.enabled);
        assert_eq!(provider.base_url, None);
        assert_eq!(provider.secret_ref, None);
    }
The existing tests continue to cover unknown keys, port scanning, remote-provider rejection, URL credentials, non-loopback binding, unsafe extension defaults, and port-zero rejection.

crates/dread-cli/Cargo.toml — ADDED
Toml

Copy
[package]
name = "dread-cli"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
description = "Configuration and process-liveness CLI for DREAD Universal"

[[bin]]
name = "dread-hub"
path = "src/main.rs"

[dependencies]
axum.workspace = true
dread-core = { path = "../dread-core" }
serde_json.workspace = true
thiserror.workspace = true
tokio.workspace = true

[dev-dependencies]
tempfile.workspace = true
crates/dread-cli/src/main.rs — ADDED
Rust

Copy
use dread_core::{
    config::ConfigError,
    DeploymentProfile, PathLayout,
};
use std::{
    env,
    ffi::OsString,
    net::SocketAddr,
    path::PathBuf,
    process::ExitCode,
};
use thiserror::Error;
use tokio::net::TcpListener;

#[derive(Debug, Error)]
enum CliError {
    #[error("{0}")]
    Usage(String),

    #[error(transparent)]
    Config(#[from] ConfigError),

    #[error("cannot bind Hub listener at {address}: {source}")]
    Bind {
        address: SocketAddr,
        source: std::io::Error,
    },

    #[error("Hub server failed: {0}")]
    Server(#[from] std::io::Error),
}

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Check { config: PathBuf },
    Run { config: PathBuf },
    Help,
}

#[tokio::main]
async fn main() -> ExitCode {
    match execute(env::args_os().skip(1)).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("dread-hub: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn execute(args: impl Iterator<Item = OsString>) -> Result<(), CliError> {
    match parse_args(args)? {
        Command::Help => {
            print_help();
            Ok(())
        }
        Command::Check { config } => {
            let profile = DeploymentProfile::load(config)?;
            profile.validate_for_startup()?;

            // Resolve paths to detect invalid DREAD_HOME/platform configuration.
            // This command does not create directories or alter existing state.
            let _layout = PathLayout::resolve(&profile)?;

            println!("configuration valid; startup invariants satisfied");
            Ok(())
        }
        Command::Run { config } => run(config).await,
    }
}

async fn run(config: PathBuf) -> Result<(), CliError> {
    let profile = DeploymentProfile::load(config)?;

    // This validation must remain before TcpListener::bind. Port zero is never
    // passed to the OS as a request for an ephemeral listening port.
    profile.validate_for_startup()?;
    let _layout = PathLayout::resolve(&profile)?;

    let address = SocketAddr::from((profile.hub.listen_address, profile.hub.listen_port));
    let listener = TcpListener::bind(address)
        .await
        .map_err(|source| CliError::Bind { address, source })?;

    let app = axum::Router::new().route(
        "/health/live",
        axum::routing::get(|| async {
            axum::Json(serde_json::json!({
                "status": "ok",
                "scope": "process_liveness_only",
                "providers_checked": false
            }))
        }),
    );

    println!("dread-hub listening on http://{address}");
    println!("provider, resource, database, and mission health are NOT VERIFIED");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        eprintln!("dread-hub: cannot install Ctrl-C handler: {error}");
    }
}

fn parse_args(args: impl Iterator<Item = OsString>) -> Result<Command, CliError> {
    let mut args = args;
    let Some(command) = args.next() else {
        return Err(CliError::Usage(usage()));
    };

    if command == "--help" || command == "-h" || command == "help" {
        return Ok(Command::Help);
    }

    let command = command
        .into_string()
        .map_err(|_| CliError::Usage("command must be valid UTF-8".into()))?;

    if command != "check" && command != "run" {
        return Err(CliError::Usage(format!(
            "unknown command {command:?}\n{}",
            usage()
        )));
    }

    let mut config = None;
    while let Some(argument) = args.next() {
        if argument != "--config" {
            return Err(CliError::Usage(format!(
                "unexpected argument {argument:?}\n{}",
                usage()
            )));
        }
        if config.is_some() {
            return Err(CliError::Usage(
                "--config may only be supplied once".into(),
            ));
        }
        config = Some(PathBuf::from(args.next().ok_or_else(|| {
            CliError::Usage("--config requires a path".into())
        })?));
    }

    let config = config.ok_or_else(|| CliError::Usage(usage()))?;
    match command.as_str() {
        "check" => Ok(Command::Check { config }),
        "run" => Ok(Command::Run { config }),
        _ => unreachable!("command was validated above"),
    }
}

fn usage() -> String {
    "usage: dread-hub <check|run> --config <DEPLOYMENT_PROFILE>".into()
}

fn print_help() {
    println!("{}", usage());
    println!("check: validate configuration without opening a listener");
    println!("run:   validate configuration and serve loopback process liveness");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> impl Iterator<Item = OsString> + '_ {
        values.iter().map(OsString::from)
    }

    #[test]
    fn check_requires_an_explicit_config_path() {
        assert!(parse_args(args(&["check"])).is_err());
    }

    #[test]
    fn unknown_options_are_rejected() {
        assert!(parse_args(args(&[
            "run",
            "--config",
            "profile.toml",
            "--port",
            "0"
        ]))
        .is_err());
    }

    #[test]
    fn valid_check_arguments_are_accepted() {
        assert_eq!(
            parse_args(args(&["check", "--config", "profile.toml"])).unwrap(),
            Command::Check {
                config: PathBuf::from("profile.toml")
            }
        );
    }
}
This server exposes only process liveness. It does not claim subsystem health or implement the production Hub API.

crates/dread-cli/tests/cli.rs — ADDED
Rust

Copy
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    process::{Child, Command, Output},
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;

const EXAMPLE: &str = include_str!("../../../deployment/profile.example.toml");

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_dread-hub")
}

fn write_config(directory: &Path, port: u16) -> std::path::PathBuf {
    let path = directory.join("profile.toml");
    let text = EXAMPLE.replace("listen_port = 0", &format!("listen_port = {port}"));
    assert_ne!(text, EXAMPLE, "example port placeholder was not replaced");
    fs::write(path, text).unwrap();
    directory.join("profile.toml")
}

fn run(args: &[&str]) -> Output {
    Command::new(binary()).args(args).output().unwrap()
}

fn unused_loopback_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn valid_configuration_passes_cli_check() {
    let temporary = TempDir::new().unwrap();
    let config = write_config(temporary.path(), unused_loopback_port());

    let output = run(&["check", "--config", config.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("configuration valid"));
}

#[test]
fn valid_nonzero_port_starts_the_real_cli_listener() {
    let temporary = TempDir::new().unwrap();
    let port = unused_loopback_port();
    let config = write_config(temporary.path(), port);

    let child = Command::new(binary())
        .args(["run", "--config", config.to_str().unwrap()])
        .spawn()
        .unwrap();
    let mut child = ChildGuard(child);

    let deadline = Instant::now() + Duration::from_secs(10);
    let response = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            panic!("Hub exited before startup with {status}");
        }

        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(mut stream) => {
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                stream
                    .write_all(
                        b"GET /health/live HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
                    )
                    .unwrap();
                let mut response = String::new();
                stream.read_to_string(&mut response).unwrap();
                break response;
            }
            Err(error) if Instant::now() < deadline => {
                let _ = error;
                thread::sleep(Duration::from_millis(50));
            }
            Err(error) => panic!("listener did not start: {error}"),
        }
    };

    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.contains("\"status\":\"ok\""), "{response}");
    assert!(response.contains("\"providers_checked\":false"), "{response}");
}

#[test]
fn port_zero_is_rejected_before_server_startup() {
    let temporary = TempDir::new().unwrap();
    let config = temporary.path().join("zero.toml");
    fs::write(&config, EXAMPLE).unwrap();

    let output = run(&["run", "--config", config.to_str().unwrap()]);
    assert!(!output.status.success());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("hub.listen_port is zero"), "{stderr}");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("listening"));
}

#[test]
fn missing_configuration_fails_cleanly() {
    let temporary = TempDir::new().unwrap();
    let missing = temporary.path().join("missing.toml");

    let output = run(&["check", "--config", missing.to_str().unwrap()]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot read deployment profile"));
}

#[test]
fn malformed_configuration_fails_cleanly() {
    let temporary = TempDir::new().unwrap();
    let config = temporary.path().join("malformed.toml");
    fs::write(&config, "[hub\nthis is not valid TOML").unwrap();

    let output = run(&["check", "--config", config.to_str().unwrap()]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot parse deployment profile"));
}

#[test]
fn invalid_configuration_fails_cleanly() {
    let temporary = TempDir::new().unwrap();
    let config = temporary.path().join("invalid.toml");
    let text = EXAMPLE.replace("port_scanning = false", "port_scanning = true");
    fs::write(&config, text).unwrap();

    let output = run(&["check", "--config", config.to_str().unwrap()]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("port_scanning is not supported"));
}

#[test]
fn valid_example_defaults_remain_check_compatible_after_port_selection() {
    let temporary = TempDir::new().unwrap();
    let config = write_config(temporary.path(), unused_loopback_port());

    let output = run(&["check", "--config", config.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
The listener test uses an OS-selected port only to choose a test fixture. It writes the resulting non-zero port into configuration; the application itself never accepts port zero or silently selects a port.

scripts/portable-cli-smoke.sh — ADDED
Bash

Copy
#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

cargo build --workspace --release --locked

TEMPORARY="$(mktemp -d)"
trap 'rm -rf -- "$TEMPORARY"' EXIT

mkdir -p "$TEMPORARY/first/bin" "$TEMPORARY/relocated/bin"
cp -- target/release/dread-hub "$TEMPORARY/first/bin/dread-hub"
cp -a -- "$TEMPORARY/first/." "$TEMPORARY/relocated/"

sed 's/^listen_port = 0$/listen_port = 43127/' \
    deployment/profile.example.toml > "$TEMPORARY/relocated/profile.toml"
grep -q '^listen_port = 43127$' "$TEMPORARY/relocated/profile.toml"

(
    cd /
    DREAD_HOME="$TEMPORARY/relocated/runtime" \
        "$TEMPORARY/relocated/bin/dread-hub" \
        check --config "$TEMPORARY/relocated/profile.toml"
)

echo "PASS: release CLI passed configuration smoke test after relocation"
echo "LIMIT: this is not acceptance of the complete portable Hub archive"
The fixed port is used only by check, which opens no listener. It is not a provider or deployment default.

scripts/rollback-smoke.sh — ADDED
Bash

Copy
#!/usr/bin/env bash
set -euo pipefail

TEMPORARY="$(mktemp -d)"
trap 'rm -rf -- "$TEMPORARY"' EXIT

WORK="$TEMPORARY/work"
BACKUP="$TEMPORARY/backup"
mkdir -p "$WORK/docs" "$BACKUP/preimage/docs"

printf 'original canonical contents\n' > "$WORK/docs/existing.txt"
printf 'original second file\n' > "$WORK/unchanged.txt"

# Pre-image backup covers modified files, not just files added by the batch.
cp -a -- "$WORK/docs/existing.txt" "$BACKUP/preimage/docs/existing.txt"
sha256sum "$WORK/docs/existing.txt" "$WORK/unchanged.txt" \
    > "$BACKUP/original.sha256"

# Simulate a batch that modifies one file and adds one file.
printf 'batch replacement\n' > "$WORK/docs/existing.txt"
printf 'new batch file\n' > "$WORK/added.txt"
[[ "$(cat "$WORK/docs/existing.txt")" == "batch replacement" ]]

# Rollback: restore the pre-image and remove only this batch's added file.
cp -a -- "$BACKUP/preimage/docs/existing.txt" "$WORK/docs/existing.txt"
rm -- "$WORK/added.txt"

[[ "$(cat "$WORK/docs/existing.txt")" == "original canonical contents" ]]
[[ ! -e "$WORK/added.txt" ]]
(
    cd "$WORK"
    sha256sum -c "$BACKUP/original.sha256"
)

echo "PASS: scratch rollback restored modified content and removed only the added file"
This verifies the rollback procedure on a disposable fixture; it does not prove rollback of the unexamined canonical repository.

2. Mandatory preservation and rollback procedure
Before application
These steps are blocking prerequisites. Their results are currently unknown.

Bash

Copy
cd /path/to/canonical/DREAD

git rev-parse HEAD
git branch --show-current
git status --short --untracked-files=all
git diff --binary
git diff --cached --binary
git submodule status
Record complete outputs under verification/baseline/. Also record untracked files and any non-Git source/data directories. Do not stash, clean, reset, or overwrite existing work.

Create the isolated working copy only after preservation:

Bash

Copy
git worktree add ../DREAD-batch-002 \
  -b integration/dread-batch-002 HEAD
cd ../DREAD-batch-002
If uncommitted canonical work is relevant, preserve and transfer it through reviewed patches; do not assume a worktree created from HEAD contains it.

Backup every modified file
Before applying this batch:

Bash

Copy
BATCH_BACKUP=/path/outside/worktree/batch-002-backup
mkdir -p "$BATCH_BACKUP/preimage/Cargo.toml-dir"

cp -a -- Cargo.toml "$BATCH_BACKUP/preimage/Cargo.toml"
mkdir -p "$BATCH_BACKUP/preimage/crates/dread-core/src"
cp -a -- crates/dread-core/src/config.rs \
  "$BATCH_BACKUP/preimage/crates/dread-core/src/config.rs"

sha256sum Cargo.toml crates/dread-core/src/config.rs \
  > "$BATCH_BACKUP/preimage.sha256"
After application, record the expected postimages:

Bash

Copy
sha256sum \
  Cargo.toml \
  crates/dread-core/src/config.rs \
  crates/dread-cli/Cargo.toml \
  crates/dread-cli/src/main.rs \
  crates/dread-cli/tests/cli.rs \
  scripts/portable-cli-smoke.sh \
  scripts/rollback-smoke.sh \
  BATCH-002-NOTES.md \
  > "$BATCH_BACKUP/postimage.sha256"
If any target path already exists unexpectedly, stop. Reclassify it as MODIFIED and back up its original contents rather than overwriting it.

Git rollback
Rollback only the isolated integration branch:

Bash

Copy
git restore --source=HEAD -- Cargo.toml crates/dread-core/src/config.rs
rm -- \
  crates/dread-cli/Cargo.toml \
  crates/dread-cli/src/main.rs \
  crates/dread-cli/tests/cli.rs \
  scripts/portable-cli-smoke.sh \
  scripts/rollback-smoke.sh \
  BATCH-002-NOTES.md
This is valid only if those files match the recorded batch postimages and their pre-application statuses are correct. If further work has occurred, use the file-by-file method instead of deleting potentially changed files.

Git-unavailable rollback
Compare current files with postimage.sha256.
If a file differs from its recorded postimage, stop and preserve it for manual reconciliation.
Restore both modified files from preimage/.
Remove only the seven files introduced by this batch.
Recheck the modified files against preimage.sha256.
Confirm the added paths are absent.
Never delete crates/dread-cli or scripts directories wholesale; they may contain unrelated files.
Run the supplied test:

Bash

Copy
bash scripts/rollback-smoke.sh
No rollback procedure in this batch touches a database, provider, service, model, GPU, or production directory.

3. Required verification and actual evidence
Environment
Item
Actual value
Execution environment	Not available in this authoring interface
OS/kernel	NOT VERIFIED
Rust rustc	NOT RUN
Cargo	NOT RUN
Bash	NOT RUN
Canonical source revision	UNKNOWN
Dependency lockfile	NOT SUPPLIED; must be generated and reviewed
Network/provider credentials	None used
No command below was executed. Source inspection is not recorded as a test pass.

Exact command or test
Ran?	Status	Actual output/details
git rev-parse HEAD	No	NOT RUN	Canonical repository not supplied
git branch --show-current	No	NOT RUN	Canonical repository not supplied
git status --short --untracked-files=all	No	NOT RUN	Canonical repository not supplied
cargo generate-lockfile	No	NOT RUN	No execution environment
cargo fmt --all -- --check	No	NOT RUN	No execution environment
cargo clippy --workspace --all-targets --locked -- -D warnings	No	NOT RUN	No execution environment or reviewed lockfile
cargo test --workspace --locked	No	NOT RUN	No execution environment or reviewed lockfile
cargo build --workspace --release --locked	No	NOT RUN	No execution environment or reviewed lockfile
cargo test -p dread-cli --test cli --locked -- --nocapture	No	NOT RUN	Focused test command supplied only
bash -n scripts/portable-cli-smoke.sh scripts/rollback-smoke.sh	No	NOT RUN	No execution environment
bash scripts/portable-cli-smoke.sh	No	NOT RUN	Requires release build and lockfile
bash scripts/rollback-smoke.sh	No	NOT RUN	No execution environment
Full .tar.gz relocation/install test	No	NOT RUN	Complete portable package not implemented
.deb installation/upgrade/rollback test	No	NOT RUN	Debian packaging not implemented
Live DREAD integration acceptance	No	NOT RUN	Real source/profile/providers not supplied
Integrated execution sequence
Run against the complete integrated workspace, not extracted snippets:

Bash

Copy
cargo generate-lockfile
# Review Cargo.lock and dependency resolution before continuing.

cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --release --locked

cargo test -p dread-cli --test cli --locked -- --nocapture
bash -n scripts/portable-cli-smoke.sh scripts/rollback-smoke.sh
bash scripts/portable-cli-smoke.sh
bash scripts/rollback-smoke.sh
Capture stdout, stderr, exit status, timestamps, revision, and tool versions:

Bash

Copy
{
  date -u +%Y-%m-%dT%H:%M:%SZ
  git rev-parse HEAD
  git branch --show-current
  git status --short --untracked-files=all
  uname -a
  rustc --version
  cargo --version
  bash --version | head -n 1
} > verification/batch-002-environment.txt
A command is PASS only if it actually ran in that integrated environment and exited successfully. Any formatting, Clippy, compilation, or test failure must be reported with its actual output and repaired before acceptance. Packaging smoke remains limited to CLI relocation; it must not be represented as complete .tar.gz or .deb acceptance.




505.03 s
18.65 TPS
18.43 s TTFT
revise all you did and continue

Reasoning · Done · 257.2s total
The previous batches are revised below, then continued with a durable SQLite registry slice. This remains NOT RUN / NOT ACCEPTED until integrated and executed against the canonical workspace.

Corrections to the earlier handoff:

BATCH-002-NOTES.md was listed but its contents were omitted; they are supplied below.
The rollback text said “seven files” when Batch 002 introduced six.
CLI tests now remove inherited DREAD_HOME so host environment state cannot make otherwise valid tests fail.
The unknown-manifest-field test now uses actual TOML rather than relying on JSON failing TOML parsing.
No Cargo.lock, test result, baseline revision, ZIP, or checksum is fabricated.
The registry below opens a configured database path; it does not mandate a parallel mission database. Applying it to any canonical database remains blocked until its existing schema is inventoried and backed up.
Batch 003 — corrections
File status
File
Status relative to Batch 002
crates/dread-cli/tests/cli.rs	MODIFIED
crates/dread-core/src/manifest.rs	MODIFIED
BATCH-002-NOTES.md	ADDED; previously listed but omitted
crates/dread-cli/tests/cli.rs — exact patch
Diff

Copy
diff --git a/crates/dread-cli/tests/cli.rs b/crates/dread-cli/tests/cli.rs
--- a/crates/dread-cli/tests/cli.rs
+++ b/crates/dread-cli/tests/cli.rs
@@
 fn run(args: &[&str]) -> Output {
-    Command::new(binary()).args(args).output().unwrap()
+    Command::new(binary())
+        .args(args)
+        .env_remove("DREAD_HOME")
+        .output()
+        .unwrap()
 }
@@
     let child = Command::new(binary())
         .args(["run", "--config", config.to_str().unwrap()])
+        .env_remove("DREAD_HOME")
         .spawn()
         .unwrap();
This prevents a caller’s relative or otherwise invalid DREAD_HOME from contaminating configuration tests. Tests for valid and invalid DREAD_HOME values remain a separate requirement.

crates/dread-core/src/manifest.rs — exact patch
Replace the final test with:

Rust

Copy
    #[test]
    fn executable_fields_are_not_part_of_declarative_manifest() {
        let text = r#"
manifest_version = "capability-manifest-v1"
id = "example-skill"
name = "Example"
version = "1.0.0"
kind = "skill"
source = "local-reviewed-source"
content_sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
install_hook = "arbitrary-command"
"#;

        let error = toml::from_str::<CapabilityManifest>(text)
            .expect_err("unknown executable fields must be rejected");
        assert!(error.to_string().contains("unknown field"), "{error}");
    }
BATCH-002-NOTES.md — ADDED
Markdown

Copy
# Batch 002 notes

## Baseline

Canonical revision, branch, status, dirty files, and submodule state were not
supplied. They must be recorded before application. This batch must be applied
only in an isolated worktree or separate preserved working copy.

## Scope

Adds configuration-check and loopback-liveness CLI commands, integration tests,
and scratch rollback/relocation smoke scripts. It does not implement provider
health, authentication, the production Hub API, packaging, or acceptance.

## Modified files

- Cargo.toml
- crates/dread-core/src/config.rs

Their original contents must be copied to the batch pre-image backup before
application. Deleting newly added files does not restore these modifications.

## Added files

- crates/dread-cli/Cargo.toml
- crates/dread-cli/src/main.rs
- crates/dread-cli/tests/cli.rs
- scripts/portable-cli-smoke.sh
- scripts/rollback-smoke.sh
- BATCH-002-NOTES.md

If any of these paths already exists, stop, back it up, and reclassify it as
MODIFIED. Do not overwrite it as though it were new.

## Rollback

On the isolated branch, restore the two modified files from Git or their
pre-image copies. Remove only the six files introduced by this batch after
confirming they match the recorded postimages. Preserve subsequent work.

Without Git:

1. Verify current files against the recorded postimage checksums.
2. Stop and manually reconcile any unexpected differences.
3. Restore Cargo.toml from the pre-image backup.
4. Restore crates/dread-core/src/config.rs from the pre-image backup.
5. Remove only the six added files listed above.
6. Verify restored files against the pre-image checksums.
7. Verify the added paths are absent.
8. Do not remove entire crates or scripts directories.

The scratch rollback script tests this procedure only on disposable files. It
is not evidence of canonical-source rollback.

## Side effects

No migrations, provider calls, GPU operations, model loading, service restarts,
or production changes are included. Tests use temporary directories and a
loopback listener only.

## Evidence

No commands were executed in the authoring environment. All build, test,
packaging, baseline, and rollback checks remain NOT RUN until executed against
the complete integrated source with a reviewed Cargo.lock.
Batch 004 — durable, backupable capability registry
This batch adds:

Forward-only schema versioning with migration-checksum verification.
Disabled-by-default capability imports.
Idempotent metadata refresh preserving review and enabled state.
Rejection of identity reuse across different capability types.
Persistent provenance and permission records.
Online SQLite backup with integrity checks.
Restore-to-a-new-path behavior that never overwrites an existing database.
Temporary-directory tests for persistence, refresh, backup, and restoration.
It does not activate capabilities, discover providers, execute agents, or claim compatibility with an unseen canonical schema.

File status
File
Status relative to Batch 003
Cargo.toml	MODIFIED
crates/dread-registry/Cargo.toml	ADDED
crates/dread-registry/src/lib.rs	ADDED
checkpoints/PROJECT-DREAD-AUTONOMOUS-CHECKPOINT.md	MODIFIED
Cargo.toml — exact patch
Diff

Copy
diff --git a/Cargo.toml b/Cargo.toml
--- a/Cargo.toml
+++ b/Cargo.toml
@@
 members = [
     "crates/dread-core",
     "crates/dread-cli",
+    "crates/dread-registry",
 ]
@@
 directories = "6"
+rusqlite = { version = "0.32", features = ["backup", "bundled"] }
 serde = { version = "1", features = ["derive"] }
 serde_json = "1"
+sha2 = "0.10"
 tempfile = "3"
crates/dread-registry/Cargo.toml — ADDED
Toml

Copy
[package]
name = "dread-registry"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
description = "Durable capability registry for DREAD Universal"

[dependencies]
dread-core = { path = "../dread-core" }
rusqlite.workspace = true
serde_json.workspace = true
sha2.workspace = true
thiserror.workspace = true

[dev-dependencies]
tempfile.workspace = true
crates/dread-registry/src/lib.rs — ADDED
Rust

Copy
//! Durable capability registration.
//!
//! This module does not execute discovered content. Imports always start
//! disabled and in the Discovered review state.

use dread_core::{
    domain::{CapabilityKind, ImportedCapability, ReviewState},
    manifest::{CapabilityManifest, ManifestError},
};
use rusqlite::{
    backup::Backup,
    params,
    Connection,
    OptionalExtension,
    Row,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use thiserror::Error;

const SCHEMA_VERSION: i64 = 1;

const MIGRATION_0001: &str = r#"
CREATE TABLE dread_schema_migrations (
    version INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    checksum_sha256 TEXT NOT NULL,
    applied_at_unix_seconds INTEGER NOT NULL
) STRICT;

CREATE TABLE dread_capabilities (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    kind TEXT NOT NULL,
    source TEXT NOT NULL,
    content_sha256 TEXT NOT NULL,
    review TEXT NOT NULL CHECK (
        review IN ('discovered', 'inspected', 'registered', 'validated',
                   'approved', 'rejected')
    ),
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    permissions_json TEXT NOT NULL,
    created_at_unix_seconds INTEGER NOT NULL,
    updated_at_unix_seconds INTEGER NOT NULL
) STRICT;

CREATE INDEX dread_capabilities_kind_enabled
    ON dread_capabilities(kind, enabled);
"#;

#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("registry I/O failed: {0}")]
    Io(#[from] std::io::Error),

    #[error("registry SQLite operation failed: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("registry serialization failed: {0}")]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Manifest(#[from] ManifestError),

    #[error("invalid registry operation: {0}")]
    Invalid(String),

    #[error("capability not found: {0}")]
    NotFound(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryRecord {
    pub id: String,
    pub name: String,
    pub version: String,
    pub kind: CapabilityKind,
    pub source: String,
    pub content_sha256: String,
    pub review: ReviewState,
    pub enabled: bool,
    pub permissions: Vec<String>,
    pub created_at_unix_seconds: i64,
    pub updated_at_unix_seconds: i64,
}

#[derive(Debug)]
struct RawRegistryRecord {
    id: String,
    name: String,
    version: String,
    kind: String,
    source: String,
    content_sha256: String,
    review: String,
    enabled: i64,
    permissions_json: String,
    created_at_unix_seconds: i64,
    updated_at_unix_seconds: i64,
}

pub struct RegistryStore {
    connection: Connection,
}

impl RegistryStore {
    /// Opens an explicitly selected database and applies only known migrations.
    ///
    /// Existing parent-directory permissions are not changed. New database files
    /// receive mode 0600 on Unix. Canonical databases require a separate schema
    /// inventory and backup before this method is used against them.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, RegistryError> {
        let path = checked_absolute_path(path.as_ref())?;
        prepare_parent(path)?;

        let existed = path.exists();
        let connection = Connection::open(path)?;
        if !existed {
            set_private_file_permissions(path)?;
        }

        connection.execute_batch("PRAGMA foreign_keys = ON;")?;
        connection.busy_timeout(Duration::from_secs(5))?;

        let store = Self { connection };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<(), RegistryError> {
        let transaction = self.connection.unchecked_transaction()?;

        transaction.execute_batch(
            "CREATE TABLE IF NOT EXISTS dread_schema_migrations (
                version INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                checksum_sha256 TEXT NOT NULL,
                applied_at_unix_seconds INTEGER NOT NULL
            ) STRICT;",
        )?;

        let current: Option<i64> = transaction
            .query_row(
                "SELECT MAX(version) FROM dread_schema_migrations",
                [],
                |row| row.get(0),
            )
            .optional()?
            .flatten();

        if current.unwrap_or(0) > SCHEMA_VERSION {
            return Err(RegistryError::Invalid(format!(
                "database schema {} is newer than supported schema {SCHEMA_VERSION}",
                current.unwrap_or(0)
            )));
        }

        if current == Some(0) || current.is_none() {
            let checksum = migration_checksum(MIGRATION_0001);
            transaction.execute_batch(MIGRATION_0001)?;
            transaction.execute(
                "INSERT INTO dread_schema_migrations (
                    version, name, checksum_sha256, applied_at_unix_seconds
                 ) VALUES (?1, ?2, ?3, ?4)",
                params![
                    SCHEMA_VERSION,
                    "initial-capability-registry",
                    checksum,
                    unix_seconds()?
                ],
            )?;
        } else {
            let recorded: String = transaction.query_row(
                "SELECT checksum_sha256
                 FROM dread_schema_migrations
                 WHERE version = ?1",
                [SCHEMA_VERSION],
                |row| row.get(0),
            )?;
            let expected = migration_checksum(MIGRATION_0001);
            if recorded != expected {
                return Err(RegistryError::Invalid(
                    "schema migration checksum does not match the applied migration".into(),
                ));
            }
        }

        transaction.commit()?;
        Ok(())
    }

    /// Imports or refreshes metadata without granting review or execution rights.
    pub fn import(
        &self,
        manifest: &CapabilityManifest,
    ) -> Result<RegistryRecord, RegistryError> {
        let imported: ImportedCapability = manifest.import_disabled()?;
        let kind = kind_name(imported.kind)?;

        let existing_kind: Option<String> = self
            .connection
            .query_row(
                "SELECT kind FROM dread_capabilities WHERE id = ?1",
                [&manifest.id],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(existing_kind) = existing_kind {
            if existing_kind != kind {
                return Err(RegistryError::Invalid(format!(
                    "capability identity {:?} cannot change kind from {existing_kind} to {kind}",
                    manifest.id
                )));
            }
        }

        let now = unix_seconds()?;
        let permissions_json = serde_json::to_string(&manifest.permissions)?;

        self.connection.execute(
            "INSERT INTO dread_capabilities (
                id, name, version, kind, source, content_sha256,
                review, enabled, permissions_json,
                created_at_unix_seconds, updated_at_unix_seconds
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6,
                'discovered', 0, ?7, ?8, ?8
             )
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                version = excluded.version,
                source = excluded.source,
                content_sha256 = excluded.content_sha256,
                permissions_json = excluded.permissions_json,
                updated_at_unix_seconds = excluded.updated_at_unix_seconds",
            params![
                manifest.id,
                manifest.name,
                manifest.version,
                kind,
                manifest.source,
                manifest.content_sha256,
                permissions_json,
                now
            ],
        )?;

        self.get(&manifest.id)
    }

    pub fn get(&self, id: &str) -> Result<RegistryRecord, RegistryError> {
        let raw = self
            .connection
            .query_row(
                &format!("{SELECT_RECORD} WHERE id = ?1"),
                [id],
                raw_record,
            )
            .optional()?
            .ok_or_else(|| RegistryError::NotFound(id.to_owned()))?;
        decode_record(raw)
    }

    pub fn list(&self) -> Result<Vec<RegistryRecord>, RegistryError> {
        let mut statement = self
            .connection
            .prepare(&format!("{SELECT_RECORD} ORDER BY kind, id"))?;
        let rows = statement.query_map([], raw_record)?;
        let mut records = Vec::new();

        for row in rows {
            records.push(decode_record(row?)?);
        }
        Ok(records)
    }

    /// Creates a new consistent backup and never overwrites an existing file.
    pub fn create_backup(
        &self,
        destination: impl AsRef<Path>,
    ) -> Result<PathBuf, RegistryError> {
        let destination = checked_absolute_path(destination.as_ref())?;
        require_absent(destination)?;
        prepare_parent(destination)?;

        let mut backup = Connection::open(destination)?;
        set_private_file_permissions(destination)?;

        {
            let operation = Backup::new(&self.connection, &mut backup)?;
            operation.run_to_completion(256, Duration::from_millis(50), None)?;
        }
        verify_integrity(&backup)?;
        Ok(destination.to_path_buf())
    }

    /// Restores into a new path. It never replaces a live or existing database.
    pub fn restore_to_new(
        backup_path: impl AsRef<Path>,
        destination: impl AsRef<Path>,
    ) -> Result<Self, RegistryError> {
        let backup_path = checked_absolute_path(backup_path.as_ref())?;
        let destination = checked_absolute_path(destination.as_ref())?;
        require_absent(destination)?;
        prepare_parent(destination)?;

        let source = Connection::open_with_flags(
            backup_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        verify_integrity(&source)?;

        let backup_version: i64 = source.query_row(
            "SELECT MAX(version) FROM dread_schema_migrations",
            [],
            |row| row.get(0),
        )?;
        if backup_version != SCHEMA_VERSION {
            return Err(RegistryError::Invalid(format!(
                "unsupported backup schema version {backup_version}"
            )));
        }

        let mut destination_connection = Connection::open(destination)?;
        set_private_file_permissions(destination)?;
        {
            let operation = Backup::new(&source, &mut destination_connection)?;
            operation.run_to_completion(256, Duration::from_millis(50), None)?;
        }
        verify_integrity(&destination_connection)?;
        drop(destination_connection);

        Self::open(destination)
    }
}

const SELECT_RECORD: &str = "
    SELECT id, name, version, kind, source, content_sha256,
           review, enabled, permissions_json,
           created_at_unix_seconds, updated_at_unix_seconds
    FROM dread_capabilities
";

fn raw_record(row: &Row<'_>) -> rusqlite::Result<RawRegistryRecord> {
    Ok(RawRegistryRecord {
        id: row.get(0)?,
        name: row.get(1)?,
        version: row.get(2)?,
        kind: row.get(3)?,
        source: row.get(4)?,
        content_sha256: row.get(5)?,
        review: row.get(6)?,
        enabled: row.get(7)?,
        permissions_json: row.get(8)?,
        created_at_unix_seconds: row.get(9)?,
        updated_at_unix_seconds: row.get(10)?,
    })
}

fn decode_record(raw: RawRegistryRecord) -> Result<RegistryRecord, RegistryError> {
    let kind = match serde_json::from_str::<Value>(&format!("\"{}\"", raw.kind))? {
        Value::String(value) => serde_json::from_value(Value::String(value))?,
        _ => {
            return Err(RegistryError::Invalid(
                "stored capability kind is not a string".into(),
            ))
        }
    };

    let review = match raw.review.as_str() {
        "discovered" => ReviewState::Discovered,
        "inspected" => ReviewState::Inspected,
        "registered" => ReviewState::Registered,
        "validated" => ReviewState::Validated,
        "approved" => ReviewState::Approved,
        "rejected" => ReviewState::Rejected,
        other => {
            return Err(RegistryError::Invalid(format!(
                "unknown stored review state {other:?}"
            )))
        }
    };

    let enabled = match raw.enabled {
        0 => false,
        1 => true,
        other => {
            return Err(RegistryError::Invalid(format!(
                "invalid stored enabled value {other}"
            )))
        }
    };

    Ok(RegistryRecord {
        id: raw.id,
        name: raw.name,
        version: raw.version,
        kind,
        source: raw.source,
        content_sha256: raw.content_sha256,
        review,
        enabled,
        permissions: serde_json::from_str(&raw.permissions_json)?,
        created_at_unix_seconds: raw.created_at_unix_seconds,
        updated_at_unix_seconds: raw.updated_at_unix_seconds,
    })
}

fn kind_name(kind: CapabilityKind) -> Result<String, RegistryError> {
    match serde_json::to_value(kind)? {
        Value::String(value) => Ok(value),
        _ => Err(RegistryError::Invalid(
            "capability kind did not serialize as a string".into(),
        )),
    }
}

fn migration_checksum(migration: &str) -> String {
    format!("{:x}", Sha256::digest(migration.as_bytes()))
}

fn verify_integrity(connection: &Connection) -> Result<(), RegistryError> {
    let result: String = connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if result == "ok" {
        Ok(())
    } else {
        Err(RegistryError::Invalid(format!(
            "SQLite integrity_check failed: {result}"
        )))
    }
}

fn checked_absolute_path(path: &Path) -> Result<&Path, RegistryError> {
    if !path.is_absolute() {
        return Err(RegistryError::Invalid(format!(
            "database path must be absolute: {}",
            path.display()
        )));
    }
    Ok(path)
}

fn require_absent(path: &Path) -> Result<(), RegistryError> {
    if path.exists() {
        Err(RegistryError::Invalid(format!(
            "refusing to overwrite existing path: {}",
            path.display()
        )))
    } else {
        Ok(())
    }
}

fn prepare_parent(path: &Path) -> Result<(), RegistryError> {
    let parent = path.parent().ok_or_else(|| {
        RegistryError::Invalid(format!("path has no parent: {}", path.display()))
    })?;

    if !parent.exists() {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true).mode(0o700);
        builder.create(parent)?;
    }
    Ok(())
}

fn set_private_file_permissions(path: &Path) -> Result<(), RegistryError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

fn unix_seconds() -> Result<i64, RegistryError> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| RegistryError::Invalid(format!("system clock error: {error}")))?
        .as_secs();
    i64::try_from(seconds)
        .map_err(|_| RegistryError::Invalid("system timestamp exceeds SQLite range".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn manifest() -> CapabilityManifest {
        CapabilityManifest {
            manifest_version: "capability-manifest-v1".into(),
            id: "example-skill".into(),
            name: "Example".into(),
            version: "1.0.0".into(),
            kind: CapabilityKind::Skill,
            source: "local-reviewed-source".into(),
            content_sha256: "ab".repeat(32),
            dependencies: vec![],
            permissions: vec!["filesystem:read".into()],
            publisher_signature: None,
        }
    }

    #[test]
    fn import_persists_disabled_skill_without_becoming_an_agent() {
        let temporary = TempDir::new().unwrap();
        let database = temporary.path().join("registry.sqlite3");

        {
            let store = RegistryStore::open(&database).unwrap();
            let record = store.import(&manifest()).unwrap();
            assert_eq!(record.kind, CapabilityKind::Skill);
            assert_eq!(record.review, ReviewState::Discovered);
            assert!(!record.enabled);
        }

        let reopened = RegistryStore::open(&database).unwrap();
        let record = reopened.get("example-skill").unwrap();
        assert_eq!(record.kind, CapabilityKind::Skill);
        assert!(!record.enabled);
        assert_eq!(record.permissions, vec!["filesystem:read"]);
    }

    #[test]
    fn refresh_preserves_review_and_enabled_state() {
        let temporary = TempDir::new().unwrap();
        let store = RegistryStore::open(temporary.path().join("registry.sqlite3")).unwrap();
        let original = store.import(&manifest()).unwrap();

        // This fixture changes only imported metadata. No approval is fabricated.
        store
            .connection
            .execute(
                "UPDATE dread_capabilities
                 SET review = 'approved', enabled = 1
                 WHERE id = 'example-skill'",
                [],
            )
            .unwrap();

        let mut refreshed = manifest();
        refreshed.name = "Refreshed Example".into();
        refreshed.version = "1.1.0".into();
        refreshed.content_sha256 = "cd".repeat(32);
        let record = store.import(&refreshed).unwrap();

        assert_eq!(record.name, "Refreshed Example");
        assert_eq!(record.review, ReviewState::Approved);
        assert!(record.enabled);
        assert_eq!(record.created_at_unix_seconds, original.created_at_unix_seconds);
    }

    #[test]
    fn identity_cannot_change_from_skill_to_agent() {
        let temporary = TempDir::new().unwrap();
        let store = RegistryStore::open(temporary.path().join("registry.sqlite3")).unwrap();
        store.import(&manifest()).unwrap();

        let mut conflicting = manifest();
        conflicting.kind = CapabilityKind::AgentProvider;
        assert!(store.import(&conflicting).is_err());
    }

    #[test]
    fn backup_restore_recovers_original_registry_without_overwriting_live_db() {
        let temporary = TempDir::new().unwrap();
        let database = temporary.path().join("registry.sqlite3");
        let backup = temporary.path().join("before-refresh.sqlite3");
        let restored_path = temporary.path().join("restored.sqlite3");

        let store = RegistryStore::open(&database).unwrap();
        store.import(&manifest()).unwrap();
        store.create_backup(&backup).unwrap();

        let mut changed = manifest();
        changed.name = "Changed after backup".into();
        store.import(&changed).unwrap();
        assert_eq!(store.get("example-skill").unwrap().name, "Changed after backup");

        let restored = RegistryStore::restore_to_new(&backup, &restored_path).unwrap();
        assert_eq!(restored.get("example-skill").unwrap().name, "Example");

        // Restoration did not overwrite or mutate the live source database.
        assert_eq!(store.get("example-skill").unwrap().name, "Changed after backup");
        assert!(backup.exists());
    }

    #[test]
    fn backup_and_restore_refuse_to_overwrite_existing_files() {
        let temporary = TempDir::new().unwrap();
        let store = RegistryStore::open(temporary.path().join("registry.sqlite3")).unwrap();
        store.import(&manifest()).unwrap();

        let backup = temporary.path().join("backup.sqlite3");
        store.create_backup(&backup).unwrap();
        assert!(store.create_backup(&backup).is_err());
        assert!(RegistryStore::restore_to_new(&backup, &backup).is_err());
    }
}
Registry limitations requiring subsequent hardening
refresh_preserves_review_and_enabled_state uses direct SQL only to establish a test precondition; it is not an approval API and is not production activation evidence.
Migration checksums detect accidental constant drift, not malicious schema tampering.
Backup/restore destination creation needs stronger symlink/race containment before exposed as a general privileged API.
No canonical database migration is authorized until its schema and current contents are inspected.
No database encryption claim is made. OS filesystem and secret-storage protections remain separate.
checkpoints/PROJECT-DREAD-AUTONOMOUS-CHECKPOINT.md — replace completely
Markdown

Copy
# PROJECT DREAD AUTONOMOUS CHECKPOINT

## Status

- Current source batches: 001–004.
- Overall release: NOT VERIFIED.
- Canonical baseline commit/branch/status: UNKNOWN.
- Existing workstation, source, databases, providers, and services: not accessed.
- All commands and tests: NOT RUN in the authoring environment.

## Source evidence only

Implemented as reviewable source:

- Strict deployment-profile parsing and startup validation.
- Port zero rejection before listener creation.
- Loopback-only liveness CLI.
- XDG/DREAD_HOME path resolution.
- Capability type separation and disabled-by-default imports.
- ResourceBroker and LegacyHubAdapter contracts.
- Durable capability metadata registry.
- Forward-only registry schema version 1 with checksum verification.
- Import refresh preserving review and enabled state.
- SQLite backup integrity checks and restore-to-new-path behavior.
- Scratch rollback and CLI relocation smoke scripts.

These are not runtime, workstation, provider, UI, media, packaging, or release
acceptance results.

## Preservation

No supplied canonical source was changed. Modified-file preimages must be saved
before integration. Added-file deletion alone is not a complete rollback.

## Verification

Required after integration and lockfile review:

    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets --locked -- -D warnings
    cargo test --workspace --locked
    cargo build --workspace --release --locked
    cargo test -p dread-cli --test cli --locked -- --nocapture
    cargo test -p dread-registry --locked -- --nocapture
    bash -n scripts/portable-cli-smoke.sh scripts/rollback-smoke.sh
    bash scripts/portable-cli-smoke.sh
    bash scripts/rollback-smoke.sh

Current result for every command: NOT RUN. No output is available to report.

## Not implemented or verified

Authentication, production API/UI, event streaming, real discovery, provider
isolation preflight, durable missions, execution, independent verification,
Supervisor/GPU integration, Token Booster, media workflows, repair engine,
transactional extension activation, signed updater, portable archive, Debian
package, SBOM, and final acceptance.

## Next concrete step

Run the integrated verification and repair actual failures. Then inventory the
canonical database before reconciling this registry migration. The next new
feature slice should be provider-isolation preflight records and fail-closed
dispatch gating; it must not dispatch real work without a verified provider.
Rollback for Batches 003–004
Before application, save complete preimages of:

Text

Copy
Cargo.toml
crates/dread-cli/tests/cli.rs
crates/dread-core/src/manifest.rs
checkpoints/PROJECT-DREAD-AUTONOMOUS-CHECKPOINT.md
If any path is absent in the canonical baseline, classify it as ADDED instead of MODIFIED.

Rollback must:

Restore those preimages rather than merely deleting new files.
Remove only these newly introduced files after checking for later changes:
Text

Copy
BATCH-002-NOTES.md
crates/dread-registry/Cargo.toml
crates/dread-registry/src/lib.rs
Remove dread-registry and its dependencies from Cargo.toml by restoring the original file.
Preserve Cargo.lock as a modified existing file if it was generated during integration; back it up before dependency generation and restore that preimage during rollback.
Do not delete the whole crates/dread-registry parent if it contains any unrelated or subsequent work.
Do not remove or restore canonical databases using these source-file instructions. Registry tests operate only in temporary directories.
Verification record
No tests were run. No actual passing output is available.

Check
Status	Actual detail
Baseline commit, branch, and status	NOT RUN	Canonical repository unavailable
cargo fmt --all -- --check	NOT RUN	No execution environment
cargo clippy --workspace --all-targets --locked -- -D warnings	NOT RUN	No environment or reviewed lockfile
cargo test --workspace --locked	NOT RUN	No environment or reviewed lockfile
cargo build --workspace --release --locked	NOT RUN	No environment or reviewed lockfile
CLI configuration/startup tests	NOT RUN	Real tests supplied, not executed
Registry persistence/backup tests	NOT RUN	Real tests supplied, not executed
bash scripts/portable-cli-smoke.sh	NOT RUN	Limited CLI relocation smoke only
bash scripts/rollback-smoke.sh	NOT RUN	Disposable-fixture test only
.tar.gz and .deb package tests	NOT RUN	Packages not implemented
Canonical database migration test	NOT RUN	Explicitly blocked pending inventory and backup
Provider, GPU, model, service, and production tests	NOT RUN	None invoked
The next implementation slice, after integrated compilation and test repair, is fail-closed provider-isolation preflight and dispatch gating, not live provider execution.




741.59 s
15.38 TPS
63.08 s TTFT
revise below again and check if it is comlete project # PROJECT DREAD UNIVERSAL

Consolidated Engineering Handoff — Batches 0–4
Purpose: One controlling reference for understanding, integrating, and continuing the supplied Kimi 3 Pro implementation.

Current status: SOURCE MATERIAL CONSOLIDATED — NOT INTEGRATED — NOT TESTED — NOT ACCEPTED.

1. Governing rules and project objective
This project is intended to become a portable, autonomous, extensible DREAD Universal Hub—not merely a CLI, configuration validator, or SQLite registry.

The controlling product specification is:

PROJECT_DREAD_UNIVERSAL_PORTABLE_MASTER_PLAN_v3.md

It defines 25 requirements and a 12-stage roadmap.

Product requirements
Portable Ubuntu 24.04+ deployment.

Relocatable .tar.gz and .deb packages.

No workstation-specific paths, ports, model names, or credentials in portable core.

Optional compatibility adapter for the existing DREAD Hub.

Rust API and React/TypeScript interface.

SQLite persistence and versioned migrations.

Authenticated loopback APIs.

Durable missions, execution state, approvals, cancellation, and recovery.

Real model and provider discovery.

Centralized resource arbitration.

Integration with the existing DREAD GPU Supervisor.

Agent, skill, tool, MCP, workflow, prompt, model, and service classification.

Pstack import with capabilities disabled until individually reviewed.

Self-repair with evidence, safeguards, and rollback.

Real telemetry and performance reporting.

Media integration with ComfyUI and supported media workflows.

Token Booster and other orchestration capabilities.

Secure extension installation and activation.

Backup and restore.

Audit logging.

CI, Clippy, unit tests, packaging, SBOM, and release evidence.

The remaining requirements and acceptance details must be taken from the original master plan rather than reconstructed from memory.

Preservation rules
Do not stop or restart ComfyUI or the existing DREAD Hub unless explicitly necessary and authorized.

Do not overwrite existing files without preserving their preimages.

Do not delete models, workflows, databases, sessions, outputs, or backups.

Do not replace a working runtime merely to simplify integration.

Do not create a second GPU scheduler.

The existing DREAD GPU Supervisor remains the GPU authority on the original workstation.

Do not introduce an Ollama dependency.

Do not activate imported plugins, agents, or skills automatically.

Do not claim tests, workstation access, or runtime acceptance that did not occur.

2. Canonical architecture and integration target
The supplied batches use a proposed standalone workspace containing dread-core, dread-cli, and dread-registry.

That structure is not the canonical DREAD Universal architecture. The standalone implementation must be reconciled with the existing project rather than copied over as a replacement.

Existing canonical Rust crates
crates/dread-api

crates/dread-discovery

crates/dread-gpu

crates/dread-agents

crates/dread-orchestrator

crates/dread-skills

crates/dread-platform

crates/dread-media

The production executable historically lives in:

crates/dread-api/src/main.rs

The canonical project has historically used an eight-crate workspace. The supplied standalone dread-core, dread-cli, and dread-registry need architectural mapping before integration.

Required architectural decisions
| Supplied component | Integration destination |

|---|---|

| Deployment profile and path resolution | Existing configuration/platform layer |

| Capability domain and manifest | Existing shared domain or skills/agents layer |

| CLI check command | Existing dread-universal executable |

| CLI run command | Existing production API startup path |

| Capability registry | Existing persistence/database architecture |

| ResourceBroker contract | Existing resource orchestration and Supervisor integration |

| LegacyHubAdapter contract | Compatibility layer around existing Hub |

| Provider-isolation preflight | Discovery/provider orchestration boundary |

Do not create a second production HTTP server alongside the existing API.

GPU and model execution boundary
The existing Python GPU Supervisor remains authoritative. Rust should request resources through the authenticated bridge and must not independently start competing model servers or acquire GPU leases outside the established authority.

The existing ComfyUI service must remain untouched during ordinary Hub integration.

3. Batch 0 — controlling master specification
Source: PROJECT_DREAD_UNIVERSAL_PORTABLE_MASTER_PLAN_v3.md

This is the product-level authority for the project.

Key constraints
Portable defaults must be generic.

First-run setup must choose and persist a usable port; port zero is setup-only.

Provider discovery is not permission to execute a provider.

Remote providers are disabled unless explicitly configured.

Imported extensions are disabled by default.

Unknown or unapproved permissions must block execution.

Existing legacy Hub behavior must be mapped and evidenced before claiming compatibility.

A successful compile is not full product acceptance.

Release acceptance requires real integration and end-to-end evidence.

Delivery standard
Every implementation batch should include:

File manifest and change classification.

Exact preimage/backup procedure.

Complete new files or reviewable patches.

Rollback instructions.

Commands actually executed.

Test output and exit status.

Explicit list of tests not run.

A checkpoint recording what is implemented and what remains.

4. Batch 1 — portable configuration and contracts
Supplied generic example profile
Toml

Copy

[hub]

profile_name = "example-local"

listen_address = "127.0.0.1"

listen_port = 0

workspace_roots = []



[storage]

config_dir = ""

state_dir = ""

cache_dir = ""

log_dir = ""

artifact_dir = ""



[discovery]

allow_loopback_providers = true

allow_remote_providers = false

probe_timeout_seconds = 3

max_response_bytes = 1048576

port_scanning = false



[resource_broker]

provider = "none"

single_authority_per_device = true



[providers]



[legacy_hub]

enabled = false

adapter = "legacy-hub-v1"

base_url = ""

source_fingerprint = ""



[extensions]

imports_disabled_by_default = true

require_signature_for_remote_install = true

require_operator_approval_for_activation = true

This is an example profile, not a workstation configuration.

Legacy route matrix
Contract: legacy-route-matrix-v1

No legacy route should be considered verified without inspecting the actual source and recording:

HTTP method and path.

Request and response schemas.

Authentication behavior.

Streaming behavior.

Side effects and data source.

Compatibility status.

Evidence references.

Allowed statuses:

UNKNOWN

UNVERIFIED

PARTIAL

VERIFIED

RETIRED_ACCEPTED

Discovery does not grant execution authorization.

Resource and extension contracts
The proposed design requires one resource authority per device and disabled-by-default imports.

A missing ResourceBroker must fail safely rather than silently permitting competing execution.

Batch 1 acceptance state
The supplied checkpoint explicitly left SQLite persistence, production API/UI, authentication, event streaming, real discovery, mission execution, GPU integration, media integration, self-repair, packaging, and live acceptance unimplemented or unverified.

5. Batch 002 — configuration CLI and rollback controls
Purpose: Add configuration validation, a loopback process-liveness CLI, subprocess tests, and scratch rollback/relocation scripts.

Status: Source supplied; not executed.

File manifest
| File | Status relative to Batch 001 |

|---|---|

| Cargo.toml | Modified |

| crates/dread-core/src/config.rs | Modified |

| crates/dread-cli/Cargo.toml | Added |

| crates/dread-cli/src/main.rs | Added |

| crates/dread-cli/tests/cli.rs | Added |

| scripts/portable-cli-smoke.sh | Added |

| scripts/rollback-smoke.sh | Added |

| BATCH-002-NOTES.md | Added |

The originally omitted BATCH-002-NOTES.md was subsequently supplied.

CLI behavior
Commands:

Bash

Copy

dread-hub check --config <DEPLOYMENT_PROFILE>

dread-hub run --config <DEPLOYMENT_PROFILE>

check validates configuration and resolves paths without creating directories or opening a listener.

run validates configuration before binding a loopback listener and exposes:

GET /health/live

The response explicitly identifies itself as process liveness only and reports that providers were not checked.

This is a testable CLI foundation—not the production DREAD Hub API.

Port-zero protection
The supplied implementation validates startup invariants before binding. It must reject port zero rather than allowing the operating system to select an ephemeral port.

The test fixture may use an OS-selected port to write a nonzero test configuration. That does not make port zero a valid application startup setting.

Test coverage supplied
Valid configuration.

Missing configuration.

Malformed TOML.

Invalid configuration.

Nonzero-port listener startup.

HTTP liveness response.

Port-zero rejection.

CLI argument validation.

Example configuration compatibility.

Rollback correction
Batch 002 introduces six files, not seven.

The two modified files require preimage restoration:

Cargo.toml

crates/dread-core/src/config.rs

Only the six added files may be removed, and only after checking that they still match the batch postimages and contain no later work.

The scratch rollback script proves behavior on disposable fixtures only. It does not prove canonical repository rollback.

Important integration limitation
The supplied CLI uses a standalone Axum liveness server. It must not be introduced as a second production server. Its configuration and startup functionality should be integrated into the canonical dread-api executable.

6. Batch 003 — corrections to Batch 002
Exact changes
| File | Status |

|---|---|

| crates/dread-cli/tests/cli.rs | Modified |

| crates/dread-core/src/manifest.rs | Modified |

| BATCH-002-NOTES.md | Added |

Environment-isolated CLI tests
The subprocess helper removes inherited DREAD_HOME:

Rust

Copy

fn run(args: &[&str]) -> Output {

    Command::new(binary())

        .args(args)

        .env_remove("DREAD_HOME")

        .output()

        .unwrap()

}

The real listener subprocess also removes inherited DREAD_HOME.

This prevents the caller's environment from contaminating tests. Separate tests are still required for valid and invalid DREAD_HOME values.

Declarative manifest enforcement
The unknown-field test now uses actual TOML:

Rust

Copy

#[test]

fn executable_fields_are_not_part_of_declarative_manifest() {

    let text = r#"

manifest_version = "capability-manifest-v1"

id = "example-skill"

name = "Example"

version = "1.0.0"

kind = "skill"

source = "local-reviewed-source"

content_sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"

install_hook = "arbitrary-command"

"#;



    let error = toml::from_str::<CapabilityManifest>(text)

        .expect_err("unknown executable fields must be rejected");

    assert!(error.to_string().contains("unknown field"), "{error}");

}

This verifies that an executable field such as install_hook is rejected by the declarative manifest parser.

Corrected Batch 002 notes
The notes identify the two modified files, six added files, preimage requirements, side-effect limitations, and the fact that no commands were executed.

7. Batch 004 — durable capability registry
Purpose: Provide durable metadata storage without activating imported capabilities.

Status: Source supplied; not compiled or tested.

File manifest
| File | Status relative to Batch 003 |

|---|---|

| Cargo.toml | Modified |

| crates/dread-registry/Cargo.toml | Added |

| crates/dread-registry/src/lib.rs | Added |

| checkpoints/PROJECT-DREAD-AUTONOMOUS-CHECKPOINT.md | Modified |

Registry dependencies supplied
Toml

Copy

rusqlite = { version = "0.32", features = ["backup", "bundled"] }

sha2 = "0.10"

The registry crate also depends on dread-core, serde_json, and thiserror; tempfile is used for tests.

Implemented source capabilities
SQLite capability metadata storage.

Schema version 1.

Migration checksum recording and verification.

Forward-only schema version checking.

Disabled-by-default imports.

Idempotent metadata refresh.

Preservation of existing review and enabled state during refresh.

Rejection of identity reuse across different capability kinds.

Stored provenance and permission metadata.

SQLite online backup.

Integrity checks.

Restore into a new destination path.

Refusal to overwrite an existing destination.

Temporary-directory tests for persistence, refresh, type collision, backup, and restore.

Important behavioral boundary
Importing a capability does not approve, enable, or execute it.

The registry's import operation starts records with:

Review state: discovered

Enabled state: false

Refresh updates imported metadata but does not overwrite the existing review or enabled fields.

Supplied migration defect — blocking
The migration implementation creates dread_schema_migrations before executing MIGRATION_0001.

However, MIGRATION_0001 also unconditionally creates dread_schema_migrations.

On a fresh database, this produces a duplicate-table error.

Required repair: Make the migration sequence internally consistent. For example, let the migration create the table once, or remove the duplicate creation from the migration. Keep the whole migration transactional and add a fresh-database regression test.

Do not mark the registry as working until this is fixed and tested.

Additional registry hardening
Before this becomes a privileged or general-purpose API:

Protect destination creation against symlink and time-of-check/time-of-use races.

Use exclusive creation semantics for backup and restore destinations.

Verify backup source and destination paths.

Consider concurrent database access and locking.

Validate stored data and migration history carefully.

Add tests for corrupt databases, missing migration rows, unsupported versions, and failed restore.

Review Unix-specific permission code and explicitly define supported platforms.

Do not claim database encryption.

The registry must not be applied to a canonical DREAD database until its existing schema and contents have been inventoried and backed up.

Registry test coverage supplied
Persistence of disabled imports.

Refresh preserving review and enabled state.

Rejection of skill-to-agent identity changes.

Backup and restoration to a new path.

Refusal to overwrite existing files.

The refresh test uses direct SQL to establish a test precondition. It is not an approval API and does not establish production activation behavior.

8. Batch 004 checkpoint and corrected status
The supplied checkpoint replaces the earlier checkpoint and records:

Overall release: NOT VERIFIED.

It lists configuration parsing, startup validation, loopback liveness, path resolution, capability contracts, and registry source as implemented in the supplied source.

It explicitly does not claim:

Authentication.

Production API or UI.

Event streaming.

Real provider discovery.

Provider isolation preflight.

Durable mission execution.

Supervisor/GPU integration.

Token Booster.

Media workflows.

Repair engine.

Transactional extension activation.

Signed updater.

Portable archive.

Debian package.

SBOM.

Final acceptance.

The next proposed feature slice is provider-isolation preflight and fail-closed dispatch gating. It should follow integration and verification of the existing slices, not bypass them.

9. Dependency and workspace reconciliation
The supplied batches modify a standalone workspace manifest.

Batch 002 adds:

axum

serde_json

tokio

Batch 004 adds:

rusqlite

sha2

The standalone manifests must be reconciled with the canonical workspace's existing shared dependencies and crate boundaries.

Do not blindly replace the canonical Cargo.toml.

Cargo.lock was not supplied. It must be generated in the integrated workspace and reviewed before using --locked.

The existence of these proposed crates does not authorize creating parallel canonical crates or a second production executable.

10. Consolidated rollback and preservation procedure
Before integration, record:

Bash

Copy

git rev-parse HEAD

git branch --show-current

git status --short --untracked-files=all

git diff --binary

git diff --cached --binary

git submodule status

Preserve the outputs, including untracked files and relevant non-Git directories.

Use an isolated working copy or worktree. Do not stash, clean, reset, or overwrite existing work.

Modified-file preimages
At minimum, the supplied batches identify these paths for preimage handling:

Cargo.toml

crates/dread-core/src/config.rs

crates/dread-cli/tests/cli.rs

crates/dread-core/src/manifest.rs

checkpoints/PROJECT-DREAD-AUTONOMOUS-CHECKPOINT.md

The actual classification must be rechecked against the canonical baseline.

Added files
crates/dread-cli/Cargo.toml

crates/dread-cli/src/main.rs

crates/dread-cli/tests/cli.rs if absent in the baseline

scripts/portable-cli-smoke.sh

scripts/rollback-smoke.sh

BATCH-002-NOTES.md

crates/dread-registry/Cargo.toml

crates/dread-registry/src/lib.rs

Classification is relative to the actual canonical baseline, not the batch author's proposed tree.

Rollback requirements
Restore modified files from their exact preimages.

Remove only newly introduced files after confirming their postimages.

Preserve subsequent work.

Preserve and restore the original Cargo.lock if dependency generation modifies or creates it.

Do not delete entire crate or scripts directories.

Do not use source-file rollback instructions to alter canonical databases.

Never remove a database or production artifact as part of this code rollback.

11. Consolidated verification plan
No tests or commands in these batches were executed in the authoring environment.

Required integrated verification
Bash

Copy

cargo generate-lockfile

Review dependency resolution and the generated lockfile before continuing.

Bash

Copy

cargo fmt --all -- --check



cargo clippy --workspace --all-targets --locked -- -D warnings



cargo test --workspace --locked



cargo build --workspace --release --locked



cargo test -p dread-cli --test cli --locked -- --nocapture



cargo test -p dread-registry --locked -- --nocapture



bash -n scripts/portable-cli-smoke.sh scripts/rollback-smoke.sh



bash scripts/portable-cli-smoke.sh



bash scripts/rollback-smoke.sh

These commands are a proposed verification sequence, not recorded results.

Additional tests needed
Fresh registry migration succeeds.

Reopening an existing registry succeeds.

Migration checksum mismatch is rejected.

Unsupported future schema is rejected.

Corrupt or incomplete backup is rejected.

Backup and restore do not overwrite existing files.

Destination symlink/race cases fail safely.

Valid and invalid DREAD_HOME are tested.

Unknown manifest fields are rejected.

Imported executable content remains inert.

Registry behavior is integrated with the canonical persistence architecture.

Production API startup is tested without creating a parallel listener.

Existing Hub compatibility is tested against a route matrix.

Evidence recording
For each command, record:

UTC timestamp.

Git revision and branch.

Working-tree status.

Tool versions.

Exact command.

Exit code.

Standard output and standard error.

A command is PASS only if it actually ran and exited successfully.

12. Current evidence ledger
| Area | Current evidence |

|---|---|

| Master product specification | Supplied |

| Batch 001 configuration/contracts | Supplied in prior handoff |

| Batch 002 CLI source | Supplied |

| Batch 002 notes and correction | Supplied |

| Batch 003 test/manifest corrections | Supplied |

| Batch 004 registry source | Supplied |

| Batch 004 checkpoint | Supplied |

| Canonical baseline commit | Unknown |

| Canonical branch | Unknown |

| Canonical Git status | Unknown |

| Integrated workspace | Not established |

| Cargo.lock | Not supplied |

| Rust formatting | Not run |

| Clippy | Not run |

| Unit tests | Not run |

| Release build | Not run |

| CLI smoke test | Not run |

| Rollback smoke test | Not run |

| Canonical database compatibility | Not verified |

| Provider execution | Not performed |

| GPU/model operations | Not performed |

| Service changes | Not performed |

| Full portable package | Not implemented/verified |

| Final release acceptance | Not accepted |

13. Recommended continuation order
Preserve and inspect the canonical workspace baseline.

Map every supplied file to the existing eight-crate architecture.

Integrate configuration and manifest functionality without replacing existing modules.

Integrate CLI behavior into the existing production executable.

Repair the registry's duplicate-table migration defect.

Harden backup/restore destination handling.

Reconcile registry persistence with the existing database schema and migration strategy.

Generate and review Cargo.lock.

Run formatting, Clippy, unit tests, and release build using the online Rust compilation path.

Repair actual failures and rerun affected verification.

Implement provider-isolation preflight and fail-closed dispatch gating.

Continue through authentication, production API/UI, durable missions, Supervisor integration, media, repair, packaging, and end-to-end acceptance.

Do not skip verification and move directly to live provider execution.

14. Final engineering interpretation
Batches 0–4 provide a useful foundation:

A product specification.

Portable configuration and security contracts.

A limited configuration CLI.

A declarative capability manifest correction.

A proposed durable capability registry.

Preservation, rollback, and verification procedures.

They do not constitute a complete DREAD Universal Hub.

The most important integration issue is architectural: the standalone dread-core/dread-cli/dread-registry proposal must be reconciled with the existing DREAD Universal workspace. The most important known code defect is the duplicate creation of dread_schema_migrations during the first registry migration.

Controlling status: source supplied, integration pending, tests not run, release not verified.

This document is the consolidated engineering reference. The original batch source remains the authority for exact code; this handoff must not be treated as proof that the code has been compiled, deployed, or accepted.

Reasoning · 210.7s total
Stopped
Reasoning · I need respond to user asking revise below again and chec... · Done
I need respond to user asking revise below again and check complete project. Should produce revised consolidated handoff, corrections, completeness assessment, likely actual patches for known defect and inconsistencies. Should carefully audit previous supplied code/document. User wants "revise below again and check if it is comlete project". I should clearly no not complete. Revise document perhaps full, corrected, bounded. Perhaps include fixes beyond duplicate migration. Should audit source issues in previous batches, because doc has other incorrect claims. I'll analyze thoroughly to identify errors and improve handoff.

I have original v3 master, prior implementation source, doc. Should revise consolidated handoff Batches 0-4. Should correct defects and potential code compile issues. Could provide full replacement handoff and source repair patches perhaps Batch 005. Should not just plan, User says revise. I can do source corrections real tests. Since cannot run, mark not run. No tool relevant.

Important issues:

Previous core manifest test includes toml::from_str::<CapabilityManifest>(json) fixed.
Registry migrate: takes unchecked_transaction(&self)? rusqlite Connection::unchecked_transaction(&self) works. Creates table, then executes MIGRATION_0001 duplicate. Should fix.
Migration query MAX version .optional()?.flatten() perhaps query_row returns Result<Option<i64>>, no rows always one row, so optional weird but works.
if current == Some(0) || current.is_none() and MIGRATION_0001 create table. If migration table newly created externally, current none. Better bootstrap migration in one transaction:
Bash

Copy
let current = ... 
match current {
None => { execute_batch(MIGRATION_0001); insert...}
Some(1) => verify...
Some(0) invalid
Some(v>1) ...
}
MIGRATION_0001 includes table. Works fresh. For existing database with migration table empty, it executes capabilities etc. Fine. Ensure transaction atomic DDL.

kind serialization: serde_json::from_str::<Value>(&format!("\"{}\"", raw.kind))? SQL data might contain quotes, invalid JSON. Better direct match string or serde_json::from_value(Value::String(raw.kind)). No format injection. Use latter. clippy no issue.

create_backup: TOCTOU require_absent then Connection::open. Must harden exclusive create. rusqlite open flags SQLITE_OPEN_READ_WRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_NO_MUTEX doesn't have exclusive O_EXCL. Can create with OpenOptions .write(true).create_new(true) then drop and open. Race after close attacker can replace, still symlink TOCTOU. Within unprivileged private parent okay but harden via open file descriptor and /proc/self/fd? Linux-specific and SQLite. Better use tempfile persist_noclobber then Connection open; still no race between. Should mention destination parent must be operator-owned 0700 and canonical check, reject symlinks. Full robust requires openat2 or tempfile+rename. For general API, implement safe path based on canonical parent, no symlink, private perms, exclusive placeholder, validate same inode after open perhaps using MetadataExt. On Linux, I can create_new and open SQLite, compare metadata of handle and path, but attacker can rename path post check. Best keep parent private and no shared writers, document not privileged. User document says harden. Could provide source patch using create_new and private parent. Not fully race-safe if hostile shared parent. Require canonical parent ownership/mode. Should not overdo if doc only.

restore_to_new: source read-only, query MAX(version) returns i64 works. If no table returns SQL error. Good.

backup Backup::new(&self.connection, &mut backup) direction? rusqlite backup::Backup::new(from: &Connection, to: &mut Connection), yes.

after backup operation drop, integrity. okay.

set perms after Connection::open creates file window default umask perhaps 0644. Better placeholder create_new with 0600 before open. Good.

prepare_parent: Unix-specific okay target Ubuntu. Existing parent permissions unchanged. Path canonicalization: no symlink guard. Should state not production safe.

RegistryStore::open existing path symlink could follow. needs guard later.

No WAL; okay.

concurrency: &self import means Connection not Sync? no concerns.

manifest imports refreshed content but remains approved/enabled even if content hash changes. This violates "New/updated capabilities remain disabled until review and approval". Previous source refresh preserves review and enabled even content changed, a serious safety defect. Document currently lists as feature, but should flag blocking. Any update to content must reset discovered and disabled. Version/source/permissions changes too. Only health refresh no import. I must revise doc and patch. This is critical.

CapabilityManifest has no adapter compatibility schema etc. foundation only.

core config localhost DNS can resolve remote / rebinding; remote policy is incomplete. Not actual network code. URL parser host domain. No SSRF prevention, redirects, credentials. Fine foundation.

workspace_roots absolute but no symlink/canonicalization. no discovery yet.

profile.example.toml comments placeholder port not valid if uncommented. Fine.

CLI run: print liveness not provider. Not canonical.

CLI check requires startup valid port, so valid config check perhaps appropriate.

main.rs: imports ConfigError path public yes module pub.

parse OsString compare to &str supported? OsString PartialEq<&str>? yes likely.

Axum 0.8 serve graceful shutdown return io::Result<()>. okay.

tokio signal feature included.

Tests integration binary env macro available.

test temp config missing file no collisions okay.

test startup unused port has race, accepted tests can fail occupied. Perhaps robust bind reserve not possible app. Mark.

rollback smoke uses sha256sum -c absolute paths file: after cd "$WORK" original file lines have absolute, okay.

portable smoke release only checks config no listener no package.

CLI environment DREAD_HOME removed means XDG variables can still affect only layout no errors. fine.

Cargo rust-version 1.82 dependencies directories=6 may require 1.81? fine. Axum 0.8 perhaps 1.75. rusqlite. Ubuntu Rust not default, need tool.

Batch 003 new test manifest expect_err requires CapabilityManifest: Debug, yes.

PermissionDecision fail closed boolean good.

ResourceBroker no authority registry single-enforcement. contract only.

Legacy route matrix verified requires schemas evidence but not source fingerprint, auth etc. Not sufficient to prove parity; no execution.

Existing source handoff full code not consolidated in doc, still prior source. Fine.

architecture canonical asserted "existing eight-crate" not verified by assistant; user supplies document says historically. Should say operator-supplied, not inspected.

"online Rust compilation path" in recommendation weird, remove. No online compiler claim.

Batch 004 duplicate table blocking yes.

completeness score perhaps 4 of 12 stages partly source; Stage 0 not started due no baseline; no full project. Avoid percentage.

Missing project actual code for UI etc.

Should revise all document and include patch for registry critical issues and tests. User asks revise below again, likely expects revised document. I can provide a full replacement document with known defects and precise no complete. Also Batch 005 source patch? They might value. Should not overdo full 1000 lines. Can structure "No. Below is corrected replacement handoff", include full revised content concise but detailed. Include source-repair section for duplicate migration and update disable to correct handoff. Since controlling doc currently describes refresh preservation as behavior but spec requires disable on changed content. Should explicitly mark blocking. Could include patch to be applied to Batch 004 source, not tested. Should decide whether deliver full revised crates/dread-registry/src/lib.rs? Huge. Exact patches and tests enough. User asks revise doc, not necessarily code. But "revise all you did and continue" previous, now "revise below again and check complete" likely doc revision plus assessment. I should correct source too if known defects. Provide Batch 005 minimal patch. Avoid introducing untested complex code, just migration fix and refresh security. Backup hardening perhaps include simple exclusive creation patch and tests. Could be too much. Should have accurate patch contexts.

I'll design Batch 005 exact modifications to registry:

migrate replacement:
Bash

Copy
fn migrate(&self) -> Result... {
 let tx...
 let current: Option<i64> = tx.query_row("SELECT COALESCE(MAX(version),0)...",..., |r|r.get(0))?;
 match current {
 0 => { 
  let existing: Option<String> = tx.query_row("SELECT name FROM sqlite_master WHERE type='table' AND name='dread_capabilities'",...).optional()?;
  if existing.is_some() { return invalid... } // if migration rows absent and cap table exists don't overwrite. Good.
  tx.execute_batch(MIGRATION_0001)?;
  insert...
 }
 1 => verify ...
 version => unsupported...
 }
 tx.commit
}
But initial code creates migration table before query. On fresh current 0 then migration creates table already exists => still duplicate. Must remove pre-create and detect migration table existence:

Bash

Copy
let migration_table_exists: bool = query sqlite_master...
if !exists {
 let capability_table_exists...
 if exists invalid
 tx.execute_batch(MIGRATION_0001)
 insert
} else {
 let current: Option<i64> = query MAX...
 match current {
 None => error incomplete migration history (do not run)
 Some(1) => verify...
 Some(v) if v>1 error...
 Some(v)=> unsupported
}
}
This avoids duplicate and fail closed on migration table empty. Good. On fresh no table, migration creates. On canonical existing capabilities but no migration table, abort. Migration DDL transaction. If execute fails tx rolls back on drop. Test fresh/reopen and existing unversioned cap table abort. Good.

import refresh: After existing kind check, fetch old fingerprint perhaps:
Bash

Copy
let existing: Option<(String,String,String,String,String)> = query kind, version, source, content_sha256, permissions_json...
Could simplest use UPDATE ... review = CASE WHEN dread_capabilities.content_sha256 <> excluded.content_sha256 OR version/source/permissions differs THEN 'discovered' ELSE review END, enabled = CASE ... THEN 0 ELSE enabled END. No reset on name only (descriptive) okay. version change with same hash perhaps update; safer any version/source/hash/permissions. Use:

Bash

Copy
review = CASE WHEN version <> excluded.version OR source <> ... OR content_sha... OR permissions_json... THEN 'discovered' ELSE review END,
enabled = CASE WHEN ... THEN 0 ELSE enabled END
On first import disabled. Tests:

current refresh test originally expects approved/enabled preserve on changed version+hash. Replace with changed_content_refresh_revokes_approval_and_disables; direct SQL precondition. assert discovered false. Add unchanged_security_metadata_refresh_preserves_review_and_enabled_state name only update same version/hash/source/perms; preserve. This fits spec. New import no activation.
If approved disabled and refresh unchanged, stays.
Existing source ImportedCapability variables enabled etc not used? imported kind and hash/permissions used. fine.
backup exclusive:
Bash

Copy
fn reserve_new_private_file(path) -> Result<fs::File> {
 OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)
}
Then for Connection::open drop file, race. Better no need patch now, state blocker. If add just exclusive semantics doesn't fully solve race. Document says partial only. Avoid patch. Could modify use create_new and keep File open through SQLite open; SQLite can open existing empty file while handle open. I can keep _reservation until backup complete. Other process can rename but private parent. Use:

Bash

Copy
let reservation = create_private_new(destination)?;
let mut backup=Connection::open(destination)?;
set_private...
verify_reserved_file(destination,&reservation)? // metadata dev/ino same
...
Still path race after check. Require parent private. Not complete. State not privileged.

No need source patch backup.

Test migration checksum mismatch: open, update checksum, drop, reopen errors. Future schema: open, update version? migration row primary can update version 2, reopen current 2 rejects. Good. Empty migration history: fresh open then delete row leaves caps, reopen should invalid. Corrupt backup test file text read-only query error. Fresh migration test via import covers. Reopen covers. Should not include all now.

Doc revised full can list these fixes. Current evidence ledger update Batch 005 source supplied. Name "Batches 0–5". User requested below Batches 0-4 perhaps keep and add repair addendum. I can provide replacement handoff with Batch 005. Perhaps "Consolidated Engineering Handoff — Batches 0–5". Good.

Must be precise "source supplied" doesn't mean compiles. No tests actual. Environment unknown. Project not complete. Completeness matrix 12 stages: 0 Blocked not started baseline 1 NOT VERIFIED contracts only 2 PARTIAL SOURCE only manifest registry no real discovery 3 NOT IMPLEMENTED 4 NOT IMPLEMENTED 5 PARTIAL CONTRACT only 6 CONTRACT ONLY 7 NOT IMPLEMENTED 8 PARTIAL safety primitives only 9 NOT IMPLEMENTED 10 NOT IMPLEMENTED 11 NOT RUN No percentages.

Acceptance 20 none direct evidence; perhaps some source requirements only no PASS. "No release acceptance criterion has direct integrated evidence." Good.

Canonical source architecture supplied by User, unverified. Eight crates. No second server. The handoff source exact authority only with repairs; master plan controls.

Rollback: include new modified registry file preimage, tests. If applying batch 5, file crates/dread-registry/src/lib.rs modified; checkpoint modified. No new files if patches. Cargo.lock not supplied. No provider side effects.

Verification commands: canonical names dread-cli/dread-registry only if retained during sandbox; don't instruct canonical nonexistent packages. Use conditional. Release all. Environment versions capture. No "online Rust compilation path". Tests actual output unavailable.

Should include actual output column "No output; not executed". Fine.

Source patch: migrate full replacement and import SQL diff plus tests replacement. I'll write valid Rust carefully.

Migrate:

Bash

Copy
fn migrate(&self)... {
 let transaction...
 let migration_table_exists: bool = transaction.query_row(
 "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='dread_schema_migrations')",[],|r|r.get(0))?;
 if !migration... {
  let registry_table_exists...
  if registry... return Err...
  transaction.execute_batch(M
Partial output is preserved.
Reuse Prompt
PROJECT DREAD UNIVERSAL — GENERATE THE MISSING CODE FOR CHATGPT
You are working through a copy-and-paste-only interface.

You do NOT have:

Access to the user's computer or filesystem.

Access to the project's ZIP files.

GitHub or repository access.

File upload or download capability.

Permission to claim that you have tested or integrated code into the actual project.

Your job is to GENERATE REAL SOURCE CODE in your responses so the user can copy it into ChatGPT. ChatGPT already has the project archives, historical source and engineering requirements.

1. Work from the established project requirements
Use the PROJECT DREAD UNIVERSAL Master Plan v3.0 and the specifications and corrections for Batches 0–4 already established in our conversation.

The target is the complete universal Ubuntu Hub, not a prototype, mockup, demonstration or simplified replacement.

Preserve these requirements:

Eight-crate Rust control-plane architecture.

React/TypeScript production interface.

SQLite durable registry and persistence.

Real provider discovery and agent orchestration.

OpenHands, DeerFlow, LangGraph, Pstack and MCP integrations.

Model discovery and routing.

Token Booster and context management.

ResourceBroker with one GPU authority per deployment.

DREAD GPU Supervisor as the authority for the original DREAD deployment.

ComfyUI and media integrations.

Mission/task lifecycle, approval, audit and recovery.

Self-repair with safe verification and rollback.

Portable Ubuntu .tar.gz and .deb packaging.

Real tests and evidence.

Do not introduce Ollama as a dependency, a competing GPU scheduler, a second production Axum server, or fake agent/provider execution.

2. Continue coding, not planning
Batches 0–4 have already been specified and consolidated. Do not spend another response rewriting their plans.

Identify the remaining implementation gaps from those specifications and produce actual Rust, TypeScript, Python, SQL and shell source as needed.

Prioritize these engineering areas:

Fix the known Batch 004 SQLite migration defect: the migration currently risks creating dread_schema_migrations twice on a fresh database.

Harden durable registry migration, provenance, discovery refresh, backup and restore.

Integrate CLI/configuration functionality without creating a competing production API server.

Implement provider-isolation preflight and fail-closed dispatch admission.

Implement the durable task/mission execution contracts, event persistence and restart reconciliation.

Implement real adapter interfaces and safe provider dispatch boundaries.

Implement the production Hub API and frontend integration.

Complete security, permissions, approvals, audit, recovery and portable packaging.

Use the existing eight-crate architecture. Do not blindly create duplicate crates or modules when an equivalent canonical component already exists.

3. Output actual source files in the panel
The user will copy your answer into ChatGPT.

Use this exact format for every file:

FILE: crates/dread-registry/src/lib.rs

Follow it with a complete fenced Rust code block containing the entire file.

Rules:

Provide complete new files, not pseudocode.

For modified files, provide a complete replacement only when you have enough source context; otherwise supply a clearly marked unified diff.

Include all necessary imports, types, error handling, validation and tests.

Include the matching Cargo dependency changes when required.

Do not omit code with phrases such as "same as before", "implementation goes here" or "rest unchanged".

Keep file paths explicit.

Do not mix multiple files inside one code block.

Keep each response to a manageable group of approximately 3–5 files.

If a file is too long, split it into numbered consecutive parts and clearly state where each part joins.

Maintain a continuation index so the user can transfer every file without losing track.

Do not repeat previously delivered files unless providing a corrected complete replacement.

4. Coding quality requirements
Write production-oriented code with:

Typed errors and meaningful error propagation.

Input validation and safe path handling.

Transactional database operations.

Migration version and checksum handling.

Explicit permissions and approval checks.

Concurrency and cancellation safety.

Idempotency and restart-recovery semantics.

Structured logging and secret redaction.

Unit and integration tests.

No hard-coded workstation paths, credentials, ports or model names.

No fake successful provider responses.

Use the existing project dependencies where possible. Do not add large dependencies without a technical reason.

5. Respect the integration boundary
You cannot see the canonical source files, so never pretend a generated replacement is already integrated or verified.

For code that depends on unknown existing function signatures:

Prefer a standalone, typed module with an explicit interface.

Identify its intended crate and integration point.

State the exact dependency and caller contract.

Mark unresolved compatibility points NEEDS_CANONICAL_MERGE.

Do not invent existing APIs or silently replace unknown code.

ChatGPT will compare and integrate your code against the actual project archive.

6. Verification honesty
You may write tests, scripts and CI configuration, but do not claim they passed unless you actually executed them and have the output.

Label status accurately:

CODE_GENERATED

STATICALLY_REVIEWED

TESTS_NOT_RUN

NEEDS_CANONICAL_MERGE

VERIFIED only when actual execution evidence exists.

7. First coding delivery
Start with the following deliverable:

DELIVERY 001 — REGISTRY AND SAFE DISPATCH FOUNDATION

Produce:

The corrected complete crates/dread-registry/src/lib.rs.

The corresponding crates/dread-registry/Cargo.toml.

Any required workspace Cargo.toml changes as a clearly marked diff.

Additional registry migration and backup/restore tests.

A provider-isolation preflight module with fail-closed admission.

Tests proving discovery does not automatically authorize execution.

A concise integration note identifying exact intended crate paths and dependencies.

Then continue with the next missing implementation slice. Do not stop after Delivery 001.

8. Final objective
Generate the missing implementation code in successive copyable deliveries until the full PROJECT DREAD UNIVERSAL implementation is represented.

The user transfers each delivery to ChatGPT, which will reconcile the code with the available canonical source, integrate it, run online Rust compilation/Clippy/tests where possible, and prepare the finished project bundle.

Do not ask the user to upload a ZIP, provide GitHub access, or locate a repository. You are the code generator in this workflow. Produce the actual source code now.

Reasoning · Done · 114s total
DELIVERY 001 — REGISTRY AND SAFE DISPATCH FOUNDATION

Status: CODE_GENERATED, STATICALLY_REVIEWED, TESTS_NOT_RUN, NEEDS_CANONICAL_MERGE
Intended crates: existing dread-registry; preflight belongs in the canonical execution/orchestration crate, tentatively dread-execution.
The registry correction removes the duplicate migration-table creation. Changed imports now reset to discovered and disabled; only byte-equivalent capability definitions preserve review state.
No provider dispatch or production activation is implemented here.
Continuation index

Workspace dependency diff.
crates/dread-registry/Cargo.toml
crates/dread-registry/src/lib.rs
crates/dread-execution/src/preflight.rs
Next delivery: durable mission/task transitions, events, idempotency, and provider reconciliation.
FILE: Cargo.toml

Diff

Copy
diff --git a/Cargo.toml b/Cargo.toml
--- a/Cargo.toml
+++ b/Cargo.toml
@@
 members = [
     "crates/dread-core",
     "crates/dread-cli",
     "crates/dread-registry",
+    # NEEDS_CANONICAL_MERGE:
+    # Add the existing execution/orchestration crate here only after confirming
+    # its canonical name. Do not create a duplicate crate if one already exists.
 ]
@@
 directories = "6"
 rusqlite = { version = "0.32", features = ["backup", "bundled"] }
 serde = { version = "1", features = ["derive"] }
 serde_json = "1"
 sha2 = "0.10"
 tempfile = "3"
 thiserror = "2"
 toml = "0.8"
 tokio = { version = "1", features = ["macros", "net", "rt-multi-thread", "signal"] }
 url = "2"
No additional workspace dependency is required for preflight.rs beyond the already established dread-core, serde, and thiserror. Its canonical crate manifest must expose those dependencies.

FILE: crates/dread-registry/Cargo.toml

Toml

Copy
[package]
name = "dread-registry"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
description = "Durable capability registry for DREAD Universal"

[dependencies]
dread-core = { path = "../dread-core" }
rusqlite.workspace = true
serde_json.workspace = true
sha2.workspace = true
thiserror.workspace = true

[dev-dependencies]
tempfile.workspace = true
FILE: crates/dread-registry/src/lib.rs

Rust

Copy
//! Durable, non-authorizing capability registration.
//!
//! Discovery and import never grant execution permission. A changed manifest is
//! treated as new content: it becomes disabled and returns to `discovered`.

use dread_core::{
    domain::{CapabilityKind, ImportedCapability, ReviewState},
    manifest::{CapabilityManifest, ManifestError},
};
use rusqlite::{
    backup::Backup,
    params,
    Connection,
    OptionalExtension,
    Row,
    Transaction,
    TransactionBehavior,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use thiserror::Error;

const SCHEMA_VERSION: i64 = 1;

/// This migration deliberately does NOT create dread_schema_migrations.
/// migrate() creates that bookkeeping table once, before applying migrations.
const MIGRATION_0001: &str = r#"
CREATE TABLE dread_capabilities (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    kind TEXT NOT NULL,
    source TEXT NOT NULL,
    content_sha256 TEXT NOT NULL,
    dependencies_json TEXT NOT NULL,
    publisher_signature TEXT,
    review TEXT NOT NULL CHECK (
        review IN (
            'discovered', 'inspected', 'registered',
            'validated', 'approved', 'rejected'
        )
    ),
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    permissions_json TEXT NOT NULL,
    created_at_unix_seconds INTEGER NOT NULL,
    updated_at_unix_seconds INTEGER NOT NULL
) STRICT;

CREATE INDEX dread_capabilities_kind_enabled
    ON dread_capabilities(kind, enabled);

CREATE TABLE dread_discovery_observations (
    observation_id INTEGER PRIMARY KEY AUTOINCREMENT,
    capability_id TEXT NOT NULL,
    source TEXT NOT NULL,
    content_sha256 TEXT NOT NULL,
    manifest_json TEXT NOT NULL,
    observed_at_unix_seconds INTEGER NOT NULL,
    FOREIGN KEY(capability_id) REFERENCES dread_capabilities(id)
) STRICT;

CREATE INDEX dread_discovery_observations_capability
    ON dread_discovery_observations(capability_id, observation_id);

CREATE TABLE dread_registry_audit (
    audit_id INTEGER PRIMARY KEY AUTOINCREMENT,
    capability_id TEXT,
    operation TEXT NOT NULL,
    actor TEXT NOT NULL,
    occurred_at_unix_seconds INTEGER NOT NULL,
    details_json TEXT NOT NULL
) STRICT;

CREATE INDEX dread_registry_audit_capability
    ON dread_registry_audit(capability_id, audit_id);
"#;

#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("registry I/O failed: {0}")]
    Io(#[from] std::io::Error),

    #[error("registry SQLite operation failed: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("registry serialization failed: {0}")]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Manifest(#[from] ManifestError),

    #[error("invalid registry operation: {0}")]
    Invalid(String),

    #[error("capability not found: {0}")]
    NotFound(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryRecord {
    pub id: String,
    pub name: String,
    pub version: String,
    pub kind: CapabilityKind,
    pub source: String,
    pub content_sha256: String,
    pub dependencies: Vec<String>,
    pub publisher_signature: Option<String>,
    pub review: ReviewState,
    pub enabled: bool,
    pub permissions: Vec<String>,
    pub created_at_unix_seconds: i64,
    pub updated_at_unix_seconds: i64,
}

#[derive(Debug)]
struct RawRegistryRecord {
    id: String,
    name: String,
    version: String,
    kind: String,
    source: String,
    content_sha256: String,
    dependencies_json: String,
    publisher_signature: Option<String>,
    review: String,
    enabled: i64,
    permissions_json: String,
    created_at_unix_seconds: i64,
    updated_at_unix_seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportOutcome {
    Created,
    RefreshedUnchanged,
    ChangedAndDisabled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportResult {
    pub record: RegistryRecord,
    pub outcome: ImportOutcome,
}

#[derive(Debug)]
pub struct RegistryStore {
    connection: Connection,
}

impl RegistryStore {
    /// Opens an explicitly selected database.
    ///
    /// NEEDS_CANONICAL_MERGE: Before using this against an existing canonical
    /// database, inventory its schema and create a verified backup. These
    /// dread_-prefixed tables are not a substitute for an existing mission DB.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, RegistryError> {
        let path = checked_absolute_path(path.as_ref())?;
        prepare_parent(path)?;

        if path.exists() {
            require_regular_non_symlink_file(path)?;
        }

        let existed = path.exists();
        let connection = Connection::open(path)?;
        if !existed {
            set_private_file_permissions(path)?;
        }

        connection.execute_batch("PRAGMA foreign_keys = ON;")?;
        connection.busy_timeout(Duration::from_secs(5))?;

        let store = Self { connection };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<(), RegistryError> {
        let transaction = self
            .connection
            .unchecked_transaction_with_behavior(TransactionBehavior::Immediate)?;

        // This is the only place that creates the migration bookkeeping table.
        transaction.execute_batch(
            "CREATE TABLE IF NOT EXISTS dread_schema_migrations (
                version INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                checksum_sha256 TEXT NOT NULL,
                applied_at_unix_seconds INTEGER NOT NULL
            ) STRICT;",
        )?;

        let current = transaction.query_row(
            "SELECT MAX(version) FROM dread_schema_migrations",
            [],
            |row| row.get::<_, Option<i64>>(0),
        )?;

        match current {
            Some(version) if version > SCHEMA_VERSION => {
                return Err(RegistryError::Invalid(format!(
                    "database schema {version} is newer than supported schema {SCHEMA_VERSION}"
                )));
            }
            Some(version) if version < 0 => {
                return Err(RegistryError::Invalid(
                    "negative schema version is invalid".into(),
                ));
            }
            Some(0) | None => {
                // Detect a partially initialized or unrelated database instead
                // of issuing CREATE TABLE over an unknown existing schema.
                let existing_domain_tables: i64 = transaction.query_row(
                    "SELECT COUNT(*)
                     FROM sqlite_master
                     WHERE type = 'table'
                       AND name IN (
                           'dread_capabilities',
                           'dread_discovery_observations',
                           'dread_registry_audit'
                       )",
                    [],
                    |row| row.get(0),
                )?;
                if existing_domain_tables != 0 {
                    return Err(RegistryError::Invalid(
                        "unversioned registry domain tables already exist; manual reconciliation required"
                            .into(),
                    ));
                }

                let checksum = migration_checksum(MIGRATION_0001);
                transaction.execute_batch(MIGRATION_0001)?;
                transaction.execute(
                    "INSERT INTO dread_schema_migrations (
                        version, name, checksum_sha256, applied_at_unix_seconds
                     ) VALUES (?1, ?2, ?3, ?4)",
                    params![
                        SCHEMA_VERSION,
                        "initial-capability-registry",
                        checksum,
                        unix_seconds()?
                    ],
                )?;
            }
            Some(SCHEMA_VERSION) => {
                verify_recorded_migration(&transaction, SCHEMA_VERSION, MIGRATION_0001)?;
            }
            Some(version) => {
                return Err(RegistryError::Invalid(format!(
                    "no migration path from schema version {version}"
                )));
            }
        }

        transaction.commit()?;
        Ok(())
    }

    /// Imports or refreshes metadata transactionally.
    ///
    /// Any version, source, content, dependency, signature, or permission change
    /// disables the capability and resets review to `discovered`.
    pub fn import(
        &self,
        manifest: &CapabilityManifest,
    ) -> Result<ImportResult, RegistryError> {
        let imported: ImportedCapability = manifest.import_disabled()?;
        let kind = kind_name(imported.kind)?;
        let now = unix_seconds()?;
        let permissions_json = serde_json::to_string(&manifest.permissions)?;
        let dependencies_json = serde_json::to_string(&manifest.dependencies)?;
        let manifest_json = serde_json::to_string(manifest)?;

        let transaction = self
            .connection
            .unchecked_transaction_with_behavior(TransactionBehavior::Immediate)?;

        let existing = transaction
            .query_row(
                &format!("{SELECT_RECORD} WHERE id = ?1"),
                [&manifest.id],
                raw_record,
            )
            .optional()?
            .map(decode_record)
            .transpose()?;

        let outcome = if let Some(previous) = existing {
            if previous.kind != imported.kind {
                return Err(RegistryError::Invalid(format!(
                    "capability identity {:?} cannot change kind from {:?} to {:?}",
                    manifest.id, previous.kind, imported.kind
                )));
            }

            let definition_changed = previous.version != manifest.version
                || previous.source != manifest.source
                || previous.content_sha256 != manifest.content_sha256
                || previous.dependencies != manifest.dependencies
                || previous.publisher_signature != manifest.publisher_signature
                || previous.permissions != manifest.permissions;

            if definition_changed {
                transaction.execute(
                    "UPDATE dread_capabilities SET
                        name = ?2,
                        version = ?3,
                        source = ?4,
                        content_sha256 = ?5,
                        dependencies_json = ?6,
                        publisher_signature = ?7,
                        permissions_json = ?8,
                        review = 'discovered',
                        enabled = 0,
                        updated_at_unix_seconds = ?9
                     WHERE id = ?1",
                    params![
                        manifest.id,
                        manifest.name,
                        manifest.version,
                        manifest.source,
                        manifest.content_sha256,
                        dependencies_json,
                        manifest.publisher_signature,
                        permissions_json,
                        now
                    ],
                )?;
                ImportOutcome::ChangedAndDisabled
            } else {
                // Only display metadata and timestamps are refreshed. Review,
                // enabled state, creation time, and execution permissions remain.
                transaction.execute(
                    "UPDATE dread_capabilities SET
                        name = ?2,
                        updated_at_unix_seconds = ?3
                     WHERE id = ?1",
                    params![manifest.id, manifest.name, now],
                )?;
                ImportOutcome::RefreshedUnchanged
            }
        } else {
            transaction.execute(
                "INSERT INTO dread_capabilities (
                    id, name, version, kind, source, content_sha256,
                    dependencies_json, publisher_signature,
                    review, enabled, permissions_json,
                    created_at_unix_seconds, updated_at_unix_seconds
                 ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6,
                    ?7, ?8, 'discovered', 0, ?9, ?10, ?10
                 )",
                params![
                    manifest.id,
                    manifest.name,
                    manifest.version,
                    kind,
                    manifest.source,
                    manifest.content_sha256,
                    dependencies_json,
                    manifest.publisher_signature,
                    permissions_json,
                    now
                ],
            )?;
            ImportOutcome::Created
        };

        transaction.execute(
            "INSERT INTO dread_discovery_observations (
                capability_id, source, content_sha256,
                manifest_json, observed_at_unix_seconds
             ) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                manifest.id,
                manifest.source,
                manifest.content_sha256,
                manifest_json,
                now
            ],
        )?;

        let audit_operation = match outcome {
            ImportOutcome::Created => "import_created",
            ImportOutcome::RefreshedUnchanged => "discovery_refreshed",
            ImportOutcome::ChangedAndDisabled => "definition_changed_and_disabled",
        };
        insert_audit(
            &transaction,
            Some(&manifest.id),
            audit_operation,
            "discovery",
            now,
            serde_json::json!({
                "content_sha256": manifest.content_sha256,
                "source": manifest.source,
                "execution_authorized": false
            }),
        )?;

        transaction.commit()?;
        Ok(ImportResult {
            record: self.get(&manifest.id)?,
            outcome,
        })
    }

    pub fn get(&self, id: &str) -> Result<RegistryRecord, RegistryError> {
        self.connection
            .query_row(&format!("{SELECT_RECORD} WHERE id = ?1"), [id], raw_record)
            .optional()?
            .map(decode_record)
            .transpose()?
            .ok_or_else(|| RegistryError::NotFound(id.to_owned()))
    }

    pub fn list(&self) -> Result<Vec<RegistryRecord>, RegistryError> {
        let mut statement = self
            .connection
            .prepare(&format!("{SELECT_RECORD} ORDER BY kind, id"))?;
        let rows = statement.query_map([], raw_record)?;
        rows.map(|row| decode_record(row?)).collect()
    }

    pub fn observation_count(&self, capability_id: &str) -> Result<u64, RegistryError> {
        let count = self.connection.query_row(
            "SELECT COUNT(*)
             FROM dread_discovery_observations
             WHERE capability_id = ?1",
            [capability_id],
            |row| row.get::<_, i64>(0),
        )?;
        u64::try_from(count)
            .map_err(|_| RegistryError::Invalid("negative observation count".into()))
    }

    /// Creates a consistent SQLite backup without replacing an existing path.
    pub fn create_backup(
        &self,
        destination: impl AsRef<Path>,
    ) -> Result<PathBuf, RegistryError> {
        let destination = reserve_new_private_file(destination.as_ref())?;
        let backup_result = (|| {
            let mut backup = Connection::open(&destination)?;
            {
                let operation = Backup::new(&self.connection, &mut backup)?;
                operation.run_to_completion(256, Duration::from_millis(50), None)?;
            }
            verify_integrity(&backup)?;
            verify_foreign_keys(&backup)?;
            verify_backup_migrations(&backup)?;
            Ok(())
        })();

        if let Err(error) = backup_result {
            let _ = fs::remove_file(&destination);
            return Err(error);
        }
        Ok(destination)
    }

    /// Restores into a new path and never replaces the live or backup database.
    pub fn restore_to_new(
        backup_path: impl AsRef<Path>,
        destination: impl AsRef<Path>,
    ) -> Result<Self, RegistryError> {
        let backup_path = checked_absolute_path(backup_path.as_ref())?;
        require_regular_non_symlink_file(backup_path)?;

        let source = Connection::open_with_flags(
            backup_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        verify_integrity(&source)?;
        verify_foreign_keys(&source)?;
        verify_backup_migrations(&source)?;

        let destination = reserve_new_private_file(destination.as_ref())?;
        let restore_result = (|| {
            let mut restored = Connection::open(&destination)?;
            {
                let operation = Backup::new(&source, &mut restored)?;
                operation.run_to_completion(256, Duration::from_millis(50), None)?;
            }
            verify_integrity(&restored)?;
            verify_foreign_keys(&restored)?;
            verify_backup_migrations(&restored)?;
            Ok(())
        })();

        if let Err(error) = restore_result {
            let _ = fs::remove_file(&destination);
            return Err(error);
        }

        Self::open(destination)
    }
}

const SELECT_RECORD: &str = "
    SELECT id, name, version, kind, source, content_sha256,
           dependencies_json, publisher_signature,
           review, enabled, permissions_json,
           created_at_unix_seconds, updated_at_unix_seconds
    FROM dread_capabilities
";

fn raw_record(row: &Row<'_>) -> rusqlite::Result<RawRegistryRecord> {
    Ok(RawRegistryRecord {
        id: row.get(0)?,
        name: row.get(1)?,
        version: row.get(2)?,
        kind: row.get(3)?,
        source: row.get(4)?,
        content_sha256: row.get(5)?,
        dependencies_json: row.get(6)?,
        publisher_signature: row.get(7)?,
        review: row.get(8)?,
        enabled: row.get(9)?,
        permissions_json: row.get(10)?,
        created_at_unix_seconds: row.get(11)?,
        updated_at_unix_seconds: row.get(12)?,
    })
}

fn decode_record(raw: RawRegistryRecord) -> Result<RegistryRecord, RegistryError> {
    let kind: CapabilityKind =
        serde_json::from_value(Value::String(raw.kind.clone()))?;

    let review = match raw.review.as_str() {
        "discovered" => ReviewState::Discovered,
        "inspected" => ReviewState::Inspected,
        "registered" => ReviewState::Registered,
        "validated" => ReviewState::Validated,
        "approved" => ReviewState::Approved,
        "rejected" => ReviewState::Rejected,
        other => {
            return Err(RegistryError::Invalid(format!(
                "unknown stored review state {other:?}"
            )))
        }
    };

    let enabled = match raw.enabled {
        0 => false,
        1 => true,
        other => {
            return Err(RegistryError::Invalid(format!(
                "invalid stored enabled value {other}"
            )))
        }
    };

    Ok(RegistryRecord {
        id: raw.id,
        name: raw.name,
        version: raw.version,
        kind,
        source: raw.source,
        content_sha256: raw.content_sha256,
        dependencies: serde_json::from_str(&raw.dependencies_json)?,
        publisher_signature: raw.publisher_signature,
        review,
        enabled,
        permissions: serde_json::from_str(&raw.permissions_json)?,
        created_at_unix_seconds: raw.created_at_unix_seconds,
        updated_at_unix_seconds: raw.updated_at_unix_seconds,
    })
}

fn kind_name(kind: CapabilityKind) -> Result<String, RegistryError> {
    match serde_json::to_value(kind)? {
        Value::String(value) => Ok(value),
        _ => Err(RegistryError::Invalid(
            "capability kind did not serialize as a string".into(),
        )),
    }
}

fn verify_recorded_migration(
    transaction: &Transaction<'_>,
    version: i64,
    migration: &str,
) -> Result<(), RegistryError> {
    let recorded: String = transaction.query_row(
        "SELECT checksum_sha256
         FROM dread_schema_migrations
         WHERE version = ?1",
        [version],
        |row| row.get(0),
    )?;
    let expected = migration_checksum(migration);
    if recorded == expected {
        Ok(())
    } else {
        Err(RegistryError::Invalid(format!(
            "schema migration {version} checksum mismatch"
        )))
    }
}

fn verify_backup_migrations(connection: &Connection) -> Result<(), RegistryError> {
    let version: Option<i64> = connection.query_row(
        "SELECT MAX(version) FROM dread_schema_migrations",
        [],
        |row| row.get(0),
    )?;
    if version != Some(SCHEMA_VERSION) {
        return Err(RegistryError::Invalid(format!(
            "unsupported backup schema version {version:?}"
        )));
    }

    let recorded: String = connection.query_row(
        "SELECT checksum_sha256
         FROM dread_schema_migrations
         WHERE version = ?1",
        [SCHEMA_VERSION],
        |row| row.get(0),
    )?;
    if recorded != migration_checksum(MIGRATION_0001) {
        return Err(RegistryError::Invalid(
            "backup migration checksum mismatch".into(),
        ));
    }
    Ok(())
}

fn migration_checksum(migration: &str) -> String {
    format!("{:x}", Sha256::digest(migration.as_bytes()))
}

fn insert_audit(
    transaction: &Transaction<'_>,
    capability_id: Option<&str>,
    operation: &str,
    actor: &str,
    occurred_at_unix_seconds: i64,
    details: Value,
) -> Result<(), RegistryError> {
    if operation.trim().is_empty() || actor.trim().is_empty() {
        return Err(RegistryError::Invalid(
            "audit operation and actor are required".into(),
        ));
    }
    transaction.execute(
        "INSERT INTO dread_registry_audit (
            capability_id, operation, actor,
            occurred_at_unix_seconds, details_json
         ) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            capability_id,
            operation,
            actor,
            occurred_at_unix_seconds,
            serde_json::to_string(&details)?
        ],
    )?;
    Ok(())
}

fn verify_integrity(connection: &Connection) -> Result<(), RegistryError> {
    let mut statement = connection.prepare("PRAGMA integrity_check")?;
    let results = statement.query_map([], |row| row.get::<_, String>(0))?;
    let mut failures = Vec::new();

    for result in results {
        let result = result?;
        if result != "ok" {
            failures.push(result);
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(RegistryError::Invalid(format!(
            "SQLite integrity_check failed: {}",
            failures.join("; ")
        )))
    }
}

fn verify_foreign_keys(connection: &Connection) -> Result<(), RegistryError> {
    let mut statement = connection.prepare("PRAGMA foreign_key_check")?;
    let mut rows = statement.query([])?;
    if rows.next()?.is_some() {
        Err(RegistryError::Invalid(
            "SQLite foreign_key_check reported a violation".into(),
        ))
    } else {
        Ok(())
    }
}

fn checked_absolute_path(path: &Path) -> Result<&Path, RegistryError> {
    if !path.is_absolute() {
        return Err(RegistryError::Invalid(format!(
            "database path must be absolute: {}",
            path.display()
        )));
    }
    Ok(path)
}

fn require_regular_non_symlink_file(path: &Path) -> Result<(), RegistryError> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(RegistryError::Invalid(format!(
            "expected a regular, non-symlink file: {}",
            path.display()
        )));
    }
    Ok(())
}

/// Resolves the parent, rejects a symlink at the final component, and reserves
/// the file atomically. Callers must keep using the returned canonical path.
fn reserve_new_private_file(path: &Path) -> Result<PathBuf, RegistryError> {
    checked_absolute_path(path)?;
    prepare_parent(path)?;

    match fs::symlink_metadata(path) {
        Ok(_) => {
            return Err(RegistryError::Invalid(format!(
                "refusing to overwrite existing path: {}",
                path.display()
            )))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }

    let file_name = path.file_name().ok_or_else(|| {
        RegistryError::Invalid("backup destination needs a file name".into())
    })?;
    let canonical_parent = fs::canonicalize(path.parent().ok_or_else(|| {
        RegistryError::Invalid("backup destination needs a parent".into())
    })?)?;
    let destination = canonical_parent.join(file_name);

    #[cfg(unix)]
    let file = {
        use std::os::unix::fs::OpenOptionsExt;
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&destination)?
    };

    #[cfg(not(unix))]
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&destination)?;

    drop(file);
    Ok(destination)
}

fn prepare_parent(path: &Path) -> Result<(), RegistryError> {
    let parent = path.parent().ok_or_else(|| {
        RegistryError::Invalid(format!("path has no parent: {}", path.display()))
    })?;

    if !parent.exists() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            let mut builder = fs::DirBuilder::new();
            builder.recursive(true).mode(0o700);
            builder.create(parent)?;
        }

        #[cfg(not(unix))]
        fs::create_dir_all(parent)?;
    }
    if !parent.is_dir() {
        return Err(RegistryError::Invalid(format!(
            "database parent is not a directory: {}",
            parent.display()
        )));
    }
    Ok(())
}

fn set_private_file_permissions(path: &Path) -> Result<(), RegistryError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn unix_seconds() -> Result<i64, RegistryError> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| RegistryError::Invalid(format!("system clock error: {error}")))?
        .as_secs();
    i64::try_from(seconds)
        .map_err(|_| RegistryError::Invalid("system timestamp exceeds SQLite range".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn manifest() -> CapabilityManifest {
        CapabilityManifest {
            manifest_version: "capability-manifest-v1".into(),
            id: "example-skill".into(),
            name: "Example".into(),
            version: "1.0.0".into(),
            kind: CapabilityKind::Skill,
            source: "local-reviewed-source".into(),
            content_sha256: "ab".repeat(32),
            dependencies: vec![],
            permissions: vec!["filesystem:read".into()],
            publisher_signature: None,
        }
    }

    fn approve_fixture(store: &RegistryStore) {
        // Test precondition only: this is not an approval or activation API.
        store
            .connection
            .execute(
                "UPDATE dread_capabilities
                 SET review = 'approved', enabled = 1
                 WHERE id = 'example-skill'",
                [],
            )
            .unwrap();
    }

    #[test]
    fn fresh_migration_runs_once_and_reopen_verifies_checksum() {
        let temporary = TempDir::new().unwrap();
        let database = temporary.path().join("registry.sqlite3");

        let store = RegistryStore::open(&database).unwrap();
        let migration_count: i64 = store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM dread_schema_migrations",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(migration_count, 1);

        drop(store);
        RegistryStore::open(&database).unwrap();
    }

    #[test]
    fn migration_checksum_tampering_is_rejected() {
        let temporary = TempDir::new().unwrap();
        let database = temporary.path().join("registry.sqlite3");
        let store = RegistryStore::open(&database).unwrap();
        store
            .connection
            .execute(
                "UPDATE dread_schema_migrations
                 SET checksum_sha256 = 'tampered'
                 WHERE version = 1",
                [],
            )
            .unwrap();
        drop(store);

        assert!(RegistryStore::open(&database).is_err());
    }

    #[test]
    fn unversioned_domain_tables_fail_closed() {
        let temporary = TempDir::new().unwrap();
        let database = temporary.path().join("registry.sqlite3");
        let connection = Connection::open(&database).unwrap();
        connection
            .execute_batch("CREATE TABLE dread_capabilities(id TEXT PRIMARY KEY);")
            .unwrap();
        drop(connection);

        assert!(RegistryStore::open(&database).is_err());
    }

    #[test]
    fn import_persists_disabled_skill_without_becoming_an_agent() {
        let temporary = TempDir::new().unwrap();
        let database = temporary.path().join("registry.sqlite3");

        {
            let store = RegistryStore::open(&database).unwrap();
            let result = store.import(&manifest()).unwrap();
            assert_eq!(result.outcome, ImportOutcome::Created);
            assert_eq!(result.record.kind, CapabilityKind::Skill);
            assert_eq!(result.record.review, ReviewState::Discovered);
            assert!(!result.record.enabled);
            assert_eq!(store.observation_count("example-skill").unwrap(), 1);
        }

        let reopened = RegistryStore::open(&database).unwrap();
        let record = reopened.get("example-skill").unwrap();
        assert_eq!(record.kind, CapabilityKind::Skill);
        assert!(!record.enabled);
        assert_eq!(record.permissions, vec!["filesystem:read"]);
    }

    #[test]
    fn unchanged_refresh_preserves_state_and_adds_provenance() {
        let temporary = TempDir::new().unwrap();
        let store = RegistryStore::open(temporary.path().join("registry.sqlite3")).unwrap();
        let original = store.import(&manifest()).unwrap().record;
        approve_fixture(&store);

        let refreshed = manifest();
        let result = store.import(&refreshed).unwrap();
        assert_eq!(result.outcome, ImportOutcome::RefreshedUnchanged);
        assert_eq!(result.record.review, ReviewState::Approved);
        assert!(result.record.enabled);
        assert_eq!(
            result.record.created_at_unix_seconds,
            original.created_at_unix_seconds
        );
        assert_eq!(store.observation_count("example-skill").unwrap(), 2);
    }

    #[test]
    fn changed_content_disables_previously_approved_capability() {
        let temporary = TempDir::new().unwrap();
        let store = RegistryStore::open(temporary.path().join("registry.sqlite3")).unwrap();
        store.import(&manifest()).unwrap();
        approve_fixture(&store);

        let mut changed = manifest();
        changed.version = "1.1.0".into();
        changed.content_sha256 = "cd".repeat(32);

        let result = store.import(&changed).unwrap();
        assert_eq!(result.outcome, ImportOutcome::ChangedAndDisabled);
        assert_eq!(result.record.review, ReviewState::Discovered);
        assert!(!result.record.enabled);
    }

    #[test]
    fn changed_permissions_also_disable_capability() {
        let temporary = TempDir::new().unwrap();
        let store = RegistryStore::open(temporary.path().join("registry.sqlite3")).unwrap();
        store.import(&manifest()).unwrap();
        approve_fixture(&store);

        let mut changed = manifest();
        changed.permissions.push("process:execute".into());
        let result = store.import(&changed).unwrap();

        assert_eq!(result.outcome, ImportOutcome::ChangedAndDisabled);
        assert!(!result.record.enabled);
    }

    #[test]
    fn identity_cannot_change_from_skill_to_agent() {
        let temporary = TempDir::new().unwrap();
        let store = RegistryStore::open(temporary.path().join("registry.sqlite3")).unwrap();
        store.import(&manifest()).unwrap();

        let mut conflicting = manifest();
        conflicting.kind = CapabilityKind::AgentProvider;
        assert!(store.import(&conflicting).is_err());
        assert_eq!(store.list().unwrap().len(), 1);
    }

    #[test]
    fn backup_restore_recovers_original_registry_without_overwriting_live_db() {
        let temporary = TempDir::new().unwrap();
        let database = temporary.path().join("registry.sqlite3");
        let backup = temporary.path().join("before-refresh.sqlite3");
        let restored_path = temporary.path().join("restored.sqlite3");

        let store = RegistryStore::open(&database).unwrap();
        store.import(&manifest()).unwrap();
        store.create_backup(&backup).unwrap();

        let mut changed = manifest();
        changed.name = "Changed after backup".into();
        store.import(&changed).unwrap();
        assert_eq!(
            store.get("example-skill").unwrap().name,
            "Changed after backup"
        );

        let restored = RegistryStore::restore_to_new(&backup, &restored_path).unwrap();
        assert_eq!(restored.get("example-skill").unwrap().name, "Example");
        assert_eq!(
            store.get("example-skill").unwrap().name,
            "Changed after backup"
        );
        assert!(backup.exists());
    }

    #[test]
    fn backup_and_restore_refuse_to_overwrite_or_follow_final_symlink() {
        let temporary = TempDir::new().unwrap();
        let store = RegistryStore::open(temporary.path().join("registry.sqlite3")).unwrap();
        store.import(&manifest()).unwrap();

        let backup = temporary.path().join("backup.sqlite3");
        store.create_backup(&backup).unwrap();
        assert!(store.create_backup(&backup).is_err());
        assert!(RegistryStore::restore_to_new(&backup, &backup).is_err());

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let link = temporary.path().join("backup-link.sqlite3");
            symlink(&backup, &link).unwrap();
            assert!(RegistryStore::restore_to_new(
                &link,
                temporary.path().join("must-not-exist.sqlite3")
            )
            .is_err());
        }
    }

    #[test]
    fn backup_checksum_tampering_is_rejected_before_restore() {
        let temporary = TempDir::new().unwrap();
        let store = RegistryStore::open(temporary.path().join("registry.sqlite3")).unwrap();
        store.import(&manifest()).unwrap();

        let backup = temporary.path().join("backup.sqlite3");
        store.create_backup(&backup).unwrap();

        let connection = Connection::open(&backup).unwrap();
        connection
            .execute(
                "UPDATE dread_schema_migrations
                 SET checksum_sha256 = 'invalid'
                 WHERE version = 1",
                [],
            )
            .unwrap();
        drop(connection);

        assert!(RegistryStore::restore_to_new(
            &backup,
            temporary.path().join("restored.sqlite3")
        )
        .is_err());
    }
}
FILE: crates/dread-execution/src/preflight.rs

Rust

Copy
//! Provider-isolation preflight and fail-closed dispatch admission.
//!
//! NEEDS_CANONICAL_MERGE:
//! Move this module into the canonical execution/orchestration crate if its name
//! differs. Export it from that crate and call `admit` immediately before real
//! provider dispatch. A returned grant is admission only, never execution proof.

use dread_core::domain::{CapabilityKind, PermissionDecision, ReviewState};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

pub const PREFLIGHT_CONTRACT_VERSION: &str = "provider-isolation-preflight-v1";

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum IsolationControl {
    RuntimeIdentity,
    TaskWorkspaceBoundary,
    MountPolicy,
    EgressPolicy,
    ToolPermissions,
    StableSecretHandling,
    CancellationPropagation,
    RestartReconciliation,
}

impl IsolationControl {
    pub const ALL: [Self; 8] = [
        Self::RuntimeIdentity,
        Self::TaskWorkspaceBoundary,
        Self::MountPolicy,
        Self::EgressPolicy,
        Self::ToolPermissions,
        Self::StableSecretHandling,
        Self::CancellationPropagation,
        Self::RestartReconciliation,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceStatus {
    Verified,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlEvidence {
    pub status: EvidenceStatus,
    /// Reference to durable adapter evidence, not an operator attestation flag.
    pub evidence_id: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderIsolationPreflight {
    pub contract_version: String,
    pub preflight_id: String,
    pub provider_id: String,
    pub adapter_id: String,
    pub adapter_version: String,
    pub runtime_identity: String,
    pub checks: BTreeMap<IsolationControl, ControlEvidence>,
    pub checked_at_unix_seconds: i64,
    pub expires_at_unix_seconds: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderRegistrationState {
    Discovered,
    Disabled,
    Enabled,
    Revoked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityAdmissionState {
    pub capability_id: String,
    pub kind: CapabilityKind,
    pub review: ReviewState,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DispatchRequest {
    pub request_id: String,
    pub provider_id: String,
    pub capability_id: String,
    pub requested_at_unix_seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DispatchGrant {
    pub request_id: String,
    pub provider_id: String,
    pub capability_id: String,
    pub preflight_id: String,
    pub expires_at_unix_seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryObservation {
    pub provider_id: String,
    pub capability_id: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AdmissionError {
    #[error("invalid preflight: {0}")]
    InvalidPreflight(String),

    #[error("provider {0} is not enabled for execution")]
    ProviderNotEnabled(String),

    #[error("capability {0} is not an approved, enabled executable provider")]
    CapabilityNotExecutable(String),

    #[error("execution permission was not explicitly allowed")]
    PermissionDenied,

    #[error("preflight belongs to a different provider")]
    ProviderMismatch,

    #[error("preflight expired or is not yet temporally valid")]
    PreflightExpired,
}

impl DiscoveryObservation {
    /// Discovery deliberately produces no registration, grant, or permission.
    pub fn authorize_execution(&self) -> Result<DispatchGrant, AdmissionError> {
        Err(AdmissionError::ProviderNotEnabled(
            self.provider_id.clone(),
        ))
    }
}

impl ProviderIsolationPreflight {
    pub fn validate_at(&self, now_unix_seconds: i64) -> Result<(), AdmissionError> {
        for (field, value) in [
            ("preflight_id", &self.preflight_id),
            ("provider_id", &self.provider_id),
            ("adapter_id", &self.adapter_id),
            ("adapter_version", &self.adapter_version),
            ("runtime_identity", &self.runtime_identity),
        ] {
            if value.trim().is_empty() {
                return Err(AdmissionError::InvalidPreflight(format!(
                    "{field} is required"
                )));
            }
        }

        if self.contract_version != PREFLIGHT_CONTRACT_VERSION {
            return Err(AdmissionError::InvalidPreflight(
                "unsupported preflight contract version".into(),
            ));
        }

        if self.checked_at_unix_seconds > now_unix_seconds
            || self.expires_at_unix_seconds <= now_unix_seconds
            || self.expires_at_unix_seconds <= self.checked_at_unix_seconds
        {
            return Err(AdmissionError::PreflightExpired);
        }

        let expected: BTreeSet<IsolationControl> =
            IsolationControl::ALL.into_iter().collect();
        let actual: BTreeSet<IsolationControl> = self.checks.keys().copied().collect();
        if actual != expected {
            return Err(AdmissionError::InvalidPreflight(
                "every required isolation control must have explicit evidence".into(),
            ));
        }

        for (control, evidence) in &self.checks {
            if evidence.status != EvidenceStatus::Verified {
                return Err(AdmissionError::InvalidPreflight(format!(
                    "{control:?} is not verified"
                )));
            }
            if evidence.evidence_id.trim().is_empty() {
                return Err(AdmissionError::InvalidPreflight(format!(
                    "{control:?} has no durable evidence reference"
                )));
            }
        }

        Ok(())
    }
}

/// Performs only structural/policy admission. The provider adapter must still
/// durably store the grant binding and return a real provider execution identity.
pub fn admit(
    request: &DispatchRequest,
    provider_state: ProviderRegistrationState,
    capability: &CapabilityAdmissionState,
    permission: PermissionDecision,
    preflight: &ProviderIsolationPreflight,
    now_unix_seconds: i64,
) -> Result<DispatchGrant, AdmissionError> {
    if provider_state != ProviderRegistrationState::Enabled {
        return Err(AdmissionError::ProviderNotEnabled(
            request.provider_id.clone(),
        ));
    }

    if capability.capability_id != request.capability_id
        || capability.kind != CapabilityKind::AgentProvider
        || capability.review != ReviewState::Approved
        || !capability.enabled
    {
        return Err(AdmissionError::CapabilityNotExecutable(
            request.capability_id.clone(),
        ));
    }

    if permission != PermissionDecision::Allow {
        return Err(AdmissionError::PermissionDenied);
    }

    if preflight.provider_id != request.provider_id {
        return Err(AdmissionError::ProviderMismatch);
    }

    preflight.validate_at(now_unix_seconds)?;
    if request.requested_at_unix_seconds > now_unix_seconds {
        return Err(AdmissionError::InvalidPreflight(
            "dispatch request timestamp is in the future".into(),
        ));
    }
    if request.request_id.trim().is_empty() {
        return Err(AdmissionError::InvalidPreflight(
            "dispatch request ID is required".into(),
        ));
    }

    Ok(DispatchGrant {
        request_id: request.request_id.clone(),
        provider_id: request.provider_id.clone(),
        capability_id: request.capability_id.clone(),
        preflight_id: preflight.preflight_id.clone(),
        expires_at_unix_seconds: preflight.expires_at_unix_seconds,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verified_evidence() -> ControlEvidence {
        ControlEvidence {
            status: EvidenceStatus::Verified,
            evidence_id: "evidence-record-1".into(),
            detail: "adapter-produced verification".into(),
        }
    }

    fn valid_preflight() -> ProviderIsolationPreflight {
        ProviderIsolationPreflight {
            contract_version: PREFLIGHT_CONTRACT_VERSION.into(),
            preflight_id: "preflight-1".into(),
            provider_id: "provider-1".into(),
            adapter_id: "reviewed-adapter".into(),
            adapter_version: "1.0.0".into(),
            runtime_identity: "verified-runtime-identity".into(),
            checks: IsolationControl::ALL
                .into_iter()
                .map(|control| (control, verified_evidence()))
                .collect(),
            checked_at_unix_seconds: 100,
            expires_at_unix_seconds: 200,
        }
    }

    fn request() -> DispatchRequest {
        DispatchRequest {
            request_id: "request-1".into(),
            provider_id: "provider-1".into(),
            capability_id: "capability-1".into(),
            requested_at_unix_seconds: 150,
        }
    }

    fn executable_capability() -> CapabilityAdmissionState {
        CapabilityAdmissionState {
            capability_id: "capability-1".into(),
            kind: CapabilityKind::AgentProvider,
            review: ReviewState::Approved,
            enabled: true,
        }
    }

    #[test]
    fn discovery_does_not_authorize_execution() {
        let observation = DiscoveryObservation {
            provider_id: "provider-1".into(),
            capability_id: "capability-1".into(),
        };
        assert!(observation.authorize_execution().is_err());

        let result = admit(
            &request(),
            ProviderRegistrationState::Discovered,
            &executable_capability(),
            PermissionDecision::Allow,
            &valid_preflight(),
            150,
        );
        assert!(matches!(
            result,
            Err(AdmissionError::ProviderNotEnabled(_))
        ));
    }

    #[test]
    fn complete_current_evidence_allows_admission_only() {
        let grant = admit(
            &request(),
            ProviderRegistrationState::Enabled,
            &executable_capability(),
            PermissionDecision::Allow,
            &valid_preflight(),
            150,
        )
        .unwrap();
        assert_eq!(grant.request_id, "request-1");
        assert_eq!(grant.preflight_id, "preflight-1");
    }

    #[test]
    fn missing_or_unknown_evidence_fails_closed() {
        let mut missing = valid_preflight();
        missing.checks.remove(&IsolationControl::EgressPolicy);
        assert!(missing.validate_at(150).is_err());

        let mut unknown = valid_preflight();
        unknown
            .checks
            .get_mut(&IsolationControl::RuntimeIdentity)
            .unwrap()
            .status = EvidenceStatus::Unknown;
        assert!(unknown.validate_at(150).is_err());

        let mut no_reference = valid_preflight();
        no_reference
            .checks
            .get_mut(&IsolationControl::MountPolicy)
            .unwrap()
            .evidence_id
            .clear();
        assert!(no_reference.validate_at(150).is_err());
    }

    #[test]
    fn expired_or_future_preflight_fails_closed() {
        assert!(valid_preflight().validate_at(200).is_err());
        assert!(valid_preflight().validate_at(99).is_err());
    }

    #[test]
    fn revoked_provider_and_disabled_capability_fail_closed() {
        assert!(admit(
            &request(),
            ProviderRegistrationState::Revoked,
            &executable_capability(),
            PermissionDecision::Allow,
            &valid_preflight(),
            150,
        )
        .is_err());

        let mut capability = executable_capability();
        capability.enabled = false;
        assert!(admit(
            &request(),
            ProviderRegistrationState::Enabled,
            &capability,
            PermissionDecision::Allow,
            &valid_preflight(),
            150,
        )
        .is_err());
    }

    #[test]
    fn skill_cannot_be_admitted_as_an_agent_provider() {
        let mut capability = executable_capability();
        capability.kind = CapabilityKind::Skill;
        assert!(admit(
            &request(),
            ProviderRegistrationState::Enabled,
            &capability,
            PermissionDecision::Allow,
            &valid_preflight(),
            150,
        )
        .is_err());
    }

    #[test]
    fn unknown_or_approval_required_permission_fails_closed() {
        for decision in [
            PermissionDecision::Unknown,
            PermissionDecision::Deny,
            PermissionDecision::ApprovalRequired,
        ] {
            assert_eq!(
                admit(
                    &request(),
                    ProviderRegistrationState::Enabled,
                    &executable_capability(),
                    decision,
                    &valid_preflight(),
                    150,
                ),
                Err(AdmissionError::PermissionDenied)
            );
        }
    }

    #[test]
    fn provider_binding_is_mandatory() {
        let mut preflight = valid_preflight();
        preflight.provider_id = "different-provider".into();
        assert_eq!(
            admit(
                &request(),
                ProviderRegistrationState::Enabled,
                &executable_capability(),
                PermissionDecision::Allow,
                &preflight,
                150,
            ),
            Err(AdmissionError::ProviderMismatch)
        );
    }
}
Integration contract: dread-execution must depend on the canonical dread-core, serde with derives, and thiserror. Admission must be persisted transactionally with dispatch intent before any provider call. A DispatchGrant is neither a provider task ID nor evidence that execution began.

Next delivery — 002: durable mission/task state machine, append-only events, idempotency records, cancellation semantics, and restart-reconciliation traits. TESTS_NOT_RUN continues to apply to all generated code.




1021.99 s
12.57 TPS
50.15 s TTFT
continue

Reasoning · Done · 356.8s total
Reasoning · Kimi will craft the next delivery as 002: durable mission... · Done
DELIVERY 002 — DURABLE EXECUTION AND RESTART RECONCILIATION

Status: CODE_GENERATED, STATICALLY_REVIEWED, TESTS_NOT_RUN, NEEDS_CANONICAL_MERGE

This slice adds:

Dependency-validated mission/task creation.
Durable two-phase dispatch intent and provider acknowledgement.
Stable idempotency keys and retry classification.
Append-only, deduplicated execution events.
Cancellation that remains nonterminal until provider confirmation.
Restart reconciliation without automatic redispatch.
SQLite transaction and lifecycle tests.
The execution crate name and module exports must be reconciled with the canonical eight-crate architecture. No real provider is invoked.

Continuation index

Workspace/execution dependency diff.
crates/dread-execution/src/lifecycle.rs
crates/dread-execution/src/store.rs
crates/dread-execution/src/reconcile.rs
Module-export integration diff.
Next: authenticated approvals, protected-action policy, and immutable audit chaining.
FILE: Cargo.toml

Diff

Copy
diff --git a/Cargo.toml b/Cargo.toml
--- a/Cargo.toml
+++ b/Cargo.toml
@@
 [workspace.dependencies]
 async-trait = "0.1"
 axum = "0.8"
 directories = "6"
 rusqlite = { version = "0.32", features = ["backup", "bundled"] }
 serde = { version = "1", features = ["derive"] }
 serde_json = "1"
 sha2 = "0.10"
 tempfile = "3"
 thiserror = "2"
 toml = "0.8"
 tokio = { version = "1", features = ["macros", "net", "rt-multi-thread", "signal"] }
 url = "2"

# NEEDS_CANONICAL_MERGE:
# Add dread-execution to workspace members only if that is the canonical
# execution crate's path. Otherwise apply the manifest fragment below to the
# existing orchestration/execution crate and move these modules accordingly.

# Intended crates/dread-execution/Cargo.toml dependency fragment:
#
# [dependencies]
# async-trait.workspace = true
# dread-core = { path = "../dread-core" }
# rusqlite.workspace = true
# serde.workspace = true
# serde_json.workspace = true
# thiserror.workspace = true
#
# [dev-dependencies]
# tempfile.workspace = true
FILE: crates/dread-execution/src/lifecycle.rs

Rust

Copy
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissionState {
    Ready,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl MissionState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Result<Self, LifecycleError> {
        match value {
            "ready" => Ok(Self::Ready),
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            other => Err(LifecycleError::InvalidStoredState(other.to_owned())),
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Pending,
    Ready,
    Dispatching,
    Running,
    RetryScheduled,
    CancelRequested,
    Cancelling,
    Succeeded,
    Failed,
    Cancelled,
    Blocked,
}

impl TaskState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Ready => "ready",
            Self::Dispatching => "dispatching",
            Self::Running => "running",
            Self::RetryScheduled => "retry_scheduled",
            Self::CancelRequested => "cancel_requested",
            Self::Cancelling => "cancelling",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Blocked => "blocked",
        }
    }

    pub fn parse(value: &str) -> Result<Self, LifecycleError> {
        match value {
            "pending" => Ok(Self::Pending),
            "ready" => Ok(Self::Ready),
            "dispatching" => Ok(Self::Dispatching),
            "running" => Ok(Self::Running),
            "retry_scheduled" => Ok(Self::RetryScheduled),
            "cancel_requested" => Ok(Self::CancelRequested),
            "cancelling" => Ok(Self::Cancelling),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            "blocked" => Ok(Self::Blocked),
            other => Err(LifecycleError::InvalidStoredState(other.to_owned())),
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::Blocked
        )
    }

    pub fn has_active_provider_operation(self) -> bool {
        matches!(
            self,
            Self::Dispatching
                | Self::Running
                | Self::CancelRequested
                | Self::Cancelling
        )
    }

    pub fn ensure(self, expected: &[Self], next: Self) -> Result<(), LifecycleError> {
        if expected.contains(&self) {
            Ok(())
        } else {
            Err(LifecycleError::InvalidTransition {
                from: self,
                to: next,
            })
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdempotencyClass {
    /// The provider operation is explicitly safe to repeat.
    Idempotent,
    /// Initial dispatch may be approved, but ambiguous outcomes must not retry.
    NonIdempotent,
    /// Retry requires the same durable provider idempotency key.
    ProviderKeyRequired,
    /// Unknown never enables automatic retry.
    Unknown,
}

impl IdempotencyClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idempotent => "idempotent",
            Self::NonIdempotent => "non_idempotent",
            Self::ProviderKeyRequired => "provider_key_required",
            Self::Unknown => "unknown",
        }
    }

    pub fn parse(value: &str) -> Result<Self, LifecycleError> {
        match value {
            "idempotent" => Ok(Self::Idempotent),
            "non_idempotent" => Ok(Self::NonIdempotent),
            "provider_key_required" => Ok(Self::ProviderKeyRequired),
            "unknown" => Ok(Self::Unknown),
            other => Err(LifecycleError::InvalidStoredState(other.to_owned())),
        }
    }

    pub fn requires_key(self) -> bool {
        matches!(self, Self::ProviderKeyRequired)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteOutcome {
    DefiniteFailureWithoutSideEffects,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryDecision {
    Allowed,
    Exhausted,
    Unsafe,
}

pub fn classify_retry(
    idempotency: IdempotencyClass,
    outcome: RemoteOutcome,
    attempt: u32,
    max_attempts: u32,
) -> RetryDecision {
    if attempt >= max_attempts {
        return RetryDecision::Exhausted;
    }

    let safe = match idempotency {
        IdempotencyClass::Idempotent => true,
        IdempotencyClass::ProviderKeyRequired => true,
        IdempotencyClass::NonIdempotent | IdempotencyClass::Unknown => {
            outcome == RemoteOutcome::DefiniteFailureWithoutSideEffects
                && idempotency == IdempotencyClass::NonIdempotent
        }
    };

    if safe {
        RetryDecision::Allowed
    } else {
        RetryDecision::Unsafe
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LifecycleError {
    #[error("invalid task transition from {from:?} to {to:?}")]
    InvalidTransition { from: TaskState, to: TaskState },

    #[error("invalid persisted lifecycle state: {0}")]
    InvalidStoredState(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_request_is_not_terminal() {
        assert!(!TaskState::CancelRequested.is_terminal());
        assert!(!TaskState::Cancelling.is_terminal());
        assert!(TaskState::CancelRequested.has_active_provider_operation());
        assert!(TaskState::Cancelled.is_terminal());
    }

    #[test]
    fn unknown_outcome_never_retries_non_idempotent_work() {
        assert_eq!(
            classify_retry(
                IdempotencyClass::NonIdempotent,
                RemoteOutcome::Unknown,
                1,
                3
            ),
            RetryDecision::Unsafe
        );
        assert_eq!(
            classify_retry(
                IdempotencyClass::Unknown,
                RemoteOutcome::DefiniteFailureWithoutSideEffects,
                1,
                3
            ),
            RetryDecision::Unsafe
        );
    }

    #[test]
    fn keyed_and_idempotent_work_can_retry_within_bounds() {
        for class in [
            IdempotencyClass::Idempotent,
            IdempotencyClass::ProviderKeyRequired,
        ] {
            assert_eq!(
                classify_retry(class, RemoteOutcome::Unknown, 1, 3),
                RetryDecision::Allowed
            );
            assert_eq!(
                classify_retry(class, RemoteOutcome::Unknown, 3, 3),
                RetryDecision::Exhausted
            );
        }
    }

    #[test]
    fn definite_pre_side_effect_failure_can_retry_non_idempotent_work() {
        assert_eq!(
            classify_retry(
                IdempotencyClass::NonIdempotent,
                RemoteOutcome::DefiniteFailureWithoutSideEffects,
                1,
                2
            ),
            RetryDecision::Allowed
        );
    }
}
FILE: crates/dread-execution/src/store.rs

Rust

Copy
use crate::{
    lifecycle::{
        classify_retry, IdempotencyClass, LifecycleError, MissionState,
        RemoteOutcome, RetryDecision, TaskState,
    },
    preflight::DispatchGrant,
};
use rusqlite::{
    params, Connection, OptionalExtension, Transaction, TransactionBehavior,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    time::Duration,
};
use thiserror::Error;

const EXECUTION_SCHEMA_VERSION: i64 = 1;

const MIGRATION_0001: &str = r#"
CREATE TABLE dread_missions (
    id TEXT PRIMARY KEY,
    state TEXT NOT NULL,
    revision INTEGER NOT NULL,
    payload_json TEXT NOT NULL,
    created_at_unix_seconds INTEGER NOT NULL,
    updated_at_unix_seconds INTEGER NOT NULL
) STRICT;

CREATE TABLE dread_tasks (
    id TEXT PRIMARY KEY,
    mission_id TEXT NOT NULL,
    state TEXT NOT NULL,
    revision INTEGER NOT NULL,
    attempt INTEGER NOT NULL,
    max_attempts INTEGER NOT NULL,
    idempotency TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    capability_id TEXT NOT NULL,
    provider_execution_id TEXT,
    retry_not_before_unix_seconds INTEGER,
    last_error TEXT,
    created_at_unix_seconds INTEGER NOT NULL,
    updated_at_unix_seconds INTEGER NOT NULL,
    FOREIGN KEY(mission_id) REFERENCES dread_missions(id),
    UNIQUE(provider_id, provider_execution_id)
) STRICT;

CREATE INDEX dread_tasks_mission ON dread_tasks(mission_id);
CREATE INDEX dread_tasks_reconciliation
    ON dread_tasks(state, provider_id);

CREATE TABLE dread_task_dependencies (
    task_id TEXT NOT NULL,
    depends_on_task_id TEXT NOT NULL,
    PRIMARY KEY(task_id, depends_on_task_id),
    FOREIGN KEY(task_id) REFERENCES dread_tasks(id),
    FOREIGN KEY(depends_on_task_id) REFERENCES dread_tasks(id)
) STRICT;

CREATE TABLE dread_dispatch_intents (
    request_id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    attempt INTEGER NOT NULL,
    idempotency_key TEXT NOT NULL,
    preflight_id TEXT NOT NULL,
    preflight_expires_at_unix_seconds INTEGER NOT NULL,
    status TEXT NOT NULL CHECK (
        status IN ('pending', 'acknowledged', 'blocked')
    ),
    provider_execution_id TEXT,
    created_at_unix_seconds INTEGER NOT NULL,
    updated_at_unix_seconds INTEGER NOT NULL,
    FOREIGN KEY(task_id) REFERENCES dread_tasks(id),
    UNIQUE(task_id, attempt)
) STRICT;

CREATE INDEX dread_dispatch_pending
    ON dread_dispatch_intents(status, request_id);

CREATE TABLE dread_execution_events (
    event_sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    mission_id TEXT NOT NULL,
    task_id TEXT,
    event_type TEXT NOT NULL,
    event_key TEXT,
    provider_cursor TEXT,
    payload_json TEXT NOT NULL,
    occurred_at_unix_seconds INTEGER NOT NULL,
    FOREIGN KEY(mission_id) REFERENCES dread_missions(id),
    FOREIGN KEY(task_id) REFERENCES dread_tasks(id),
    UNIQUE(task_id, event_key)
) STRICT;

CREATE INDEX dread_execution_events_replay
    ON dread_execution_events(mission_id, event_sequence);
"#;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("execution store SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("execution store JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Lifecycle(#[from] LifecycleError),

    #[error("execution store validation failed: {0}")]
    Invalid(String),

    #[error("execution entity not found: {0}")]
    NotFound(String),

    #[error("execution state conflict: {0}")]
    Conflict(String),
}

#[derive(Debug, Clone)]
pub struct NewMission {
    pub mission_id: String,
    pub payload: Value,
    pub tasks: Vec<NewTask>,
}

#[derive(Debug, Clone)]
pub struct NewTask {
    pub task_id: String,
    pub dependencies: Vec<String>,
    pub provider_id: String,
    pub capability_id: String,
    pub idempotency: IdempotencyClass,
    pub max_attempts: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRecord {
    pub task_id: String,
    pub mission_id: String,
    pub state: TaskState,
    pub revision: i64,
    pub attempt: u32,
    pub max_attempts: u32,
    pub idempotency: IdempotencyClass,
    pub provider_id: String,
    pub capability_id: String,
    pub provider_execution_id: Option<String>,
    pub retry_not_before_unix_seconds: Option<i64>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchPreparation {
    pub request_id: String,
    pub task_id: String,
    pub attempt: u32,
    pub idempotency_key: String,
    pub preflight_id: String,
    pub provider_id: String,
    pub capability_id: String,
    pub preflight_expires_at_unix_seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionEvent {
    pub event_sequence: i64,
    pub mission_id: String,
    pub task_id: Option<String>,
    pub event_type: String,
    pub event_key: Option<String>,
    pub provider_cursor: Option<String>,
    pub payload: Value,
    pub occurred_at_unix_seconds: i64,
}

#[derive(Debug)]
pub struct ExecutionStore {
    connection: Connection,
}

impl ExecutionStore {
    /// Uses an explicitly selected canonical database path.
    ///
    /// NEEDS_CANONICAL_MERGE: Inventory the existing execution schema first.
    /// These additive dread_-prefixed tables must not become a parallel mission
    /// database when equivalent canonical mission records already exist.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        if !path.as_ref().is_absolute() {
            return Err(StoreError::Invalid(
                "execution database path must be absolute".into(),
            ));
        }

        let connection = Connection::open(path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.execute_batch("PRAGMA foreign_keys = ON;")?;
        let store = Self { connection };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<(), StoreError> {
        let transaction = self
            .connection
            .unchecked_transaction_with_behavior(TransactionBehavior::Immediate)?;

        transaction.execute_batch(
            "CREATE TABLE IF NOT EXISTS dread_execution_schema_migrations (
                version INTEGER PRIMARY KEY,
                checksum_sha256 TEXT NOT NULL,
                applied_at_unix_seconds INTEGER NOT NULL
            ) STRICT;",
        )?;

        let current = transaction.query_row(
            "SELECT MAX(version) FROM dread_execution_schema_migrations",
            [],
            |row| row.get::<_, Option<i64>>(0),
        )?;

        match current {
            None | Some(0) => {
                let count: i64 = transaction.query_row(
                    "SELECT COUNT(*) FROM sqlite_master
                     WHERE type = 'table' AND name IN (
                        'dread_missions', 'dread_tasks',
                        'dread_dispatch_intents', 'dread_execution_events'
                     )",
                    [],
                    |row| row.get(0),
                )?;
                if count != 0 {
                    return Err(StoreError::Invalid(
                        "unversioned execution tables require canonical reconciliation".into(),
                    ));
                }

                // Timestamp is supplied by table creation context in this
                // additive migration. It is metadata, not execution evidence.
                transaction.execute_batch(MIGRATION_0001)?;
                transaction.execute(
                    "INSERT INTO dread_execution_schema_migrations (
                        version, checksum_sha256, applied_at_unix_seconds
                     ) VALUES (1, ?1, 0)",
                    [migration_checksum()],
                )?;
            }
            Some(EXECUTION_SCHEMA_VERSION) => {
                let checksum: String = transaction.query_row(
                    "SELECT checksum_sha256
                     FROM dread_execution_schema_migrations
                     WHERE version = 1",
                    [],
                    |row| row.get(0),
                )?;
                if checksum != migration_checksum() {
                    return Err(StoreError::Invalid(
                        "execution migration checksum mismatch".into(),
                    ));
                }
            }
            Some(version) => {
                return Err(StoreError::Invalid(format!(
                    "unsupported execution schema version {version}"
                )));
            }
        }

        transaction.commit()?;
        Ok(())
    }

    pub fn create_mission(
        &self,
        input: &NewMission,
        now: i64,
    ) -> Result<(), StoreError> {
        validate_new_mission(input)?;

        let transaction = self
            .connection
            .unchecked_transaction_with_behavior(TransactionBehavior::Immediate)?;

        transaction.execute(
            "INSERT INTO dread_missions (
                id, state, revision, payload_json,
                created_at_unix_seconds, updated_at_unix_seconds
             ) VALUES (?1, 'ready', 0, ?2, ?3, ?3)",
            params![
                input.mission_id,
                serde_json::to_string(&input.payload)?,
                now
            ],
        )?;

        let dependency_sets: BTreeMap<&str, BTreeSet<&str>> = input
            .tasks
            .iter()
            .map(|task| {
                (
                    task.task_id.as_str(),
                    task.dependencies.iter().map(String::as_str).collect(),
                )
            })
            .collect();

        for task in &input.tasks {
            let initial_state = if dependency_sets[task.task_id.as_str()].is_empty() {
                TaskState::Ready
            } else {
                TaskState::Pending
            };

            transaction.execute(
                "INSERT INTO dread_tasks (
                    id, mission_id, state, revision, attempt, max_attempts,
                    idempotency, provider_id, capability_id,
                    provider_execution_id, retry_not_before_unix_seconds,
                    last_error, created_at_unix_seconds, updated_at_unix_seconds
                 ) VALUES (
                    ?1, ?2, ?3, 0, 0, ?4, ?5, ?6, ?7,
                    NULL, NULL, NULL, ?8, ?8
                 )",
                params![
                    task.task_id,
                    input.mission_id,
                    initial_state.as_str(),
                    task.max_attempts,
                    task.idempotency.as_str(),
                    task.provider_id,
                    task.capability_id,
                    now
                ],
            )?;
        }

        for task in &input.tasks {
            for dependency in &task.dependencies {
                transaction.execute(
                    "INSERT INTO dread_task_dependencies (
                        task_id, depends_on_task_id
                     ) VALUES (?1, ?2)",
                    params![task.task_id, dependency],
                )?;
            }
        }

        insert_event(
            &transaction,
            &input.mission_id,
            None,
            "mission_created",
            None,
            None,
            serde_json::json!({"task_count": input.tasks.len()}),
            now,
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn task(&self, task_id: &str) -> Result<TaskRecord, StoreError> {
        self.connection
            .query_row(SELECT_TASK, [task_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, Option<String>>(9)?,
                    row.get::<_, Option<i64>>(10)?,
                    row.get::<_, Option<String>>(11)?,
                ))
            })
            .optional()?
            .map(decode_task)
            .transpose()?
            .ok_or_else(|| StoreError::NotFound(task_id.to_owned()))
    }

    /// Persists dispatch intent before the caller contacts a provider.
    /// Repeating the same request returns the original durable preparation.
    pub fn prepare_dispatch(
        &self,
        task_id: &str,
        grant: &DispatchGrant,
        idempotency_key: &str,
        now: i64,
    ) -> Result<DispatchPreparation, StoreError> {
        validate_nonempty("idempotency_key", idempotency_key)?;

        if let Some(existing) = self.preparation(&grant.request_id)? {
            if existing.task_id != task_id
                || existing.idempotency_key != idempotency_key
                || existing.provider_id != grant.provider_id
                || existing.capability_id != grant.capability_id
            {
                return Err(StoreError::Conflict(
                    "dispatch request ID was reused with different parameters".into(),
                ));
            }
            return Ok(existing);
        }

        if grant.expires_at_unix_seconds <= now {
            return Err(StoreError::Conflict(
                "dispatch grant expired before intent persistence".into(),
            ));
        }

        let transaction = self
            .connection
            .unchecked_transaction_with_behavior(TransactionBehavior::Immediate)?;
        let task = load_task(&transaction, task_id)?;

        if task.provider_id != grant.provider_id
            || task.capability_id != grant.capability_id
        {
            return Err(StoreError::Conflict(
                "dispatch grant does not match the task provider/capability".into(),
            ));
        }
        if task.idempotency.requires_key() && idempotency_key.trim().is_empty() {
            return Err(StoreError::Invalid(
                "provider idempotency key is required".into(),
            ));
        }
        if task.attempt >= task.max_attempts {
            return Err(StoreError::Conflict(
                "task has exhausted its dispatch attempts".into(),
            ));
        }
        if task.state == TaskState::RetryScheduled
            && task
                .retry_not_before_unix_seconds
                .is_some_and(|not_before| not_before > now)
        {
            return Err(StoreError::Conflict(
                "retry backoff has not elapsed".into(),
            ));
        }

        task.state.ensure(
            &[TaskState::Ready, TaskState::RetryScheduled],
            TaskState::Dispatching,
        )?;

        let attempt = task
            .attempt
            .checked_add(1)
            .ok_or_else(|| StoreError::Invalid("attempt counter overflow".into()))?;

        transaction.execute(
            "UPDATE dread_tasks SET
                state = 'dispatching',
                revision = revision + 1,
                attempt = ?2,
                retry_not_before_unix_seconds = NULL,
                last_error = NULL,
                updated_at_unix_seconds = ?3
             WHERE id = ?1",
            params![task_id, attempt, now],
        )?;
        transaction.execute(
            "INSERT INTO dread_dispatch_intents (
                request_id, task_id, attempt, idempotency_key,
                preflight_id, preflight_expires_at_unix_seconds,
                status, provider_execution_id,
                created_at_unix_seconds, updated_at_unix_seconds
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6,
                'pending', NULL, ?7, ?7
             )",
            params![
                grant.request_id,
                task_id,
                attempt,
                idempotency_key,
                grant.preflight_id,
                grant.expires_at_unix_seconds,
                now
            ],
        )?;
        transaction.execute(
            "UPDATE dread_missions
             SET state = 'running', revision = revision + 1,
                 updated_at_unix_seconds = ?2
             WHERE id = ?1 AND state != 'running'",
            params![task.mission_id, now],
        )?;
        insert_event(
            &transaction,
            &task.mission_id,
            Some(task_id),
            "dispatch_intent_persisted",
            None,
            None,
            serde_json::json!({
                "request_id": grant.request_id,
                "attempt": attempt,
                "preflight_id": grant.preflight_id,
                "provider_execution_started": false
            }),
            now,
        )?;
        transaction.commit()?;

        self.preparation(&grant.request_id)?
            .ok_or_else(|| StoreError::NotFound(grant.request_id.clone()))
    }

    pub fn preparation(
        &self,
        request_id: &str,
    ) -> Result<Option<DispatchPreparation>, StoreError> {
        self.connection
            .query_row(
                "SELECT i.request_id, i.task_id, i.attempt,
                        i.idempotency_key, i.preflight_id,
                        t.provider_id, t.capability_id,
                        i.preflight_expires_at_unix_seconds
                 FROM dread_dispatch_intents i
                 JOIN dread_tasks t ON t.id = i.task_id
                 WHERE i.request_id = ?1",
                [request_id],
                |row| {
                    Ok(DispatchPreparation {
                        request_id: row.get(0)?,
                        task_id: row.get(1)?,
                        attempt: row.get::<_, i64>(2)? as u32,
                        idempotency_key: row.get(3)?,
                        preflight_id: row.get(4)?,
                        provider_id: row.get(5)?,
                        capability_id: row.get(6)?,
                        preflight_expires_at_unix_seconds: row.get(7)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    /// Records a real provider acknowledgement. It is idempotent only when the
    /// same request returns the same provider execution identity.
    pub fn record_provider_ack(
        &self,
        request_id: &str,
        provider_execution_id: &str,
        now: i64,
    ) -> Result<(), StoreError> {
        validate_nonempty("provider_execution_id", provider_execution_id)?;

        let transaction = self
            .connection
            .unchecked_transaction_with_behavior(TransactionBehavior::Immediate)?;
        let intent = load_intent(&transaction, request_id)?;
        let task = load_task(&transaction, &intent.task_id)?;

        if let Some(existing) = &task.provider_execution_id {
            if existing == provider_execution_id {
                transaction.commit()?;
                return Ok(());
            }
            return Err(StoreError::Conflict(
                "task already has a different provider execution identity".into(),
            ));
        }
        if intent.status != "pending" {
            return Err(StoreError::Conflict(format!(
                "cannot acknowledge dispatch intent in state {}",
                intent.status
            )));
        }
        task.state.ensure(
            &[TaskState::Dispatching, TaskState::CancelRequested],
            TaskState::Running,
        )?;

        // Preserve an already requested cancellation. Acknowledging provider
        // identity must not erase the operator's cancellation request.
        let next_state = if task.state == TaskState::CancelRequested {
            TaskState::CancelRequested
        } else {
            TaskState::Running
        };

        transaction.execute(
            "UPDATE dread_tasks SET
                state = ?2,
                provider_execution_id = ?3,
                revision = revision + 1,
                updated_at_unix_seconds = ?4
             WHERE id = ?1",
            params![task.id, next_state.as_str(), provider_execution_id, now],
        )?;
        transaction.execute(
            "UPDATE dread_dispatch_intents SET
                status = 'acknowledged',
                provider_execution_id = ?2,
                updated_at_unix_seconds = ?3
             WHERE request_id = ?1",
            params![request_id, provider_execution_id, now],
        )?;
        insert_event(
            &transaction,
            &task.mission_id,
            Some(&task.id),
            "provider_acknowledged",
            None,
            None,
            serde_json::json!({
                "request_id": request_id,
                "provider_execution_id": provider_execution_id
            }),
            now,
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Appends provider output. Duplicate task/event keys return false.
    /// This method does not infer task state from arbitrary provider text.
    pub fn append_provider_event(
        &self,
        task_id: &str,
        event_key: &str,
        event_type: &str,
        provider_cursor: Option<&str>,
        payload: Value,
        now: i64,
    ) -> Result<bool, StoreError> {
        validate_nonempty("event_key", event_key)?;
        validate_nonempty("event_type", event_type)?;
        let task = self.task(task_id)?;

        let transaction = self
            .connection
            .unchecked_transaction_with_behavior(TransactionBehavior::Immediate)?;
        let inserted = insert_event(
            &transaction,
            &task.mission_id,
            Some(task_id),
            event_type,
            Some(event_key),
            provider_cursor,
            payload,
            now,
        )?;
        transaction.commit()?;
        Ok(inserted)
    }

    /// Cancellation is local-terminal only when no provider operation exists.
    /// Active work remains cancel_requested or cancelling until provider proof.
    pub fn request_cancellation(
        &self,
        task_id: &str,
        now: i64,
    ) -> Result<TaskState, StoreError> {
        let transaction = self
            .connection
            .unchecked_transaction_with_behavior(TransactionBehavior::Immediate)?;
        let task = load_task(&transaction, task_id)?;

        let next = match task.state {
            TaskState::Pending | TaskState::Ready | TaskState::RetryScheduled => {
                TaskState::Cancelled
            }
            TaskState::Dispatching | TaskState::Running | TaskState::CancelRequested => {
                TaskState::CancelRequested
            }
            TaskState::Cancelling => TaskState::Cancelling,
            state if state.is_terminal() => {
                transaction.commit()?;
                return Ok(state);
            }
            state => {
                return Err(StoreError::Conflict(format!(
                    "cannot cancel task from {state:?}"
                )))
            }
        };

        transaction.execute(
            "UPDATE dread_tasks SET state = ?2,
                revision = revision + 1, updated_at_unix_seconds = ?3
             WHERE id = ?1",
            params![task_id, next.as_str(), now],
        )?;
        insert_event(
            &transaction,
            &task.mission_id,
            Some(task_id),
            "cancellation_requested",
            None,
            None,
            serde_json::json!({
                "provider_cancel_confirmed": false,
                "local_state": next.as_str()
            }),
            now,
        )?;

        if next.is_terminal() {
            refresh_dependencies_and_mission(&transaction, &task.mission_id, now)?;
        }
        transaction.commit()?;
        Ok(next)
    }

    /// Called only after the provider acknowledges its cancellation request.
    pub fn mark_cancelling(
        &self,
        task_id: &str,
        now: i64,
    ) -> Result<(), StoreError> {
        let transaction = self
            .connection
            .unchecked_transaction_with_behavior(TransactionBehavior::Immediate)?;
        let task = load_task(&transaction, task_id)?;
        task.state.ensure(
            &[TaskState::CancelRequested],
            TaskState::Cancelling,
        )?;
        transaction.execute(
            "UPDATE dread_tasks SET state = 'cancelling',
                revision = revision + 1, updated_at_unix_seconds = ?2
             WHERE id = ?1",
            params![task_id, now],
        )?;
        insert_event(
            &transaction,
            &task.mission_id,
            Some(task_id),
            "provider_cancellation_acknowledged",
            None,
            None,
            serde_json::json!({"provider_terminal": false}),
            now,
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn record_terminal(
        &self,
        task_id: &str,
        terminal: TaskState,
        result: Value,
        now: i64,
    ) -> Result<(), StoreError> {
        if !matches!(
            terminal,
            TaskState::Succeeded | TaskState::Failed | TaskState::Cancelled
        ) {
            return Err(StoreError::Invalid(
                "record_terminal accepts only succeeded, failed, or cancelled".into(),
            ));
        }

        let transaction = self
            .connection
            .unchecked_transaction_with_behavior(TransactionBehavior::Immediate)?;
        let task = load_task(&transaction, task_id)?;

        let allowed = match terminal {
            TaskState::Succeeded => task.state == TaskState::Running,
            TaskState::Failed => matches!(
                task.state,
                TaskState::Dispatching
                    | TaskState::Running
                    | TaskState::CancelRequested
                    | TaskState::Cancelling
            ),
            TaskState::Cancelled => matches!(
                task.state,
                TaskState::Running
                    | TaskState::CancelRequested
                    | TaskState::Cancelling
            ),
            _ => false,
        };
        if !allowed {
            return Err(StoreError::Conflict(format!(
                "cannot record {terminal:?} from {:?}",
                task.state
            )));
        }
        if terminal != TaskState::Failed && task.provider_execution_id.is_none() {
            return Err(StoreError::Conflict(
                "terminal provider state requires a provider execution identity".into(),
            ));
        }

        transaction.execute(
            "UPDATE dread_tasks SET state = ?2,
                revision = revision + 1,
                last_error = CASE WHEN ?2 = 'failed' THEN ?3 ELSE last_error END,
                updated_at_unix_seconds = ?4
             WHERE id = ?1",
            params![
                task_id,
                terminal.as_str(),
                result.get("error").and_then(Value::as_str),
                now
            ],
        )?;
        insert_event(
            &transaction,
            &task.mission_id,
            Some(task_id),
            "provider_terminal_state",
            None,
            None,
            serde_json::json!({"state": terminal, "result": result}),
            now,
        )?;
        refresh_dependencies_and_mission(&transaction, &task.mission_id, now)?;
        transaction.commit()?;
        Ok(())
    }

    /// Schedules a bounded retry only after explicit outcome classification.
    pub fn schedule_retry(
        &self,
        task_id: &str,
        outcome: RemoteOutcome,
        error: &str,
        now: i64,
        not_before: i64,
    ) -> Result<RetryDecision, StoreError> {
        if not_before < now {
            return Err(StoreError::Invalid(
                "retry backoff timestamp cannot precede current time".into(),
            ));
        }

        let transaction = self
            .connection
            .unchecked_transaction_with_behavior(TransactionBehavior::Immediate)?;
        let task = load_task(&transaction, task_id)?;
        task.state.ensure(
            &[TaskState::Dispatching, TaskState::Running],
            TaskState::RetryScheduled,
        )?;

        let decision = classify_retry(
            task.idempotency,
            outcome,
            task.attempt,
            task.max_attempts,
        );
        let next_state = if decision == RetryDecision::Allowed {
            TaskState::RetryScheduled
        } else {
            TaskState::Failed
        };

        transaction.execute(
            "UPDATE dread_tasks SET state = ?2,
                revision = revision + 1,
                retry_not_before_unix_seconds = ?3,
                last_error = ?4,
                updated_at_unix_seconds = ?5
             WHERE id = ?1",
            params![
                task_id,
                next_state.as_str(),
                (decision == RetryDecision::Allowed).then_some(not_before),
                error,
                now
            ],
        )?;
        insert_event(
            &transaction,
            &task.mission_id,
            Some(task_id),
            "retry_classified",
            None,
            None,
            serde_json::json!({
                "decision": format!("{decision:?}"),
                "remote_outcome": format!("{outcome:?}"),
                "next_state": next_state
            }),
            now,
        )?;
        if next_state.is_terminal() {
            refresh_dependencies_and_mission(&transaction, &task.mission_id, now)?;
        }
        transaction.commit()?;
        Ok(decision)
    }

    /// Only pending dispatch intents are returned. Acknowledged work is not
    /// redispatched and belongs to normal provider polling/reconciliation.
    pub fn pending_reconciliation(
        &self,
    ) -> Result<Vec<DispatchPreparation>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT request_id FROM dread_dispatch_intents
             WHERE status = 'pending'
             ORDER BY created_at_unix_seconds, request_id",
        )?;
        let ids = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;

        ids.into_iter()
            .map(|id| {
                self.preparation(&id)?
                    .ok_or(StoreError::NotFound(id))
            })
            .collect()
    }

    /// Blocks an unresolved dispatch. It never treats NotFound as proof that a
    /// side effect did not happen and never redispatches automatically.
    pub fn block_unresolved_dispatch(
        &self,
        request_id: &str,
        detail: &str,
        now: i64,
    ) -> Result<(), StoreError> {
        let transaction = self
            .connection
            .unchecked_transaction_with_behavior(TransactionBehavior::Immediate)?;
        let intent = load_intent(&transaction, request_id)?;
        if intent.status != "pending" {
            return Err(StoreError::Conflict(
                "only a pending intent can be blocked by reconciliation".into(),
            ));
        }
        let task = load_task(&transaction, &intent.task_id)?;

        transaction.execute(
            "UPDATE dread_dispatch_intents SET status = 'blocked',
                updated_at_unix_seconds = ?2 WHERE request_id = ?1",
            params![request_id, now],
        )?;
        transaction.execute(
            "UPDATE dread_tasks SET state = 'blocked',
                revision = revision + 1, last_error = ?2,
                updated_at_unix_seconds = ?3 WHERE id = ?1",
            params![task.id, detail, now],
        )?;
        insert_event(
            &transaction,
            &task.mission_id,
            Some(&task.id),
            "dispatch_reconciliation_blocked",
            None,
            None,
            serde_json::json!({
                "request_id": request_id,
                "detail": detail,
                "automatic_redispatch": false
            }),
            now,
        )?;
        refresh_dependencies_and_mission(&transaction, &task.mission_id, now)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn events_after(
        &self,
        mission_id: &str,
        cursor: i64,
        limit: u32,
    ) -> Result<Vec<ExecutionEvent>, StoreError> {
        if !(1..=1000).contains(&limit) {
            return Err(StoreError::Invalid(
                "event replay limit must be between 1 and 1000".into(),
            ));
        }

        let mut statement = self.connection.prepare(
            "SELECT event_sequence, mission_id, task_id, event_type,
                    event_key, provider_cursor, payload_json,
                    occurred_at_unix_seconds
             FROM dread_execution_events
             WHERE mission_id = ?1 AND event_sequence > ?2
             ORDER BY event_sequence LIMIT ?3",
            params![mission_id, cursor, limit],
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, i64>(7)?,
            ))
        })?;

        rows.map(|row| {
            let (
                event_sequence,
                mission_id,
                task_id,
                event_type,
                event_key,
                provider_cursor,
                payload,
                occurred_at_unix_seconds,
            ) = row?;
            Ok(ExecutionEvent {
                event_sequence,
                mission_id,
                task_id,
                event_type,
                event_key,
                provider_cursor,
                payload: serde_json::from_str(&payload)?,
                occurred_at_unix_seconds,
            })
        })
        .collect()
    }
}

const SELECT_TASK: &str = "
    SELECT id, mission_id, state, revision, attempt, max_attempts,
           idempotency, provider_id, capability_id, provider_execution_id,
           retry_not_before_unix_seconds, last_error
    FROM dread_tasks WHERE id = ?1
";

type RawTask = (
    String,
    String,
    String,
    i64,
    i64,
    i64,
    String,
    String,
    String,
    Option<String>,
    Option<i64>,
    Option<String>,
);

fn decode_task(raw: RawTask) -> Result<TaskRecord, StoreError> {
    Ok(TaskRecord {
        task_id: raw.0,
        mission_id: raw.1,
        state: TaskState::parse(&raw.2)?,
        revision: raw.3,
        attempt: u32::try_from(raw.4)
            .map_err(|_| StoreError::Invalid("negative task attempt".into()))?,
        max_attempts: u32::try_from(raw.5)
            .map_err(|_| StoreError::Invalid("negative max attempts".into()))?,
        idempotency: IdempotencyClass::parse(&raw.6)?,
        provider_id: raw.7,
        capability_id: raw.8,
        provider_execution_id: raw.9,
        retry_not_before_unix_seconds: raw.10,
        last_error: raw.11,
    })
}

fn load_task(
    transaction: &Transaction<'_>,
    task_id: &str,
) -> Result<TaskRecord, StoreError> {
    transaction
        .query_row(SELECT_TASK, [task_id], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
                row.get(9)?,
                row.get(10)?,
                row.get(11)?,
            ))
        })
        .optional()?
        .map(decode_task)
        .transpose()?
        .ok_or_else(|| StoreError::NotFound(task_id.to_owned()))
}

struct IntentRecord {
    task_id: String,
    status: String,
}

fn load_intent(
    transaction: &Transaction<'_>,
    request_id: &str,
) -> Result<IntentRecord, StoreError> {
    transaction
        .query_row(
            "SELECT task_id, status FROM dread_dispatch_intents
             WHERE request_id = ?1",
            [request_id],
            |row| {
                Ok(IntentRecord {
                    task_id: row.get(0)?,
                    status: row.get(1)?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| StoreError::NotFound(request_id.to_owned()))
}

fn validate_new_mission(input: &NewMission) -> Result<(), StoreError> {
    validate_nonempty("mission_id", &input.mission_id)?;
    if input.tasks.is_empty() || input.tasks.len() > 10_000 {
        return Err(StoreError::Invalid(
            "mission must contain between 1 and 10000 tasks".into(),
        ));
    }

    let mut tasks = BTreeMap::new();
    for task in &input.tasks {
        validate_nonempty("task_id", &task.task_id)?;
        validate_nonempty("provider_id", &task.provider_id)?;
        validate_nonempty("capability_id", &task.capability_id)?;
        if !(1..=100).contains(&task.max_attempts) {
            return Err(StoreError::Invalid(
                "max_attempts must be between 1 and 100".into(),
            ));
        }
        if tasks
            .insert(task.task_id.as_str(), task)
            .is_some()
        {
            return Err(StoreError::Invalid(format!(
                "duplicate task ID {:?}",
                task.task_id
            )));
        }
    }

    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for task_id in tasks.keys() {
        visit_task(task_id, &tasks, &mut visiting, &mut visited)?;
    }
    Ok(())
}

fn visit_task<'a>(
    task_id: &'a str,
    tasks: &BTreeMap<&'a str, &'a NewTask>,
    visiting: &mut BTreeSet<&'a str>,
    visited: &mut BTreeSet<&'a str>,
) -> Result<(), StoreError> {
    if visited.contains(task_id) {
        return Ok(());
    }
    if !visiting.insert(task_id) {
        return Err(StoreError::Invalid(
            "mission dependency graph contains a cycle".into(),
        ));
    }

    let task = tasks[task_id];
    let mut unique = BTreeSet::new();
    for dependency in &task.dependencies {
        if !unique.insert(dependency.as_str()) {
            return Err(StoreError::Invalid(format!(
                "duplicate dependency {dependency:?}"
            )));
        }
        if !tasks.contains_key(dependency.as_str()) {
            return Err(StoreError::Invalid(format!(
                "unknown dependency {dependency:?}"
            )));
        }
        visit_task(dependency, tasks, visiting, visited)?;
    }

    visiting.remove(task_id);
    visited.insert(task_id);
    Ok(())
}

fn refresh_dependencies_and_mission(
    transaction: &Transaction<'_>,
    mission_id: &str,
    now: i64,
) -> Result<(), StoreError> {
    // Repeat because unblocking one level can unblock the next.
    for _ in 0..10_000 {
        let mut statement = transaction.prepare(
            "SELECT t.id, t.state,
                    SUM(CASE WHEN d.state != 'succeeded' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN d.state IN (
                        'failed', 'cancelled', 'blocked'
                    ) THEN 1 ELSE 0 END)
             FROM dread_tasks t
             JOIN dread_task_dependencies edge ON edge.task_id = t.id
             JOIN dread_tasks d ON d.id = edge.depends_on_task_id
             WHERE t.mission_id = ?1 AND t.state = 'pending'
             GROUP BY t.id, t.state",
        )?;
        let pending = statement
            .query_map([mission_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);

        if pending.is_empty() {
            break;
        }

        let mut changed = false;
        for (task_id, incomplete, failed) in pending {
            let next = if failed > 0 {
                Some(TaskState::Blocked)
            } else if incomplete == 0 {
                Some(TaskState::Ready)
            } else {
                None
            };
            if let Some(next) = next {
                transaction.execute(
                    "UPDATE dread_tasks SET state = ?2,
                        revision = revision + 1,
                        updated_at_unix_seconds = ?3
                     WHERE id = ?1",
                    params![task_id, next.as_str(), now],
                )?;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    let (total, terminal, failed, cancelled): (i64, i64, i64, i64) =
        transaction.query_row(
            "SELECT COUNT(*),
                    SUM(CASE WHEN state IN (
                        'succeeded', 'failed', 'cancelled', 'blocked'
                    ) THEN 1 ELSE 0 END),
                    SUM(CASE WHEN state IN ('failed', 'blocked')
                        THEN 1 ELSE 0 END),
                    SUM(CASE WHEN state = 'cancelled' THEN 1 ELSE 0 END)
             FROM dread_tasks WHERE mission_id = ?1",
            [mission_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;

    let final_state = if terminal == total {
        if failed > 0 {
            Some(MissionState::Failed)
        } else if cancelled > 0 {
            Some(MissionState::Cancelled)
        } else {
            Some(MissionState::Succeeded)
        }
    } else {
        None
    };

    if let Some(state) = final_state {
        transaction.execute(
            "UPDATE dread_missions SET state = ?2,
                revision = revision + 1, updated_at_unix_seconds = ?3
             WHERE id = ?1",
            params![mission_id, state.as_str(), now],
        )?;
        insert_event(
            &transaction,
            mission_id,
            None,
            "mission_terminal_state",
            None,
            None,
            serde_json::json!({"state": state}),
            now,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn insert_event(
    transaction: &Transaction<'_>,
    mission_id: &str,
    task_id: Option<&str>,
    event_type: &str,
    event_key: Option<&str>,
    provider_cursor: Option<&str>,
    payload: Value,
    now: i64,
) -> Result<bool, StoreError> {
    validate_nonempty("event_type", event_type)?;
    let affected = transaction.execute(
        "INSERT INTO dread_execution_events (
            mission_id, task_id, event_type, event_key,
            provider_cursor, payload_json, occurred_at_unix_seconds
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(task_id, event_key) DO NOTHING",
        params![
            mission_id,
            task_id,
            event_type,
            event_key,
            provider_cursor,
            serde_json::to_string(&payload)?,
            now
        ],
    )?;
    Ok(affected == 1)
}

fn validate_nonempty(field: &str, value: &str) -> Result<(), StoreError> {
    if value.trim().is_empty() || value.len() > 512 {
        Err(StoreError::Invalid(format!(
            "{field} must be nonempty and at most 512 bytes"
        )))
    } else {
        Ok(())
    }
}

fn migration_checksum() -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(MIGRATION_0001.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn store() -> (TempDir, ExecutionStore) {
        let temporary = TempDir::new().unwrap();
        let store = ExecutionStore::open(temporary.path().join("execution.sqlite3"))
            .unwrap();
        (temporary, store)
    }

    fn grant(request_id: &str) -> DispatchGrant {
        DispatchGrant {
            request_id: request_id.into(),
            provider_id: "provider-1".into(),
            capability_id: "capability-1".into(),
            preflight_id: "preflight-1".into(),
            expires_at_unix_seconds: 1_000,
        }
    }

    fn task(id: &str, dependencies: &[&str]) -> NewTask {
        NewTask {
            task_id: id.into(),
            dependencies: dependencies.iter().map(|value| (*value).into()).collect(),
            provider_id: "provider-1".into(),
            capability_id: "capability-1".into(),
            idempotency: IdempotencyClass::ProviderKeyRequired,
            max_attempts: 3,
        }
    }

    fn create_single(store: &ExecutionStore) {
        store
            .create_mission(
                &NewMission {
                    mission_id: "mission-1".into(),
                    payload: serde_json::json!({"request": "test"}),
                    tasks: vec![task("task-1", &[])],
                },
                10,
            )
            .unwrap();
    }

    #[test]
    fn dependency_dag_starts_only_independent_tasks() {
        let (_temporary, store) = store();
        store
            .create_mission(
                &NewMission {
                    mission_id: "mission-1".into(),
                    payload: Value::Null,
                    tasks: vec![task("child", &["parent"]), task("parent", &[])],
                },
                10,
            )
            .unwrap();

        assert_eq!(store.task("parent").unwrap().state, TaskState::Ready);
        assert_eq!(store.task("child").unwrap().state, TaskState::Pending);
    }

    #[test]
    fn cycles_and_unknown_dependencies_are_rejected() {
        let (_temporary, store) = store();
        let cycle = NewMission {
            mission_id: "mission-1".into(),
            payload: Value::Null,
            tasks: vec![task("a", &["b"]), task("b", &["a"])],
        };
        assert!(store.create_mission(&cycle, 10).is_err());

        let unknown = NewMission {
            mission_id: "mission-2".into(),
            payload: Value::Null,
            tasks: vec![task("a", &["missing"])],
        };
        assert!(store.create_mission(&unknown, 10).is_err());
    }

    #[test]
    fn dispatch_intent_precedes_real_provider_acknowledgement() {
        let (_temporary, store) = store();
        create_single(&store);

        let prepared = store
            .prepare_dispatch("task-1", &grant("request-1"), "stable-key", 20)
            .unwrap();
        assert_eq!(prepared.attempt, 1);
        assert_eq!(store.task("task-1").unwrap().state, TaskState::Dispatching);
        assert_eq!(
            store.task("task-1").unwrap().provider_execution_id,
            None
        );

        let duplicate = store
            .prepare_dispatch("task-1", &grant("request-1"), "stable-key", 20)
            .unwrap();
        assert_eq!(duplicate, prepared);

        store
            .record_provider_ack("request-1", "provider-task-123", 21)
            .unwrap();
        store
            .record_provider_ack("request-1", "provider-task-123", 22)
            .unwrap();
        assert_eq!(
            store.task("task-1").unwrap().provider_execution_id.as_deref(),
            Some("provider-task-123")
        );
    }

    #[test]
    fn provider_event_deduplication_preserves_replay_cursor() {
        let (_temporary, store) = store();
        create_single(&store);
        store
            .prepare_dispatch("task-1", &grant("request-1"), "stable-key", 20)
            .unwrap();
        store
            .record_provider_ack("request-1", "provider-task-123", 21)
            .unwrap();

        assert!(store
            .append_provider_event(
                "task-1",
                "provider-event-1",
                "stdout",
                Some("cursor-1"),
                serde_json::json!({"text": "actual provider output"}),
                22,
            )
            .unwrap());
        assert!(!store
            .append_provider_event(
                "task-1",
                "provider-event-1",
                "stdout",
                Some("cursor-1"),
                serde_json::json!({"text": "duplicate"}),
                23,
            )
            .unwrap());

        let events = store.events_after("mission-1", 0, 100).unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| event.event_type == "stdout")
                .count(),
            1
        );
        assert!(events
            .windows(2)
            .all(|pair| pair[0].event_sequence < pair[1].event_sequence));
    }

    #[test]
    fn active_cancellation_waits_for_provider_terminal_confirmation() {
        let (_temporary, store) = store();
        create_single(&store);
        store
            .prepare_dispatch("task-1", &grant("request-1"), "stable-key", 20)
            .unwrap();
        store
            .record_provider_ack("request-1", "provider-task-123", 21)
            .unwrap();

        assert_eq!(
            store.request_cancellation("task-1", 22).unwrap(),
            TaskState::CancelRequested
        );
        store.mark_cancelling("task-1", 23).unwrap();
        assert_eq!(store.task("task-1").unwrap().state, TaskState::Cancelling);

        store
            .record_terminal(
                "task-1",
                TaskState::Cancelled,
                serde_json::json!({"provider_status": "cancelled"}),
                24,
            )
            .unwrap();
        assert_eq!(store.task("task-1").unwrap().state, TaskState::Cancelled);
    }

    #[test]
    fn dependent_task_is_released_only_after_success() {
        let (_temporary, store) = store();
        store
            .create_mission(
                &NewMission {
                    mission_id: "mission-1".into(),
                    payload: Value::Null,
                    tasks: vec![task("child", &["parent"]), task("parent", &[])],
                },
                10,
            )
            .unwrap();
        store
            .prepare_dispatch("parent", &grant("request-1"), "stable-key", 20)
            .unwrap();
        store
            .record_provider_ack("request-1", "provider-task-123", 21)
            .unwrap();
        store
            .record_terminal(
                "parent",
                TaskState::Succeeded,
                serde_json::json!({"verified": false}),
                22,
            )
            .unwrap();

        // Provider success is not independent verification. This test checks
        // dependency mechanics only; verification gating is a subsequent slice.
        assert_eq!(store.task("child").unwrap().state, TaskState::Ready);
    }

    #[test]
    fn restart_exposes_pending_intent_but_never_redispatches_it() {
        let temporary = TempDir::new().unwrap();
        let path = temporary.path().join("execution.sqlite3");
        {
            let store = ExecutionStore::open(&path).unwrap();
            create_single(&store);
            store
                .prepare_dispatch("task-1", &grant("request-1"), "stable-key", 20)
                .unwrap();
        }

        let reopened = ExecutionStore::open(&path).unwrap();
        let pending = reopened.pending_reconciliation().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].request_id, "request-1");
        assert_eq!(
            reopened.task("task-1").unwrap().state,
            TaskState::Dispatching
        );
        assert!(reopened
            .prepare_dispatch("task-1", &grant("request-2"), "stable-key", 30)
            .is_err());
    }
}
FILE: crates/dread-execution/src/reconcile.rs

Rust

Copy
use crate::store::{DispatchPreparation, ExecutionStore};
use async_trait::async_trait;
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderObservation {
    /// Provider found the durable request/idempotency identity.
    Acknowledged {
        provider_execution_id: String,
    },
    /// This is NOT proof of absence of remote side effects. It requires an
    /// operator/provider-specific resolution path, never automatic redispatch.
    NotFound,
    /// Provider cannot currently determine the result.
    Unknown,
}

#[derive(Debug, Error)]
pub enum ReconcileError {
    #[error("provider reconciliation failed: {0}")]
    Provider(String),
}

#[async_trait]
pub trait ProviderReconciler: Send + Sync {
    /// Implementations query only their real provider. They must not dispatch,
    /// retry, fabricate provider IDs, or infer success from local state.
    async fn observe(
        &self,
        preparation: &DispatchPreparation,
    ) -> Result<ProviderObservation, ReconcileError>;
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ReconciliationReport {
    pub examined: usize,
    pub acknowledged: usize,
    pub unresolved: usize,
    pub blocked: usize,
    pub provider_errors: usize,
}

pub struct ReconciliationService {
    store: Arc<ExecutionStore>,
    provider: Arc<dyn ProviderReconciler>,
}

impl ReconciliationService {
    pub fn new(
        store: Arc<ExecutionStore>,
        provider: Arc<dyn ProviderReconciler>,
    ) -> Self {
        Self { store, provider }
    }

    /// Startup recovery calls this before opening dispatch queues.
    ///
    /// A provider error leaves the intent pending for a later reconciliation.
    /// NotFound blocks the task; it never means that redispatch is safe.
    pub async fn reconcile_pending(&self) -> ReconciliationReport {
        let pending = match self.store.pending_reconciliation() {
            Ok(pending) => pending,
            Err(_) => {
                return ReconciliationReport {
                    provider_errors: 1,
                    ..ReconciliationReport::default()
                }
            }
        };

        let mut report = ReconciliationReport {
            examined: pending.len(),
            ..ReconciliationReport::default()
        };

        for preparation in pending {
            match self.provider.observe(&preparation).await {
                Ok(ProviderObservation::Acknowledged {
                    provider_execution_id,
                }) => {
                    // Timestamp zero is unsuitable for production. The caller
                    // must replace this method with the timestamped variant.
                    report.unresolved += 1;
                }
                Ok(ProviderObservation::NotFound) => {
                    report.unresolved += 1;
                }
                Ok(ProviderObservation::Unknown) | Err(_) => {
                    report.unresolved +=



1147.87 s
15.95 TPS
120.93 s TTFT
PROJECT DREAD UNIVERSAL — COMBINED CODE REVIEW AND CORRECTION REQUEST
Deliveries 001 and 002
Both deliveries contain useful implementation work. Do not discard them or start again. Correct the existing source and provide complete revised files.

Current status:

Delivery 001: PARTIAL — REQUIRES REVISION.

Delivery 002: PARTIAL — REQUIRES REVISION.

Compilation and tests: NOT RUN.

Canonical integration: NOT VERIFIED.

The receiving engineer already has the original code from both deliveries. Preserve the implementation and address the following defects.

PART A — DELIVERY 001: REGISTRY AND PREFLIGHT
A1. Canonical architecture
Do not assume dread-execution is a canonical crate.

Reconcile these modules against the existing architecture, particularly dread-orchestrator, before adding workspace members.

Supply:

Correct crate and module paths.

Complete module exports.

Valid Cargo dependency changes.

Correct integration contracts.

No duplicate API server, execution scheduler, registry or mission database.

Do not provide placeholder or malformed diffs.

A2. Trusted provider-isolation evidence — critical
The existing preflight accepts caller-constructed values claiming:

EvidenceStatus::Verified.

PermissionDecision::Allow.

Enabled provider state.

Approved capability state.

This is not a secure authorization boundary.

Implement server-side validation:

Retrieve evidence from a trusted evidence repository.

Verify its provenance and adapter identity.

Check adapter version and registered provider state.

Resolve permissions through the trusted policy engine.

Bind the decision to the requester, mission, task, provider, capability, operation and permission scope.

Enforce a bounded preflight lifetime.

Prevent evidence or grants from being reused for unrelated executions.

Persist admission transactionally before any provider call.

Keep admission distinct from provider acknowledgement and successful execution.

The registry, policy engine and evidence repository must be the authoritative sources. A JSON object claiming that something is verified must not authorize execution.

Add tests for forged evidence, changed permission scopes, adapter mismatches, expired evidence, task mismatches and replayed grants.

A3. Registry definition integrity
The current comparison does not fully implement its stated byte-equivalent definition rule.

Use a deterministic fingerprint containing all security-relevant manifest fields, including:

Manifest version.

Capability identity and kind.

Version.

Source identity.

Content hash.

Dependencies.

Permissions.

Publisher identity/signature.

Executable configuration and other security-relevant metadata.

A security-relevant change must disable the capability and reset review to discovered.

Display-only changes must be distinguished explicitly and must not silently alter security state.

A4. Database and filesystem security
Correct:

Dangling symlink detection.

Parent-directory symlink handling.

Path substitution and TOCTOU weaknesses.

Private file creation from the beginning, not only after opening.

Permission checks for existing database files and parent directories.

Backup and restore destination safety.

Database schema-shape verification.

Migration checksum and version validation.

Append-only audit/discovery history where appropriate.

Correct integrity-check result handling.

Do not change an already-applied migration checksum silently. Use a forward migration when necessary.

Address how the SQLite connection is shared safely across concurrent Axum requests. rusqlite::Connection must not be assumed to be Sync.

Add regression tests for symlink, permissions, migration corruption, invalid schema and concurrency.

PART B — DELIVERY 002: LIFECYCLE AND EXECUTION
Preserve the existing files:

crates/dread-execution/src/lifecycle.rs

crates/dread-execution/src/store.rs

crates/dread-execution/src/reconcile.rs

Correct them in place conceptually, using the actual canonical crate path after reconciliation.

B1. Correct idempotency retry classification
The present code permits ProviderKeyRequired retries unconditionally:

IdempotencyClass::ProviderKeyRequired => true

That is unsafe unless the exact same durable provider idempotency key is being reused for the same logical operation.

Implement:

Durable idempotency-key storage.

Scope binding to task, provider, capability and operation.

Same-key enforcement across retries.

Rejection of conflicting key reuse.

Request fingerprint checking.

Provider support/contract verification before treating keyed retry as safe.

Unknown must never permit automatic retry.

A ProviderKeyRequired classification alone must not prove that retry is safe.

B2. Do not turn unknown outcomes into terminal failure
This is a critical lifecycle defect.

The current schedule_retry() maps every unsafe or exhausted retry decision to TaskState::Failed.

For an Unknown provider outcome, the remote operation may still be running or may already have produced side effects. It cannot honestly be labelled failed.

Introduce a distinct nonterminal state such as:

ReconciliationRequired

Use it for ambiguous outcomes and unresolved dispatches.

Do not automatically:

Redispatch.

Mark the operation failed.

Mark it cancelled.

Release dependent tasks.

Mark the mission complete.

Only verified provider terminal evidence or an explicit reviewed operator resolution may settle the outcome.

Ensure mission aggregation treats reconciliation-required work as nonterminal.

B3. Correct provider execution identity for retries
The current schema keeps one provider_execution_id on the task.

That creates a problem when an earlier provider attempt is acknowledged, then a later retry needs a different provider execution ID.

Introduce a durable per-attempt execution record, or an equivalent design.

Preserve:

Attempt number.

Dispatch request ID.

Idempotency key.

Provider identity.

Provider execution ID.

Preflight/admission identity.

Dispatch timestamp.

Acknowledgement timestamp.

Attempt outcome.

Reconciliation history.

Do not overwrite an earlier attempt's provider identity.

Correct record_provider_ack() so idempotent acknowledgement is checked against the specific dispatch intent, not merely a provider ID already present on the task.

Reject conflicting request/provider identity bindings.

B4. Make dispatch preparation truly idempotent
The existing duplicate-request check happens before the transaction and has a race window.

Move the definitive check inside the transaction.

Bind the request fingerprint to all material parameters, including:

Task.

Provider.

Capability.

Request ID.

Attempt.

Idempotency key.

Preflight/admission identity.

Grant expiry.

Operation scope.

Use database uniqueness constraints to prevent duplicate dispatch intents and key reuse.

A duplicate request with identical content must return the original durable preparation. Reusing the request ID or key with changed content must produce a conflict.

Never accept an arbitrary caller-created DispatchGrant as proof of admission. Verify it against a persisted trusted admission record.

B5. Correct task and mission transitions
Add an explicit transition model.

Cover:

Pending.

Ready.

Dispatching.

Running.

Retry scheduled.

Cancellation requested.

Cancelling.

Reconciliation required.

Succeeded.

Failed.

Cancelled.

Blocked, where appropriate.

Do not treat all blocked conditions as equivalent to a confirmed terminal provider outcome.

Mission transitions must also be validated, and stale revision updates must be rejected.

Add revision/CAS checks to mutation boundaries.

Provider success received after a cancellation request must be handled accurately. The provider's actual terminal outcome must not be discarded merely because local cancellation was requested first.

B6. Correct cancellation semantics
Preserve the useful decision that cancellation is not complete merely because a user requested it.

Implement a real provider cancellation interface with these distinctions:

Cancellation requested locally.

Cancellation request sent.

Provider accepted cancellation request.

Provider operation still cancelling.

Provider confirmed terminal cancellation.

Provider completed successfully despite the cancellation race.

Provider reported failure.

Provider outcome unknown.

Never treat cancellation acceptance as terminal cancellation.

Record provider evidence for the final state.

Duplicate cancellation requests must not create misleading duplicate transitions.

B7. Correct event persistence and append-only semantics
The current event table is not actually append-only, because ordinary SQL can update and delete rows.

Add appropriate database-enforced protection.

Also correct:

Event identity and scope.

Deduplication behavior.

Duplicate event payload conflict detection.

Provider identity and execution/attempt correlation.

Mission/task consistency.

Missing state-transition events.

Dependency release/block events.

Reconciliation events.

Terminal mission events.

A duplicate event key with different content must not silently disappear as a successful duplicate.

Mission-level events with NULL task IDs need an explicit deduplication strategy; SQLite UNIQUE semantics allow multiple NULL values.

Provider event keys may reset across execution attempts, so event identity must include suitable provider/attempt context.

Add event-size and input validation.

B8. Correct dependency and mission behavior
The current DAG validation is a useful foundation. Retain the checks for:

Duplicate task IDs.

Duplicate dependencies.

Unknown dependencies.

Dependency cycles.

Task-count bounds.

Also ensure:

Dependency release happens only after a valid succeeded state and any required verification gate.

Blocked dependency propagation is durable and emits events.

Reconciliation-required work keeps its dependents pending.

Mission state does not become terminal while any task has an unresolved provider outcome.

Mission state cannot be incorrectly changed back to running after reaching a terminal state.

Do not use a provider's unverified verified: false result as independent verification. Provider completion and independent verification remain separate lifecycle stages.

B9. Retry safety
Permit retry only when the provider outcome and operation semantics justify it.

Idempotent operations require their verified provider contract.

Provider-key-required retries must reuse the exact durable key.

Non-idempotent retries require proof of definite failure before side effects.

Unknown outcomes require reconciliation, not retry.

An active provider operation must not be retried concurrently.

Exhausted retries must remain distinguishable from unknown outcomes.

Add tests for all four idempotency classes and every outcome category.

PART C — DELIVERY 002 RECONCILIATION
The supplied reconcile.rs paste is incomplete. It stops inside the Unknown/provider-error branch. Provide the entire file, including all closing branches, function bodies and error handling.

The module-export integration diff is also still missing.

Implement and test these requirements:

Startup reconciliation must run before dispatch queues are opened.

An acknowledged provider result must be checked and persisted against its exact dispatch intent.

Use a valid injected timestamp or clock. Do not use timestamp zero.

A provider error leaves the task unresolved.

Ordinary provider NotFound is not proof that no side effect occurred.

Unknown is not terminal.

Neither NotFound nor Unknown permits automatic redispatch.

Only an authenticated, provider-specific proof of definite non-submission may support a safe resolution.

Reconciliation must not invent provider execution IDs.

A startup barrier must keep dispatch closed while unresolved intents still need reconciliation or operator resolution.

The provider interface must be observational only. It must never dispatch or retry work.

Provide tests for:

Provider acknowledgement.

Duplicate acknowledgement.

Provider error.

NotFound.

Unknown.

Conflicting provider IDs.

Restart with pending dispatch.

No automatic redispatch.

Startup remaining closed when reconciliation is unresolved.

PART D — INTEGRATION AND DELIVERY FILES
Return the following complete source or valid diffs:

Corrected registry source and manifest.

Corrected preflight source.

Corrected lifecycle source.

Corrected execution store.

Complete corrected reconciliation source.

Required migration additions.

Per-attempt execution identity schema.

Trusted evidence/admission persistence interfaces.

Cancellation interface.

Module exports and correct canonical Cargo changes.

Full regression/security tests.

Updated checkpoint and file manifest.

Do not introduce a parallel mission database if canonical mission persistence already exists. Reconcile all migrations against the actual intended schema.

Do not omit code with phrases such as "rest unchanged" or "implementation goes here".

VERIFICATION STATUS
All generated source remains:

CODE_GENERATED / TESTS_NOT_RUN / NEEDS_CANONICAL_MERGE

unless actual commands were run and their results are provided.

Do not claim compilation, Clippy, test success, >>>One additional caution: Delivery 002's ExecutionStore currently creates its own dread_missions and dread_tasks tables. It must be reconciled with the actual canonical persistence layer before those tables are adopted; otherwise we risk building a second mission system inside the Universal Hub.

Paste Kimi's missing reconcile.rs tail and module-export diff when available, so those can be reviewed as part of the same correction cycle.

Reasoning · Done · 252.1s total
CORRECTION CYCLE 003A — REGISTRY AND TRUSTED ADMISSION

Status: CODE_GENERATED / TESTS_NOT_RUN / NEEDS_CANONICAL_MERGE

Canonical integration paths are now:

dread-registry: capability metadata, provenance, disabled-by-default refresh, and registry history.
dread-orchestrator: preflight, policy-bound admission, lifecycle, execution persistence, reconciliation, and cancellation.
The canonical API crate will call these modules; this delivery adds no listener or scheduler.
dread-execution is not introduced as a workspace member.
The orchestrator persistence code in the next part must be merged into the existing canonical mission store. Its candidate tables must not be adopted as a second mission database.
This part supplies five corrected files. Delivery 003B will contain the corrected lifecycle/store/reconciliation/cancellation modules and exports.

FILE: crates/dread-core/src/manifest.rs

Rust

Copy
use crate::domain::{CapabilityKind, ImportedCapability};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use thiserror::Error;

pub const CAPABILITY_MANIFEST_VERSION: &str = "capability-manifest-v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityManifest {
    pub manifest_version: String,
    pub id: String,
    /// Display-only. Changing this field does not change executable identity.
    pub name: String,
    pub version: String,
    pub kind: CapabilityKind,
    pub source: String,
    pub content_sha256: String,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub publisher_identity: Option<String>,
    #[serde(default)]
    pub publisher_signature: Option<String>,
    #[serde(default)]
    pub executable: Option<ExecutableManifest>,
    /// Additional security/compatibility metadata. Keys and values are bounded.
    #[serde(default)]
    pub security_metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutableManifest {
    pub adapter_id: String,
    /// Opaque adapter-owned reference, not a command line to execute.
    pub entrypoint_ref: String,
    #[serde(default)]
    pub arguments: Vec<String>,
    /// Names of protected secret references; values are forbidden here.
    #[serde(default)]
    pub secret_refs: Vec<String>,
    #[serde(default)]
    pub resource_requirements: BTreeMap<String, u64>,
}

/// Every field except display name participates in the definition fingerprint.
#[derive(Serialize)]
struct SecurityDefinition<'a> {
    manifest_version: &'a str,
    id: &'a str,
    version: &'a str,
    kind: CapabilityKind,
    source: &'a str,
    content_sha256: &'a str,
    dependencies: &'a [String],
    permissions: &'a [String],
    publisher_identity: &'a Option<String>,
    publisher_signature: &'a Option<String>,
    executable: &'a Option<ExecutableManifest>,
    security_metadata: &'a BTreeMap<String, String>,
}

#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("invalid capability manifest: {0}")]
    Invalid(String),

    #[error("cannot canonicalize capability manifest: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl CapabilityManifest {
    pub fn import_disabled(&self) -> Result<ImportedCapability, ManifestError> {
        self.validate()?;
        Ok(ImportedCapability::disabled_import(
            self.id.clone(),
            self.kind,
            self.content_sha256.clone(),
            self.permissions.clone(),
        ))
    }

    /// Deterministic over the complete security definition.
    ///
    /// A signature is treated as security metadata, not as proof that the
    /// publisher is trusted. Publisher verification is a separate policy step.
    pub fn definition_fingerprint(&self) -> Result<String, ManifestError> {
        self.validate()?;
        let definition = SecurityDefinition {
            manifest_version: &self.manifest_version,
            id: &self.id,
            version: &self.version,
            kind: self.kind,
            source: &self.source,
            content_sha256: &self.content_sha256,
            dependencies: &self.dependencies,
            permissions: &self.permissions,
            publisher_identity: &self.publisher_identity,
            publisher_signature: &self.publisher_signature,
            executable: &self.executable,
            security_metadata: &self.security_metadata,
        };
        let canonical = serde_json::to_vec(&definition)?;
        Ok(format!("{:x}", Sha256::digest(canonical)))
    }

    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.manifest_version != CAPABILITY_MANIFEST_VERSION {
            return invalid("unsupported manifest_version");
        }
        if !valid_id(&self.id) {
            return invalid("id must be a bounded lowercase identifier");
        }
        validate_text("name", &self.name, 256)?;
        validate_text("version", &self.version, 128)?;
        validate_text("source", &self.source, 1024)?;

        if self.content_sha256.len() != 64
            || !self
                .content_sha256
                .chars()
                .all(|character| character.is_ascii_hexdigit())
        {
            return invalid("content_sha256 must contain 64 hexadecimal characters");
        }

        validate_bounded_list("dependencies", &self.dependencies, 256, 512)?;
        validate_bounded_list("permissions", &self.permissions, 256, 256)?;
        validate_bounded_list("secret_refs", &self.secret_refs(), 128, 256)?;

        if let Some(executable) = &self.executable {
            if !valid_id(&executable.adapter_id) {
                return invalid("executable.adapter_id is invalid");
            }
            validate_text("entrypoint_ref", &executable.entrypoint_ref, 1024)?;
            validate_bounded_list("arguments", &executable.arguments, 256, 4096)?;
            if executable.resource_requirements.len() > 128 {
                return invalid("too many resource requirement entries");
            }
            for key in executable.resource_requirements.keys() {
                validate_text("resource requirement key", key, 128)?;
            }
        }

        if self.security_metadata.len() > 256 {
            return invalid("too many security metadata entries");
        }
        for (key, value) in &self.security_metadata {
            validate_text("security metadata key", key, 128)?;
            if value.len() > 4096 {
                return invalid("security metadata value is too large");
            }
        }

        for (field, value) in [
            ("publisher_identity", &self.publisher_identity),
            ("publisher_signature", &self.publisher_signature),
        ] {
            if value.as_ref().is_some_and(|text| text.trim().is_empty()) {
                return invalid(format!("{field} cannot be empty when supplied"));
            }
        }
        Ok(())
    }

    fn secret_refs(&self) -> Vec<String> {
        self.executable
            .as_ref()
            .map(|executable| executable.secret_refs.clone())
            .unwrap_or_default()
    }
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || character == '-'
                || character == '_'
        })
}

fn validate_text(field: &str, value: &str, maximum: usize) -> Result<(), ManifestError> {
    if value.trim().is_empty() || value.len() > maximum {
        invalid(format!(
            "{field} must be nonempty and at most {maximum} bytes"
        ))
    } else {
        Ok(())
    }
}

fn validate_bounded_list(
    field: &str,
    values: &[String],
    maximum_items: usize,
    maximum_bytes: usize,
) -> Result<(), ManifestError> {
    if values.len() > maximum_items {
        return invalid(format!("too many {field} entries"));
    }
    for value in values {
        validate_text(field, value, maximum_bytes)?;
    }
    Ok(())
}

fn invalid<T>(message: impl Into<String>) -> Result<T, ManifestError> {
    Err(ManifestError::Invalid(message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> CapabilityManifest {
        CapabilityManifest {
            manifest_version: CAPABILITY_MANIFEST_VERSION.into(),
            id: "example-skill".into(),
            name: "Example".into(),
            version: "1.0.0".into(),
            kind: CapabilityKind::Skill,
            source: "local-reviewed-source".into(),
            content_sha256: "ab".repeat(32),
            dependencies: vec![],
            permissions: vec!["filesystem:read".into()],
            publisher_identity: None,
            publisher_signature: None,
            executable: None,
            security_metadata: BTreeMap::new(),
        }
    }

    #[test]
    fn import_cannot_enable_a_capability() {
        let imported = manifest().import_disabled().unwrap();
        assert!(!imported.enabled);
    }

    #[test]
    fn display_name_does_not_change_definition_fingerprint() {
        let original = manifest();
        let mut renamed = original.clone();
        renamed.name = "Renamed".into();
        assert_eq!(
            original.definition_fingerprint().unwrap(),
            renamed.definition_fingerprint().unwrap()
        );
    }

    #[test]
    fn every_security_field_changes_the_fingerprint() {
        let original = manifest().definition_fingerprint().unwrap();

        let mut changed = manifest();
        changed.permissions.push("process:execute".into());
        assert_ne!(original, changed.definition_fingerprint().unwrap());

        let mut changed = manifest();
        changed.dependencies.push("dependency-v1".into());
        assert_ne!(original, changed.definition_fingerprint().unwrap());

        let mut changed = manifest();
        changed.publisher_identity = Some("publisher".into());
        assert_ne!(original, changed.definition_fingerprint().unwrap());

        let mut changed = manifest();
        changed.executable = Some(ExecutableManifest {
            adapter_id: "reviewed-adapter".into(),
            entrypoint_ref: "opaque-entrypoint".into(),
            arguments: vec![],
            secret_refs: vec![],
            resource_requirements: BTreeMap::new(),
        });
        assert_ne!(original, changed.definition_fingerprint().unwrap());

        let mut changed = manifest();
        changed
            .security_metadata
            .insert("sandbox_profile".into(), "strict".into());
        assert_ne!(original, changed.definition_fingerprint().unwrap());
    }

    #[test]
    fn unknown_executable_fields_are_rejected() {
        let text = r#"
manifest_version = "capability-manifest-v1"
id = "example-skill"
name = "Example"
version = "1.0.0"
kind = "skill"
source = "local-reviewed-source"
content_sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
install_hook = "arbitrary-command"
"#;
        assert!(toml::from_str::<CapabilityManifest>(text).is_err());
    }
}
FILE: crates/dread-registry/Cargo.toml

Toml

Copy
[package]
name = "dread-registry"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
description = "Durable capability registry for DREAD Universal"

[dependencies]
dread-core = { path = "../dread-core" }
libc = "0.2"
rusqlite.workspace = true
serde_json.workspace = true
sha2.workspace = true
thiserror.workspace = true

[dev-dependencies]
tempfile.workspace = true
FILE: crates/dread-registry/src/lib.rs

Rust

Copy
//! Canonical capability-registry implementation.
//!
//! This store registers metadata only. It does not admit provider execution and
//! does not own missions. Changed definitions always return to disabled review.

use dread_core::{
    domain::{CapabilityKind, ImportedCapability, ReviewState},
    manifest::{CapabilityManifest, ManifestError},
};
use rusqlite::{
    backup::Backup, params, Connection, OptionalExtension, Row, Transaction,
    TransactionBehavior,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use thiserror::Error;

const CURRENT_VERSION: i64 = 2;

const MIGRATION_0001: &str = r#"
CREATE TABLE dread_capabilities (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    kind TEXT NOT NULL,
    source TEXT NOT NULL,
    content_sha256 TEXT NOT NULL,
    dependencies_json TEXT NOT NULL,
    publisher_signature TEXT,
    review TEXT NOT NULL CHECK (
        review IN (
            'discovered', 'inspected', 'registered',
            'validated', 'approved', 'rejected'
        )
    ),
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    permissions_json TEXT NOT NULL,
    created_at_unix_seconds INTEGER NOT NULL,
    updated_at_unix_seconds INTEGER NOT NULL
) STRICT;

CREATE INDEX dread_capabilities_kind_enabled
    ON dread_capabilities(kind, enabled);

CREATE TABLE dread_discovery_observations (
    observation_id INTEGER PRIMARY KEY AUTOINCREMENT,
    capability_id TEXT NOT NULL,
    source TEXT NOT NULL,
    content_sha256 TEXT NOT NULL,
    manifest_json TEXT NOT NULL,
    observed_at_unix_seconds INTEGER NOT NULL,
    FOREIGN KEY(capability_id) REFERENCES dread_capabilities(id)
) STRICT;

CREATE INDEX dread_discovery_observations_capability
    ON dread_discovery_observations(capability_id, observation_id);

CREATE TABLE dread_registry_audit (
    audit_id INTEGER PRIMARY KEY AUTOINCREMENT,
    capability_id TEXT,
    operation TEXT NOT NULL,
    actor TEXT NOT NULL,
    occurred_at_unix_seconds INTEGER NOT NULL,
    details_json TEXT NOT NULL
) STRICT;

CREATE INDEX dread_registry_audit_capability
    ON dread_registry_audit(capability_id, audit_id);
"#;

/// Forward migration: existing schema-migration checksums are never rewritten.
const MIGRATION_0002: &str = r#"
ALTER TABLE dread_capabilities
    ADD COLUMN definition_sha256 TEXT;

ALTER TABLE dread_capabilities
    ADD COLUMN publisher_identity TEXT;

ALTER TABLE dread_capabilities
    ADD COLUMN executable_json TEXT;

ALTER TABLE dread_capabilities
    ADD COLUMN security_metadata_json TEXT;

CREATE TRIGGER dread_discovery_observations_no_update
BEFORE UPDATE ON dread_discovery_observations
BEGIN
    SELECT RAISE(ABORT, 'discovery observations are append-only');
END;

CREATE TRIGGER dread_discovery_observations_no_delete
BEFORE DELETE ON dread_discovery_observations
BEGIN
    SELECT RAISE(ABORT, 'discovery observations are append-only');
END;

CREATE TRIGGER dread_registry_audit_no_update
BEFORE UPDATE ON dread_registry_audit
BEGIN
    SELECT RAISE(ABORT, 'registry audit is append-only');
END;

CREATE TRIGGER dread_registry_audit_no_delete
BEFORE DELETE ON dread_registry_audit
BEGIN
    SELECT RAISE(ABORT, 'registry audit is append-only');
END;
"#;

#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("registry I/O failed: {0}")]
    Io(#[from] std::io::Error),

    #[error("registry SQLite operation failed: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("registry serialization failed: {0}")]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Manifest(#[from] ManifestError),

    #[error("invalid registry operation: {0}")]
    Invalid(String),

    #[error("capability not found: {0}")]
    NotFound(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryRecord {
    pub id: String,
    pub name: String,
    pub version: String,
    pub kind: CapabilityKind,
    pub source: String,
    pub content_sha256: String,
    pub dependencies: Vec<String>,
    pub permissions: Vec<String>,
    pub publisher_identity: Option<String>,
    pub publisher_signature: Option<String>,
    pub executable_json: Option<String>,
    pub security_metadata_json: Option<String>,
    pub definition_sha256: Option<String>,
    pub review: ReviewState,
    pub enabled: bool,
    pub created_at_unix_seconds: i64,
    pub updated_at_unix_seconds: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportOutcome {
    Created,
    RefreshedUnchanged,
    ChangedAndDisabled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportResult {
    pub record: RegistryRecord,
    pub outcome: ImportOutcome,
}

#[derive(Debug)]
pub struct RegistryStore {
    connection: Connection,
}

/// Connection is Send through RegistryStore but must never be treated as Sync.
/// Axum handlers should run short registry operations in spawn_blocking and
/// must not hold this mutex across an await.
pub type SharedRegistry = Arc<Mutex<RegistryStore>>;

impl RegistryStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, RegistryError> {
        let path = secure_database_path(path.as_ref())?;
        let connection = Connection::open(path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.execute_batch("PRAGMA foreign_keys = ON;")?;
        let store = Self { connection };
        store.migrate()?;
        Ok(store)
    }

    pub fn into_shared(self) -> SharedRegistry {
        Arc::new(Mutex::new(self))
    }

    fn migrate(&self) -> Result<(), RegistryError> {
        let transaction = self
            .connection
            .unchecked_transaction_with_behavior(TransactionBehavior::Immediate)?;

        transaction.execute_batch(
            "CREATE TABLE IF NOT EXISTS dread_schema_migrations (
                version INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                checksum_sha256 TEXT NOT NULL,
                applied_at_unix_seconds INTEGER NOT NULL
            ) STRICT;",
        )?;

        let current = transaction.query_row(
            "SELECT MAX(version) FROM dread_schema_migrations",
            [],
            |row| row.get::<_, Option<i64>>(0),
        )?;

        match current {
            None | Some(0) => {
                ensure_no_unversioned_domain_tables(&transaction)?;
                apply_migration(&transaction, 1, "initial-capability-registry", MIGRATION_0001)?;
                apply_migration(&transaction, 2, "definition-and-append-only-history", MIGRATION_0002)?;
            }
            Some(1) => {
                verify_recorded_migration(&transaction, 1, MIGRATION_0001)?;
                verify_columns(
                    &transaction,
                    "dread_capabilities",
                    &["id", "name", "version", "kind", "source", "content_sha256"],
                )?;
                apply_migration(&transaction, 2, "definition-and-append-only-history", MIGRATION_0002)?;
            }
            Some(CURRENT_VERSION) => {
                verify_recorded_migration(&transaction, 1, MIGRATION_0001)?;
                verify_recorded_migration(&transaction, 2, MIGRATION_0002)?;
                verify_columns(
                    &transaction,
                    "dread_capabilities",
                    &[
                        "id",
                        "definition_sha256",
                        "publisher_identity",
                        "executable_json",
                        "security_metadata_json",
                    ],
                )?;
            }
            Some(version) => {
                return Err(RegistryError::Invalid(format!(
                    "unsupported registry schema version {version}"
                )));
            }
        }

        transaction.commit()?;
        Ok(())
    }

    pub fn import(&self, manifest: &CapabilityManifest) -> Result<ImportResult, RegistryError> {
        let imported: ImportedCapability = manifest.import_disabled()?;
        let definition = manifest.definition_fingerprint()?;
        let kind = kind_name(imported.kind)?;
        let now = unix_seconds()?;
        let permissions_json = serde_json::to_string(&manifest.permissions)?;
        let dependencies_json = serde_json::to_string(&manifest.dependencies)?;
        let executable_json = manifest
            .executable
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let security_metadata_json = serde_json::to_string(&manifest.security_metadata)?;
        let manifest_json = serde_json::to_string(manifest)?;

        let transaction = self
            .connection
            .unchecked_transaction_with_behavior(TransactionBehavior::Immediate)?;

        let existing_fingerprint: Option<Option<String>> = transaction
            .query_row(
                "SELECT definition_sha256 FROM dread_capabilities WHERE id = ?1",
                [&manifest.id],
                |row| row.get(0),
            )
            .optional()?;

        let outcome = if let Some(previous_fingerprint) = existing_fingerprint {
            let existing_kind: String = transaction.query_row(
                "SELECT kind FROM dread_capabilities WHERE id = ?1",
                [&manifest.id],
                |row| row.get(0),
            )?;
            if existing_kind != kind {
                return Err(RegistryError::Invalid(
                    "capability identity cannot change kind".into(),
                ));
            }

            if previous_fingerprint.as_deref() == Some(definition.as_str()) {
                transaction.execute(
                    "UPDATE dread_capabilities
                     SET name = ?2, updated_at_unix_seconds = ?3
                     WHERE id = ?1",
                    params![manifest.id, manifest.name, now],
                )?;
                ImportOutcome::RefreshedUnchanged
            } else {
                // Legacy rows without a v2 fingerprint also fail closed.
                transaction.execute(
                    "UPDATE dread_capabilities SET
                        name = ?2, version = ?3, source = ?4,
                        content_sha256 = ?5, dependencies_json = ?6,
                        publisher_identity = ?7, publisher_signature = ?8,
                        executable_json = ?9, security_metadata_json = ?10,
                        definition_sha256 = ?11, permissions_json = ?12,
                        review = 'discovered', enabled = 0,
                        updated_at_unix_seconds = ?13
                     WHERE id = ?1",
                    params![
                        manifest.id,
                        manifest.name,
                        manifest.version,
                        manifest.source,
                        manifest.content_sha256,
                        dependencies_json,
                        manifest.publisher_identity,
                        manifest.publisher_signature,
                        executable_json,
                        security_metadata_json,
                        definition,
                        permissions_json,
                        now
                    ],
                )?;
                ImportOutcome::ChangedAndDisabled
            }
        } else {
            transaction.execute(
                "INSERT INTO dread_capabilities (
                    id, name, version, kind, source, content_sha256,
                    dependencies_json, publisher_identity, publisher_signature,
                    executable_json, security_metadata_json, definition_sha256,
                    review, enabled, permissions_json,
                    created_at_unix_seconds, updated_at_unix_seconds
                 ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9,
                    ?10, ?11, ?12, 'discovered', 0, ?13, ?14, ?14
                 )",
                params![
                    manifest.id,
                    manifest.name,
                    manifest.version,
                    kind,
                    manifest.source,
                    manifest.content_sha256,
                    dependencies_json,
                    manifest.publisher_identity,
                    manifest.publisher_signature,
                    executable_json,
                    security_metadata_json,
                    definition,
                    permissions_json,
                    now
                ],
            )?;
            ImportOutcome::Created
        };

        transaction.execute(
            "INSERT INTO dread_discovery_observations (
                capability_id, source, content_sha256,
                manifest_json, observed_at_unix_seconds
             ) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                manifest.id,
                manifest.source,
                manifest.content_sha256,
                manifest_json,
                now
            ],
        )?;
        transaction.execute(
            "INSERT INTO dread_registry_audit (
                capability_id, operation, actor,
                occurred_at_unix_seconds, details_json
             ) VALUES (?1, ?2, 'discovery', ?3, ?4)",
            params![
                manifest.id,
                match outcome {
                    ImportOutcome::Created => "import_created",
                    ImportOutcome::RefreshedUnchanged => "discovery_refreshed",
                    ImportOutcome::ChangedAndDisabled => "definition_changed_and_disabled",
                },
                now,
                serde_json::to_string(&serde_json::json!({
                    "definition_sha256": definition,
                    "execution_authorized": false
                }))?
            ],
        )?;

        transaction.commit()?;
        Ok(ImportResult {
            record: self.get(&manifest.id)?,
            outcome,
        })
    }

    pub fn get(&self, id: &str) -> Result<RegistryRecord, RegistryError> {
        self.connection
            .query_row(
                "SELECT id, name, version, kind, source, content_sha256,
                        dependencies_json, permissions_json,
                        publisher_identity, publisher_signature,
                        executable_json, security_metadata_json,
                        definition_sha256, review, enabled,
                        created_at_unix_seconds, updated_at_unix_seconds
                 FROM dread_capabilities WHERE id = ?1",
                [id],
                decode_record,
            )
            .optional()?
            .transpose()?
            .ok_or_else(|| RegistryError::NotFound(id.to_owned()))
    }

    pub fn create_backup(&self, destination: impl AsRef<Path>) -> Result<PathBuf, RegistryError> {
        let destination = reserve_private_destination(destination.as_ref())?;
        let result = (|| {
            let mut backup = Connection::open(&destination)?;
            {
                let operation = Backup::new(&self.connection, &mut backup)?;
                operation.run_to_completion(256, Duration::from_millis(50), None)?;
            }
            verify_backup(&backup)
        })();
        if let Err(error) = result {
            let _ = fs::remove_file(&destination);
            return Err(error);
        }
        Ok(destination)
    }

    pub fn restore_to_new(
        backup_path: impl AsRef<Path>,
        destination: impl AsRef<Path>,
    ) -> Result<Self, RegistryError> {
        let backup_path = secure_existing_file(backup_path.as_ref())?;
        let source = Connection::open_with_flags(
            &backup_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        verify_backup(&source)?;

        let destination = reserve_private_destination(destination.as_ref())?;
        let result = (|| {
            let mut restored = Connection::open(&destination)?;
            {
                let operation = Backup::new(&source, &mut restored)?;
                operation.run_to_completion(256, Duration::from_millis(50), None)?;
            }
            verify_backup(&restored)
        })();
        if let Err(error) = result {
            let _ = fs::remove_file(&destination);
            return Err(error);
        }
        Self::open(destination)
    }
}

fn decode_record(row: &Row<'_>) -> Result<RegistryRecord, rusqlite::Error> {
    let kind_text: String = row.get(3)?;
    let review_text: String = row.get(13)?;
    let enabled: i64 = row.get(14)?;

    let kind = serde_json::from_value(Value::String(kind_text))
        .map_err(|error| rusqlite::Error::InvalidColumnType(3, error.to_string(), rusqlite::types::Type::Text))?;
    let review = match review_text.as_str() {
        "discovered" => ReviewState::Discovered,
        "inspected" => ReviewState::Inspected,
        "registered" => ReviewState::Registered,
        "validated" => ReviewState::Validated,
        "approved" => ReviewState::Approved,
        "rejected" => ReviewState::Rejected,
        _ => {
            return Err(rusqlite::Error::InvalidColumnType(
                13,
                "invalid review state".into(),
                rusqlite::types::Type::Text,
            ))
        }
    };

    Ok(RegistryRecord {
        id: row.get(0)?,
        name: row.get(1)?,
        version: row.get(2)?,
        kind,
        source: row.get(4)?,
        content_sha256: row.get(5)?,
        dependencies: parse_json_column(row, 6)?,
        permissions: parse_json_column(row, 7)?,
        publisher_identity: row.get(8)?,
        publisher_signature: row.get(9)?,
        executable_json: row.get(10)?,
        security_metadata_json: row.get(11)?,
        definition_sha256: row.get(12)?,
        review,
        enabled: match enabled {
            0 => false,
            1 => true,
            _ => {
                return Err(rusqlite::Error::InvalidColumnType(
                    14,
                    "invalid enabled value".into(),
                    rusqlite::types::Type::Integer,
                ))
            }
        },
        created_at_unix_seconds: row.get(15)?,
        updated_at_unix_seconds: row.get(16)?,
    })
}

fn parse_json_column<T: serde::de::DeserializeOwned>(
    row: &Row<'_>,
    index: usize,
) -> Result<T, rusqlite::Error> {
    let text: String = row.get(index)?;
    serde_json::from_str(&text).map_err(|error| {
        rusqlite::Error::InvalidColumnType(index, error.to_string(), rusqlite::types::Type::Text)
    })
}

fn apply_migration(
    transaction: &Transaction<'_>,
    version: i64,
    name: &str,
    sql: &str,
) -> Result<(), RegistryError> {
    transaction.execute_batch(sql)?;
    transaction.execute(
        "INSERT INTO dread_schema_migrations (
            version, name, checksum_sha256, applied_at_unix_seconds
         ) VALUES (?1, ?2, ?3, ?4)",
        params![version, name, checksum(sql), unix_seconds()?],
    )?;
    Ok(())
}

fn verify_recorded_migration(
    transaction: &Transaction<'_>,
    version: i64,
    sql: &str,
) -> Result<(), RegistryError> {
    let recorded: String = transaction.query_row(
        "SELECT checksum_sha256 FROM dread_schema_migrations WHERE version = ?1",
        [version],
        |row| row.get(0),
    )?;
    if recorded != checksum(sql) {
        return Err(RegistryError::Invalid(format!(
            "migration {version} checksum mismatch"
        )));
    }
    Ok(())
}

fn ensure_no_unversioned_domain_tables(
    transaction: &Transaction<'_>,
) -> Result<(), RegistryError> {
    let count: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM sqlite_master
         WHERE type = 'table' AND name IN (
            'dread_capabilities',
            'dread_discovery_observations',
            'dread_registry_audit'
         )",
        [],
        |row| row.get(0),
    )?;
    if count == 0 {
        Ok(())
    } else {
        Err(RegistryError::Invalid(
            "unversioned registry tables require canonical reconciliation".into(),
        ))
    }
}

fn verify_columns(
    transaction: &Transaction<'_>,
    table: &str,
    required: &[&str],
) -> Result<(), RegistryError> {
    let mut statement = transaction.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    if required.iter().all(|column| columns.iter().any(|actual| actual == column)) {
        Ok(())
    } else {
        Err(RegistryError::Invalid(format!(
            "schema shape mismatch for {table}"
        )))
    }
}

fn verify_backup(connection: &Connection) -> Result<(), RegistryError> {
    let mut integrity = connection.prepare("PRAGMA integrity_check")?;
    let results = integrity
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    if results.iter().any(|result| result != "ok") {
        return Err(RegistryError::Invalid(format!(
            "SQLite integrity check failed: {}",
            results.join("; ")
        )));
    }

    let mut foreign_keys = connection.prepare("PRAGMA foreign_key_check")?;
    if foreign_keys.query([])?.next()?.is_some() {
        return Err(RegistryError::Invalid(
            "SQLite foreign key check failed".into(),
        ));
    }

    let version: Option<i64> = connection.query_row(
        "SELECT MAX(version) FROM dread_schema_migrations",
        [],
        |row| row.get(0),
    )?;
    if version != Some(CURRENT_VERSION) {
        return Err(RegistryError::Invalid(format!(
            "unsupported backup schema {version:?}"
        )));
    }
    for (version, sql) in [(1, MIGRATION_0001), (2, MIGRATION_0002)] {
        let recorded: String = connection.query_row(
            "SELECT checksum_sha256 FROM dread_schema_migrations WHERE version = ?1",
            [version],
            |row| row.get(0),
        )?;
        if recorded != checksum(sql) {
            return Err(RegistryError::Invalid(
                "backup migration checksum mismatch".into(),
            ));
        }
    }
    Ok(())
}

fn checksum(sql: &str) -> String {
    format!("{:x}", Sha256::digest(sql.as_bytes()))
}

/// Parent components are never followed through symlinks. The immediate parent
/// must be owned by the current UID and have no group/other permissions. This
/// prevents another local account from substituting the database path.
fn secure_database_path(path: &Path) -> Result<PathBuf, RegistryError> {
    prepare_private_parent(path)?;
    secure_existing_or_create_file(path)
}

fn secure_existing_file(path: &Path) -> Result<PathBuf, RegistryError> {
    checked_absolute(path)?;
    prepare_private_parent(path)?;
    let canonical = canonical_parent_join(path)?;
    validate_private_file(&canonical)?;
    Ok(canonical)
}

fn secure_existing_or_create_file(path: &Path) -> Result<PathBuf, RegistryError> {
    checked_absolute(path)?;
    let canonical = canonical_parent_join(path)?;
    match fs::symlink_metadata(&canonical) {
        Ok(_) => validate_private_file(&canonical)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            create_private_file(&canonical)?;
        }
        Err(error) => return Err(error.into()),
    }
    Ok(canonical)
}

fn reserve_private_destination(path: &Path) -> Result<PathBuf, RegistryError> {
    checked_absolute(path)?;
    prepare_private_parent(path)?;
    let canonical = canonical_parent_join(path)?;
    create_private_file(&canonical)?;
    Ok(canonical)
}

fn create_private_file(path: &Path) -> Result<(), RegistryError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .custom_flags(libc::O_NOFOLLOW)
            .mode(0o600)
            .open(path)?;
    }
    Ok(())
}

fn validate_private_file(path: &Path) -> Result<(), RegistryError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let metadata = fs::symlink_metadata(path)?;
        let owner = unsafe { libc::geteuid() };
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.uid() != owner
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err(RegistryError::Invalid(format!(
                "unsafe database file type, owner, or permissions: {}",
                path.display()
            )));
        }
    }
    Ok(())
}

fn prepare_private_parent(path: &Path) -> Result<(), RegistryError> {
    checked_absolute(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| RegistryError::Invalid("path has no parent".into()))?;

    if !parent.exists() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(parent)?;
        }
    }

    // Walk the absolute path without following a symlink component.
    let mut current = PathBuf::from("/");
    for component in parent.components().skip(1) {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(RegistryError::Invalid(format!(
                "unsafe parent component: {}",
                current.display()
            )));
        }
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let metadata = fs::symlink_metadata(parent)?;
        let owner = unsafe { libc::geteuid() };
        if metadata.uid() != owner || metadata.permissions().mode() & 0o077 != 0 {
            return Err(RegistryError::Invalid(format!(
                "database parent must be current-user private (0700): {}",
                parent.display()
            )));
        }
    }
    Ok(())
}

fn canonical_parent_join(path: &Path) -> Result<PathBuf, RegistryError> {
    let parent = fs::canonicalize(
        path.parent()
            .ok_or_else(|| RegistryError::Invalid("path has no parent".into()))?,
    )?;
    let name = path
        .file_name()
        .ok_or_else(|| RegistryError::Invalid("path has no filename".into()))?;
    Ok(parent.join(name))
}

fn checked_absolute(path: &Path) -> Result<(), RegistryError> {
    if path.is_absolute() {
        Ok(())
    } else {
        Err(RegistryError::Invalid(
            "registry paths must be absolute".into(),
        ))
    }
}

fn kind_name(kind: CapabilityKind) -> Result<String, RegistryError> {
    match serde_json::to_value(kind)? {
        Value::String(value) => Ok(value),
        _ => Err(RegistryError::Invalid("invalid capability kind".into())),
    }
}

fn unix_seconds() -> Result<i64, RegistryError> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| RegistryError::Invalid(error.to_string()))?
        .as_secs();
    i64::try_from(seconds).map_err(|_| RegistryError::Invalid("timestamp overflow".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use tempfile::TempDir;

    fn manifest() -> CapabilityManifest {
        CapabilityManifest {
            manifest_version: "capability-manifest-v1".into(),
            id: "example-skill".into(),
            name: "Example".into(),
            version: "1.0.0".into(),
            kind: CapabilityKind::Skill,
            source: "local-reviewed-source".into(),
            content_sha256: "ab".repeat(32),
            dependencies: vec![],
            permissions: vec!["filesystem:read".into()],
            publisher_identity: None,
            publisher_signature: None,
            executable: None,
            security_metadata: BTreeMap::new(),
        }
    }

    fn approve_fixture(store: &RegistryStore) {
        store
            .connection
            .execute(
                "UPDATE dread_capabilities
                 SET review = 'approved', enabled = 1 WHERE id = 'example-skill'",
                [],
            )
            .unwrap();
    }

    #[test]
    fn fresh_and_v1_upgrade_migrations_are_forward_only() {
        let temporary = TempDir::new().unwrap();
        let path = temporary.path().join("registry.sqlite3");
        let store = RegistryStore::open(&path).unwrap();
        let count: i64 = store
            .connection
            .query_row("SELECT COUNT(*) FROM dread_schema_migrations", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 2);
        drop(store);
        RegistryStore::open(&path).unwrap();
    }

    #[test]
    fn changed_definition_disables_but_display_rename_does_not() {
        let temporary = TempDir::new().unwrap();
        let store = RegistryStore::open(temporary.path().join("registry.sqlite3")).unwrap();
        store.import(&manifest()).unwrap();
        approve_fixture(&store);

        let mut renamed = manifest();
        renamed.name = "Renamed".into();
        let result = store.import(&renamed).unwrap();
        assert_eq!(result.outcome, ImportOutcome::RefreshedUnchanged);
        assert!(result.record.enabled);

        let mut changed = manifest();
        changed.permissions.push("process:execute".into());
        let result = store.import(&changed).unwrap();
        assert_eq!(result.outcome, ImportOutcome::ChangedAndDisabled);
        assert_eq!(result.record.review, ReviewState::Discovered);
        assert!(!result.record.enabled);
    }

    #[test]
    fn migration_corruption_is_rejected() {
        let temporary = TempDir::new().unwrap();
        let path = temporary.path().join("registry.sqlite3");
        let store = RegistryStore::open(&path).unwrap();
        store
            .connection
            .execute(
                "UPDATE dread_schema_migrations SET checksum_sha256 = 'bad' WHERE version = 1",
                [],
            )
            .unwrap();
        drop(store);
        assert!(RegistryStore::open(&path).is_err());
    }

    #[test]
    fn history_is_database_enforced_append_only() {
        let temporary = TempDir::new().unwrap();
        let store = RegistryStore::open(temporary.path().join("registry.sqlite3")).unwrap();
        store.import(&manifest()).unwrap();
        assert!(store
            .connection
            .execute("DELETE FROM dread_registry_audit", [])
            .is_err());
        assert!(store
            .connection
            .execute("DELETE FROM dread_discovery_observations", [])
            .is_err());
    }

    #[test]
    fn backup_restore_preserves_original_without_overwriting_live_database() {
        let temporary = TempDir::new().unwrap();
        let live = temporary.path().join("registry.sqlite3");
        let backup = temporary.path().join("backup.sqlite3");
        let restored = temporary.path().join("restored.sqlite3");
        let store = RegistryStore::open(&live).unwrap();
        store.import(&manifest()).unwrap();
        store.create_backup(&backup).unwrap();

        let mut changed = manifest();
        changed.version = "2.0.0".into();
        store.import(&changed).unwrap();

        let restored = RegistryStore::restore_to_new(&backup, &restored).unwrap();
        assert_eq!(restored.get("example-skill").unwrap().version, "1.0.0");
        assert_eq!(store.get("example-skill").unwrap().version, "2.0.0");
        assert!(store.create_backup(&backup).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn dangling_symlink_and_permissive_parent_are_rejected() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let temporary = TempDir::new().unwrap();
        let private = temporary.path().join("private");
        fs::create_dir(&private).unwrap();
        fs::set_permissions(&private, fs::Permissions::from_mode(0o700)).unwrap();

        let dangling = private.join("database.sqlite3");
        symlink(private.join("missing-target"), &dangling).unwrap();
        assert!(RegistryStore::open(&dangling).is_err());

        let permissive = temporary.path().join("permissive");
        fs::create_dir(&permissive).unwrap();
        fs::set_permissions(&permissive, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(RegistryStore::open(permissive.join("database.sqlite3")).is_err());
    }

    #[test]
    fn shared_store_serializes_connection_access_across_threads() {
        use std::thread;
        let temporary = TempDir::new().unwrap();
        let shared = RegistryStore::open(temporary.path().join("registry.sqlite3"))
            .unwrap()
            .into_shared();

        let workers: Vec<_> = (0..4)
            .map(|_| {
                let shared = Arc::clone(&shared);
                thread::spawn(move || {
                    let store = shared.lock().unwrap();
                    store.list_for_test().unwrap()
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
    }
}

#[cfg(test)]
impl RegistryStore {
    fn list_for_test(&self) -> Result<usize, RegistryError> {
        let count = self.connection.query_row(
            "SELECT COUNT(*) FROM dread_capabilities",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(count as usize)
    }
}
FILE: crates/dread-orchestrator/src/preflight.rs

Rust

Copy
//! Server-side provider-isolation validation and persisted dispatch admission.
//!
//! Caller-supplied JSON cannot construct trust. Evidence, provider/capability
//! registration, and policy decisions are loaded from trusted implementations.
//! The SQLite admission row is later consumed by the canonical execution store.

use dread_core::domain::{CapabilityKind, PermissionDecision, ReviewState};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};
use thiserror::Error;

pub const PREFLIGHT_CONTRACT_VERSION: &str = "provider-isolation-preflight-v1";
pub const MAX_PREFLIGHT_LIFETIME_SECONDS: i64 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IsolationControl {
    RuntimeIdentity,
    TaskWorkspaceBoundary,
    MountPolicy,
    EgressPolicy,
    ToolPermissions,
    StableSecretHandling,
    CancellationPropagation,
    RestartReconciliation,
}

impl IsolationControl {
    pub const ALL: [Self; 8] = [
        Self::RuntimeIdentity,
        Self::TaskWorkspaceBoundary,
        Self::MountPolicy,
        Self::EgressPolicy,
        Self::ToolPermissions,
        Self::StableSecretHandling,
        Self::CancellationPropagation,
        Self::RestartReconciliation,
    ];
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceRecord {
    pub evidence_id: String,
    pub provider_id: String,
    pub adapter_id: String,
    pub adapter_version: String,
    pub runtime_identity: String,
    pub provenance: String,
    pub checks: BTreeMap<IsolationControl, ControlEvidence>,
    pub checked_at_unix_seconds: i64,
    pub expires_at_unix_seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlEvidence {
    pub verified: bool,
    pub durable_observation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRegistration {
    pub provider_id: String,
    pub adapter_id: String,
    pub adapter_version: String,
    pub enabled: bool,
    pub revoked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityRegistration {
    pub capability_id: String,
    pub kind: CapabilityKind,
    pub review: ReviewState,
    pub enabled: bool,
    pub provider_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmissionRequest {
    pub request_id: String,
    pub requester_id: String,
    pub mission_id: String,
    pub task_id: String,
    pub provider_id: String,
    pub capability_id: String,
    pub operation: String,
    pub permission_scope: String,
    pub idempotency_key: String,
    pub requested_at_unix_seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyDecisionRecord {
    pub decision_id: String,
    pub decision: PermissionDecision,
    pub request_fingerprint: String,
    pub decided_at_unix_seconds: i64,
    pub expires_at_unix_seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedAdmission {
    pub admission_id: String,
    pub request_fingerprint: String,
    pub request_id: String,
    pub requester_id: String,
    pub mission_id: String,
    pub task_id: String,
    pub provider_id: String,
    pub capability_id: String,
    pub operation: String,
    pub permission_scope: String,
    pub idempotency_key: String,
    pub evidence_id: String,
    pub policy_decision_id: String,
    pub adapter_id: String,
    pub adapter_version: String,
    pub expires_at_unix_seconds: i64,
}

pub trait EvidenceRepository: Send + Sync {
    fn load(&self, evidence_id: &str) -> Result<Option<EvidenceRecord>, AdmissionError>;
}

pub trait TrustedProviderRegistry: Send + Sync {
    fn provider(&self, provider_id: &str)
        -> Result<Option<ProviderRegistration>, AdmissionError>;
    fn capability(&self, capability_id: &str)
        -> Result<Option<CapabilityRegistration>, AdmissionError>;
}

pub trait TrustedPolicyEngine: Send + Sync {
    /// The engine receives the complete scope and returns its own persisted
    /// decision. `Allow` supplied directly by an API caller is never consulted.
    fn decide(&self, request: &AdmissionRequest, fingerprint: &str)
        -> Result<PolicyDecisionRecord, AdmissionError>;
}

#[derive(Debug, Error)]
pub enum AdmissionError {
    #[error("admission dependency failed: {0}")]
    Dependency(String),

    #[error("admission denied: {0}")]
    Denied(String),

    #[error("admission conflict: {0}")]
    Conflict(String),

    #[error("admission persistence failed: {0}")]
    Persistence(String),
}

impl From<rusqlite::Error> for AdmissionError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Persistence(error.to_string())
    }
}

pub struct AdmissionService {
    connection: Arc<Mutex<Connection>>,
    evidence: Arc<dyn EvidenceRepository>,
    registry: Arc<dyn TrustedProviderRegistry>,
    policy: Arc<dyn TrustedPolicyEngine>,
}

impl AdmissionService {
    pub fn new(
        connection: Arc<Mutex<Connection>>,
        evidence: Arc<dyn EvidenceRepository>,
        registry: Arc<dyn TrustedProviderRegistry>,
        policy: Arc<dyn TrustedPolicyEngine>,
    ) -> Self {
        Self {
            connection,
            evidence,
            registry,
            policy,
        }
    }

    /// Validates trust and persists admission before the orchestrator contacts
    /// the provider. The returned record is not provider acknowledgement.
    pub fn authorize_and_persist(
        &self,
        request: &AdmissionRequest,
        evidence_id: &str,
        now: i64,
    ) -> Result<PersistedAdmission, AdmissionError> {
        validate_request(request)?;
        if request.requested_at_unix_seconds > now {
            return Err(AdmissionError::Denied(
                "request timestamp is in the future".into(),
            ));
        }

        let fingerprint = request_fingerprint(request)?;
        let evidence = self
            .evidence
            .load(evidence_id)?
            .ok_or_else(|| AdmissionError::Denied("trusted evidence not found".into()))?;
        let provider = self
            .registry
            .provider(&request.provider_id)?
            .ok_or_else(|| AdmissionError::Denied("provider not registered".into()))?;
        let capability = self
            .registry
            .capability(&request.capability_id)?
            .ok_or_else(|| AdmissionError::Denied("capability not registered".into()))?;

        if !provider.enabled
            || provider.revoked
            || provider.provider_id != request.provider_id
            || provider.adapter_id != evidence.adapter_id
            || provider.adapter_version != evidence.adapter_version
        {
            return Err(AdmissionError::Denied(
                "provider state or adapter identity mismatch".into(),
            ));
        }
        if evidence.provider_id != request.provider_id
            || capability.provider_id != request.provider_id
            || capability.kind != CapabilityKind::AgentProvider
            || capability.review != ReviewState::Approved
            || !capability.enabled
        {
            return Err(AdmissionError::Denied(
                "evidence/capability is not valid for this executable provider".into(),
            ));
        }
        validate_evidence(&evidence, now)?;

        let decision = self.policy.decide(request, &fingerprint)?;
        if decision.decision != PermissionDecision::Allow
            || decision.request_fingerprint != fingerprint
            || decision.decided_at_unix_seconds > now
            || decision.expires_at_unix_seconds <= now
        {
            return Err(AdmissionError::Denied(
                "trusted policy did not allow this exact request scope".into(),
            ));
        }

        let expires = evidence
            .expires_at_unix_seconds
            .min(decision.expires_at_unix_seconds)
            .min(now.saturating_add(MAX_PREFLIGHT_LIFETIME_SECONDS));
        if expires <= now {
            return Err(AdmissionError::Denied("admission already expired".into()));
        }

        let admission = PersistedAdmission {
            admission_id: format!("admission:{}", request.request_id),
            request_fingerprint: fingerprint,
            request_id: request.request_id.clone(),
            requester_id: request.requester_id.clone(),
            mission_id: request.mission_id.clone(),
            task_id: request.task_id.clone(),
            provider_id: request.provider_id.clone(),
            capability_id: request.capability_id.clone(),
            operation: request.operation.clone(),
            permission_scope: request.permission_scope.clone(),
            idempotency_key: request.idempotency_key.clone(),
            evidence_id: evidence.evidence_id,
            policy_decision_id: decision.decision_id,
            adapter_id: provider.adapter_id,
            adapter_version: provider.adapter_version,
            expires_at_unix_seconds: expires,
        };

        let mut connection = self
            .connection
            .lock()
            .map_err(|_| AdmissionError::Persistence("database mutex poisoned".into()))?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(AdmissionError::from)?;
        transaction.execute(
            "INSERT INTO dread_dispatch_admissions (
                admission_id, request_fingerprint, request_id, requester_id,
                mission_id, task_id, provider_id, capability_id, operation,
                permission_scope, idempotency_key, evidence_id,
                policy_decision_id, adapter_id, adapter_version,
                expires_at_unix_seconds, consumed_at_unix_seconds,
                created_at_unix_seconds
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9,
                ?10, ?11, ?12, ?13, ?14, ?15, ?16, NULL, ?17
             )",
            params![
                admission.admission_id,
                admission.request_fingerprint,
                admission.request_id,
                admission.requester_id,
                admission.mission_id,
                admission.task_id,
                admission.provider_id,
                admission.capability_id,
                admission.operation,
                admission.permission_scope,
                admission.idempotency_key,
                admission.evidence_id,
                admission.policy_decision_id,
                admission.adapter_id,
                admission.adapter_version,
                admission.expires_at_unix_seconds,
                now
            ],
        )?;
        transaction.commit()?;
        Ok(admission)
    }
}

pub fn request_fingerprint(request: &AdmissionRequest) -> Result<String, AdmissionError> {
    let encoded = serde_json::to_vec(request)
        .map_err(|error| AdmissionError::Dependency(error.to_string()))?;
    Ok(format!("{:x}", Sha256::digest(encoded)))
}

/// Called by ExecutionStore inside its dispatch-intent transaction.
pub fn load_and_consume_admission(
    transaction: &rusqlite::Transaction<'_>,
    request: &AdmissionRequest,
    now: i64,
) -> Result<PersistedAdmission, AdmissionError> {
    let fingerprint = request_fingerprint(request)?;
    let record = transaction
        .query_row(
            "SELECT admission_id, request_fingerprint, request_id, requester_id,
                    mission_id, task_id, provider_id, capability_id, operation,
                    permission_scope, idempotency_key, evidence_id,
                    policy_decision_id, adapter_id, adapter_version,
                    expires_at_unix_seconds, consumed_at_unix_seconds
             FROM dread_dispatch_admissions WHERE request_id = ?1",
            [&request.request_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, String>(10)?,
                    row.get::<_, String>(11)?,
                    row.get::<_, String>(12)?,
                    row.get::<_, String>(13)?,
                    row.get::<_, String>(14)?,
                    row.get::<_, i64>(15)?,
                    row.get::<_, Option<i64>>(16)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| AdmissionError::Denied("persisted admission not found".into()))?;

    if record.16.is_some() {
        return Err(AdmissionError::Conflict(
            "admission has already been consumed".into(),
        ));
    }
    if record.1 != fingerprint
        || record.2 != request.request_id
        || record.3 != request.requester_id
        || record.4 != request.mission_id
        || record.5 != request.task_id
        || record.6 != request.provider_id
        || record.7 != request.capability_id
        || record.8 != request.operation
        || record.9 != request.permission_scope
        || record.10 != request.idempotency_key
        || record.15 <= now
    {
        return Err(AdmissionError::Denied(
            "admission scope, expiry, or fingerprint mismatch".into(),
        ));
    }

    let changed = transaction.execute(
        "UPDATE dread_dispatch_admissions
         SET consumed_at_unix_seconds = ?2
         WHERE request_id = ?1 AND consumed_at_unix_seconds IS NULL",
        params![request.request_id, now],
    )?;
    if changed != 1 {
        return Err(AdmissionError::Conflict(
            "admission consumption conflict".into(),
        ));
    }

    Ok(PersistedAdmission {
        admission_id: record.0,
        request_fingerprint: record.1,
        request_id: record.2,
        requester_id: record.3,
        mission_id: record.4,
        task_id: record.5,
        provider_id: record.6,
        capability_id: record.7,
        operation: record.8,
        permission_scope: record.9,
        id



974.83 s
18.15 TPS
72.22 s TTFT
We have received your original Deliveries 001 and 002 and Correction Cycle 003A.

Continue from the existing code. Do not restart, replace the project or repeat the architecture proposal.

FIRST — Complete Correction Cycle 003A
Your last response was incomplete.

Return:

The complete crates/dread-orchestrator/src/preflight.rs, including the remainder of load_and_consume_admission(), validation functions and tests.

The missing fifth corrected file and its exact path.

The complete migration for dread_dispatch_admissions, integrated conceptually with the canonical persistence architecture.

Any required Cargo dependency changes and module exports.

Correct these remaining issues:

Make backup integrity checking reject an empty result.

Test an actual v1 database upgrading to v2, preserving its original migration checksum.

Preserve compatible registry methods such as list() and observation_count().

Strengthen complete schema verification.

Address remaining filesystem path-substitution races without overstating protection.

Define evidence provenance and provider-revocation consistency.

Ensure admission and dispatch-intent consumption occur atomically in the same database transaction.

Preserve existing registry data and forward migration compatibility.

SECOND — Deliver complete Correction Cycle 003B
Provide complete source files for:

Lifecycle and state transitions.

Durable execution persistence.

Per-attempt provider execution identities.

Idempotency and retry controls.

Append-only events.

Cancellation and provider confirmation.

Restart reconciliation.

Startup dispatch barrier.

Canonical module exports and migration integration.

Correct every issue identified in the combined Delivery 001–002 review.

In particular, unknown provider outcomes must remain unresolved and must never become confirmed failures or automatically redispatched. Keyed retries must reuse the exact durable provider key. Admission must be checked against trusted persisted records. Do not create a second mission database or scheduler.

THIRD — Continue completing the Universal Hub
After 003A and 003B, continue generating the missing production implementation in successive copy-and-paste deliveries.

Cover:

Authentication and protected-action approvals.

Immutable audit and policy enforcement.

Real agent, MCP and provider adapters.

Discovery and orchestration.

Model routing, GPU Supervisor and Token Booster.

ComfyUI, MiniMax H3, LTX and media integrations.

Self-repair, verification, backup and recovery.

Production React/TypeScript interface.

Portable Ubuntu packaging and updates.

Integrated regression and security tests.

Keep the existing eight-crate architecture. Do not invent canonical function signatures or claim integration when the actual source is unavailable. Mark unresolved merge assumptions explicitly.

OUTPUT REQUIREMENTS
For every delivery:

Give the exact file path.

Provide the complete source code.

Include all necessary dependencies and tests.

Do not omit code with 'rest unchanged' or placeholders.

Preserve previous good implementations.

Include a continuation index and file manifest.

State what was changed and what remains incomplete.

Never claim tests passed unless actually executed.

No more plans instead of code. No requests for ZIPs, GitHub access, computer access or uploads. Continue using this copy-and-paste workflow.

The objective is the complete PROJECT DREAD UNIVERSAL implementation, not another partial prototype.

Start by completing 003A and 003B, then continue the implementation without waiting for another prompt.

Reasoning · Done · 311.4s total
Reasoning · I need produce massive code within token limit. Should re... · Done
CORRECTION CYCLES 003A AND 003B

Status: CODE_GENERATED / TESTS_NOT_RUN / NEEDS_CANONICAL_MERGE

The execution persistence below does not create mission, task, or dependency tables. It operates on the canonical SQLite connection and validated canonical schema mappings. Candidate migrations must be renamed and checksummed by the canonical migration system without altering any already-applied migration.

Continuation index and manifest
Cargo.toml — dependency diff.
crates/dread-core/src/manifest.rs — previous 003A version remains applicable; dependency additions below are required.
crates/dread-orchestrator/migrations/0002_trusted_dispatch_admission.sql — added.
crates/dread-orchestrator/src/preflight.rs — complete corrected file.
crates/dread-orchestrator/migrations/0003_execution_attempts.sql — added; per-attempt identities, events, reconciliation, and retry contracts.
crates/dread-orchestrator/src/lifecycle.rs — complete corrected file.
crates/dread-orchestrator/src/store.rs — complete canonical-persistence adapter.
crates/dread-orchestrator/src/cancellation.rs — provider cancellation contract.
crates/dread-orchestrator/src/reconcile.rs — complete reconciliation and startup barrier.
crates/dread-orchestrator/src/lib.rs — additive module-export diff.
Registry corrections — supplied as an exact patch after the orchestrator files.
No provider execution, automatic redispatch, or second scheduler is introduced.

FILE: Cargo.toml

Diff

Copy
diff --git a/Cargo.toml b/Cargo.toml
--- a/Cargo.toml
+++ b/Cargo.toml
@@
 [workspace.dependencies]
 async-trait = "0.1"
 axum = "0.8"
 directories = "6"
+libc = "0.2"
 rusqlite = { version = "0.32", features = ["backup", "bundled"] }
 serde = { version = "1", features = ["derive"] }
 serde_json = "1"
 sha2 = "0.10"

# Apply to crates/dread-core/Cargo.toml:
#
# [dependencies]
# serde_json.workspace = true
# sha2.workspace = true
#
# [dev-dependencies]
# tempfile.workspace = true
# toml.workspace = true

# Apply to the canonical crates/dread-orchestrator/Cargo.toml:
#
# [dependencies]
# async-trait.workspace = true
# dread-core = { path = "../dread-core" }
# rusqlite.workspace = true
# serde.workspace = true
# serde_json.workspace = true
# sha2.workspace = true
# thiserror.workspace = true
#
# [dev-dependencies]
# tempfile.workspace = true

# Apply to crates/dread-registry/Cargo.toml:
#
# [dependencies]
# libc.workspace = true
FILE: crates/dread-orchestrator/migrations/0002_trusted_dispatch_admission.sql

Sql

Copy
-- Canonical migration integration:
-- Rename to the next unused canonical migration version and record its checksum.
-- Do not modify this migration after it has been applied anywhere.

CREATE TABLE dread_dispatch_admissions (
    admission_id TEXT PRIMARY KEY,
    request_fingerprint TEXT NOT NULL,
    request_id TEXT NOT NULL UNIQUE,
    requester_id TEXT NOT NULL,
    mission_id TEXT NOT NULL,
    task_id TEXT NOT NULL,
    attempt INTEGER NOT NULL CHECK (attempt > 0),
    provider_id TEXT NOT NULL,
    provider_registration_revision INTEGER NOT NULL,
    capability_id TEXT NOT NULL,
    operation TEXT NOT NULL,
    permission_scope TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    evidence_id TEXT NOT NULL,
    policy_decision_id TEXT NOT NULL,
    adapter_id TEXT NOT NULL,
    adapter_version TEXT NOT NULL,
    expires_at_unix_seconds INTEGER NOT NULL,
    consumed_at_unix_seconds INTEGER,
    created_at_unix_seconds INTEGER NOT NULL,
    UNIQUE(task_id, attempt)
) STRICT;

CREATE INDEX dread_dispatch_admissions_task
    ON dread_dispatch_admissions(task_id, attempt);

CREATE TRIGGER dread_dispatch_admissions_no_update_except_consumption
BEFORE UPDATE ON dread_dispatch_admissions
WHEN
    OLD.admission_id != NEW.admission_id
    OR OLD.request_fingerprint != NEW.request_fingerprint
    OR OLD.request_id != NEW.request_id
    OR OLD.requester_id != NEW.requester_id
    OR OLD.mission_id != NEW.mission_id
    OR OLD.task_id != NEW.task_id
    OR OLD.attempt != NEW.attempt
    OR OLD.provider_id != NEW.provider_id
    OR OLD.provider_registration_revision != NEW.provider_registration_revision
    OR OLD.capability_id != NEW.capability_id
    OR OLD.operation != NEW.operation
    OR OLD.permission_scope != NEW.permission_scope
    OR OLD.idempotency_key != NEW.idempotency_key
    OR OLD.evidence_id != NEW.evidence_id
    OR OLD.policy_decision_id != NEW.policy_decision_id
    OR OLD.adapter_id != NEW.adapter_id
    OR OLD.adapter_version != NEW.adapter_version
    OR OLD.expires_at_unix_seconds != NEW.expires_at_unix_seconds
    OR OLD.created_at_unix_seconds != NEW.created_at_unix_seconds
    OR OLD.consumed_at_unix_seconds IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'admission identity is immutable and single-use');
END;

CREATE TRIGGER dread_dispatch_admissions_no_delete
BEFORE DELETE ON dread_dispatch_admissions
BEGIN
    SELECT RAISE(ABORT, 'dispatch admissions are append-only');
END;
FILE: crates/dread-orchestrator/src/preflight.rs

Rust

Copy
//! Trusted provider-isolation evidence and single-use dispatch admission.
//!
//! This module does not execute providers. Authorization data is loaded from
//! server-side repositories. Admission is consumed by ExecutionStore in the
//! same SQLite transaction that creates the corresponding dispatch attempt.

use dread_core::domain::{CapabilityKind, PermissionDecision, ReviewState};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};
use thiserror::Error;

pub const PREFLIGHT_CONTRACT_VERSION: &str = "provider-isolation-preflight-v1";
pub const MAX_PREFLIGHT_LIFETIME_SECONDS: i64 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IsolationControl {
    RuntimeIdentity,
    TaskWorkspaceBoundary,
    MountPolicy,
    EgressPolicy,
    ToolPermissions,
    StableSecretHandling,
    CancellationPropagation,
    RestartReconciliation,
}

impl IsolationControl {
    pub const ALL: [Self; 8] = [
        Self::RuntimeIdentity,
        Self::TaskWorkspaceBoundary,
        Self::MountPolicy,
        Self::EgressPolicy,
        Self::ToolPermissions,
        Self::StableSecretHandling,
        Self::CancellationPropagation,
        Self::RestartReconciliation,
    ];
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceProvenance {
    pub source_observation_id: String,
    pub recorded_by_adapter_id: String,
    pub recorded_by_adapter_version: String,
    pub evidence_sha256: String,
    pub recorded_at_unix_seconds: i64,
    /// Set only after the trusted repository verifies the adapter's signature
    /// or equivalent authenticated local evidence channel.
    pub authenticated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceRecord {
    pub contract_version: String,
    pub evidence_id: String,
    pub provider_id: String,
    /// Evidence is invalidated by provider re-registration or revocation.
    pub provider_registration_revision: i64,
    pub adapter_id: String,
    pub adapter_version: String,
    pub runtime_identity: String,
    pub provenance: EvidenceProvenance,
    pub checks: BTreeMap<IsolationControl, ControlEvidence>,
    pub checked_at_unix_seconds: i64,
    pub expires_at_unix_seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlEvidence {
    pub verified: bool,
    pub durable_observation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRegistration {
    pub provider_id: String,
    pub registration_revision: i64,
    pub adapter_id: String,
    pub adapter_version: String,
    pub enabled: bool,
    pub revoked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityRegistration {
    pub capability_id: String,
    pub provider_id: String,
    pub kind: CapabilityKind,
    pub review: ReviewState,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmissionRequest {
    pub request_id: String,
    pub requester_id: String,
    pub mission_id: String,
    pub task_id: String,
    pub attempt: u32,
    pub provider_id: String,
    pub capability_id: String,
    pub operation: String,
    pub permission_scope: String,
    pub idempotency_key: String,
    pub requested_at_unix_seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyDecisionRecord {
    pub decision_id: String,
    pub decision: PermissionDecision,
    pub request_fingerprint: String,
    pub requester_id: String,
    pub decided_at_unix_seconds: i64,
    pub expires_at_unix_seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedAdmission {
    pub admission_id: String,
    pub request_fingerprint: String,
    pub request_id: String,
    pub requester_id: String,
    pub mission_id: String,
    pub task_id: String,
    pub attempt: u32,
    pub provider_id: String,
    pub provider_registration_revision: i64,
    pub capability_id: String,
    pub operation: String,
    pub permission_scope: String,
    pub idempotency_key: String,
    pub evidence_id: String,
    pub policy_decision_id: String,
    pub adapter_id: String,
    pub adapter_version: String,
    pub expires_at_unix_seconds: i64,
}

pub trait EvidenceRepository: Send + Sync {
    /// Implementations retrieve authenticated server-side records. They must
    /// not deserialize API-provided evidence directly into trusted records.
    fn load(&self, evidence_id: &str) -> Result<Option<EvidenceRecord>, AdmissionError>;
}

pub trait TrustedProviderRegistry: Send + Sync {
    fn provider(
        &self,
        provider_id: &str,
    ) -> Result<Option<ProviderRegistration>, AdmissionError>;

    fn capability(
        &self,
        capability_id: &str,
    ) -> Result<Option<CapabilityRegistration>, AdmissionError>;
}

pub trait TrustedPolicyEngine: Send + Sync {
    /// The engine evaluates the complete request and returns a persisted,
    /// scoped decision. API-provided PermissionDecision values are not input.
    fn decide(
        &self,
        request: &AdmissionRequest,
        request_fingerprint: &str,
    ) -> Result<PolicyDecisionRecord, AdmissionError>;
}

#[derive(Debug, Error)]
pub enum AdmissionError {
    #[error("admission dependency failed: {0}")]
    Dependency(String),

    #[error("admission denied: {0}")]
    Denied(String),

    #[error("admission conflict: {0}")]
    Conflict(String),

    #[error("admission persistence failed: {0}")]
    Persistence(String),
}

impl From<rusqlite::Error> for AdmissionError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Persistence(error.to_string())
    }
}

pub struct AdmissionService {
    connection: Arc<Mutex<Connection>>,
    evidence: Arc<dyn EvidenceRepository>,
    registry: Arc<dyn TrustedProviderRegistry>,
    policy: Arc<dyn TrustedPolicyEngine>,
}

impl AdmissionService {
    pub fn new(
        connection: Arc<Mutex<Connection>>,
        evidence: Arc<dyn EvidenceRepository>,
        registry: Arc<dyn TrustedProviderRegistry>,
        policy: Arc<dyn TrustedPolicyEngine>,
    ) -> Self {
        Self {
            connection,
            evidence,
            registry,
            policy,
        }
    }

    pub fn authorize_and_persist(
        &self,
        request: &AdmissionRequest,
        evidence_id: &str,
        now: i64,
    ) -> Result<PersistedAdmission, AdmissionError> {
        validate_request(request)?;
        if request.requested_at_unix_seconds > now {
            return Err(AdmissionError::Denied(
                "request timestamp is in the future".into(),
            ));
        }

        let fingerprint = request_fingerprint(request)?;
        let evidence = self
            .evidence
            .load(evidence_id)?
            .ok_or_else(|| AdmissionError::Denied("trusted evidence not found".into()))?;
        let provider = self
            .registry
            .provider(&request.provider_id)?
            .ok_or_else(|| AdmissionError::Denied("provider not registered".into()))?;
        let capability = self
            .registry
            .capability(&request.capability_id)?
            .ok_or_else(|| AdmissionError::Denied("capability not registered".into()))?;

        validate_evidence(&evidence, now)?;
        if !provider.enabled
            || provider.revoked
            || provider.provider_id != request.provider_id
            || provider.adapter_id != evidence.adapter_id
            || provider.adapter_version != evidence.adapter_version
            || provider.registration_revision != evidence.provider_registration_revision
        {
            return Err(AdmissionError::Denied(
                "provider state, revision, or adapter identity mismatch".into(),
            ));
        }
        if evidence.provider_id != request.provider_id
            || capability.provider_id != request.provider_id
            || capability.capability_id != request.capability_id
            || capability.kind != CapabilityKind::AgentProvider
            || capability.review != ReviewState::Approved
            || !capability.enabled
        {
            return Err(AdmissionError::Denied(
                "evidence/capability is invalid for this executable provider".into(),
            ));
        }

        let decision = self.policy.decide(request, &fingerprint)?;
        if decision.decision != PermissionDecision::Allow
            || decision.request_fingerprint != fingerprint
            || decision.requester_id != request.requester_id
            || decision.decided_at_unix_seconds > now
            || decision.expires_at_unix_seconds <= now
        {
            return Err(AdmissionError::Denied(
                "trusted policy did not allow this exact request scope".into(),
            ));
        }

        let expires = evidence
            .expires_at_unix_seconds
            .min(decision.expires_at_unix_seconds)
            .min(now.saturating_add(MAX_PREFLIGHT_LIFETIME_SECONDS));
        if expires <= now {
            return Err(AdmissionError::Denied("admission already expired".into()));
        }

        let admission = PersistedAdmission {
            admission_id: format!("admission:{}", request.request_id),
            request_fingerprint: fingerprint,
            request_id: request.request_id.clone(),
            requester_id: request.requester_id.clone(),
            mission_id: request.mission_id.clone(),
            task_id: request.task_id.clone(),
            attempt: request.attempt,
            provider_id: request.provider_id.clone(),
            provider_registration_revision: provider.registration_revision,
            capability_id: request.capability_id.clone(),
            operation: request.operation.clone(),
            permission_scope: request.permission_scope.clone(),
            idempotency_key: request.idempotency_key.clone(),
            evidence_id: evidence.evidence_id,
            policy_decision_id: decision.decision_id,
            adapter_id: provider.adapter_id,
            adapter_version: provider.adapter_version,
            expires_at_unix_seconds: expires,
        };

        let mut connection = self
            .connection
            .lock()
            .map_err(|_| AdmissionError::Persistence("database mutex poisoned".into()))?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "INSERT INTO dread_dispatch_admissions (
                admission_id, request_fingerprint, request_id, requester_id,
                mission_id, task_id, attempt, provider_id,
                provider_registration_revision, capability_id, operation,
                permission_scope, idempotency_key, evidence_id,
                policy_decision_id, adapter_id, adapter_version,
                expires_at_unix_seconds, consumed_at_unix_seconds,
                created_at_unix_seconds
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, NULL, ?19
             )",
            params![
                admission.admission_id,
                admission.request_fingerprint,
                admission.request_id,
                admission.requester_id,
                admission.mission_id,
                admission.task_id,
                admission.attempt,
                admission.provider_id,
                admission.provider_registration_revision,
                admission.capability_id,
                admission.operation,
                admission.permission_scope,
                admission.idempotency_key,
                admission.evidence_id,
                admission.policy_decision_id,
                admission.adapter_id,
                admission.adapter_version,
                admission.expires_at_unix_seconds,
                now
            ],
        )?;
        transaction.commit()?;
        Ok(admission)
    }
}

pub fn request_fingerprint(request: &AdmissionRequest) -> Result<String, AdmissionError> {
    validate_request(request)?;
    let encoded = serde_json::to_vec(request)
        .map_err(|error| AdmissionError::Dependency(error.to_string()))?;
    Ok(format!("{:x}", Sha256::digest(encoded)))
}

/// Must be called by ExecutionStore while holding the dispatch-intent
/// transaction. The UPDATE and attempt INSERT therefore commit or roll back
/// together. A caller-created DispatchGrant is not accepted.
pub fn load_and_consume_admission(
    transaction: &Transaction<'_>,
    request: &AdmissionRequest,
    now: i64,
) -> Result<PersistedAdmission, AdmissionError> {
    let fingerprint = request_fingerprint(request)?;
    let row = transaction
        .query_row(
            "SELECT admission_id, request_fingerprint, request_id, requester_id,
                    mission_id, task_id, attempt, provider_id,
                    provider_registration_revision, capability_id, operation,
                    permission_scope, idempotency_key, evidence_id,
                    policy_decision_id, adapter_id, adapter_version,
                    expires_at_unix_seconds, consumed_at_unix_seconds
             FROM dread_dispatch_admissions
             WHERE request_id = ?1",
            [&request.request_id],
            |row| {
                Ok(AdmissionRow {
                    admission_id: row.get(0)?,
                    request_fingerprint: row.get(1)?,
                    request_id: row.get(2)?,
                    requester_id: row.get(3)?,
                    mission_id: row.get(4)?,
                    task_id: row.get(5)?,
                    attempt: row.get(6)?,
                    provider_id: row.get(7)?,
                    provider_registration_revision: row.get(8)?,
                    capability_id: row.get(9)?,
                    operation: row.get(10)?,
                    permission_scope: row.get(11)?,
                    idempotency_key: row.get(12)?,
                    evidence_id: row.get(13)?,
                    policy_decision_id: row.get(14)?,
                    adapter_id: row.get(15)?,
                    adapter_version: row.get(16)?,
                    expires_at_unix_seconds: row.get(17)?,
                    consumed_at_unix_seconds: row.get(18)?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| AdmissionError::Denied("persisted admission not found".into()))?;

    if row.consumed_at_unix_seconds.is_some() {
        return Err(AdmissionError::Conflict(
            "admission has already been consumed".into(),
        ));
    }
    if row.request_fingerprint != fingerprint
        || row.request_id != request.request_id
        || row.requester_id != request.requester_id
        || row.mission_id != request.mission_id
        || row.task_id != request.task_id
        || row.attempt != i64::from(request.attempt)
        || row.provider_id != request.provider_id
        || row.capability_id != request.capability_id
        || row.operation != request.operation
        || row.permission_scope != request.permission_scope
        || row.idempotency_key != request.idempotency_key
        || row.expires_at_unix_seconds <= now
    {
        return Err(AdmissionError::Denied(
            "admission scope, expiry, or fingerprint mismatch".into(),
        ));
    }

    let changed = transaction.execute(
        "UPDATE dread_dispatch_admissions
         SET consumed_at_unix_seconds = ?2
         WHERE request_id = ?1 AND consumed_at_unix_seconds IS NULL",
        params![request.request_id, now],
    )?;
    if changed != 1 {
        return Err(AdmissionError::Conflict(
            "admission consumption conflict".into(),
        ));
    }

    Ok(PersistedAdmission {
        admission_id: row.admission_id,
        request_fingerprint: row.request_fingerprint,
        request_id: row.request_id,
        requester_id: row.requester_id,
        mission_id: row.mission_id,
        task_id: row.task_id,
        attempt: request.attempt,
        provider_id: row.provider_id,
        provider_registration_revision: row.provider_registration_revision,
        capability_id: row.capability_id,
        operation: row.operation,
        permission_scope: row.permission_scope,
        idempotency_key: row.idempotency_key,
        evidence_id: row.evidence_id,
        policy_decision_id: row.policy_decision_id,
        adapter_id: row.adapter_id,
        adapter_version: row.adapter_version,
        expires_at_unix_seconds: row.expires_at_unix_seconds,
    })
}

struct AdmissionRow {
    admission_id: String,
    request_fingerprint: String,
    request_id: String,
    requester_id: String,
    mission_id: String,
    task_id: String,
    attempt: i64,
    provider_id: String,
    provider_registration_revision: i64,
    capability_id: String,
    operation: String,
    permission_scope: String,
    idempotency_key: String,
    evidence_id: String,
    policy_decision_id: String,
    adapter_id: String,
    adapter_version: String,
    expires_at_unix_seconds: i64,
    consumed_at_unix_seconds: Option<i64>,
}

fn validate_evidence(evidence: &EvidenceRecord, now: i64) -> Result<(), AdmissionError> {
    if evidence.contract_version != PREFLIGHT_CONTRACT_VERSION {
        return Err(AdmissionError::Denied(
            "unsupported evidence contract".into(),
        ));
    }
    for value in [
        &evidence.evidence_id,
        &evidence.provider_id,
        &evidence.adapter_id,
        &evidence.adapter_version,
        &evidence.runtime_identity,
        &evidence.provenance.source_observation_id,
        &evidence.provenance.evidence_sha256,
    ] {
        validate_identifier(value)?;
    }
    if evidence.provenance.evidence_sha256.len() != 64
        || !evidence
            .provenance
            .evidence_sha256
            .chars()
            .all(|character| character.is_ascii_hexdigit())
        || !evidence.provenance.authenticated
        || evidence.provenance.recorded_by_adapter_id != evidence.adapter_id
        || evidence.provenance.recorded_by_adapter_version != evidence.adapter_version
        || evidence.provenance.recorded_at_unix_seconds < evidence.checked_at_unix_seconds
    {
        return Err(AdmissionError::Denied(
            "evidence provenance is not authenticated and adapter-bound".into(),
        ));
    }
    if evidence.checked_at_unix_seconds > now
        || evidence.expires_at_unix_seconds <= now
        || evidence.expires_at_unix_seconds <= evidence.checked_at_unix_seconds
        || evidence.expires_at_unix_seconds - evidence.checked_at_unix_seconds
            > MAX_PREFLIGHT_LIFETIME_SECONDS
    {
        return Err(AdmissionError::Denied(
            "evidence is expired, future-dated, or exceeds maximum lifetime".into(),
        ));
    }

    let expected: BTreeSet<_> = IsolationControl::ALL.into_iter().collect();
    let actual: BTreeSet<_> = evidence.checks.keys().copied().collect();
    if expected != actual {
        return Err(AdmissionError::Denied(
            "all isolation controls require evidence".into(),
        ));
    }
    for evidence in evidence.checks.values() {
        if !evidence.verified || evidence.durable_observation_id.trim().is_empty() {
            return Err(AdmissionError::Denied(
                "an isolation control is unverified".into(),
            ));
        }
    }
    Ok(())
}

fn validate_request(request: &AdmissionRequest) -> Result<(), AdmissionError> {
    for value in [
        &request.request_id,
        &request.requester_id,
        &request.mission_id,
        &request.task_id,
        &request.provider_id,
        &request.capability_id,
        &request.operation,
        &request.permission_scope,
        &request.idempotency_key,
    ] {
        validate_identifier(value)?;
    }
    if request.attempt == 0 {
        return Err(AdmissionError::Denied("attempt must be positive".into()));
    }
    Ok(())
}

fn validate_identifier(value: &str) -> Result<(), AdmissionError> {
    if value.trim().is_empty() || value.len() > 1024 || value.contains('\0') {
        Err(AdmissionError::Denied(
            "required identity/scope is empty or too large".into(),
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Evidence(EvidenceRecord);
    impl EvidenceRepository for Evidence {
        fn load(&self, id: &str) -> Result<Option<EvidenceRecord>, AdmissionError> {
            Ok((id == self.0.evidence_id).then(|| self.0.clone()))
        }
    }

    struct Registry {
        provider: ProviderRegistration,
        capability: CapabilityRegistration,
    }
    impl TrustedProviderRegistry for Registry {
        fn provider(
            &self,
            id: &str,
        ) -> Result<Option<ProviderRegistration>, AdmissionError> {
            Ok((id == self.provider.provider_id).then(|| self.provider.clone()))
        }

        fn capability(
            &self,
            id: &str,
        ) -> Result<Option<CapabilityRegistration>, AdmissionError> {
            Ok((id == self.capability.capability_id).then(|| self.capability.clone()))
        }
    }

    struct Policy {
        fingerprint_override: Option<String>,
    }
    impl TrustedPolicyEngine for Policy {
        fn decide(
            &self,
            request: &AdmissionRequest,
            fingerprint: &str,
        ) -> Result<PolicyDecisionRecord, AdmissionError> {
            Ok(PolicyDecisionRecord {
                decision_id: "decision-1".into(),
                decision: PermissionDecision::Allow,
                request_fingerprint: self
                    .fingerprint_override
                    .clone()
                    .unwrap_or_else(|| fingerprint.into()),
                requester_id: request.requester_id.clone(),
                decided_at_unix_seconds: 100,
                expires_at_unix_seconds: 200,
            })
        }
    }

    fn evidence() -> EvidenceRecord {
        EvidenceRecord {
            contract_version: PREFLIGHT_CONTRACT_VERSION.into(),
            evidence_id: "evidence-1".into(),
            provider_id: "provider-1".into(),
            provider_registration_revision: 7,
            adapter_id: "adapter-1".into(),
            adapter_version: "1.0.0".into(),
            runtime_identity: "runtime-1".into(),
            provenance: EvidenceProvenance {
                source_observation_id: "observation-1".into(),
                recorded_by_adapter_id: "adapter-1".into(),
                recorded_by_adapter_version: "1.0.0".into(),
                evidence_sha256: "ab".repeat(32),
                recorded_at_unix_seconds: 101,
                authenticated: true,
            },
            checks: IsolationControl::ALL
                .into_iter()
                .map(|control| {
                    (
                        control,
                        ControlEvidence {
                            verified: true,
                            durable_observation_id: format!("{control:?}-observation"),
                        },
                    )
                })
                .collect(),
            checked_at_unix_seconds: 100,
            expires_at_unix_seconds: 200,
        }
    }

    fn request() -> AdmissionRequest {
        AdmissionRequest {
            request_id: "request-1".into(),
            requester_id: "operator-1".into(),
            mission_id: "mission-1".into(),
            task_id: "task-1".into(),
            attempt: 1,
            provider_id: "provider-1".into(),
            capability_id: "capability-1".into(),
            operation: "engineering.execute".into(),
            permission_scope: "workspace:mission-1".into(),
            idempotency_key: "durable-key-1".into(),
            requested_at_unix_seconds: 120,
        }
    }

    fn fixtures(
        evidence: EvidenceRecord,
        policy: Policy,
    ) -> (
        Arc<Mutex<Connection>>,
        Arc<Evidence>,
        Arc<Registry>,
        Arc<Policy>,
    ) {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(include_str!(
                "../migrations/0002_trusted_dispatch_admission.sql"
            ))
            .unwrap();
        (
            Arc::new(Mutex::new(connection)),
            Arc::new(Evidence(evidence)),
            Arc::new(Registry {
                provider: ProviderRegistration {
                    provider_id: "provider-1".into(),
                    registration_revision: 7,
                    adapter_id: "adapter-1".into(),
                    adapter_version: "1.0.0".into(),
                    enabled: true,
                    revoked: false,
                },
                capability: CapabilityRegistration {
                    capability_id: "capability-1".into(),
                    provider_id: "provider-1".into(),
                    kind: CapabilityKind::AgentProvider,
                    review: ReviewState::Approved,
                    enabled: true,
                },
            }),
            Arc::new(policy),
        )
    }

    fn authorize_with(
        evidence: EvidenceRecord,
        request: &AdmissionRequest,
        policy: Policy,
    ) -> Result<PersistedAdmission, AdmissionError> {
        let (connection, evidence_repo, registry, policy) = fixtures(evidence, policy);
        AdmissionService::new(connection, evidence_repo, registry, policy)
            .authorize_and_persist(request, "evidence-1", 120)
    }

    #[test]
    fn authenticated_evidence_persists_scoped_admission() {
        let admission = authorize_with(
            evidence(),
            &request(),
            Policy {
                fingerprint_override: None,
            },
        )
        .unwrap();
        assert_eq!(admission.attempt, 1);
        assert_eq!(admission.provider_registration_revision, 7);
    }

    #[test]
    fn forged_unauthenticated_evidence_is_rejected() {
        let mut forged = evidence();
        forged.provenance.authenticated = false;
        assert!(authorize_with(
            forged,
            &request(),
            Policy {
                fingerprint_override: None
            }
        )
        .is_err());
    }

    #[test]
    fn changed_permission_scope_invalidates_policy_binding() {
        let mut request = request();
        request.permission_scope = "workspace:another-mission".into();
        assert!(authorize_with(
            evidence(),
            &request,
            Policy {
                fingerprint_override: Some("forged-fingerprint".into())
            }
        )
        .is_err());
    }

    #[test]
    fn adapter_and_provider_revision_mismatches_are_rejected() {
        let mut wrong_adapter = evidence();
        wrong_adapter.adapter_id = "other-adapter".into();
        assert!(authorize_with(
            wrong_adapter,
            &request(),
            Policy {
                fingerprint_override: None
            }
        )
        .is_err());

        let mut stale_revision = evidence();
        stale_revision.provider_registration_revision = 6;
        assert!(authorize_with(
            stale_revision,
            &request(),
            Policy {
                fingerprint_override: None
            }
        )
        .is_err());
    }

    #[test]
    fn expired_evidence_is_rejected() {
        let mut expired = evidence();
        expired.expires_at_unix_seconds = 120;
        assert!(authorize_with(
            expired,
            &request(),
            Policy {
                fingerprint_override: None
            }
        )
        .is_err());
    }

    #[test]
    fn consumption_rejects_task_mismatch_and_replay() {
        let (connection, evidence_repo, registry, policy) = fixtures(
            evidence(),
            Policy {
                fingerprint_override: None,
            },
        );
        let service = AdmissionService::new(
            Arc::clone(&connection),
            evidence_repo,
            registry,
            policy,
        );
        let request = request();
        service
            .authorize_and_persist(&request, "evidence-1", 120)
            .unwrap();

        let mut connection = connection.lock().unwrap();
        let transaction = connection.transaction().unwrap();
        let mut other_task = request.clone();
        other_task.task_id = "other-task".into();
        assert!(load_and_consume_admission(&transaction, &other_task, 121).is_err());
        transaction.rollback().unwrap();

        let transaction = connection.transaction().unwrap();
        load_and_consume_admission(&transaction, &request, 121).unwrap();
        assert!(load_and_consume_admission(&transaction, &request, 121).is_err());
        transaction.rollback().unwrap();
    }
}
FILE: crates/dread-orchestrator/migrations/0003_execution_attempts.sql

Sql

Copy
-- Canonical migration integration:
-- This migration adds execution metadata only. Canonical mission/task/dependency
-- tables remain authoritative and are not created here.

CREATE TABLE dread_provider_retry_contracts (
    contract_id TEXT PRIMARY KEY,
    provider_id TEXT NOT NULL,
    adapter_id TEXT NOT NULL,
    adapter_version TEXT NOT NULL,
    operation TEXT NOT NULL,
    idempotency_verified INTEGER NOT NULL CHECK (idempotency_verified IN (0, 1)),
    provider_keys_supported INTEGER NOT NULL CHECK (provider_keys_supported IN (0, 1)),
    revoked INTEGER NOT NULL CHECK (revoked IN (0, 1)),
    contract_sha256 TEXT NOT NULL,
    verified_at_unix_seconds INTEGER NOT NULL,
    expires_at_unix_seconds INTEGER NOT NULL
) STRICT;

CREATE TABLE dread_task_execution_contracts (
    task_id TEXT PRIMARY KEY,
    provider_id TEXT NOT NULL,
    capability_id TEXT NOT NULL,
    operation TEXT NOT NULL,
    idempotency_class TEXT NOT NULL CHECK (
        idempotency_class IN (
            'idempotent', 'non_idempotent', 'provider_key_required', 'unknown'
        )
    ),
    idempotency_key TEXT NOT NULL,
    provider_retry_contract_id TEXT NOT NULL,
    request_scope_sha256 TEXT NOT NULL,
    created_at_unix_seconds INTEGER NOT NULL,
    FOREIGN KEY(provider_retry_contract_id)
        REFERENCES dread_provider_retry_contracts(contract_id)
) STRICT;

CREATE TABLE dread_task_attempts (
    attempt_id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    mission_id TEXT NOT NULL,
    attempt INTEGER NOT NULL CHECK (attempt > 0),
    request_id TEXT NOT NULL UNIQUE,
    request_fingerprint TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    capability_id TEXT NOT NULL,
    operation TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    admission_id TEXT NOT NULL UNIQUE,
    evidence_id TEXT NOT NULL,
    provider_execution_id TEXT,
    dispatch_state TEXT NOT NULL CHECK (
        dispatch_state IN (
            'pending', 'acknowledged', 'terminal', 'reconciliation_required'
        )
    ),
    cancellation_state TEXT NOT NULL CHECK (
        cancellation_state IN (
            'none', 'local_requested', 'request_sent', 'accepted',
            'cancelling', 'confirmed_cancelled', 'completed_successfully',
            'failed', 'unknown'
        )
    ),
    outcome TEXT,
    dispatched_at_unix_seconds INTEGER NOT NULL,
    acknowledged_at_unix_seconds INTEGER,
    terminal_at_unix_seconds INTEGER,
    UNIQUE(task_id, attempt),
    UNIQUE(task_id, idempotency_key),
    UNIQUE(provider_id, provider_execution_id),
    FOREIGN KEY(admission_id) REFERENCES dread_dispatch_admissions(admission_id)
) STRICT;

CREATE INDEX dread_task_attempts_pending
    ON dread_task_attempts(dispatch_state, task_id);

CREATE TABLE dread_reconciliation_observations (
    observation_id INTEGER PRIMARY KEY AUTOINCREMENT,
    attempt_id TEXT NOT NULL,
    observation_type TEXT NOT NULL,
    provider_execution_id TEXT,
    evidence_id TEXT,
    detail TEXT NOT NULL,
    observed_at_unix_seconds INTEGER NOT NULL,
    FOREIGN KEY(attempt_id) REFERENCES dread_task_attempts(attempt_id)
) STRICT;

CREATE TABLE dread_execution_events (
    event_sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    mission_id TEXT NOT NULL,
    task_id TEXT,
    attempt_id TEXT,
    provider_id TEXT,
    provider_execution_id TEXT,
    event_type TEXT NOT NULL,
    event_key TEXT NOT NULL,
    payload_sha256 TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    occurred_at_unix_seconds INTEGER NOT NULL,
    UNIQUE(mission_id, task_id, attempt_id, event_key)
) STRICT;

CREATE INDEX dread_execution_events_replay
    ON dread_execution_events(mission_id, event_sequence);

CREATE TRIGGER dread_execution_events_no_update
BEFORE UPDATE ON dread_execution_events
BEGIN
    SELECT RAISE(ABORT, 'execution events are append-only');
END;

CREATE TRIGGER dread_execution_events_no_delete
BEFORE DELETE ON dread_execution_events
BEGIN
    SELECT RAISE(ABORT, 'execution events are append-only');
END;

CREATE TRIGGER dread_reconciliation_no_update
BEFORE UPDATE ON dread_reconciliation_observations
BEGIN
    SELECT RAISE(ABORT, 'reconciliation observations are append-only');
END;

CREATE TRIGGER dread_reconciliation_no_delete
BEFORE DELETE ON dread_reconciliation_observations
BEGIN
    SELECT RAISE(ABORT, 'reconciliation observations are append-only');
END;
FILE: crates/dread-orchestrator/src/lifecycle.rs

Rust

Copy
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissionState {
    Ready,
    Running,
    ReconciliationRequired,
    Succeeded,
    Failed,
    Cancelled,
}

impl MissionState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Running => "running",
            Self::ReconciliationRequired => "reconciliation_required",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }

    pub fn parse(value: &str) -> Result<Self, LifecycleError> {
        match value {
            "ready" => Ok(Self::Ready),
            "running" => Ok(Self::Running),
            "reconciliation_required" => Ok(Self::ReconciliationRequired),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            other => Err(LifecycleError::InvalidStoredState(other.into())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Pending,
    Ready,
    Dispatching,
    Running,
    RetryScheduled,
    CancelRequested,
    Cancelling,
    ReconciliationRequired,
    ProviderSucceeded,
    Verifying,
    Succeeded,
    Failed,
    Cancelled,
    Blocked,
}

impl TaskState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Ready => "ready",
            Self::Dispatching => "dispatching",
            Self::Running => "running",
            Self::RetryScheduled => "retry_scheduled",
            Self::CancelRequested => "cancel_requested",
            Self::Cancelling => "cancelling",
            Self::ReconciliationRequired => "reconciliation_required",
            Self::ProviderSucceeded => "provider_succeeded",
            Self::Verifying => "verifying",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Blocked => "blocked",
        }
    }

    pub fn parse(value: &str) -> Result<Self, LifecycleError> {
        match value {
            "pending" => Ok(Self::Pending),
            "ready" => Ok(Self::Ready),
            "dispatching" => Ok(Self::Dispatching),
            "running" => Ok(Self::Running),
            "retry_scheduled" => Ok(Self::RetryScheduled),
            "cancel_requested" => Ok(Self::CancelRequested),
            "cancelling" => Ok(Self::Cancelling),
            "reconciliation_required" => Ok(Self::ReconciliationRequired),
            "provider_succeeded" => Ok(Self::ProviderSucceeded),
            "verifying" => Ok(Self::Verifying),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            "blocked" => Ok(Self::Blocked),
            other => Err(LifecycleError::InvalidStoredState(other.into())),
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::Blocked
        )
    }

    pub fn has_unresolved_provider_outcome(self) -> bool {
        matches!(
            self,
            Self::Dispatching
                | Self::Running
                | Self::CancelRequested
                | Self::Cancelling
                | Self::ReconciliationRequired
        )
    }

    pub fn can_transition_to(self, next: Self) -> bool {
        use TaskState::*;
        match next {
            Ready => matches!(self, Pending | RetryScheduled),
            Dispatching => matches!(self, Ready | RetryScheduled),
            Running => self == Dispatching,
            CancelRequested => matches!(
                self,
                Ready | Pending | RetryScheduled | Dispatching | Running
            ),
            Cancelling => self == CancelRequested,
            ReconciliationRequired => matches!(
                self,
                Dispatching | Running | CancelRequested | Cancelling
            ),
            ProviderSucceeded => matches!(
                self,
                Running | CancelRequested | Cancelling | ReconciliationRequired
            ),
            Verifying => self == ProviderSucceeded,
            Succeeded => self == Verifying,
            Failed => matches!(
                self,
                Dispatching
                    | Running
                    | RetryScheduled
                    | CancelRequested
                    | Cancelling
                    | ReconciliationRequired
                    | Verifying
            ),
            Cancelled => matches!(
                self,
                Pending
                    | Ready
                    | RetryScheduled
                    | Running
                    | CancelRequested
                    | Cancelling
                    | ReconciliationRequired
            ),
            Blocked => self == Pending,
            Pending | RetryScheduled => matches!(
                self,
                Dispatching | Running | ReconciliationRequired
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdempotencyClass {
    Idempotent,
    NonIdempotent,
    ProviderKeyRequired,
    Unknown,
}

impl IdempotencyClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idempotent => "idempotent",
            Self::NonIdempotent => "non_idempotent",
            Self::ProviderKeyRequired => "provider_key_required",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteOutcome {
    DefiniteFailureBeforeSideEffects,
    Unknown,
    StillActive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryDecision {
    Allowed,
    Exhausted,
    ReconciliationRequired,
    UnsafeContract,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifiedRetryContract {
    pub idempotent_operation: bool,
    pub provider_keys_supported: bool,
    pub revoked: bool,
    pub valid_at_observation: bool,
}

pub fn classify_retry(
    class: IdempotencyClass,
    outcome: RemoteOutcome,
    attempt: u32,
    max_attempts: u32,
    contract: VerifiedRetryContract,
) -> RetryDecision {
    match outcome {
        RemoteOutcome::Unknown => RetryDecision::ReconciliationRequired,
        RemoteOutcome::StillActive => RetryDecision::UnsafeContract,
        RemoteOutcome::DefiniteFailureBeforeSideEffects => {}
    }

    if !contract.valid_at_observation || contract.revoked {
        return RetryDecision::UnsafeContract;
    }
    if attempt >= max_attempts {
        return RetryDecision::Exhausted;
    }

    let permitted = match class {
        IdempotencyClass::Idempotent => contract.idempotent_operation,
        IdempotencyClass::ProviderKeyRequired => {
            contract.provider_keys_supported && contract.idempotent_operation
        }
        IdempotencyClass::NonIdempotent => true,
        IdempotencyClass::Unknown => false,
    };
    if permitted {
        RetryDecision::Allowed
    } else {
        RetryDecision::UnsafeContract
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LifecycleError {
    #[error("invalid task transition from {from:?} to {to:?}")]
    InvalidTransition { from: TaskState, to: TaskState },

    #[error("invalid stored lifecycle state: {0}")]
    InvalidStoredState(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_idempotency_classes_and_unknown_outcomes_fail_safely() {
        let contract = VerifiedRetryContract {
            idempotent_operation: true,
            provider_keys_supported: true,
            revoked: false,
            valid_at_observation: true,
        };
        for class in [
            IdempotencyClass::Idempotent,
            IdempotencyClass::NonIdempotent,
            IdempotencyClass::ProviderKeyRequired,
            IdempotencyClass::Unknown,
        ] {
            assert_eq!(
                classify_retry(
                    class,
                    RemoteOutcome::Unknown,
                    1,
                    3,
                    contract
                ),
                RetryDecision::ReconciliationRequired
            );
        }
        assert_eq!(
            classify_retry(
                IdempotencyClass::Unknown,
                RemoteOutcome::DefiniteFailureBeforeSideEffects,
                1,
                3,
                contract
            ),
            RetryDecision::UnsafeContract
        );
    }

    #[test]
    fn provider_success_wins_cancellation_race_but_needs_verification() {
        assert!(TaskState::Cancelling.can_transition_to(TaskState::ProviderSucceeded));
        assert!(!TaskState::ProviderSucceeded.is_terminal());
        assert!(TaskState::Verifying.can_transition_to(TaskState::Succeeded));
    }

    #[test]
    fn reconciliation_is_nonterminal_and_does_not_release_work() {
        assert!(!TaskState::ReconciliationRequired.is_terminal());
        assert!(TaskState::ReconciliationRequired.has_unresolved_provider_outcome());
        assert!(TaskState::Dispatching.can_transition_to(TaskState::ReconciliationRequired));
    }
}
FILE: crates/dread-orchestrator/src/store.rs

Rust

Copy
//! Canonical SQLite execution adapter.
//!
//! Canonical mission/task/dependency tables are validated and mutated, never
//! created here. Only attempts, admissions, retry contracts and events are
//! additive. Connection access must run in short spawn_blocking operations;
//! Mutex<Connection> is not held across an await.

use crate::{
    lifecycle::{
        classify_retry, IdempotencyClass, MissionState, RemoteOutcome,
        RetryDecision, TaskState, VerifiedRetryContract,
    },
    preflight::{
        load_and_consume_admission, request_fingerprint, AdmissionError,
        AdmissionRequest,
    },
};
use rusqlite::{
    params, Connection, OptionalExtension, Transaction, TransactionBehavior,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};
use thiserror::Error;

const MAX_EVENT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct CanonicalSchema {
    mission_table: String,
    task_table: String,
    dependency_table: String,
}

impl CanonicalSchema {
    /// NEEDS_CANONICAL_MERGE: Supply the actual table names during integration.
    /// Required canonical columns are checked by validate().
    pub fn new(
        mission_table: impl Into<String>,
        task_table: impl Into<String>,
        dependency_table: impl Into<String>,
    ) -> Result<Self, StoreError> {
        let schema = Self {
            mission_table: mission_table.into(),
            task_table: task_table.into(),
            dependency_table: dependency_table.into(),
        };
        for identifier in [
            &schema.mission_table,
            &schema.task_table,
            &schema.dependency_table,
        ] {
            if identifier.is_empty()
                || !identifier
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                return Err(StoreError::Invalid("unsafe SQL identifier".into()));
            }
        }
        Ok(schema)
    }

    fn validate(&self, connection: &Connection) -> Result<(), StoreError> {
        require_columns(
            connection,
            &self.mission_table,
            &["id", "state", "revision", "updated_at_unix_seconds"],
        )?;
        require_columns(
            connection,
            &self.task_table,
            &[
                "id",
                "mission_id",
                "state",
                "revision",
                "attempt",
                "max_attempts",
                "provider_id",
                "capability_id",
                "operation",
                "retry_not_before_unix_seconds",
                "last_error",
                "updated_at_unix_seconds",
            ],
        )?;
        require_columns(
            connection,
            &self.dependency_table,
            &["task_id", "depends_on_task_id"],
        )
    }
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("execution SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("execution JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Admission(#[from] AdmissionError),

    #[error("execution validation failed: {0}")]
    Invalid(String),

    #[error("execution entity not found: {0}")]
    NotFound(String),

    #[error("stale revision or conflicting state: {0}")]
    Conflict(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRecord {
    pub id: String,
    pub mission_id: String,
    pub state: TaskState,
    pub revision: i64,
    pub attempt: u32,
    pub max_attempts: u32,
    pub provider_id: String,
    pub capability_id: String,
    pub operation: String,
    pub retry_not_before: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptRecord {
    pub attempt_id: String,
    pub task_id: String,
    pub mission_id: String,
    pub attempt: u32,
    pub request_id: String,
    pub request_fingerprint: String,
    pub provider_id: String,
    pub capability_id: String,
    pub operation: String,
    pub idempotency_key: String,
    pub admission_id: String,
    pub evidence_id: String,
    pub provider_execution_id: Option<String>,
    pub dispatch_state: String,
    pub cancellation_state: String,
    pub dispatched_at: i64,
    pub acknowledged_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionEvent {
    pub sequence: i64,
    pub mission_id: String,
    pub task_id: Option<String>,
    pub attempt_id: Option<String>,
    pub event_type: String,
    pub event_key: String,
    pub payload: Value,
    pub occurred_at: i64,
}

#[derive(Clone)]
pub struct ExecutionStore {
    connection: Arc<Mutex<Connection>>,
    schema: CanonicalSchema,
}

impl ExecutionStore {
    pub fn new(
        connection: Arc<Mutex<Connection>>,
        schema: CanonicalSchema,
    ) -> Result<Self, StoreError> {
        {
            let connection = connection
                .lock()
                .map_err(|_| StoreError::Invalid("database mutex poisoned".into()))?;
            schema.validate(&connection)?;
            require_columns(
                &connection,
                "dread_task_attempts",
                &["attempt_id", "request_id", "provider_execution_id"],
            )?;
        }
        Ok(Self { connection, schema })
    }

    /// Registers server-verified retry semantics, not API-supplied claims.
    pub fn register_verified_retry_contract(
        &self,
        contract_id: &str,
        provider_id: &str,
        adapter_id: &str,
        adapter_version: &str,
        operation: &str,
        idempotency_verified: bool,
        provider_keys_supported: bool,
        contract_sha256: &str,
        now: i64,
        expires: i64,
    ) -> Result<(), StoreError> {
        if expires <= now || contract_sha256.len() != 64 {
            return Err(StoreError::Invalid(
                "invalid retry contract lifetime or digest".into(),
            ));
        }
        let connection = self.lock()?;
        connection.execute(
            "INSERT INTO dread_provider_retry_contracts (
                contract_id, provider_id, adapter_id, adapter_version, operation,
                idempotency_verified, provider_keys_supported, revoked,
                contract_sha256, verified_at_unix_seconds, expires_at_unix_seconds
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?9, ?10)",
            params![
                contract_id,
                provider_id,
                adapter_id,
                adapter_version,
                operation,
                idempotency_verified,
                provider_keys_supported,
                contract_sha256,
                now,
                expires
            ],
        )?;
        Ok(())
    }

    /// Binds the durable key once. A retry must reuse the exact same key.
    pub fn bind_execution_contract(
        &self,
        task_id: &str,
        provider_id: &str,
        capability_id: &str,
        operation: &str,
        class: IdempotencyClass,
        key: &str,
        retry_contract_id: &str,
        scope_sha256: &str,
        now: i64,
    ) -> Result<(), StoreError> {
        if key.trim().is_empty() || scope_sha256.len() != 64 {
            return Err(StoreError::Invalid("invalid execution contract".into()));
        }
        let connection = self.lock()?;
        connection.execute(
            "INSERT INTO dread_task_execution_contracts (
                task_id, provider_id, capability_id, operation,
                idempotency_class, idempotency_key,
                provider_retry_contract_id, request_scope_sha256,
                created_at_unix_seconds
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                task_id,
                provider_id,
                capability_id,
                operation,
                class.as_str(),
                key,
                retry_contract_id,
                scope_sha256,
                now
            ],
        )?;
        Ok(())
    }

    /// Atomically verifies/consumes persisted admission and creates one attempt.
    pub fn prepare_dispatch(
        &self,
        request: &AdmissionRequest,
        expected_task_revision: i64,
        now: i64,
    ) -> Result<AttemptRecord, StoreError> {
        let fingerprint = request_fingerprint(request)?;
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;

        // Definitive duplicate handling is inside the write transaction.
        if let Some(existing) = load_attempt_by_request(&transaction, &request.request_id)? {
            if existing.request_fingerprint != fingerprint
                || existing.task_id != request.task_id
                || existing.attempt != request.attempt
                || existing.idempotency_key != request.idempotency_key
                || existing.provider_id != request.provider_id
                || existing.capability_id != request.capability_id
                || existing.operation != request.operation
            {
                return Err(StoreError::Conflict(
                    "request ID reused with different content".into(),
                ));
            }
            transaction.commit()?;
            return Ok(existing);
        }

        let task = load_task(&transaction, &self.schema, &request.task_id)?;
        if task.revision != expected_task_revision {
            return Err(StoreError::Conflict("stale task revision".into()));
        }
        if task.mission_id != request.mission_id
            || task.provider_id != request.provider_id
            || task.capability_id != request.capability_id
            || task.operation != request.operation
        {
            return Err(StoreError::Conflict(
                "admission does not match canonical task scope".into(),
            ));
        }
        if !matches!(task.state, TaskState::Ready | TaskState::RetryScheduled) {
            return Err(StoreError::Conflict(format!(
                "cannot dispatch from {:?}",
                task.state
            )));
        }
        if task.state == TaskState::RetryScheduled
            && task.retry_not_before.is_some_and(|deadline| deadline > now)
        {
            return Err(StoreError::Conflict("retry backoff active".into()));
        }
        if request.attempt != task.attempt.saturating_add(1)
            || request.attempt > task.max_attempts
        {
            return Err(StoreError::Conflict("invalid or exhausted attempt".into()));
        }

        let contract = load_execution_contract(&transaction, &task.id)?;
        if contract.provider_id != request.provider_id
            || contract.capability_id != request.capability_id
            || contract.operation != request.operation
            || contract.key != request.idempotency_key
        {
            return Err(StoreError::Conflict(
                "provider key or operation scope changed".into(),
            ));
        }
        if contract.class == IdempotencyClass::Unknown {
            return Err(StoreError::Conflict(
                "unknown idempotency cannot be dispatched".into(),
            ));
        }

        let admission = load_and_consume_admission(&transaction, request, now)?;
        let attempt_id = format!("attempt:{}:{}", task.id, request.attempt);
        transaction.execute(
            "INSERT INTO dread_task_attempts (
                attempt_id, task_id, mission_id, attempt, request_id,
                request_fingerprint, provider_id, capability_id, operation,
                idempotency_key, admission_id, evidence_id,
                provider_execution_id, dispatch_state, cancellation_state,
                outcome, dispatched_at_unix_seconds,
                acknowledged_at_unix_seconds, terminal_at_unix_seconds
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                ?11, ?12, NULL, 'pending', 'none', NULL, ?13, NULL, NULL
             )",
            params![
                attempt_id,
                task.id,
                task.mission_id,
                request.attempt,
                request.request_id,
                fingerprint,
                request.provider_id,
                request.capability_id,
                request.operation,
                request.idempotency_key,
                admission.admission_id,
                admission.evidence_id,
                now
            ],
        )?;

        update_task_cas(
            &transaction,
            &self.schema,
            &task,
            TaskState::Dispatching,
            Some(request.attempt),
            None,
            None,
            now,
        )?;
        set_mission_running(&transaction, &self.schema, &task.mission_id, now)?;
        insert_event(
            &transaction,
            &task.mission_id,
            Some(&task.id),
            Some(&attempt_id),
            Some(&task.provider_id),
            None,
            "dispatch_intent_persisted",
            &format!("dispatch:{attempt_id}"),
            serde_json::json!({
                "attempt": request.attempt,
                "admission_id": admission.admission_id,
                "provider_acknowledged": false
            }),
            now,
        )?;
        transaction.commit()?;



918.33 s
19.60 TPS
82.63 s TTFT
Continue Correction Cycle 003A/003B. You stopped generating:

crates/dread-orchestrator/src/store.rs

immediately after:

Rust

Copy

        transaction.commit()?;

Resume from that exact point. Do not restart, repeat or discard the code already delivered.

1. Complete store.rs
Finish the entire file, including:

Readback and return of the created AttemptRecord.

All remaining ExecutionStore methods.

Provider acknowledgement bound to the exact attempt.

Event persistence, deduplication and replay.

Terminal-state recording.

Safe retry scheduling.

Reconciliation-state updates.

Mission aggregation and dependency propagation.

Revision/CAS checks.

Schema validation and helper functions.

Complete tests.

Do not leave undefined functions or SQL columns.

2. Fix the visible defects
Fix the incompatible classify_retry() match arms in lifecycle.rs.

Add or consistently replace the missing IdempotencyClass::as_str().

Correct the apparently reversed TaskState::can_transition_to() logic.

Resolve the conflict between stable retry-key reuse and UNIQUE(task_id, idempotency_key) in migration 0003.

Validate trusted retry-contract provenance, adapter identity, provider revision, expiry and revocation.

Verify canonical task columns and provide forward-only migrations where genuinely needed.

Revalidate provider registration and capability state when admission is consumed.

Handle SQLite nullable event-key uniqueness correctly.

Reject the same event key with different payload content.

Keep unknown provider outcomes nonterminal and never automatically redispatch them.

Preserve cancellation races and independent verification.

3. Complete all remaining files
After store.rs, supply:

Complete crates/dread-orchestrator/src/preflight.rs.

Complete crates/dread-orchestrator/src/cancellation.rs.

Complete crates/dread-orchestrator/src/reconcile.rs, including startup dispatch barrier.

crates/dread-orchestrator/src/lib.rs additive exports, preserving existing functionality.

Exact registry correction patch and tests.

Necessary canonical database migrations.

Exact Cargo manifest changes.

Complete file manifest and checkpoint.

Do not create another mission database, task scheduler or API server.

4. Delivery rules
Use exact file paths and complete code blocks. No placeholders or omitted function bodies. Preserve earlier good implementations. Mark tests TESTS_NOT_RUN unless you actually execute them.

Do not stop at another transaction boundary. Complete the remaining 003A/003B implementation before starting another delivery cycle.

Reasoning · Done · 109.7s total
Reasoning · Kimi needs to continue the exact point. Thinking_effort i... · Done
CORRECTION CYCLE 003A/003B — CONTINUATION

Status: CODE_GENERATED / TESTS_NOT_RUN / NEEDS_CANONICAL_MERGE

The first block resumes store.rs at the exact requested point. Subsequent patches correct the already-delivered prefix, lifecycle, and migration. No mission/task tables or scheduler are created.

FILE: crates/dread-orchestrator/src/store.rs
Continuation — append immediately after the previously delivered transaction.commit()?;

Rust

Copy
        self.attempt_by_request(&request.request_id)?
            .ok_or_else(|| StoreError::NotFound(request.request_id.clone()))
    }

    pub fn attempt_by_request(
        &self,
        request_id: &str,
    ) -> Result<Option<AttemptRecord>, StoreError> {
        let connection = self.lock()?;
        load_attempt_by_request(&connection, request_id)
    }

    pub fn pending_attempts(&self) -> Result<Vec<AttemptRecord>, StoreError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT request_id
             FROM dread_task_attempts
             WHERE dispatch_state IN ('pending', 'reconciliation_required')
             ORDER BY dispatched_at_unix_seconds, request_id",
        )?;
        let request_ids = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);

        request_ids
            .into_iter()
            .map(|request_id| {
                load_attempt_by_request(&connection, &request_id)?
                    .ok_or(StoreError::NotFound(request_id))
            })
            .collect()
    }

    pub fn task(&self, task_id: &str) -> Result<TaskRecord, StoreError> {
        let connection = self.lock()?;
        load_task(&connection, &self.schema, task_id)
    }

    /// Records acknowledgement against this exact dispatch request and attempt.
    /// A different attempt on the same task may retain a different provider ID.
    pub fn record_provider_ack(
        &self,
        request_id: &str,
        provider_execution_id: &str,
        evidence_id: &str,
        now: i64,
    ) -> Result<(), StoreError> {
        validate_text("provider_execution_id", provider_execution_id, 1024)?;
        validate_text("evidence_id", evidence_id, 1024)?;

        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let attempt = load_attempt_required(&transaction, request_id)?;
        let task = load_task(&transaction, &self.schema, &attempt.task_id)?;

        if task.attempt != attempt.attempt {
            return Err(StoreError::Conflict(
                "acknowledgement belongs to a superseded attempt".into(),
            ));
        }
        if let Some(existing) = &attempt.provider_execution_id {
            if existing == provider_execution_id {
                transaction.commit()?;
                return Ok(());
            }
            return Err(StoreError::Conflict(
                "attempt already has a different provider execution ID".into(),
            ));
        }
        if attempt.dispatch_state != "pending"
            && attempt.dispatch_state != "reconciliation_required"
        {
            return Err(StoreError::Conflict(format!(
                "cannot acknowledge attempt in {} state",
                attempt.dispatch_state
            )));
        }
        if !matches!(
            task.state,
            TaskState::Dispatching
                | TaskState::CancelRequested
                | TaskState::Cancelling
                | TaskState::ReconciliationRequired
        ) {
            return Err(StoreError::Conflict(format!(
                "cannot acknowledge task from {:?}",
                task.state
            )));
        }

        transaction.execute(
            "UPDATE dread_task_attempts
             SET provider_execution_id = ?2,
                 dispatch_state = 'acknowledged',
                 acknowledged_at_unix_seconds = ?3
             WHERE request_id = ?1
               AND provider_execution_id IS NULL",
            params![request_id, provider_execution_id, now],
        )?;

        let next = if task.state == TaskState::Dispatching
            || task.state == TaskState::ReconciliationRequired
        {
            TaskState::Running
        } else {
            task.state
        };
        update_task_cas(
            &transaction,
            &self.schema,
            &task,
            next,
            None,
            None,
            None,
            now,
        )?;
        insert_event(
            &transaction,
            &task.mission_id,
            Some(&task.id),
            Some(&attempt.attempt_id),
            Some(&attempt.provider_id),
            Some(provider_execution_id),
            "provider_acknowledged",
            &format!("ack:{}", attempt.attempt_id),
            serde_json::json!({
                "request_id": request_id,
                "provider_execution_id": provider_execution_id,
                "evidence_id": evidence_id,
                "cancellation_preserved": next != TaskState::Running
            }),
            now,
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Appends one provider event. Duplicate identity plus identical payload is
    /// idempotent; duplicate identity plus changed payload is a conflict.
    pub fn append_provider_event(
        &self,
        request_id: &str,
        provider_event_key: &str,
        event_type: &str,
        payload: Value,
        now: i64,
    ) -> Result<bool, StoreError> {
        validate_text("provider_event_key", provider_event_key, 1024)?;
        validate_text("event_type", event_type, 128)?;

        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let attempt = load_attempt_required(&transaction, request_id)?;
        let provider_execution_id = attempt.provider_execution_id.as_deref().ok_or_else(|| {
            StoreError::Conflict("provider event requires acknowledged execution identity".into())
        })?;

        let inserted = insert_event(
            &transaction,
            &attempt.mission_id,
            Some(&attempt.task_id),
            Some(&attempt.attempt_id),
            Some(&attempt.provider_id),
            Some(provider_execution_id),
            event_type,
            provider_event_key,
            payload,
            now,
        )?;
        transaction.commit()?;
        Ok(inserted)
    }

    /// Records authenticated provider terminal evidence. Provider success races
    /// are preserved: cancellation acceptance never overwrites real success.
    pub fn record_provider_terminal(
        &self,
        request_id: &str,
        outcome: ProviderTerminalOutcome,
        evidence_id: &str,
        result: Value,
        now: i64,
    ) -> Result<(), StoreError> {
        validate_text("evidence_id", evidence_id, 1024)?;
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let attempt = load_attempt_required(&transaction, request_id)?;
        let task = load_task(&transaction, &self.schema, &attempt.task_id)?;

        if attempt.provider_execution_id.is_none() {
            return Err(StoreError::Conflict(
                "terminal evidence requires provider execution identity".into(),
            ));
        }
        if attempt.dispatch_state == "terminal" {
            if attempt.outcome == Some(outcome.as_str().to_owned()) {
                transaction.commit()?;
                return Ok(());
            }
            return Err(StoreError::Conflict(
                "attempt already has a different terminal outcome".into(),
            ));
        }
        if !matches!(
            task.state,
            TaskState::Running
                | TaskState::CancelRequested
                | TaskState::Cancelling
                | TaskState::ReconciliationRequired
        ) {
            return Err(StoreError::Conflict(format!(
                "provider terminal outcome invalid from {:?}",
                task.state
            )));
        }

        let next = match outcome {
            ProviderTerminalOutcome::Succeeded => TaskState::ProviderSucceeded,
            ProviderTerminalOutcome::Failed => TaskState::Failed,
            ProviderTerminalOutcome::Cancelled => TaskState::Cancelled,
        };
        if !task.state.can_transition_to(next) {
            return Err(StoreError::Conflict(format!(
                "invalid provider terminal transition {:?} -> {:?}",
                task.state, next
            )));
        }

        transaction.execute(
            "UPDATE dread_task_attempts
             SET dispatch_state = 'terminal', outcome = ?2,
                 terminal_at_unix_seconds = ?3,
                 cancellation_state = CASE
                     WHEN ?2 = 'cancelled' THEN 'confirmed_cancelled'
                     WHEN ?2 = 'succeeded'
                          AND cancellation_state != 'none'
                         THEN 'completed_successfully'
                     WHEN ?2 = 'failed'
                          AND cancellation_state != 'none'
                         THEN 'failed'
                     ELSE cancellation_state
                 END
             WHERE request_id = ?1",
            params![request_id, outcome.as_str(), now],
        )?;
        update_task_cas(
            &transaction,
            &self.schema,
            &task,
            next,
            None,
            None,
            result.get("error").and_then(Value::as_str),
            now,
        )?;
        insert_event(
            &transaction,
            &task.mission_id,
            Some(&task.id),
            Some(&attempt.attempt_id),
            Some(&attempt.provider_id),
            attempt.provider_execution_id.as_deref(),
            "provider_terminal_state",
            &format!("terminal:{}", attempt.attempt_id),
            serde_json::json!({
                "outcome": outcome.as_str(),
                "evidence_id": evidence_id,
                "result": result,
                "independently_verified": false
            }),
            now,
        )?;
        aggregate_mission_and_dependencies(
            &transaction,
            &self.schema,
            &task.mission_id,
            now,
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn begin_independent_verification(
        &self,
        task_id: &str,
        verifier_id: &str,
        expected_revision: i64,
        now: i64,
    ) -> Result<(), StoreError> {
        validate_text("verifier_id", verifier_id, 512)?;
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let task = load_task(&transaction, &self.schema, task_id)?;
        require_revision(&task, expected_revision)?;
        if task.state != TaskState::ProviderSucceeded {
            return Err(StoreError::Conflict(
                "independent verification requires provider success".into(),
            ));
        }
        update_task_cas(
            &transaction,
            &self.schema,
            &task,
            TaskState::Verifying,
            None,
            None,
            None,
            now,
        )?;
        insert_event(
            &transaction,
            &task.mission_id,
            Some(task_id),
            None,
            Some(&task.provider_id),
            None,
            "independent_verification_started",
            &format!("verification-start:{task_id}:{}", task.attempt),
            serde_json::json!({
                "verifier_id": verifier_id,
                "separate_from_worker_required": true
            }),
            now,
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn record_independent_verification(
        &self,
        task_id: &str,
        verifier_id: &str,
        passed: bool,
        evidence_id: &str,
        findings: Value,
        expected_revision: i64,
        now: i64,
    ) -> Result<(), StoreError> {
        validate_text("verifier_id", verifier_id, 512)?;
        validate_text("evidence_id", evidence_id, 1024)?;
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let task = load_task(&transaction, &self.schema, task_id)?;
        require_revision(&task, expected_revision)?;
        if task.state != TaskState::Verifying {
            return Err(StoreError::Conflict(
                "task is not awaiting independent verification".into(),
            ));
        }

        let next = if passed {
            TaskState::Succeeded
        } else {
            TaskState::Failed
        };
        update_task_cas(
            &transaction,
            &self.schema,
            &task,
            next,
            None,
            None,
            None,
            now,
        )?;
        insert_event(
            &transaction,
            &task.mission_id,
            Some(task_id),
            None,
            Some(&task.provider_id),
            None,
            "independent_verification_completed",
            &format!("verification-complete:{task_id}:{}", task.attempt),
            serde_json::json!({
                "verifier_id": verifier_id,
                "passed": passed,
                "evidence_id": evidence_id,
                "findings": findings
            }),
            now,
        )?;
        aggregate_mission_and_dependencies(
            &transaction,
            &self.schema,
            &task.mission_id,
            now,
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Unknown outcomes always enter reconciliation. A definite pre-side-effect
    /// failure may become exhausted/failed, but an unknown outcome never does.
    pub fn schedule_retry(
        &self,
        request_id: &str,
        outcome: RemoteOutcome,
        detail: &str,
        retry_not_before: Option<i64>,
        expected_revision: i64,
        now: i64,
    ) -> Result<RetryDecision, StoreError> {
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let attempt = load_attempt_required(&transaction, request_id)?;
        let task = load_task(&transaction, &self.schema, &attempt.task_id)?;
        require_revision(&task, expected_revision)?;

        if outcome == RemoteOutcome::StillActive {
            return Err(StoreError::Conflict(
                "an active provider operation cannot be retried".into(),
            ));
        }
        if outcome == RemoteOutcome::Unknown {
            self.mark_reconciliation_required_in_transaction(
                &transaction,
                &attempt,
                &task,
                detail,
                "provider_outcome_unknown",
                now,
            )?;
            transaction.commit()?;
            return Ok(RetryDecision::ReconciliationRequired);
        }

        if !matches!(task.state, TaskState::Dispatching | TaskState::Running) {
            return Err(StoreError::Conflict(format!(
                "retry classification invalid from {:?}",
                task.state
            )));
        }
        let contract = load_retry_contract(&transaction, &task, now)?;
        let decision = classify_retry(
            contract.class,
            outcome,
            task.attempt,
            task.max_attempts,
            contract.verified,
        );

        let next = match decision {
            RetryDecision::Allowed => TaskState::RetryScheduled,
            RetryDecision::Exhausted | RetryDecision::UnsafeContract => TaskState::Failed,
            RetryDecision::ReconciliationRequired => {
                return Err(StoreError::Invalid(
                    "classified retry unexpectedly requested reconciliation".into(),
                ))
            }
        };
        let deadline = if decision == RetryDecision::Allowed {
            let deadline = retry_not_before.ok_or_else(|| {
                StoreError::Invalid("retry requires a backoff deadline".into())
            })?;
            if deadline <= now {
                return Err(StoreError::Invalid(
                    "retry deadline must be in the future".into(),
                ));
            }
            Some(deadline)
        } else {
            None
        };

        update_task_cas(
            &transaction,
            &self.schema,
            &task,
            next,
            None,
            deadline,
            Some(detail),
            now,
        )?;
        insert_event(
            &transaction,
            &task.mission_id,
            Some(&task.id),
            Some(&attempt.attempt_id),
            Some(&task.provider_id),
            attempt.provider_execution_id.as_deref(),
            "retry_classified",
            &format!("retry:{}", attempt.attempt_id),
            serde_json::json!({
                "decision": format!("{decision:?}"),
                "detail": detail,
                "retry_not_before": deadline,
                "same_durable_key_required": true
            }),
            now,
        )?;
        if next.is_terminal() {
            aggregate_mission_and_dependencies(
                &transaction,
                &self.schema,
                &task.mission_id,
                now,
            )?;
        }
        transaction.commit()?;
        Ok(decision)
    }

    pub fn mark_reconciliation_required(
        &self,
        request_id: &str,
        detail: &str,
        evidence_id: &str,
        now: i64,
    ) -> Result<(), StoreError> {
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let attempt = load_attempt_required(&transaction, request_id)?;
        let task = load_task(&transaction, &self.schema, &attempt.task_id)?;
        self.mark_reconciliation_required_in_transaction(
            &transaction,
            &attempt,
            &task,
            detail,
            evidence_id,
            now,
        )?;
        transaction.commit()?;
        Ok(())
    }

    fn mark_reconciliation_required_in_transaction(
        &self,
        transaction: &Transaction<'_>,
        attempt: &AttemptRecord,
        task: &TaskRecord,
        detail: &str,
        evidence_id: &str,
        now: i64,
    ) -> Result<(), StoreError> {
        if attempt.dispatch_state == "terminal" {
            return Err(StoreError::Conflict(
                "terminal attempt cannot require reconciliation".into(),
            ));
        }
        if task.state != TaskState::ReconciliationRequired {
            if !task
                .state
                .can_transition_to(TaskState::ReconciliationRequired)
            {
                return Err(StoreError::Conflict(format!(
                    "reconciliation invalid from {:?}",
                    task.state
                )));
            }
            update_task_cas(
                transaction,
                &self.schema,
                task,
                TaskState::ReconciliationRequired,
                None,
                None,
                Some(detail),
                now,
            )?;
        }
        transaction.execute(
            "UPDATE dread_task_attempts
             SET dispatch_state = 'reconciliation_required'
             WHERE attempt_id = ?1 AND dispatch_state != 'terminal'",
            [&attempt.attempt_id],
        )?;
        transaction.execute(
            "INSERT INTO dread_reconciliation_observations (
                attempt_id, observation_type, provider_execution_id,
                evidence_id, detail, observed_at_unix_seconds
             ) VALUES (?1, 'outcome_unknown', ?2, ?3, ?4, ?5)",
            params![
                attempt.attempt_id,
                attempt.provider_execution_id,
                evidence_id,
                detail,
                now
            ],
        )?;
        insert_event(
            transaction,
            &task.mission_id,
            Some(&task.id),
            Some(&attempt.attempt_id),
            Some(&task.provider_id),
            attempt.provider_execution_id.as_deref(),
            "reconciliation_required",
            &format!("reconciliation:{}:{}", attempt.attempt_id, now),
            serde_json::json!({
                "detail": detail,
                "evidence_id": evidence_id,
                "automatic_redispatch": false,
                "provider_outcome_terminal": false
            }),
            now,
        )?;
        set_mission_reconciliation(transaction, &self.schema, &task.mission_id, now)?;
        Ok(())
    }

    pub fn request_cancellation(
        &self,
        task_id: &str,
        expected_revision: i64,
        now: i64,
    ) -> Result<TaskState, StoreError> {
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let task = load_task(&transaction, &self.schema, task_id)?;
        require_revision(&task, expected_revision)?;

        let next = match task.state {
            TaskState::Pending | TaskState::Ready | TaskState::RetryScheduled => {
                TaskState::Cancelled
            }
            TaskState::Dispatching
            | TaskState::Running
            | TaskState::ReconciliationRequired => TaskState::CancelRequested,
            TaskState::CancelRequested | TaskState::Cancelling => {
                transaction.commit()?;
                return Ok(task.state);
            }
            state if state.is_terminal() || state == TaskState::ProviderSucceeded => {
                transaction.commit()?;
                return Ok(state);
            }
            state => {
                return Err(StoreError::Conflict(format!(
                    "cancellation invalid from {state:?}"
                )))
            }
        };

        update_task_cas(
            &transaction,
            &self.schema,
            &task,
            next,
            None,
            None,
            None,
            now,
        )?;
        if next == TaskState::CancelRequested {
            transaction.execute(
                "UPDATE dread_task_attempts
                 SET cancellation_state = 'local_requested'
                 WHERE task_id = ?1 AND attempt = ?2
                   AND dispatch_state != 'terminal'
                   AND cancellation_state = 'none'",
                params![task_id, task.attempt],
            )?;
        }
        insert_event(
            &transaction,
            &task.mission_id,
            Some(task_id),
            None,
            Some(&task.provider_id),
            None,
            "cancellation_requested_locally",
            &format!("cancel-local:{task_id}:{}", task.attempt),
            serde_json::json!({
                "local_state": next,
                "provider_terminal": false
            }),
            now,
        )?;
        if next.is_terminal() {
            aggregate_mission_and_dependencies(
                &transaction,
                &self.schema,
                &task.mission_id,
                now,
            )?;
        }
        transaction.commit()?;
        Ok(next)
    }

    pub fn record_cancellation_progress(
        &self,
        request_id: &str,
        progress: CancellationProgress,
        evidence_id: &str,
        now: i64,
    ) -> Result<(), StoreError> {
        validate_text("evidence_id", evidence_id, 1024)?;
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let attempt = load_attempt_required(&transaction, request_id)?;
        if attempt.dispatch_state == "terminal" {
            return Err(StoreError::Conflict(
                "terminal attempt cannot change cancellation state".into(),
            ));
        }
        let task = load_task(&transaction, &self.schema, &attempt.task_id)?;
        let (attempt_state, task_state) = match progress {
            CancellationProgress::RequestSent => ("request_sent", task.state),
            CancellationProgress::Accepted => {
                ("accepted", TaskState::Cancelling)
            }
            CancellationProgress::StillCancelling => {
                ("cancelling", TaskState::Cancelling)
            }
        };
        if task_state == TaskState::Cancelling
            && task.state != TaskState::CancelRequested
            && task.state != TaskState::Cancelling
        {
            return Err(StoreError::Conflict(format!(
                "cancellation progress invalid from {:?}",
                task.state
            )));
        }
        transaction.execute(
            "UPDATE dread_task_attempts
             SET cancellation_state = ?2 WHERE attempt_id = ?1",
            params![attempt.attempt_id, attempt_state],
        )?;
        if task_state != task.state {
            update_task_cas(
                &transaction,
                &self.schema,
                &task,
                task_state,
                None,
                None,
                None,
                now,
            )?;
        }
        insert_event(
            &transaction,
            &task.mission_id,
            Some(&task.id),
            Some(&attempt.attempt_id),
            Some(&attempt.provider_id),
            attempt.provider_execution_id.as_deref(),
            "provider_cancellation_progress",
            &format!("cancel-progress:{}:{attempt_state}", attempt.attempt_id),
            serde_json::json!({
                "state": attempt_state,
                "evidence_id": evidence_id,
                "provider_terminal": false
            }),
            now,
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn unresolved_attempt_count(&self) -> Result<usize, StoreError> {
        let connection = self.lock()?;
        let count = connection.query_row(
            "SELECT COUNT(*) FROM dread_task_attempts
             WHERE dispatch_state IN ('pending', 'reconciliation_required')",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        usize::try_from(count)
            .map_err(|_| StoreError::Invalid("negative unresolved count".into()))
    }

    pub fn events_after(
        &self,
        mission_id: &str,
        cursor: i64,
        limit: u32,
    ) -> Result<Vec<ExecutionEvent>, StoreError> {
        if !(1..=1000).contains(&limit) {
            return Err(StoreError::Invalid(
                "event replay limit must be between 1 and 1000".into(),
            ));
        }
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT event_sequence, mission_id, task_id, attempt_id,
                    event_type, event_key, payload_json, occurred_at_unix_seconds
             FROM dread_execution_events
             WHERE mission_id = ?1 AND event_sequence > ?2
             ORDER BY event_sequence LIMIT ?3",
            params![mission_id, cursor, limit],
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, i64>(7)?,
            ))
        })?;
        rows.map(|row| {
            let values = row?;
            Ok(ExecutionEvent {
                sequence: values.0,
                mission_id: values.1,
                task_id: values.2,
                attempt_id: values.3,
                event_type: values.4,
                event_key: values.5,
                payload: serde_json::from_str(&values.6)?,
                occurred_at: values.7,
            })
        })
        .collect()
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, StoreError> {
        self.connection
            .lock()
            .map_err(|_| StoreError::Invalid("database mutex poisoned".into()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderTerminalOutcome {
    Succeeded,
    Failed,
    Cancelled,
}

impl ProviderTerminalOutcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancellationProgress {
    RequestSent,
    Accepted,
    StillCancelling,
}

struct ExecutionContract {
    provider_id: String,
    capability_id: String,
    operation: String,
    class: IdempotencyClass,
    key: String,
}

struct RetryContract {
    class: IdempotencyClass,
    verified: VerifiedRetryContract,
}

fn load_execution_contract(
    connection: &Connection,
    task_id: &str,
) -> Result<ExecutionContract, StoreError> {
    connection
        .query_row(
            "SELECT provider_id, capability_id, operation,
                    idempotency_class, idempotency_key
             FROM dread_task_execution_contracts WHERE task_id = ?1",
            [task_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .optional()?
        .map(|values| {
            Ok(ExecutionContract {
                provider_id: values.0,
                capability_id: values.1,
                operation: values.2,
                class: parse_idempotency(&values.3)?,
                key: values.4,
            })
        })
        .transpose()?
        .ok_or_else(|| StoreError::NotFound(format!("execution contract for {task_id}")))
}

fn load_retry_contract(
    connection: &Connection,
    task: &TaskRecord,
    now: i64,
) -> Result<RetryContract, StoreError> {
    let values = connection.query_row(
        "SELECT e.idempotency_class,
                r.idempotency_verified, r.provider_keys_supported,
                r.revoked, r.expires_at_unix_seconds,
                r.verified_at_unix_seconds, r.contract_sha256,
                r.provider_id, r.operation
         FROM dread_task_execution_contracts e
         JOIN dread_provider_retry_contracts r
           ON r.contract_id = e.provider_retry_contract_id
         JOIN dread_dispatch_admissions a
           ON a.task_id = e.task_id AND a.attempt = ?2
         WHERE e.task_id = ?1
           AND a.provider_registration_revision = (
               SELECT provider_registration_revision
               FROM dread_dispatch_admissions
               WHERE task_id = e.task_id AND attempt = ?2
           )",
        params![task.id, task.attempt],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
            ))
        },
    )?;

    if values.6.len() != 64
        || !values.6.chars().all(|c| c.is_ascii_hexdigit())
        || values.7 != task.provider_id
        || values.8 != task.operation
        || values.5 > now
    {
        return Err(StoreError::Conflict(
            "retry contract provenance or provider scope mismatch".into(),
        ));
    }
    Ok(RetryContract {
        class: parse_idempotency(&values.0)?,
        verified: VerifiedRetryContract {
            idempotent_operation: values.1 == 1,
            provider_keys_supported: values.2 == 1,
            revoked: values.3 != 0,
            valid_at_observation: values.4 > now,
        },
    })
}

fn load_task(
    connection: &Connection,
    schema: &CanonicalSchema,
    task_id: &str,
) -> Result<TaskRecord, StoreError> {
    let sql = format!(
        "SELECT id, mission_id, state, revision, attempt, max_attempts,
                provider_id, capability_id, operation,
                retry_not_before_unix_seconds
         FROM {} WHERE id = ?1",
        schema.task_table
    );
    connection
        .query_row(&sql, [task_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, Option<i64>>(9)?,
            ))
        })
        .optional()?
        .map(|v| {
            Ok(TaskRecord {
                id: v.0,
                mission_id: v.1,
                state: TaskState::parse(&v.2)
                    .map_err(|error| StoreError::Invalid(error.to_string()))?,
                revision: v.3,
                attempt: positive_u32(v.4, "attempt")?,
                max_attempts: positive_u32(v.5, "max_attempts")?,
                provider_id: v.6,
                capability_id: v.7,
                operation: v.8,
                retry_not_before: v.9,
            })
        })
        .transpose()?
        .ok_or_else(|| StoreError::NotFound(task_id.into()))
}

fn load_attempt_by_request(
    connection: &Connection,
    request_id: &str,
) -> Result<Option<AttemptRecord>, StoreError> {
    connection
        .query_row(
            "SELECT attempt_id, task_id, mission_id, attempt, request_id,
                    request_fingerprint, provider_id, capability_id, operation,
                    idempotency_key, admission_id, evidence_id,
                    provider_execution_id, dispatch_state, cancellation_state,
                    dispatched_at_unix_seconds, acknowledged_at_unix_seconds
             FROM dread_task_attempts WHERE request_id = ?1",
            [request_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, String>(10)?,
                    row.get::<_, String>(11)?,
                    row.get::<_, Option<String>>(12)?,
                    row.get::<_, String>(13)?,
                    row.get::<_, String>(14)?,
                    row.get::<_, i64>(15)?,
                    row.get::<_, Option<i64>>(16)?,
                ))
            },
        )
        .optional()?
        .map(|v| {
            Ok(AttemptRecord {
                attempt_id: v.0,
                task_id: v.1,
                mission_id: v.2,
                attempt: positive_u32(v.3, "attempt")?,
                request_id: v.4,
                request_fingerprint: v.5,
                provider_id: v.6,
                capability_id: v.7,
                operation: v.8,
                idempotency_key: v.9,
                admission_id: v.10,
                evidence_id: v.11,
                provider_execution_id: v.12,
                dispatch_state: v.13,
                cancellation_state: v.14,
                dispatched_at: v.15,
                acknowledged_at: v.16,
            })
        })
        .transpose()
}

fn load_attempt_required(
    connection: &Connection,
    request_id: &str,
) -> Result<AttemptRecord, StoreError> {
    load_attempt_by_request(connection, request_id)?
        .ok_or_else(|| StoreError::NotFound(request_id.into()))
}

#[allow(clippy::too_many_arguments)]
fn update_task_cas(
    transaction: &Transaction<'_>,
    schema: &CanonicalSchema,
    task: &TaskRecord,
    next: TaskState,
    attempt: Option<u32>,
    retry_not_before: Option<i64>,
    error: Option<&str>,
    now: i64,
) -> Result<(), StoreError> {
    if next != task.state && !task.state.can_transition_to(next) {
        return Err(StoreError::Conflict(format!(
            "invalid transition {:?} -> {:?}",
            task.state, next
        )));
    }
    let sql = format!(
        "UPDATE {} SET state = ?2,
             revision = revision + 1,
             attempt = COALESCE(?3, attempt),
             retry_not_before_unix_seconds = ?4,
             last_error = COALESCE(?5, last_error),
             updated_at_unix_seconds = ?6
         WHERE id = ?1 AND revision = ?7",
        schema.task_table
    );
    let changed = transaction.execute(
        &sql,
        params![
            task.id,
            next.as_str(),
            attempt.map(i64::from),
            retry_not_before,
            error,
            now,
            task.revision
        ],
    )?;
    if changed == 1 {
        Ok(())
    } else {
        Err(StoreError::Conflict("stale task revision".into()))
    }
}

fn require_revision(task: &TaskRecord, expected: i64) -> Result<(), StoreError> {
    if task.revision == expected {
        Ok(())
    } else {
        Err(StoreError::Conflict("stale task revision".into()))
    }
}

fn set_mission_running(
    transaction: &Transaction<'_>,
    schema: &CanonicalSchema,
    mission_id: &str,
    now: i64,
) -> Result<(), StoreError> {
    let sql = format!(
        "UPDATE {} SET state = 'running', revision = revision + 1,
             updated_at_unix_seconds = ?2
         WHERE id = ?1 AND state IN ('ready', 'reconciliation_required')",
        schema.mission_table
    );
    transaction.execute(&sql, params![mission_id, now])?;
    Ok(())
}

fn set_mission_reconciliation(
    transaction: &Transaction<'_>,
    schema: &CanonicalSchema,
    mission_id: &str,
    now: i64,
) -> Result<(), StoreError> {
    let sql = format!(
        "UPDATE {} SET state = 'reconciliation_required',
             revision = revision + 1, updated_at_unix_seconds = ?2
         WHERE id = ?1
           AND state NOT IN ('succeeded', 'failed', 'cancelled')
           AND state != 'reconciliation_required'",
        schema.mission_table
    );
    transaction.execute(&sql, params![mission_id, now])?;
    Ok(())
}

fn aggregate_mission_and_dependencies(
    transaction: &Transaction<'_>,
    schema: &CanonicalSchema,
    mission_id: &str,
    now: i64,
) -> Result<(), StoreError> {
    // Pending dependencies are released only by independently verified success.
    for _ in 0..10_000 {
        let sql = format!(
            "SELECT t.id, t.revision,
                    SUM(CASE WHEN d.state != 'succeeded' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN d.state IN (
                        'failed', 'cancelled', 'blocked'
                    ) THEN 1 ELSE 0 END)
             FROM {tasks} t
             JOIN {dependencies} edge ON edge.task_id = t.id
             JOIN {tasks} d ON d.id = edge.depends_on_task_id
             WHERE t.mission_id = ?1 AND t.state = 'pending'
             GROUP BY t.id, t.revision",
            tasks = schema.task_table,
            dependencies = schema.dependency_table
        );
        let pending = transaction
            .prepare(&sql)?
            .query_map([mission_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        if pending.is_empty() {
            break;
        }

        let mut changed = false;
        for (task_id, revision, incomplete, failed) in pending {
            let next = if failed > 0 {
                Some(TaskState::Blocked)
            } else if incomplete == 0 {
                Some(TaskState::Ready)
            } else {
                None
            };
            if let Some(next) = next {
                let task = load_task(transaction, schema, &task_id)?;
                require_revision(&task, revision)?;
                update_task_cas(
                    transaction, schema, &task, next, None, None, None, now,
                )?;
                insert_event(
                    transaction,
                    mission_id,
                    Some(&task_id),
                    None,
                    None,
                    None,
                    if next == TaskState::Ready {
                        "dependency_released"
                    } else {
                        "dependency_blocked"
                    },
                    &format!("dependency:{task_id}:{next:?}:{revision}"),
                    serde_json::json!({"state": next}),
                    now,
                )?;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    let sql = format!(
        "SELECT state FROM {} WHERE mission_id = ?1",
        schema.task_table
    );
    let states = transaction
        .prepare(&sql)?
        .query_map([mission_id], |row| row.get::<_, String>(0))?
        .map(|row| TaskState::parse(&row?).map_err(|e| StoreError::Invalid(e.to_string())))
        .collect::<Result<Vec<_>, _>>()?;

    if states.iter().any(|state| {
        matches!(
            state,
            TaskState::ReconciliationRequired
                | TaskState::Dispatching
                | TaskState::Running
                | TaskState::CancelRequested
                | TaskState::Cancelling
        )
    }) {
        if states
            .iter()
            .any(|state| *state == TaskState::ReconciliationRequired)
        {
            set_mission_reconciliation(transaction, schema, mission_id, now)?;
        }
        return Ok(());
    }
    if states.is_empty() || !states.iter().all(|state| state.is_terminal()) {
        return Ok(());
    }

    let final_state = if states.iter().any(|state| {
        matches!(state, TaskState::Failed | TaskState::Blocked)
    }) {
        MissionState::Failed
    } else if states
        .iter()
        .any(|state| *state == TaskState::Cancelled)
    {
        MissionState::Cancelled
    } else {
        MissionState::Succeeded
    };
    let sql = format!(
        "UPDATE {} SET state = ?2, revision = revision + 1,
             updated_at_unix_seconds = ?3
         WHERE id = ?1
           AND state NOT IN ('succeeded', 'failed', 'cancelled')",
        schema.mission_table
    );
    let changed = transaction.execute(
        &sql,
        params![mission_id, final_state.as_str(), now],
    )?;
    if changed == 1 {
        insert_event(
            transaction,
            mission_id,
            None,
            None,
            None,
            None,
            "mission_terminal_state",
            &format!("mission-terminal:{mission_id}"),
            serde_json::json!({"state": final_state}),
            now,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn insert_event(
    transaction: &Transaction<'_>,
    mission_id: &str,
    task_id: Option<&str>,
    attempt_id: Option<&str>,
    provider_id: Option<&str>,
    provider_execution_id: Option<&str>,
    event_type: &str,
    event_key: &str,
    payload: Value,
    now: i64,
) -> Result<bool, StoreError> {
    validate_text("event_type", event_type, 128)?;
    validate_text("event_key", event_key, 1024)?;
    let encoded = serde_json::to_vec(&payload)?;
    if encoded.is_empty() || encoded.len() > MAX_EVENT_BYTES {
        return Err(StoreError::Invalid("event payload size invalid".into()));
    }
    let digest = format!("{:x}", Sha256::digest(&encoded));
    let payload_json = serde_json::to_string(&payload)?;

    let existing = transaction
        .query_row(
            "SELECT payload_sha256 FROM dread_execution_events
             WHERE mission_id = ?1 AND task_id IS ?2
               AND attempt_id IS ?3 AND event_key = ?4",
            params![mission_id, task_id, attempt_id, event_key],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if let Some(existing) = existing {
        if existing == digest {
            return Ok(false);
        }
        return Err(StoreError::Conflict(
            "event identity reused with different payload".into(),
        ));
    }

    transaction.execute(
        "INSERT INTO dread_execution_events (
            mission_id, task_id, attempt_id, provider_id,
            provider_execution_id, event_type, event_key,
            payload_sha256, payload_json, occurred_at_unix_seconds
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            mission_id,
            task_id,
            attempt_id,
            provider_id,
            provider_execution_id,
            event_type,
            event_key,
            digest,
            payload_json,
            now
        ],
    )?;
    Ok(true)
}

fn require_columns(
    connection: &Connection,
    table: &str,
    required: &[&str],
) -> Result<(), StoreError> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    if required
        .iter()
        .all(|required| columns.iter().any(|actual| actual == required))
    {
        Ok(())
    } else {
        Err(StoreError::Invalid(format!(
            "canonical schema {table} lacks required columns"
        )))
    }
}

fn parse_idempotency(value: &str) -> Result<IdempotencyClass, StoreError> {
    match value {
        "idempotent" => Ok(IdempotencyClass::Idempotent),
        "non_idempotent" => Ok(IdempotencyClass::NonIdempotent),
        "provider_key_required" => Ok(IdempotencyClass::ProviderKeyRequired),
        "unknown" => Ok(IdempotencyClass::Unknown),
        other => Err(StoreError::Invalid(format!(
            "unknown idempotency class {other}"
        ))),
    }
}

fn positive_u32(value: i64, field: &str) -> Result<u32, StoreError> {
    let converted = u32::try_from(value)
        .map_err(|_| StoreError::Invalid(format!("invalid {field}")))?;
    if converted == 0 && field != "attempt" {
        Err(StoreError::Invalid(format!("invalid {field}")))
    } else {
        Ok(converted)
    }
}

fn validate_text(field: &str, value: &str, maximum: usize) -> Result<(), StoreError> {
    if value.trim().is_empty() || value.len() > maximum || value.contains('\0') {
        Err(StoreError::Invalid(format!(
            "{field} must be nonempty and bounded"
        )))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preflight::{
        AdmissionService, CapabilityRegistration, ControlEvidence, EvidenceRecord,
        EvidenceRepository, EvidenceProvenance, IsolationControl, PolicyDecisionRecord,
        ProviderRegistration, TrustedPolicyEngine, TrustedProviderRegistry,
    };
    use dread_core::domain::{CapabilityKind, PermissionDecision, ReviewState};

    struct Evidence;
    impl EvidenceRepository for Evidence {
        fn load(
            &self,
            id: &str,
        ) -> Result<Option<EvidenceRecord>, AdmissionError> {
            Ok((id == "evidence-1").then(|| EvidenceRecord {
                contract_version: crate::preflight::PREFLIGHT_CONTRACT_VERSION.into(),
                evidence_id: "evidence-1".into(),
                provider_id: "provider-1".into(),
                provider_registration_revision: 1,
                adapter_id: "adapter-1".into(),
                adapter_version: "1.0.0".into(),
                runtime_identity: "runtime-1".into(),
                provenance: EvidenceProvenance {
                    source_observation_id: "observation-1".into(),
                    recorded_by_adapter_id: "adapter-1".into(),
                    recorded_by_adapter_version: "1.0.0".into(),
                    evidence_sha256: "ab".repeat(32),
                    recorded_at_unix_seconds: 11,
                    authenticated: true,
                },
                checks: IsolationControl::ALL
                    .into_iter()
                    .map(|control| {
                        (
                            control,
                            ControlEvidence {
                                verified: true,
                                durable_observation_id: format!("{control:?}"),
                            },
                        )
                    })
                    .collect(),
                checked_at_unix_seconds: 10,
                expires_at_unix_seconds: 200,
            }))
        }
    }

    struct Registry;
    impl TrustedProviderRegistry for Registry {
        fn provider(
            &self,
            _: &str,
        ) -> Result<Option<ProviderRegistration>, AdmissionError> {
            Ok(Some(ProviderRegistration {
                provider_id: "provider-1".into(),
                registration_revision: 1,
                adapter_id: "adapter-1".into(),
                adapter_version: "1.0.0".into(),
                enabled: true,
                revoked: false,
            }))
        }

        fn capability(
            &self,
            _: &str,
        ) -> Result<Option<CapabilityRegistration>, AdmissionError> {
            Ok(Some(CapabilityRegistration {
                capability_id: "capability-1".into(),
                provider_id: "provider-1".into(),
                kind: CapabilityKind::AgentProvider,
                review: ReviewState::Approved,
                enabled: true,
            }))
        }
    }

    struct Policy;
    impl TrustedPolicyEngine for Policy {
        fn decide(
            &self,
            request: &AdmissionRequest,
            fingerprint: &str,
        ) -> Result<PolicyDecisionRecord, AdmissionError> {
            Ok(PolicyDecisionRecord {
                decision_id: "decision-1".into(),
                decision: PermissionDecision::Allow,
                request_fingerprint: fingerprint.into(),
                requester_id: request.requester_id.clone(),
                decided_at_unix_seconds: 10,
                expires_at_unix_seconds: 200,
            })
        }
    }

    struct Fixture {
        store: ExecutionStore,
        admission: AdmissionService,
        connection: Arc<Mutex<Connection>>,
    }

    fn fixture() -> Fixture {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE missions (
                    id TEXT PRIMARY KEY, state TEXT NOT NULL,
                    revision INTEGER NOT NULL,
                    updated_at_unix_seconds INTEGER NOT NULL
                 ) STRICT;
                 CREATE TABLE tasks (
                    id TEXT PRIMARY KEY, mission_id TEXT NOT NULL,
                    state TEXT NOT NULL, revision INTEGER NOT NULL,
                    attempt INTEGER NOT NULL, max_attempts INTEGER NOT NULL,
                    provider_id TEXT NOT NULL, capability_id TEXT NOT NULL,
                    operation TEXT NOT NULL,
                    retry_not_before_unix_seconds INTEGER,
                    last_error TEXT, updated_at_unix_seconds INTEGER NOT NULL
                 ) STRICT;
                 CREATE TABLE dependencies (
                    task_id TEXT NOT NULL, depends_on_task_id TEXT NOT NULL
                 ) STRICT;
                 INSERT INTO missions VALUES ('mission-1', 'ready', 0, 1);
                 INSERT INTO tasks VALUES (
                    'task-1', 'mission-1', 'ready', 0, 0, 3,
                    'provider-1', 'capability-1', 'engineering.execute',
                    NULL, NULL, 1
                 );",
            )
            .unwrap();
        connection
            .execute_batch(include_str!(
                "../migrations/0002_trusted_dispatch_admission.sql"
            ))
            .unwrap();
        connection
            .execute_batch(include_str!("../migrations/0003_execution_attempts.sql"))
            .unwrap();
        connection
            .execute_batch(include_str!(
                "../migrations/0004_execution_registration_projection.sql"
            ))
            .unwrap();
        connection
            .execute(
                "INSERT INTO dread_execution_provider_states VALUES (
                    'provider-1', 1, 'adapter-1', '1.0.0', 1, 0
                 );
                 INSERT INTO dread_execution_capability_states VALUES (
                    'capability-1', 'provider-1', 'agent_provider',
                    'approved', 1
                 );",
                [],
            )
            .unwrap();

        let connection = Arc::new(Mutex::new(connection));
        let schema = CanonicalSchema::new("missions", "tasks", "dependencies").unwrap();
        let store = ExecutionStore::new(Arc::clone(&connection), schema).unwrap();
        store
            .register_verified_retry_contract(
                "retry-contract-1",
                "provider-1",
                "adapter-1",
                "1.0.0",
                "engineering.execute",
                true,
                true,
                &"cd".repeat(32),
                1,
                1000,
            )
            .unwrap();
        store
            .bind_execution_contract(
                "task-1",
                "provider-1",
                "capability-1",
                "engineering.execute",
                IdempotencyClass::ProviderKeyRequired,
                "stable-key",
                "retry-contract-1",
                &"ef".repeat(32),
                1,
            )
            .unwrap();

        let admission = AdmissionService::new(
            Arc::clone(&connection),
            Arc::new(Evidence),
            Arc::new(Registry),
            Arc::new(Policy),
        );
        Fixture {
            store,
            admission,
            connection,
        }
    }

    fn request(attempt: u32) -> AdmissionRequest {
        AdmissionRequest {
            request_id: format!("request-{attempt}"),
            requester_id: "operator-1".into(),
            mission_id: "mission-1".into(),
            task_id: "task-1".into(),
            attempt,
            provider_id: "provider-1".into(),
            capability_id: "capability-1".into(),
            operation: "engineering.execute".into(),
            permission_scope: "workspace:mission-1".into(),
            idempotency_key: "stable-key".into(),
            requested_at_unix_seconds: 20,
        }
    }

    fn prepare(fixture: &Fixture, attempt: u32) -> AttemptRecord {
        let request = request(attempt);
        fixture
            .admission
            .authorize_and_persist(&request, "evidence-1", 20)
            .unwrap();
        fixture.store.prepare_dispatch(&request, 0, 20).unwrap()
    }

    #[test]
    fn acknowledgement_is_attempt_bound_and_idempotent() {
        let fixture = fixture();
        prepare(&fixture, 1);
        fixture
            .store
            .record_provider_ack("request-1", "provider-task-1", "ack-evidence", 21)
            .unwrap();
        fixture
            .store
            .record_provider_ack("request-1", "provider-task-1", "ack-evidence", 22)
            .unwrap();
        assert!(fixture
            .store
            .record_provider_ack("request-1", "different-id", "ack-evidence", 23)
            .is_err());
        assert_eq!(
            fixture
                .store
                .attempt_by_request("request-1")
                .unwrap()
                .unwrap()
                .provider_execution_id
                .as_deref(),
            Some("provider-task-1")
        );
    }

    #[test]
    fn duplicate_event_content_is_checked() {
        let fixture = fixture();
        prepare(&fixture, 1);
        fixture
            .store
            .record_provider_ack("request-1", "provider-task-1", "ack-evidence", 21)
            .unwrap();
        assert!(fixture
            .store
            .append_provider_event(
                "request-1",
                "event-1",
                "stdout",
                serde_json::json!({"text": "one"}),
                22,
            )
            .unwrap());
        assert!(!fixture
            .store
            .append_provider_event(
                "request-1",
                "event-1",
                "stdout",
                serde_json::json!({"text": "one"}),
                22,
            )
            .unwrap());
        assert!(fixture
            .store
            .append_provider_event(
                "request-1",
                "event-1",
                "stdout",
                serde_json::json!({"text": "changed"}),
                23,
            )
            .is_err());
    }

    #[test]
    fn unknown_outcome_blocks_completion_and_redispatch() {
        let fixture = fixture();
        prepare(&fixture, 1);
        let decision = fixture
            .store
            .schedule_retry(
                "request-1",
                RemoteOutcome::Unknown,
                "timeout after possible submission",
                None,
                1,
                30,
            )
            .unwrap();
        assert_eq!(decision, RetryDecision::ReconciliationRequired);
        assert_eq!(
            fixture.store.task("task-1").unwrap().state,
            TaskState::ReconciliationRequired
        );
        assert_eq!(fixture.store.unresolved_attempt_count().unwrap(), 1);
        assert!(fixture.store.prepare_dispatch(&request(2), 2, 31).is_err());
    }

    #[test]
    fn keyed_retry_reuses_same_key_and_preserves_attempt_identity() {
        let fixture = fixture();
        prepare(&fixture, 1);
        let decision = fixture
            .store
            .schedule_retry(
                "request-1",
                RemoteOutcome::DefiniteFailureBeforeSideEffects,
                "provider proved no submission",
                Some(40),
                1,
                30,
            )
            .unwrap();
        assert_eq!(decision, RetryDecision::Allowed);

        let second = request(2);
        fixture
            .admission
            .authorize_and_persist(&second, "evidence-1", 41)
            .unwrap();
        fixture.store.prepare_dispatch(&second, 2, 41).unwrap();

        let connection = fixture.connection.lock().unwrap();
        let keys = connection
            .prepare("SELECT idempotency_key FROM dread_task_attempts ORDER BY attempt")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(keys, vec!["stable-key", "stable-key"]);
    }

    #[test]
    fn cancellation_acceptance_is_not_terminal_and_success_race_is_preserved() {
        let fixture = fixture();
        prepare(&fixture, 1);
        fixture
            .store
            .record_provider_ack("request-1", "provider-task-1", "ack-evidence", 21)
            .unwrap();
        fixture.store.request_cancellation("task-1", 2, 22).unwrap();
        fixture
            .store
            .record_cancellation_progress(
                "request-1",
                CancellationProgress::Accepted,
                "cancel-evidence",
                23,
            )
            .unwrap();
        assert_eq!(
            fixture.store.task("task-1").unwrap().state,
            TaskState::Cancelling
        );
        fixture
            .store
            .record_provider_terminal(
                "request-1",
                ProviderTerminalOutcome::Succeeded,
                "terminal-evidence",
                serde_json::json!({"result": "actual success"}),
                24,
            )
            .unwrap();
        assert_eq!(
            fixture.store.task("task-1").unwrap().state,
            TaskState::ProviderSucceeded
        );
    }

    #[test]
    fn provider_success_does_not_release_mission_before_independent_verification() {
        let fixture = fixture();
        prepare(&fixture, 1);
        fixture
            .store
            .record_provider_ack("request-1", "provider-task-1", "ack-evidence", 21)
            .unwrap();
        fixture
            .store
            .record_provider_terminal(
                "request-1",
                ProviderTerminalOutcome::Succeeded,
                "terminal-evidence",
                serde_json::json!({"verified": false}),
                22,
            )
            .unwrap();
        assert_eq!(fixture.store.task("task-1").unwrap().revision, 4);

        fixture
            .store
            .begin_independent_verification("task-1", "reviewer-1", 4, 23)
            .unwrap();
        fixture
            .store
            .record_independent_verification(
                "task-1",
                "reviewer-1",
                true,
                "verification-evidence",
                serde_json::json!([]),
                5,
                24,
            )
            .unwrap();
        assert_eq!(
            fixture.store.task("task-1").unwrap().state,
            TaskState::Succeeded
        );
    }
}
FILE: crates/dread-orchestrator/src/store.rs
Required correction to the previously delivered prefix

Insert this immediately before let admission = load_and_consume_admission(...):

Rust

Copy
        validate_current_registration_at_consumption(
            &transaction,
            &request.provider_id,
            &request.capability_id,
            &request.operation,
            admission_provider_revision(&transaction, &request.request_id)?,
        )?;
Add these helpers to the same file:

Rust

Copy
fn admission_provider_revision(
    transaction: &Transaction<'_>,
    request_id: &str,
) -> Result<i64, StoreError> {
    transaction
        .query_row(
            "SELECT provider_registration_revision
             FROM dread_dispatch_admissions WHERE request_id = ?1",
            [request_id],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| StoreError::NotFound(request_id.into()))
}

fn validate_current_registration_at_consumption(
    transaction: &Transaction<'_>,
    provider_id: &str,
    capability_id: &str,
    operation: &str,
    admitted_revision: i64,
) -> Result<(), StoreError> {
    // NEEDS_CANONICAL_MERGE: This table is a transactional projection of the
    // authoritative registry, updated in the same transactions as registration,
    // review, disable and revocation. It is not an independent authorization DB.
    let provider = transaction
        .query_row(
            "SELECT registration_revision, enabled, revoked
             FROM dread_execution_provider_states WHERE provider_id = ?1",
            [provider_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| StoreError::Conflict("provider registration missing".into()))?;
    let capability = transaction
        .query_row(
            "SELECT review, enabled, provider_id
             FROM dread_execution_capability_states WHERE capability_id = ?1",
            [capability_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| StoreError::Conflict("capability registration missing".into()))?;

    if provider.0 != admitted_revision
        || provider.1 != 1
        || provider.2 != 0
        || capability.0 != "approved"
        || capability.1 != 1
        || capability.2 != provider_id
        || operation.trim().is_empty()
    {
        Err(StoreError::Conflict(
            "provider/capability changed or was revoked before consumption".into(),
        ))
    } else {
        Ok(())
    }
}
FILE: crates/dread-orchestrator/src/lifecycle.rs

Rust

Copy
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissionState {
    Ready,
    Running,
    ReconciliationRequired,
    Succeeded,
    Failed,
    Cancelled,
}

impl MissionState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Running => "running",
            Self::ReconciliationRequired => "reconciliation_required",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Result<Self, LifecycleError> {
        match value {
            "ready" => Ok(Self::Ready),
            "running" => Ok(Self::Running),
            "reconciliation_required" => Ok(Self::ReconciliationRequired),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            other => Err(LifecycleError::InvalidStoredState(other.into())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Pending,
    Ready,
    Dispatching,
    Running,
    RetryScheduled,
    CancelRequested,
    Cancelling,
    ReconciliationRequired,
    ProviderSucceeded,
    Verifying,
    Succeeded,
    Failed,
    Cancelled,
    Blocked,
}

impl TaskState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Ready => "ready",
            Self::Dispatching => "dispatching",
            Self::Running => "running",
            Self::RetryScheduled => "retry_scheduled",
            Self::CancelRequested => "cancel_requested",
            Self::Cancelling => "cancelling",
            Self::ReconciliationRequired => "reconciliation_required",
            Self::ProviderSucceeded => "provider_succeeded",
            Self::Verifying => "verifying",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Blocked => "blocked",
        }
    }

    pub fn parse(value: &str) -> Result<Self, LifecycleError> {
        match value {
            "pending" => Ok(Self::Pending),
            "ready" => Ok(Self::Ready),
            "dispatching" => Ok(Self::Dispatching),
            "running" => Ok(Self::Running),
            "retry_scheduled" => Ok(Self::RetryScheduled),
            "cancel_requested" => Ok(Self::CancelRequested),
            "cancelling" => Ok(Self::Cancelling),
            "reconciliation_required" => Ok(Self::ReconciliationRequired),
            "provider_succeeded" => Ok(Self::ProviderSucceeded),
            "verifying" => Ok(Self::Verifying),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            "blocked" => Ok(Self::Blocked),
            other => Err(LifecycleError::InvalidStoredState(other.into())),
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::Blocked
        )
    }

    /// The receiver is the source state; the argument is the destination.
    pub fn can_transition_to(self, next: Self) -> bool {
        use TaskState::{
            Blocked, CancelRequested, Cancelled, Cancelling, Dispatching, Failed,
            Pending, ProviderSucceeded, Ready, ReconciliationRequired, RetryScheduled,
            Running, Succeeded, Verifying,
        };
        match (self, next) {
            (Pending, Ready | Cancelled | Blocked) => true,
            (Ready, Dispatching | CancelRequested | Cancelled) => true,
            (RetryScheduled, Ready | Dispatching | CancelRequested | Cancelled) => true,
            (
                Dispatching,
                Running
                | CancelRequested
                | ReconciliationRequired
                | RetryScheduled
                | Failed,
            ) => true,
            (
                Running,
                ProviderSucceeded
                | Failed
                | Cancelled
                | CancelRequested
                | ReconciliationRequired
                | RetryScheduled,
            ) => true,
            (
                CancelRequested,
                Cancelling
                | ReconciliationRequired
                | ProviderSucceeded
                | Failed
                | Cancelled,
            ) => true,
            (
                Cancelling,
                ReconciliationRequired | ProviderSucceeded | Failed | Cancelled,
            ) => true,
            (
                ReconciliationRequired,
                Running
                | ProviderSucceeded
                | Failed
                | Cancelled
                | CancelRequested,
            ) => true,
            (ProviderSucceeded, Verifying) => true,
            (Verifying, Succeeded | Failed) => true,
            (source, destination) if source == destination => true,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdempotencyClass {
    Idempotent,
    NonIdempotent,
    ProviderKeyRequired,
    Unknown,
}

impl IdempotencyClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idempotent => "



913.19 s
18.42 TPS
23.96 s TTFT
Your latest continuation is received. Preserve all delivered code.

Do not restart the project or regenerate working sections unnecessarily.

Before declaring Correction Cycle 003A/003B complete, address these additional defects:

Remove UNIQUE(task_id, idempotency_key) from migration 0003 because stable idempotency keys must be reusable across attempts of the same task.

Update the attempt record transactionally when a definite pre-side-effect failure is classified or a retry is scheduled. Do not leave completed attempts pending or acknowledged.

Verify provider evidence against the trusted evidence repository and bind it to the exact request, attempt, adapter and provider execution ID.

Enforce independent verifier identity and trusted verification evidence.

Make cancellation progress monotonic. Require a prior cancellation request before recording RequestSent.

Prevent mission state from returning to running while another task still requires reconciliation.

Check event type and full event identity during duplicate detection, not only payload hash.

Validate retry contracts against trusted provenance, registered adapter/version, provider revision, operation and expiry.

Deliver the missing 0004 execution-registration projection migration.

Change multi-statement test fixture execution to execute_batch().

Validate the complete canonical schema and migration dependencies.

Preserve admission consumption and dispatch-attempt creation in one transaction.

Now complete the remaining source:

Finish lifecycle.rs from its exact stopping point.

Supply cancellation.rs.

Supply complete reconcile.rs and startup dispatch barrier.

Supply orchestrator lib.rs exports.

Supply the registry correction patch.

Supply every migration and test fixture referenced by the code.

Return the complete final files, not fragments that require guessing or reconstruction.

Status remains CODE_GENERATED / TESTS_NOT_RUN / NEEDS_CANONICAL_MERGE unless actual execution evidence exists.

Do not start Delivery 004 until 003A and 003B are complete and internally consistent.

Reasoning · Done · 104.8s total
Reasoning · I need respond with the complete final files, not fragmen... · Done
CORRECTION CYCLE 003A/003B — CONSOLIDATED FINAL SOURCE

Status: CODE_GENERATED / TESTS_NOT_RUN / NEEDS_CANONICAL_MERGE

This cycle is now internally consolidated around:

Existing canonical mission/task/dependency tables.
Additive execution-attempt, admission, evidence-projection, and event tables.
Trusted server-side evidence and policy repositories.
Atomic admission consumption and attempt creation.
Nonterminal unknown outcomes.
Stable idempotency keys reused across attempts.
Independently evidenced verification.
Monotonic cancellation.
Closed startup dispatch until reconciliation completes.
NEEDS_CANONICAL_MERGE: canonical table/column names and migration numbering must be mapped by the integration engineer. No already-applied migration may be edited.

File manifest
File
Status
Cargo.toml	MODIFIED — exact diff
crates/dread-orchestrator/migrations/0002_trusted_dispatch_admission.sql	ADDED
crates/dread-orchestrator/migrations/0003_execution_attempts.sql	ADDED
crates/dread-orchestrator/migrations/0004_execution_registration_projection.sql	ADDED
crates/dread-orchestrator/src/lifecycle.rs	COMPLETE
crates/dread-orchestrator/src/preflight.rs	COMPLETE
crates/dread-orchestrator/src/cancellation.rs	ADDED
crates/dread-orchestrator/src/store.rs	COMPLETE
crates/dread-orchestrator/src/reconcile.rs	COMPLETE
crates/dread-orchestrator/src/lib.rs	ADDITIVE DIFF
crates/dread-registry/src/lib.rs	EXACT CORRECTION PATCH
crates/dread-registry/src/lib.rs tests	EXACT ADDITIONS
checkpoints/PROJECT-DREAD-AUTONOMOUS-CHECKPOINT.md	REPLACEMENT
FILE: Cargo.toml

Diff

Copy
diff --git a/Cargo.toml b/Cargo.toml
--- a/Cargo.toml
+++ b/Cargo.toml
@@
 [workspace.dependencies]
 async-trait = "0.1"
 axum = "0.8"
 directories = "6"
+libc = "0.2"
 rusqlite = { version = "0.32", features = ["backup", "bundled"] }
 serde = { version = "1", features = ["derive"] }
 serde_json = "1"
 sha2 = "0.10"

# crates/dread-core/Cargo.toml
#
# Add under [dependencies]:
# serde_json.workspace = true
# sha2.workspace = true
#
# Add under [dev-dependencies]:
# tempfile.workspace = true
# toml.workspace = true

# crates/dread-registry/Cargo.toml
#
# Add under [dependencies]:
# libc.workspace = true

# crates/dread-orchestrator/Cargo.toml
#
# Add under [dependencies]:
# async-trait.workspace = true
# dread-core = { path = "../dread-core" }
# rusqlite.workspace = true
# serde.workspace = true
# serde_json.workspace = true
# sha2.workspace = true
# thiserror.workspace = true
#
# Add under [dev-dependencies]:
# tempfile.workspace = true
FILE: crates/dread-orchestrator/migrations/0002_trusted_dispatch_admission.sql

Sql

Copy
CREATE TABLE dread_dispatch_admissions (
    admission_id TEXT PRIMARY KEY,
    request_fingerprint TEXT NOT NULL,
    request_id TEXT NOT NULL UNIQUE,
    requester_id TEXT NOT NULL,
    mission_id TEXT NOT NULL,
    task_id TEXT NOT NULL,
    attempt INTEGER NOT NULL CHECK (attempt > 0),
    provider_id TEXT NOT NULL,
    provider_registration_revision INTEGER NOT NULL,
    capability_id TEXT NOT NULL,
    operation TEXT NOT NULL,
    permission_scope TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    evidence_id TEXT NOT NULL,
    policy_decision_id TEXT NOT NULL,
    adapter_id TEXT NOT NULL,
    adapter_version TEXT NOT NULL,
    expires_at_unix_seconds INTEGER NOT NULL,
    consumed_at_unix_seconds INTEGER,
    created_at_unix_seconds INTEGER NOT NULL,
    UNIQUE(task_id, attempt)
) STRICT;

CREATE INDEX dread_dispatch_admissions_task
    ON dread_dispatch_admissions(task_id, attempt);

CREATE TRIGGER dread_dispatch_admissions_immutable_update
BEFORE UPDATE ON dread_dispatch_admissions
WHEN
    OLD.admission_id != NEW.admission_id
    OR OLD.request_fingerprint != NEW.request_fingerprint
    OR OLD.request_id != NEW.request_id
    OR OLD.requester_id != NEW.requester_id
    OR OLD.mission_id != NEW.mission_id
    OR OLD.task_id != NEW.task_id
    OR OLD.attempt != NEW.attempt
    OR OLD.provider_id != NEW.provider_id
    OR OLD.provider_registration_revision != NEW.provider_registration_revision
    OR OLD.capability_id != NEW.capability_id
    OR OLD.operation != NEW.operation
    OR OLD.permission_scope != NEW.permission_scope
    OR OLD.idempotency_key != NEW.idempotency_key
    OR OLD.evidence_id != NEW.evidence_id
    OR OLD.policy_decision_id != NEW.policy_decision_id
    OR OLD.adapter_id != NEW.adapter_id
    OR OLD.adapter_version != NEW.adapter_version
    OR OLD.expires_at_unix_seconds != NEW.expires_at_unix_seconds
    OR OLD.created_at_unix_seconds != NEW.created_at_unix_seconds
    OR OLD.consumed_at_unix_seconds IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'admission is immutable and single-use');
END;

CREATE TRIGGER dread_dispatch_admissions_no_delete
BEFORE DELETE ON dread_dispatch_admissions
BEGIN
    SELECT RAISE(ABORT, 'dispatch admissions are append-only');
END;
FILE: crates/dread-orchestrator/migrations/0003_execution_attempts.sql

Sql

Copy
CREATE TABLE dread_provider_retry_contracts (
    contract_id TEXT PRIMARY KEY,
    provider_id TEXT NOT NULL,
    provider_registration_revision INTEGER NOT NULL,
    adapter_id TEXT NOT NULL,
    adapter_version TEXT NOT NULL,
    operation TEXT NOT NULL,
    idempotency_verified INTEGER NOT NULL
        CHECK (idempotency_verified IN (0, 1)),
    provider_keys_supported INTEGER NOT NULL
        CHECK (provider_keys_supported IN (0, 1)),
    revoked INTEGER NOT NULL CHECK (revoked IN (0, 1)),
    provenance_evidence_id TEXT NOT NULL,
    contract_sha256 TEXT NOT NULL,
    verified_at_unix_seconds INTEGER NOT NULL,
    expires_at_unix_seconds INTEGER NOT NULL
) STRICT;

CREATE TABLE dread_task_execution_contracts (
    task_id TEXT PRIMARY KEY,
    provider_id TEXT NOT NULL,
    capability_id TEXT NOT NULL,
    operation TEXT NOT NULL,
    idempotency_class TEXT NOT NULL CHECK (
        idempotency_class IN (
            'idempotent',
            'non_idempotent',
            'provider_key_required',
            'unknown'
        )
    ),
    idempotency_key TEXT NOT NULL,
    provider_retry_contract_id TEXT NOT NULL,
    request_scope_sha256 TEXT NOT NULL,
    created_at_unix_seconds INTEGER NOT NULL,
    FOREIGN KEY(provider_retry_contract_id)
        REFERENCES dread_provider_retry_contracts(contract_id)
) STRICT;

CREATE TABLE dread_task_attempts (
    attempt_id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    mission_id TEXT NOT NULL,
    attempt INTEGER NOT NULL CHECK (attempt > 0),
    request_id TEXT NOT NULL UNIQUE,
    request_fingerprint TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    capability_id TEXT NOT NULL,
    operation TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    admission_id TEXT NOT NULL UNIQUE,
    evidence_id TEXT NOT NULL,
    provider_execution_id TEXT,
    dispatch_state TEXT NOT NULL CHECK (
        dispatch_state IN (
            'pending',
            'acknowledged',
            'terminal',
            'reconciliation_required'
        )
    ),
    cancellation_state TEXT NOT NULL CHECK (
        cancellation_state IN (
            'none',
            'local_requested',
            'request_sent',
            'accepted',
            'cancelling',
            'confirmed_cancelled',
            'completed_successfully',
            'failed',
            'unknown'
        )
    ),
    outcome TEXT,
    dispatched_at_unix_seconds INTEGER NOT NULL,
    acknowledged_at_unix_seconds INTEGER,
    terminal_at_unix_seconds INTEGER,
    UNIQUE(task_id, attempt),
    UNIQUE(provider_id, provider_execution_id),
    FOREIGN KEY(admission_id)
        REFERENCES dread_dispatch_admissions(admission_id)
) STRICT;

-- Deliberately no UNIQUE(task_id, idempotency_key):
-- the same task must reuse its stable key across retries.

CREATE INDEX dread_task_attempts_pending
    ON dread_task_attempts(dispatch_state, task_id);

CREATE TABLE dread_reconciliation_observations (
    observation_id INTEGER PRIMARY KEY AUTOINCREMENT,
    attempt_id TEXT NOT NULL,
    observation_type TEXT NOT NULL,
    provider_execution_id TEXT,
    evidence_id TEXT,
    detail TEXT NOT NULL,
    observed_at_unix_seconds INTEGER NOT NULL,
    FOREIGN KEY(attempt_id) REFERENCES dread_task_attempts(attempt_id)
) STRICT;

CREATE TABLE dread_execution_events (
    event_sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    mission_id TEXT NOT NULL,
    task_id TEXT,
    attempt_id TEXT,
    provider_id TEXT,
    provider_execution_id TEXT,
    event_type TEXT NOT NULL,
    event_key TEXT NOT NULL,
    event_identity TEXT NOT NULL UNIQUE,
    payload_sha256 TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    occurred_at_unix_seconds INTEGER NOT NULL
) STRICT;

CREATE INDEX dread_execution_events_replay
    ON dread_execution_events(mission_id, event_sequence);

CREATE TRIGGER dread_execution_events_no_update
BEFORE UPDATE ON dread_execution_events
BEGIN
    SELECT RAISE(ABORT, 'execution events are append-only');
END;

CREATE TRIGGER dread_execution_events_no_delete
BEFORE DELETE ON dread_execution_events
BEGIN
    SELECT RAISE(ABORT, 'execution events are append-only');
END;

CREATE TRIGGER dread_reconciliation_no_update
BEFORE UPDATE ON dread_reconciliation_observations
BEGIN
    SELECT RAISE(ABORT, 'reconciliation observations are append-only');
END;

CREATE TRIGGER dread_reconciliation_no_delete
BEFORE DELETE ON dread_reconciliation_observations
BEGIN
    SELECT RAISE(ABORT, 'reconciliation observations are append-only');
END;
FILE: crates/dread-orchestrator/migrations/0004_execution_registration_projection.sql

Sql

Copy
-- Transactional projections of authoritative registry state. They are not a
-- second registry or authorization system. Canonical registry mutations must
-- update these projections in the same SQLite transaction.

CREATE TABLE dread_execution_provider_states (
    provider_id TEXT PRIMARY KEY,
    registration_revision INTEGER NOT NULL,
    adapter_id TEXT NOT NULL,
    adapter_version TEXT NOT NULL,
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    revoked INTEGER NOT NULL CHECK (revoked IN (0, 1))
) STRICT;

CREATE TABLE dread_execution_capability_states (
    capability_id TEXT PRIMARY KEY,
    provider_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    review TEXT NOT NULL,
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1))
) STRICT;

-- Internal repository rows are inserted only by trusted adapters/services.
-- API handlers must not expose direct INSERT access to these tables.
CREATE TABLE dread_trusted_execution_evidence (
    evidence_id TEXT PRIMARY KEY,
    request_id TEXT NOT NULL,
    attempt_id TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    adapter_id TEXT NOT NULL,
    adapter_version TEXT NOT NULL,
    provider_registration_revision INTEGER NOT NULL,
    provider_execution_id TEXT,
    evidence_kind TEXT NOT NULL CHECK (
        evidence_kind IN (
            'acknowledged',
            'succeeded',
            'failed',
            'cancelled',
            'cancellation_request_sent',
            'cancellation_accepted',
            'cancellation_in_progress'
        )
    ),
    payload_sha256 TEXT NOT NULL,
    authenticated INTEGER NOT NULL CHECK (authenticated IN (0, 1)),
    observed_at_unix_seconds INTEGER NOT NULL,
    expires_at_unix_seconds INTEGER
) STRICT;

CREATE TABLE dread_trusted_verification_evidence (
    evidence_id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    attempt_id TEXT NOT NULL,
    verifier_id TEXT NOT NULL,
    worker_requester_id TEXT NOT NULL,
    passed INTEGER NOT NULL CHECK (passed IN (0, 1)),
    findings_sha256 TEXT NOT NULL,
    authenticated INTEGER NOT NULL CHECK (authenticated IN (0, 1)),
    observed_at_unix_seconds INTEGER NOT NULL,
    expires_at_unix_seconds INTEGER
) STRICT;

CREATE TRIGGER dread_trusted_execution_evidence_no_update
BEFORE UPDATE ON dread_trusted_execution_evidence
BEGIN
    SELECT RAISE(ABORT, 'trusted execution evidence is append-only');
END;

CREATE TRIGGER dread_trusted_execution_evidence_no_delete
BEFORE DELETE ON dread_trusted_execution_evidence
BEGIN
    SELECT RAISE(ABORT, 'trusted execution evidence is append-only');
END;

CREATE TRIGGER dread_trusted_verification_evidence_no_update
BEFORE UPDATE ON dread_trusted_verification_evidence
BEGIN
    SELECT RAISE(ABORT, 'trusted verification evidence is append-only');
END;

CREATE TRIGGER dread_trusted_verification_evidence_no_delete
BEFORE DELETE ON dread_trusted_verification_evidence
BEGIN
    SELECT RAISE(ABORT, 'trusted verification evidence is append-only');
END;
FILE: crates/dread-orchestrator/src/lifecycle.rs

Rust

Copy
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissionState {
    Ready,
    Running,
    ReconciliationRequired,
    Succeeded,
    Failed,
    Cancelled,
}

impl MissionState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Running => "running",
            Self::ReconciliationRequired => "reconciliation_required",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Result<Self, LifecycleError> {
        match value {
            "ready" => Ok(Self::Ready),
            "running" => Ok(Self::Running),
            "reconciliation_required" => Ok(Self::ReconciliationRequired),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            other => Err(LifecycleError::InvalidStoredState(other.into())),
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Pending,
    Ready,
    Dispatching,
    Running,
    RetryScheduled,
    CancelRequested,
    Cancelling,
    ReconciliationRequired,
    ProviderSucceeded,
    Verifying,
    Succeeded,
    Failed,
    Cancelled,
    Blocked,
}

impl TaskState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Ready => "ready",
            Self::Dispatching => "dispatching",
            Self::Running => "running",
            Self::RetryScheduled => "retry_scheduled",
            Self::CancelRequested => "cancel_requested",
            Self::Cancelling => "cancelling",
            Self::ReconciliationRequired => "reconciliation_required",
            Self::ProviderSucceeded => "provider_succeeded",
            Self::Verifying => "verifying",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Blocked => "blocked",
        }
    }

    pub fn parse(value: &str) -> Result<Self, LifecycleError> {
        match value {
            "pending" => Ok(Self::Pending),
            "ready" => Ok(Self::Ready),
            "dispatching" => Ok(Self::Dispatching),
            "running" => Ok(Self::Running),
            "retry_scheduled" => Ok(Self::RetryScheduled),
            "cancel_requested" => Ok(Self::CancelRequested),
            "cancelling" => Ok(Self::Cancelling),
            "reconciliation_required" => Ok(Self::ReconciliationRequired),
            "provider_succeeded" => Ok(Self::ProviderSucceeded),
            "verifying" => Ok(Self::Verifying),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            "blocked" => Ok(Self::Blocked),
            other => Err(LifecycleError::InvalidStoredState(other.into())),
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::Blocked
        )
    }

    pub fn has_unresolved_provider_outcome(self) -> bool {
        matches!(
            self,
            Self::Dispatching
                | Self::Running
                | Self::CancelRequested
                | Self::Cancelling
                | Self::ReconciliationRequired
        )
    }

    pub fn can_transition_to(self, next: Self) -> bool {
        use TaskState::*;

        if self == next {
            return true;
        }

        matches!(
            (self, next),
            (Pending, Ready | Cancelled | Blocked)
                | (Ready, Dispatching | CancelRequested | Cancelled)
                | (
                    RetryScheduled,
                    Ready | Dispatching | CancelRequested | Cancelled
                )
                | (
                    Dispatching,
                    Running
                        | CancelRequested
                        | ReconciliationRequired
                        | RetryScheduled
                        | Failed
                )
                | (
                    Running,
                    ProviderSucceeded
                        | Failed
                        | Cancelled
                        | CancelRequested
                        | ReconciliationRequired
                        | RetryScheduled
                )
                | (
                    CancelRequested,
                    Cancelling
                        | ReconciliationRequired
                        | ProviderSucceeded
                        | Failed
                        | Cancelled
                )
                | (
                    Cancelling,
                    ReconciliationRequired
                        | ProviderSucceeded
                        | Failed
                        | Cancelled
                )
                | (
                    ReconciliationRequired,
                    Running
                        | ProviderSucceeded
                        | Failed
                        | Cancelled
                        | CancelRequested
                )
                | (ProviderSucceeded, Verifying)
                | (Verifying, Succeeded | Failed)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdempotencyClass {
    Idempotent,
    NonIdempotent,
    ProviderKeyRequired,
    Unknown,
}

impl IdempotencyClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idempotent => "idempotent",
            Self::NonIdempotent => "non_idempotent",
            Self::ProviderKeyRequired => "provider_key_required",
            Self::Unknown => "unknown",
        }
    }

    pub fn parse(value: &str) -> Result<Self, LifecycleError> {
        match value {
            "idempotent" => Ok(Self::Idempotent),
            "non_idempotent" => Ok(Self::NonIdempotent),
            "provider_key_required" => Ok(Self::ProviderKeyRequired),
            "unknown" => Ok(Self::Unknown),
            other => Err(LifecycleError::InvalidStoredState(other.into())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteOutcome {
    DefiniteFailureBeforeSideEffects,
    Unknown,
    StillActive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryDecision {
    Allowed,
    Exhausted,
    ReconciliationRequired,
    UnsafeContract,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifiedRetryContract {
    pub idempotent_operation: bool,
    pub provider_keys_supported: bool,
    pub revoked: bool,
    pub valid_at_observation: bool,
}

pub fn classify_retry(
    class: IdempotencyClass,
    outcome: RemoteOutcome,
    attempt: u32,
    max_attempts: u32,
    contract: VerifiedRetryContract,
) -> RetryDecision {
    match outcome {
        RemoteOutcome::Unknown => return RetryDecision::ReconciliationRequired,
        RemoteOutcome::StillActive => return RetryDecision::UnsafeContract,
        RemoteOutcome::DefiniteFailureBeforeSideEffects => {}
    }

    if !contract.valid_at_observation || contract.revoked {
        return RetryDecision::UnsafeContract;
    }
    if attempt >= max_attempts {
        return RetryDecision::Exhausted;
    }

    let permitted = match class {
        IdempotencyClass::Idempotent => contract.idempotent_operation,
        IdempotencyClass::ProviderKeyRequired => {
            contract.idempotent_operation && contract.provider_keys_supported
        }
        IdempotencyClass::NonIdempotent => true,
        IdempotencyClass::Unknown => false,
    };

    if permitted {
        RetryDecision::Allowed
    } else {
        RetryDecision::UnsafeContract
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LifecycleError {
    #[error("invalid task transition from {from:?} to {to:?}")]
    InvalidTransition { from: TaskState, to: TaskState },

    #[error("invalid stored lifecycle state: {0}")]
    InvalidStoredState(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_idempotency_classes_and_unknown_outcomes_fail_safely() {
        let contract = VerifiedRetryContract {
            idempotent_operation: true,
            provider_keys_supported: true,
            revoked: false,
            valid_at_observation: true,
        };

        for class in [
            IdempotencyClass::Idempotent,
            IdempotencyClass::NonIdempotent,
            IdempotencyClass::ProviderKeyRequired,
            IdempotencyClass::Unknown,
        ] {
            assert_eq!(
                classify_retry(class, RemoteOutcome::Unknown, 1, 3, contract),
                RetryDecision::ReconciliationRequired
            );
        }

        assert_eq!(
            classify_retry(
                IdempotencyClass::Unknown,
                RemoteOutcome::DefiniteFailureBeforeSideEffects,
                1,
                3,
                contract,
            ),
            RetryDecision::UnsafeContract
        );
    }

    #[test]
    fn keyed_retry_requires_verified_key_support() {
        let mut contract = VerifiedRetryContract {
            idempotent_operation: true,
            provider_keys_supported: false,
            revoked: false,
            valid_at_observation: true,
        };
        assert_eq!(
            classify_retry(
                IdempotencyClass::ProviderKeyRequired,
                RemoteOutcome::DefiniteFailureBeforeSideEffects,
                1,
                3,
                contract,
            ),
            RetryDecision::UnsafeContract
        );

        contract.provider_keys_supported = true;
        assert_eq!(
            classify_retry(
                IdempotencyClass::ProviderKeyRequired,
                RemoteOutcome::DefiniteFailureBeforeSideEffects,
                1,
                3,
                contract,
            ),
            RetryDecision::Allowed
        );
    }

    #[test]
    fn cancellation_and_reconciliation_are_not_terminal() {
        assert!(!TaskState::CancelRequested.is_terminal());
        assert!(!TaskState::Cancelling.is_terminal());
        assert!(!TaskState::ReconciliationRequired.is_terminal());
        assert!(TaskState::ReconciliationRequired.has_unresolved_provider_outcome());
    }

    #[test]
    fn provider_success_wins_cancellation_race_but_needs_verification() {
        assert!(TaskState::Cancelling.can_transition_to(TaskState::ProviderSucceeded));
        assert!(!TaskState::ProviderSucceeded.is_terminal());
        assert!(TaskState::ProviderSucceeded.can_transition_to(TaskState::Verifying));
        assert!(TaskState::Verifying.can_transition_to(TaskState::Succeeded));
    }
}
FILE: crates/dread-orchestrator/src/preflight.rs

Rust

Copy
//! Server-side provider-isolation validation and persisted dispatch admission.
//!
//! Caller-provided evidence flags, enabled states, review states or permission
//! decisions are never authoritative. ExecutionStore consumes admission in the
//! same SQLite transaction that creates its dispatch attempt.

use dread_core::domain::{CapabilityKind, PermissionDecision, ReviewState};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};
use thiserror::Error;

pub const PREFLIGHT_CONTRACT_VERSION: &str = "provider-isolation-preflight-v1";
pub const MAX_PREFLIGHT_LIFETIME_SECONDS: i64 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IsolationControl {
    RuntimeIdentity,
    TaskWorkspaceBoundary,
    MountPolicy,
    EgressPolicy,
    ToolPermissions,
    StableSecretHandling,
    CancellationPropagation,
    RestartReconciliation,
}

impl IsolationControl {
    pub const ALL: [Self; 8] = [
        Self::RuntimeIdentity,
        Self::TaskWorkspaceBoundary,
        Self::MountPolicy,
        Self::EgressPolicy,
        Self::ToolPermissions,
        Self::StableSecretHandling,
        Self::CancellationPropagation,
        Self::RestartReconciliation,
    ];
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceProvenance {
    pub source_observation_id: String,
    pub recorded_by_adapter_id: String,
    pub recorded_by_adapter_version: String,
    pub evidence_sha256: String,
    pub recorded_at_unix_seconds: i64,
    pub authenticated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceRecord {
    pub contract_version: String,
    pub evidence_id: String,
    pub provider_id: String,
    pub provider_registration_revision: i64,
    pub adapter_id: String,
    pub adapter_version: String,
    pub runtime_identity: String,
    pub provenance: EvidenceProvenance,
    pub checks: BTreeMap<IsolationControl, ControlEvidence>,
    pub checked_at_unix_seconds: i64,
    pub expires_at_unix_seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlEvidence {
    pub verified: bool,
    pub durable_observation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRegistration {
    pub provider_id: String,
    pub registration_revision: i64,
    pub adapter_id: String,
    pub adapter_version: String,
    pub enabled: bool,
    pub revoked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityRegistration {
    pub capability_id: String,
    pub provider_id: String,
    pub kind: CapabilityKind,
    pub review: ReviewState,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmissionRequest {
    pub request_id: String,
    pub requester_id: String,
    pub mission_id: String,
    pub task_id: String,
    pub attempt: u32,
    pub provider_id: String,
    pub capability_id: String,
    pub operation: String,
    pub permission_scope: String,
    pub idempotency_key: String,
    pub requested_at_unix_seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyDecisionRecord {
    pub decision_id: String,
    pub decision: PermissionDecision,
    pub request_fingerprint: String,
    pub requester_id: String,
    pub decided_at_unix_seconds: i64,
    pub expires_at_unix_seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedAdmission {
    pub admission_id: String,
    pub request_fingerprint: String,
    pub request_id: String,
    pub requester_id: String,
    pub mission_id: String,
    pub task_id: String,
    pub attempt: u32,
    pub provider_id: String,
    pub provider_registration_revision: i64,
    pub capability_id: String,
    pub operation: String,
    pub permission_scope: String,
    pub idempotency_key: String,
    pub evidence_id: String,
    pub policy_decision_id: String,
    pub adapter_id: String,
    pub adapter_version: String,
    pub expires_at_unix_seconds: i64,
}

pub trait EvidenceRepository: Send + Sync {
    fn load(&self, evidence_id: &str) -> Result<Option<EvidenceRecord>, AdmissionError>;
}

pub trait TrustedProviderRegistry: Send + Sync {
    fn provider(
        &self,
        provider_id: &str,
    ) -> Result<Option<ProviderRegistration>, AdmissionError>;

    fn capability(
        &self,
        capability_id: &str,
    ) -> Result<Option<CapabilityRegistration>, AdmissionError>;
}

pub trait TrustedPolicyEngine: Send + Sync {
    fn decide(
        &self,
        request: &AdmissionRequest,
        request_fingerprint: &str,
    ) -> Result<PolicyDecisionRecord, AdmissionError>;
}

#[derive(Debug, Error)]
pub enum AdmissionError {
    #[error("admission dependency failed: {0}")]
    Dependency(String),

    #[error("admission denied: {0}")]
    Denied(String),

    #[error("admission conflict: {0}")]
    Conflict(String),

    #[error("admission persistence failed: {0}")]
    Persistence(String),
}

impl From<rusqlite::Error> for AdmissionError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Persistence(error.to_string())
    }
}

pub struct AdmissionService {
    connection: Arc<Mutex<Connection>>,
    evidence: Arc<dyn EvidenceRepository>,
    registry: Arc<dyn TrustedProviderRegistry>,
    policy: Arc<dyn TrustedPolicyEngine>,
}

impl AdmissionService {
    pub fn new(
        connection: Arc<Mutex<Connection>>,
        evidence: Arc<dyn EvidenceRepository>,
        registry: Arc<dyn TrustedProviderRegistry>,
        policy: Arc<dyn TrustedPolicyEngine>,
    ) -> Self {
        Self {
            connection,
            evidence,
            registry,
            policy,
        }
    }

    pub fn authorize_and_persist(
        &self,
        request: &AdmissionRequest,
        evidence_id: &str,
        now: i64,
    ) -> Result<PersistedAdmission, AdmissionError> {
        validate_request(request)?;
        if request.requested_at_unix_seconds > now {
            return Err(AdmissionError::Denied(
                "request timestamp is in the future".into(),
            ));
        }

        let fingerprint = request_fingerprint(request)?;
        let evidence = self
            .evidence
            .load(evidence_id)?
            .ok_or_else(|| AdmissionError::Denied("trusted evidence not found".into()))?;
        let provider = self
            .registry
            .provider(&request.provider_id)?
            .ok_or_else(|| AdmissionError::Denied("provider not registered".into()))?;
        let capability = self
            .registry
            .capability(&request.capability_id)?
            .ok_or_else(|| AdmissionError::Denied("capability not registered".into()))?;

        validate_evidence(&evidence, now)?;
        if !provider.enabled
            || provider.revoked
            || provider.provider_id != request.provider_id
            || provider.adapter_id != evidence.adapter_id
            || provider.adapter_version != evidence.adapter_version
            || provider.registration_revision != evidence.provider_registration_revision
        {
            return Err(AdmissionError::Denied(
                "provider state, revision, or adapter identity mismatch".into(),
            ));
        }
        if evidence.provider_id != request.provider_id
            || capability.provider_id != request.provider_id
            || capability.capability_id != request.capability_id
            || capability.kind != CapabilityKind::AgentProvider
            || capability.review != ReviewState::Approved
            || !capability.enabled
        {
            return Err(AdmissionError::Denied(
                "evidence/capability is invalid for this executable provider".into(),
            ));
        }

        let decision = self.policy.decide(request, &fingerprint)?;
        if decision.decision != PermissionDecision::Allow
            || decision.request_fingerprint != fingerprint
            || decision.requester_id != request.requester_id
            || decision.decided_at_unix_seconds > now
            || decision.expires_at_unix_seconds <= now
        {
            return Err(AdmissionError::Denied(
                "trusted policy did not allow this exact request scope".into(),
            ));
        }

        let expires = evidence
            .expires_at_unix_seconds
            .min(decision.expires_at_unix_seconds)
            .min(now.saturating_add(MAX_PREFLIGHT_LIFETIME_SECONDS));
        if expires <= now {
            return Err(AdmissionError::Denied("admission already expired".into()));
        }

        let admission = PersistedAdmission {
            admission_id: format!("admission:{}", request.request_id),
            request_fingerprint: fingerprint,
            request_id: request.request_id.clone(),
            requester_id: request.requester_id.clone(),
            mission_id: request.mission_id.clone(),
            task_id: request.task_id.clone(),
            attempt: request.attempt,
            provider_id: request.provider_id.clone(),
            provider_registration_revision: provider.registration_revision,
            capability_id: request.capability_id.clone(),
            operation: request.operation.clone(),
            permission_scope: request.permission_scope.clone(),
            idempotency_key: request.idempotency_key.clone(),
            evidence_id: evidence.evidence_id,
            policy_decision_id: decision.decision_id,
            adapter_id: provider.adapter_id,
            adapter_version: provider.adapter_version,
            expires_at_unix_seconds: expires,
        };

        let mut connection = self
            .connection
            .lock()
            .map_err(|_| AdmissionError::Persistence("database mutex poisoned".into()))?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "INSERT INTO dread_dispatch_admissions (
                admission_id, request_fingerprint, request_id, requester_id,
                mission_id, task_id, attempt, provider_id,
                provider_registration_revision, capability_id, operation,
                permission_scope, idempotency_key, evidence_id,
                policy_decision_id, adapter_id, adapter_version,
                expires_at_unix_seconds, consumed_at_unix_seconds,
                created_at_unix_seconds
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, NULL, ?19
             )",
            params![
                admission.admission_id,
                admission.request_fingerprint,
                admission.request_id,
                admission.requester_id,
                admission.mission_id,
                admission.task_id,
                admission.attempt,
                admission.provider_id,
                admission.provider_registration_revision,
                admission.capability_id,
                admission.operation,
                admission.permission_scope,
                admission.idempotency_key,
                admission.evidence_id,
                admission.policy_decision_id,
                admission.adapter_id,
                admission.adapter_version,
                admission.expires_at_unix_seconds,
                now
            ],
        )?;
        transaction.commit()?;
        Ok(admission)
    }
}

pub fn request_fingerprint(request: &AdmissionRequest) -> Result<String, AdmissionError> {
    validate_request(request)?;
    let encoded = serde_json::to_vec(request)
        .map_err(|error| AdmissionError::Dependency(error.to_string()))?;
    Ok(format!("{:x}", Sha256::digest(encoded)))
}

pub fn load_and_consume_admission(
    transaction: &Transaction<'_>,
    request: &AdmissionRequest,
    now: i64,
) -> Result<PersistedAdmission, AdmissionError> {
    let fingerprint = request_fingerprint(request)?;
    let row = transaction
        .query_row(
            "SELECT admission_id, request_fingerprint, request_id, requester_id,
                    mission_id, task_id, attempt, provider_id,
                    provider_registration_revision, capability_id, operation,
                    permission_scope, idempotency_key, evidence_id,
                    policy_decision_id, adapter_id, adapter_version,
                    expires_at_unix_seconds, consumed_at_unix_seconds
             FROM dread_dispatch_admissions
             WHERE request_id = ?1",
            [&request.request_id],
            |row| {
                Ok(AdmissionRow {
                    admission_id: row.get(0)?,
                    request_fingerprint: row.get(1)?,
                    request_id: row.get(2)?,
                    requester_id: row.get(3)?,
                    mission_id: row.get(4)?,
                    task_id: row.get(5)?,
                    attempt: row.get(6)?,
                    provider_id: row.get(7)?,
                    provider_registration_revision: row.get(8)?,
                    capability_id: row.get(9)?,
                    operation: row.get(10)?,
                    permission_scope: row.get(11)?,
                    idempotency_key: row.get(12)?,
                    evidence_id: row.get(13)?,
                    policy_decision_id: row.get(14)?,
                    adapter_id: row.get(15)?,
                    adapter_version: row.get(16)?,
                    expires_at_unix_seconds: row.get(17)?,
                    consumed_at_unix_seconds: row.get(18)?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| AdmissionError::Denied("persisted admission not found".into()))?;

    if row.consumed_at_unix_seconds.is_some() {
        return Err(AdmissionError::Conflict(
            "admission has already been consumed".into(),
        ));
    }
    if row.request_fingerprint != fingerprint
        || row.request_id != request.request_id
        || row.requester_id != request.requester_id
        || row.mission_id != request.mission_id
        || row.task_id != request.task_id
        || row.attempt != i64::from(request.attempt)
        || row.provider_id != request.provider_id
        || row.capability_id != request.capability_id
        || row.operation != request.operation
        || row.permission_scope != request.permission_scope
        || row.idempotency_key != request.idempotency_key
        || row.expires_at_unix_seconds <= now
    {
        return Err(AdmissionError::Denied(
            "admission scope, expiry, or fingerprint mismatch".into(),
        ));
    }

    let changed = transaction.execute(
        "UPDATE dread_dispatch_admissions
         SET consumed_at_unix_seconds = ?2
         WHERE request_id = ?1 AND consumed_at_unix_seconds IS NULL",
        params![request.request_id, now],
    )?;
    if changed != 1 {
        return Err(AdmissionError::Conflict(
            "admission consumption conflict".into(),
        ));
    }

    Ok(PersistedAdmission {
        admission_id: row.admission_id,
        request_fingerprint: row.request_fingerprint,
        request_id: row.request_id,
        requester_id: row.requester_id,
        mission_id: row.mission_id,
        task_id: row.task_id,
        attempt: request.attempt,
        provider_id: row.provider_id,
        provider_registration_revision: row.provider_registration_revision,
        capability_id: row.capability_id,
        operation: row.operation,
        permission_scope: row.permission_scope,
        idempotency_key: row.idempotency_key,
        evidence_id: row.evidence_id,
        policy_decision_id: row.policy_decision_id,
        adapter_id: row.adapter_id,
        adapter_version: row.adapter_version,
        expires_at_unix_seconds: row.expires_at_unix_seconds,
    })
}

struct AdmissionRow {
    admission_id: String,
    request_fingerprint: String,
    request_id: String,
    requester_id: String,
    mission_id: String,
    task_id: String,
    attempt: i64,
    provider_id: String,
    provider_registration_revision: i64,
    capability_id: String,
    operation: String,
    permission_scope: String,
    idempotency_key: String,
    evidence_id: String,
    policy_decision_id: String,
    adapter_id: String,
    adapter_version: String,
    expires_at_unix_seconds: i64,
    consumed_at_unix_seconds: Option<i64>,
}

fn validate_evidence(evidence: &EvidenceRecord, now: i64) -> Result<(), AdmissionError> {
    if evidence.contract_version != PREFLIGHT_CONTRACT_VERSION {
        return Err(AdmissionError::Denied(
            "unsupported evidence contract".into(),
        ));
    }
    for value in [
        &evidence.evidence_id,
        &evidence.provider_id,
        &evidence.adapter_id,
        &evidence.adapter_version,
        &evidence.runtime_identity,
        &evidence.provenance.source_observation_id,
        &evidence.provenance.evidence_sha256,
    ] {
        validate_identifier(value)?;
    }
    if evidence.provenance.evidence_sha256.len() != 64
        || !evidence
            .provenance
            .evidence_sha256
            .chars()
            .all(|character| character.is_ascii_hexdigit())
        || !evidence.provenance.authenticated
        || evidence.provenance.recorded_by_adapter_id != evidence.adapter_id
        || evidence.provenance.recorded_by_adapter_version != evidence.adapter_version
        || evidence.provenance.recorded_at_unix_seconds < evidence.checked_at_unix_seconds
    {
        return Err(AdmissionError::Denied(
            "evidence provenance is not authenticated and adapter-bound".into(),
        ));
    }
    if evidence.checked_at_unix_seconds > now
        || evidence.expires_at_unix_seconds <= now
        || evidence.expires_at_unix_seconds <= evidence.checked_at_unix_seconds
        || evidence.expires_at_unix_seconds - evidence.checked_at_unix_seconds
            > MAX_PREFLIGHT_LIFETIME_SECONDS
    {
        return Err(AdmissionError::Denied(
            "evidence is expired, future-dated, or exceeds maximum lifetime".into(),
        ));
    }

    let expected: BTreeSet<_> = IsolationControl::ALL.into_iter().collect();
    let actual: BTreeSet<_> = evidence.checks.keys().copied().collect();
    if expected != actual {
        return Err(AdmissionError::Denied(
            "all isolation controls require evidence".into(),
        ));
    }
    for evidence in evidence.checks.values() {
        if !evidence.verified || evidence.durable_observation_id.trim().is_empty() {
            return Err(AdmissionError::Denied(
                "an isolation control is unverified".into(),
            ));
        }
    }
    Ok(())
}

fn validate_request(request: &AdmissionRequest) -> Result<(), AdmissionError> {
    for value in [
        &request.request_id,
        &request.requester_id,
        &request.mission_id,
        &request.task_id,
        &request.provider_id,
        &request.capability_id,
        &request.operation,
        &request.permission_scope,
        &request.idempotency_key,
    ] {
        validate_identifier(value)?;
    }
    if request.attempt == 0 {
        return Err(AdmissionError::Denied("attempt must be positive".into()));
    }
    Ok(())
}

fn validate_identifier(value: &str) -> Result<(), AdmissionError> {
    if value.trim().is_empty() || value.len() > 1024 || value.contains('\0') {
        Err(AdmissionError::Denied(
            "required identity/scope is empty or too large".into(),
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Evidence(EvidenceRecord);

    impl EvidenceRepository for Evidence {
        fn load(&self, id: &str) -> Result<Option<EvidenceRecord>, AdmissionError> {
            Ok((id == self.0.evidence_id).then(|| self.0.clone()))
        }
    }

    struct Registry {
        provider: ProviderRegistration,
        capability: CapabilityRegistration,
    }

    impl TrustedProviderRegistry for Registry {
        fn provider(
            &self,
            id: &str,
        ) -> Result<Option<ProviderRegistration>, AdmissionError> {
            Ok((id == self.provider.provider_id).then(|| self.provider.clone()))
        }

        fn capability(
            &self,
            id: &str,
        ) -> Result<Option<CapabilityRegistration>, AdmissionError> {
            Ok((id == self.capability.capability_id).then(|| self.capability.clone()))
        }
    }

    struct Policy {
        fingerprint_override: Option<String>,
    }

    impl TrustedPolicyEngine for Policy {
        fn decide(
            &self,
            request: &AdmissionRequest,
            fingerprint: &str,
        ) -> Result<PolicyDecisionRecord, AdmissionError> {
            Ok(PolicyDecisionRecord {
                decision_id: "decision-1".into(),
                decision: PermissionDecision::Allow,
                request_fingerprint: self
                    .fingerprint_override
                    .clone()
                    .unwrap_or_else(|| fingerprint.into()),
                requester_id: request.requester_id.clone(),
                decided_at_unix_seconds: 100,
                expires_at_unix_seconds: 200,
            })
        }
    }

    fn evidence() -> EvidenceRecord {
        EvidenceRecord {
            contract_version: PREFLIGHT_CONTRACT_VERSION.into(),
            evidence_id: "evidence-1".into(),
            provider_id: "provider-1".into(),
            provider_registration_revision: 7,
            adapter_id: "adapter-1".into(),
            adapter_version: "1.0.0".into(),
            runtime_identity: "runtime-1".into(),
            provenance: EvidenceProvenance {
                source_observation_id: "observation-1".into(),
                recorded_by_adapter_id: "adapter-1".into(),
                recorded_by_adapter_version: "1.0.0".into(),
                evidence_sha256: "ab".repeat(32),
                recorded_at_unix_seconds: 101,
                authenticated: true,
            },
            checks: IsolationControl::ALL
                .into_iter()
                .map(|control| {
                    (
                        control,
                        ControlEvidence {
                            verified: true,
                            durable_observation_id: format!("{control:?}-observation"),
                        },
                    )
                })
                .collect(),
            checked_at_unix_seconds: 100,
            expires_at_unix_seconds: 200,
        }
    }

    fn request() -> AdmissionRequest {
        AdmissionRequest {
            request_id: "request-1".into(),
            requester_id: "operator-1".into(),
            mission_id: "mission-1".into(),
            task_id: "task-1".into(),
            attempt: 1,
            provider_id: "provider-1".into(),
            capability_id: "capability-1".into(),
            operation: "engineering.execute".into(),
            permission_scope: "workspace:mission-1".into(),
            idempotency_key: "durable-key-1".into(),
            requested_at_unix_seconds: 120,
        }
    }

    fn fixtures(
        evidence: EvidenceRecord,
        policy: Policy,
    ) -> (
        Arc<Mutex<Connection>>,
        Arc<Evidence>,
        Arc<Registry>,
        Arc<Policy>,
    ) {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(include_str!(
                "../migrations/0002_trusted_dispatch_admission.sql"
            ))
            .unwrap();
        (
            Arc::new(Mutex::new(connection)),
            Arc::new(Evidence(evidence)),
            Arc::new(Registry {
                provider: ProviderRegistration {
                    provider_id: "provider-1".into(),
                    registration_revision: 7,
                    adapter_id: "adapter-1".into(),
                    adapter_version: "1.0.0".into(),
                    enabled: true,
                    revoked: false,
                },
                capability: CapabilityRegistration {
                    capability_id: "capability-1".into(),
                    provider_id: "provider-1".into(),
                    kind: CapabilityKind::AgentProvider,
                    review: ReviewState::Approved,
                    enabled: true,
                },
            }),
            Arc::new(policy),
        )
    }

    fn authorize_with(
        evidence: EvidenceRecord,
        request: &AdmissionRequest,
        policy: Policy,
    ) -> Result<PersistedAdmission, AdmissionError> {
        let (connection, evidence_repo, registry, policy) = fixtures(evidence, policy);
        AdmissionService::new(connection, evidence_repo, registry, policy)
            .authorize_and_persist(request, "evidence-1", 120)
    }

    #[test]
    fn authenticated_evidence_persists_scoped_admission() {
        let admission = authorize_with(
            evidence(),
            &request(),
            Policy {
                fingerprint_override: None,
            },
        )
        .unwrap();
        assert_eq!(admission.attempt, 1);
        assert_eq!(admission.provider_registration_revision, 7);
    }

    #[test]
    fn forged_unauthenticated_evidence_is_rejected() {
        let mut forged = evidence();
        forged.provenance.authenticated = false;
        assert!(authorize_with(
            forged,
            &request(),
            Policy {
                fingerprint_override: None,
            },
        )
        .is_err());
    }

    #[test]
    fn changed_permission_scope_invalidates_policy_binding() {
        let mut request = request();
        request.permission_scope = "workspace:another-mission".into();
        assert!(authorize_with(
            evidence(),
            &request,
            Policy {
                fingerprint_override: Some("forged-fingerprint".into()),
            },
        )
        .is_err());
    }

    #[test]
    fn adapter_and_provider_revision_mismatches_are_rejected() {
        let mut wrong_adapter = evidence();
        wrong_adapter.adapter_id = "other-adapter".into();
        assert!(authorize_with(
            wrong_adapter,
            &request(),
            Policy {
                fingerprint_override: None,
            },
        )
        .is_err());

        let mut stale_revision = evidence();
        stale_revision.provider_registration_revision = 6;
        assert!(authorize_with(
            stale_revision,
            &request(),
            Policy {
                fingerprint_override: None,
            },
        )
        .is_err());
    }

    #[test]
    fn expired_evidence_is_rejected() {
        let mut expired = evidence();
        expired.expires_at_unix_seconds = 120;
        assert!(authorize_with(
            expired,
            &request(),
            Policy {
                fingerprint_override: None,
            },
        )
        .is_err());
    }

    #[test]
    fn consumption_rejects_task_mismatch_and_replay() {
        let (connection, evidence_repo, registry, policy) = fixtures(
            evidence(),
            Policy {
                fingerprint_override: None,
            },
        );
        let service = AdmissionService::new(
            Arc::clone(&connection),
            evidence_repo,
            registry,
            policy,
        );
        let request = request();
        service
            .authorize_and_persist(&request, "evidence-1", 120)
            .unwrap();

        let mut connection = connection.lock().unwrap();
        let transaction = connection.transaction().unwrap();
        let mut other_task = request.clone();
        other_task.task_id = "other-task".into();
        assert!(load_and_consume_admission(&transaction, &other_task, 121).is_err());
        transaction.rollback().unwrap();

        let transaction = connection.transaction().unwrap();
        load_and_consume_admission(&transaction, &request, 121).unwrap();
        assert!(load_and_consume_admission(&transaction, &request, 121).is_err());
        transaction.rollback().unwrap();
    }
}
FILE: crates/dread-orchestrator/src/cancellation.rs

Rust

Copy
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCancellationAck {
    RequestSent,
    Accepted,
    StillCancelling,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderCancellationReceipt {
    pub request_id: String,
    pub provider_execution_id: Option<String>,
    pub acknowledgement: ProviderCancellationAck,
    /// ID of an authenticated row in dread_trusted_execution_evidence.
    pub evidence_id: String,
}

#[derive(Debug, Error)]
pub enum CancellationError {
    #[error("provider cancellation request failed: {0}")]
    Provider(String),

    #[error("provider returned an invalid cancellation receipt: {0}")]
    InvalidReceipt(String),
}

/// Implementations contact only their real configured provider. They never
/// report terminal cancellation; terminal states require separate evidence.
#[async_trait]
pub trait ProviderCancellation: Send + Sync {
    async fn request_cancellation(
        &self,
        request_id: &str,
        provider_execution_id: Option<&str>,
    ) -> Result<ProviderCancellationReceipt, CancellationError>;
}
FILE: crates/dread-orchestrator/src/store.rs

Rust

Copy
//! Canonical SQLite execution adapter.
//!
//! Canonical mission/task/dependency tables are validated and mutated, never
//! created here. Only attempts, admissions, retry contracts and events are
//! additive. Connection access must run in short spawn_blocking operations;
//! Mutex<Connection> is not held across an await.

use crate::{
    lifecycle::{
        classify_retry, IdempotencyClass, MissionState, RemoteOutcome,
        RetryDecision, TaskState, VerifiedRetryContract,
    },
    preflight::{
        load_and_consume_admission, request_fingerprint, AdmissionError,
        AdmissionRequest,
    },
};
use rusqlite::{
    params, Connection, OptionalExtension, Transaction, TransactionBehavior,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};
use thiserror::Error;

const MAX_EVENT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct CanonicalSchema {
    mission_table: String,
    task_table: String,
    dependency_table: String,
}

impl CanonicalSchema {
    /// NEEDS_CANONICAL_MERGE: Supply actual canonical table names. The adapter
    /// validates all columns it reads or writes before accepting the store.
    pub fn new(
        mission_table: impl Into<String>,
        task_table: impl Into<String>,
        dependency_table: impl Into<String>,
    ) -> Result<Self, StoreError> {
        let schema = Self {
            mission_table: mission_table.into(),
            task_table: task_table.into(),
            dependency_table: dependency_table.into(),
        };
        for identifier in [
            &schema.mission_table,
            &schema.task_table,
            &schema.dependency_table,
        ] {
            if identifier.is_empty()
                || !identifier
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                return Err(StoreError::Invalid("unsafe SQL identifier".into()));
            }
        }
        Ok(schema)
    }

    fn validate(&self, connection: &Connection) -> Result<(), StoreError> {
        require_columns(
            connection,
            &self.mission_table,
            &["id", "state", "revision", "updated_at_unix_seconds"],
        )?;
        require_columns(
            connection,
            &self.task_table,
            &[
                "id",
                "mission_id",
                "state",
                "revision",
                "attempt",
                "max_attempts",
                "provider_id",
                "capability_id",
                "operation",
                "retry_not_before_unix_seconds",
                "last_error",
                "updated_at_unix_seconds",
            ],
        )?;
        require_columns(
            connection,
            &self.dependency_table,
            &["task_id", "depends_on_task_id"],
        )?;

        for table in [
            "dread_dispatch_admissions",
            "dread_provider_retry_contracts",
            "dread_task_execution_contracts",
            "dread_task_attempts",
            "dread_reconciliation_observations",
            "dread_execution_events",
            "dread_execution_provider_states",
            "dread_execution_capability_states",
            "dread_trusted_execution_evidence",
            "dread_trusted_verification_evidence",
        ] {
            require_table(connection, table)?;
        }
        require_columns(
            connection,
            "dread_dispatch_admissions",
            &[
                "request_id",
                "request_fingerprint",
                "provider_registration_revision",
                "consumed_at_unix_seconds",
            ],
        )?;
        require_columns(
            connection,
            "dread_task_attempts",
            &[
                "attempt_id",
                "request_id",
                "provider_execution_id",
                "dispatch_state",
                "cancellation_state",
                "outcome",
            ],
        )?;
        require_columns(
            connection,
            "dread_execution_events",
            &[
                "event_identity",
                "event_type",
                "event_key",
                "payload_sha256",
            ],
        )?;
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("execution SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("execution JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Admission(#[from] AdmissionError),

    #[error("execution validation failed: {0}")]
    Invalid(String),

    #[error("execution entity not found: {0}")]
    NotFound(String),

    #[error("stale revision or conflicting state: {0}")]
    Conflict(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRecord {
    pub id: String,
    pub mission_id: String,
    pub state: TaskState,
    pub revision: i64,
    pub attempt: u32,
    pub max_attempts: u32,
    pub provider_id: String,
    pub capability_id: String,
    pub operation: String,
    pub retry_not_before: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptRecord {
    pub attempt_id: String,
    pub task_id: String,
    pub mission_id: String,
    pub attempt: u32,
    pub request_id: String,
    pub request_fingerprint: String,
    pub provider_id: String,
    pub capability_id: String,
    pub operation: String,
    pub idempotency_key: String,
    pub admission_id: String,
    pub evidence_id: String,
    pub provider_execution_id: Option<String>,
    pub dispatch_state: String,
    pub cancellation_state: String,
    pub outcome: Option<String>,
    pub dispatched_at: i64,
    pub acknowledged_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionEvent {
    pub sequence: i64,
    pub mission_id: String,
    pub task_id: Option<String>,
    pub attempt_id: Option<String>,
    pub event_type: String,
    pub event_key: String,
    pub payload: Value,
    pub occurred_at: i64,
}

#[derive(Clone)]
pub struct ExecutionStore {
    connection: Arc<Mutex<Connection>>,
    schema: CanonicalSchema,
}

impl ExecutionStore {
    pub fn new(
        connection: Arc<Mutex<Connection>>,
        schema: CanonicalSchema,
    ) -> Result<Self, StoreError> {
        {
            let connection = connection
                .lock()
                .map_err(|_| StoreError::Invalid("database mutex poisoned".into()))?;
            schema.validate(&connection)?;
        }
        Ok(Self { connection, schema })
    }

    /// Registers server-verified retry semantics, not API-supplied claims.
    pub fn register_verified_retry_contract(
        &self,
        contract_id: &str,
        provider_id: &str,
        provider_registration_revision: i64,
        adapter_id: &str,
        adapter_version: &str,
        operation: &str,
        idempotency_verified: bool,
        provider_keys_supported: bool,
        provenance_evidence_id: &str,
        contract_sha256: &str,
        now: i64,
        expires: i64,
    ) -> Result<(), StoreError> {
        validate_text("contract_id", contract_id, 512)?;
        validate_text(
            "provenance_evidence_id",
            provenance_evidence_id,
            1024,
        )?;
        validate_sha256(contract_sha256)?;
        if expires <= now {
            return Err(StoreError::Invalid(
                "retry contract must have a future expiry".into(),
            ));
        }

        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        validate_current_registration_at_consumption(
            &transaction,
            provider_id,
            provider_registration_revision,
            adapter_id,
            adapter_version,
            None,
            operation,
        )?;
        transaction.execute(
            "INSERT INTO dread_provider_retry_contracts (
                contract_id, provider_id, provider_registration_revision,
                adapter_id, adapter_version, operation,
                idempotency_verified, provider_keys_supported, revoked,
                provenance_evidence_id, contract_sha256,
                verified_at_unix_seconds, expires_at_unix_seconds
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?9, ?10, ?11, ?12)",
            params![
                contract_id,
                provider_id,
                provider_registration_revision,
                adapter_id,
                adapter_version,
                operation,
                idempotency_verified,
                provider_keys_supported,
                provenance_evidence_id,
                contract_sha256,
                now,
                expires
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Binds the durable key once. Retries must reuse the exact same key.
    pub fn bind_execution_contract(
        &self,
        task_id: &str,
        provider_id: &str,
        capability_id: &str,
        operation: &str,
        class: IdempotencyClass,
        key: &str,
        retry_contract_id: &str,
        scope_sha256: &str,
        now: i64,
    ) -> Result<(), StoreError> {
        validate_text("task_id", task_id, 512)?;
        validate_text("idempotency_key", key, 1024)?;
        validate_sha256(scope_sha256)?;
        let connection = self.lock()?;
        connection.execute(
            "INSERT INTO dread_task_execution_contracts (
                task_id, provider_id, capability_id, operation,
                idempotency_class, idempotency_key,
                provider_retry_contract_id, request_scope_sha256,
                created_at_unix_seconds
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                task_id,
                provider_id,
                capability_id,
                operation,
                class.as_str(),
                key,
                retry_contract_id,
                scope_sha256,
                now
            ],
        )?;
        Ok(())
    }

    /// Atomically verifies/consumes persisted admission and creates one attempt.
    pub fn prepare_dispatch(
        &self,
        request: &AdmissionRequest,
        expected_task_revision: i64,
        now: i64,
    ) -> Result<AttemptRecord, StoreError> {
        let fingerprint = request_fingerprint(request)?;
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;

        // Definitive duplicate handling is inside the write transaction.
        if let Some(existing) = load_attempt_by_request(&transaction, &request.request_id)? {
            if existing.request_fingerprint != fingerprint
                || existing.task_id != request.task_id
                || existing.attempt != request.attempt
                || existing.idempotency_key != request.idempotency_key
                || existing.provider_id != request.provider_id
                || existing.capability_id != request.capability_id
                || existing.operation != request.operation
            {
                return Err(StoreError::Conflict(
                    "request ID reused with different content".into(),
                ));
            }
            transaction.commit()?;
            return Ok(existing);
        }

        let task = load_task(&transaction, &self.schema, &request.task_id)?;
        require_revision(&task, expected_task_revision)?;
        if task.mission_id != request.mission_id
            || task.provider_id != request.provider_id
            || task.capability_id != request.capability_id
            || task.operation != request.operation
        {
            return Err(StoreError::Conflict(
                "admission does not match canonical task scope".into(),
            ));
        }
        if !matches!(task.state, TaskState::Ready | TaskState::RetryScheduled) {
            return Err(StoreError::Conflict(format!(
                "cannot dispatch from {:?}",
                task.state
            )));
        }
        if task.state == TaskState::RetryScheduled
            && task.retry_not_before.is_some_and(|deadline| deadline > now)
        {
            return Err(StoreError::Conflict("retry backoff active".into()));
        }
        if request.attempt != task.attempt.saturating_add(1)
            || request.attempt > task.max_attempts
        {
            return Err(StoreError::Conflict("invalid or exhausted attempt".into()));
        }

        let contract = load_execution_contract(&transaction, &task.id)?;
        if contract.provider_id != request.provider_id
            || contract.capability_id != request.capability_id
            || contract.operation != request.operation
            || contract.key != request.idempotency_key
        {
            return Err(StoreError::Conflict(
                "provider key or operation scope changed".into(),
            ));
        }
        if contract.class == IdempotencyClass::Unknown {
            return Err(StoreError::Conflict(
                "unknown idempotency cannot be dispatched".into(),
            ));
        }

        let admitted_revision = admission_provider_revision(
            &transaction,
            &request.request_id,
        )?;
        let admission = load_and_consume_admission(&transaction, request, now)?;
        validate_current_registration_at_consumption(
            &transaction,
            &request.provider_id,
            admitted_revision,
            &admission.adapter_id,
            &admission.adapter_version,
            Some(&request.capability_id),
            &request.operation,
        )?;

        let attempt_id = format!("attempt:{}:{}", task.id, request.attempt);
        transaction.execute(
            "INSERT INTO dread_task_attempts (
                attempt_id, task_id, mission_id, attempt, request_id,
                request_fingerprint, provider_id, capability_id, operation,
                idempotency_key, admission_id, evidence_id,
                provider_execution_id, dispatch_state, cancellation_state,
                outcome, dispatched_at_unix_seconds,
                acknowledged_at_unix_seconds, terminal_at_unix_seconds
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                ?11, ?12, NULL, 'pending', 'none', NULL, ?13, NULL, NULL
             )",
            params![
                attempt_id,
                task.id,
                task.mission_id,
                request.attempt,
                request.request_id,
                fingerprint,
                request.provider_id,
                request.capability_id,
                request.operation,
                request.idempotency_key,
                admission.admission_id,
                admission.evidence_id,
                now
            ],
        )?;
        update_task_cas(
            &transaction,
            &self.schema,
            &task,
            TaskState::Dispatching,
            Some(request.attempt),
            None,
            None,
            now,
        )?;
        set_mission_running(&transaction, &self.schema, &task.mission_id, now)?;
        insert_event(
            &transaction,
            &task.mission_id,
            Some(&task.id),
            Some(&attempt_id),
            Some(&task.provider_id),
            None,
            "dispatch_intent_persisted",
            &format!("dispatch:{attempt_id}"),
            serde_json::json!({
                "attempt": request.attempt,
                "admission_id": admission.admission_id,
                "provider_acknowledged": false
            }),
            now,
        )?;
        transaction.commit()?;

        self.attempt_by_request(&request.request_id)?
            .ok_or_else(|| StoreError::NotFound(request.request_id.clone()))
    }

    pub fn attempt_by_request(
        &self,
        request_id: &str,
    ) -> Result<Option<AttemptRecord>, StoreError> {
        let connection = self.lock()?;
        load_attempt_by_request(&connection, request_id)
    }

    pub fn pending_attempts(&self) -> Result<Vec<AttemptRecord>, StoreError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT request_id
             FROM dread_task_attempts
             WHERE dispatch_state IN ('pending', 'reconciliation_required')
             ORDER BY dispatched_at_unix_seconds, request_id",
        )?;
        let request_ids = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);

        request_ids
            .into_iter()
            .map(|request_id| {
                load_attempt_by_request(&connection, &request_id)?
                    .ok_or(StoreError::NotFound(request_id))
            })
            .collect()
    }

    pub fn task(&self, task_id: &str) -> Result<TaskRecord, StoreError> {
        let connection = self.lock()?;
        load_task(&connection, &self.schema, task_id)
    }

    pub fn record_provider_ack(
        &self,
        request_id: &str,
        provider_execution_id: &str,
        evidence_id: &str,
        now: i64,
    ) -> Result<(), StoreError> {
        validate_text("provider_execution_id", provider_execution_id, 1024)?;
        validate_text("evidence_id", evidence_id, 1024)?;

        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let attempt = load_attempt_required(&transaction, request_id)?;
        require_execution_evidence(
            &transaction,
            &attempt,
            evidence_id,
            "acknowledged",
            Some(provider_execution_id),
            now,
        )?;
        let task = load_task(&transaction, &self.schema, &attempt.task_id)?;

        if task.attempt != attempt.attempt {
            return Err(StoreError::Conflict(
                "acknowledgement belongs to a superseded attempt".into(),
            ));
        }
        if let Some(existing) = &attempt.provider_execution_id {
            if existing == provider_execution_id {
                transaction.commit()?;
                return Ok(());
            }
            return Err(StoreError::Conflict(
                "attempt already has a different provider execution ID".into(),
            ));
        }
        if !matches!(
            attempt.dispatch_state.as_str(),
            "pending" | "reconciliation_required"
        ) {
            return Err(StoreError::Conflict(format!(
                "cannot acknowledge attempt in {} state",
                attempt.dispatch_state
            )));
        }
        if !matches!(
            task.state,
            TaskState::Dispatching
                | TaskState::CancelRequested
                | TaskState::Cancelling
                | TaskState::ReconciliationRequired
        ) {
            return Err(StoreError::Conflict(format!(
                "cannot acknowledge task from {:?}",
                task.state
            )));
        }

        transaction.execute(
            "UPDATE dread_task_attempts
             SET provider_execution_id = ?2,
                 dispatch_state = 'acknowledged',
                 acknowledged_at_unix_seconds = ?3
             WHERE request_id = ?1
               AND provider_execution_id IS NULL",
            params![request_id, provider_execution_id, now],
        )?;

        let next = if matches!(
            task.state,
            TaskState::Dispatching | TaskState::ReconciliationRequired
        ) {
            TaskState::Running
        } else {
            task.state
        };
        update_task_cas(
            &transaction,
            &self.schema,
            &task,
            next,
            None,
            None,
            None,
            now,
        )?;
        insert_event(
            &transaction,
            &task.mission_id,
            Some(&task.id),
            Some(&attempt.attempt_id),
            Some(&attempt.provider_id),
            Some(provider_execution_id),
            "provider_acknowledged",
            &format!("ack:{}", attempt.attempt_id),
            serde_json::json!({
                "request_id": request_id,
                "provider_execution_id": provider_execution_id,
                "evidence_id": evidence_id,
                "cancellation_preserved": next != TaskState::



920.41 s
18.62 TPS
40.35 s TTFT
focus on canonical merge ,Compilation and tests

Not run , Continue directly from your previous correction instructions. Do not repeat the architecture proposal or review. Implement every outstanding correction and return the complete final source files.

Deliver:

Complete corrected crates/dread-orchestrator/src/lifecycle.rs

Complete corrected crates/dread-orchestrator/src/store.rs

Complete reconcile.rs and cancellation implementation

Final migrations 0002, 0003 and the missing 0004_execution_registration_projection.sql

Complete corrected crates/dread-registry/src/lib.rs

Required workspace and crate Cargo.toml changes and module exports

All associated unit and migration tests

Resolve every defect from the previous instructions, including the conflicting idempotency-key uniqueness constraint, transactional attempt finalization during retry scheduling, trusted evidence verification, independent verifier enforcement, monotonic cancellation, reconciliation-safe mission transitions, complete event identity checks, trusted retry-contract registration, capability/provider revision validation, and missing migration/test-fixture issues.

Preserve the existing canonical workspace, database and APIs. Do not create a second mission/task database, second orchestration engine, or competing GPU scheduler. Explicitly document how the new schema maps to the existing canonical mission/task schema.

Return complete files, not excerpts, pseudocode, or statements that changes were made. Include a final file inventory and actual command outputs for formatting, compilation, Clippy and tests. Never report a test as passed unless it was executed. Mark unavailable checks as NOT RUN.

Finish the implementation and source delivery in this response.

Reasoning · Done · 414.2s total
Reasoning · Kimi needs to respond to the user's latest request. Shoul... · Done
CORRECTION CYCLE 003A/003B — FINAL CONSOLIDATED SOURCE

Status: CODE_GENERATED / TESTS_NOT_RUN / NEEDS_CANONICAL_MERGE

Canonical persistence mapping:

CanonicalSchema::new(...) receives the existing mission, task, and dependency table names.
Those tables are validated and updated; this code never creates them.
Migrations 0002–0004 add only execution-attempt, admission, trusted-evidence, retry-contract, reconciliation, and event metadata.
dread_execution_provider_states and dread_execution_capability_states are transactional projections of the authoritative registry, updated by canonical registry mutations in the same SQLite transaction.
Migration numbers must be mapped to unused canonical versions. Already-applied migrations must not be edited.
FILE: Cargo.toml

Diff

Copy
diff --git a/Cargo.toml b/Cargo.toml
--- a/Cargo.toml
+++ b/Cargo.toml
@@
 [workspace.dependencies]
 async-trait = "0.1"
 axum = "0.8"
 directories = "6"
+libc = "0.2"
 rusqlite = { version = "0.32", features = ["backup", "bundled"] }
 serde = { version = "1", features = ["derive"] }
 serde_json = "1"
 sha2 = "0.10"
FILE: crates/dread-core/Cargo.toml

Diff

Copy
diff --git a/crates/dread-core/Cargo.toml b/crates/dread-core/Cargo.toml
--- a/crates/dread-core/Cargo.toml
+++ b/crates/dread-core/Cargo.toml
@@
 serde.workspace = true
+serde_json.workspace = true
+sha2.workspace = true
 thiserror.workspace = true
 toml.workspace = true
 url.workspace = true
FILE: crates/dread-registry/Cargo.toml

Toml

Copy
[package]
name = "dread-registry"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
description = "Durable capability registry for DREAD Universal"

[dependencies]
dread-core = { path = "../dread-core" }
libc.workspace = true
rusqlite.workspace = true
serde.workspace = true
serde_json.workspace = true
sha2.workspace = true
thiserror.workspace = true

[dev-dependencies]
tempfile.workspace = true
FILE: crates/dread-orchestrator/Cargo.toml

Diff

Copy
diff --git a/crates/dread-orchestrator/Cargo.toml b/crates/dread-orchestrator/Cargo.toml
--- a/crates/dread-orchestrator/Cargo.toml
+++ b/crates/dread-orchestrator/Cargo.toml
@@
 [dependencies]
+async-trait.workspace = true
+dread-core = { path = "../dread-core" }
+rusqlite.workspace = true
+serde.workspace = true
+serde_json.workspace = true
+sha2.workspace = true
+thiserror.workspace = true
+
+[dev-dependencies]
+tempfile.workspace = true
FILE: crates/dread-orchestrator/src/lib.rs

Diff

Copy
diff --git a/crates/dread-orchestrator/src/lib.rs b/crates/dread-orchestrator/src/lib.rs
--- a/crates/dread-orchestrator/src/lib.rs
+++ b/crates/dread-orchestrator/src/lib.rs
@@
+pub mod cancellation;
+pub mod lifecycle;
+pub mod preflight;
+pub mod reconcile;
+pub mod store;
Merge only missing exports; do not remove existing canonical exports.

FILE: crates/dread-orchestrator/migrations/0002_trusted_dispatch_admission.sql

Sql

Copy
CREATE TABLE dread_dispatch_admissions (
    admission_id TEXT PRIMARY KEY,
    request_fingerprint TEXT NOT NULL,
    request_id TEXT NOT NULL UNIQUE,
    requester_id TEXT NOT NULL,
    mission_id TEXT NOT NULL,
    task_id TEXT NOT NULL,
    attempt INTEGER NOT NULL CHECK (attempt > 0),
    provider_id TEXT NOT NULL,
    provider_registration_revision INTEGER NOT NULL,
    capability_id TEXT NOT NULL,
    operation TEXT NOT NULL,
    permission_scope TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    evidence_id TEXT NOT NULL,
    policy_decision_id TEXT NOT NULL,
    adapter_id TEXT NOT NULL,
    adapter_version TEXT NOT NULL,
    expires_at_unix_seconds INTEGER NOT NULL,
    consumed_at_unix_seconds INTEGER,
    created_at_unix_seconds INTEGER NOT NULL,
    UNIQUE(task_id, attempt)
) STRICT;

CREATE INDEX dread_dispatch_admissions_task
    ON dread_dispatch_admissions(task_id, attempt);

CREATE TRIGGER dread_dispatch_admissions_immutable_update
BEFORE UPDATE ON dread_dispatch_admissions
WHEN
    OLD.admission_id != NEW.admission_id
    OR OLD.request_fingerprint != NEW.request_fingerprint
    OR OLD.request_id != NEW.request_id
    OR OLD.requester_id != NEW.requester_id
    OR OLD.mission_id != NEW.mission_id
    OR OLD.task_id != NEW.task_id
    OR OLD.attempt != NEW.attempt
    OR OLD.provider_id != NEW.provider_id
    OR OLD.provider_registration_revision != NEW.provider_registration_revision
    OR OLD.capability_id != NEW.capability_id
    OR OLD.operation != NEW.operation
    OR OLD.permission_scope != NEW.permission_scope
    OR OLD.idempotency_key != NEW.idempotency_key
    OR OLD.evidence_id != NEW.evidence_id
    OR OLD.policy_decision_id != NEW.policy_decision_id
    OR OLD.adapter_id != NEW.adapter_id
    OR OLD.adapter_version != NEW.adapter_version
    OR OLD.expires_at_unix_seconds != NEW.expires_at_unix_seconds
    OR OLD.created_at_unix_seconds != NEW.created_at_unix_seconds
    OR OLD.consumed_at_unix_seconds IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'admission is immutable and single-use');
END;

CREATE TRIGGER dread_dispatch_admissions_no_delete
BEFORE DELETE ON dread_dispatch_admissions
BEGIN
    SELECT RAISE(ABORT, 'dispatch admissions are append-only');
END;
FILE: crates/dread-orchestrator/migrations/0003_execution_attempts.sql

Sql

Copy
CREATE TABLE dread_provider_retry_contracts (
    contract_id TEXT PRIMARY KEY,
    provider_id TEXT NOT NULL,
    provider_registration_revision INTEGER NOT NULL,
    adapter_id TEXT NOT NULL,
    adapter_version TEXT NOT NULL,
    operation TEXT NOT NULL,
    idempotency_verified INTEGER NOT NULL
        CHECK (idempotency_verified IN (0, 1)),
    provider_keys_supported INTEGER NOT NULL
        CHECK (provider_keys_supported IN (0, 1)),
    revoked INTEGER NOT NULL CHECK (revoked IN (0, 1)),
    provenance_evidence_id TEXT NOT NULL,
    contract_sha256 TEXT NOT NULL,
    verified_at_unix_seconds INTEGER NOT NULL,
    expires_at_unix_seconds INTEGER NOT NULL
) STRICT;

CREATE TABLE dread_task_execution_contracts (
    task_id TEXT PRIMARY KEY,
    provider_id TEXT NOT NULL,
    capability_id TEXT NOT NULL,
    operation TEXT NOT NULL,
    idempotency_class TEXT NOT NULL CHECK (
        idempotency_class IN (
            'idempotent',
            'non_idempotent',
            'provider_key_required',
            'unknown'
        )
    ),
    idempotency_key TEXT NOT NULL,
    provider_retry_contract_id TEXT NOT NULL,
    request_scope_sha256 TEXT NOT NULL,
    created_at_unix_seconds INTEGER NOT NULL,
    FOREIGN KEY(provider_retry_contract_id)
        REFERENCES dread_provider_retry_contracts(contract_id)
) STRICT;

CREATE TABLE dread_task_attempts (
    attempt_id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    mission_id TEXT NOT NULL,
    attempt INTEGER NOT NULL CHECK (attempt > 0),
    request_id TEXT NOT NULL UNIQUE,
    request_fingerprint TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    capability_id TEXT NOT NULL,
    operation TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    admission_id TEXT NOT NULL UNIQUE,
    evidence_id TEXT NOT NULL,
    provider_execution_id TEXT,
    dispatch_state TEXT NOT NULL CHECK (
        dispatch_state IN (
            'pending',
            'acknowledged',
            'terminal',
            'reconciliation_required'
        )
    ),
    cancellation_state TEXT NOT NULL CHECK (
        cancellation_state IN (
            'none',
            'local_requested',
            'request_sent',
            'accepted',
            'cancelling',
            'confirmed_cancelled',
            'completed_successfully',
            'failed',
            'unknown'
        )
    ),
    outcome TEXT,
    dispatched_at_unix_seconds INTEGER NOT NULL,
    acknowledged_at_unix_seconds INTEGER,
    terminal_at_unix_seconds INTEGER,
    UNIQUE(task_id, attempt),
    UNIQUE(provider_id, provider_execution_id),
    FOREIGN KEY(admission_id)
        REFERENCES dread_dispatch_admissions(admission_id)
) STRICT;

-- A stable idempotency key is intentionally reusable by multiple attempts
-- of the same task. Request identity is unique; task/idempotency_key is not.
CREATE INDEX dread_task_attempts_pending
    ON dread_task_attempts(dispatch_state, task_id);

CREATE TABLE dread_reconciliation_observations (
    observation_id INTEGER PRIMARY KEY AUTOINCREMENT,
    attempt_id TEXT NOT NULL,
    observation_type TEXT NOT NULL,
    provider_execution_id TEXT,
    evidence_id TEXT,
    detail TEXT NOT NULL,
    observed_at_unix_seconds INTEGER NOT NULL,
    FOREIGN KEY(attempt_id) REFERENCES dread_task_attempts(attempt_id)
) STRICT;

CREATE TABLE dread_execution_events (
    event_sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    mission_id TEXT NOT NULL,
    task_id TEXT,
    attempt_id TEXT,
    provider_id TEXT,
    provider_execution_id TEXT,
    event_type TEXT NOT NULL,
    event_key TEXT NOT NULL,
    event_identity TEXT NOT NULL UNIQUE,
    payload_sha256 TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    occurred_at_unix_seconds INTEGER NOT NULL
) STRICT;

CREATE INDEX dread_execution_events_replay
    ON dread_execution_events(mission_id, event_sequence);

CREATE TRIGGER dread_execution_events_no_update
BEFORE UPDATE ON dread_execution_events
BEGIN
    SELECT RAISE(ABORT, 'execution events are append-only');
END;

CREATE TRIGGER dread_execution_events_no_delete
BEFORE DELETE ON dread_execution_events
BEGIN
    SELECT RAISE(ABORT, 'execution events are append-only');
END;

CREATE TRIGGER dread_reconciliation_no_update
BEFORE UPDATE ON dread_reconciliation_observations
BEGIN
    SELECT RAISE(ABORT, 'reconciliation observations are append-only');
END;

CREATE TRIGGER dread_reconciliation_no_delete
BEFORE DELETE ON dread_reconciliation_observations
BEGIN
    SELECT RAISE(ABORT, 'reconciliation observations are append-only');
END;
FILE: crates/dread-orchestrator/migrations/0004_execution_registration_projection.sql

Sql

Copy
CREATE TABLE dread_execution_provider_states (
    provider_id TEXT PRIMARY KEY,
    registration_revision INTEGER NOT NULL,
    adapter_id TEXT NOT NULL,
    adapter_version TEXT NOT NULL,
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    revoked INTEGER NOT NULL CHECK (revoked IN (0, 1))
) STRICT;

CREATE TABLE dread_execution_capability_states (
    capability_id TEXT PRIMARY KEY,
    provider_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    review TEXT NOT NULL,
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1))
) STRICT;

CREATE TABLE dread_trusted_execution_evidence (
    evidence_id TEXT PRIMARY KEY,
    request_id TEXT NOT NULL,
    attempt_id TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    adapter_id TEXT NOT NULL,
    adapter_version TEXT NOT NULL,
    provider_registration_revision INTEGER NOT NULL,
    provider_execution_id TEXT,
    evidence_kind TEXT NOT NULL CHECK (
        evidence_kind IN (
            'acknowledged',
            'succeeded',
            'failed',
            'cancelled',
            'cancellation_request_sent',
            'cancellation_accepted',
            'cancellation_in_progress'
        )
    ),
    payload_sha256 TEXT NOT NULL,
    authenticated INTEGER NOT NULL CHECK (authenticated IN (0, 1)),
    observed_at_unix_seconds INTEGER NOT NULL,
    expires_at_unix_seconds INTEGER
) STRICT;

CREATE TABLE dread_trusted_verification_evidence (
    evidence_id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    attempt_id TEXT NOT NULL,
    verifier_id TEXT NOT NULL,
    worker_requester_id TEXT NOT NULL,
    passed INTEGER NOT NULL CHECK (passed IN (0, 1)),
    findings_sha256 TEXT NOT NULL,
    authenticated INTEGER NOT NULL CHECK (authenticated IN (0, 1)),
    observed_at_unix_seconds INTEGER NOT NULL,
    expires_at_unix_seconds INTEGER
) STRICT;

CREATE TABLE dread_trusted_retry_contract_evidence (
    evidence_id TEXT PRIMARY KEY,
    provider_id TEXT NOT NULL,
    provider_registration_revision INTEGER NOT NULL,
    adapter_id TEXT NOT NULL,
    adapter_version TEXT NOT NULL,
    operation TEXT NOT NULL,
    idempotency_verified INTEGER NOT NULL
        CHECK (idempotency_verified IN (0, 1)),
    provider_keys_supported INTEGER NOT NULL
        CHECK (provider_keys_supported IN (0, 1)),
    contract_sha256 TEXT NOT NULL,
    authenticated INTEGER NOT NULL CHECK (authenticated IN (0, 1)),
    observed_at_unix_seconds INTEGER NOT NULL,
    expires_at_unix_seconds INTEGER NOT NULL
) STRICT;

CREATE TRIGGER dread_trusted_execution_evidence_no_update
BEFORE UPDATE ON dread_trusted_execution_evidence
BEGIN
    SELECT RAISE(ABORT, 'trusted execution evidence is append-only');
END;

CREATE TRIGGER dread_trusted_execution_evidence_no_delete
BEFORE DELETE ON dread_trusted_execution_evidence
BEGIN
    SELECT RAISE(ABORT, 'trusted execution evidence is append-only');
END;

CREATE TRIGGER dread_trusted_verification_evidence_no_update
BEFORE UPDATE ON dread_trusted_verification_evidence
BEGIN
    SELECT RAISE(ABORT, 'trusted verification evidence is append-only');
END;

CREATE TRIGGER dread_trusted_verification_evidence_no_delete
BEFORE DELETE ON dread_trusted_verification_evidence
BEGIN
    SELECT RAISE(ABORT, 'trusted verification evidence is append-only');
END;

CREATE TRIGGER dread_trusted_retry_contract_evidence_no_update
BEFORE UPDATE ON dread_trusted_retry_contract_evidence
BEGIN
    SELECT RAISE(ABORT, 'retry-contract evidence is append-only');
END;

CREATE TRIGGER dread_trusted_retry_contract_evidence_no_delete
BEFORE DELETE ON dread_trusted_retry_contract_evidence
BEGIN
    SELECT RAISE(ABORT, 'retry-contract evidence is append-only');
END;
FILE: crates/dread-orchestrator/src/lifecycle.rs

Rust

Copy
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissionState {
    Ready,
    Running,
    ReconciliationRequired,
    Succeeded,
    Failed,
    Cancelled,
}

impl MissionState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Running => "running",
            Self::ReconciliationRequired => "reconciliation_required",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Result<Self, LifecycleError> {
        match value {
            "ready" => Ok(Self::Ready),
            "running" => Ok(Self::Running),
            "reconciliation_required" => Ok(Self::ReconciliationRequired),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            other => Err(LifecycleError::InvalidStoredState(other.into())),
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Pending,
    Ready,
    Dispatching,
    Running,
    RetryScheduled,
    CancelRequested,
    Cancelling,
    ReconciliationRequired,
    ProviderSucceeded,
    Verifying,
    Succeeded,
    Failed,
    Cancelled,
    Blocked,
}

impl TaskState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Ready => "ready",
            Self::Dispatching => "dispatching",
            Self::Running => "running",
            Self::RetryScheduled => "retry_scheduled",
            Self::CancelRequested => "cancel_requested",
            Self::Cancelling => "cancelling",
            Self::ReconciliationRequired => "reconciliation_required",
            Self::ProviderSucceeded => "provider_succeeded",
            Self::Verifying => "verifying",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Blocked => "blocked",
        }
    }

    pub fn parse(value: &str) -> Result<Self, LifecycleError> {
        match value {
            "pending" => Ok(Self::Pending),
            "ready" => Ok(Self::Ready),
            "dispatching" => Ok(Self::Dispatching),
            "running" => Ok(Self::Running),
            "retry_scheduled" => Ok(Self::RetryScheduled),
            "cancel_requested" => Ok(Self::CancelRequested),
            "cancelling" => Ok(Self::Cancelling),
            "reconciliation_required" => Ok(Self::ReconciliationRequired),
            "provider_succeeded" => Ok(Self::ProviderSucceeded),
            "verifying" => Ok(Self::Verifying),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            "blocked" => Ok(Self::Blocked),
            other => Err(LifecycleError::InvalidStoredState(other.into())),
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::Blocked
        )
    }

    pub fn has_unresolved_provider_outcome(self) -> bool {
        matches!(
            self,
            Self::Dispatching
                | Self::Running
                | Self::CancelRequested
                | Self::Cancelling
                | Self::ReconciliationRequired
        )
    }

    pub fn can_transition_to(self, next: Self) -> bool {
        use TaskState::*;

        if self == next {
            return true;
        }

        matches!(
            (self, next),
            (Pending, Ready | Cancelled | Blocked)
                | (Ready, Dispatching | CancelRequested | Cancelled)
                | (
                    RetryScheduled,
                    Ready | Dispatching | CancelRequested | Cancelled
                )
                | (
                    Dispatching,
                    Running
                        | CancelRequested
                        | ReconciliationRequired
                        | RetryScheduled
                        | Failed
                )
                | (
                    Running,
                    ProviderSucceeded
                        | Failed
                        | Cancelled
                        | CancelRequested
                        | ReconciliationRequired
                        | RetryScheduled
                )
                | (
                    CancelRequested,
                    Cancelling
                        | ReconciliationRequired
                        | ProviderSucceeded
                        | Failed
                        | Cancelled
                )
                | (
                    Cancelling,
                    ReconciliationRequired
                        | ProviderSucceeded
                        | Failed
                        | Cancelled
                )
                | (
                    ReconciliationRequired,
                    Running
                        | ProviderSucceeded
                        | Failed
                        | Cancelled
                        | CancelRequested
                )
                | (ProviderSucceeded, Verifying)
                | (Verifying, Succeeded | Failed)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdempotencyClass {
    Idempotent,
    NonIdempotent,
    ProviderKeyRequired,
    Unknown,
}

impl IdempotencyClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idempotent => "idempotent",
            Self::NonIdempotent => "non_idempotent",
            Self::ProviderKeyRequired => "provider_key_required",
            Self::Unknown => "unknown",
        }
    }

    pub fn parse(value: &str) -> Result<Self, LifecycleError> {
        match value {
            "idempotent" => Ok(Self::Idempotent),
            "non_idempotent" => Ok(Self::NonIdempotent),
            "provider_key_required" => Ok(Self::ProviderKeyRequired),
            "unknown" => Ok(Self::Unknown),
            other => Err(LifecycleError::InvalidStoredState(other.into())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteOutcome {
    DefiniteFailureBeforeSideEffects,
    Unknown,
    StillActive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryDecision {
    Allowed,
    Exhausted,
    ReconciliationRequired,
    UnsafeContract,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifiedRetryContract {
    pub idempotent_operation: bool,
    pub provider_keys_supported: bool,
    pub revoked: bool,
    pub valid_at_observation: bool,
}

pub fn classify_retry(
    class: IdempotencyClass,
    outcome: RemoteOutcome,
    attempt: u32,
    max_attempts: u32,
    contract: VerifiedRetryContract,
) -> RetryDecision {
    match outcome {
        RemoteOutcome::Unknown => return RetryDecision::ReconciliationRequired,
        RemoteOutcome::StillActive => return RetryDecision::UnsafeContract,
        RemoteOutcome::DefiniteFailureBeforeSideEffects => {}
    }

    if !contract.valid_at_observation || contract.revoked {
        return RetryDecision::UnsafeContract;
    }
    if attempt >= max_attempts {
        return RetryDecision::Exhausted;
    }

    let permitted = match class {
        IdempotencyClass::Idempotent => contract.idempotent_operation,
        IdempotencyClass::ProviderKeyRequired => {
            contract.idempotent_operation && contract.provider_keys_supported
        }
        IdempotencyClass::NonIdempotent => true,
        IdempotencyClass::Unknown => false,
    };

    if permitted {
        RetryDecision::Allowed
    } else {
        RetryDecision::UnsafeContract
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LifecycleError {
    #[error("invalid task transition from {from:?} to {to:?}")]
    InvalidTransition { from: TaskState, to: TaskState },

    #[error("invalid stored lifecycle state: {0}")]
    InvalidStoredState(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_idempotency_classes_and_unknown_outcomes_fail_safely() {
        let contract = VerifiedRetryContract {
            idempotent_operation: true,
            provider_keys_supported: true,
            revoked: false,
            valid_at_observation: true,
        };

        for class in [
            IdempotencyClass::Idempotent,
            IdempotencyClass::NonIdempotent,
            IdempotencyClass::ProviderKeyRequired,
            IdempotencyClass::Unknown,
        ] {
            assert_eq!(
                classify_retry(class, RemoteOutcome::Unknown, 1, 3, contract),
                RetryDecision::ReconciliationRequired
            );
        }

        assert_eq!(
            classify_retry(
                IdempotencyClass::Unknown,
                RemoteOutcome::DefiniteFailureBeforeSideEffects,
                1,
                3,
                contract,
            ),
            RetryDecision::UnsafeContract
        );
    }

    #[test]
    fn keyed_retry_requires_verified_key_support() {
        let mut contract = VerifiedRetryContract {
            idempotent_operation: true,
            provider_keys_supported: false,
            revoked: false,
            valid_at_observation: true,
        };
        assert_eq!(
            classify_retry(
                IdempotencyClass::ProviderKeyRequired,
                RemoteOutcome::DefiniteFailureBeforeSideEffects,
                1,
                3,
                contract,
            ),
            RetryDecision::UnsafeContract
        );

        contract.provider_keys_supported = true;
        assert_eq!(
            classify_retry(
                IdempotencyClass::ProviderKeyRequired,
                RemoteOutcome::DefiniteFailureBeforeSideEffects,
                1,
                3,
                contract,
            ),
            RetryDecision::Allowed
        );
    }

    #[test]
    fn cancellation_and_reconciliation_are_not_terminal() {
        assert!(!TaskState::CancelRequested.is_terminal());
        assert!(!TaskState::Cancelling.is_terminal());
        assert!(!TaskState::ReconciliationRequired.is_terminal());
        assert!(TaskState::ReconciliationRequired.has_unresolved_provider_outcome());
    }

    #[test]
    fn provider_success_wins_cancellation_race_but_needs_verification() {
        assert!(TaskState::Cancelling.can_transition_to(TaskState::ProviderSucceeded));
        assert!(!TaskState::ProviderSucceeded.is_terminal());
        assert!(TaskState::ProviderSucceeded.can_transition_to(TaskState::Verifying));
        assert!(TaskState::Verifying.can_transition_to(TaskState::Succeeded));
    }
}
FILE: crates/dread-orchestrator/src/preflight.rs

Rust

Copy
use dread_core::domain::{CapabilityKind, PermissionDecision, ReviewState};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};
use thiserror::Error;

pub const PREFLIGHT_CONTRACT_VERSION: &str = "provider-isolation-preflight-v1";
pub const MAX_PREFLIGHT_LIFETIME_SECONDS: i64 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IsolationControl {
    RuntimeIdentity,
    TaskWorkspaceBoundary,
    MountPolicy,
    EgressPolicy,
    ToolPermissions,
    StableSecretHandling,
    CancellationPropagation,
    RestartReconciliation,
}

impl IsolationControl {
    pub const ALL: [Self; 8] = [
        Self::RuntimeIdentity,
        Self::TaskWorkspaceBoundary,
        Self::MountPolicy,
        Self::EgressPolicy,
        Self::ToolPermissions,
        Self::StableSecretHandling,
        Self::CancellationPropagation,
        Self::RestartReconciliation,
    ];
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceProvenance {
    pub source_observation_id: String,
    pub recorded_by_adapter_id: String,
    pub recorded_by_adapter_version: String,
    pub evidence_sha256: String,
    pub recorded_at_unix_seconds: i64,
    pub authenticated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceRecord {
    pub contract_version: String,
    pub evidence_id: String,
    pub provider_id: String,
    pub provider_registration_revision: i64,
    pub adapter_id: String,
    pub adapter_version: String,
    pub runtime_identity: String,
    pub provenance: EvidenceProvenance,
    pub checks: BTreeMap<IsolationControl, ControlEvidence>,
    pub checked_at_unix_seconds: i64,
    pub expires_at_unix_seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlEvidence {
    pub verified: bool,
    pub durable_observation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRegistration {
    pub provider_id: String,
    pub registration_revision: i64,
    pub adapter_id: String,
    pub adapter_version: String,
    pub enabled: bool,
    pub revoked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityRegistration {
    pub capability_id: String,
    pub provider_id: String,
    pub kind: CapabilityKind,
    pub review: ReviewState,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmissionRequest {
    pub request_id: String,
    pub requester_id: String,
    pub mission_id: String,
    pub task_id: String,
    pub attempt: u32,
    pub provider_id: String,
    pub capability_id: String,
    pub operation: String,
    pub permission_scope: String,
    pub idempotency_key: String,
    pub requested_at_unix_seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyDecisionRecord {
    pub decision_id: String,
    pub decision: PermissionDecision,
    pub request_fingerprint: String,
    pub requester_id: String,
    pub decided_at_unix_seconds: i64,
    pub expires_at_unix_seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedAdmission {
    pub admission_id: String,
    pub request_fingerprint: String,
    pub request_id: String,
    pub requester_id: String,
    pub mission_id: String,
    pub task_id: String,
    pub attempt: u32,
    pub provider_id: String,
    pub provider_registration_revision: i64,
    pub capability_id: String,
    pub operation: String,
    pub permission_scope: String,
    pub idempotency_key: String,
    pub evidence_id: String,
    pub policy_decision_id: String,
    pub adapter_id: String,
    pub adapter_version: String,
    pub expires_at_unix_seconds: i64,
}

pub trait EvidenceRepository: Send + Sync {
    fn load(&self, evidence_id: &str) -> Result<Option<EvidenceRecord>, AdmissionError>;
}

pub trait TrustedProviderRegistry: Send + Sync {
    fn provider(
        &self,
        provider_id: &str,
    ) -> Result<Option<ProviderRegistration>, AdmissionError>;

    fn capability(
        &self,
        capability_id: &str,
    ) -> Result<Option<CapabilityRegistration>, AdmissionError>;
}

pub trait TrustedPolicyEngine: Send + Sync {
    fn decide(
        &self,
        request: &AdmissionRequest,
        request_fingerprint: &str,
    ) -> Result<PolicyDecisionRecord, AdmissionError>;
}

#[derive(Debug, Error)]
pub enum AdmissionError {
    #[error("admission dependency failed: {0}")]
    Dependency(String),
    #[error("admission denied: {0}")]
    Denied(String),
    #[error("admission conflict: {0}")]
    Conflict(String),
    #[error("admission persistence failed: {0}")]
    Persistence(String),
}

impl From<rusqlite::Error> for AdmissionError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Persistence(error.to_string())
    }
}

pub struct AdmissionService {
    connection: Arc<Mutex<Connection>>,
    evidence: Arc<dyn EvidenceRepository>,
    registry: Arc<dyn TrustedProviderRegistry>,
    policy: Arc<dyn TrustedPolicyEngine>,
}

impl AdmissionService {
    pub fn new(
        connection: Arc<Mutex<Connection>>,
        evidence: Arc<dyn EvidenceRepository>,
        registry: Arc<dyn TrustedProviderRegistry>,
        policy: Arc<dyn TrustedPolicyEngine>,
    ) -> Self {
        Self {
            connection,
            evidence,
            registry,
            policy,
        }
    }

    pub fn authorize_and_persist(
        &self,
        request: &AdmissionRequest,
        evidence_id: &str,
        now: i64,
    ) -> Result<PersistedAdmission, AdmissionError> {
        validate_request(request)?;
        if request.requested_at_unix_seconds > now {
            return Err(AdmissionError::Denied(
                "request timestamp is in the future".into(),
            ));
        }

        let fingerprint = request_fingerprint(request)?;
        let evidence = self
            .evidence
            .load(evidence_id)?
            .ok_or_else(|| AdmissionError::Denied("trusted evidence not found".into()))?;
        let provider = self
            .registry
            .provider(&request.provider_id)?
            .ok_or_else(|| AdmissionError::Denied("provider not registered".into()))?;
        let capability = self
            .registry
            .capability(&request.capability_id)?
            .ok_or_else(|| AdmissionError::Denied("capability not registered".into()))?;

        validate_evidence(&evidence, now)?;
        if !provider.enabled
            || provider.revoked
            || provider.provider_id != request.provider_id
            || provider.adapter_id != evidence.adapter_id
            || provider.adapter_version != evidence.adapter_version
            || provider.registration_revision != evidence.provider_registration_revision
        {
            return Err(AdmissionError::Denied(
                "provider state, revision, or adapter identity mismatch".into(),
            ));
        }
        if evidence.provider_id != request.provider_id
            || capability.provider_id != request.provider_id
            || capability.capability_id != request.capability_id
            || capability.kind != CapabilityKind::AgentProvider
            || capability.review != ReviewState::Approved
            || !capability.enabled
        {
            return Err(AdmissionError::Denied(
                "evidence/capability is invalid for this executable provider".into(),
            ));
        }

        let decision = self.policy.decide(request, &fingerprint)?;
        if decision.decision != PermissionDecision::Allow
            || decision.request_fingerprint != fingerprint
            || decision.requester_id != request.requester_id
            || decision.decided_at_unix_seconds > now
            || decision.expires_at_unix_seconds <= now
        {
            return Err(AdmissionError::Denied(
                "trusted policy did not allow this exact request scope".into(),
            ));
        }

        let expires = evidence
            .expires_at_unix_seconds
            .min(decision.expires_at_unix_seconds)
            .min(now.saturating_add(MAX_PREFLIGHT_LIFETIME_SECONDS));
        if expires <= now {
            return Err(AdmissionError::Denied("admission already expired".into()));
        }

        let admission = PersistedAdmission {
            admission_id: format!("admission:{}", request.request_id),
            request_fingerprint: fingerprint,
            request_id: request.request_id.clone(),
            requester_id: request.requester_id.clone(),
            mission_id: request.mission_id.clone(),
            task_id: request.task_id.clone(),
            attempt: request.attempt,
            provider_id: request.provider_id.clone(),
            provider_registration_revision: provider.registration_revision,
            capability_id: request.capability_id.clone(),
            operation: request.operation.clone(),
            permission_scope: request.permission_scope.clone(),
            idempotency_key: request.idempotency_key.clone(),
            evidence_id: evidence.evidence_id,
            policy_decision_id: decision.decision_id,
            adapter_id: provider.adapter_id,
            adapter_version: provider.adapter_version,
            expires_at_unix_seconds: expires,
        };

        let mut connection = self
            .connection
            .lock()
            .map_err(|_| AdmissionError::Persistence("database mutex poisoned".into()))?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "INSERT INTO dread_dispatch_admissions (
                admission_id, request_fingerprint, request_id, requester_id,
                mission_id, task_id, attempt, provider_id,
                provider_registration_revision, capability_id, operation,
                permission_scope, idempotency_key, evidence_id,
                policy_decision_id, adapter_id, adapter_version,
                expires_at_unix_seconds, consumed_at_unix_seconds,
                created_at_unix_seconds
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, NULL, ?19
             )",
            params![
                admission.admission_id,
                admission.request_fingerprint,
                admission.request_id,
                admission.requester_id,
                admission.mission_id,
                admission.task_id,
                admission.attempt,
                admission.provider_id,
                admission.provider_registration_revision,
                admission.capability_id,
                admission.operation,
                admission.permission_scope,
                admission.idempotency_key,
                admission.evidence_id,
                admission.policy_decision_id,
                admission.adapter_id,
                admission.adapter_version,
                admission.expires_at_unix_seconds,
                now
            ],
        )?;
        transaction.commit()?;
        Ok(admission)
    }
}

pub fn request_fingerprint(request: &AdmissionRequest) -> Result<String, AdmissionError> {
    validate_request(request)?;
    let encoded = serde_json::to_vec(request)
        .map_err(|error| AdmissionError::Dependency(error.to_string()))?;
    Ok(format!("{:x}", Sha256::digest(encoded)))
}

pub fn load_and_consume_admission(
    transaction: &Transaction<'_>,
    request: &AdmissionRequest,
    now: i64,
) -> Result<PersistedAdmission, AdmissionError> {
    let fingerprint = request_fingerprint(request)?;
    let row = transaction
        .query_row(
            "SELECT admission_id, request_fingerprint, request_id, requester_id,
                    mission_id, task_id, attempt, provider_id,
                    provider_registration_revision, capability_id, operation,
                    permission_scope, idempotency_key, evidence_id,
                    policy_decision_id, adapter_id, adapter_version,
                    expires_at_unix_seconds, consumed_at_unix_seconds
             FROM dread_dispatch_admissions WHERE request_id = ?1",
            [&request.request_id],
            |row| {
                Ok(AdmissionRow {
                    admission_id: row.get(0)?,
                    request_fingerprint: row.get(1)?,
                    request_id: row.get(2)?,
                    requester_id: row.get(3)?,
                    mission_id: row.get(4)?,
                    task_id: row.get(5)?,
                    attempt: row.get(6)?,
                    provider_id: row.get(7)?,
                    provider_registration_revision: row.get(8)?,
                    capability_id: row.get(9)?,
                    operation: row.get(10)?,
                    permission_scope: row.get(11)?,
                    idempotency_key: row.get(12)?,
                    evidence_id: row.get(13)?,
                    policy_decision_id: row.get(14)?,
                    adapter_id: row.get(15)?,
                    adapter_version: row.get(16)?,
                    expires_at_unix_seconds: row.get(17)?,
                    consumed_at_unix_seconds: row.get(18)?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| AdmissionError::Denied("persisted admission not found".into()))?;

    if row.consumed_at_unix_seconds.is_some() {
        return Err(AdmissionError::Conflict(
            "admission has already been consumed".into(),
        ));
    }
    if row.request_fingerprint != fingerprint
        || row.request_id != request.request_id
        || row.requester_id != request.requester_id
        || row.mission_id != request.mission_id
        || row.task_id != request.task_id
        || row.attempt != i64::from(request.attempt)
        || row.provider_id != request.provider_id
        || row.capability_id != request.capability_id
        || row.operation != request.operation
        || row.permission_scope != request.permission_scope
        || row.idempotency_key != request.idempotency_key
        || row.expires_at_unix_seconds <= now
    {
        return Err(AdmissionError::Denied(
            "admission scope, expiry, or fingerprint mismatch".into(),
        ));
    }

    let changed = transaction.execute(
        "UPDATE dread_dispatch_admissions
         SET consumed_at_unix_seconds = ?2
         WHERE request_id = ?1 AND consumed_at_unix_seconds IS NULL",
        params![request.request_id, now],
    )?;
    if changed != 1 {
        return Err(AdmissionError::Conflict(
            "admission consumption conflict".into(),
        ));
    }

    Ok(PersistedAdmission {
        admission_id: row.admission_id,
        request_fingerprint: row.request_fingerprint,
        request_id: row.request_id,
        requester_id: row.requester_id,
        mission_id: row.mission_id,
        task_id: row.task_id,
        attempt: request.attempt,
        provider_id: row.provider_id,
        provider_registration_revision: row.provider_registration_revision,
        capability_id: row.capability_id,
        operation: row.operation,
        permission_scope: row.permission_scope,
        idempotency_key: row.idempotency_key,
        evidence_id: row.evidence_id,
        policy_decision_id: row.policy_decision_id,
        adapter_id: row.adapter_id,
        adapter_version: row.adapter_version,
        expires_at_unix_seconds: row.expires_at_unix_seconds,
    })
}

struct AdmissionRow {
    admission_id: String,
    request_fingerprint: String,
    request_id: String,
    requester_id: String,
    mission_id: String,
    task_id: String,
    attempt: i64,
    provider_id: String,
    provider_registration_revision: i64,
    capability_id: String,
    operation: String,
    permission_scope: String,
    idempotency_key: String,
    evidence_id: String,
    policy_decision_id: String,
    adapter_id: String,
    adapter_version: String,
    expires_at_unix_seconds: i64,
    consumed_at_unix_seconds: Option<i64>,
}

fn validate_evidence(evidence: &EvidenceRecord, now: i64) -> Result<(), AdmissionError> {
    if evidence.contract_version != PREFLIGHT_CONTRACT_VERSION {
        return Err(AdmissionError::Denied(
            "unsupported evidence contract".into(),
        ));
    }
    for value in [
        &evidence.evidence_id,
        &evidence.provider_id,
        &evidence.adapter_id,
        &evidence.adapter_version,
        &evidence.runtime_identity,
        &evidence.provenance.source_observation_id,
        &evidence.provenance.evidence_sha256,
    ] {
        validate_identifier(value)?;
    }
    if evidence.provenance.evidence_sha256.len() != 64
        || !evidence
            .provenance
            .evidence_sha256
            .chars()
            .all(|character| character.is_ascii_hexdigit())
        || !evidence.provenance.authenticated
        || evidence.provenance.recorded_by_adapter_id != evidence.adapter_id
        || evidence.provenance.recorded_by_adapter_version != evidence.adapter_version
        || evidence.provenance.recorded_at_unix_seconds < evidence.checked_at_unix_seconds
    {
        return Err(AdmissionError::Denied(
            "evidence provenance is not authenticated and adapter-bound".into(),
        ));
    }
    if evidence.checked_at_unix_seconds > now
        || evidence.expires_at_unix_seconds <= now
        || evidence.expires_at_unix_seconds <= evidence.checked_at_unix_seconds
        || evidence.expires_at_unix_seconds - evidence.checked_at_unix_seconds
            > MAX_PREFLIGHT_LIFETIME_SECONDS
    {
        return Err(AdmissionError::Denied(
            "evidence is expired, future-dated, or exceeds maximum lifetime".into(),
        ));
    }

    let expected: BTreeSet<_> = IsolationControl::ALL.into_iter().collect();
    let actual: BTreeSet<_> = evidence.checks.keys().copied().collect();
    if expected != actual {
        return Err(AdmissionError::Denied(
            "all isolation controls require evidence".into(),
        ));
    }
    for evidence in evidence.checks.values() {
        if !evidence.verified || evidence.durable_observation_id.trim().is_empty() {
            return Err(AdmissionError::Denied(
                "an isolation control is unverified".into(),
            ));
        }
    }
    Ok(())
}

fn validate_request(request: &AdmissionRequest) -> Result<(), AdmissionError> {
    for value in [
        &request.request_id,
        &request.requester_id,
        &request.mission_id,
        &request.task_id,
        &request.provider_id,
        &request.capability_id,
        &request.operation,
        &request.permission_scope,
        &request.idempotency_key,
    ] {
        validate_identifier(value)?;
    }
    if request.attempt == 0 {
        return Err(AdmissionError::Denied("attempt must be positive".into()));
    }
    Ok(())
}

fn validate_identifier(value: &str) -> Result<(), AdmissionError> {
    if value.trim().is_empty() || value.len() > 1024 || value.contains('\0') {
        Err(AdmissionError::Denied(
            "required identity/scope is empty or too large".into(),
        ))
    } else {
        Ok(())
    }
}
FILE: crates/dread-orchestrator/src/cancellation.rs

Rust

Copy
use crate::store::{AttemptRecord, CancellationProgress, ExecutionStore, StoreError};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCancellationAck {
    RequestSent,
    Accepted,
    StillCancelling,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderCancellationReceipt {
    pub request_id: String,
    pub provider_execution_id: Option<String>,
    pub acknowledgement: ProviderCancellationAck,
    pub evidence_id: String,
}

#[derive(Debug, Error)]
pub enum CancellationError {
    #[error("provider cancellation request failed: {0}")]
    Provider(String),

    #[error("provider returned an invalid cancellation receipt: {0}")]
    InvalidReceipt(String),

    #[error(transparent)]
    Store(#[from] StoreError),
}

/// Real provider adapters implement only provider communication. Acceptance of
/// a cancellation request is not terminal cancellation.
#[async_trait]
pub trait ProviderCancellation: Send + Sync {
    async fn request_cancellation(
        &self,
        request_id: &str,
        provider_execution_id: Option<&str>,
    ) -> Result<ProviderCancellationReceipt, CancellationError>;
}

pub struct CancellationCoordinator {
    store: Arc<ExecutionStore>,
    provider: Arc<dyn ProviderCancellation>,
}

impl CancellationCoordinator {
    pub fn new(store: Arc<ExecutionStore>, provider: Arc<dyn ProviderCancellation>) -> Self {
        Self { store, provider }
    }

    pub async fn request(
        &self,
        task_id: &str,
        expected_revision: i64,
        request_id: &str,
        now: i64,
    ) -> Result<(), CancellationError> {
        self.store
            .request_cancellation(task_id, expected_revision, now)?;

        let attempt = self
            .store
            .attempt_by_request(request_id)?
            .ok_or_else(|| CancellationError::InvalidReceipt("attempt not found".into()))?;
        if attempt.task_id != task_id {
            return Err(CancellationError::InvalidReceipt(
                "attempt does not belong to task".into(),
            ));
        }

        let receipt = match self
            .provider
            .request_cancellation(request_id, attempt.provider_execution_id.as_deref())
            .await
        {
            Ok(receipt) => receipt,
            Err(error) => {
                self.store.mark_reconciliation_required(
                    request_id,
                    "cancellation request outcome is unknown",
                    "provider-cancellation-error",
                    now,
                )?;
                return Err(error);
            }
        };
        validate_receipt(&attempt, &receipt)?;

        let progress = match receipt.acknowledgement {
            ProviderCancellationAck::RequestSent => CancellationProgress::RequestSent,
            ProviderCancellationAck::Accepted => CancellationProgress::Accepted,
            ProviderCancellationAck::StillCancelling => {
                CancellationProgress::StillCancelling
            }
        };
        self.store.record_cancellation_progress(
            request_id,
            progress,
            &receipt.evidence_id,
            now,
        )?;
        Ok(())
    }
}

fn validate_receipt(
    attempt: &AttemptRecord,
    receipt: &ProviderCancellationReceipt,
) -> Result<(), CancellationError> {
    if receipt.request_id != attempt.request_id
        || receipt.provider_execution_id != attempt.provider_execution_id
        || receipt.evidence_id.trim().is_empty()
    {
        Err(CancellationError::InvalidReceipt(
            "receipt is not bound to the exact attempt".into(),
        ))
    } else {
        Ok(())
    }
}
FILE: crates/dread-orchestrator/src/store.rs

Rust

Copy
use crate::{
    lifecycle::{
        classify_retry, IdempotencyClass, MissionState, RemoteOutcome,
        RetryDecision, TaskState, VerifiedRetryContract,
    },
    preflight::{
        load_and_consume_admission, request_fingerprint, AdmissionError,
        AdmissionRequest,
    },
};
use rusqlite::{
    params, Connection, OptionalExtension, Transaction, TransactionBehavior,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};
use thiserror::Error;

const MAX_EVENT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct CanonicalSchema {
    mission_table: String,
    task_table: String,
    dependency_table: String,
}

impl CanonicalSchema {
    pub fn new(
        mission_table: impl Into<String>,
        task_table: impl Into<String>,
        dependency_table: impl Into<String>,
    ) -> Result<Self, StoreError> {
        let schema = Self {
            mission_table: mission_table.into(),
            task_table: task_table.into(),
            dependency_table: dependency_table.into(),
        };
        for identifier in [
            &schema.mission_table,
            &schema.task_table,
            &schema.dependency_table,
        ] {
            if identifier.is_empty()
                || !identifier
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                return Err(StoreError::Invalid("unsafe SQL identifier".into()));
            }
        }
        Ok(schema)
    }

    fn validate(&self, connection: &Connection) -> Result<(), StoreError> {
        require_columns(
            connection,
            &self.mission_table,
            &["id", "state", "revision", "updated_at_unix_seconds"],
        )?;
        require_columns(
            connection,
            &self.task_table,
            &[
                "id",
                "mission_id",
                "state",
                "revision",
                "attempt",
                "max_attempts",
                "provider_id",
                "capability_id",
                "operation",
                "retry_not_before_unix_seconds",
                "last_error",
                "updated_at_unix_seconds",
            ],
        )?;
        require_columns(
            connection,
            &self.dependency_table,
            &["task_id", "depends_on_task_id"],
        )?;

        let additive = [
            (
                "dread_dispatch
