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

    let mut head_dim_note: Option<String> = None;
    let head_dim = match (metadata.embedding_length, metadata.head_count) {
        (Some(emb), Some(heads)) if heads > 0 => {
            if emb % heads != 0 {
                head_dim_note = Some(
                    "embedding_length is not divisible by head_count: head_dim is rounded down"
                        .to_owned(),
                );
            }
            Some(emb / heads)
        }
        _ => None,
    };

    // KV elements per token, exactly: one vector each for K and V per kv
    // head per layer. q8_0 packs 32 elements into a 36-byte block, so its
    // cost is whole blocks per token - never fewer than ceil(blocks) x 36.
    // Hostile metadata that overflows u64 degrades to "unknown", never to
    // a wrong number.
    let kv_elements = match (metadata.block_count, metadata.head_count_kv, head_dim) {
        (Some(layers), Some(kv_heads), Some(dim)) if layers > 0 && kv_heads > 0 && dim > 0 => {
            layers
                .checked_mul(2)
                .and_then(|v| v.checked_mul(kv_heads))
                .and_then(|v| v.checked_mul(dim))
        }
        _ => None,
    };
    let kv_f16 = kv_elements.and_then(|e| e.checked_mul(F16_BYTES_PER_ELEMENT));
    let kv_q8_0 = kv_elements.and_then(|e| {
        e.div_ceil(Q8_0_BLOCK_ELEMENTS)
            .checked_mul(Q8_0_BLOCK_BYTES)
    });

    // Free space for KV once weights and runtime overhead are placed. This
    // is valid whatever else is known, so it survives into Partial advice.
    let after_runtime = total_vram_bytes.checked_sub(runtime_overhead_bytes);
    let kv_budget = after_runtime.and_then(|free| free.checked_sub(file_size_bytes));

    let (max_ctx_q8_0, max_ctx_f16) = match kv_budget {
        Some(budget) => (
            kv_q8_0.and_then(|per_token| (per_token > 0).then(|| budget / per_token)),
            kv_f16.and_then(|per_token| (per_token > 0).then(|| budget / per_token)),
        ),
        None => (None, None),
    };

    // Offload advice requires the exact KV cost: without it no context can
    // be reserved, so we report the budget and refuse to guess layers.
    let mut n_gpu_layers = None;
    let mut estimated = false;
    let mut ctx_guaranteed: Option<u64> = None;
    if let (Some(layers), Some(per_token_q8)) = (metadata.block_count, kv_q8_0) {
        let full_context_fits = kv_budget.zip(metadata.context_length).is_some_and(
            |(budget, train_ctx)| per_token_q8.saturating_mul(train_ctx) <= budget,
        );
        if full_context_fits {
            n_gpu_layers = Some(-1);
            ctx_guaranteed = max_ctx_q8_0;
        } else if let Some(free) = after_runtime {
            // Weights do not all fit with the trained context. Budget a
            // modest context first, then size the GPU-resident layer slice
            // against the REAL free memory (post-weights budgets double-
            // count, because offloaded layers come back as free space).
            let modest_ctx_tokens: u64 = 4_096;
            let ctx_target = metadata
                .context_length
                .unwrap_or(modest_ctx_tokens)
                .min(modest_ctx_tokens);
            let kv_reserve = per_token_q8.saturating_mul(ctx_target);
            if let Some(weights_budget) = free.checked_sub(kv_reserve) {
                // layers x weights_budget >= file_size x N guarantees the
                // N-th layer plus the reserved KV cache both fit.
                let fits = if file_size_bytes > 0 {
                    weights_budget
                        .checked_mul(layers)
                        .map_or(0, |scaled| scaled / file_size_bytes)
                        .min(layers)
                } else {
                    layers
                };
                if fits >= layers {
                    n_gpu_layers = Some(-1);
                    ctx_guaranteed = Some(ctx_target);
                } else {
                    n_gpu_layers = Some(i64::try_from(fits).unwrap_or(i64::MAX));
                    estimated = true;
                    if fits > 0 {
                        ctx_guaranteed = Some(ctx_target);
                    }
                }
            } else {
                // Not even a modest context fits next to the runtime.
                n_gpu_layers = Some(0);
                estimated = true;
            }
        }
    }

    if let Some(layers) = n_gpu_layers {
        launch_args.push("--n-gpu-layers".to_owned());
        launch_args.push(layers.to_string());
    }
    // The emitted --ctx-size is only what the advice has actually budgeted;
    // max_resident_context_* fields stay as separate full-offload ceilings.
    if let Some(ctx) = ctx_guaranteed {
        let capped = match metadata.context_length {
            Some(train_ctx) if train_ctx > 0 => ctx.min(train_ctx),
            _ => ctx,
        };
        if capped > 0 {
            launch_args.push("--ctx-size".to_owned());
            launch_args.push(i64::try_from(capped).unwrap_or(i64::MAX).to_string());
        }
    }

    let (confidence, reason) = if let Some(note) = head_dim_note {
        (AdviceConfidence::Partial, note)
    } else if kv_f16.is_some() && kv_q8_0.is_some() {
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
    let (metadata, size) = crate::models::read_gguf_metadata_with_size(path)?;
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
        // 80 layers x 2 x 8 kv heads x 128 dim = 163_840 elements/token.
        let advice = boost_advice(&meta_full(), 40 * GIB, 48 * GIB, GIB);
        assert_eq!(advice.head_dim, Some(128));
        // f16: 163_840 x 2 B = 327_680 B/token.
        assert_eq!(advice.kv_bytes_per_token_f16, Some(327_680));
        // q8_0: 5_120 whole 36-byte blocks = 184_320 B/token (exactly
        // 9/16 of f16 here because the element count is block-aligned).
        assert_eq!(advice.kv_bytes_per_token_q8_0, Some(184_320));
        assert_eq!(advice.confidence, AdviceConfidence::Precise);
    }

    #[test]
    fn q8_0_costs_whole_blocks_per_token() {
        // Head dim 33: 4 x 2 x 2 x 33 = 528 elements = 16.5 q8_0 blocks.
        // The 17th block is paid in full: 612 B, not the pro-rated 594.
        let mut meta = meta_full();
        meta.block_count = Some(4);
        meta.head_count = Some(1);
        meta.head_count_kv = Some(2);
        meta.embedding_length = Some(33);
        let advice = boost_advice(&meta, GIB, GIB, GIB);
        assert_eq!(advice.head_dim, Some(33));
        assert_eq!(advice.kv_bytes_per_token_f16, Some(1_056));
        assert_eq!(advice.kv_bytes_per_token_q8_0, Some(612));
        // 33 % 1 == 0, so the division is exact and stays Precise.
        assert_eq!(advice.confidence, AdviceConfidence::Precise);
    }

    #[test]
    fn lossy_head_dim_division_degrades_confidence() {
        // 3585 / 28 rounds down: honest advice says so and goes Partial.
        let mut meta = meta_full();
        meta.head_count = Some(28);
        meta.embedding_length = Some(3_585);
        let advice = boost_advice(&meta, GIB, GIB, GIB);
        assert_eq!(advice.head_dim, Some(128));
        assert_eq!(advice.confidence, AdviceConfidence::Partial);
        assert!(advice.reason.contains("divisible"));
    }

    #[test]
    fn budget_and_context_are_honest_arithmetic() {
        // 48 GiB VRAM - 1 GiB overhead = 47 GiB free; 48_600 MiB of
        // weights leave less KV headroom than the trained context needs.
        let file = 48_600 * 1024 * 1024;
        let free = 47 * GIB;
        let advice = boost_advice(&meta_full(), file, 48 * GIB, GIB);
        assert_eq!(advice.kv_budget_bytes, None); // weights beat full free
        assert_eq!(advice.max_resident_context_q8_0, None);
        assert_eq!(advice.max_resident_context_f16, None);
        // The honest answer: a maximal layer slice that fits alongside a
        // reserved 4096-token q8_0 cache inside FREE memory.
        let kv_reserve = 4_096 * 184_320;
        let expected = (free - kv_reserve) * 80 / file;
        assert!(expected > 0 && expected < 80, "fixture must be partial");
        let n = advice.n_gpu_layers.expect("layers");
        assert_eq!(u64::try_from(n).expect("non-negative"), expected);
        assert!(advice.n_gpu_layers_estimated);
        // Invariants: the slice + reserved KV fit, and one more layer not.
        let per_layer = file / 80;
        assert!(expected * per_layer + kv_reserve <= free);
        assert!((expected + 1) * per_layer + kv_reserve > free);
        // ctx-size flag carries the reserved, budgeted context.
        let idx = advice
            .launch_args
            .iter()
            .position(|arg| arg == "--ctx-size")
            .expect("ctx-size flag");
        let ctx: u64 = advice.launch_args[idx + 1].parse().unwrap();
        assert_eq!(ctx, 4_096);
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
        // Full offload: the ctx-size flag carries the ceiling, capped at
        // the model's trained context - advice never exceeds the model.
        let idx = advice
            .launch_args
            .iter()
            .position(|arg| arg == "--ctx-size")
            .unwrap();
        let ctx: u64 = advice.launch_args[idx + 1].parse().unwrap();
        assert_eq!(ctx, 32_768);
        assert!(ctx <= advice.max_resident_context_q8_0.expect("ceiling"));
    }

    #[test]
    fn missing_kv_fields_never_emit_launch_guesses() {
        let mut meta = meta_full();
        meta.head_count_kv = None;
        let advice = boost_advice(&meta, 40 * GIB, 48 * GIB, GIB);
        assert_eq!(advice.confidence, AdviceConfidence::Partial);
        assert_eq!(advice.kv_bytes_per_token_f16, None);
        assert_eq!(advice.kv_bytes_per_token_q8_0, None);
        // No KV cost means no reserved context, so no layers and no ctx.
        assert_eq!(advice.n_gpu_layers, None);
        assert!(!advice.launch_args.contains(&"--n-gpu-layers".to_owned()));
        assert!(!advice.launch_args.contains(&"--ctx-size".to_owned()));
        // The byte budget is still honest arithmetic.
        assert_eq!(advice.kv_budget_bytes, Some(7 * GIB));
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

        struct TempDir(std::path::PathBuf);
        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let dir = TempDir(
            std::env::temp_dir().join(format!(
                "dread-booster-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or_else(|_| 0, |d| d.subsec_nanos())
            )),
        );
        std::fs::create_dir_all(&dir.0).expect("temp dir");
        let path = dir.0.join("tiny.gguf");
        std::fs::write(&path, &bytes).expect("write fixture");

        let advice = advice_for_model_path(&path, 24 * GIB, GIB).expect("advice");
        // 28 layers x 2 x 4 kv heads x (3584/28=128 dim) = 28_672 elements.
        assert_eq!(advice.kv_bytes_per_token_f16, Some(57_344));
        assert_eq!(advice.kv_bytes_per_token_q8_0, Some(32_256));
        assert_eq!(advice.confidence, AdviceConfidence::Precise);
        // Tiny file on a 24 GiB card: full residency.
        assert_eq!(advice.n_gpu_layers, Some(-1));
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
    fn oversized_weights_answer_partial_offload_not_wrapping() {
        // 80 GiB weights on a 24 GiB card: full residency is impossible,
        // but a maximal layer slice + reserved modest context DOES fit,
        // and that is the honest answer. No wrapping, no panics, ever.
        let advice = boost_advice(&meta_full(), 80 * GIB, 24 * GIB, GIB);
        assert_eq!(advice.kv_budget_bytes, None);
        assert_eq!(advice.max_resident_context_q8_0, None);
        let n = u64::try_from(advice.n_gpu_layers.expect("layers")).expect("non-negative");
        assert!(n < 80);
        assert!(advice.n_gpu_layers_estimated);
        // Invariant holds even here: slice + reserved KV fit in free.
        let free = 23 * GIB;
        assert!(n.saturating_mul((80 * GIB) / 80) + 4_096 * 184_320 <= free);
        let idx = advice
            .launch_args
            .iter()
            .position(|arg| arg == "--ctx-size")
            .unwrap();
        let ctx: u64 = advice.launch_args[idx + 1].parse().unwrap();
        assert_eq!(ctx * 184_320 + n * ((80 * GIB) / 80), ctx * 184_320 + n * GIB);
    }

    #[test]
    fn tiny_card_refuses_layers_instead_of_guessing() {
        // 3 GiB card, 44 GiB weights: not even 4096 tokens of KV fit next
        // to the runtime, so layer advice would be a lie. Honest answer:
        // zero GPU layers, no context flag.
        let advice = boost_advice(&meta_full(), 44 * GIB, 3 * GIB, 2 * GIB);
        assert_eq!(advice.n_gpu_layers, Some(0));
        assert!(advice.launch_args.contains(&"--n-gpu-layers".to_owned()));
        assert!(!advice.launch_args.contains(&"--ctx-size".to_owned()));
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
