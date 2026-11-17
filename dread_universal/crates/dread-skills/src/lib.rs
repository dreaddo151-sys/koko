//! Safe discovery and bounded loading of local Markdown skills and agent profiles.
//!
//! Discovery is metadata-only: imported files are never executed. Markdown is
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
pub struct AgentProfileDescriptor {
    pub id: String,
    pub name: String,
    pub description: String,
    pub relative_path: String,
    pub bytes: u64,
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentProfileContext {
    pub descriptor: AgentProfileDescriptor,
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
            Self::TooManySkills => write!(f, "catalog exceeds {MAX_SKILLS} entries"),
            Self::InvalidId => write!(f, "invalid capability identifier"),
            Self::UnsafePath => write!(f, "capability path is not a safe regular file"),
            Self::TooLarge => write!(f, "Markdown file exceeds {MAX_SKILL_BYTES} bytes"),
            Self::InvalidUtf8 => write!(f, "Markdown file is not valid UTF-8"),
            Self::Io(s) => write!(f, "capability I/O error: {s}"),
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
        && id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
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
            let value = value.trim().trim_matches(&['"', '\''][..]);
            if !value.is_empty() && value.len() <= 200 {
                name = value.to_string();
            }
        } else if let Some(value) = trimmed.strip_prefix("description:") {
            let value = value.trim().trim_matches(&['"', '\''][..]);
            if value.len() <= 2_000 {
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

    let dir = root.join("skills").join(id);
    let file = dir.join("SKILL.md");
    let dir_meta = fs::symlink_metadata(&dir).map_err(|e| SkillError::Io(e.to_string()))?;
    let file_meta = fs::symlink_metadata(&file).map_err(|e| SkillError::Io(e.to_string()))?;

    if !dir_meta.is_dir()
        || dir_meta.file_type().is_symlink()
        || !file_meta.is_file()
        || file_meta.file_type().is_symlink()
    {
        return Err(SkillError::UnsafePath);
    }
    if file_meta.len() > MAX_SKILL_BYTES as u64 {
        return Err(SkillError::TooLarge);
    }

    let canonical = file
        .canonicalize()
        .map_err(|e| SkillError::Io(e.to_string()))?;
    if !canonical.starts_with(root) {
        return Err(SkillError::UnsafePath);
    }

    let bytes = fs::read(canonical).map_err(|e| SkillError::Io(e.to_string()))?;
    if bytes.len() > MAX_SKILL_BYTES {
        return Err(SkillError::TooLarge);
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| SkillError::InvalidUtf8)?;
    let (name, description) = metadata(text, id);

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
    let root = root
        .as_ref()
        .canonicalize()
        .map_err(|e| SkillError::InvalidRoot(e.to_string()))?;
    let skills = root.join("skills");
    let skills_meta =
        fs::symlink_metadata(&skills).map_err(|_| SkillError::MissingSkillsDirectory)?;

    if !skills_meta.is_dir() || skills_meta.file_type().is_symlink() {
        return Err(SkillError::MissingSkillsDirectory);
    }

    let mut ids = Vec::new();
    let entries = fs::read_dir(&skills).map_err(|e| SkillError::Io(e.to_string()))?;
    for entry in entries {
        let entry = entry.map_err(|e| SkillError::Io(e.to_string()))?;
        let file_type = entry
            .file_type()
            .map_err(|e| SkillError::Io(e.to_string()))?;
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
    ids.into_iter()
        .map(|id| read_descriptor(&root, &id))
        .collect()
}

/// Discover Pstack prompt-agent profiles in agents/<name>.md.
/// These are prompt definitions, not live AgentAdapter processes.
pub fn discover_agent_profiles(
    root: impl AsRef<Path>,
) -> Result<Vec<AgentProfileDescriptor>, SkillError> {
    let root = root
        .as_ref()
        .canonicalize()
        .map_err(|e| SkillError::InvalidRoot(e.to_string()))?;
    let agents = root.join("agents");
    let agents_meta = match fs::symlink_metadata(&agents) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(SkillError::Io(error.to_string())),
    };

    if !agents_meta.is_dir() || agents_meta.file_type().is_symlink() {
        return Err(SkillError::MissingSkillsDirectory);
    }

    let mut profiles = Vec::new();
    let entries = fs::read_dir(&agents).map_err(|e| SkillError::Io(e.to_string()))?;
    for entry in entries {
        let entry = entry.map_err(|e| SkillError::Io(e.to_string()))?;
        let kind = entry
            .file_type()
            .map_err(|e| SkillError::Io(e.to_string()))?;
        if !kind.is_file()
            || kind.is_symlink()
            || entry.path().extension().and_then(|value| value.to_str()) != Some("md")
        {
            continue;
        }

        let entry_path = entry.path();
        let Some(stem) = entry_path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        if !safe_id(stem) {
            continue;
        }

        let file_meta =
            fs::symlink_metadata(entry.path()).map_err(|e| SkillError::Io(e.to_string()))?;
        if file_meta.len() > MAX_SKILL_BYTES as u64 {
            return Err(SkillError::TooLarge);
        }

        let canonical = entry
            .path()
            .canonicalize()
            .map_err(|e| SkillError::Io(e.to_string()))?;
        if !canonical.starts_with(&root) {
            return Err(SkillError::UnsafePath);
        }

        let bytes = fs::read(canonical).map_err(|e| SkillError::Io(e.to_string()))?;
        if bytes.len() > MAX_SKILL_BYTES {
            return Err(SkillError::TooLarge);
        }
        let text = std::str::from_utf8(&bytes).map_err(|_| SkillError::InvalidUtf8)?;
        let (name, description) = metadata(text, stem);

        profiles.push(AgentProfileDescriptor {
            id: format!("pstack.agent.{stem}"),
            name,
            description,
            relative_path: format!("agents/{stem}.md"),
            bytes: bytes.len() as u64,
            fingerprint: fingerprint(&bytes),
        });
        if profiles.len() > MAX_SKILLS {
            return Err(SkillError::TooManySkills);
        }
    }

    profiles.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(profiles)
}

/// Load an exact Markdown agent profile as untrusted model context, never as executable code.
pub fn load_agent_profile_context(
    root: impl AsRef<Path>,
    capability_id: &str,
) -> Result<AgentProfileContext, SkillError> {
    let id = capability_id
        .strip_prefix("pstack.agent.")
        .ok_or(SkillError::InvalidId)?;
    if !safe_id(id) {
        return Err(SkillError::InvalidId);
    }

    let root = root
        .as_ref()
        .canonicalize()
        .map_err(|e| SkillError::InvalidRoot(e.to_string()))?;
    let path = root.join("agents").join(format!("{id}.md"));
    let file_meta = fs::symlink_metadata(&path).map_err(|e| SkillError::Io(e.to_string()))?;

    if !file_meta.is_file() || file_meta.file_type().is_symlink() {
        return Err(SkillError::UnsafePath);
    }
    if file_meta.len() > MAX_SKILL_BYTES as u64 {
        return Err(SkillError::TooLarge);
    }

    let canonical = path
        .canonicalize()
        .map_err(|e| SkillError::Io(e.to_string()))?;
    if !canonical.starts_with(&root) {
        return Err(SkillError::UnsafePath);
    }

    let bytes = fs::read(canonical).map_err(|e| SkillError::Io(e.to_string()))?;
    if bytes.len() > MAX_SKILL_BYTES {
        return Err(SkillError::TooLarge);
    }
    let text = String::from_utf8(bytes.clone()).map_err(|_| SkillError::InvalidUtf8)?;
    let (name, description) = metadata(&text, id);

    Ok(AgentProfileContext {
        descriptor: AgentProfileDescriptor {
            id: format!("pstack.agent.{id}"),
            name,
            description,
            relative_path: format!("agents/{id}.md"),
            bytes: bytes.len() as u64,
            fingerprint: fingerprint(&bytes),
        },
        content: text,
    })
}

/// Load one skill by its exact catalog id; the returned Markdown remains untrusted.
pub fn load_skill_context(
    root: impl AsRef<Path>,
    capability_id: &str,
) -> Result<SkillContext, SkillError> {
    let id = capability_id
        .strip_prefix("pstack.skill.")
        .ok_or(SkillError::InvalidId)?;
    if !safe_id(id) {
        return Err(SkillError::InvalidId);
    }

    let root = root
        .as_ref()
        .canonicalize()
        .map_err(|e| SkillError::InvalidRoot(e.to_string()))?;
    let descriptor = read_descriptor(&root, id)?;
    let relative_path = PathBuf::from(&descriptor.relative_path);
    if relative_path.is_absolute()
        || relative_path.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(SkillError::UnsafePath);
    }

    let path = root.join(relative_path);
    let metadata = fs::symlink_metadata(&path).map_err(|e| SkillError::Io(e.to_string()))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(SkillError::UnsafePath);
    }
    if metadata.len() > MAX_SKILL_BYTES as u64 {
        return Err(SkillError::TooLarge);
    }
    let canonical = path
        .canonicalize()
        .map_err(|e| SkillError::Io(e.to_string()))?;
    if !canonical.starts_with(&root) {
        return Err(SkillError::UnsafePath);
    }

    let bytes = fs::read(canonical).map_err(|e| SkillError::Io(e.to_string()))?;
    if bytes.len() > MAX_SKILL_BYTES {
        return Err(SkillError::TooLarge);
    }
    let text = String::from_utf8(bytes).map_err(|_| SkillError::InvalidUtf8)?;
    Ok(SkillContext {
        descriptor,
        content: text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    struct Temp(PathBuf);

    impl Temp {
        fn new() -> Self {
            let id = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!("dread-skills-{id}"));
            fs::create_dir_all(path.join("skills/example")).unwrap();
            Self(path)
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn discovers_metadata_without_executing_markdown() {
        let temp = Temp::new();
        fs::write(
            temp.0.join("skills/example/SKILL.md"),
            "---\nname: Example Skill\ndescription: Safe metadata\n---\n# instructions\n",
        )
        .unwrap();

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
        assert!(matches!(
            load_skill_context(&temp.0, "pstack.skill.../../../etc/passwd"),
            Err(SkillError::InvalidId)
        ));
    }

    #[test]
    fn rejects_oversized_skill_files() {
        let temp = Temp::new();
        fs::write(
            temp.0.join("skills/example/SKILL.md"),
            vec![b'x'; MAX_SKILL_BYTES + 1],
        )
        .unwrap();

        assert_eq!(discover_skills(&temp.0), Err(SkillError::TooLarge));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_skill_files() {
        use std::os::unix::fs::symlink;

        let temp = Temp::new();
        let outside = temp.0.join("outside.md");
        fs::write(&outside, "# outside").unwrap();
        symlink(&outside, temp.0.join("skills/example/SKILL.md")).unwrap();

        assert_eq!(discover_skills(&temp.0), Err(SkillError::UnsafePath));
    }

    #[test]
    fn discovers_and_loads_markdown_agent_profiles_as_data() {
        let temp = Temp::new();
        fs::create_dir_all(temp.0.join("agents")).unwrap();
        fs::write(
            temp.0.join("agents/poteto-agent.md"),
            "---\nname: Poteto Agent\ndescription: Review profile\n---\n# instructions\n",
        )
        .unwrap();

        let profiles = discover_agent_profiles(&temp.0).unwrap();
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].id, "pstack.agent.poteto-agent");

        let loaded = load_agent_profile_context(&temp.0, "pstack.agent.poteto-agent").unwrap();
        assert_eq!(loaded.descriptor.name, "Poteto Agent");
        assert!(loaded.content.contains("# instructions"));
    }

    #[test]
    fn rejects_missing_catalog_root() {
        let temp = Temp::new();
        assert!(matches!(
            discover_skills(temp.0.join("missing")),
            Err(SkillError::InvalidRoot(_))
        ));
    }
}
