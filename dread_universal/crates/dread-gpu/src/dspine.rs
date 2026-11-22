//! Read-only D-Spine awareness: detect SGLang integration roots, catalog local
//! HuggingFace-layout draft weights, and propose advisory draft/target pairings.
//!
//! v1 scope is detection and advice ONLY. This module never installs, never
//! downloads, never launches, never allocates VRAM, and never claims llama.cpp
//! compatibility: D-Spine draft trees require SGLang. Everything fails closed
//! and says so in the output. Filesystem access is bounded and symlink-safe.

use serde::Serialize;
use std::{fs, io::Read, path::PathBuf};

const MAX_CONFIG: u64 = 64 * 1024;
const MAX_ENTRIES: usize = 4096;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Precise,
    Partial,
    Unavailable,
}

#[derive(Debug, Serialize)]
pub struct EngineRoot {
    pub path: String,
    pub present: bool,
    pub marker_evidence: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Draft {
    pub path: String,
    pub family: Option<String>,
    pub size_hint: Option<String>,
    pub config_present: bool,
    pub weights_present: bool,
}

/// Minimal projection of a registry row for pairing. The database stays in
/// the API crate; this module only consumes plain data.
#[derive(Debug, Clone)]
pub struct RegistryRow {
    pub id: String,
    pub kind: String,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct Proposal {
    pub target_id: String,
    pub draft_path: String,
    pub serving_stack: String,
    pub confidence: Confidence,
    pub reason: String,
}

/// Exact marker: a real directory `sglang_dspine`, or a regular file
/// `pyproject.toml`/`setup.py` whose first 64 KiB contain `sglang_dspine`.
/// Symlinks are never followed.
fn marker(root: &std::path::Path) -> Option<String> {
    let package = root.join("sglang_dspine");
    // NOTE: must NOT early-return via `?` here: a missing package directory
    // still has to fall through to the declared-file markers.
    if fs::symlink_metadata(&package)
        .is_ok_and(|metadata| metadata.file_type().is_dir())
    {
        return Some("sglang_dspine/".into());
    }
    for name in ["pyproject.toml", "setup.py"] {
        let path = root.join(name);
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !metadata.file_type().is_file() {
            continue;
        }
        if read_bounded(&path).is_some_and(|text| text.contains("sglang_dspine")) {
            return Some(name.into());
        }
    }
    None
}

fn read_bounded(path: &std::path::Path) -> Option<String> {
    let file = fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_CONFIG + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() as u64 <= MAX_CONFIG)
        .then(|| String::from_utf8_lossy(&bytes).into_owned())
}

/// Roots are supplied by the caller (env roots plus the known repo). Missing
/// roots remain visible as `present:false`; errors degrade to absent, never
/// panic and never prune anything.
pub fn detect_engine_roots(roots: &[PathBuf]) -> Vec<EngineRoot> {
    roots
        .iter()
        .map(|root| {
            let evidence = marker(root);
            EngineRoot {
                path: root.display().to_string(),
                present: evidence.is_some(),
                marker_evidence: evidence,
            }
        })
        .collect()
}

fn lexical_dir(path: &std::path::Path) -> Option<PathBuf> {
    let metadata = fs::symlink_metadata(path).ok()?;
    metadata.file_type().is_dir().then(|| path.to_path_buf())
}

fn hints(name: &str) -> (Option<String>, Option<String>) {
    let lower = name.to_ascii_lowercase();
    let family = ["qwen3.8", "qwen3", "qwen2.5", "llama"]
        .iter()
        .find(|needle| lower.contains(**needle))
        .map(|found| found.to_string());
    let bytes = name.as_bytes();
    let size = bytes
        .windows(2)
        .enumerate()
        .find_map(|(index, window)| {
            if window[0].is_ascii_digit() && (window[1] == b'b' || window[1] == b'B') {
                let mut start = index;
                while start > 0
                    && (bytes[start - 1].is_ascii_digit() || bytes[start - 1] == b'.')
                {
                    start -= 1;
                }
                // End past the 'B' byte itself: the digit is 1 ASCII byte and
                // the trailing B is at byte `index + 1`, so the slice must
                // extend to `index + 2` (Kimi's spec truncated the unit).
                Some(name[start..index + 2].to_string())
            } else {
                None
            }
        });
    (family, size)
}

/// Scans only immediate children of each lexical model root. Draft
/// directories must contain `dspine` (case-insensitive) in the name and
/// either a regular `config.json` or an immediate regular `.safetensors`
/// file. Symlinked directories are rejected.
pub fn catalog_drafts(model_roots: &[PathBuf]) -> Vec<Draft> {
    let mut out = Vec::new();
    for root in model_roots {
        let Some(root) = lexical_dir(root) else {
            continue;
        };
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        for entry in entries.take(MAX_ENTRIES).flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let entry_name = entry.file_name().to_string_lossy().to_string();
            if !file_type.is_dir() || !entry_name.to_ascii_lowercase().contains("dspine") {
                continue;
            }
            let config_present = fs::symlink_metadata(path.join("config.json"))
                .is_ok_and(|metadata| metadata.file_type().is_file());
            let weights_present = fs::read_dir(&path)
                .ok()
                .into_iter()
                .flatten()
                .take(MAX_ENTRIES)
                .flatten()
                .any(|child| {
                    child.file_type().is_ok_and(|t| t.is_file())
                        && child
                            .path()
                            .extension()
                            .is_some_and(|ext| ext == "safetensors")
                });
            if !config_present && !weights_present {
                continue;
            }
            let (family, size_hint) = hints(&entry_name);
            out.push(Draft {
                path: path.display().to_string(),
                family,
                size_hint,
                config_present,
                weights_present,
            });
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// Advisory only. Family agreement is Partial; unknown or mismatched family
/// is Unavailable. No target means no proposal; heuristic pairing is never
/// launch authorization. Multiple targets are all returned deterministically.
pub fn build_proposals(drafts: &[Draft], rows: &[RegistryRow]) -> Vec<Proposal> {
    let mut targets: Vec<_> = rows
        .iter()
        .filter(|row| row.kind == "model.gguf")
        .collect();
    targets.sort_by(|a, b| a.id.cmp(&b.id));
    let mut out = Vec::new();
    for draft in drafts {
        for target in &targets {
            let target_family = hints(&target.name).0;
            let (confidence, reason) = match (&draft.family, &target_family) {
                (Some(draft_family), Some(target_family)) if draft_family == target_family => {
                    (
                        Confidence::Partial,
                        "heuristic family match; SGLang compatibility and draft/target pairing unverified".into(),
                    )
                }
                _ => (
                    Confidence::Unavailable,
                    "target family unknown or mismatched; manual verification required".into(),
                ),
            };
            out.push(Proposal {
                target_id: target.id.clone(),
                draft_path: draft.path.clone(),
                serving_stack: "sglang_required_llama_cpp_cannot_consume_dspine_draft_trees".into(),
                confidence,
                reason,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Self-cleaning temp fixture guard: Drop removes the tree even when an
    /// assertion panics below it (B9 lesson: no leaked fixtures).
    struct TempDir(std::path::PathBuf);
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn temp(tag: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!(
            "dspine-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos()
        ));
        fs::create_dir_all(&dir).expect("temp fixture root");
        TempDir(dir)
    }

    #[test]
    fn empty_roots_detect_and_catalog_nothing() {
        assert!(detect_engine_roots(&[]).is_empty());
        assert!(catalog_drafts(&[]).is_empty());
        assert!(build_proposals(&[], &[]).is_empty());
    }

    #[test]
    fn missing_marker_is_not_present() {
        let dir = temp("missing-marker");
        fs::create_dir_all(dir.0.join("engines/empty")).unwrap();
        let roots = detect_engine_roots(&[dir.0.join("engines/empty"), dir.0.join("absent")]);
        assert_eq!(roots.len(), 2);
        assert!(roots.iter().all(|root| !root.present));
        assert!(roots.iter().all(|root| root.marker_evidence.is_none()));
    }

    #[test]
    fn package_and_declared_file_markers_work() {
        let dir = temp("markers");
        fs::create_dir_all(dir.0.join("a/sglang_dspine")).unwrap();
        fs::create_dir_all(dir.0.join("b")).unwrap();
        fs::write(dir.0.join("b/pyproject.toml"), "name = \"sglang_dspine_ext\"").unwrap();
        fs::create_dir_all(dir.0.join("c")).unwrap();
        fs::write(dir.0.join("c/pyproject.toml"), "name = \"unrelated\"").unwrap();
        let found = detect_engine_roots(&[
            dir.0.join("a"),
            dir.0.join("b"),
            dir.0.join("c"),
        ]);
        assert_eq!(found[0].marker_evidence.as_deref(), Some("sglang_dspine/"));
        assert_eq!(found[1].marker_evidence.as_deref(), Some("pyproject.toml"));
        assert!(!found[2].present);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_marker_is_never_trusted() {
        let dir = temp("symlink-marker");
        let real = dir.0.join("real/sglang_dspine");
        fs::create_dir_all(&real).unwrap();
        fs::create_dir_all(dir.0.join("fake")).unwrap();
        std::os::unix::fs::symlink(&real, dir.0.join("fake/sglang_dspine")).unwrap();
        let roots = detect_engine_roots(&[dir.0.join("fake")]);
        // A symlinked package directory is rejected (symlink_metadata, not
        // is_dir): an attacker-controlled root cannot point the marker claim
        // at arbitrary content elsewhere.
        assert!(!roots[0].present);
    }

    #[test]
    fn two_drafts_one_target_yield_two_sorted_proposals() {
        let drafts = vec![
            Draft {
                path: "/m/Qwen3.8-27B-DSpine-b16".into(),
                family: Some("qwen3.8".into()),
                size_hint: Some("27B".into()),
                config_present: true,
                weights_present: false,
            },
            Draft {
                path: "/m/Qwen3-8B-DSpine-b16".into(),
                family: Some("qwen3".into()),
                size_hint: Some("8B".into()),
                config_present: true,
                weights_present: true,
            },
        ];
        let rows = vec![RegistryRow {
            id: "model-aaaaaaaaaaaa".into(),
            kind: "model.gguf".into(),
            name: "Qwen3-8B-Q5_KM.gguf".into(),
        }];
        let proposals = build_proposals(&drafts, &rows);
        assert_eq!(proposals.len(), 2);
        // Deterministic draft order comes from catalog_drafts sorting; here the
        // caller controls draft order, so assert content, not ordering luck.
        let qwen3 = proposals
            .iter()
            .find(|p| p.draft_path.ends_with("Qwen3-8B-DSpine-b16"))
            .unwrap();
        assert_eq!(qwen3.confidence, Confidence::Partial);
        let other = &proposals[if qwen3.draft_path == proposals[0].draft_path { 1 } else { 0 }];
        assert_eq!(other.confidence, Confidence::Unavailable);
    }

    #[test]
    fn zero_targets_yield_zero_proposals() {
        let drafts = vec![Draft {
            path: "/m/Any-DSpine".into(),
            family: Some("qwen3".into()),
            size_hint: None,
            config_present: true,
            weights_present: false,
        }];
        let rows = vec![RegistryRow {
            id: "comfy-1".into(),
            kind: "comfyui.instance".into(),
            name: "ComfyUI".into(),
        }];
        assert!(build_proposals(&drafts, &rows).is_empty());
    }

    #[test]
    fn unicode_draft_names_are_lossy_safe_and_stable() {
        let dir = temp("unicode");
        let draft = dir.0.join("Qwen3-8B-DSpine-b16-üñî");
        fs::create_dir_all(&draft).unwrap();
        fs::write(draft.join("config.json"), "{}").unwrap();
        let drafts = catalog_drafts(std::slice::from_ref(&dir.0));
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].family.as_deref(), Some("qwen3"));
        assert_eq!(drafts[0].size_hint.as_deref(), Some("8B"));
    }

    #[test]
    fn absent_or_unreadable_root_degrades_to_empty() {
        // Absent roots and plain files where a dir is expected must not panic.
        let dir = temp("absent");
        fs::write(dir.0.join("notadir"), "x").unwrap();
        assert!(catalog_drafts(&[dir.0.join("absent"), dir.0.join("notadir")]).is_empty());
    }

    #[test]
    fn drafts_require_name_and_evidence() {
        let dir = temp("evidence");
        // name matches but no config and no weights -> not a draft
        fs::create_dir_all(dir.0.join("SomeDSpine-empty")).unwrap();
        // weights but no config.json -> still a draft
        fs::create_dir_all(dir.0.join("Qwen3-8B-DSpine-b16")).unwrap();
        fs::write(
            dir.0.join("Qwen3-8B-DSpine-b16/model-00001-of-00002.safetensors"),
            "stub",
        )
        .unwrap();
        let drafts = catalog_drafts(std::slice::from_ref(&dir.0));
        assert_eq!(drafts.len(), 1);
        assert!(drafts[0].weights_present);
        assert!(!drafts[0].config_present);
    }

    #[test]
    fn foreign_kind_rows_never_become_targets() {
        let rows = vec![
            RegistryRow {
                id: "engine-zzz".into(),
                kind: "engine.dspine".into(),
                name: "Qwen3 DSpine".into(),
            },
            RegistryRow {
                id: "model-b".into(),
                kind: "model.gguf".into(),
                name: "qwen3 target".into(),
            },
        ];
        let proposals = build_proposals(
            &[Draft {
                path: "/d/DSpine".into(),
                family: Some("qwen3".into()),
                size_hint: None,
                config_present: true,
                weights_present: false,
            }],
            &rows,
        );
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].target_id, "model-b");
    }

    #[test]
    fn family_hints_prefer_specific_before_generic() {
        assert_eq!(hints("Qwen3.8-27B-DSpine-b16").0.as_deref(), Some("qwen3.8"));
        assert_eq!(hints("qwen2.5-coder-DSpine").0.as_deref(), Some("qwen2.5"));
        assert_eq!(hints("llama-7B-DSpine").1.as_deref(), Some("7B"));
        assert_eq!(hints("llama-3-DSpine").1, None);
        assert_eq!(hints("random-dspine").0, None);
    }
}
