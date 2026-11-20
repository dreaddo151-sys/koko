//! Bounded, read-only discovery of local ComfyUI workflow JSON files.
//!
//! Workflow payloads can contain arbitrary user data. This module returns
//! only file metadata and node type names; it never echoes widget values or
//! submits a prompt to ComfyUI.

use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{BTreeSet, VecDeque},
    env, fs,
    path::PathBuf,
};

const MAX_ROOTS: usize = 16;
const MAX_DEPTH: usize = 6;
const MAX_ENTRIES: usize = 4_000;
const MAX_WORKFLOWS: usize = 256;
const MAX_WORKFLOW_BYTES: u64 = 4 * 1024 * 1024;
const MAX_TOTAL_WORKFLOW_BYTES: u64 = 64 * 1024 * 1024;
const MAX_NODES_PER_WORKFLOW: usize = 4_096;
const MAX_NODE_TYPES: usize = 64;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorkflowDescriptor {
    pub id: String,
    pub name: String,
    pub root: String,
    pub relative_path: String,
    pub format: String,
    pub node_count: usize,
    pub node_types: Vec<String>,
    pub truncated_node_types: bool,
    pub file_size_bytes: u64,
    pub status: String,
    pub execution: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorkflowInventory {
    pub roots_scanned: Vec<String>,
    pub scanned_entries: usize,
    pub bytes_read: u64,
    pub truncated: bool,
    pub workflows: Vec<WorkflowDescriptor>,
    pub warnings: Vec<String>,
    pub note: String,
}

pub fn discover_local_workflows() -> WorkflowInventory {
    let roots = configured_roots();
    discover_local_workflows_in(&roots)
}

fn configured_roots() -> Vec<PathBuf> {
    if let Some(value) = env::var_os("DREAD_COMFYUI_WORKFLOW_ROOTS") {
        return env::split_paths(&value).take(MAX_ROOTS + 1).collect();
    }

    let mut roots = Vec::new();
    if let Some(user_root) = env::var_os("DREAD_COMFYUI_USER_ROOT").map(PathBuf::from) {
        roots.push(user_root.join("workflows"));
    }
    for key in ["DREAD_COMFYUI_ROOT", "COMFYUI_HOME"] {
        if let Some(root) = env::var_os(key).map(PathBuf::from) {
            roots.push(root.join("user/default/workflows"));
        }
    }
    if let Some(workspace) = env::var_os("DREAD_WORKSPACE").map(PathBuf::from) {
        roots.push(workspace.join("ComfyUI/user/default/workflows"));
    }
    if let Ok(current) = env::current_dir() {
        roots.push(current.join("ComfyUI/user/default/workflows"));
    }
    if let Some(home) = env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
    {
        roots.push(home.join("ComfyUI/user/default/workflows"));
        roots.push(home.join(".config/ComfyUI/user/default/workflows"));
    }
    if let Some(local_app_data) = env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        roots.push(local_app_data.join("ComfyUI/user/default/workflows"));
    }

    let mut unique = BTreeSet::new();
    roots
        .into_iter()
        .filter(|path| unique.insert(path.clone()))
        .collect()
}

pub fn discover_local_workflows_in(roots: &[PathBuf]) -> WorkflowInventory {
    let mut inventory = WorkflowInventory {
        roots_scanned: Vec::new(),
        scanned_entries: 0,
        bytes_read: 0,
        truncated: roots.len() > MAX_ROOTS,
        workflows: Vec::new(),
        warnings: Vec::new(),
        note: "Workflow metadata only. Discovery does not submit prompts, load models, access credentials, or execute workflow nodes. Workflow files remain untrusted until reviewed.".into(),
    };
    let mut seen_roots = BTreeSet::new();
    let mut seen_files = BTreeSet::new();

    if roots.len() > MAX_ROOTS {
        inventory.warnings.push(format!(
            "Workflow root list exceeded the {MAX_ROOTS}-root limit; additional roots were not scanned"
        ));
    }

    'roots: for configured_root in roots.iter().take(MAX_ROOTS) {
        if inventory.scanned_entries >= MAX_ENTRIES || inventory.workflows.len() >= MAX_WORKFLOWS {
            inventory.truncated = true;
            break;
        }
        let metadata = match fs::symlink_metadata(configured_root) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                inventory.warnings.push(format!(
                    "Workflow root does not exist: {}",
                    configured_root.display()
                ));
                continue;
            }
            Err(error) => {
                inventory.warnings.push(format!(
                    "Could not inspect workflow root {}: {error}",
                    configured_root.display()
                ));
                continue;
            }
        };
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            inventory.warnings.push(format!(
                "Workflow root is not a regular directory: {}",
                configured_root.display()
            ));
            continue;
        }
        let root = match configured_root.canonicalize() {
            Ok(value) => value,
            Err(error) => {
                inventory.warnings.push(format!(
                    "Could not resolve workflow root {}: {error}",
                    configured_root.display()
                ));
                continue;
            }
        };
        if !seen_roots.insert(root.clone()) {
            continue;
        }
        inventory.roots_scanned.push(root.display().to_string());

        let mut pending = VecDeque::from([(root.clone(), 0usize)]);
        while let Some((directory, depth)) = pending.pop_front() {
            if inventory.scanned_entries >= MAX_ENTRIES
                || inventory.workflows.len() >= MAX_WORKFLOWS
            {
                inventory.truncated = true;
                break 'roots;
            }
            let entries = match fs::read_dir(&directory) {
                Ok(value) => value,
                Err(error) => {
                    inventory.warnings.push(format!(
                        "Could not read workflow directory {}: {error}",
                        directory.display()
                    ));
                    continue;
                }
            };
            let mut children = Vec::new();
            for entry in entries {
                if inventory.scanned_entries >= MAX_ENTRIES {
                    inventory.truncated = true;
                    break;
                }
                inventory.scanned_entries += 1;
                let entry = match entry {
                    Ok(value) => value,
                    Err(error) => {
                        inventory
                            .warnings
                            .push(format!("Could not read workflow directory entry: {error}"));
                        continue;
                    }
                };
                let kind = match entry.file_type() {
                    Ok(value) => value,
                    Err(error) => {
                        inventory
                            .warnings
                            .push(format!("Could not inspect workflow entry: {error}"));
                        continue;
                    }
                };
                if kind.is_symlink() {
                    continue;
                }
                if kind.is_dir() {
                    if depth < MAX_DEPTH {
                        children.push(entry.path());
                    }
                    continue;
                }
                if !kind.is_file()
                    || !entry
                        .path()
                        .extension()
                        .and_then(|value| value.to_str())
                        .map(|value| value.eq_ignore_ascii_case("json"))
                        .unwrap_or(false)
                {
                    continue;
                }

                let path = entry.path();
                let file_meta = match fs::symlink_metadata(&path) {
                    Ok(value) if value.is_file() && !value.file_type().is_symlink() => value,
                    _ => continue,
                };
                if file_meta.len() > MAX_WORKFLOW_BYTES {
                    inventory.warnings.push(format!(
                        "Workflow exceeds the 4 MiB limit and was skipped: {}",
                        path.display()
                    ));
                    continue;
                }
                let canonical = match path.canonicalize() {
                    Ok(value) if value.starts_with(&root) => value,
                    _ => {
                        inventory.warnings.push(format!(
                            "Workflow path escaped its configured root and was skipped: {}",
                            path.display()
                        ));
                        continue;
                    }
                };
                if !seen_files.insert(canonical.clone()) {
                    continue;
                }
                if inventory.bytes_read.saturating_add(file_meta.len()) > MAX_TOTAL_WORKFLOW_BYTES {
                    inventory.truncated = true;
                    inventory.warnings.push(
                        "Total workflow metadata read budget (64 MiB) reached; remaining files were not inspected".into(),
                    );
                    break 'roots;
                }
                let bytes = match fs::read(&canonical) {
                    Ok(value) if value.len() as u64 <= MAX_WORKFLOW_BYTES => value,
                    Ok(_) => continue,
                    Err(error) => {
                        inventory.warnings.push(format!(
                            "Could not read workflow {}: {error}",
                            path.display()
                        ));
                        continue;
                    }
                };
                if inventory.bytes_read.saturating_add(bytes.len() as u64)
                    > MAX_TOTAL_WORKFLOW_BYTES
                {
                    inventory.truncated = true;
                    inventory.warnings.push(
                        "Total workflow metadata read budget (64 MiB) reached; remaining files were not inspected".into(),
                    );
                    break 'roots;
                }
                inventory.bytes_read = inventory.bytes_read.saturating_add(bytes.len() as u64);
                let value: Value = match serde_json::from_slice::<Value>(&bytes) {
                    Ok(value) if value.is_object() => value,
                    _ => {
                        inventory.warnings.push(format!(
                            "Workflow is invalid JSON or not an object: {}",
                            path.display()
                        ));
                        continue;
                    }
                };
                let Some((format, node_count, node_types, type_truncated)) =
                    summarize_workflow(&value)
                else {
                    inventory.warnings.push(format!(
                        "JSON file is not a recognized ComfyUI workflow: {}",
                        path.display()
                    ));
                    continue;
                };
                let relative = canonical
                    .strip_prefix(&root)
                    .map(|value| value.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_default();
                let name = path
                    .file_stem()
                    .map(|value| value.to_string_lossy().into_owned())
                    .unwrap_or_else(|| relative.clone());
                inventory.workflows.push(WorkflowDescriptor {
                    id: format!("comfyui:{}:{}", root.display(), relative),
                    name,
                    root: root.display().to_string(),
                    relative_path: relative,
                    format: format.into(),
                    node_count,
                    node_types,
                    truncated_node_types: type_truncated,
                    file_size_bytes: bytes.len() as u64,
                    status: "discovered_not_validated".into(),
                    execution: "never_during_discovery".into(),
                });
            }
            children.sort();
            for child in children {
                pending.push_back((child, depth + 1));
            }
        }
    }

    inventory
        .workflows
        .sort_by(|left, right| left.id.cmp(&right.id));
    inventory
}

fn summarize_workflow(value: &Value) -> Option<(&'static str, usize, Vec<String>, bool)> {
    let mut node_types = BTreeSet::new();
    let mut truncated = false;

    if let Some(nodes) = value.get("nodes").and_then(Value::as_array) {
        let node_count = nodes.len();
        for node in nodes.iter().take(MAX_NODES_PER_WORKFLOW) {
            let node_type = node
                .get("type")
                .or_else(|| node.get("class_type"))
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty());
            if let Some(node_type) = node_type {
                add_node_type(&mut node_types, node_type, &mut truncated);
            }
        }
        truncated |= nodes.len() > MAX_NODES_PER_WORKFLOW;
        return Some((
            "comfyui_graph",
            node_count,
            node_types.into_iter().collect(),
            truncated,
        ));
    }

    let object = value.as_object()?;
    let mut recognized = 0usize;
    for node in object.values().take(MAX_NODES_PER_WORKFLOW) {
        let Some(node_type) = node.get("class_type").and_then(Value::as_str) else {
            continue;
        };
        recognized += 1;
        add_node_type(&mut node_types, node_type, &mut truncated);
    }
    truncated |= object.len() > MAX_NODES_PER_WORKFLOW;
    if recognized == 0 {
        return None;
    }
    Some((
        "comfyui_api_prompt",
        object.len(),
        node_types.into_iter().collect(),
        truncated,
    ))
}

fn add_node_type(types: &mut BTreeSet<String>, raw: &str, truncated: &mut bool) {
    if raw.len() > 160 || raw.trim().is_empty() {
        return;
    }
    if types.len() < MAX_NODE_TYPES || types.contains(raw) {
        types.insert(raw.to_string());
    } else {
        *truncated = true;
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
        let root = env::temp_dir().join(format!("dread-workflows-{label}-{nonce}"));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn summarizes_graph_without_returning_workflow_widget_values() {
        let root = temporary_directory("canvas");
        let workflow = root.join("video.json");
        fs::write(
            &workflow,
            r#"{"last_node_id":2,"nodes":[{"id":1,"type":"CheckpointLoaderSimple"},{"id":2,"type":"VHS_VideoCombine","widgets_values":["private-value"]}]}"#,
        ).unwrap();
        let inventory = discover_local_workflows_in(&[root.clone()]);
        assert_eq!(inventory.workflows.len(), 1);
        let item = &inventory.workflows[0];
        assert_eq!(item.format, "comfyui_graph");
        assert_eq!(item.node_count, 2);
        assert!(item.node_types.contains(&"VHS_VideoCombine".to_string()));
        assert_eq!(item.status, "discovered_not_validated");
        assert!(
            !serde_json::to_string(item)
                .unwrap()
                .contains("private-value")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recognizes_api_prompt_and_skips_unrelated_json() {
        let root = temporary_directory("api-prompt");
        fs::write(
            root.join("api.json"),
            r#"{"1":{"class_type":"KSampler","inputs":{}},"2":{"class_type":"VAELoader","inputs":{}}}"#,
        ).unwrap();
        fs::write(root.join("unrelated.json"), r#"{"hello":"world"}"#).unwrap();
        let inventory = discover_local_workflows_in(&[root.clone()]);
        assert_eq!(inventory.workflows.len(), 1);
        assert_eq!(inventory.workflows[0].format, "comfyui_api_prompt");
        assert_eq!(inventory.workflows[0].node_count, 2);
        assert_eq!(inventory.warnings.len(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn does_not_follow_symlink_workflow_files() {
        use std::os::unix::fs::symlink;
        let root = temporary_directory("symlink");
        let outside = temporary_directory("outside");
        fs::write(
            outside.join("external.json"),
            r#"{"nodes":[{"type":"KSampler"}]}"#,
        )
        .unwrap();
        symlink(outside.join("external.json"), root.join("linked.json")).unwrap();
        let inventory = discover_local_workflows_in(&[root.clone()]);
        assert!(inventory.workflows.is_empty());
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn honors_root_limit_and_reports_truncation() {
        let parent = temporary_directory("many-roots");
        let roots = (0..MAX_ROOTS + 2)
            .map(|index| {
                let path = parent.join(format!("root-{index}"));
                fs::create_dir(&path).unwrap();
                path
            })
            .collect::<Vec<_>>();
        let inventory = discover_local_workflows_in(&roots);
        assert!(inventory.truncated);
        assert_eq!(inventory.roots_scanned.len(), MAX_ROOTS);
        fs::remove_dir_all(parent).unwrap();
    }
}
