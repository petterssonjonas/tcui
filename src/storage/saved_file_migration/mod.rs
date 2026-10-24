//! Data-preserving relocation of application Saved files into the built-in Vault.
mod marker;

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::OpenOptions;

use crate::config::AppConfig;
use crate::storage::paths::TcuiDataPaths;

pub(crate) struct SavedFileMigration;

pub(crate) struct StartupMigration {
    pub(crate) saved_root: PathBuf,
    pub(crate) data_paths: TcuiDataPaths,
    pub(crate) warning: Option<MigrationError>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum MigrationError {
    #[error("Saved file migration is busy")]
    MigrationBusy,
    #[error("Saved file migration refused unsafe source {path}")]
    UnsafeSavedRoot { path: PathBuf },
    #[error("Saved file migration conflicts with an existing internal file: {path}")]
    Conflict { path: PathBuf },
    #[error("Saved file migration is incomplete; reconnect the legacy Saved directory and restart: {path}")]
    SourceUnavailable { path: PathBuf },
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("migration marker is not a regular file: {path}")]
    InvalidMarker { path: PathBuf },
}

impl SavedFileMigration {
    pub(crate) fn startup(config: Option<&AppConfig>) -> StartupMigration {
        let data_paths = TcuiDataPaths::discover();
        let saved_root = data_paths.root.join("vault");
        let warning = config.and_then(|config| migrate_saved_files(config).err());
        StartupMigration {
            saved_root,
            data_paths,
            warning,
        }
    }
}

/// Copy old Saved files before protection configuration enumerates the internal Vault.
/// Sources are never removed. Publication is no-clobber and restartable after interruption.
pub(crate) fn migrate_saved_files(config: &AppConfig) -> Result<(), MigrationError> {
    let paths = TcuiDataPaths::discover();
    let _lock = marker::MigrationLock::acquire(&paths)?;
    let marker = paths.migrations_dir.join("saved-internal-vault-v1.done");
    match std::fs::symlink_metadata(&marker) {
        Ok(metadata) if metadata.is_file() => return Ok(()),
        Ok(_) => return Err(MigrationError::InvalidMarker { path: marker }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let pending = paths.migrations_dir.join("saved-internal-vault-v1.pending");
    crate::protection::write_plain(&pending, b"saved-internal-vault-v1\n")
        .map_err(|error| io::Error::other(error.to_string()))?;
    let source = config
        .saved_files_dir
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            crate::app::generated_file::expand_user_path(
                Path::new(value),
                dirs::home_dir().as_deref(),
            )
        })
        .unwrap_or_else(|| paths.root.join("artifacts"));
    let destination = paths.root.join("vault");
    if !source.is_absolute()
        || source.parent().is_none()
        || dirs::home_dir().as_ref() == Some(&source)
    {
        return Err(MigrationError::UnsafeSavedRoot { path: source });
    }
    if source != destination {
        if source != paths.root.join("artifacts") && !source.try_exists()? {
            return Err(MigrationError::SourceUnavailable { path: source });
        }
        copy_saved_root(&source, &destination)?;
    }
    let artifacts = paths.root.join("artifacts");
    if artifacts != source && artifacts != destination {
        copy_saved_root(&artifacts, &destination)?;
    }
    crate::protection::open_directory(&destination, true)
        .map_err(|error| io::Error::other(error.to_string()))?;
    crate::storage::chat_files::ChatSavedFileStore::new(destination.clone(), paths.clone())
        .verify_migrated_targets()
        .map_err(|error| io::Error::other(error.to_string()))?;
    crate::protection::write(
        crate::protection::Domain::Vault,
        &marker,
        b"saved-internal-vault-v1\n",
    )
    .map_err(|error| io::Error::other(error.to_string()))?;
    let directory = crate::protection::open_directory(&paths.migrations_dir, false)
        .map_err(|error| io::Error::other(error.to_string()))?;
    directory.remove_file("saved-internal-vault-v1.pending")?;
    rustix::fs::fsync(&directory).map_err(io::Error::from)?;
    Ok(())
}

pub(crate) fn require_complete(paths: &TcuiDataPaths) -> io::Result<bool> {
    let directory = match crate::protection::open_directory(&paths.migrations_dir, false) {
        Ok(directory) => directory,
        Err(error)
            if error
                .downcast_ref::<io::Error>()
                .is_some_and(|source| source.kind() == io::ErrorKind::NotFound) =>
        {
            return Ok(false)
        }
        Err(error) => return Err(io::Error::other(error.to_string())),
    };
    match directory.symlink_metadata("saved-internal-vault-v1.done") {
        Ok(metadata) if metadata.is_file() => return Ok(true),
        Ok(_) => {
            return Err(io::Error::other(
                "Saved migration completion marker is not a regular file",
            ))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    match directory.symlink_metadata("saved-internal-vault-v1.pending") {
        Ok(_) => Err(io::Error::other("Saved file migration is incomplete; reconnect the legacy Saved directory and restart. Saved-file changes and link pruning are disabled.")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn copy_saved_root(source: &Path, destination: &Path) -> Result<(), MigrationError> {
    crate::protection::require(crate::protection::Domain::Vault)
        .map_err(|error| io::Error::other(error.to_string()))?;
    match std::fs::symlink_metadata(source) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Ok(metadata) if metadata.is_dir() && !metadata.is_symlink() => {}
        Ok(_) => {
            return Err(MigrationError::UnsafeSavedRoot {
                path: source.to_path_buf(),
            })
        }
        Err(error) => return Err(error.into()),
    }
    let source_dir = crate::protection::open_directory(source, false)
        .map_err(|error| io::Error::other(error.to_string()))?;
    let destination_dir = crate::protection::open_directory(destination, true)
        .map_err(|error| io::Error::other(error.to_string()))?;
    let decode_source = source.starts_with(TcuiDataPaths::discover().root.join("artifacts"));
    for entry in source_dir.entries()? {
        let entry = entry?;
        let name = entry.file_name();
        if name.as_encoded_bytes().first() == Some(&b'.') {
            continue;
        }
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_saved_root(&source.join(&name), &destination.join(&name))?;
            continue;
        }
        if !kind.is_file() {
            continue;
        }
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        let mut file = source_dir.open_with(&name, &options)?;
        if !file.metadata()?.is_file() {
            return Err(MigrationError::UnsafeSavedRoot {
                path: source.join(&name),
            });
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        let bytes = if decode_source {
            crate::protection::decode(crate::protection::Domain::Vault, &bytes)
                .map_err(|error| io::Error::other(error.to_string()))?
        } else {
            bytes
        };
        match destination_dir.open_with(&name, &options) {
            Ok(mut existing) => {
                if !existing.metadata()?.is_file() {
                    return Err(MigrationError::Conflict {
                        path: destination.join(&name),
                    });
                }
                let mut stored = Vec::new();
                existing.read_to_end(&mut stored)?;
                let stored = crate::protection::decode(crate::protection::Domain::Vault, &stored)
                    .map_err(|error| io::Error::other(error.to_string()))?;
                if stored != bytes {
                    return Err(MigrationError::Conflict {
                        path: destination.join(&name),
                    });
                }
                continue;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let encoded = crate::protection::encode(crate::protection::Domain::Vault, &bytes)
            .map_err(|error| io::Error::other(error.to_string()))?;
        let temporary = format!(".saved-migration-{:032x}.tmp", rand::random::<u128>());
        let descriptor = rustix::fs::openat(
            &destination_dir,
            temporary.as_str(),
            rustix::fs::OFlags::WRONLY
                | rustix::fs::OFlags::CREATE
                | rustix::fs::OFlags::EXCL
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::from_bits_truncate(0o600),
        )
        .map_err(io::Error::from)?;
        let mut output = std::fs::File::from(descriptor);
        let result = (|| -> io::Result<()> {
            output.write_all(&encoded)?;
            output.sync_all()?;
            destination_dir.hard_link(&temporary, &destination_dir, &name)?;
            Ok(())
        })();
        drop(output);
        let _ = destination_dir.remove_file(&temporary);
        result?;
    }
    rustix::fs::fsync(&destination_dir).map_err(io::Error::from)?;
    Ok(())
}
