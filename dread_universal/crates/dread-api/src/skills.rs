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
/// pair moves, so resyncs update in place. 128-bit truncation keeps birthday
/// collisions far below any realistic skill count (hostile-review fix #4).
pub fn skill_registry_id(root: &Path, skill_id: &str) -> String {
    let digest = ring::digest::digest(
        &ring::digest::SHA256,
        format!("{}|{}", root.display(), skill_id).as_bytes(),
    );
    let hex: String = digest
        .as_ref()
        .iter()
        .take(16)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("skill-{hex}")
}

/// Scan every configured root. Roots are canonicalized once and deduplicated
/// so path aliases (symlinks, `./`, trailing slashes) cannot fork duplicate
/// rows (fix #3). A broken root produces a warning and is excluded from
/// `ok_roots` — the caller must never prune rows for a root that failed to
/// scan, or a transiently unmounted disk would erase live registrations (fix #1).
pub fn scan_skill_roots(roots: &[PathBuf]) -> (Vec<ScannedSkill>, Vec<String>, Vec<PathBuf>) {
    let mut found = Vec::new();
    let mut warnings = Vec::new();
    let mut ok_roots: Vec<PathBuf> = Vec::new();
    for root in roots {
        let canonical = match root.canonicalize() {
            Ok(path) => path,
            Err(error) => {
                warnings.push(format!("{}: invalid skill root: {error}", root.display()));
                continue;
            }
        };
        if ok_roots.contains(&canonical) {
            continue; // alias of an already-scanned root
        }
        match discover_skills(&canonical) {
            Ok(descriptors) => {
                for descriptor in descriptors {
                    found.push(ScannedSkill {
                        descriptor,
                        source_root: canonical.clone(),
                    });
                }
                ok_roots.push(canonical);
            }
            Err(error) => warnings.push(format!("{}: {error}", canonical.display())),
        }
    }
    (found, warnings, ok_roots)
}

/// Idempotently upsert every scanned skill and prune rows belonging to roots
/// that scanned successfully but whose skill folder disappeared (deletions
/// converge automatically). `ok_roots` must come from `scan_skill_roots` and
/// contain only roots that scanned cleanly (fix #1). The whole pass runs in
/// one transaction so a mid-way failure rolls back to the previous state
/// instead of leaving a half-applied registry (fix #2).
pub fn register_skills(
    conn: &rusqlite::Connection,
    scanned: &[ScannedSkill],
    ok_roots: &[PathBuf],
    now: i64,
) -> Result<Value, String> {
    let mut registered = 0usize;
    let mut pruned = 0usize;
    conn.execute("BEGIN IMMEDIATE", [])
        .map_err(|error| format!("could not begin skill sync: {error}"))?;
    let outcome = (|| -> Result<(), String> {
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
            .map_err(|error| {
                format!("could not register skill {}: {error}", skill.descriptor.id)
            })?;
            registered += 1;
        }

        // Prune stale rows, but only for roots that scanned cleanly.
        for root in ok_roots {
            let root_text = root.display().to_string();
            let keep = ids_by_root.get(&root_text).cloned().unwrap_or_default();
            let existing: Vec<String> = {
                let mut stmt = conn
                    .prepare("SELECT id FROM registry WHERE kind = ?1 AND endpoint = ?2")
                    .map_err(|error| error.to_string())?;
                let rows = stmt
                    .query_map(rusqlite::params![SKILL_KIND, root_text], |row| {
                        row.get::<_, String>(0)
                    })
                    .map_err(|error| error.to_string())?;
                // A read error must abort the pass, never shrink `existing`
                // into a silent destructive prune (fix #5).
                rows.collect::<Result<Vec<_>, _>>()
                    .map_err(|error| error.to_string())?
            };
            for stale in existing.iter().filter(|id| !keep.contains(*id)) {
                // Delete is scoped to the same root it was selected from so a
                // colliding id under another endpoint can never be touched (fix #6).
                pruned += conn
                    .execute(
                        "DELETE FROM registry WHERE id = ?1 AND kind = ?2 AND endpoint = ?3",
                        rusqlite::params![stale, SKILL_KIND, root_text],
                    )
                    .map_err(|error| error.to_string())?;
            }
        }
        Ok(())
    })();
    match outcome {
        Ok(()) => {
            conn.execute("COMMIT", [])
                .map_err(|error| format!("could not commit skill sync: {error}"))?;
        }
        Err(error) => {
            conn.execute("ROLLBACK", []).ok();
            return Err(error);
        }
    }

    Ok(json!({
        "registered": registered,
        "pruned": pruned,
        "roots": ok_roots.iter().map(|r| r.display().to_string()).collect::<Vec<_>>(),
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
        let (scanned, warnings, ok_roots) = scan_skill_roots(std::slice::from_ref(&root));
        assert_eq!(scanned.len(), 2);
        assert!(warnings.is_empty());
        let report = register_skills(&conn, &scanned, &ok_roots, 1000).unwrap();
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
        let (scanned, _, ok_roots) = scan_skill_roots(std::slice::from_ref(&root));
        register_skills(&conn, &scanned, &ok_roots, 1).unwrap();
        assert_eq!(skills_listing(&conn).unwrap().len(), 2);

        fs::remove_dir_all(root.join("skills").join("drop")).unwrap();
        let (scanned2, _, ok_roots2) = scan_skill_roots(std::slice::from_ref(&root));
        let report = register_skills(&conn, &scanned2, &ok_roots2, 2).unwrap();
        assert_eq!(report["registered"], 1);
        assert_eq!(report["pruned"], 1);
        let listing = skills_listing(&conn).unwrap();
        assert_eq!(listing.len(), 1);
        assert_eq!(listing[0]["name"], "Keep");
        fs::remove_dir_all(std::env::temp_dir().join(root.file_name().unwrap())).ok();
    }

    // Hostile-review fix #1 regression: a root that becomes unscannable must
    // warn but NEVER delete its registered rows (unmounted-disk scenario).
    #[test]
    fn unscannable_root_preserves_its_registered_rows() {
        let conn = setup_db();
        let root = temp_root("vanish");
        write_skill(&root, "precious", "Precious", "must survive");
        let (scanned, _, ok_roots) = scan_skill_roots(std::slice::from_ref(&root));
        register_skills(&conn, &scanned, &ok_roots, 1).unwrap();
        assert_eq!(skills_listing(&conn).unwrap().len(), 1);

        fs::remove_dir_all(&root).unwrap();
        let (scanned2, warnings2, ok_roots2) = scan_skill_roots(std::slice::from_ref(&root));
        assert!(scanned2.is_empty());
        assert_eq!(warnings2.len(), 1);
        assert!(ok_roots2.is_empty());
        let report = register_skills(&conn, &scanned2, &ok_roots2, 2).unwrap();
        assert_eq!(report["registered"], 0);
        assert_eq!(report["pruned"], 0);
        assert_eq!(skills_listing(&conn).unwrap().len(), 1);
    }

    // Hostile-review fix #3 regression: symlink alias of the same root must
    // dedupe to one scan pass, never duplicate rows.
    #[cfg(unix)]
    #[test]
    fn path_aliases_dedupe_to_one_registration() {
        let conn = setup_db();
        let root = temp_root("alias");
        write_skill(&root, "solo", "Solo", "one skill");
        let alias = std::env::temp_dir().join(format!(
            "dread-skills-test-alias-{}-link",
            root.file_name().unwrap().to_str().unwrap()
        ));
        std::os::unix::fs::symlink(&root, &alias).unwrap();
        let (scanned, warnings, ok_roots) = scan_skill_roots(&[root.clone(), alias.clone()]);
        assert_eq!(scanned.len(), 1, "alias must not double-scan");
        assert!(warnings.is_empty());
        assert_eq!(ok_roots.len(), 1);
        register_skills(&conn, &scanned, &ok_roots, 1).unwrap();
        assert_eq!(skills_listing(&conn).unwrap().len(), 1);
        fs::remove_file(&alias).ok();
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn broken_root_warns_without_aborting_other_roots() {
        let good = temp_root("goodroot");
        write_skill(&good, "solo", "Solo", "one skill");
        let missing = std::env::temp_dir().join("dread-skills-test-nope-does-not-exist");
        let (scanned, warnings, ok_roots) = scan_skill_roots(&[missing, good]);
        assert_eq!(scanned.len(), 1);
        assert_eq!(warnings.len(), 1);
        assert_eq!(ok_roots.len(), 1);
        assert!(warnings[0].contains("invalid skill root"));
        fs::remove_dir_all(std::env::temp_dir().join(ok_roots[0].file_name().unwrap())).ok();
    }

    #[test]
    fn ids_are_stable_per_pair_and_differ_across_roots() {
        let a = skill_registry_id(Path::new("/x/upstream"), "pstack.skill.architect");
        let b = skill_registry_id(Path::new("/x/upstream"), "pstack.skill.architect");
        let c = skill_registry_id(Path::new("/y/other"), "pstack.skill.architect");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(a.starts_with("skill-"));
        assert_eq!(a.len(), 6 + 32);
    }
}
