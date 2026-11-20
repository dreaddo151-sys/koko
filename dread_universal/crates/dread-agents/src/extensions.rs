//! Read-only discovery of local Cursor, Claude and generic plugin manifests.
//!
//! Discovered content is untrusted metadata. This module never loads code,
//! runs hooks or commands, starts MCP servers, or enables a plugin.

use serde::Serialize;
use serde_json::Value;
use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
};

const DEFAULT_MAX_DEPTH: usize = 7;
const DEFAULT_MAX_ENTRIES: usize = 12_000;
const MAX_ROOTS: usize = 16;
const MAX_PLUGINS: usize = 512;
const MAX_MANIFEST_BYTES: u64 = 128 * 1024;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PluginComponentCounts {
    pub agents: usize,
    pub skills: usize,
    pub commands: usize,
    pub hooks: usize,
    pub mcp_servers: usize,
    pub tools: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LocalPluginDescriptor {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub license: Option<String>,
    pub homepage: Option<String>,
    pub repository: Option<String>,
    pub ecosystem: String,
    pub manifest_path: String,
    pub plugin_root: String,
    pub manifest_bytes: u64,
    pub components: PluginComponentCounts,
    pub status: String,
    pub trust: String,
    pub execution: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PluginInventory {
    pub roots_scanned: Vec<String>,
    pub scanned_entries: usize,
    pub truncated: bool,
    pub plugins: Vec<LocalPluginDescriptor>,
    pub warnings: Vec<String>,
    pub note: String,
}

pub fn discover_local_plugins() -> PluginInventory {
    discover_local_plugins_in(
        &configured_plugin_roots(),
        DEFAULT_MAX_DEPTH,
        DEFAULT_MAX_ENTRIES,
    )
}

fn configured_plugin_roots() -> Vec<PathBuf> {
    if let Some(value) = env::var_os("DREAD_PLUGIN_ROOTS") {
        return env::split_paths(&value).take(MAX_ROOTS + 1).collect();
    }
    let mut roots = vec![PathBuf::from("integrations/cursor-plugins-full")];
    if let Some(home) = env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
    {
        roots.push(home.join(".cursor/plugins"));
        roots.push(home.join(".claude/plugins"));
    }
    if let Some(app_data) = env::var_os("APPDATA").map(PathBuf::from) {
        roots.push(app_data.join("Cursor/plugins"));
        roots.push(app_data.join("Claude/plugins"));
    }
    roots
}

pub fn discover_local_plugins_in(
    roots: &[PathBuf],
    max_depth: usize,
    max_entries: usize,
) -> PluginInventory {
    let mut inventory = PluginInventory {
        roots_scanned: Vec::new(),
        scanned_entries: 0,
        truncated: roots.len() > MAX_ROOTS,
        plugins: Vec::new(),
        warnings: Vec::new(),
        note: "Manifest metadata only. Discovered plugins are untrusted and disabled; no code, hooks, commands, agents, tools, or MCP servers were executed.".into(),
    };
    let mut seen_roots = BTreeSet::new();
    let mut seen_plugins = BTreeSet::new();

    if roots.len() > MAX_ROOTS {
        inventory.warnings.push(format!(
            "Plugin root list exceeded the {MAX_ROOTS}-root limit; additional roots were not scanned"
        ));
    }

    for configured_root in roots.iter().take(MAX_ROOTS) {
        if inventory.plugins.len() >= MAX_PLUGINS || inventory.scanned_entries >= max_entries {
            inventory.truncated = true;
            break;
        }
        let root_meta = match fs::symlink_metadata(configured_root) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                inventory.warnings.push(format!(
                    "Plugin root does not exist: {}",
                    configured_root.display()
                ));
                continue;
            }
            Err(error) => {
                inventory.warnings.push(format!(
                    "Could not inspect plugin root {}: {error}",
                    configured_root.display()
                ));
                continue;
            }
        };
        if !root_meta.is_dir() || root_meta.file_type().is_symlink() {
            inventory.warnings.push(format!(
                "Plugin root is not a regular directory: {}",
                configured_root.display()
            ));
            continue;
        }
        let root = match configured_root.canonicalize() {
            Ok(value) => value,
            Err(error) => {
                inventory.warnings.push(format!(
                    "Could not resolve plugin root {}: {error}",
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
            if inventory.plugins.len() >= MAX_PLUGINS || inventory.scanned_entries >= max_entries {
                inventory.truncated = true;
                break;
            }

            inspect_plugin_directory(&root, &directory, &mut inventory, &mut seen_plugins);

            let entries = match fs::read_dir(&directory) {
                Ok(value) => value,
                Err(error) => {
                    inventory.warnings.push(format!(
                        "Could not read plugin directory {}: {error}",
                        directory.display()
                    ));
                    continue;
                }
            };
            let mut children = Vec::new();
            for entry in entries {
                if inventory.scanned_entries >= max_entries {
                    inventory.truncated = true;
                    break;
                }
                inventory.scanned_entries += 1;
                let entry = match entry {
                    Ok(value) => value,
                    Err(error) => {
                        inventory
                            .warnings
                            .push(format!("Could not read plugin directory entry: {error}"));
                        continue;
                    }
                };
                let kind = match entry.file_type() {
                    Ok(value) => value,
                    Err(error) => {
                        inventory
                            .warnings
                            .push(format!("Could not inspect plugin entry: {error}"));
                        continue;
                    }
                };
                if kind.is_dir() && !kind.is_symlink() && depth < max_depth {
                    children.push(entry.path());
                }
            }
            children.sort();
            for child in children.into_iter().rev() {
                pending.push((child, depth + 1));
            }
        }
    }

    inventory.plugins.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.manifest_path.cmp(&right.manifest_path))
    });
    inventory
}

fn inspect_plugin_directory(
    configured_root: &Path,
    directory: &Path,
    inventory: &mut PluginInventory,
    seen_plugins: &mut BTreeSet<String>,
) {
    for (relative, ecosystem) in [
        (PathBuf::from(".cursor-plugin/plugin.json"), "cursor"),
        (PathBuf::from(".claude-plugin/plugin.json"), "claude"),
        (PathBuf::from("plugin.json"), "generic"),
    ] {
        if inventory.plugins.len() >= MAX_PLUGINS {
            inventory.truncated = true;
            return;
        }
        let manifest_path = directory.join(relative);
        let metadata = match fs::symlink_metadata(&manifest_path) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.len() > MAX_MANIFEST_BYTES {
            inventory.warnings.push(format!(
                "Plugin manifest exceeds the 128 KiB limit and was skipped: {}",
                manifest_path.display()
            ));
            continue;
        }
        let canonical_manifest = match manifest_path.canonicalize() {
            Ok(value) if value.starts_with(configured_root) => value,
            _ => {
                inventory.warnings.push(format!(
                    "Plugin manifest escapes its configured root and was skipped: {}",
                    manifest_path.display()
                ));
                continue;
            }
        };
        let canonical_plugin_root = match directory.canonicalize() {
            Ok(value) if value.starts_with(configured_root) => value,
            _ => continue,
        };
        let bytes = match fs::read(&canonical_manifest) {
            Ok(value) if value.len() as u64 <= MAX_MANIFEST_BYTES => value,
            Ok(_) => continue,
            Err(error) => {
                inventory.warnings.push(format!(
                    "Could not read plugin manifest {}: {error}",
                    manifest_path.display()
                ));
                continue;
            }
        };
        let manifest: Value = match serde_json::from_slice::<Value>(&bytes) {
            Ok(value) if value.is_object() => value,
            _ => {
                inventory.warnings.push(format!(
                    "Plugin manifest is invalid JSON or not an object: {}",
                    manifest_path.display()
                ));
                continue;
            }
        };
        let Some(name) = manifest
            .get("name")
            .and_then(Value::as_str)
            .filter(|value| valid_plugin_id(value))
        else {
            inventory.warnings.push(format!(
                "Plugin manifest has no valid name: {}",
                manifest_path.display()
            ));
            continue;
        };
        let key = format!(
            "{}:{}:{}",
            configured_root.display(),
            canonical_plugin_root.display(),
            name
        );
        if !seen_plugins.insert(key) {
            continue;
        }
        let display_name = manifest
            .get("displayName")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty() && value.len() <= 200)
            .unwrap_or(name);
        let relative_manifest = canonical_manifest
            .strip_prefix(configured_root)
            .map(|value| value.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        inventory.plugins.push(LocalPluginDescriptor {
            id: format!(
                "plugin:{}:{}",
                name.to_ascii_lowercase(),
                canonical_plugin_root.display()
            ),
            name: name.to_string(),
            display_name: display_name.to_string(),
            version: bounded_string(&manifest, "version", 80),
            description: bounded_string(&manifest, "description", 2_000),
            license: bounded_string(&manifest, "license", 120),
            homepage: bounded_string(&manifest, "homepage", 2_000),
            repository: bounded_string(&manifest, "repository", 2_000),
            ecosystem: ecosystem.to_string(),
            manifest_path: relative_manifest,
            plugin_root: canonical_plugin_root.display().to_string(),
            manifest_bytes: bytes.len() as u64,
            components: PluginComponentCounts {
                agents: component_count(&manifest, "agents"),
                skills: component_count(&manifest, "skills"),
                commands: component_count(&manifest, "commands"),
                hooks: component_count(&manifest, "hooks"),
                mcp_servers: component_count(&manifest, "mcpServers"),
                tools: component_count(&manifest, "tools"),
            },
            status: "disabled".into(),
            trust: "untrusted".into(),
            execution: "never_during_discovery".into(),
        });
    }
}

fn valid_plugin_id(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 160
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
}

fn bounded_string(manifest: &Value, key: &str, max: usize) -> Option<String> {
    manifest
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty() && value.len() <= max)
        .map(ToString::to_string)
}

fn component_count(manifest: &Value, key: &str) -> usize {
    match manifest.get(key) {
        Some(Value::Array(items)) => items.len().min(10_000),
        Some(Value::Object(items)) => items.len().min(10_000),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_directory(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = env::temp_dir().join(format!("dread-plugins-{label}-{nonce}"));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn discovers_metadata_without_enabling_or_executing_plugin() {
        let root = temporary_directory("metadata");
        let plugin = root.join("sample");
        let manifest_dir = plugin.join(".claude-plugin");
        fs::create_dir_all(&manifest_dir).unwrap();
        fs::write(
            manifest_dir.join("plugin.json"),
            r#"{"name":"sample-plugin","displayName":"Sample Plugin","version":"1.2.3","skills":["a"],"agents":["b"],"mcpServers":{"demo":{}},"hooks":{"run":"evil.sh"}}"#,
        ).unwrap();

        let inventory = discover_local_plugins_in(std::slice::from_ref(&root), 5, 100);
        assert_eq!(inventory.plugins.len(), 1);
        let item = &inventory.plugins[0];
        assert_eq!(item.ecosystem, "claude");
        assert_eq!(item.components.skills, 1);
        assert_eq!(item.components.mcp_servers, 1);
        assert_eq!(item.status, "disabled");
        assert_eq!(item.trust, "untrusted");
        assert_eq!(item.execution, "never_during_discovery");

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn direct_plugin_root_is_discovered() {
        let root = temporary_directory("direct");
        fs::create_dir_all(root.join(".cursor-plugin")).unwrap();
        fs::write(
            root.join(".cursor-plugin/plugin.json"),
            r#"{"name":"direct-plugin"}"#,
        )
        .unwrap();
        let inventory = discover_local_plugins_in(std::slice::from_ref(&root), 3, 100);
        assert_eq!(inventory.plugins.len(), 1);
        assert_eq!(inventory.plugins[0].ecosystem, "cursor");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn malformed_manifest_is_skipped_with_warning() {
        let root = temporary_directory("invalid");
        fs::create_dir_all(root.join(".claude-plugin")).unwrap();
        fs::write(root.join(".claude-plugin/plugin.json"), "{").unwrap();
        let inventory = discover_local_plugins_in(std::slice::from_ref(&root), 3, 100);
        assert!(inventory.plugins.is_empty());
        assert!(!inventory.warnings.is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn does_not_follow_symlinked_manifest() {
        use std::os::unix::fs::symlink;
        let root = temporary_directory("symlink");
        let outside = temporary_directory("outside");
        fs::create_dir_all(root.join(".claude-plugin")).unwrap();
        fs::write(outside.join("plugin.json"), r#"{"name":"linked-plugin"}"#).unwrap();
        symlink(
            outside.join("plugin.json"),
            root.join(".claude-plugin/plugin.json"),
        )
        .unwrap();
        let inventory = discover_local_plugins_in(std::slice::from_ref(&root), 3, 100);
        assert!(inventory.plugins.is_empty());
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }
}
