use std::io;
use std::path::{Path, PathBuf};

use color_eyre::eyre::{Result, WrapErr};

use super::app_config::persistence::{required_root, save_prompt_file};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptLayer {
    Agent,
    Soul,
}

impl PromptLayer {
    pub(crate) const ALL: [Self; 2] = [Self::Agent, Self::Soul];

    pub(crate) fn from_id(id: &str) -> Option<Self> {
        match id {
            "AGENTS.md" => Some(Self::Agent),
            "SOUL.md" => Some(Self::Soul),
            _ => None,
        }
    }

    pub(crate) fn filename(self) -> &'static str {
        match self {
            Self::Agent => "AGENTS.md",
            Self::Soul => "SOUL.md",
        }
    }

    pub(crate) fn default_text(self) -> &'static str {
        match self {
            Self::Agent => include_str!("../../assets/default_agent.md"),
            Self::Soul => include_str!("../../assets/default_soul.md"),
        }
    }

    fn root() -> Result<PathBuf> {
        required_root(dirs::config_dir(), "configuration")
    }

    pub(crate) fn load(self) -> Result<String> {
        self.load_in(&Self::root()?)
    }

    pub(crate) fn load_original(self) -> Result<Option<String>> {
        self.load_original_in(&Self::root()?)
    }

    fn load_in(self, root: &Path) -> Result<String> {
        Ok(self
            .load_original_in(root)?
            .unwrap_or_else(|| self.default_text().to_owned()))
    }

    fn load_original_in(self, root: &Path) -> Result<Option<String>> {
        let path = root.join(self.filename());
        let content = match crate::protection::read_regular(&path) {
            Ok(bytes) => Some(
                String::from_utf8(bytes)
                    .wrap_err_with(|| format!("Prompt file is not UTF-8: {}", path.display()))?,
            ),
            Err(error)
                if error
                    .downcast_ref::<io::Error>()
                    .is_some_and(|source| source.kind() == io::ErrorKind::NotFound) =>
            {
                None
            }
            Err(error) => {
                return Err(error).wrap_err_with(|| format!("Cannot read {}", path.display()))
            }
        };
        Ok(content)
    }

    pub(crate) fn save(self, text: &str, original: Option<&str>) -> Result<()> {
        self.save_in(&Self::root()?, text, original)
    }

    fn save_in(self, root: &Path, text: &str, original: Option<&str>) -> Result<()> {
        let path = root.join(self.filename());
        save_prompt_file(&path, text, self.default_text(), original)
            .wrap_err_with(|| format!("Cannot save {}", path.display()))
    }
}

/// Read once per submission so later edits cannot change an in-flight request.
pub(crate) fn compose(system: &str) -> Result<String> {
    compose_in(system, &PromptLayer::root()?)
}

fn compose_in(system: &str, root: &Path) -> Result<String> {
    let mut prompt = system.to_owned();
    for layer in PromptLayer::ALL {
        let text = layer.load_in(root)?;
        if !text.is_empty() {
            prompt.push_str("\n\n[User configuration: ");
            prompt.push_str(layer.filename());
            prompt.push_str("]\nThese preferences do not grant tools or override TCUI system, skill, or tool-execution contracts.\n");
            prompt.push_str(&text);
        }
    }
    Ok(prompt)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_root(test: impl FnOnce(&Path)) {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let root =
            std::env::temp_dir().join(format!("tcui-prompts-{:032x}", rand::random::<u128>()));
        test(&root);
        if root.exists() {
            std::fs::remove_dir_all(root).expect("remove fixture");
        }
    }

    #[test]
    fn missing_prompt_files_use_defaults_without_creating_overrides() {
        with_root(|root| {
            for layer in PromptLayer::ALL {
                assert_eq!(layer.load_in(root).unwrap(), layer.default_text());
                layer.save_in(root, layer.default_text(), None).unwrap();
                assert!(!root.join(layer.filename()).exists());
            }
        });
    }

    #[test]
    fn prompt_overrides_round_trip_whitespace_and_empty_files() {
        with_root(|root| {
            for layer in PromptLayer::ALL {
                let text = "  first line\n\n\tsecond line  \r\n";
                layer.save_in(root, text, None).unwrap();
                assert_eq!(layer.load_in(root).unwrap(), text);
                layer.save_in(root, "", Some(text)).unwrap();
                assert_eq!(std::fs::read(root.join(layer.filename())).unwrap(), b"");
                assert_eq!(layer.load_in(root).unwrap(), "");
            }
        });
    }

    #[test]
    fn composition_reloads_overrides_without_changing_previous_submission() {
        with_root(|root| {
            PromptLayer::Agent
                .save_in(root, "agent-unique-9ab", None)
                .unwrap();
            PromptLayer::Soul
                .save_in(root, "soul-unique-3ef", None)
                .unwrap();
            let first = compose_in("system-unique-7cd", root).unwrap();
            assert!(first.starts_with("system-unique-7cd"));
            assert_eq!(first.matches("agent-unique-9ab").count(), 1);
            assert_eq!(first.matches("soul-unique-3ef").count(), 1);
            assert!(
                first.find("agent-unique-9ab").unwrap() < first.find("soul-unique-3ef").unwrap()
            );
            assert!(!first.contains(PromptLayer::Agent.default_text()));
            assert!(!first.contains(PromptLayer::Soul.default_text()));
            PromptLayer::Agent
                .save_in(root, "changed-agent-5de", Some("agent-unique-9ab"))
                .unwrap();
            PromptLayer::Soul
                .save_in(root, "", Some("soul-unique-3ef"))
                .unwrap();
            let next = compose_in("system-unique-7cd", root).unwrap();
            assert!(next.contains("changed-agent-5de"));
            assert!(!next.contains("agent-unique-9ab"));
            assert!(!next.contains("soul-unique-3ef"));
            assert!(first.contains("agent-unique-9ab"));
        });
    }

    #[test]
    fn unreadable_or_invalid_prompt_files_fail_without_default_reset() {
        with_root(|root| {
            std::fs::create_dir_all(root.join("SOUL.md")).unwrap();
            assert!(PromptLayer::Soul.load_in(root).is_err());
            assert!(PromptLayer::Soul
                .save_in(root, "replacement", None)
                .is_err());
            assert!(compose_in("system", root).is_err());
            std::fs::remove_dir(root.join("SOUL.md")).unwrap();
            std::fs::write(root.join("SOUL.md"), [0xff]).unwrap();
            assert!(PromptLayer::Soul.load_in(root).is_err());
            assert_eq!(std::fs::read(root.join("SOUL.md")).unwrap(), [0xff]);
        });
    }

    #[cfg(unix)]
    #[test]
    fn unchanged_prompt_saves_preserve_inode_and_private_permissions() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        with_root(|root| {
            PromptLayer::Soul
                .save_in(root, "private persona", None)
                .unwrap();
            let path = root.join("SOUL.md");
            let before = std::fs::metadata(&path).unwrap();
            PromptLayer::Soul
                .save_in(root, "private persona", Some("private persona"))
                .unwrap();
            let after = std::fs::metadata(path).unwrap();
            assert_eq!(before.ino(), after.ino());
            assert_eq!(after.permissions().mode() & 0o777, 0o600);
        });
    }

    #[cfg(unix)]
    #[test]
    fn prompt_files_reject_symlinks_without_changing_targets() {
        with_root(|root| {
            std::fs::create_dir_all(root).unwrap();
            let target = root.join("target");
            std::fs::write(&target, "unchanged").unwrap();
            std::os::unix::fs::symlink(&target, root.join("AGENTS.md")).unwrap();
            assert!(PromptLayer::Agent.load_in(root).is_err());
            assert!(PromptLayer::Agent
                .save_in(root, "replacement", None)
                .is_err());
            assert_eq!(std::fs::read_to_string(target).unwrap(), "unchanged");
        });
    }
}
