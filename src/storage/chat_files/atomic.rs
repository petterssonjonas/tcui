use std::fs::File;
use std::io::{self, Write};
use std::path::Path;

use cap_std::fs::Dir;
use rustix::fs::{openat, Mode, OFlags};

#[cfg(test)]
use std::sync::atomic::{AtomicU8, Ordering};
#[cfg(test)]
use std::sync::Arc;

use super::model::ChatFileError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AtomicWriteTarget {
    SavedFile,
    Manifest,
    OrphanMarker,
}

impl AtomicWriteTarget {
    #[cfg(test)]
    const fn code(self) -> u8 {
        match self {
            Self::SavedFile => 1,
            Self::Manifest => 2,
            Self::OrphanMarker => 3,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(super) struct AtomicWriter {
    #[cfg(test)]
    failure: Arc<AtomicU8>,
}

impl AtomicWriter {
    pub(super) fn write_replace(
        &self,
        path: &Path,
        contents: &[u8],
        target: AtomicWriteTarget,
    ) -> Result<(), ChatFileError> {
        let mut temporary = self.write_temp(path, contents, target)?;
        let name = path
            .file_name()
            .ok_or_else(|| io::Error::other("missing filename"))?;
        match temporary.directory.symlink_metadata(name) {
            Ok(metadata) if !metadata.is_file() || metadata.is_symlink() => {
                return Err(ChatFileError::NotRegularFile {
                    path: path.to_path_buf(),
                });
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        temporary
            .directory
            .rename(&temporary.name, &temporary.directory, name)?;
        temporary.armed = false;
        sync_handle(&temporary.directory)
    }

    pub(super) fn write_new(
        &self,
        path: &Path,
        contents: &[u8],
        target: AtomicWriteTarget,
    ) -> Result<(), ChatFileError> {
        let mut temporary = self.write_temp(path, contents, target)?;
        let name = path
            .file_name()
            .ok_or_else(|| io::Error::other("missing filename"))?;
        temporary
            .directory
            .hard_link(&temporary.name, &temporary.directory, name)?;
        temporary.directory.remove_file(&temporary.name)?;
        temporary.armed = false;
        sync_handle(&temporary.directory)
    }

    fn write_temp(
        &self,
        path: &Path,
        contents: &[u8],
        target: AtomicWriteTarget,
    ) -> Result<TemporaryFile, ChatFileError> {
        #[cfg(not(test))]
        let _ = target;
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::other("path has no parent"))?;
        let directory = crate::protection::open_directory(parent, false)
            .map_err(|error| io::Error::other(error.to_string()))?;
        let name = format!(".chat-file-{:032x}.tmp", rand::random::<u128>());
        let descriptor = openat(
            &directory,
            name.as_str(),
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_bits_truncate(0o600),
        )
        .map_err(io::Error::from)?;
        let temporary = TemporaryFile {
            directory,
            name,
            armed: true,
        };
        let mut file = File::from(descriptor);
        file.write_all(contents)?;
        file.sync_all()?;
        #[cfg(test)]
        if self.should_fail(target) {
            return Err(ChatFileError::InjectedAtomicWriteFailure);
        }
        Ok(temporary)
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

fn sync_handle(directory: &Dir) -> Result<(), ChatFileError> {
    rustix::fs::fsync(directory).map_err(io::Error::from)?;
    Ok(())
}

pub(super) fn sync_directory(path: &Path) -> Result<(), ChatFileError> {
    let directory = crate::protection::open_directory(path, false)
        .map_err(|error| io::Error::other(error.to_string()))?;
    sync_handle(&directory)
}

struct TemporaryFile {
    directory: Dir,
    name: String,
    armed: bool,
}

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.directory.remove_file(&self.name);
        }
    }
}
