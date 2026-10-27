use super::KeyStoreError;
use cap_std::fs::Dir;
use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};

const BACKUP_FILE_ATTEMPTS: u8 = 8;

#[cfg(all(test, unix))]
thread_local! {
    static FAIL_NEXT_DIRECTORY_CLONE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static INJECT_RESTORE_BACKUP_RENAME_FAILURE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static INJECT_PERSISTENT_RESTORE_BACKUP_RENAME_FAILURE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static INJECT_RESTORE_TARGET_REMOVE_FAILURE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static INJECT_COMMIT_BACKUP_REMOVE_FAILURE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(unix)]
pub(super) struct PreviousFileGuard {
    directory: Dir,
    target: PathBuf,
    backup: Option<PathBuf>,
    replaced: bool,
}

#[cfg(unix)]
impl PreviousFileGuard {
    pub(super) fn capture(
        directory: &Dir,
        target: &OsStr,
    ) -> std::result::Result<Self, KeyStoreError> {
        let target = PathBuf::from(target);
        let directory = clone_directory(directory)?;
        let backup = create_backup(&directory, &target)?;
        Ok(Self {
            directory,
            target,
            backup,
            replaced: false,
        })
    }

    pub(super) fn mark_replaced(&mut self) {
        self.replaced = true;
    }

    pub(super) fn restore(&mut self) -> std::result::Result<(), KeyStoreError> {
        if !self.replaced {
            return Ok(());
        }
        match self.backup.as_ref() {
            Some(backup) => {
                #[cfg(test)]
                if INJECT_PERSISTENT_RESTORE_BACKUP_RENAME_FAILURE.with(std::cell::Cell::get)
                    || INJECT_RESTORE_BACKUP_RENAME_FAILURE.with(|failure| failure.replace(false))
                {
                    return Err(KeyStoreError::Write);
                }
                self.directory
                    .rename(backup, &self.directory, &self.target)
                    .map_err(|_| KeyStoreError::Write)?;
                self.backup = None;
            }
            None => {
                #[cfg(test)]
                if INJECT_RESTORE_TARGET_REMOVE_FAILURE.with(|failure| failure.replace(false)) {
                    return Err(KeyStoreError::Write);
                }
                self.directory
                    .remove_file(&self.target)
                    .map_err(|_| KeyStoreError::Write)?;
            }
        }
        self.replaced = false;
        Ok(())
    }

    pub(super) fn commit(&mut self) -> std::result::Result<(), KeyStoreError> {
        if let Some(backup) = self.backup.as_ref() {
            #[cfg(test)]
            if INJECT_COMMIT_BACKUP_REMOVE_FAILURE.with(|failure| failure.replace(false)) {
                return Err(KeyStoreError::Write);
            }
            self.directory
                .remove_file(backup)
                .map_err(|_| KeyStoreError::Write)?;
            self.backup = None;
        }
        self.replaced = false;
        Ok(())
    }
}

#[cfg(unix)]
fn clone_directory(directory: &Dir) -> std::result::Result<Dir, KeyStoreError> {
    #[cfg(test)]
    if FAIL_NEXT_DIRECTORY_CLONE.with(|failure| failure.replace(false)) {
        return Err(KeyStoreError::Write);
    }

    directory.try_clone().map_err(|_| KeyStoreError::Write)
}

#[cfg(all(test, unix))]
fn fail_next_directory_clone() {
    FAIL_NEXT_DIRECTORY_CLONE.with(|failure| failure.set(true));
}

#[cfg(all(test, unix))]
pub(super) fn inject_restore_backup_rename_failure() {
    INJECT_RESTORE_BACKUP_RENAME_FAILURE.with(|failure| failure.set(true));
}

#[cfg(all(test, unix))]
pub(super) struct PersistentRestoreBackupRenameFailure;

#[cfg(all(test, unix))]
impl Drop for PersistentRestoreBackupRenameFailure {
    fn drop(&mut self) {
        INJECT_PERSISTENT_RESTORE_BACKUP_RENAME_FAILURE.with(|failure| failure.set(false));
    }
}

#[cfg(all(test, unix))]
pub(super) fn inject_persistent_restore_backup_rename_failure(
) -> PersistentRestoreBackupRenameFailure {
    INJECT_PERSISTENT_RESTORE_BACKUP_RENAME_FAILURE.with(|failure| failure.set(true));
    PersistentRestoreBackupRenameFailure
}

#[cfg(all(test, unix))]
pub(super) fn inject_restore_target_remove_failure() {
    INJECT_RESTORE_TARGET_REMOVE_FAILURE.with(|failure| failure.set(true));
}

#[cfg(all(test, unix))]
pub(super) fn inject_commit_backup_remove_failure() {
    INJECT_COMMIT_BACKUP_REMOVE_FAILURE.with(|failure| failure.set(true));
}

#[cfg(unix)]
impl Drop for PreviousFileGuard {
    fn drop(&mut self) {
        if self.replaced {
            let _ = self.restore();
        }
        if self.replaced {
            // Preserve recovery material when restoration cannot complete.
            return;
        }
        if let Some(backup) = self.backup.as_ref() {
            if self.directory.remove_file(backup).is_ok() {
                self.backup = None;
            }
        }
    }
}

#[cfg(unix)]
fn create_backup(
    directory: &Dir,
    target: &Path,
) -> std::result::Result<Option<PathBuf>, KeyStoreError> {
    match directory.symlink_metadata(target) {
        Ok(metadata) if metadata.file_type().is_symlink() => return Err(KeyStoreError::UnsafePath),
        Ok(metadata) if !metadata.is_file() => return Err(KeyStoreError::Write),
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(KeyStoreError::Write),
    }

    for _ in 0..BACKUP_FILE_ATTEMPTS {
        let backup = PathBuf::from(format!(".tcui-keys-backup-{}.tmp", rand::random::<u64>()));
        match directory.hard_link(target, directory, &backup) {
            Ok(()) => return Ok(Some(backup)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(KeyStoreError::Write),
        }
    }

    Err(KeyStoreError::Write)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use cap_std::ambient_authority;
    use std::fs;

    struct TestRoot(PathBuf);

    impl TestRoot {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "tcui-rollback-clone-failure-{}-{}",
                std::process::id(),
                rand::random::<u64>()
            ));
            fs::create_dir_all(&root).expect("create isolated rollback test root");
            Self(root)
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn capture_leaves_no_artifacts_when_directory_clone_fails() {
        let root = TestRoot::new();
        let target = root.0.join("keys.toml");
        let original = b"encrypted-credential-bytes";
        fs::write(&target, original).expect("write original credential file");
        let directory = Dir::open_ambient_dir(&root.0, ambient_authority())
            .expect("open isolated rollback test directory");

        fail_next_directory_clone();
        let result = PreviousFileGuard::capture(&directory, OsStr::new("keys.toml"));

        assert!(matches!(result, Err(KeyStoreError::Write)));
        assert_eq!(
            fs::read(&target).expect("read original credential file"),
            original
        );
        assert!(fs::read_dir(&root.0)
            .expect("read rollback test directory")
            .all(|entry| {
                !entry
                    .expect("read rollback test entry")
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".tcui-keys-")
            }));
    }
}
