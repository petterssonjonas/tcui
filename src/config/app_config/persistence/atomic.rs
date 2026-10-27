use std::ffi::OsStr;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use cap_std::fs::Dir;
use color_eyre::eyre::{eyre, Result, WrapErr};
use rustix::fs::{fstat, openat, statat, AtFlags, Mode, OFlags};

pub(super) const CONFIG_FILE: &str = "config.toml";
const LOCK_FILE: &str = ".config.lock";

#[derive(Debug, thiserror::Error)]
pub(crate) enum ConfigError {
    #[error("configuration is busy: {path}")]
    ConfigBusy { path: PathBuf },
    #[error("invalid configuration: {message}")]
    InvalidConfig { message: String },
    #[cfg(test)]
    #[error("injected configuration failure during {operation}")]
    InjectedFailure { operation: &'static str },
}

pub(super) struct ConfigLock {
    _file: File,
}

impl Drop for ConfigLock {
    fn drop(&mut self) {
        // A forked child can retain the open description until exec or exit.
        // End the critical section now, not when every inherited copy closes.
        let _ = self._file.unlock();
    }
}

struct OwnedFile<'a> {
    directory: &'a Dir,
    name: String,
    file: File,
    armed: bool,
}

impl<'a> OwnedFile<'a> {
    fn create(directory: &'a Dir, name: String) -> io::Result<Self> {
        let descriptor = openat(
            directory,
            name.as_str(),
            OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_bits_truncate(0o600),
        )?;
        Ok(Self {
            directory,
            name,
            file: File::from(descriptor),
            armed: true,
        })
    }

    fn temporary(directory: &'a Dir) -> io::Result<Self> {
        Self::create(
            directory,
            format!(".tcui-config-{:032x}.tmp", rand::random::<u128>()),
        )
    }

    fn matches(&self, name: &OsStr) -> io::Result<bool> {
        same_file(self.directory, name, &self.file)
    }

    fn publish(&mut self, name: &OsStr) -> Result<()> {
        if !self.matches(OsStr::new(&self.name))? {
            return Err(eyre!("private temporary file changed before publication"));
        }
        self.directory.rename(&self.name, self.directory, name)?;
        self.armed = false;
        Ok(())
    }

    fn publish_new(&mut self, name: &OsStr) -> io::Result<()> {
        if !self.matches(OsStr::new(&self.name))? {
            return Err(io::Error::other(
                "private temporary file changed before publication",
            ));
        }
        #[cfg(any(target_os = "linux", target_os = "android"))]
        rustix::fs::renameat_with(
            self.directory,
            self.name.as_str(),
            self.directory,
            name,
            rustix::fs::RenameFlags::NOREPLACE,
        )?;
        #[cfg(not(any(target_os = "linux", target_os = "android")))]
        {
            self.directory.hard_link(&self.name, self.directory, name)?;
            self.remove()?;
        }
        self.armed = false;
        Ok(())
    }

    fn remove(&mut self) -> io::Result<()> {
        if !self.matches(OsStr::new(&self.name))? {
            return Err(io::Error::other("private file changed before cleanup"));
        }
        self.directory.remove_file(&self.name)?;
        self.armed = false;
        Ok(())
    }
}

impl Drop for OwnedFile<'_> {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.remove();
        }
    }
}

fn same_file(directory: &Dir, name: &OsStr, file: &File) -> io::Result<bool> {
    let opened = fstat(file)?;
    let current = match statat(directory, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(current) => current,
        Err(rustix::io::Errno::NOENT) => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    Ok(opened.st_dev == current.st_dev && opened.st_ino == current.st_ino)
}

pub(super) fn acquire_lock(config_root: &Path) -> Result<ConfigLock> {
    let directory = crate::protection::open_directory(config_root, true)?;
    let path = config_root.join(LOCK_FILE);
    let descriptor = openat(
        &directory,
        LOCK_FILE,
        OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::from_bits_truncate(0o600),
    )?;
    let file = File::from(descriptor);
    if !file.metadata()?.is_file() || fstat(&file)?.st_nlink != 1 {
        return Err(eyre!("configuration lock is not a private regular file"));
    }
    match File::try_lock(&file) {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => {
            return Err(ConfigError::ConfigBusy { path }.into());
        }
        Err(std::fs::TryLockError::Error(error)) => return Err(error.into()),
    }
    let guard = ConfigLock { _file: file };
    if !same_file(&directory, OsStr::new(LOCK_FILE), &guard._file)? {
        return Err(eyre!("configuration lock changed while opening it"));
    }
    rustix::fs::fsync(&directory)?;
    Ok(guard)
}

fn parent_and_name(path: &Path) -> Result<(&Path, &OsStr)> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| eyre!("private file path has no parent: {}", path.display()))?;
    let name = path
        .file_name()
        .ok_or_else(|| eyre!("private file path has no filename: {}", path.display()))?;
    Ok((parent, name))
}

fn read_optional_in(directory: &Dir, name: &OsStr) -> io::Result<Option<Vec<u8>>> {
    let descriptor = match openat(
        directory,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    ) {
        Ok(descriptor) => descriptor,
        Err(rustix::io::Errno::NOENT) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut file = File::from(descriptor);
    if !file.metadata()?.is_file() {
        return Err(io::Error::other(
            "private storage path is not a regular file",
        ));
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(Some(bytes))
}

pub(super) fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    let (parent, name) = parent_and_name(path)?;
    let directory = crate::protection::open_directory(parent, false)?;
    Ok(read_optional_in(&directory, name)?)
}

pub(super) fn atomic_replace(path: &Path, content: &[u8]) -> Result<()> {
    let (parent, name) = parent_and_name(path)?;
    let directory = crate::protection::open_directory(parent, true)?;
    atomic_replace_in(&directory, name, content)
}

fn check_destination(directory: &Dir, name: &OsStr) -> io::Result<()> {
    match directory.symlink_metadata(name) {
        Ok(metadata) if !metadata.is_file() || metadata.is_symlink() => Err(io::Error::other(
            "refusing to replace a non-regular private file",
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Replaces a private file through its retained parent capability.
/// The caller must hold a stable lock for the complete read-modify-write operation.
pub(crate) fn atomic_replace_in(directory: &Dir, name: &OsStr, content: &[u8]) -> Result<()> {
    let previous = read_optional_in(directory, name)?;
    let mut temporary = OwnedFile::temporary(directory)?;
    temporary.file.write_all(content)?;
    maybe_fail(FailurePoint::Sync)?;
    temporary.file.sync_all()?;
    maybe_fail(FailurePoint::Rename)?;
    check_destination(directory, name)?;
    temporary.publish(name)?;
    if let Err(error) = sync_parent(directory) {
        if !temporary.matches(name)? {
            return Err(error.wrap_err("replacement changed; rollback was not safe"));
        }
        restore_in(directory, name, previous.as_deref())
            .wrap_err_with(|| format!("rollback failed after {error}"))?;
        return Err(error);
    }
    Ok(())
}

pub(super) fn restore_previous(path: &Path, previous: Option<&[u8]>) -> Result<()> {
    let (parent, name) = parent_and_name(path)?;
    let directory = crate::protection::open_directory(parent, false)?;
    // Recovery must reject links and special files just like normal writes.
    check_destination(&directory, name)?;
    restore_in(&directory, name, previous)
}

fn restore_in(directory: &Dir, name: &OsStr, previous: Option<&[u8]>) -> Result<()> {
    match previous {
        Some(content) => {
            let mut temporary = OwnedFile::temporary(directory)?;
            temporary.file.write_all(content)?;
            temporary.file.sync_all()?;
            temporary.publish(name)?;
        }
        None => match directory.remove_file(name) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        },
    }
    rustix::fs::fsync(directory)?;
    Ok(())
}

fn sync_parent(directory: &Dir) -> Result<()> {
    maybe_fail(FailurePoint::ParentSync)?;
    rustix::fs::fsync(directory)?;
    Ok(())
}

pub(super) fn write_backup_once(config_path: &Path, original: &[u8]) -> Result<()> {
    let backup_path = config_path.with_extension("toml.bak");
    let (parent, name) = parent_and_name(&backup_path)?;
    let directory = crate::protection::open_directory(parent, false)?;
    // Publish only a complete, synced backup, never a partially written backup.
    match directory.symlink_metadata(name) {
        Ok(_) => {
            check_destination(&directory, name)?;
            return Ok(());
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let mut backup = OwnedFile::temporary(&directory)?;
    backup.file.write_all(original)?;
    backup.file.sync_all()?;
    match backup.publish_new(name) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            check_destination(&directory, name)?;
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    }
    if let Err(error) = sync_parent(&directory) {
        if !backup.matches(name)? {
            return Err(error.wrap_err("backup changed; rollback was not safe"));
        }
        directory.remove_file(name)?;
        rustix::fs::fsync(&directory)?;
        return Err(error);
    }
    Ok(())
}

#[derive(Clone, Copy)]
pub(super) enum FailurePoint {
    Sync,
    Rename,
    ParentSync,
    Reload,
}

#[cfg(not(test))]
pub(super) fn maybe_fail(_point: FailurePoint) -> Result<()> {
    Ok(())
}

#[cfg(test)]
static FAILURE_POINT: std::sync::Mutex<Option<FailurePoint>> = std::sync::Mutex::new(None);

#[cfg(test)]
pub(super) fn maybe_fail(point: FailurePoint) -> Result<()> {
    let mut configured = FAILURE_POINT.lock().expect("failure point lock poisoned");
    let should_fail = configured.as_ref().is_some_and(|configured| {
        std::mem::discriminant(configured) == std::mem::discriminant(&point)
    });
    if should_fail {
        *configured = None;
        let operation = match point {
            FailurePoint::Sync => "sync",
            FailurePoint::Rename => "rename",
            FailurePoint::ParentSync => "parent sync",
            FailurePoint::Reload => "reload",
        };
        return Err(ConfigError::InjectedFailure { operation }.into());
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn inject_failure(point: TestFailurePoint) {
    let point = match point {
        TestFailurePoint::Sync => FailurePoint::Sync,
        TestFailurePoint::Rename => FailurePoint::Rename,
        TestFailurePoint::ParentSync => FailurePoint::ParentSync,
        TestFailurePoint::Reload => FailurePoint::Reload,
    };
    *FAILURE_POINT.lock().expect("failure point lock poisoned") = Some(point);
}

#[cfg(test)]
#[derive(Clone, Copy)]
pub(crate) enum TestFailurePoint {
    Sync,
    Rename,
    ParentSync,
    Reload,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};

    #[test]
    fn configuration_lock_releases_while_a_duplicated_descriptor_stays_open() {
        let root = std::env::temp_dir().join(format!(
            "tcui-config-lock-release-{:032x}",
            rand::random::<u128>()
        ));
        let guard = acquire_lock(&root).expect("initial lock");
        let inherited = guard._file.try_clone().expect("duplicate open description");
        assert!(acquire_lock(&root)
            .err()
            .expect("a live guard excludes another writer")
            .downcast_ref::<ConfigError>()
            .is_some_and(|error| matches!(error, ConfigError::ConfigBusy { .. })));
        drop(guard);
        let next = acquire_lock(&root).expect("guard must release before every duplicate closes");
        drop(inherited);
        assert!(acquire_lock(&root).is_err());
        drop(next);
        assert!(acquire_lock(&root).is_ok());
        std::fs::remove_dir_all(root).expect("remove lock fixture");
    }

    #[test]
    fn exclusive_private_temporary_cleanup_preserves_a_replaced_path() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "tcui-config-owned-cleanup-{:032x}",
            rand::random::<u128>()
        ));
        let directory = crate::protection::open_directory(&root, true).unwrap();
        let mut temporary = OwnedFile::temporary(&directory).unwrap();
        assert_eq!(
            temporary.file.metadata().unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(temporary.file.metadata().unwrap().len(), 0);
        temporary.file.write_all(b"owned dummy bytes").unwrap();
        let temporary_name = temporary.name.clone();
        directory
            .rename(&temporary_name, &directory, "retained")
            .unwrap();
        let sentinel = root.join("sentinel");
        std::fs::write(&sentinel, b"unowned dummy sentinel").unwrap();
        symlink(&sentinel, root.join(&temporary_name)).unwrap();

        assert_eq!(
            OwnedFile::create(&directory, temporary_name.clone())
                .err()
                .unwrap()
                .kind(),
            io::ErrorKind::AlreadyExists
        );
        assert!(temporary.publish(OsStr::new("config.toml")).is_err());
        drop(temporary);
        assert!(std::fs::symlink_metadata(root.join(&temporary_name))
            .unwrap()
            .is_symlink());
        assert_eq!(std::fs::read(&sentinel).unwrap(), b"unowned dummy sentinel");
        assert_eq!(
            std::fs::read(root.join("retained")).unwrap(),
            b"owned dummy bytes"
        );
        assert!(!root.join("config.toml").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
