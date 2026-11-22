//! Token Booster: pure arithmetic that turns a model's GGUF metadata plus a
//! VRAM budget into concrete llama.cpp launch advice.
//!
//! The formulas follow the local-inference ceiling model:
//! - KV cache per token = `block_count × 2 (K+V) × head_count_kv × head_dim × bytes_per_element`
//!   summed over every layer that holds KV;
//! - `head_dim = embedding_length / head_count`;
//! - q8_0 KV blocks 32 elements into 32×1 B + one f32 scale = 36 B, i.e.
//!   1.125 B per element versus 2 B for f16, so q8_0 bytes = f16 bytes × 9 ÷ 8;
//! - per-layer weight bytes are estimated as `file_size / block_count` because
//!   the bounded header reader never walks tensor infos. Every number derived
//!   that way is labelled an estimate — the advice never pretends precision it
//!   does not have.
//!
//! This module spawns nothing and touches no device: it is unit-testable
//! arithmetic, and the hub route feeds it live observations.

use serde::Serialize;

use crate::models::GgufMetadata;

/// llama.cpp compute buffers, CUDA graph, and host slack reserved when
/// reasoning about free VRAM. Deliberately blunt: the advice is labelled.
pub const RUNTIME_OVERHEAD_BYTES: u64 = 1024 * 1024 * 1024;

/// Bytes per element for the default f16 KV cache.
const F16_BYTES_PER_ELEMENT: u64 = 2;
/// q8_0 stores 32 elements + an f32 scale in 36 bytes.
const Q8_0_BLOCK_BYTES: u64 = 36;
const Q8_0_BLOCK_ELEMENTS: u64 = 32;

/// How much of the advice the available metadata can actually support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdviceConfidence {
    /// Every KV math input is present; numbers are exact arithmetic.
    Precise,
    /// Some inputs are missing; the remaining numbers carry stated heuristics.
    Partial,
    /// Too little metadata to say anything trustworthy.
    Unavailable,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TokenBoostAdvice {
    pub confidence: AdviceConfidence,
    pub reason: String,
    /// Bytes the KV cache needs per token of context with an f16 cache.
    pub kv_bytes_per_token_f16: Option<u64>,
    /// Bytes per token with q8_0 KV (`--cache-type-k q8_0 --cache-type-v q8_0`).
    pub kv_bytes_per_token_q8_0: Option<u64>,
    pub head_dim: Option<u64>,
    /// Bytes left for KV after weights and runtime overhead fit in VRAM.
    pub kv_budget_bytes: Option<u64>,
    /// Largest context fully resident in VRAM at q8_0 KV.
    pub max_resident_context_q8_0: Option<u64>,
    /// Same budget spent on an f16 cache.
    pub max_resident_context_f16: Option<u64>,
    /// llama.cpp `--n-gpu-layers` suggestion; `-1` means all layers.
    pub n_gpu_layers: Option<i64>,
    /// True when the suggestion came from the average-layer-size heuristic.
    pub n_gpu_layers_estimated: bool,
    /// Ready-made llama.cpp flags reflecting the advice above.
    pub launch_args: Vec<String>,
}

/// Compute launch advice for one GGUF file.
///
/// * `metadata` — fields from the bounded header reader; missing fields
///   degrade confidence, they never abort.
/// * `file_size_bytes` — on-disk GGUF size (weights ≈ mmap resident size).
/// * `total_vram_bytes` — summed VRAM of the GPUs llama.cpp will use
///   (single-GPU: that card; `--tensor-split` multi-GPU: the sum).
/// * `runtime_overhead_bytes` — compute buffers, graph, and host slack.
pub fn boost_advice(
    metadata: &GgufMetadata,
    file_size_bytes: u64,
    total_vram_bytes: u64,
    runtime_overhead_bytes: u64,
) -> TokenBoostAdvice {
    let mut launch_args = vec![
        "--cache-type-k".to_owned(),
        "q8_0".to_owned(),
        "--cache-type-v".to_owned(),
        "q8_0".to_owned(),
    ];

    let head_dim = match (metadata.embedding_length, metadata.head_count) {
        (Some(emb), Some(heads)) if heads > 0 => Some(emb / heads),
        _ => None,
    };

    let kv_f16 = match (
        metadata.block_count,
        metadata.head_count_kv,
        head_dim,
    ) {
        (Some(layers), Some(kv_heads), Some(dim)) if layers > 0 && kv_heads > 0 && dim > 0 => {
            Some(layers.saturating_mul(2).saturating_mul(kv_heads).saturating_mul(dim).saturating_mul(F16_BYTES_PER_ELEMENT))
        }
        _ => None,
    };
    let kv_q8_0 = kv_f16.map(|bytes| {
        bytes
            .saturating_mul(Q8_0_BLOCK_BYTES)
            .saturating_div(Q8_0_BLOCK_ELEMENTS * F16_BYTES_PER_ELEMENT)
    });

    // Free space for KV once weights and runtime overhead are placed. This is
    // valid whatever else is known, so it survives into Partial advice.
    let kv_budget = total_vram_bytes
        .checked_sub(runtime_overhead_bytes)
        .and_then(|after_runtime| after_runtime.checked_sub(file_size_bytes));

    let (max_ctx_q8_0, max_ctx_f16) = match kv_budget {
        Some(budget) => (
            kv_q8_0.and_then(|per_token| (per_token > 0).then(|| budget / per_token)),
            kv_f16.and_then(|per_token| (per_token > 0).then(|| budget / per_token)),
        ),
        None => (None, None),
    };

    let mut n_gpu_layers = None;
    let mut estimated = false;
    if let (Some(layers), Some(budget)) = (metadata.block_count, kv_budget) {
        // No per-layer size means no offload suggestion. Note budget can
        // only exist if the weights themselves fit, so the only question
        // left is whether the full training-context KV cache also fits.
        let per_layer = (layers > 0)
            .then(|| file_size_bytes.checked_div(layers))
            .flatten()
            .filter(|bytes| *bytes > 0);
        if let Some(per_layer) = per_layer {
            let modest_ctx_tokens: u64 = 4_096;
            let full_context_fits = kv_q8_0
                .zip(metadata.context_length)
                .is_some_and(|(per_token, train_ctx)| {
                    per_token.saturating_mul(train_ctx) <= budget
                });
            if full_context_fits {
                n_gpu_layers = Some(-1);
            } else {
                // Average-layer-size heuristic: transformer layers are close
                // to equal in size, and the advice flags itself estimated.
                // The reservation keeps a modest context resident; when the
                // KV size is unknown there is nothing to reserve.
                let reserve = kv_q8_0
                    .map(|per_token| per_token.saturating_mul(modest_ctx_tokens))
                    .unwrap_or(0);
                let usable = budget.saturating_sub(reserve);
                let fits = usable.checked_div(per_layer).unwrap_or(0);
                n_gpu_layers = Some(i64::try_from(fits.min(layers)).unwrap_or(i64::MAX));
                estimated = true;
            }
        }
    }

    if let Some(layers) = n_gpu_layers {
        launch_args.push("--n-gpu-layers".to_owned());
        launch_args.push(layers.to_string());
    }
    if let Some(ctx) = max_ctx_q8_0 {
        let capped = match metadata.context_length {
            Some(train_ctx) if train_ctx > 0 => ctx.min(train_ctx),
            _ => ctx,
        };
        if capped > 0 {
            launch_args.push("--ctx-size".to_owned());
            launch_args.push(i64::try_from(capped).unwrap_or(i64::MAX).to_string());
        }
    }

    let (confidence, reason) = if kv_f16.is_some() && kv_q8_0.is_some() {
        (AdviceConfidence::Precise, "all KV math inputs parsed from GGUF".to_owned())
    } else if metadata.block_count.is_some() {
        (
            AdviceConfidence::Partial,
            "KV field(s) missing from GGUF header; layer offload is a size heuristic".to_owned(),
        )
    } else {
        (
            AdviceConfidence::Unavailable,
            "no block_count in GGUF header; refusing to guess".to_owned(),
        )
    };

    TokenBoostAdvice {
        confidence,
        reason,
        kv_bytes_per_token_f16: kv_f16,
        kv_bytes_per_token_q8_0: kv_q8_0,
        head_dim,
        kv_budget_bytes: kv_budget,
        max_resident_context_q8_0: max_ctx_q8_0,
        max_resident_context_f16: max_ctx_f16,
        n_gpu_layers,
        n_gpu_layers_estimated: estimated,
        launch_args,
    }
}

/// One-shot helper for the hub: read the bounded GGUF header from disk and
/// compute advice. Blocking; callers must wrap it in `spawn_blocking`.
pub fn advice_for_model_path(
    path: &std::path::Path,
    total_vram_bytes: u64,
    runtime_overhead_bytes: u64,
) -> Result<TokenBoostAdvice, String> {
    let metadata = crate::models::read_gguf_metadata(path)?;
    let size = std::fs::metadata(path)
        .map_err(|error| format!("could not stat {}: {error}", path.display()))?
        .len();
    Ok(boost_advice(&metadata, size, total_vram_bytes, runtime_overhead_bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta_full() -> GgufMetadata {
        GgufMetadata {
            version: 3,
            tensor_count: 0,
            metadata_count: 0,
            model_name: Some("kv math probe".into()),
            architecture: Some("qwen2".into()),
            context_length: Some(32_768),
            file_type_code: Some(15),
            quantization_version: Some(2),
            block_count: Some(80),
            head_count: Some(64),
            head_count_kv: Some(8),
            embedding_length: Some(8_192),
        }
    }

    const GIB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn kv_math_matches_the_ceiling_formula() {
        // 80 layers × 2 × 8 kv heads × 128 dim × 2 B = 327_680 B/token f16.
        let advice = boost_advice(&meta_full(), 40 * GIB, 48 * GIB, GIB);
        assert_eq!(advice.head_dim, Some(128));
        assert_eq!(advice.kv_bytes_per_token_f16, Some(327_680));
        // q8_0 costs 1.125 B/element vs 2 B for f16: exactly 9/16 of f16.
        assert_eq!(advice.kv_bytes_per_token_q8_0, Some(184_320));
        assert_eq!(advice.confidence, AdviceConfidence::Precise);
    }

    #[test]
    fn budget_and_context_are_honest_arithmetic() {
        // 48 GiB VRAM - 1 GiB overhead - 44 GiB weights = 3 GiB KV budget.
        let advice = boost_advice(&meta_full(), 44 * GIB, 48 * GIB, GIB);
        let budget = advice.kv_budget_bytes.expect("budget");
        assert_eq!(budget, 3 * GIB);
        assert_eq!(advice.max_resident_context_q8_0, Some((3 * GIB) / 184_320));
        assert_eq!(advice.max_resident_context_f16, Some((3 * GIB) / 327_680));
        // The f16 cache always allows strictly less context.
        assert!(advice.max_resident_context_f16 < advice.max_resident_context_q8_0);
        // Full train context (32_768 × 184_320 = ~6.0e9 B) does NOT fit in
        // 3 GiB: partial offload via the average-layer heuristic.
        assert!(advice.n_gpu_layers.is_some());
        assert_ne!(advice.n_gpu_layers, Some(-1));
        assert!(advice.n_gpu_layers_estimated);
        // ctx-size flag must never exceed the model's own train length.
        let idx = advice
            .launch_args
            .iter()
            .position(|arg| arg == "--ctx-size")
            .expect("ctx-size flag");
        let ctx: u64 = advice.launch_args[idx + 1].parse().unwrap();
        assert!(ctx <= 32_768);
    }

    #[test]
    fn small_model_gets_all_layers_and_no_estimation_flag() {
        let advice = boost_advice(&meta_full(), 4 * GIB, 24 * GIB, GIB);
        assert_eq!(advice.n_gpu_layers, Some(-1));
        assert!(!advice.n_gpu_layers_estimated);
        assert!(advice.launch_args.contains(&"--n-gpu-layers".to_owned()));
        let idx = advice
            .launch_args
            .iter()
            .position(|arg| arg == "--n-gpu-layers")
            .unwrap();
        assert_eq!(advice.launch_args[idx + 1], "-1");
    }

    #[test]
    fn missing_kv_fields_degrade_to_partial_never_panic() {
        let mut meta = meta_full();
        meta.head_count_kv = None;
        let advice = boost_advice(&meta, 40 * GIB, 48 * GIB, GIB);
        assert_eq!(advice.confidence, AdviceConfidence::Partial);
        assert_eq!(advice.kv_bytes_per_token_f16, None);
        // The layer heuristic still works from block_count + sizes.
        assert!(advice.n_gpu_layers.is_some());
        assert!(advice.n_gpu_layers_estimated);
    }

    #[test]
    fn advice_for_model_path_reads_a_real_gguf_file() {
        let mut bytes = b"GGUF".to_vec();
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&0u64.to_le_bytes());
        bytes.extend_from_slice(&7u64.to_le_bytes());
        let mut entry = |key: &str, kind: u32, payload: Vec<u8>| {
            bytes.extend_from_slice(&(key.len() as u64).to_le_bytes());
            bytes.extend_from_slice(key.as_bytes());
            bytes.extend_from_slice(&kind.to_le_bytes());
            bytes.extend_from_slice(&payload);
        };
        let u32le = |v: u32| v.to_le_bytes().to_vec();
        let strle = |v: &str| {
            let mut b = (v.len() as u64).to_le_bytes().to_vec();
            b.extend_from_slice(v.as_bytes());
            b
        };
        entry("general.architecture", 8, strle("qwen2"));
        entry("general.name", 8, strle("booster file test"));
        entry("qwen2.block_count", 4, u32le(28));
        entry("qwen2.attention.head_count", 4, u32le(28));
        entry("qwen2.attention.head_count_kv", 4, u32le(4));
        entry("qwen2.embedding_length", 4, u32le(3_584));
        entry("qwen2.context_length", 4, u32le(32_768));
        entry("general.file_type", 4, u32le(15));

        let dir = std::env::temp_dir().join(format!("dread-booster-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tiny.gguf");
        std::fs::write(&path, &bytes).unwrap();

        let advice = advice_for_model_path(&path, 24 * GIB, GIB).expect("advice");
        // 28 layers x 2 x 4 kv heads x (3584/28=128 dim) x 2 B = 57_344 B/tok
        assert_eq!(advice.kv_bytes_per_token_f16, Some(57_344));
        assert_eq!(advice.kv_bytes_per_token_q8_0, Some(32_256));
        assert_eq!(advice.confidence, AdviceConfidence::Precise);
        // Tiny file on a 24 GiB card: full residency.
        assert_eq!(advice.n_gpu_layers, Some(-1));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_block_count_is_unavailable_and_says_so() {
        let mut meta = meta_full();
        meta.block_count = None;
        let advice = boost_advice(&meta, 40 * GIB, 48 * GIB, GIB);
        assert_eq!(advice.confidence, AdviceConfidence::Unavailable);
        assert!(advice.reason.contains("refusing"));
        assert_eq!(advice.n_gpu_layers, None);
    }

    #[test]
    fn zero_vram_budget_never_wraps() {
        // Weights bigger than the whole card: budget underflows to None,
        // context advice disappears, no nonsense layers.
        let advice = boost_advice(&meta_full(), 80 * GIB, 24 * GIB, GIB);
        assert_eq!(advice.kv_budget_bytes, None);
        assert_eq!(advice.max_resident_context_q8_0, None);
        assert_eq!(advice.n_gpu_layers, None);
    }

    #[test]
    fn division_by_zero_fields_never_panics() {
        let mut meta = meta_full();
        meta.head_count = Some(0);
        meta.block_count = Some(0);
        meta.head_count_kv = Some(0);
        let advice = boost_advice(&meta, GIB, 24 * GIB, GIB);
        assert_eq!(advice.head_dim, None);
        assert_eq!(advice.kv_bytes_per_token_f16, None);
        assert_eq!(advice.n_gpu_layers, None);
    }
}
