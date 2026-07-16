use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Component, Path, PathBuf};

use cap_fs_ext::{DirExt, FollowSymlinks, MetadataExt as _, OpenOptionsFollowExt};
use cap_std::fs::{Dir, File, OpenOptions};

use super::VaultRelativePath;

pub struct ValidatedEditorTarget {
    absolute: PathBuf,
    identity: FileIdentity,
}

impl ValidatedEditorTarget {
    pub(crate) fn from_trusted_path(path: &Path) -> Result<Self, EditorTargetError> {
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|source| spawn_io_error("resolve", source))?
                .join(path)
        };
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        let file = File::open_ambient_with(&absolute, &options, cap_std::ambient_authority())
            .map_err(|source| classify_spawn_error(&absolute, source))?;
        let metadata = file
            .metadata()
            .map_err(|source| spawn_io_error("read identity for", source))?;
        if !metadata.is_file() {
            return Err(EditorTargetError::SpawnNotFile);
        }
        Ok(Self {
            absolute,
            identity: FileIdentity::from_metadata(&metadata),
        })
    }

    pub(crate) fn revalidate_for_spawn(self) -> Result<PathBuf, EditorTargetError> {
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        let file = File::open_ambient_with(&self.absolute, &options, cap_std::ambient_authority())
            .map_err(|source| classify_spawn_error(&self.absolute, source))?;
        let metadata = file
            .metadata()
            .map_err(|source| spawn_io_error("re-read identity for", source))?;
        if !metadata.is_file() {
            return Err(EditorTargetError::SpawnNotFile);
        }
        if self.identity != FileIdentity::from_metadata(&metadata) {
            return Err(EditorTargetError::SpawnIdentityChanged);
        }
        Ok(self.absolute)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
    device: u64,
    inode: u64,
}

impl FileIdentity {
    fn from_metadata(metadata: &cap_std::fs::Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EditorTargetError {
    #[error("Vault editor target points through a symbolic link: {path}")]
    Symlink { path: PathBuf },
    #[error("Vault editor target is not a regular file: {path}")]
    NotFile { path: PathBuf },
    #[error("Vault editor target identity changed during validation: {path}")]
    IdentityChanged { path: PathBuf },
    #[error("failed to {operation} Vault editor target {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Editor target became a symbolic link before spawn")]
    SpawnSymlink,
    #[error("Editor target is not a regular file before spawn")]
    SpawnNotFile,
    #[error("Editor target identity changed before spawn")]
    SpawnIdentityChanged,
    #[error("failed to {operation} editor target before spawn: {source}")]
    SpawnIo {
        operation: &'static str,
        #[source]
        source: io::Error,
    },
}

pub(crate) fn validate_editor_target(
    canonical_root: &Path,
    held_root: &Dir,
    path: &VaultRelativePath,
) -> Result<ValidatedEditorTarget, EditorTargetError> {
    validate_editor_target_with_hook(canonical_root, held_root, path, || {})
}

fn validate_editor_target_with_hook(
    canonical_root: &Path,
    held_root: &Dir,
    path: &VaultRelativePath,
    before_ambient_open: impl FnOnce(),
) -> Result<ValidatedEditorTarget, EditorTargetError> {
    let current_root = Dir::open_ambient_dir(canonical_root, cap_std::ambient_authority())
        .map_err(|source| io_error("re-open root for", path.as_path(), source))?;
    let held_identity = held_root
        .metadata(".")
        .map_err(|source| io_error("read held root identity for", path.as_path(), source))?;
    let current_identity = current_root
        .metadata(".")
        .map_err(|source| io_error("read current root identity for", path.as_path(), source))?;
    if !same_identity(&held_identity, &current_identity) {
        return Err(EditorTargetError::IdentityChanged {
            path: path.as_path().to_path_buf(),
        });
    }

    let (parent, name) = open_parent(&current_root, path.as_path())?;
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let file = parent
        .open_with(&name, &options)
        .map_err(|source| classify_open_error(&parent, &name, path.as_path(), "open", source))?;
    let capability_identity = file
        .metadata()
        .map_err(|source| io_error("read capability identity for", path.as_path(), source))?;
    if !capability_identity.is_file() {
        return Err(EditorTargetError::NotFile {
            path: path.as_path().to_path_buf(),
        });
    }

    before_ambient_open();
    let absolute = canonical_root.join(path.as_path());
    let ambient = File::open_ambient_with(&absolute, &options, cap_std::ambient_authority())
        .map_err(|source| classify_ambient_error(&absolute, path.as_path(), source))?;
    let ambient_identity = ambient
        .metadata()
        .map_err(|source| io_error("read ambient identity for", path.as_path(), source))?;
    if !same_identity(&capability_identity, &ambient_identity) {
        return Err(EditorTargetError::IdentityChanged {
            path: path.as_path().to_path_buf(),
        });
    }
    Ok(ValidatedEditorTarget {
        absolute,
        identity: FileIdentity::from_metadata(&ambient_identity),
    })
}

fn open_parent(root: &Dir, path: &Path) -> Result<(Dir, OsString), EditorTargetError> {
    let mut directory = root
        .try_clone()
        .map_err(|source| io_error("clone root for", path, source))?;
    let mut components = path.components().peekable();
    let mut traversed = PathBuf::new();
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
            return Err(EditorTargetError::IdentityChanged {
                path: path.to_path_buf(),
            });
        };
        if components.peek().is_none() {
            return Ok((directory, name.to_os_string()));
        }
        traversed.push(name);
        directory = directory.open_dir_nofollow(name).map_err(|source| {
            classify_open_error(&directory, name, &traversed, "open directory", source)
        })?;
    }
    Err(EditorTargetError::IdentityChanged {
        path: path.to_path_buf(),
    })
}

fn classify_open_error(
    directory: &Dir,
    name: &OsStr,
    path: &Path,
    operation: &'static str,
    source: io::Error,
) -> EditorTargetError {
    match directory.symlink_metadata(name) {
        Ok(metadata) if metadata.is_symlink() => EditorTargetError::Symlink {
            path: path.to_path_buf(),
        },
        Ok(_) | Err(_) => io_error(operation, path, source),
    }
}

fn classify_ambient_error(
    absolute: &Path,
    relative: &Path,
    source: io::Error,
) -> EditorTargetError {
    match std::fs::symlink_metadata(absolute) {
        Ok(metadata) if metadata.file_type().is_symlink() => EditorTargetError::Symlink {
            path: relative.to_path_buf(),
        },
        Ok(_) | Err(_) => io_error("re-open ambient", relative, source),
    }
}

fn same_identity(left: &cap_std::fs::Metadata, right: &cap_std::fs::Metadata) -> bool {
    left.dev() == right.dev() && left.ino() == right.ino()
}

fn classify_spawn_error(absolute: &Path, source: io::Error) -> EditorTargetError {
    match std::fs::symlink_metadata(absolute) {
        Ok(metadata) if metadata.file_type().is_symlink() => EditorTargetError::SpawnSymlink,
        Ok(_) | Err(_) => spawn_io_error("open", source),
    }
}

fn spawn_io_error(operation: &'static str, source: io::Error) -> EditorTargetError {
    EditorTargetError::SpawnIo { operation, source }
}

fn io_error(operation: &'static str, path: &Path, source: io::Error) -> EditorTargetError {
    EditorTargetError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::symlink;

    use cap_std::fs::Dir;

    use super::{validate_editor_target_with_hook, EditorTargetError};
    use crate::vault::VaultRelativePath;

    #[cfg(unix)]
    #[test]
    fn vault_confined_write_editor_revalidation_rejects_final_symlink_swap() {
        // Given
        let base = std::env::temp_dir().join(format!(
            "tcui-vault-editor-swap-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let root = base.join("vault");
        let outside = base.join("outside.md");
        fs::create_dir_all(&root).expect("create vault root");
        fs::write(root.join("note.md"), b"inside").expect("write inside note");
        fs::write(&outside, b"outside-sentinel").expect("write outside sentinel");
        let directory = Dir::open_ambient_dir(&root, cap_std::ambient_authority())
            .expect("open capability root");
        let path =
            VaultRelativePath::parse(std::path::Path::new("note.md")).expect("valid vault path");

        // When
        let result = validate_editor_target_with_hook(&root, &directory, &path, || {
            fs::remove_file(root.join("note.md")).expect("remove original note");
            symlink(&outside, root.join("note.md")).expect("install raced symlink");
        });

        // Then
        assert!(matches!(
            result,
            Err(EditorTargetError::Symlink { .. } | EditorTargetError::IdentityChanged { .. })
        ));
        assert_eq!(
            fs::read(&outside).expect("read outside sentinel"),
            b"outside-sentinel"
        );
        fs::remove_dir_all(base).expect("remove editor test root");
    }
}
