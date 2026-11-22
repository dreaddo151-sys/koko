//! Auto-configuration for locally discovered GGUF models (B2).
//!
//! Turns a [`LocalModel`] from the scanner into a concrete llama-server launch
//! suggestion: quantization label, split-shard awareness, context clamp, and
//! GPU offload sizing. Split shards are detected by the llama.cpp filename
//! convention (`-00001-of-00009.gguf`): only shard 00001 is launchable, the
//! rest are referenced parts of the same logical model.

use crate::models::LocalModel;
use serde::{Deserialize, Serialize};

/// Canonical `general.file_type` enum values, taken from the maintainer's own
/// llama.cpp checkout (`gguf-py/gguf/constants.py`, FileType), not invented.
pub fn quantization_label(code: Option<u32>) -> Option<&'static str> {
    Some(match code? {
        0 => "F32",
        1 => "F16",
        2 => "Q4_0",
        3 => "Q4_1",
        7 => "Q8_0",
        8 => "Q5_0",
        9 => "Q5_1",
        10 => "Q2_K",
        11 => "Q3_K_S",
        12 => "Q3_K_M",
        13 => "Q3_K_L",
        14 => "Q4_K_S",
        15 => "Q4_K_M",
        16 => "Q5_K_S",
        17 => "Q5_K_M",
        18 => "Q6_K",
        19 => "IQ2_XXS",
        20 => "IQ2_XS",
        21 => "Q2_K_S",
        22 => "IQ3_XS",
        23 => "IQ3_XXS",
        24 => "IQ1_S",
        25 => "IQ4_NL",
        26 => "IQ3_S",
        27 => "IQ3_M",
        28 => "IQ2_S",
        29 => "IQ2_M",
        30 => "IQ4_XS",
        31 => "IQ1_M",
        32 => "BF16",
        36 => "TQ1_0",
        37 => "TQ2_0",
        38 => "MXFP4_MOE",
        _ => return None, // removed (33-35) or unknown: honest None
    })
}

/// Detects the llama.cpp split-file convention in a path/filename:
/// returns `(shard_index, total_parts)` for names containing `00001-of-00009`.
pub fn detect_split(path: &str) -> Option<(u32, u32)> {
    let stem = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    let stem = stem
        .strip_suffix(".gguf")
        .or_else(|| stem.strip_suffix(".safetensors"))?;
    let idx = stem.rfind("-of-")?;
    let total: u32 = stem[idx + 4..].parse().ok()?;
    let head = &stem[..idx];
    let dash = head.rfind('-')?;
    let no: u32 = head[dash + 1..].parse().ok()?;
    // Real splits are zero-padded 5-digit pairs and more than one part.
    let no_part = &head[dash + 1..];
    let total_part = &stem[idx + 4..];
    if no_part.len() != 5 || total_part.len() != 5 || total <= 1 || no == 0 || no > total {
        return None;
    }
    Some((no, total))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchConfig {
    pub model_path: String,
    pub bin: String,
    pub args: Vec<String>,
    pub notes: Vec<String>,
}

fn default_bin() -> String {
    std::env::var("DREAD_LLAMA_SERVER")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "dread_llama.cpp/build/bin/llama-server".to_string())
}

/// Suggests a llama-server launch for one model. `vram_gib_per_gpu` empty or
/// total < 14 GiB means CPU-only: no layer offload. Context is clamped to
/// [2048, 16384] regardless of the file's nominal size so one bad GGUF header
/// can never OOM the box; the true value is preserved in notes.
pub fn suggest_launch_config(model: &LocalModel, vram_gib_per_gpu: &[f64]) -> LaunchConfig {
    let mut args = vec![
        "--model".to_string(),
        format!("{}/{}", model.root, model.relative_path),
        "--host".to_string(),
        "127.0.0.1".to_string(),
        "--port".to_string(),
        "0".to_string(),
        "--flash-attn".to_string(),
    ];
    let mut notes = Vec::new();

    let nominal_ctx = model
        .gguf_metadata
        .as_ref()
        .and_then(|meta| meta.context_length);
    let ctx = nominal_ctx.unwrap_or(4096).clamp(2048, 16384);
    if nominal_ctx.is_some_and(|value| value != ctx) {
        notes.push(format!("ctx clamped from nominal {nominal_ctx:?} to {ctx}"));
    }
    args.extend(["--ctx-size".to_string(), ctx.to_string()]);

    if let Some((no, total)) = detect_split(&model.relative_path) {
        notes.push(format!("split shard {no}/{total}"));
        if no != 1 {
            notes.push("not launchable: only shard 00001 is the entry point".into());
        }
    }

    let total_vram: f64 = vram_gib_per_gpu.iter().sum();
    if total_vram >= 14.0 {
        args.extend(["--n-gpu-layers".to_string(), "999".to_string()]);
        args.extend(["--tensor-split".to_string(), gpu_split(vram_gib_per_gpu)]);
    } else {
        args.extend(["--n-gpu-layers".to_string(), "0".to_string()]);
        notes.push("CPU-only: GPU telemetry reported < 14 GiB total VRAM".into());
    }

    if model
        .gguf_metadata
        .as_ref()
        .and_then(|meta| meta.architecture.as_deref())
        .is_some_and(|arch| {
            matches!(
                arch,
                "qwen2" | "qwen3" | "qwen35" | "qwen3moe" | "qwen35moe" | "gemma3"
            )
        })
    {
        args.push("--jinja".to_string());
    }

    LaunchConfig {
        model_path: format!("{}/{}", model.root, model.relative_path),
        bin: default_bin(),
        args,
        notes,
    }
}

/// Proportional tensor split like "1,1" or "3,1" by VRAM share.
fn gpu_split(vram_gib_per_gpu: &[f64]) -> String {
    let max = vram_gib_per_gpu.iter().cloned().fold(0.0f64, f64::max);
    if max <= 0.0 {
        return "1".into();
    }
    vram_gib_per_gpu
        .iter()
        .map(|value| (((value / max) * 8.0).round().max(1.0) as u32).to_string())
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::GgufMetadata;

    fn model(rel: &str, meta: Option<GgufMetadata>) -> LocalModel {
        LocalModel {
            id: "t-1".into(),
            name: "Test Model".into(),
            model_kind: "language".into(),
            format: "gguf".into(),
            root: "/data/models".into(),
            relative_path: rel.into(),
            size_bytes: 1024,
            status: "discovered_not_loaded".into(),
            metadata_status: "parsed".into(),
            metadata_error: None,
            gguf_metadata: meta,
        }
    }

    fn meta(ctx: Option<u64>, arch: Option<&str>) -> GgufMetadata {
        GgufMetadata {
            version: 3,
            tensor_count: 0,
            metadata_count: 0,
            model_name: Some("Test Model".into()),
            architecture: arch.map(str::to_string),
            context_length: ctx,
            file_type_code: Some(15),
            quantization_version: Some(2),
            block_count: None,
            head_count: None,
            head_count_kv: None,
            embedding_length: None,
        }
    }

    #[test]
    fn quantization_label_matches_canonical_enum() {
        assert_eq!(quantization_label(Some(15)), Some("Q4_K_M"));
        assert_eq!(quantization_label(Some(7)), Some("Q8_0"));
        assert_eq!(quantization_label(Some(32)), Some("BF16"));
        assert_eq!(quantization_label(Some(33)), None); // removed value
        assert_eq!(quantization_label(None), None);
    }

    #[test]
    fn split_detection_accepts_and_rejects() {
        assert_eq!(
            detect_split("qwen2.5-72b-q3km/qwen-00001-of-00009.gguf"),
            Some((1, 9))
        );
        assert_eq!(detect_split("dir/model-00004-of-00009.gguf"), Some((4, 9)));
        assert_eq!(detect_split("plain/model.gguf"), None);
        assert_eq!(detect_split("traps/season-1-of-2.gguf"), None); // not 5-digit padded
    }

    #[test]
    fn cpu_only_launch_no_offload_ctx_clamped_up() {
        let cfg = suggest_launch_config(&model("a.gguf", Some(meta(Some(1024), None))), &[]);
        assert!(cfg.args.contains(&"--n-gpu-layers".to_string()));
        let idx = cfg.args.iter().position(|a| a == "--n-gpu-layers").unwrap();
        assert_eq!(cfg.args[idx + 1], "0");
        let cidx = cfg.args.iter().position(|a| a == "--ctx-size").unwrap();
        assert_eq!(cfg.args[cidx + 1], "2048"); // clamped up from 1024
        assert!(cfg.notes.iter().any(|n| n.contains("clamped")));
    }

    #[test]
    fn gpu_launch_offloads_and_jinja_for_qwen3() {
        let cfg = suggest_launch_config(
            &model("q.gguf", Some(meta(Some(131072), Some("qwen3")))),
            &[23.6, 11.0],
        );
        let idx = cfg.args.iter().position(|a| a == "--n-gpu-layers").unwrap();
        assert_eq!(cfg.args[idx + 1], "999");
        assert!(cfg.args.contains(&"--jinja".to_string()));
        let cidx = cfg.args.iter().position(|a| a == "--ctx-size").unwrap();
        assert_eq!(cfg.args[cidx + 1], "16384"); // clamped down from 131072
        let tidx = cfg.args.iter().position(|a| a == "--tensor-split").unwrap();
        assert_eq!(cfg.args[tidx + 1], "8,4");
        assert_eq!(cfg.model_path, "/data/models/q.gguf");
    }

    #[test]
    fn non_first_shard_flagged_not_launchable() {
        let cfg = suggest_launch_config(
            &model("x-00003-of-00009.gguf", Some(meta(Some(8192), None))),
            &[23.6],
        );
        assert!(cfg.notes.iter().any(|n| n.contains("shard 3/9")));
        assert!(cfg.notes.iter().any(|n| n.contains("not launchable")));
    }
}
