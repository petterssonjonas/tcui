use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use color_eyre::eyre::{eyre, Result};

pub(super) const CONFIG_FILE: &str = "config.toml";
const LOCK_FILE: &str = ".config.lock";

#[derive(Debug, thiserror::Error)]
pub(in crate::config::app_config) enum ConfigError {
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

struct TempFile {
    path: PathBuf,
    armed: bool,
}

impl TempFile {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if self.armed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

pub(super) fn acquire_lock(config_root: &Path) -> Result<ConfigLock> {
    let path = config_root.join(LOCK_FILE);
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)?;
    match File::try_lock(&file) {
        Ok(()) => Ok(ConfigLock { _file: file }),
        Err(std::fs::TryLockError::WouldBlock) => Err(ConfigError::ConfigBusy { path }.into()),
        Err(std::fs::TryLockError::Error(error)) => Err(error.into()),
    }
}

pub(super) fn atomic_replace(path: &Path, content: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| eyre!("configuration path has no parent: {}", path.display()))?;
    std::fs::create_dir_all(parent)?;
    let previous = match std::fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    let temp_path = parent.join(format!(".{CONFIG_FILE}.tmp"));
    let mut temp_guard = TempFile::new(temp_path.clone());
    let mut temp = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temp_path)?;
    temp.write_all(content)?;
    maybe_fail(FailurePoint::Sync)?;
    temp.sync_all()?;
    drop(temp);
    maybe_fail(FailurePoint::Rename)?;
    std::fs::rename(&temp_path, path)?;
    temp_guard.disarm();
    if let Err(error) = sync_parent(parent) {
        restore_previous(path, previous.as_deref())?;
        return Err(error);
    }
    Ok(())
}

pub(super) fn restore_previous(path: &Path, previous: Option<&[u8]>) -> Result<()> {
    match previous {
        Some(content) => replace_without_rollback(path, content),
        None => {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            let parent = path
                .parent()
                .ok_or_else(|| eyre!("configuration path has no parent: {}", path.display()))?;
            sync_parent(parent)
        }
    }
}

fn replace_without_rollback(path: &Path, content: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| eyre!("configuration path has no parent: {}", path.display()))?;
    let temp_path = parent.join(format!(".{CONFIG_FILE}.restore.tmp"));
    let mut temp_guard = TempFile::new(temp_path.clone());
    let mut temp = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temp_path)?;
    temp.write_all(content)?;
    temp.sync_all()?;
    drop(temp);
    std::fs::rename(&temp_path, path)?;
    temp_guard.disarm();
    File::open(parent)?.sync_all()?;
    Ok(())
}

fn sync_parent(parent: &Path) -> Result<()> {
    maybe_fail(FailurePoint::ParentSync)?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

pub(super) fn write_backup_once(config_path: &Path, original: &[u8]) -> Result<()> {
    let backup_path = config_path.with_extension("toml.bak");
    let mut backup = match OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&backup_path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if let Err(error) = backup.write_all(original).and_then(|()| backup.sync_all()) {
        drop(backup);
        let _ = std::fs::remove_file(&backup_path);
        return Err(error.into());
    }
    let parent = config_path.parent().ok_or_else(|| {
        eyre!(
            "configuration path has no parent: {}",
            config_path.display()
        )
    })?;
    sync_parent(parent)
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
pub(in crate::config::app_config) fn inject_failure(point: TestFailurePoint) {
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
pub(in crate::config::app_config) enum TestFailurePoint {
    Sync,
    Rename,
    ParentSync,
    Reload,
}
