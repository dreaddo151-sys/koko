//! Bounded, read-only discovery of model and ComfyUI weight files.
//!
//! Filesystem candidates are never assumed to be loaded or runnable. Model
//! process lifecycle and GPU leases remain exclusively controlled by the
//! authenticated Python GPU Supervisor.

use serde::Serialize;
use std::{
    collections::BTreeSet,
    env, fs,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

const DEFAULT_MAX_DEPTH: usize = 8;
const DEFAULT_MAX_ENTRIES: usize = 20_000;
const MAX_MODEL_ROOTS: usize = 16;
const MAX_MODELS: usize = 4_096;
const MAX_GGUF_FILES_PER_SCAN: usize = 64;
const MAX_GGUF_HEADER_BYTES: u64 = 1024 * 1024;
const MAX_GGUF_METADATA_ENTRIES: u64 = 16_384;
const MAX_GGUF_ARRAY_ELEMENTS: u64 = 1_000_000;
const MAX_GGUF_KEY_BYTES: u64 = 4 * 1024;
const MAX_GGUF_STRING_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GgufMetadata {
    pub version: u32,
    pub tensor_count: u64,
    pub metadata_count: u64,
    pub model_name: Option<String>,
    pub architecture: Option<String>,
    pub context_length: Option<u64>,
    /// Raw GGUF general.file_type enum value; not translated into a quantization label.
    pub file_type_code: Option<u32>,
    pub quantization_version: Option<u32>,
}

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
    pub metadata_status: String,
    pub metadata_error: Option<String>,
    pub gguf_metadata: Option<GgufMetadata>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LocalModelInventory {
    pub roots_scanned: Vec<String>,
    pub scanned_entries: usize,
    pub truncated: bool,
    pub models: Vec<LocalModel>,
    pub warnings: Vec<String>,
}

/// A forward-only, bounded reader for the metadata section of a GGUF file.
/// The tensor records and weight payload are intentionally never read.
struct GgufHeaderReader {
    file: File,
    file_size: u64,
}

impl GgufHeaderReader {
    fn open(path: &Path) -> Result<Self, String> {
        let file =
            File::open(path).map_err(|error| format!("Could not open GGUF file: {error}"))?;
        let file_size = file
            .metadata()
            .map_err(|error| format!("Could not stat GGUF file: {error}"))?
            .len();
        Ok(Self { file, file_size })
    }

    fn position(&mut self) -> Result<u64, String> {
        self.file
            .stream_position()
            .map_err(|error| format!("Could not read GGUF header position: {error}"))
    }

    fn ensure_span(&mut self, length: u64) -> Result<u64, String> {
        let start = self.position()?;
        let end = start
            .checked_add(length)
            .ok_or_else(|| "GGUF header offset overflow".to_string())?;
        if end > self.file_size {
            return Err("GGUF header is truncated".into());
        }
        if end > MAX_GGUF_HEADER_BYTES {
            return Err("GGUF metadata scan exceeded the 1 MiB header limit".into());
        }
        Ok(end)
    }

    fn read_u32(&mut self) -> Result<u32, String> {
        self.ensure_span(4)?;
        let mut bytes = [0u8; 4];
        self.file
            .read_exact(&mut bytes)
            .map_err(|error| format!("Could not read GGUF u32: {error}"))?;
        Ok(u32::from_le_bytes(bytes))
    }

    fn read_u64(&mut self) -> Result<u64, String> {
        self.ensure_span(8)?;
        let mut bytes = [0u8; 8];
        self.file
            .read_exact(&mut bytes)
            .map_err(|error| format!("Could not read GGUF u64: {error}"))?;
        Ok(u64::from_le_bytes(bytes))
    }

    fn read_bytes(&mut self, length: u64) -> Result<Vec<u8>, String> {
        if length > MAX_GGUF_STRING_BYTES {
            return Err("GGUF string exceeds the 64 KiB value limit".into());
        }
        self.ensure_span(length)?;
        let length = usize::try_from(length)
            .map_err(|_| "GGUF string length is not representable".to_string())?;
        let mut bytes = vec![0u8; length];
        self.file
            .read_exact(&mut bytes)
            .map_err(|error| format!("Could not read GGUF string: {error}"))?;
        Ok(bytes)
    }

    fn read_string(&mut self, max_length: u64) -> Result<Option<String>, String> {
        let length = self.read_u64()?;
        if length > max_length {
            self.skip_bytes(length)?;
            return Ok(None);
        }
        let bytes = self.read_bytes(length)?;
        String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| "GGUF string contains invalid UTF-8".to_string())
    }

    fn skip_bytes(&mut self, length: u64) -> Result<(), String> {
        let end = self.ensure_span(length)?;
        self.file
            .seek(SeekFrom::Start(end))
            .map_err(|error| format!("Could not skip GGUF metadata value: {error}"))?;
        Ok(())
    }

    fn skip_string(&mut self) -> Result<(), String> {
        let length = self.read_u64()?;
        self.skip_bytes(length)
    }

    fn read_nonnegative_integer(&mut self, value_type: u32) -> Result<Option<u64>, String> {
        match value_type {
            0 => {
                self.ensure_span(1)?;
                let mut value = [0u8; 1];
                self.file
                    .read_exact(&mut value)
                    .map_err(|error| error.to_string())?;
                Ok(Some(u64::from(value[0])))
            }
            1 => {
                self.ensure_span(1)?;
                let mut value = [0u8; 1];
                self.file
                    .read_exact(&mut value)
                    .map_err(|error| error.to_string())?;
                Ok(i8::from_le_bytes(value).try_into().ok())
            }
            2 => {
                self.ensure_span(2)?;
                let mut value = [0u8; 2];
                self.file
                    .read_exact(&mut value)
                    .map_err(|error| error.to_string())?;
                Ok(Some(u64::from(u16::from_le_bytes(value))))
            }
            3 => {
                self.ensure_span(2)?;
                let mut value = [0u8; 2];
                self.file
                    .read_exact(&mut value)
                    .map_err(|error| error.to_string())?;
                Ok(i16::from_le_bytes(value).try_into().ok())
            }
            4 => Ok(Some(u64::from(self.read_u32()?))),
            5 => {
                let value = i32::from_le_bytes(self.read_u32()?.to_le_bytes());
                Ok(value.try_into().ok())
            }
            10 => Ok(Some(self.read_u64()?)),
            11 => {
                let value = i64::from_le_bytes(self.read_u64()?.to_le_bytes());
                Ok(value.try_into().ok())
            }
            _ => {
                self.skip_value(value_type)?;
                Ok(None)
            }
        }
    }

    fn skip_value(&mut self, value_type: u32) -> Result<(), String> {
        let width = match value_type {
            0 | 1 | 7 => 1u64,
            2 | 3 => 2,
            4..=6 => 4,
            8 => {
                self.skip_string()?;
                return Ok(());
            }
            9 => {
                let element_type = self.read_u32()?;
                let count = self.read_u64()?;
                if count > MAX_GGUF_ARRAY_ELEMENTS {
                    return Err("GGUF array exceeds the 1,000,000-element limit".into());
                }
                if element_type == 8 {
                    for _ in 0..count {
                        self.skip_string()?;
                    }
                    return Ok(());
                }
                let element_width = match element_type {
                    0 | 1 | 7 => 1,
                    2 | 3 => 2,
                    4..=6 => 4,
                    10..=12 => 8,
                    _ => {
                        return Err(format!(
                            "Unsupported GGUF array element type {element_type}"
                        ));
                    }
                };
                let bytes = count
                    .checked_mul(element_width)
                    .ok_or_else(|| "GGUF array byte length overflow".to_string())?;
                self.skip_bytes(bytes)?;
                return Ok(());
            }
            10..=12 => 8,
            _ => return Err(format!("Unsupported GGUF metadata value type {value_type}")),
        };
        self.skip_bytes(width)
    }
}

/// Parse only the bounded GGUF header and selected scalar metadata; never load tensors.
fn read_gguf_metadata(path: &Path) -> Result<GgufMetadata, String> {
    let mut reader = GgufHeaderReader::open(path)?;
    let mut magic = [0u8; 4];
    reader
        .file
        .read_exact(&mut magic)
        .map_err(|error| format!("Could not read GGUF magic: {error}"))?;
    if &magic != b"GGUF" {
        return Err("File does not have GGUF magic bytes".into());
    }

    let version = reader.read_u32()?;
    if !(2..=3).contains(&version) {
        return Err(format!("Unsupported GGUF version {version}"));
    }
    let tensor_count = reader.read_u64()?;
    let metadata_count = reader.read_u64()?;
    if tensor_count > 1_000_000 || metadata_count > MAX_GGUF_METADATA_ENTRIES {
        return Err("GGUF tensor or metadata count exceeds the inventory safety limit".into());
    }

    let mut metadata = GgufMetadata {
        version,
        tensor_count,
        metadata_count,
        model_name: None,
        architecture: None,
        context_length: None,
        file_type_code: None,
        quantization_version: None,
    };

    for _ in 0..metadata_count {
        let key = reader
            .read_string(MAX_GGUF_KEY_BYTES)?
            .ok_or_else(|| "GGUF metadata key exceeds the 4 KiB limit".to_string())?;
        let value_type = reader.read_u32()?;
        match key.as_str() {
            "general.architecture" if value_type == 8 => {
                metadata.architecture = reader.read_string(MAX_GGUF_STRING_BYTES)?;
            }
            "general.name" if value_type == 8 => {
                metadata.model_name = reader.read_string(MAX_GGUF_STRING_BYTES)?;
            }
            "general.file_type" => {
                metadata.file_type_code = reader
                    .read_nonnegative_integer(value_type)?
                    .and_then(|value| u32::try_from(value).ok());
            }
            "general.quantization_version" => {
                metadata.quantization_version = reader
                    .read_nonnegative_integer(value_type)?
                    .and_then(|value| u32::try_from(value).ok());
            }
            key if key.ends_with(".context_length") => {
                metadata.context_length = reader.read_nonnegative_integer(value_type)?;
            }
            _ => reader.skip_value(value_type)?,
        }

        // These fields are enough for a useful inventory record. Stop before
        // large tokenizer arrays, which can follow the architecture metadata.
        if metadata.architecture.is_some()
            && metadata.model_name.is_some()
            && metadata.context_length.is_some()
        {
            break;
        }
    }

    Ok(metadata)
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

    let mut roots = Vec::new();
    for key in ["DREAD_COMFYUI_ROOT", "COMFYUI_HOME"] {
        if let Some(path) = env::var_os(key).map(PathBuf::from) {
            roots.push(path.join("models"));
        }
    }
    if let Some(workspace) = env::var_os("DREAD_WORKSPACE").map(PathBuf::from) {
        roots.push(workspace.join("ComfyUI/models"));
        roots.push(workspace.join("models"));
    }
    if let Ok(current) = env::current_dir() {
        roots.push(current.join("ComfyUI/models"));
        roots.push(current.join("models"));
    }
    if let Some(home) = env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
    {
        roots.push(home.join("ComfyUI/models"));
        roots.push(home.join("models"));
        roots.push(home.join(".cache/huggingface/hub"));
    }
    if let Some(data_home) = env::var_os("XDG_DATA_HOME").map(PathBuf::from) {
        roots.push(data_home.join("ComfyUI/models"));
        roots.push(data_home.join("huggingface/hub"));
    }
    if let Some(local_app_data) = env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        roots.push(local_app_data.join("ComfyUI/models"));
        roots.push(local_app_data.join("huggingface/hub"));
    }

    let mut unique = BTreeSet::new();
    roots
        .into_iter()
        .filter(|path| unique.insert(path.clone()))
        .collect()
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
    let mut gguf_files_scanned = 0usize;
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
        let mut seen_directories = BTreeSet::from([root.clone()]);
        let mut seen_files = BTreeSet::new();
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
                let canonical_path = match path.canonicalize() {
                    Ok(value) if value.starts_with(&root) => value,
                    // Reject broken links, loops and any target outside the configured root.
                    _ => continue,
                };
                let metadata = match fs::metadata(&canonical_path) {
                    Ok(metadata) => metadata,
                    Err(_) => continue,
                };
                if metadata.is_dir() {
                    if depth < max_depth && seen_directories.insert(canonical_path.clone()) {
                        pending.push((canonical_path, depth + 1));
                    }
                    continue;
                }
                if !metadata.is_file() {
                    continue;
                }
                let Some((format, fallback_kind)) = classify_file(&path) else {
                    continue;
                };
                if !seen_files.insert(canonical_path.clone()) {
                    continue;
                }
                let relative = match path.strip_prefix(&root) {
                    Ok(value) => value,
                    Err(_) => continue,
                };
                let relative_path = relative.to_string_lossy().replace('\\', "/");
                let name = path
                    .file_name()
                    .map(|value| value.to_string_lossy().into_owned())
                    .unwrap_or_else(|| relative_path.clone());
                let (metadata_status, gguf_metadata, metadata_error) = if format == "gguf" {
                    if gguf_files_scanned >= MAX_GGUF_FILES_PER_SCAN {
                        (
                            "scan_limit".to_string(),
                            None,
                            Some("Per-scan GGUF metadata limit reached".to_string()),
                        )
                    } else {
                        gguf_files_scanned += 1;
                        match read_gguf_metadata(&canonical_path) {
                            Ok(metadata) => ("parsed".to_string(), Some(metadata), None),
                            Err(error) => ("unavailable".to_string(), None, Some(error)),
                        }
                    }
                } else {
                    ("not_applicable".to_string(), None, None)
                };
                inventory.models.push(LocalModel {
                    id: format!("local:{}:{relative_path}", root.display()),
                    name,
                    model_kind: infer_kind_from_path(&relative_path, fallback_kind).into(),
                    format: format.into(),
                    root: root.display().to_string(),
                    relative_path,
                    size_bytes: metadata.len(),
                    status: "discovered_not_loaded".into(),
                    metadata_status,
                    metadata_error,
                    gguf_metadata,
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

fn infer_kind_from_path<'a>(path: &str, fallback: &'a str) -> &'a str {
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

    fn append_u32(buffer: &mut Vec<u8>, value: u32) {
        buffer.extend_from_slice(&value.to_le_bytes());
    }

    fn append_u64(buffer: &mut Vec<u8>, value: u64) {
        buffer.extend_from_slice(&value.to_le_bytes());
    }

    fn append_gguf_string(buffer: &mut Vec<u8>, value: &str) {
        append_u64(buffer, value.len() as u64);
        buffer.extend_from_slice(value.as_bytes());
    }

    fn synthetic_gguf() -> Vec<u8> {
        let mut bytes = b"GGUF".to_vec();
        append_u32(&mut bytes, 3);
        append_u64(&mut bytes, 12_345);
        append_u64(&mut bytes, 6);

        // A tokenizer string array appears before some general model fields.
        append_gguf_string(&mut bytes, "tokenizer.ggml.tokens");
        append_u32(&mut bytes, 9); // ARRAY
        append_u32(&mut bytes, 8); // STRING element type
        append_u64(&mut bytes, 3);
        append_gguf_string(&mut bytes, "alpha");
        append_gguf_string(&mut bytes, "beta");
        append_gguf_string(&mut bytes, "gamma");

        append_gguf_string(&mut bytes, "general.architecture");
        append_u32(&mut bytes, 8); // STRING
        append_gguf_string(&mut bytes, "llama");

        append_gguf_string(&mut bytes, "general.name");
        append_u32(&mut bytes, 8); // STRING
        append_gguf_string(&mut bytes, "DREAD test model");

        append_gguf_string(&mut bytes, "general.file_type");
        append_u32(&mut bytes, 4); // UINT32
        append_u32(&mut bytes, 2);

        append_gguf_string(&mut bytes, "general.quantization_version");
        append_u32(&mut bytes, 4); // UINT32
        append_u32(&mut bytes, 2);

        append_gguf_string(&mut bytes, "llama.context_length");
        append_u32(&mut bytes, 4); // UINT32
        append_u32(&mut bytes, 8_192);
        bytes
    }

    #[test]
    fn parses_gguf_metadata_header_and_skips_string_arrays_without_loading_weights() {
        let root = temporary_directory("gguf-header");
        let path = root.join("fixture.gguf");
        fs::write(&path, synthetic_gguf()).unwrap();

        let metadata = read_gguf_metadata(&path).unwrap();
        assert_eq!(metadata.version, 3);
        assert_eq!(metadata.tensor_count, 12_345);
        assert_eq!(metadata.metadata_count, 6);
        assert_eq!(metadata.architecture.as_deref(), Some("llama"));
        assert_eq!(metadata.model_name.as_deref(), Some("DREAD test model"));
        assert_eq!(metadata.context_length, Some(8_192));
        assert_eq!(metadata.file_type_code, Some(2));
        assert_eq!(metadata.quantization_version, Some(2));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_malformed_or_unsupported_gguf_headers() {
        let root = temporary_directory("bad-gguf");
        let bad_magic = root.join("bad-magic.gguf");
        fs::write(&bad_magic, b"NOPE").unwrap();
        assert!(read_gguf_metadata(&bad_magic).is_err());

        let unsupported = root.join("unsupported-version.gguf");
        let mut bytes = b"GGUF".to_vec();
        append_u32(&mut bytes, 99);
        append_u64(&mut bytes, 0);
        append_u64(&mut bytes, 0);
        fs::write(&unsupported, bytes).unwrap();
        assert!(read_gguf_metadata(&unsupported).is_err());

        let truncated = root.join("truncated.gguf");
        let mut bytes = b"GGUF".to_vec();
        append_u32(&mut bytes, 3);
        append_u64(&mut bytes, 1);
        append_u64(&mut bytes, 1);
        append_u64(&mut bytes, 24);
        fs::write(&truncated, bytes).unwrap();
        assert!(read_gguf_metadata(&truncated).is_err());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn local_inventory_includes_parsed_gguf_metadata_without_loading_the_model() {
        let root = temporary_directory("gguf-inventory");
        let model_path = root.join("example.gguf");
        fs::write(&model_path, synthetic_gguf()).unwrap();

        let inventory = discover_local_models_in(std::slice::from_ref(&root), 2, 100);
        assert_eq!(inventory.models.len(), 1);
        let model = &inventory.models[0];
        assert_eq!(model.status, "discovered_not_loaded");
        assert_eq!(model.metadata_status, "parsed");
        let metadata = model.gguf_metadata.as_ref().unwrap();
        assert_eq!(metadata.model_name.as_deref(), Some("DREAD test model"));
        assert_eq!(metadata.architecture.as_deref(), Some("llama"));
        assert_eq!(metadata.context_length, Some(8_192));
        assert!(model.metadata_error.is_none());

        fs::remove_dir_all(root).unwrap();
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

        let inventory = discover_local_models_in(std::slice::from_ref(&root), 4, 100);
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
    fn follows_huggingface_snapshot_symlinks_only_when_targets_stay_inside_root() {
        use std::os::unix::fs::symlink;
        let root = temporary_directory("hf-cache");
        let blobs = root.join("blobs");
        let snapshot = root.join("snapshots").join("revision-abc");
        fs::create_dir_all(&blobs).unwrap();
        fs::create_dir_all(&snapshot).unwrap();
        fs::write(blobs.join("model-blob"), b"weight-data").unwrap();
        symlink(blobs.join("model-blob"), snapshot.join("model.safetensors")).unwrap();

        let inventory = discover_local_models_in(std::slice::from_ref(&root), 6, 100);
        assert_eq!(inventory.models.len(), 1);
        assert_eq!(inventory.models[0].format, "safetensors");
        assert!(
            inventory.models[0]
                .relative_path
                .contains("snapshots/revision-abc")
        );
        assert_eq!(inventory.models[0].status, "discovered_not_loaded");

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
        let inventory = discover_local_models_in(std::slice::from_ref(&root), 2, 100);
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
        let inventory = discover_local_models_in(std::slice::from_ref(&root), 2, 3);
        assert!(inventory.truncated);
        assert!(inventory.scanned_entries <= 3);
        fs::remove_dir_all(root).unwrap();
    }
}
