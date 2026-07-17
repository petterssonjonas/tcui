use std::fs::{File, OpenOptions, TryLockError};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(test)]
use std::sync::Arc;

use crate::storage::paths::{ensure_directory, set_unix_mode, TcuiDataPaths};

use super::LegacyResetError;

const MARKER_CONTENT: &[u8] = b"chat-saved-files-v2\n";

pub(super) struct MigrationLock {
    _file: File,
}

impl MigrationLock {
    pub(super) fn acquire(paths: &TcuiDataPaths) -> Result<Self, LegacyResetError> {
        ensure_directory(&paths.migrations_dir)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&paths.chat_saved_files_v2_lock)?;
        set_unix_mode(&paths.chat_saved_files_v2_lock, 0o600)?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(TryLockError::WouldBlock) => Err(LegacyResetError::MigrationBusy),
            Err(TryLockError::Error(error)) => Err(error.into()),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(super) struct MarkerWriter {
    #[cfg(test)]
    failure: Arc<AtomicBool>,
}

impl MarkerWriter {
    pub(super) fn write(&self, paths: &TcuiDataPaths) -> Result<(), LegacyResetError> {
        let temp_path = paths.migrations_dir.join(format!(
            ".chat-saved-files-v2.{}.tmp",
            rand::random::<u64>()
        ));
        let mut temp = TempMarker::new(temp_path);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(temp.path())?;
        set_unix_mode(temp.path(), 0o600)?;
        file.write_all(MARKER_CONTENT)?;
        file.sync_all()?;
        drop(file);

        #[cfg(test)]
        if self.failure.swap(false, Ordering::SeqCst) {
            return Err(LegacyResetError::InjectedMarkerWriteFailure);
        }

        std::fs::rename(temp.path(), &paths.chat_saved_files_v2_marker)?;
        temp.disarm();
        set_unix_mode(&paths.chat_saved_files_v2_marker, 0o600)?;
        sync_directory(&paths.migrations_dir)?;
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn fail_next(&self) {
        self.failure.store(true, Ordering::SeqCst);
    }
}

pub(super) fn is_complete(paths: &TcuiDataPaths) -> Result<bool, LegacyResetError> {
    match std::fs::symlink_metadata(&paths.chat_saved_files_v2_marker) {
        Ok(metadata) if metadata.file_type().is_file() => Ok(true),
        Ok(_) => Err(LegacyResetError::InvalidMarker {
            path: paths.chat_saved_files_v2_marker.clone(),
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn sync_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    OpenOptions::new().read(true).open(path)?.sync_all()?;

    #[cfg(not(unix))]
    let _ = path;

    Ok(())
}

struct TempMarker {
    path: PathBuf,
    armed: bool,
}

impl TempMarker {
    const fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    const fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TempMarker {
    fn drop(&mut self) {
        if self.armed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
