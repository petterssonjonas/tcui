use std::path::{Component, Path, PathBuf};

use cap_std::fs::Dir;
use rustix::fs::{fchmod, fsync, mkdirat, openat, Mode, OFlags};

use crate::storage::paths::TcuiDataPaths;
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

#[derive(Debug, Clone)]
pub(crate) struct MemoryPaths {
    vault: PathBuf,
    legacy_memories: PathBuf,
    memories: PathBuf,
    trash: PathBuf,
}

impl MemoryPaths {
    #[cfg(test)]
    pub(crate) fn new(vault: &Path) -> Result<Self, PathError> {
        let vault = std::fs::canonicalize(vault)?;
        let legacy_memories = vault.join("memories");
        if legacy_memories.exists() {
            let canonical = std::fs::canonicalize(&legacy_memories)?;
            if !canonical.starts_with(&vault) {
                return Err(PathError::Escape);
            }
        }
        let storage = TcuiDataPaths::discover();
        Self::storage_directories(&storage.memories_dir, true)?;
        Ok(Self {
            vault,
            legacy_memories,
            memories: storage.memories_dir,
            trash: storage.memories_trash_dir,
        })
    }

    pub(crate) fn open_default() -> Result<Self, PathError> {
        let storage = TcuiDataPaths::discover();
        Self::storage_directories(&storage.memories_dir, true)?;
        Ok(Self {
            vault: storage.root.clone(),
            legacy_memories: storage.memories_dir.clone(),
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

    /// Opens both native directories without following any path component.
    /// Mutations retain these handles instead of resolving `.trash` again.
    pub(super) fn directories(&self) -> Result<(Dir, Dir), PathError> {
        Self::storage_directories(&self.memories, false)
    }

    fn storage_directories(root: &Path, create: bool) -> Result<(Dir, Dir), PathError> {
        let root = crate::protection::open_native_directory(root, create)?;
        let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        let descriptor = match openat(&root, ".trash", flags, Mode::empty()) {
            Ok(descriptor) => descriptor,
            Err(rustix::io::Errno::NOENT) if create => {
                match mkdirat(&root, ".trash", Mode::from_bits_truncate(0o700)) {
                    Ok(()) => fsync(&root).map_err(std::io::Error::from)?,
                    Err(rustix::io::Errno::EXIST) => {}
                    Err(error) => return Err(std::io::Error::from(error).into()),
                }
                openat(&root, ".trash", flags, Mode::empty()).map_err(std::io::Error::from)?
            }
            Err(error) => return Err(std::io::Error::from(error).into()),
        };
        let trash = Dir::from(descriptor);
        if create {
            // Validate both handles before changing either directory's mode.
            fchmod(&root, Mode::from_bits_truncate(0o700)).map_err(std::io::Error::from)?;
            fchmod(&trash, Mode::from_bits_truncate(0o700)).map_err(std::io::Error::from)?;
        }
        Ok((root, trash))
    }

    pub(super) fn validate_logical_path(path: &Path) -> Result<(), PathError> {
        if path.as_os_str().is_empty()
            || path.extension().and_then(|value| value.to_str()) != Some("md")
            || path
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(PathError::Invalid);
        }
        Ok(())
    }

    pub(crate) fn logical_path(&self, path: &Path) -> Result<PathBuf, PathError> {
        if path.is_absolute() {
            return Err(PathError::Invalid);
        }
        let relative = path.strip_prefix("memories").unwrap_or(path);
        Self::validate_logical_path(relative)?;

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
        let _guard = crate::test_support::env_lock()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root =
            std::env::temp_dir().join(format!("tcui-memory-paths-{}", rand::random::<u64>()));
        fs::create_dir_all(&root).expect("temporary vault");
        std::env::set_var("XDG_DATA_HOME", root.join("data-home"));
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
        std::env::remove_var("XDG_DATA_HOME");
    }

    #[cfg(unix)]
    #[test]
    fn paths_reject_symlink_escape() {
        use std::os::unix::fs::symlink;

        // Given
        let _guard = crate::test_support::env_lock()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = std::env::temp_dir().join(format!("tcui-memory-root-{}", rand::random::<u64>()));
        let outside =
            std::env::temp_dir().join(format!("tcui-memory-outside-{}", rand::random::<u64>()));
        fs::create_dir_all(root.join("memories")).expect("memory root");
        std::env::set_var("XDG_DATA_HOME", root.join("data-home"));
        fs::create_dir_all(&outside).expect("outside root");
        symlink(&outside, root.join("memories/escaped")).expect("escape symlink");
        let paths = MemoryPaths::new(&root).expect("memory paths");

        // When
        let result = paths.logical_path(std::path::Path::new("escaped/note.md"));

        // Then
        fs::remove_dir_all(&root).expect("root cleanup");
        fs::remove_dir_all(&outside).expect("outside cleanup");
        assert!(result.is_err());
        std::env::remove_var("XDG_DATA_HOME");
    }
}
