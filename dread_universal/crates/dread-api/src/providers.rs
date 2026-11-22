//! Provider drop-ins: any remote LLM provider (OpenAI-compatible endpoint,
//! NVIDIA NIM, …) becomes a registry entry by dropping a small JSON file into
//! the provider drop-in directory. This is the "add any model automatically
//! once downloaded" promise for remote providers: copy a file, call sync, the
//! hub knows the provider — idempotently.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const REGISTRY_UPSERT: &str = "INSERT INTO registry (id, name, kind, endpoint, metadata, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT(id) DO UPDATE SET name=excluded.name, endpoint=excluded.endpoint, metadata=excluded.metadata";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderSpec {
    pub name: String,
    pub kind: String,
    pub base_url: String,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
}

fn default_true() -> bool {
    true
}

fn is_valid_kind(kind: &str) -> bool {
    matches!(kind, "openai-compatible" | "nvidia-nim" | "custom")
}

fn is_valid_env_name(value: &str) -> bool {
    value.len() >= 2
        && value
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
}

impl ProviderSpec {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("name must not be empty".to_string());
        }
        if !is_valid_kind(&self.kind) {
            return Err(format!(
                "unsupported kind {} (allowed: openai-compatible, nvidia-nim, custom)",
                self.kind
            ));
        }
        let base = self.base_url.trim();
        if !(base.starts_with("http://") || base.starts_with("https://")) {
            return Err("base_url must start with http:// or https://".to_string());
        }
        if base.chars().any(char::is_whitespace) {
            return Err("base_url must not contain whitespace".to_string());
        }
        if self
            .api_key_env
            .as_ref()
            .is_some_and(|env_name| !is_valid_env_name(env_name))
        {
            return Err("api_key_env must be [A-Z0-9_], at least 2 characters".to_string());
        }
        Ok(())
    }
}

/// Stable identity: derived from endpoint + kind, never from the display name,
/// so renaming a provider in its drop-in updates the row instead of leaving a
/// stale duplicate behind.
fn provider_id(base_url: &str, kind: &str) -> String {
    format!(
        "provider-{}",
        sha256_hex_prefix(format!("{base_url}|{kind}").as_bytes(), 12)
    )
}

fn sha256_hex_prefix(bytes: &[u8], len: usize) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, bytes);
    digest
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
        .chars()
        .take(len)
        .collect()
}

/// Reads `*.json` drop-in files. Per-file problems become warnings so one bad
/// download cannot poison the batch; directory entries are never followed.
pub fn scan_provider_dropins(dir: &Path) -> (Vec<(PathBuf, ProviderSpec)>, Vec<String>) {
    let mut specs = Vec::new();
    let mut warnings = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            warnings.push(format!(
                "could not read provider drop-in directory {}: {error}",
                dir.display()
            ));
            return (specs, warnings);
        }
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
        })
        .filter(|path| path.is_file())
        .collect();
    paths.sort();
    for path in paths {
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) => {
                warnings.push(format!("{}: {error}", path.display()));
                continue;
            }
        };
        match serde_json::from_str::<ProviderSpec>(&text) {
            Ok(spec) => match spec.validate() {
                Ok(()) => specs.push((path, spec)),
                Err(problem) => warnings.push(format!("{}: {problem}", path.display())),
            },
            Err(error) => warnings.push(format!(
                "{}: invalid provider drop-in: {error}",
                path.display()
            )),
        }
    }
    (specs, warnings)
}

pub fn register_provider(
    conn: &rusqlite::Connection,
    spec: &ProviderSpec,
    source: Option<&Path>,
    now: i64,
) -> Result<String, String> {
    spec.validate()?;
    let id = provider_id(&spec.base_url, &spec.kind);
    let mut metadata = Map::new();
    metadata.insert(
        "provider_spec".to_string(),
        serde_json::to_value(spec).map_err(|error| error.to_string())?,
    );
    metadata.insert("dropin_enabled".to_string(), json!(spec.enabled));
    metadata.insert("registered_at".to_string(), json!(now));
    if let Some(source) = source {
        metadata.insert("dropin_path".to_string(), json!(source.to_string_lossy()));
    }
    conn.execute(
        REGISTRY_UPSERT,
        rusqlite::params![
            id,
            spec.name,
            format!("provider.{}", spec.kind),
            spec.base_url,
            Value::Object(metadata).to_string(),
            now
        ],
    )
    .map_err(|error| format!("could not register provider {}: {error}", spec.name))?;
    Ok(id)
}

pub fn register_all_providers(conn: &rusqlite::Connection, dir: &Path, now: i64) -> Value {
    let (specs, mut warnings) = scan_provider_dropins(dir);
    let mut registered = 0usize;
    for (path, spec) in specs {
        match register_provider(conn, &spec, Some(&path), now) {
            Ok(_) => registered += 1,
            Err(problem) => warnings.push(format!("{}: {problem}", path.display())),
        }
    }
    json!({"registered": registered, "warnings": warnings})
}

/// Registry rows for providers, newest first, as JSON for the UI.
pub fn providers_listing(conn: &rusqlite::Connection) -> Result<Vec<Value>, String> {
    let mut statement = conn
        .prepare(
            "SELECT id, name, kind, endpoint, metadata, created_at FROM registry WHERE kind LIKE 'provider.%' ORDER BY created_at DESC, id ASC",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| {
            let metadata_text: String = row.get(4)?;
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "name": row.get::<_, String>(1)?,
                "kind": row.get::<_, String>(2)?,
                "endpoint": row.get::<_, String>(3)?,
                "metadata": serde_json::from_str::<Value>(&metadata_text).unwrap_or(Value::Null),
                "created_at": row.get::<_, i64>(5)?,
            }))
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<Value>, _>>()
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn memory_db() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE registry (id TEXT PRIMARY KEY, name TEXT NOT NULL, kind TEXT NOT NULL, endpoint TEXT NOT NULL, metadata TEXT NOT NULL, created_at INTEGER NOT NULL);",
        )
        .unwrap();
        conn
    }

    fn temp_label(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("dread-providers-{label}-{nonce}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn kimi_spec(name: &str) -> ProviderSpec {
        ProviderSpec {
            name: name.to_string(),
            kind: "nvidia-nim".to_string(),
            base_url: "https://integrate.api.nvidia.com/v1".to_string(),
            api_key_env: Some("DREAD_NVIDIA_API_KEY".to_string()),
            model: Some("moonshotai/kimi-k3.1-pro".to_string()),
            enabled: true,
            headers: BTreeMap::new(),
        }
    }

    #[test]
    fn dropin_file_round_trips_into_registry_once() {
        let dir = temp_label("roundtrip");
        fs::write(
            dir.join("kimi.json"),
            r#"{"name":"Kimi K3.1 Pro","kind":"nvidia-nim","base_url":"https://integrate.api.nvidia.com/v1","api_key_env":"DREAD_NVIDIA_API_KEY","model":"moonshotai/kimi-k3.1-pro"}"#,
        )
        .unwrap();
        let conn = memory_db();
        let report = register_all_providers(&conn, &dir, 100);
        assert_eq!(report["registered"], 1);
        assert_eq!(report["warnings"].as_array().unwrap().len(), 0);
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM registry", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
        let kind: String = conn
            .query_row("SELECT kind FROM registry", [], |row| row.get(0))
            .unwrap();
        assert_eq!(kind, "provider.nvidia-nim");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn resync_is_idempotent_and_picks_up_renames() {
        let conn = memory_db();
        register_provider(&conn, &kimi_spec("Kimi Old"), None, 1).unwrap();
        register_provider(&conn, &kimi_spec("Kimi New"), None, 2).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM registry", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
        let name: String = conn
            .query_row("SELECT name FROM registry", [], |row| row.get(0))
            .unwrap();
        assert_eq!(name, "Kimi New");
    }

    #[test]
    fn bad_files_warn_without_poisoning_the_batch() {
        let dir = temp_label("badfiles");
        fs::write(dir.join("broken.json"), "{not json").unwrap();
        fs::write(
            dir.join("alien.json"),
            r#"{"name":"X","kind":"magic-cloud","base_url":"https://x.test/v1"}"#,
        )
        .unwrap();
        fs::write(
            dir.join("space.json"),
            r#"{"name":"Y","kind":"custom","base_url":"https://y.test/ has space"}"#,
        )
        .unwrap();
        fs::write(
            dir.join("good.json"),
            r#"{"name":"Good","kind":"openai-compatible","base_url":"https://g.test/v1"}"#,
        )
        .unwrap();
        fs::write(dir.join("ignored.txt"), "not json extension").unwrap();
        let conn = memory_db();
        let report = register_all_providers(&conn, &dir, 5);
        assert_eq!(report["registered"], 1);
        assert_eq!(report["warnings"].as_array().unwrap().len(), 3);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn validation_rejects_dangerous_shapes() {
        let mut spec = kimi_spec("ok");
        spec.base_url = "https://x.test/v1 ".to_string() + "\ttab";
        assert!(spec.validate().is_err());
        let mut spec = kimi_spec("ok");
        spec.api_key_env = Some("lower case".to_string());
        assert!(spec.validate().is_err());
        assert!(kimi_spec("ok").validate().is_ok());
    }

    #[test]
    fn listing_filters_provider_rows_only() {
        let conn = memory_db();
        register_provider(&conn, &kimi_spec("Kimi"), None, 7).unwrap();
        conn.execute(
            REGISTRY_UPSERT,
            rusqlite::params![
                "model-abc",
                "some.gguf",
                "model.gguf",
                "/models/some.gguf",
                "{}",
                8
            ],
        )
        .unwrap();
        let listing = providers_listing(&conn).unwrap();
        assert_eq!(listing.len(), 1);
        assert_eq!(
            listing[0]["id"],
            provider_id("https://integrate.api.nvidia.com/v1", "nvidia-nim")
        );
        assert_eq!(listing[0]["metadata"]["dropin_enabled"], true);
    }
}
