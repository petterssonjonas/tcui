use std::path::{Component, Path, PathBuf};

use crate::storage::paths::{ensure_directory, TcuiDataPaths};
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum PathError {
    #[error("memory path must be a relative Markdown path")]
    Invalid,
    #[error("memory path escapes the vault")]
    Escape,
    #[error("memory path is unavailable: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConfinedPath {
    pub(crate) relative: PathBuf,
    pub(crate) absolute: PathBuf,
}

#[derive(Debug, Clone)]
pub(crate) struct MemoryPaths {
    vault: PathBuf,
    legacy_memories: PathBuf,
    memories: PathBuf,
    trash: PathBuf,
}

impl MemoryPaths {
    pub(crate) fn new(vault: &Path) -> Result<Self, PathError> {
        let vault = std::fs::canonicalize(vault)?;
        let legacy_memories = vault.join("memories");
        std::fs::create_dir_all(&legacy_memories)?;
        let legacy_memories = std::fs::canonicalize(legacy_memories)?;
        if !legacy_memories.starts_with(&vault) {
            return Err(PathError::Escape);
        }
        let storage = TcuiDataPaths::discover();
        ensure_directory(&storage.memories_dir)?;
        ensure_directory(&storage.memories_trash_dir)?;
        Ok(Self {
            vault,
            legacy_memories,
            memories: storage.memories_dir,
            trash: storage.memories_trash_dir,
        })
    }

    pub(crate) fn vault(&self) -> &Path {
        &self.vault
    }

    pub(crate) fn root(&self) -> &Path {
        &self.memories
    }

    pub(crate) fn legacy_root(&self) -> &Path {
        &self.legacy_memories
    }

    pub(crate) fn logical_path(&self, path: &Path) -> Result<PathBuf, PathError> {
        if path.is_absolute() {
            return Err(PathError::Invalid);
        }
        let relative = path.strip_prefix("memories").unwrap_or(path);
        if relative.as_os_str().is_empty()
            || relative.extension().and_then(|value| value.to_str()) != Some("md")
            || relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(PathError::Invalid);
        }

        let mut cursor = self.legacy_memories.clone();
        for component in relative.components() {
            let Component::Normal(part) = component else {
                return Err(PathError::Invalid);
            };
            cursor.push(part);
            match std::fs::symlink_metadata(&cursor) {
                Ok(metadata) if metadata.file_type().is_symlink() => return Err(PathError::Escape),
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
                Err(error) => return Err(error.into()),
            }
        }
        Ok(relative.to_path_buf())
    }

    pub(crate) fn legacy_target(&self, path: &Path) -> Result<ConfinedPath, PathError> {
        let relative = self.logical_path(path)?;
        let absolute = self.legacy_memories.join(&relative);
        let canonical = std::fs::canonicalize(&absolute)?;
        if !canonical.starts_with(&self.legacy_memories) {
            return Err(PathError::Escape);
        }
        Ok(ConfinedPath { relative, absolute })
    }

    pub(crate) fn active_document_path(&self, id: u64) -> PathBuf {
        self.memories.join(format!("{id:016x}.tcui-memory"))
    }

    pub(crate) fn trash_document_path(&self, id: u64) -> PathBuf {
        self.trash.join(format!("{id:016x}.tcui-memory"))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::MemoryPaths;

    #[test]
    fn paths_reject_absolute_parent_and_non_markdown_targets() {
        // Given
        let root =
            std::env::temp_dir().join(format!("tcui-memory-paths-{}", rand::random::<u64>()));
        fs::create_dir_all(&root).expect("temporary vault");
        let paths = MemoryPaths::new(&root).expect("memory paths");

        // When / Then
        assert!(paths
            .logical_path(std::path::Path::new("/tmp/outside.md"))
            .is_err());
        assert!(paths
            .logical_path(std::path::Path::new("../outside.md"))
            .is_err());
        assert!(paths
            .logical_path(std::path::Path::new("note.txt"))
            .is_err());
        assert!(paths
            .logical_path(std::path::Path::new("nested/note.md"))
            .is_ok());
        fs::remove_dir_all(root).expect("temporary vault cleanup");
    }

    #[cfg(unix)]
    #[test]
    fn paths_reject_symlink_escape() {
        use std::os::unix::fs::symlink;

        // Given
        let root = std::env::temp_dir().join(format!("tcui-memory-root-{}", rand::random::<u64>()));
        let outside =
            std::env::temp_dir().join(format!("tcui-memory-outside-{}", rand::random::<u64>()));
        fs::create_dir_all(root.join("memories")).expect("memory root");
        fs::create_dir_all(&outside).expect("outside root");
        symlink(&outside, root.join("memories/escaped")).expect("escape symlink");
        let paths = MemoryPaths::new(&root).expect("memory paths");

        // When
        let result = paths.logical_path(std::path::Path::new("escaped/note.md"));

        // Then
        fs::remove_dir_all(&root).expect("root cleanup");
        fs::remove_dir_all(&outside).expect("outside cleanup");
        assert!(result.is_err());
    }
}
