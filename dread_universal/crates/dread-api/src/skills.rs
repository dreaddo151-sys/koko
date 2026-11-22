//! Skill autodiscovery for the hub: scan configured roots with the hardened
//! dread-skills scanner and register results idempotently in the registry.
//!
//! Metadata-only by design: SKILL.md files are untrusted prompt content and
//! are never executed or treated as policy (see dread-skills crate docs).

use dread_skills::{SkillDescriptor, discover_skills};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const REGISTRY_UPSERT: &str = "INSERT INTO registry (id, name, kind, endpoint, metadata, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT(id) DO UPDATE SET name=excluded.name, endpoint=excluded.endpoint, metadata=excluded.metadata";

pub const SKILL_KIND: &str = "skill.local";

/// A discovered skill plus the configured root it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedSkill {
    pub descriptor: SkillDescriptor,
    pub source_root: PathBuf,
}

/// Stable registry id for (root, skill) pairs: content moves only when the
/// pair moves, so resyncs update in place. 12 hex chars, like provider ids.
pub fn skill_registry_id(root: &Path, skill_id: &str) -> String {
    let digest = ring::digest::digest(
        &ring::digest::SHA256,
        format!("{}|{}", root.display(), skill_id).as_bytes(),
    );
    let hex: String = digest
        .as_ref()
        .iter()
        .take(6)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("skill-{hex}")
}

/// Scan every configured root. A broken root produces a warning, never an
/// aborted pass (mirrors provider drop-in semantics).
pub fn scan_skill_roots(roots: &[PathBuf]) -> (Vec<ScannedSkill>, Vec<String>) {
    let mut found = Vec::new();
    let mut warnings = Vec::new();
    for root in roots {
        match discover_skills(root) {
            Ok(descriptors) => {
                for descriptor in descriptors {
                    found.push(ScannedSkill {
                        descriptor,
                        source_root: root.clone(),
                    });
                }
            }
            Err(error) => warnings.push(format!("{}: {error}", root.display())),
        }
    }
    (found, warnings)
}

/// Idempotently upsert every scanned skill and prune rows belonging to the
/// same roots that no longer exist (folder deletions converge automatically).
pub fn register_skills(
    conn: &rusqlite::Connection,
    scanned: &[ScannedSkill],
    roots: &[PathBuf],
    now: i64,
) -> Result<Value, String> {
    let mut registered = 0usize;
    let mut ids_by_root: std::collections::BTreeMap<String, BTreeSet<String>> =
        std::collections::BTreeMap::new();
    for skill in scanned {
        let root_key = skill.source_root.display().to_string();
        let id = skill_registry_id(&skill.source_root, &skill.descriptor.id);
        ids_by_root.entry(root_key).or_default().insert(id.clone());
        let metadata = json!({
            "skill_name": skill.descriptor.name,
            "description": skill.descriptor.description,
            "relative_path": skill.descriptor.relative_path,
            "fingerprint": skill.descriptor.fingerprint,
            "bytes": skill.descriptor.bytes,
            "source_root": skill.source_root.display().to_string(),
            "registered_at": now,
        });
        conn.execute(
            REGISTRY_UPSERT,
            rusqlite::params![
                id,
                skill.descriptor.name,
                SKILL_KIND,
                skill.source_root.display().to_string(),
                metadata.to_string(),
                now
            ],
        )
        .map_err(|error| format!("could not register skill {}: {error}", skill.descriptor.id))?;
        registered += 1;
    }

    // Prune stale rows per scanned root so removed skills disappear on resync.
    let mut pruned = 0usize;
    for root in roots {
        let root_text = root.display().to_string();
        let keep = ids_by_root.get(&root_text).cloned().unwrap_or_default();
        let existing: Vec<String> = {
            let mut stmt = conn
                .prepare("SELECT id, metadata FROM registry WHERE kind = ?1 AND endpoint = ?2")
                .map_err(|error| error.to_string())?;
            let rows = stmt
                .query_map(rusqlite::params![SKILL_KIND, root_text], |row| {
                    row.get::<_, String>(0)
                })
                .map_err(|error| error.to_string())?;
            rows.filter_map(Result::ok).collect()
        };
        for stale in existing.iter().filter(|id| !keep.contains(*id)) {
            conn.execute(
                "DELETE FROM registry WHERE id = ?1 AND kind = ?2",
                rusqlite::params![stale, SKILL_KIND],
            )
            .map_err(|error| error.to_string())?;
            pruned += 1;
        }
    }

    Ok(json!({
        "registered": registered,
        "pruned": pruned,
        "roots": roots.iter().map(|r| r.display().to_string()).collect::<Vec<_>>(),
    }))
}

/// List registered skills for the API (metadata parsed back into JSON).
pub fn skills_listing(conn: &rusqlite::Connection) -> Result<Vec<Value>, String> {
    let mut stmt = conn
        .prepare("SELECT id, name, endpoint, metadata FROM registry WHERE kind = ?1 ORDER BY name")
        .map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params![SKILL_KIND], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(|error| error.to_string())?;
    let mut out = Vec::new();
    for row in rows {
        let (id, name, endpoint, metadata) = row.map_err(|error| error.to_string())?;
        let meta: Value = serde_json::from_str(&metadata).unwrap_or(Value::Null);
        out.push(json!({
            "id": id,
            "name": name,
            "source_root": endpoint,
            "description": meta.get("description").cloned().unwrap_or(Value::Null),
            "fingerprint": meta.get("fingerprint").cloned().unwrap_or(Value::Null),
            "bytes": meta.get("bytes").cloned().unwrap_or(Value::Null),
        }));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_db() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE registry (id TEXT PRIMARY KEY, name TEXT, kind TEXT, endpoint TEXT, metadata TEXT, created_at INTEGER);",
        )
        .unwrap();
        conn
    }

    fn temp_root(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dread-skills-test-{}-{}",
            label,
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(dir.join("skills")).unwrap();
        dir
    }

    fn write_skill(root: &Path, id: &str, name: &str, desc: &str) {
        let dir = root.join("skills").join(id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {desc}\n---\n\n# Body\n"),
        )
        .unwrap();
    }

    #[test]
    fn scan_and_register_round_trip_is_one_row_per_skill() {
        let conn = setup_db();
        let root = temp_root("roundtrip");
        write_skill(&root, "alpha", "Alpha", "first skill");
        write_skill(&root, "beta", "Beta", "second skill");
        let (scanned, warnings) = scan_skill_roots(std::slice::from_ref(&root));
        assert_eq!(scanned.len(), 2);
        assert!(warnings.is_empty());
        let report = register_skills(&conn, &scanned, std::slice::from_ref(&root), 1000).unwrap();
        assert_eq!(report["registered"], 2);
        assert_eq!(report["pruned"], 0);
        let listing = skills_listing(&conn).unwrap();
        assert_eq!(listing.len(), 2);
        assert!(listing.iter().any(|s| s["name"] == "Alpha"));
        fs::remove_dir_all(std::env::temp_dir().join(root.file_name().unwrap())).ok();
    }

    #[test]
    fn resync_is_idempotent_and_prunes_deleted_skills() {
        let conn = setup_db();
        let root = temp_root("prune");
        write_skill(&root, "keep", "Keep", "stays");
        write_skill(&root, "drop", "Drop", "goes away");
        let (scanned, _) = scan_skill_roots(std::slice::from_ref(&root));
        register_skills(&conn, &scanned, std::slice::from_ref(&root), 1).unwrap();
        assert_eq!(skills_listing(&conn).unwrap().len(), 2);

        fs::remove_dir_all(root.join("skills").join("drop")).unwrap();
        let (scanned2, _) = scan_skill_roots(std::slice::from_ref(&root));
        let report = register_skills(&conn, &scanned2, std::slice::from_ref(&root), 2).unwrap();
        assert_eq!(report["registered"], 1);
        assert_eq!(report["pruned"], 1);
        let listing = skills_listing(&conn).unwrap();
        assert_eq!(listing.len(), 1);
        assert_eq!(listing[0]["name"], "Keep");
        fs::remove_dir_all(std::env::temp_dir().join(root.file_name().unwrap())).ok();
    }

    #[test]
    fn broken_root_warns_without_aborting_other_roots() {
        let good = temp_root("goodroot");
        write_skill(&good, "solo", "Solo", "one skill");
        let missing = std::env::temp_dir().join("dread-skills-test-nope-does-not-exist");
        let (scanned, warnings) = scan_skill_roots(&[missing, good]);
        assert_eq!(scanned.len(), 1);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("invalid skill root"));
    }

    #[test]
    fn ids_are_stable_per_pair_and_differ_across_roots() {
        let a = skill_registry_id(Path::new("/x/upstream"), "pstack.skill.architect");
        let b = skill_registry_id(Path::new("/x/upstream"), "pstack.skill.architect");
        let c = skill_registry_id(Path::new("/y/other"), "pstack.skill.architect");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(a.starts_with("skill-"));
        assert_eq!(a.len(), 6 + 12);
    }
}
