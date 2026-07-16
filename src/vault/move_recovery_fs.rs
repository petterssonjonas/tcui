use std::ffi::{OsStr, OsString};
use std::io::{self, Read, Write};
use std::path::Component;

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};

use crate::config::vault_profiles::VaultCatalog;

use super::move_journal::MoveJournalFileRef;
use super::VaultRelativePath;

#[derive(Debug, thiserror::Error)]
pub(super) enum RecoveryFsError {
    #[error("Vault profile is unavailable for recovery path {path:?}")]
    UnknownProfile { path: VaultRelativePath },
    #[error("Vault recovery path conflicts with unexpected data: {path:?}")]
    Conflict { path: VaultRelativePath },
    #[error("failed to {operation} Vault recovery path {path:?}: {source}")]
    Io {
        operation: &'static str,
        path: VaultRelativePath,
        #[source]
        source: io::Error,
    },
}

impl RecoveryFsError {
    pub(super) const fn path(&self) -> &VaultRelativePath {
        match self {
            Self::UnknownProfile { path } | Self::Conflict { path } | Self::Io { path, .. } => path,
        }
    }
}

pub(super) fn read_optional(
    catalog: &VaultCatalog,
    target: &MoveJournalFileRef,
) -> Result<Option<Vec<u8>>, RecoveryFsError> {
    let root = open_profile(catalog, target)?;
    let (parent, name) = open_parent(&root, &target.path)?;
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let mut file = match parent.open_with(&name, &options) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return match parent.symlink_metadata(&name) {
                Ok(_) => Err(RecoveryFsError::Conflict {
                    path: target.path.clone(),
                }),
                Err(missing) if missing.kind() == io::ErrorKind::NotFound => Ok(None),
                Err(source) => Err(io_error("inspect", &target.path, source)),
            };
        }
        Err(source) => return Err(io_error("open", &target.path, source)),
    };
    let metadata = file
        .metadata()
        .map_err(|source| io_error("inspect", &target.path, source))?;
    if !metadata.is_file() {
        return Err(RecoveryFsError::Conflict {
            path: target.path.clone(),
        });
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|source| io_error("read", &target.path, source))?;
    Ok(Some(bytes))
}

pub(super) fn replace_file(
    catalog: &VaultCatalog,
    target: &MoveJournalFileRef,
    bytes: &[u8],
) -> Result<(), RecoveryFsError> {
    let root = open_profile(catalog, target)?;
    let (parent, name) = open_parent(&root, &target.path)?;
    if parent
        .symlink_metadata(&name)
        .is_ok_and(|metadata| metadata.is_symlink())
    {
        return Err(RecoveryFsError::Conflict {
            path: target.path.clone(),
        });
    }
    let (mut temporary, mut cleanup) = create_temporary(&parent, &target.path)?;
    temporary
        .write_all(bytes)
        .and_then(|()| temporary.flush())
        .and_then(|()| temporary.sync_all())
        .map_err(|source| io_error("write", &target.path, source))?;
    drop(temporary);
    parent
        .rename(cleanup.name(), &parent, &name)
        .map_err(|source| io_error("publish", &target.path, source))?;
    cleanup.disarm();
    sync_directory(&parent, &target.path)
}

pub(super) fn remove_if_matches(
    catalog: &VaultCatalog,
    target: &MoveJournalFileRef,
    expected: &[u8],
) -> Result<(), RecoveryFsError> {
    let Some(current) = read_optional(catalog, target)? else {
        return Ok(());
    };
    if current != expected {
        return Err(RecoveryFsError::Conflict {
            path: target.path.clone(),
        });
    }
    let root = open_profile(catalog, target)?;
    let (parent, name) = open_parent(&root, &target.path)?;
    parent
        .remove_file(&name)
        .map_err(|source| io_error("remove", &target.path, source))?;
    sync_directory(&parent, &target.path)
}

pub(super) fn remove_regular_if_present(
    catalog: &VaultCatalog,
    target: &MoveJournalFileRef,
) -> Result<(), RecoveryFsError> {
    if read_optional(catalog, target)?.is_none() {
        return Ok(());
    }
    let root = open_profile(catalog, target)?;
    let (parent, name) = open_parent(&root, &target.path)?;
    parent
        .remove_file(&name)
        .map_err(|source| io_error("remove", &target.path, source))?;
    sync_directory(&parent, &target.path)
}

fn open_profile(
    catalog: &VaultCatalog,
    target: &MoveJournalFileRef,
) -> Result<Dir, RecoveryFsError> {
    let profile =
        catalog
            .get(&target.profile_id)
            .ok_or_else(|| RecoveryFsError::UnknownProfile {
                path: target.path.clone(),
            })?;
    Dir::open_ambient_dir(&profile.root, cap_std::ambient_authority())
        .map_err(|source| io_error("open profile for", &target.path, source))
}

fn open_parent(root: &Dir, path: &VaultRelativePath) -> Result<(Dir, OsString), RecoveryFsError> {
    let mut directory = root
        .try_clone()
        .map_err(|source| io_error("clone root for", path, source))?;
    let mut components = path.as_path().components().peekable();
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
            return Err(invalid_path(path));
        };
        if components.peek().is_none() {
            return Ok((directory, name.to_os_string()));
        }
        directory = directory
            .open_dir_nofollow(name)
            .map_err(|source| io_error("open parent for", path, source))?;
    }
    Err(invalid_path(path))
}

struct TemporaryFile {
    directory: Dir,
    name: OsString,
    armed: bool,
}

impl TemporaryFile {
    fn name(&self) -> &OsStr {
        &self.name
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.directory.remove_file(&self.name);
        }
    }
}

fn create_temporary(
    parent: &Dir,
    path: &VaultRelativePath,
) -> Result<(cap_std::fs::File, TemporaryFile), RecoveryFsError> {
    let cleanup_directory = parent
        .try_clone()
        .map_err(|source| io_error("clone parent for", path, source))?;
    for _ in 0..8 {
        let name = OsString::from(format!(".tcui-recover-{:032x}.tmp", rand::random::<u128>()));
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        match parent.open_with(&name, &options) {
            Ok(file) => {
                return Ok((
                    file,
                    TemporaryFile {
                        directory: cleanup_directory,
                        name,
                        armed: true,
                    },
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(source) => return Err(io_error("create temporary for", path, source)),
        }
    }
    Err(io_error(
        "create unique temporary for",
        path,
        io::Error::new(io::ErrorKind::AlreadyExists, "temporary name collision"),
    ))
}

fn sync_directory(directory: &Dir, path: &VaultRelativePath) -> Result<(), RecoveryFsError> {
    #[cfg(unix)]
    {
        let syncable = rustix::fs::openat(
            directory,
            ".",
            rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::DIRECTORY,
            rustix::fs::Mode::empty(),
        )
        .map_err(|source| io_error("open parent for sync of", path, source.into()))?;
        rustix::fs::fsync(&syncable)
            .map_err(|source| io_error("sync parent for", path, source.into()))?;
    }
    #[cfg(not(unix))]
    let _ = (directory, path);
    Ok(())
}

fn invalid_path(path: &VaultRelativePath) -> RecoveryFsError {
    io_error(
        "resolve",
        path,
        io::Error::new(io::ErrorKind::InvalidInput, "invalid relative path"),
    )
}

fn io_error(
    operation: &'static str,
    path: &VaultRelativePath,
    source: io::Error,
) -> RecoveryFsError {
    RecoveryFsError::Io {
        operation,
        path: path.clone(),
        source,
    }
}
