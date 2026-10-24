use std::fs::{File, OpenOptions, TryLockError};

use super::MigrationError;
use crate::storage::paths::{ensure_directory, set_unix_mode, TcuiDataPaths};

pub(super) struct MigrationLock {
    _file: File,
}

impl MigrationLock {
    pub(super) fn acquire(paths: &TcuiDataPaths) -> Result<Self, MigrationError> {
        ensure_directory(&paths.migrations_dir)?;
        let path = paths.migrations_dir.join("saved-internal-vault-v1.lock");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;
        set_unix_mode(&path, 0o600)?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(TryLockError::WouldBlock) => Err(MigrationError::MigrationBusy),
            Err(TryLockError::Error(error)) => Err(error.into()),
        }
    }
}
