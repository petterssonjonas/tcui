// allow: SIZE_OK - T04B keeps atomic mutation invariants and their fault-injection tests colocated.

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fmt::Display;
use std::fs::{File, OpenOptions as StdOpenOptions, TryLockError};
use std::hash::Hash;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use similar::TextDiff;

use crate::storage::paths::{ensure_directory, set_unix_mode, TcuiDataPaths};

use super::backup::{BackupError, BackupState, BackupStore, LoadedBackup, StandaloneBackupId};
use super::editor::{self, EditorTargetError, ValidatedEditorTarget};
use super::move_journal::{JournalStore, MoveJournalError};
use super::VaultRelativePath;

const BACKUP_RETENTION_DAYS: i64 = 7;

pub struct VaultMutationLock {
    path: PathBuf,
}

impl VaultMutationLock {
    pub fn at_data_root(data_root: &Path) -> Self {
        Self {
            path: data_root.join("locks").join("vault-mutations.lock"),
        }
    }

    pub fn try_acquire(&self) -> Result<VaultMutationGuard, VaultWriteError> {
        let parent = self.path.parent().ok_or(VaultWriteError::InvalidDataRoot)?;
        ensure_directory(parent)
            .map_err(|source| io_error("create lock directory for", &self.path, source))?;
        let file = StdOpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&self.path)
            .map_err(|source| io_error("open lock for", &self.path, source))?;
        set_unix_mode(&self.path, 0o600)
            .map_err(|source| io_error("set lock permissions for", &self.path, source))?;
        match file.try_lock() {
            Ok(()) => Ok(VaultMutationGuard { _file: file }),
            Err(TryLockError::WouldBlock) => Err(VaultWriteError::VaultBusy),
            Err(TryLockError::Error(source)) => Err(io_error("lock", &self.path, source)),
        }
    }
}

pub struct VaultMutationGuard {
    _file: File,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultDiff {
    pub unified: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultWriteOutcome {
    pub backup_id: StandaloneBackupId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultRestoreOutcome {
    pub replaced_content_backup_id: StandaloneBackupId,
}

#[derive(Debug, thiserror::Error)]
pub enum VaultWriteError {
    #[error("Vault is busy with another mutation")]
    VaultBusy,
    #[error("Vault profile is unavailable: {profile_id}")]
    UnknownProfile { profile_id: String },
    #[error("Vault path points through a symbolic link: {path}")]
    Symlink { path: PathBuf },
    #[error("Vault path is not a regular file: {path}")]
    NotFile { path: PathBuf },
    #[error("Vault path is not a directory: {path}")]
    NotDirectory { path: PathBuf },
    #[error("Vault file is not UTF-8 text: {path}")]
    InvalidUtf8 { path: PathBuf },
    #[error("standalone backup does not belong to the requested Vault file")]
    BackupTargetMismatch,
    #[error("TCUI data root has no valid parent")]
    InvalidDataRoot,
    #[error("unsupported Vault move journal version {version}")]
    UnsupportedMoveJournalVersion { version: u64 },
    #[error("Vault move recovery is required for operation {operation_id}")]
    MoveRecoveryRequired { operation_id: String },
    #[error("Vault move journal blocks mutation: {reason}")]
    MoveJournalBlocked { reason: String },
    #[error(transparent)]
    Backup(#[from] BackupError),
    #[error(transparent)]
    EditorTarget(#[from] EditorTargetError),
    #[cfg(test)]
    #[error("injected Vault mutation failure during {operation}")]
    InjectedFailure { operation: &'static str },
    #[error("failed to {operation} Vault path {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

struct ProfileHandle {
    canonical_root: PathBuf,
    directory: Dir,
}

pub struct VaultProfileRegistration<ProfileId> {
    profile_id: ProfileId,
    root: PathBuf,
}

impl<ProfileId> VaultProfileRegistration<ProfileId> {
    pub fn new(profile_id: ProfileId, root: impl Into<PathBuf>) -> Self {
        Self {
            profile_id,
            root: root.into(),
        }
    }
}

pub struct VaultWriteService<ProfileId> {
    profiles: HashMap<ProfileId, ProfileHandle>,
    mutation_lock: VaultMutationLock,
    backups: BackupStore,
    journals: JournalStore,
}

impl<ProfileId> VaultWriteService<ProfileId>
where
    ProfileId: Clone + Display + Eq + Hash,
{
    pub fn new(
        profiles: impl IntoIterator<Item = VaultProfileRegistration<ProfileId>>,
    ) -> Result<Self, VaultWriteError> {
        Self::open(profiles, &TcuiDataPaths::discover().root)
    }

    #[cfg(test)]
    pub(crate) fn at_data_root(
        profiles: impl IntoIterator<Item = VaultProfileRegistration<ProfileId>>,
        data_root: &Path,
    ) -> Result<Self, VaultWriteError> {
        Self::open(profiles, data_root)
    }

    fn open(
        registrations: impl IntoIterator<Item = VaultProfileRegistration<ProfileId>>,
        data_root: &Path,
    ) -> Result<Self, VaultWriteError> {
        let mut profiles = HashMap::new();
        for registration in registrations {
            let canonical_root = std::fs::canonicalize(&registration.root)
                .map_err(|source| io_error("canonicalize profile", &registration.root, source))?;
            let directory = Dir::open_ambient_dir(&canonical_root, cap_std::ambient_authority())
                .map_err(|source| io_error("open profile", &canonical_root, source))?;
            profiles.insert(
                registration.profile_id,
                ProfileHandle {
                    canonical_root,
                    directory,
                },
            );
        }
        Ok(Self {
            profiles,
            mutation_lock: VaultMutationLock::at_data_root(data_root),
            backups: BackupStore::new(data_root.join("vault-backups")),
            journals: JournalStore::at_data_root(data_root),
        })
    }

    pub fn diff(
        &self,
        profile_id: &ProfileId,
        path: &VaultRelativePath,
        replacement: &str,
    ) -> Result<VaultDiff, VaultWriteError> {
        let profile = self.profile(profile_id)?;
        let (parent, name) = open_parent(&profile.directory, path.as_path())?;
        let original = read_existing(&parent, &name, path.as_path())?.unwrap_or_default();
        let original = String::from_utf8(original).map_err(|_| VaultWriteError::InvalidUtf8 {
            path: path.as_path().to_path_buf(),
        })?;
        let unified = TextDiff::from_lines(original.as_str(), replacement)
            .unified_diff()
            .context_radius(3)
            .header("current", "proposed")
            .to_string();
        Ok(VaultDiff { unified })
    }

    pub fn write(
        &self,
        profile_id: &ProfileId,
        path: &VaultRelativePath,
        replacement: &[u8],
    ) -> Result<VaultWriteOutcome, VaultWriteError> {
        let guard = self.mutation_lock.try_acquire()?;
        self.ensure_journal_mutation_allowed()?;
        self.backups.prune_terminal(BACKUP_RETENTION_DAYS)?;
        self.write_with_guard(&guard, profile_id, path, replacement)
    }

    pub fn restore(
        &self,
        profile_id: &ProfileId,
        path: &VaultRelativePath,
        backup_id: &StandaloneBackupId,
    ) -> Result<VaultRestoreOutcome, VaultWriteError> {
        let guard = self.mutation_lock.try_acquire()?;
        self.ensure_journal_mutation_allowed()?;
        self.backups.prune_terminal(BACKUP_RETENTION_DAYS)?;
        let LoadedBackup { envelope, contents } = self.backups.load(backup_id)?;
        if envelope.profile_id != profile_id.to_string() || envelope.path != *path {
            return Err(VaultWriteError::BackupTargetMismatch);
        }
        let outcome = match contents {
            Some(bytes) => self.write_with_guard(&guard, profile_id, path, &bytes)?,
            None => self.remove_with_guard(&guard, profile_id, path)?,
        };
        Ok(VaultRestoreOutcome {
            replaced_content_backup_id: outcome.backup_id,
        })
    }

    pub fn try_acquire_guard(&self) -> Result<VaultMutationGuard, VaultWriteError> {
        self.mutation_lock.try_acquire()
    }

    pub(crate) fn write_with_guard(
        &self,
        _guard: &VaultMutationGuard,
        profile_id: &ProfileId,
        path: &VaultRelativePath,
        replacement: &[u8],
    ) -> Result<VaultWriteOutcome, VaultWriteError> {
        let profile = self.profile(profile_id)?;
        let (parent, name) = open_parent(&profile.directory, path.as_path())?;
        let original = read_existing(&parent, &name, path.as_path())?;
        let (mut temporary, mut cleanup) = create_temporary(&parent, path.as_path())?;
        let prepared = (|| {
            maybe_fail(FailurePoint::Write)?;
            temporary
                .write_all(replacement)
                .map_err(|source| io_error("write temporary for", path.as_path(), source))?;
            temporary
                .flush()
                .map_err(|source| io_error("flush temporary for", path.as_path(), source))?;
            maybe_fail(FailurePoint::Sync)?;
            temporary
                .sync_all()
                .map_err(|source| io_error("sync temporary for", path.as_path(), source))?;
            maybe_fail(FailurePoint::Backup)?;
            let profile_key = profile_id.to_string();
            self.backups
                .create_prepared(&profile_key, path, original.as_deref())
                .map_err(VaultWriteError::from)
        })();
        drop(temporary);
        let backup_id = prepared?;

        let rename_result = maybe_fail(FailurePoint::Rename).and_then(|()| {
            parent
                .rename(cleanup.name(), &parent, &name)
                .map_err(|source| io_error("rename temporary for", path.as_path(), source))
        });
        if let Err(error) = rename_result {
            let _ = self.backups.mark_state(&backup_id, BackupState::Failed);
            return Err(error);
        }
        cleanup.disarm();

        let parent_sync = maybe_fail(FailurePoint::ParentSync)
            .and_then(|()| sync_directory(&parent, path.as_path()));
        if let Err(error) = parent_sync {
            rollback(&parent, &name, original.as_deref(), path.as_path())?;
            let _ = self.backups.mark_state(&backup_id, BackupState::Failed);
            return Err(error);
        }
        let _ = self.backups.mark_state(&backup_id, BackupState::Complete);
        Ok(VaultWriteOutcome { backup_id })
    }

    pub fn validate_editor_target(
        &self,
        profile_id: &ProfileId,
        path: &VaultRelativePath,
    ) -> Result<ValidatedEditorTarget, VaultWriteError> {
        let profile = self.profile(profile_id)?;
        editor::validate_editor_target(&profile.canonical_root, &profile.directory, path)
            .map_err(VaultWriteError::from)
    }

    fn remove_with_guard(
        &self,
        _guard: &VaultMutationGuard,
        profile_id: &ProfileId,
        path: &VaultRelativePath,
    ) -> Result<VaultWriteOutcome, VaultWriteError> {
        let profile = self.profile(profile_id)?;
        let (parent, name) = open_parent(&profile.directory, path.as_path())?;
        let original = read_existing(&parent, &name, path.as_path())?;
        let profile_key = profile_id.to_string();
        let backup_id = self
            .backups
            .create_prepared(&profile_key, path, original.as_deref())?;
        if original.is_none() {
            let _ = self.backups.mark_state(&backup_id, BackupState::Complete);
            return Ok(VaultWriteOutcome { backup_id });
        }

        let mut tombstone = match maybe_fail(FailurePoint::Rename)
            .and_then(|()| move_to_temporary(&parent, &name, path.as_path()))
        {
            Ok(tombstone) => tombstone,
            Err(error) => {
                let _ = self.backups.mark_state(&backup_id, BackupState::Failed);
                return Err(error);
            }
        };
        let parent_sync = maybe_fail(FailurePoint::ParentSync)
            .and_then(|()| sync_directory(&parent, path.as_path()));
        if let Err(error) = parent_sync {
            let tombstone_name = tombstone.name().to_os_string();
            tombstone.disarm();
            parent
                .rename(&tombstone_name, &parent, &name)
                .map_err(|source| io_error("rollback removal for", path.as_path(), source))?;
            sync_directory(&parent, path.as_path())?;
            let _ = self.backups.mark_state(&backup_id, BackupState::Failed);
            return Err(error);
        }

        if parent.remove_file(tombstone.name()).is_ok() {
            tombstone.disarm();
            let _ = sync_directory(&parent, path.as_path());
        }
        let _ = self.backups.mark_state(&backup_id, BackupState::Complete);
        Ok(VaultWriteOutcome { backup_id })
    }

    fn profile(&self, id: &ProfileId) -> Result<&ProfileHandle, VaultWriteError> {
        self.profiles
            .get(id)
            .ok_or_else(|| VaultWriteError::UnknownProfile {
                profile_id: id.to_string(),
            })
    }

    fn ensure_journal_mutation_allowed(&self) -> Result<(), VaultWriteError> {
        self.journals
            .ensure_mutation_allowed()
            .map_err(|error| match error {
                MoveJournalError::UnknownVersion { version } => {
                    VaultWriteError::UnsupportedMoveJournalVersion { version }
                }
                MoveJournalError::RecoveryPending { operation_id, .. } => {
                    VaultWriteError::MoveRecoveryRequired {
                        operation_id: operation_id.to_string(),
                    }
                }
                other @ (MoveJournalError::InvalidTransition { .. }
                | MoveJournalError::Corrupt { .. }
                | MoveJournalError::Serialize(_)
                | MoveJournalError::Io { .. }) => VaultWriteError::MoveJournalBlocked {
                    reason: other.to_string(),
                },
            })
    }
}

struct TemporaryFile {
    directory: Dir,
    name: OsString,
    armed: bool,
}

impl TemporaryFile {
    fn name(&self) -> &OsStr {
        &self.name
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.directory.remove_file(&self.name);
        }
    }
}

fn create_temporary(
    parent: &Dir,
    path: &Path,
) -> Result<(cap_std::fs::File, TemporaryFile), VaultWriteError> {
    let cleanup_directory = parent
        .try_clone()
        .map_err(|source| io_error("clone parent for", path, source))?;
    for _ in 0..8 {
        let name = OsString::from(format!(".tcui-write-{:032x}.tmp", rand::random::<u128>()));
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        match parent.open_with(&name, &options) {
            Ok(file) => {
                return Ok((
                    file,
                    TemporaryFile {
                        directory: cleanup_directory,
                        name,
                        armed: true,
                    },
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(source) => return Err(io_error("create temporary for", path, source)),
        }
    }
    Err(io_error(
        "create unique temporary for",
        path,
        io::Error::new(io::ErrorKind::AlreadyExists, "temporary name collision"),
    ))
}

fn move_to_temporary(
    parent: &Dir,
    name: &OsStr,
    path: &Path,
) -> Result<TemporaryFile, VaultWriteError> {
    let cleanup_directory = parent
        .try_clone()
        .map_err(|source| io_error("clone parent for", path, source))?;
    for _ in 0..8 {
        let temporary_name =
            OsString::from(format!(".tcui-remove-{:032x}.tmp", rand::random::<u128>()));
        match parent.rename(name, parent, &temporary_name) {
            Ok(()) => {
                return Ok(TemporaryFile {
                    directory: cleanup_directory,
                    name: temporary_name,
                    armed: true,
                });
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(source) => return Err(io_error("stage removal for", path, source)),
        }
    }
    Err(io_error(
        "create unique removal temporary for",
        path,
        io::Error::new(io::ErrorKind::AlreadyExists, "temporary name collision"),
    ))
}

fn rollback(
    parent: &Dir,
    name: &OsStr,
    original: Option<&[u8]>,
    path: &Path,
) -> Result<(), VaultWriteError> {
    match original {
        Some(bytes) => {
            let (mut temporary, mut cleanup) = create_temporary(parent, path)?;
            let prepared = temporary
                .write_all(bytes)
                .and_then(|()| temporary.flush())
                .and_then(|()| temporary.sync_all())
                .map_err(|source| io_error("rollback", path, source));
            drop(temporary);
            prepared?;
            parent
                .rename(cleanup.name(), parent, name)
                .map_err(|source| io_error("publish rollback for", path, source))?;
            cleanup.disarm();
        }
        None => parent
            .remove_file(name)
            .map_err(|source| io_error("rollback new file for", path, source))?,
    }
    sync_directory(parent, path)
}

fn open_parent(root: &Dir, path: &Path) -> Result<(Dir, OsString), VaultWriteError> {
    let mut directory = root
        .try_clone()
        .map_err(|source| io_error("clone root for", path, source))?;
    let mut components = path.components().peekable();
    let mut traversed = PathBuf::new();
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
            return Err(io_error(
                "resolve",
                path,
                io::Error::new(io::ErrorKind::InvalidInput, "invalid relative path"),
            ));
        };
        if components.peek().is_none() {
            return Ok((directory, name.to_os_string()));
        }
        traversed.push(name);
        directory = directory.open_dir_nofollow(name).map_err(|source| {
            classify_open_error(&directory, name, &traversed, "open directory", source)
        })?;
    }
    Err(io_error(
        "resolve",
        path,
        io::Error::new(io::ErrorKind::InvalidInput, "empty relative path"),
    ))
}

fn read_existing(
    parent: &Dir,
    name: &OsStr,
    path: &Path,
) -> Result<Option<Vec<u8>>, VaultWriteError> {
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let mut file = match parent.open_with(name, &options) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return match parent.symlink_metadata(name) {
                Ok(metadata) if metadata.is_symlink() => Err(VaultWriteError::Symlink {
                    path: path.to_path_buf(),
                }),
                Ok(_) => Err(io_error("open", path, error)),
                Err(missing) if missing.kind() == io::ErrorKind::NotFound => Ok(None),
                Err(source) => Err(io_error("inspect", path, source)),
            };
        }
        Err(source) => {
            return Err(classify_open_error(parent, name, path, "open", source));
        }
    };
    let metadata = file
        .metadata()
        .map_err(|source| io_error("read metadata for", path, source))?;
    if !metadata.is_file() {
        return Err(VaultWriteError::NotFile {
            path: path.to_path_buf(),
        });
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|source| io_error("read", path, source))?;
    Ok(Some(bytes))
}

fn classify_open_error(
    directory: &Dir,
    name: &OsStr,
    path: &Path,
    operation: &'static str,
    source: io::Error,
) -> VaultWriteError {
    match directory.symlink_metadata(name) {
        Ok(metadata) if metadata.is_symlink() => VaultWriteError::Symlink {
            path: path.to_path_buf(),
        },
        Ok(metadata) if !metadata.is_dir() && operation == "open directory" => {
            VaultWriteError::NotDirectory {
                path: path.to_path_buf(),
            }
        }
        Ok(_) | Err(_) => io_error(operation, path, source),
    }
}

fn sync_directory(directory: &Dir, path: &Path) -> Result<(), VaultWriteError> {
    #[cfg(unix)]
    {
        let syncable = rustix::fs::openat(
            directory,
            ".",
            rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::DIRECTORY,
            rustix::fs::Mode::empty(),
        )
        .map_err(|source| io_error("open parent for sync of", path, source.into()))?;
        rustix::fs::fsync(&syncable)
            .map_err(|source| io_error("sync parent for", path, source.into()))?;
    }
    #[cfg(not(unix))]
    let _ = (directory, path);
    Ok(())
}

fn io_error(operation: &'static str, path: &Path, source: io::Error) -> VaultWriteError {
    VaultWriteError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    }
}

#[derive(Clone, Copy)]
enum FailurePoint {
    Write,
    Sync,
    Backup,
    Rename,
    ParentSync,
}

#[cfg(not(test))]
fn maybe_fail(_point: FailurePoint) -> Result<(), VaultWriteError> {
    Ok(())
}

#[cfg(test)]
std::thread_local! {
    static FAILURE_POINT: std::cell::RefCell<Option<FailurePoint>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn maybe_fail(point: FailurePoint) -> Result<(), VaultWriteError> {
    FAILURE_POINT.with(|configured| {
        let mut configured = configured.borrow_mut();
        let fails = configured
            .as_ref()
            .is_some_and(|configured| std::mem::discriminant(configured) == std::mem::discriminant(&point));
        if fails {
            *configured = None;
            return Err(VaultWriteError::InjectedFailure {
                operation: point.label(),
            });
        }
        Ok(())
    })
}

impl FailurePoint {
    #[cfg(test)]
    const fn label(self) -> &'static str {
        match self {
            Self::Write => "write",
            Self::Sync => "sync",
            Self::Backup => "backup",
            Self::Rename => "rename",
            Self::ParentSync => "parent sync",
        }
    }
}

#[cfg(test)]
#[derive(Clone, Copy)]
enum TestFailurePoint {
    Write,
    Sync,
    Backup,
    Rename,
    ParentSync,
}

#[cfg(test)]
impl TestFailurePoint {
    const fn label(self) -> &'static str {
        match self {
            Self::Write => "write",
            Self::Sync => "sync",
            Self::Backup => "backup",
            Self::Rename => "rename",
            Self::ParentSync => "parent-sync",
        }
    }
}

#[cfg(test)]
fn inject_failure(point: TestFailurePoint) {
    let point = match point {
        TestFailurePoint::Write => FailurePoint::Write,
        TestFailurePoint::Sync => FailurePoint::Sync,
        TestFailurePoint::Backup => FailurePoint::Backup,
        TestFailurePoint::Rename => FailurePoint::Rename,
        TestFailurePoint::ParentSync => FailurePoint::ParentSync,
    };
    FAILURE_POINT.with(|configured| *configured.borrow_mut() = Some(point));
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use super::{
        inject_failure, TestFailurePoint, VaultMutationLock, VaultProfileRegistration,
        VaultWriteError, VaultWriteService,
    };
    use crate::config::vault_profiles::{VaultProfile, VaultProfileId};
    use crate::vault::VaultRelativePath;

    struct Fixture {
        base: PathBuf,
        root: PathBuf,
        data_root: PathBuf,
        profile_id: VaultProfileId,
    }

    impl Fixture {
        fn new(label: &str) -> Self {
            let base = std::env::temp_dir().join(format!(
                "tcui-vault-write-{label}-{}-{}",
                std::process::id(),
                rand::random::<u64>()
            ));
            let root = base.join("vault");
            let data_root = base.join("data").join("tcui");
            fs::create_dir_all(&root).expect("create vault root");
            let profile = VaultProfile::new(label, &root).expect("create vault profile");
            let profile_id = profile.id.clone();
            Self {
                base,
                root,
                data_root,
                profile_id,
            }
        }

        fn path(raw: &str) -> VaultRelativePath {
            VaultRelativePath::parse(Path::new(raw)).expect("valid vault-relative path")
        }

        fn service(&self) -> VaultWriteService<VaultProfileId> {
            let registration =
                VaultProfileRegistration::new(self.profile_id.clone(), self.root.clone());
            VaultWriteService::at_data_root([registration], &self.data_root)
                .expect("open write service")
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.base);
        }
    }

    #[test]
    fn vault_confined_write_lock_contention_returns_busy_before_mutation() {
        // Given
        let root = std::env::temp_dir().join(format!(
            "tcui-vault-write-lock-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        fs::create_dir_all(&root).expect("create test data root");
        let lock = VaultMutationLock::at_data_root(&root);
        let _guard = lock.try_acquire().expect("acquire first guard");

        // When
        let result = lock.try_acquire();

        // Then
        assert!(matches!(result, Err(VaultWriteError::VaultBusy)));
        drop(_guard);
        fs::remove_dir_all(root).expect("remove test data root");
    }

    #[test]
    fn vault_confined_write_lock_contention_is_cross_process() {
        // Given
        let fixture = Fixture::new("cross-process-lock");
        fs::create_dir_all(&fixture.data_root).expect("create data root");
        let lock = VaultMutationLock::at_data_root(&fixture.data_root);
        let _guard = lock.try_acquire().expect("acquire parent guard");

        // When
        let status = Command::new(std::env::current_exe().expect("test executable"))
            .arg("--exact")
            .arg("vault::write::tests::vault_confined_write_lock_child_probe")
            .arg("--nocapture")
            .arg("--test-threads=1")
            .env("TCUI_VAULT_LOCK_CHILD_ROOT", &fixture.data_root)
            .status()
            .expect("run lock child process");

        // Then
        assert!(status.success());
    }

    #[test]
    fn vault_confined_write_lock_child_probe() {
        let Some(root) = std::env::var_os("TCUI_VAULT_LOCK_CHILD_ROOT") else {
            return;
        };
        let result = VaultMutationLock::at_data_root(Path::new(&root)).try_acquire();
        assert!(matches!(result, Err(VaultWriteError::VaultBusy)));
    }

    #[test]
    fn vault_confined_write_contention_leaves_file_and_backup_store_untouched() {
        // Given
        let fixture = Fixture::new("contention-no-partial");
        fs::write(fixture.root.join("note.md"), b"original").expect("write original");
        fs::create_dir_all(&fixture.data_root).expect("create data root");
        let lock = VaultMutationLock::at_data_root(&fixture.data_root);
        let _guard = lock.try_acquire().expect("hold mutation guard");
        let service = fixture.service();

        // When
        let result = service.write(
            &fixture.profile_id,
            &Fixture::path("note.md"),
            b"replacement",
        );

        // Then
        assert!(matches!(result, Err(VaultWriteError::VaultBusy)));
        assert_eq!(
            fs::read(fixture.root.join("note.md")).expect("read original"),
            b"original"
        );
        assert!(!fixture.data_root.join("vault-backups").exists());
    }

    #[test]
    fn vault_confined_write_failure_points_preserve_original() {
        for point in [
            TestFailurePoint::Write,
            TestFailurePoint::Sync,
            TestFailurePoint::Backup,
            TestFailurePoint::Rename,
            TestFailurePoint::ParentSync,
        ] {
            // Given
            let fixture = Fixture::new(point.label());
            fs::write(fixture.root.join("note.md"), b"original").expect("write original");
            let service = fixture.service();
            inject_failure(point);

            // When
            let result = service.write(
                &fixture.profile_id,
                &Fixture::path("note.md"),
                b"replacement",
            );

            // Then
            assert!(matches!(
                result,
                Err(VaultWriteError::InjectedFailure { .. })
            ));
            assert_eq!(
                fs::read(fixture.root.join("note.md")).expect("read original"),
                b"original",
                "failure point {} changed the original",
                point.label()
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn vault_confined_write_symlink_escape_never_reaches_outside_sentinel() {
        use std::os::unix::fs::symlink;

        // Given
        let fixture = Fixture::new("symlink-escape");
        let outside = fixture.base.join("outside");
        fs::create_dir_all(&outside).expect("create outside directory");
        fs::write(outside.join("sentinel.md"), b"outside-sentinel")
            .expect("write outside sentinel");
        symlink(&outside, fixture.root.join("escaped")).expect("create escaping symlink");
        let service = fixture.service();

        // When
        let result = service.write(
            &fixture.profile_id,
            &Fixture::path("escaped/sentinel.md"),
            b"replacement",
        );

        // Then
        assert!(matches!(result, Err(VaultWriteError::Symlink { .. })));
        assert_eq!(
            fs::read(outside.join("sentinel.md")).expect("read outside sentinel"),
            b"outside-sentinel"
        );
    }

    #[cfg(unix)]
    #[test]
    fn vault_confined_write_final_symlink_never_reaches_outside_sentinel() {
        use std::os::unix::fs::symlink;

        // Given
        let fixture = Fixture::new("final-symlink");
        let outside = fixture.base.join("outside.md");
        fs::write(&outside, b"outside-sentinel").expect("write outside sentinel");
        symlink(&outside, fixture.root.join("note.md")).expect("create final symlink");
        let service = fixture.service();

        // When
        let result = service.write(
            &fixture.profile_id,
            &Fixture::path("note.md"),
            b"replacement",
        );

        // Then
        assert!(matches!(result, Err(VaultWriteError::Symlink { .. })));
        assert_eq!(
            fs::read(&outside).expect("read outside sentinel"),
            b"outside-sentinel"
        );
    }

    #[test]
    fn vault_confined_write_restore_uses_versioned_backup_and_preserves_replaced_bytes() {
        // Given
        let fixture = Fixture::new("restore");
        fs::write(fixture.root.join("note.md"), b"original").expect("write original");
        let service = fixture.service();
        let write = service
            .write(
                &fixture.profile_id,
                &Fixture::path("note.md"),
                b"replacement",
            )
            .expect("write replacement");

        // When
        let restore = service.restore(
            &fixture.profile_id,
            &Fixture::path("note.md"),
            &write.backup_id,
        );

        // Then
        assert!(restore.is_ok(), "restore failed: {restore:?}");
        assert_eq!(
            fs::read(fixture.root.join("note.md")).expect("read restored note"),
            b"original"
        );
    }

    #[test]
    fn vault_confined_write_restore_missing_original_removes_created_file() {
        // Given
        let fixture = Fixture::new("restore-missing");
        let service = fixture.service();
        let write = service
            .write(
                &fixture.profile_id,
                &Fixture::path("created.md"),
                b"created",
            )
            .expect("create note");

        // When
        let result = service.restore(
            &fixture.profile_id,
            &Fixture::path("created.md"),
            &write.backup_id,
        );

        // Then
        assert!(result.is_ok(), "restore failed: {result:?}");
        assert!(!fixture.root.join("created.md").exists());
    }

    #[test]
    fn vault_confined_write_restore_missing_rolls_back_on_parent_sync_failure() {
        // Given
        let fixture = Fixture::new("restore-missing-sync");
        let service = fixture.service();
        let write = service
            .write(
                &fixture.profile_id,
                &Fixture::path("created.md"),
                b"created",
            )
            .expect("create note");
        inject_failure(TestFailurePoint::ParentSync);

        // When
        let result = service.restore(
            &fixture.profile_id,
            &Fixture::path("created.md"),
            &write.backup_id,
        );

        // Then
        assert!(matches!(
            result,
            Err(VaultWriteError::InjectedFailure { .. })
        ));
        assert_eq!(
            fs::read(fixture.root.join("created.md")).expect("read rolled back note"),
            b"created"
        );
    }

    #[test]
    fn vault_confined_write_diff_is_profile_and_relative_path_scoped() {
        // Given
        let fixture = Fixture::new("diff");
        fs::write(fixture.root.join("note.md"), b"old line\n").expect("write original");
        let service = fixture.service();

        // When
        let diff = service
            .diff(&fixture.profile_id, &Fixture::path("note.md"), "new line\n")
            .expect("generate diff");

        // Then
        assert!(diff.unified.contains("-old line"));
        assert!(diff.unified.contains("+new line"));
    }

    #[test]
    fn vault_confined_write_editor_target_is_validated_and_consumed_once() {
        // Given
        let fixture = Fixture::new("editor-target");
        fs::write(fixture.root.join("note.md"), b"inside").expect("write editor target");
        let service = fixture.service();

        // When
        let target = service
            .validate_editor_target(&fixture.profile_id, &Fixture::path("note.md"))
            .expect("validate editor target")
            .revalidate_for_spawn()
            .expect("revalidate editor target");

        // Then
        assert_eq!(target, fixture.root.join("note.md"));
    }
}
