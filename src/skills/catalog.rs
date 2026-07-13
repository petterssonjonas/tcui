use std::borrow::Cow;
use std::collections::HashSet;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::builtins::{self, BUILTINS};
use super::frontmatter::read_metadata;
use super::{Skill, SkillMeta, SkillOrigin, mentions};

#[derive(Debug)]
pub struct SkillCatalog {
    skills: Vec<SkillMeta>,
}

impl SkillCatalog {
    pub fn discover() -> io::Result<Self> {
        let repo_skills = env::current_dir()?.join("skills");
        let home = env::var_os("HOME").map(PathBuf::from);
        Self::discover_from(&repo_skills, home.as_deref())
    }

    pub fn discover_from(repo_skills: &Path, home: Option<&Path>) -> io::Result<Self> {
        let mut catalog = Self {
            skills: BUILTINS
                .iter()
                .map(|builtin| SkillMeta {
                    name: builtin.name.to_owned(),
                    description: builtin.description.to_owned(),
                    origin: SkillOrigin::Builtin,
                })
                .collect(),
        };
        let mut names = catalog
            .skills
            .iter()
            .map(|skill| skill.name.clone())
            .collect::<HashSet<_>>();

        catalog.scan_root(repo_skills, &mut names)?;
        if let Some(home) = home {
            for root in [
                home.join(".config/tcui/skills"),
                home.join(".codex/skills"),
                home.join(".config/opencode/skills"),
            ] {
                catalog.scan_root(&root, &mut names)?;
            }
        }
        Ok(catalog)
    }

    pub fn list(&self) -> &[SkillMeta] {
        &self.skills
    }

    pub fn find(&self, name: &str) -> Option<&SkillMeta> {
        self.skills.iter().find(|skill| skill.name == name)
    }

    pub fn load(&self, name: &str) -> io::Result<Option<Skill>> {
        let Some(metadata) = self.find(name) else {
            return Ok(None);
        };
        let source = match &metadata.origin {
            SkillOrigin::Builtin => builtins::source(name).map(Cow::Borrowed).ok_or_else(|| {
                io::Error::other(format!("missing built-in skill source: {name}"))
            })?,
            SkillOrigin::External(path) => Cow::Owned(fs::read_to_string(path)?),
        };
        Ok(Some(Skill {
            name: metadata.name.clone(),
            description: metadata.description.clone(),
            source,
            origin: metadata.origin.clone(),
        }))
    }

    pub fn load_mentions(&self, text: &str) -> io::Result<Vec<Skill>> {
        let mut loaded = Vec::new();
        for name in mentions(text) {
            if let Some(skill) = self.load(&name)? {
                loaded.push(skill);
            }
        }
        Ok(loaded)
    }

    fn scan_root(&mut self, root: &Path, names: &mut HashSet<String>) -> io::Result<()> {
        let entries = match fs::read_dir(root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        let canonical_root = fs::canonicalize(root)?;
        let mut paths = entries
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<io::Result<Vec<_>>>()?;
        paths.sort();

        for directory in paths {
            let Some(fallback_name) = directory.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let skill_path = directory.join("SKILL.md");
            if !skill_path.is_file() {
                continue;
            }
            let canonical_skill = fs::canonicalize(&skill_path)?;
            if !canonical_skill.starts_with(&canonical_root) {
                continue;
            }
            let metadata = read_metadata(&canonical_skill, fallback_name)?;
            if memory_skill_name_is_disabled(&metadata.name) {
                continue;
            }
            if names.insert(metadata.name.clone()) {
                self.skills.push(SkillMeta {
                    name: metadata.name,
                    description: metadata.description,
                    origin: SkillOrigin::External(canonical_skill),
                });
            }
        }
        Ok(())
    }
}

fn memory_skill_name_is_disabled(name: &str) -> bool {
    #[cfg(feature = "memory")]
    {
        let _ = name;
        false
    }
    #[cfg(not(feature = "memory"))]
    {
        matches!(name, "remember" | "memory" | "memorize")
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{SkillCatalog, SkillOrigin};

    #[test]
    fn catalog_loads_embedded_builtins_for_explicit_mentions() {
        // Given
        let catalog =
            SkillCatalog::discover_from(Path::new("skills"), None).expect("catalog discovery");

        // When
        let loaded = catalog
            .load_mentions("Use @websearch, @research, and @save.")
            .expect("built-in loading");

        // Then
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded[0].name, "websearch");
        assert_eq!(loaded[1].name, "research");
        assert_eq!(loaded[2].name, "save");
        assert!(
            loaded
                .iter()
                .all(|skill| skill.origin == SkillOrigin::Builtin)
        );
        assert!(loaded.iter().all(|skill| !skill.source.is_empty()));
    }

    #[cfg(feature = "memory")]
    #[test]
    fn catalog_exposes_memory_builtins() {
        // Given
        let catalog =
            SkillCatalog::discover_from(Path::new("skills"), None).expect("catalog discovery");

        // When
        let loaded = catalog
            .load_mentions("@remember concise answers. @memory search preferences @memorize status")
            .expect("memory skill loading");

        // Then
        assert_eq!(
            loaded
                .iter()
                .map(|skill| skill.name.as_str())
                .collect::<Vec<_>>(),
            ["remember", "memory", "memorize"]
        );
    }

    #[cfg(not(feature = "memory"))]
    #[test]
    fn catalog_omits_memory_builtins_when_feature_is_disabled() {
        let catalog =
            SkillCatalog::discover_from(Path::new("skills"), None).expect("catalog discovery");

        let loaded = catalog
            .load_mentions("@remember concise answers. @memory search preferences @memorize status")
            .expect("catalog loading");

        assert!(loaded.is_empty());
        assert!(catalog.find("remember").is_none());
        assert!(catalog.find("memory").is_none());
        assert!(catalog.find("memorize").is_none());
    }

    #[test]
    fn catalog_discovery_reads_only_external_frontmatter() {
        // Given
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("tcui-skills-{nonce}"));
        let skill_dir = root.join("external");
        fs::create_dir_all(&skill_dir).expect("temporary skill directory");
        fs::write(
            skill_dir.join("SKILL.md"),
            b"---\nname: external\ndescription: External metadata.\n---\n\xff",
        )
        .expect("temporary skill");

        // When
        let catalog = SkillCatalog::discover_from(&root, None).expect("metadata discovery");

        // Then
        let metadata = catalog.find("external").cloned();
        fs::remove_dir_all(root).expect("temporary skill cleanup");
        assert_eq!(
            metadata.map(|skill| skill.description),
            Some("External metadata.".to_owned())
        );
    }

    #[cfg(unix)]
    #[test]
    fn catalog_rejects_skill_symlinks_outside_the_root() {
        use std::os::unix::fs::symlink;

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("tcui-skills-root-{nonce}"));
        let outside = std::env::temp_dir().join(format!("tcui-skills-outside-{nonce}"));
        fs::create_dir_all(root.join("escaped")).expect("skill directory");
        fs::create_dir_all(&outside).expect("outside directory");
        fs::write(
            outside.join("SKILL.md"),
            "---\nname: escaped\ndescription: Must not load.\n---\nsecret",
        )
        .expect("outside skill");
        symlink(outside.join("SKILL.md"), root.join("escaped/SKILL.md")).expect("skill symlink");

        let catalog = SkillCatalog::discover_from(&root, None).expect("catalog discovery");

        fs::remove_dir_all(root).expect("root cleanup");
        fs::remove_dir_all(outside).expect("outside cleanup");
        assert!(catalog.find("escaped").is_none());
    }
}
