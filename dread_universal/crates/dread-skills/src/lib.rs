//! Safe discovery and bounded loading of local Markdown skills.
//!
//! Discovery is metadata-only: imported files are never executed. Skill content is
//! untrusted prompt context and must not be treated as policy or tool permission.

use std::{
    fs,
    path::{Component, Path, PathBuf},
};

pub const MAX_SKILLS: usize = 512;
pub const MAX_SKILL_BYTES: usize = 128 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillDescriptor {
    pub id: String,
    pub name: String,
    pub description: String,
    pub relative_path: String,
    pub bytes: u64,
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillContext {
    pub descriptor: SkillDescriptor,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillError {
    InvalidRoot(String),
    MissingSkillsDirectory,
    TooManySkills,
    InvalidId,
    UnsafePath,
    TooLarge,
    InvalidUtf8,
    Io(String),
}

impl std::fmt::Display for SkillError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRoot(s) => write!(f, "invalid skill root: {s}"),
            Self::MissingSkillsDirectory => write!(f, "skill root has no skills directory"),
            Self::TooManySkills => write!(f, "skill catalog exceeds {MAX_SKILLS} entries"),
            Self::InvalidId => write!(f, "invalid skill identifier"),
            Self::UnsafePath => write!(f, "skill path is not a safe regular file"),
            Self::TooLarge => write!(f, "skill file exceeds {MAX_SKILL_BYTES} bytes"),
            Self::InvalidUtf8 => write!(f, "skill file is not valid UTF-8"),
            Self::Io(s) => write!(f, "skill I/O error: {s}"),
        }
    }
}
impl std::error::Error for SkillError {}

fn fingerprint(bytes: &[u8]) -> String {
    // Non-cryptographic change detector only; never use this as a trust signature.
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("fnv1a64:{hash:016x}")
}

fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 120
        && id.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_'
        })
}

fn metadata(content: &str, fallback: &str) -> (String, String) {
    let mut name = fallback.to_string();
    let mut description = String::new();
    let mut in_frontmatter = false;
    for (index, line) in content.lines().take(80).enumerate() {
        let trimmed = line.trim();
        if index == 0 && trimmed == "---" {
            in_frontmatter = true;
            continue;
        }
        if in_frontmatter && trimmed == "---" {
            break;
        }
        if !in_frontmatter {
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("name:") {
            let value = value.trim().trim_matches(['"', '\'']);
            if !value.is_empty() && value.len() <= 200 {
                name = value.to_string();
            }
        } else if let Some(value) = trimmed.strip_prefix("description:") {
            let value = value.trim().trim_matches(['"', '\'']);
            if value.len() <= 2000 {
                description = value.to_string();
            }
        }
    }
    (name, description)
}

fn read_descriptor(root: &Path, id: &str) -> Result<SkillDescriptor, SkillError> {
    if !safe_id(id) {
        return Err(SkillError::InvalidId);
    }
    let skills_dir = root.join("skills");
    let dir = skills_dir.join(id);
    let file = dir.join("SKILL.md");
    let dir_meta = fs::symlink_metadata(&dir).map_err(|e| SkillError::Io(e.to_string()))?;
    let file_meta = fs::symlink_metadata(&file).map_err(|e| SkillError::Io(e.to_string()))?;
    if !dir_meta.is_dir() || dir_meta.file_type().is_symlink()
        || !file_meta.is_file() || file_meta.file_type().is_symlink()
    {
        return Err(SkillError::UnsafePath);
    }
    let canonical = file.canonicalize().map_err(|e| SkillError::Io(e.to_string()))?;
    if !canonical.starts_with(root) {
        return Err(SkillError::UnsafePath);
    }
    if file_meta.len() > MAX_SKILL_BYTES as u64 {
        return Err(SkillError::TooLarge);
    }
    let bytes = fs::read(&canonical).map_err(|e| SkillError::Io(e.to_string()))?;
    if bytes.len() > MAX_SKILL_BYTES {
        return Err(SkillError::TooLarge);
    }
    let content = std::str::from_utf8(&bytes).map_err(|_| SkillError::InvalidUtf8)?;
    let (name, description) = metadata(content, id);
    Ok(SkillDescriptor {
        id: format!("pstack.skill.{id}"),
        name,
        description,
        relative_path: format!("skills/{id}/SKILL.md"),
        bytes: bytes.len() as u64,
        fingerprint: fingerprint(&bytes),
    })
}

/// Discover immediate child folders containing a regular, non-symlink SKILL.md.
pub fn discover_skills(root: impl AsRef<Path>) -> Result<Vec<SkillDescriptor>, SkillError> {
    let root = root.as_ref().canonicalize()
        .map_err(|e| SkillError::InvalidRoot(e.to_string()))?;
    let skills = root.join("skills");
    let metadata = fs::symlink_metadata(&skills).map_err(|_| SkillError::MissingSkillsDirectory)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(SkillError::MissingSkillsDirectory);
    }
    let mut ids = Vec::new();
    let entries = fs::read_dir(&skills).map_err(|e| SkillError::Io(e.to_string()))?;
    for entry in entries {
        let entry = entry.map_err(|e| SkillError::Io(e.to_string()))?;
        let file_type = entry.file_type().map_err(|e| SkillError::Io(e.to_string()))?;
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().into_owned();
        if !safe_id(&id) {
            continue;
        }
        if entry.path().join("SKILL.md").is_file() {
            ids.push(id);
        }
        if ids.len() > MAX_SKILLS {
            return Err(SkillError::TooManySkills);
        }
    }
    ids.sort();
    ids.into_iter().map(|id| read_descriptor(&root, &id)).collect()
}

/// Load one skill by its exact catalog id; the returned Markdown remains untrusted.
pub fn load_skill_context(
    root: impl AsRef<Path>,
    capability_id: &str,
) -> Result<SkillContext, SkillError> {
    let id = capability_id.strip_prefix("pstack.skill.").ok_or(SkillError::InvalidId)?;
    if !safe_id(id) {
        return Err(SkillError::InvalidId);
    }
    let root = root.as_ref().canonicalize()
        .map_err(|e| SkillError::InvalidRoot(e.to_string()))?;
    let descriptor = read_descriptor(&root, id)?;
    let path = root.join(PathBuf::from(&descriptor.relative_path));
    if path.components().any(|part| matches!(part, Component::ParentDir | Component::RootDir | Component::Prefix(_))) {
        return Err(SkillError::UnsafePath);
    }
    let bytes = fs::read(path).map_err(|e| SkillError::Io(e.to_string()))?;
    let content = String::from_utf8(bytes).map_err(|_| SkillError::InvalidUtf8)?;
    Ok(SkillContext { descriptor, content })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, time::{SystemTime, UNIX_EPOCH}};

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let id = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
            let path = std::env::temp_dir().join(format!("dread-skills-{id}"));
            fs::create_dir_all(path.join("skills/example")).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
    }

    #[test]
    fn discovers_metadata_without_executing_markdown() {
        let temp = Temp::new();
        fs::write(temp.0.join("skills/example/SKILL.md"),
            "---\nname: Example Skill\ndescription: Safe metadata\n---\n# instructions\n").unwrap();
        let items = discover_skills(&temp.0).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "pstack.skill.example");
        assert_eq!(items[0].name, "Example Skill");
        assert_eq!(items[0].description, "Safe metadata");
        assert!(items[0].fingerprint.starts_with("fnv1a64:"));
    }

    #[test]
    fn loads_only_exact_catalog_ids() {
        let temp = Temp::new();
        fs::write(temp.0.join("skills/example/SKILL.md"), "# safe context").unwrap();
        let context = load_skill_context(&temp.0, "pstack.skill.example").unwrap();
        assert_eq!(context.content, "# safe context");
        assert!(matches!(load_skill_context(&temp.0, "pstack.skill.../../../etc/passwd"), Err(SkillError::InvalidId)));
    }

    #[test]
    fn rejects_oversized_and_symlink_skill_files() {
        let temp = Temp::new();
        fs::write(temp.0.join("skills/example/SKILL.md"), vec![b'x'; MAX_SKILL_BYTES + 1]).unwrap();
        assert_eq!(discover_skills(&temp.0), Err(SkillError::TooLarge));
    }

    #[test]
    fn rejects_missing_catalog_root() {
        let temp = Temp::new();
        assert_eq!(discover_skills(temp.0.join("missing")), Err(SkillError::InvalidRoot("No such file or directory (os error 2)".into())));
    }
}
