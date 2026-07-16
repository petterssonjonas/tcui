use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

#[cfg(test)]
use std::sync::atomic::{AtomicU8, Ordering};
#[cfg(test)]
use std::sync::Arc;

use super::model::ChatFileError;
use crate::storage::paths::set_unix_mode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AtomicWriteTarget {
    Payload,
    Manifest,
}

impl AtomicWriteTarget {
    #[cfg(test)]
    const fn code(self) -> u8 {
        match self {
            Self::Payload => 1,
            Self::Manifest => 2,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(super) struct AtomicWriter {
    #[cfg(test)]
    failure: Arc<AtomicU8>,
}

impl AtomicWriter {
    pub(super) fn write(
        &self,
        path: &Path,
        contents: &[u8],
        target: AtomicWriteTarget,
    ) -> Result<(), ChatFileError> {
        #[cfg(not(test))]
        let _ = target;

        let parent = path.parent().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no parent")
        })?;
        let temp_name = format!(
            ".{}.{}.tmp",
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("chat-file"),
            rand::random::<u64>()
        );
        let mut temp = TempPath::new(parent.join(temp_name));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(temp.path())?;
        set_unix_mode(temp.path(), 0o600)?;
        file.write_all(contents)?;
        file.sync_all()?;
        drop(file);

        #[cfg(test)]
        if self.should_fail(target) {
            return Err(ChatFileError::InjectedAtomicWriteFailure);
        }

        fs::rename(temp.path(), path)?;
        temp.disarm();
        set_unix_mode(path, 0o600)?;
        sync_directory(parent)?;
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn fail_next(&self, target: AtomicWriteTarget) {
        self.failure.store(target.code(), Ordering::SeqCst);
    }

    #[cfg(test)]
    fn should_fail(&self, target: AtomicWriteTarget) -> bool {
        self.failure
            .compare_exchange(target.code(), 0, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }
}

pub(super) fn sync_directory(path: &Path) -> Result<(), ChatFileError> {
    #[cfg(unix)]
    {
        OpenOptions::new().read(true).open(path)?.sync_all()?;
    }

    #[cfg(not(unix))]
    let _ = path;

    Ok(())
}

struct TempPath {
    path: PathBuf,
    armed: bool,
}

impl TempPath {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TempPath {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(&self.path);
        }
    }
}
