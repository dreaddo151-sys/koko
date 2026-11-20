//! Bounded, read-only discovery of model and ComfyUI weight files.
//!
//! Filesystem candidates are never assumed to be loaded or runnable. Model
//! process lifecycle and GPU leases remain exclusively controlled by the
//! authenticated Python GPU Supervisor.

use serde::Serialize;
use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
};

const DEFAULT_MAX_DEPTH: usize = 8;
const DEFAULT_MAX_ENTRIES: usize = 20_000;
const MAX_MODEL_ROOTS: usize = 16;
const MAX_MODELS: usize = 4_096;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LocalModel {
    pub id: String,
    pub name: String,
    pub model_kind: String,
    pub format: String,
    pub root: String,
    pub relative_path: String,
    pub size_bytes: u64,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LocalModelInventory {
    pub roots_scanned: Vec<String>,
    pub scanned_entries: usize,
    pub truncated: bool,
    pub models: Vec<LocalModel>,
    pub warnings: Vec<String>,
}

pub fn discover_local_models() -> LocalModelInventory {
    discover_local_models_in(
        &configured_model_roots(),
        DEFAULT_MAX_DEPTH,
        DEFAULT_MAX_ENTRIES,
    )
}

fn configured_model_roots() -> Vec<PathBuf> {
    if let Some(value) = env::var_os("DREAD_MODEL_ROOTS") {
        return env::split_paths(&value)
            .filter(|path| !path.as_os_str().is_empty())
            .take(MAX_MODEL_ROOTS + 1)
            .collect();
    }

    let workspace = env::var_os("DREAD_WORKSPACE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/mnt/sabrent/dread_workspace"));
    let mut roots = vec![workspace.join("ComfyUI/models"), workspace.join("models")];
    if let Some(home) = env::var_os("HOME").map(PathBuf::from) {
        roots.push(home.join("models"));
        roots.push(home.join(".cache/huggingface/hub"));
    }
    roots
}

pub fn discover_local_models_in(
    roots: &[PathBuf],
    max_depth: usize,
    max_entries: usize,
) -> LocalModelInventory {
    let mut inventory = LocalModelInventory {
        roots_scanned: Vec::new(),
        scanned_entries: 0,
        truncated: roots.len() > MAX_MODEL_ROOTS,
        models: Vec::new(),
        warnings: Vec::new(),
    };
    let mut canonical_roots = BTreeSet::new();
    if roots.len() > MAX_MODEL_ROOTS {
        inventory.warnings.push(format!(
            "Model root list exceeded the {MAX_MODEL_ROOTS}-root limit; additional roots were not scanned"
        ));
    }

    'roots: for configured_root in roots.iter().take(MAX_MODEL_ROOTS) {
        if inventory.scanned_entries >= max_entries || inventory.models.len() >= MAX_MODELS {
            inventory.truncated = true;
            break;
        }
        let root_meta = match fs::symlink_metadata(configured_root) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                inventory.warnings.push(format!(
                    "Model root does not exist: {}",
                    configured_root.display()
                ));
                continue;
            }
            Err(error) => {
                inventory.warnings.push(format!(
                    "Could not inspect model root {}: {error}",
                    configured_root.display()
                ));
                continue;
            }
        };
        if !root_meta.is_dir() || root_meta.file_type().is_symlink() {
            inventory.warnings.push(format!(
                "Model root is not a regular directory: {}",
                configured_root.display()
            ));
            continue;
        }
        let root = match configured_root.canonicalize() {
            Ok(value) => value,
            Err(error) => {
                inventory.warnings.push(format!(
                    "Could not resolve model root {}: {error}",
                    configured_root.display()
                ));
                continue;
            }
        };
        if !canonical_roots.insert(root.clone()) {
            continue;
        }
        inventory.roots_scanned.push(root.display().to_string());

        let mut pending = vec![(root.clone(), 0usize)];
        while let Some((directory, depth)) = pending.pop() {
            if inventory.scanned_entries >= max_entries || inventory.models.len() >= MAX_MODELS {
                inventory.truncated = true;
                break 'roots;
            }
            let entries = match fs::read_dir(&directory) {
                Ok(value) => value,
                Err(error) => {
                    inventory.warnings.push(format!(
                        "Could not read model directory {}: {error}",
                        directory.display()
                    ));
                    continue;
                }
            };
            let mut paths = Vec::new();
            for entry in entries {
                if inventory.scanned_entries >= max_entries {
                    inventory.truncated = true;
                    break;
                }
                inventory.scanned_entries += 1;
                match entry {
                    Ok(entry) => paths.push(entry.path()),
                    Err(error) => inventory
                        .warnings
                        .push(format!("Could not read a model directory entry: {error}")),
                }
            }
            paths.sort();

            for path in paths.into_iter().rev() {
                let metadata = match fs::symlink_metadata(&path) {
                    Ok(value) => value,
                    Err(_) => continue,
                };
                if metadata.file_type().is_symlink() {
                    continue;
                }
                if metadata.is_dir() {
                    if depth < max_depth {
                        pending.push((path, depth + 1));
                    }
                    continue;
                }
                if !metadata.is_file() {
                    continue;
                }
                let Some((format, fallback_kind)) = classify_file(&path) else {
                    continue;
                };
                let canonical_file = match path.canonicalize() {
                    Ok(value) if value.starts_with(&root) => value,
                    _ => continue,
                };
                let Ok(relative) = canonical_file.strip_prefix(&root) else {
                    continue;
                };
                let relative_path = relative.to_string_lossy().replace('\\', "/");
                let name = path
                    .file_name()
                    .map(|value| value.to_string_lossy().into_owned())
                    .unwrap_or_else(|| relative_path.clone());
                inventory.models.push(LocalModel {
                    id: format!("local:{}:{relative_path}", root.display()),
                    name,
                    model_kind: infer_kind_from_path(&relative_path, fallback_kind).into(),
                    format: format.into(),
                    root: root.display().to_string(),
                    relative_path,
                    size_bytes: metadata.len(),
                    status: "discovered_not_loaded".into(),
                });
                if inventory.models.len() >= MAX_MODELS {
                    inventory.truncated = true;
                    break 'roots;
                }
            }
        }
    }

    inventory
        .models
        .sort_by(|left, right| left.id.cmp(&right.id));
    inventory.models.dedup_by(|left, right| left.id == right.id);
    inventory
}

fn classify_file(path: &Path) -> Option<(&'static str, &'static str)> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "gguf" => Some(("gguf", "inference_model")),
        "ggml" => Some(("ggml", "inference_model")),
        "safetensors" => Some(("safetensors", "model_weight")),
        "ckpt" => Some(("ckpt", "model_weight")),
        "pt" => Some(("pt", "model_weight")),
        "pth" => Some(("pth", "model_weight")),
        "onnx" => Some(("onnx", "model_weight")),
        "bin" => Some(("bin", "model_weight")),
        _ => None,
    }
}

fn infer_kind_from_path(path: &str, fallback: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    if lower.contains("/text_encoders/") || lower.contains("/clip/") {
        "text_encoder"
    } else if lower.contains("/vae/") {
        "vae"
    } else if lower.contains("/loras/") || lower.contains("/lora/") {
        "lora"
    } else if lower.contains("/controlnet/") || lower.contains("/control_net/") {
        "controlnet"
    } else if lower.contains("/upscale_models/") || lower.contains("/upscalers/") {
        "upscaler"
    } else if lower.contains("/diffusion_models/")
        || lower.contains("/checkpoints/")
        || lower.contains("/unet/")
    {
        "diffusion_model"
    } else {
        fallback
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
        let root = env::temp_dir().join(format!("dread-models-{label}-{nonce}"));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn finds_weight_files_and_never_claims_they_are_loaded() {
        let root = temporary_directory("inventory");
        let vae = root.join("vae");
        let diffusion = root.join("diffusion_models");
        fs::create_dir_all(&vae).unwrap();
        fs::create_dir_all(&diffusion).unwrap();
        fs::write(vae.join("video.safetensors"), b"test").unwrap();
        fs::write(diffusion.join("model.gguf"), b"test").unwrap();
        fs::write(root.join("notes.txt"), b"not a model").unwrap();

        let inventory = discover_local_models_in(&[root.clone()], 4, 100);
        assert_eq!(inventory.models.len(), 2);
        assert!(
            inventory
                .models
                .iter()
                .all(|item| item.status == "discovered_not_loaded")
        );
        assert!(inventory.models.iter().any(|item| item.model_kind == "vae"));
        assert!(
            inventory
                .models
                .iter()
                .any(|item| item.model_kind == "diffusion_model")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn does_not_follow_symlinked_model_files() {
        use std::os::unix::fs::symlink;
        let root = temporary_directory("symlink");
        let outside = temporary_directory("outside");
        fs::write(outside.join("external.gguf"), b"model").unwrap();
        symlink(outside.join("external.gguf"), root.join("linked.gguf")).unwrap();
        let inventory = discover_local_models_in(&[root.clone()], 2, 100);
        assert!(inventory.models.is_empty());
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn scan_entry_budget_is_enforced_and_reported() {
        let root = temporary_directory("budget");
        for index in 0..10 {
            fs::write(root.join(format!("{index}.gguf")), b"x").unwrap();
        }
        let inventory = discover_local_models_in(&[root.clone()], 2, 3);
        assert!(inventory.truncated);
        assert!(inventory.scanned_entries <= 3);
        fs::remove_dir_all(root).unwrap();
    }
}
