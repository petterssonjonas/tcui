use std::fs::{File, OpenOptions, TryLockError};

use super::model::ChatFileError;
use crate::storage::cache_paths::TcuiCachePaths;
use crate::storage::paths::{ensure_directory, set_unix_mode};

pub(super) struct ConversationLock {
    _file: File,
}

impl ConversationLock {
    pub(super) fn acquire(
        paths: &TcuiCachePaths,
        conversation_id: i64,
    ) -> Result<Self, ChatFileError> {
        paths.ensure_layout()?;
        let directory = paths.conversation_dir(conversation_id);
        ensure_directory(&directory)?;
        let lock_path = directory.join(".lock");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)?;
        set_unix_mode(&lock_path, 0o600)?;

        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(TryLockError::WouldBlock) => Err(ChatFileError::ChatFilesBusy),
            Err(TryLockError::Error(error)) => Err(ChatFileError::Io(error)),
        }
    }
}
