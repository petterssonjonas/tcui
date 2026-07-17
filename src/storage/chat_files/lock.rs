use std::fs::{File, OpenOptions, TryLockError};

use super::model::ChatFileError;
use crate::storage::paths::{ensure_directory, set_unix_mode, TcuiDataPaths};

pub(super) struct GlobalLinkLock {
    _file: File,
}

impl GlobalLinkLock {
    pub(super) fn acquire(paths: &TcuiDataPaths) -> Result<Self, ChatFileError> {
        ensure_directory(&paths.root)?;
        ensure_directory(&paths.chat_file_links_dir)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&paths.chat_file_links_lock)?;
        set_unix_mode(&paths.chat_file_links_lock, 0o600)?;

        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(TryLockError::WouldBlock) => Err(ChatFileError::ChatFilesBusy),
            Err(TryLockError::Error(error)) => Err(ChatFileError::Io(error)),
        }
    }
}
