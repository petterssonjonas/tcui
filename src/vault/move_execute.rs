// allow: SIZE_OK - transaction ordering, guarded I/O, and failpoint boundaries stay auditable together.
#![allow(dead_code)]

use std::ffi::{OsStr, OsString};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};

use crate::config::vault_profiles::{VaultCatalog, VaultProfileId};
use crate::storage::paths::TcuiDataPaths;

use super::backup::{BackupError, BackupState, BackupStore};
use super::move_journal::{
    JournalStore, MoveJournalError, MoveJournalState, MoveJournalV1, MoveOperationId,
};
use super::move_plan::{MoveCollision, MovePlan, MovePlanError, VaultFileRef};
use super::move_rollback::MoveRollback;
use super::{VaultMutationGuard, VaultMutationLock, VaultRelativePath, VaultWriteError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MoveExecutionOutcome {
    pub(crate) operation_id: MoveOperationId,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum MoveExecuteError {
    #[error(transparent)]
    Write(#[from] VaultWriteError),
    #[error(transparent)]
    Plan(#[from] MovePlanError),
    #[error(transparent)]
    Backup(#[from] BackupError),
    #[error("unsupported Vault move journal version {version}")]
    UnknownJournalVersion { version: u64 },
    #[error("Vault move recovery is required for operation {operation_id}")]
    RecoveryRequired { operation_id: MoveOperationId },
    #[error("move destination collision was not approved: {destination:?}")]
    UnapprovedCollision { destination: VaultFileRef },
    #[cfg(test)]
    #[error("injected Vault move failure during {operation}")]
    InjectedFailure { operation: &'static str },
    #[error("failed to {operation} Vault move path {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error(transparent)]
    Journal(MoveJournalError),
}

impl From<MoveJournalError> for MoveExecuteError {
    fn from(error: MoveJournalError) -> Self {
        match error {
            MoveJournalError::UnknownVersion { version } => Self::UnknownJournalVersion { version },
            MoveJournalError::RecoveryPending { operation_id, .. } => {
                Self::RecoveryRequired { operation_id }
            }
            other @ (MoveJournalError::InvalidTransition { .. }
            | MoveJournalError::Corrupt { .. }
            | MoveJournalError::Serialize(_)
            | MoveJournalError::Io { .. }) => Self::Journal(other),
        }
    }
}

pub(crate) struct MoveExecutor<'a> {
    catalog: &'a VaultCatalog,
    mutation_lock: VaultMutationLock,
    journals: JournalStore,
    backups: BackupStore,
}

impl<'a> MoveExecutor<'a> {
    pub(crate) fn new(catalog: &'a VaultCatalog) -> Self {
        Self::at_data_root(catalog, &TcuiDataPaths::discover().root)
    }

    pub(crate) fn at_data_root(catalog: &'a VaultCatalog, data_root: &Path) -> Self {
        Self {
            catalog,
            mutation_lock: VaultMutationLock::at_data_root(data_root),
            journals: JournalStore::at_data_root(data_root),
            backups: BackupStore::new(data_root.join("vault-backups")),
        }
    }

    pub(crate) fn execute(&self, plan: MovePlan) -> Result<MoveExecutionOutcome, MoveExecuteError> {
        clear_test_events();
        let guard = self.mutation_lock.try_acquire()?;
        self.journals.ensure_mutation_allowed()?;
        plan.revalidate(self.catalog)?;
        self.reject_unapproved_collisions(&plan)?;
        maybe_fail(FailurePoint::JournalCreate)?;
        let mut journal = MoveJournalV1::planned(&plan);
        self.create_journal(&guard, &journal)?;
        match self.execute_guarded(&guard, &plan, &mut journal) {
            Ok(()) => Ok(MoveExecutionOutcome {
                operation_id: journal.operation_id,
            }),
            Err(error) => {
                self.mark_rolling_back(&guard, &mut journal)?;
                if leave_for_startup_recovery() {
                    return Err(error);
                }
                let operation_id = journal.operation_id.clone();
                let rollback = MoveRollback::new(self.catalog, &self.journals, &self.backups)
                    .rollback_guarded(&guard, &mut journal)
                    .and_then(|()| {
                        self.journals
                            .transition(&mut journal, MoveJournalState::RolledBack)
                            .map_err(|_| super::move_rollback::RollbackFailure::persist_progress())
                    });
                match rollback {
                    Ok(()) => Err(error),
                    Err(_) => {
                        let _ = self
                            .journals
                            .transition(&mut journal, MoveJournalState::RecoveryRequired);
                        Err(MoveExecuteError::RecoveryRequired { operation_id })
                    }
                }
            }
        }
    }

    fn execute_guarded(
        &self,
        guard: &VaultMutationGuard,
        plan: &MovePlan,
        journal: &mut MoveJournalV1,
    ) -> Result<(), MoveExecuteError> {
        self.transition(guard, journal, MoveJournalState::Staging)?;
        for index in 0..plan.file_count() {
            self.back_up_pair(guard, plan, journal, index)?;
        }
        for index in 0..plan.file_count() {
            self.stage_pair(guard, plan, journal, index)?;
        }
        for index in 0..plan.file_count() {
            self.commit_destination(guard, plan, journal, index)?;
        }
        self.transition(guard, journal, MoveJournalState::DestinationsCommitted)?;
        self.transition(guard, journal, MoveJournalState::SourcesRemoving)?;
        for index in 0..plan.file_count() {
            self.remove_source(guard, plan, journal, index)?;
        }
        self.complete_backups(guard, journal)?;
        self.transition(guard, journal, MoveJournalState::Complete)
    }

    fn reject_unapproved_collisions(&self, plan: &MovePlan) -> Result<(), MoveExecuteError> {
        for (pair, collision) in plan.pairs().iter().zip(plan.collisions()) {
            match collision {
                MoveCollision::Create | MoveCollision::ExplicitOverwrite => {}
                MoveCollision::UnapprovedCollision => {
                    return Err(MoveExecuteError::UnapprovedCollision {
                        destination: pair.destination().clone(),
                    });
                }
                MoveCollision::InvalidType(_) => {
                    return Err(MoveExecuteError::UnapprovedCollision {
                        destination: pair.destination().clone(),
                    });
                }
            }
        }
        Ok(())
    }

    fn create_journal(
        &self,
        _guard: &VaultMutationGuard,
        journal: &MoveJournalV1,
    ) -> Result<(), MoveExecuteError> {
        self.journals.create(journal).map_err(Into::into)
    }

    fn transition(
        &self,
        _guard: &VaultMutationGuard,
        journal: &mut MoveJournalV1,
        state: MoveJournalState,
    ) -> Result<(), MoveExecuteError> {
        maybe_fail(FailurePoint::JournalTransition)?;
        self.journals.transition(journal, state).map_err(Into::into)
    }

    fn persist_artifacts(
        &self,
        _guard: &VaultMutationGuard,
        journal: &MoveJournalV1,
    ) -> Result<(), MoveExecuteError> {
        self.journals.persist(journal).map_err(Into::into)
    }

    fn mark_rolling_back(
        &self,
        _guard: &VaultMutationGuard,
        journal: &mut MoveJournalV1,
    ) -> Result<(), MoveExecuteError> {
        match journal.state {
            MoveJournalState::Planned
            | MoveJournalState::Staging
            | MoveJournalState::DestinationsCommitted
            | MoveJournalState::SourcesRemoving => self
                .journals
                .transition(journal, MoveJournalState::RollingBack)
                .map_err(Into::into),
            MoveJournalState::RollingBack | MoveJournalState::RecoveryRequired => Ok(()),
            MoveJournalState::Complete | MoveJournalState::RolledBack => {
                Err(MoveJournalError::InvalidTransition {
                    from: journal.state,
                    to: MoveJournalState::RollingBack,
                }
                .into())
            }
        }
    }

    fn back_up_pair(
        &self,
        guard: &VaultMutationGuard,
        plan: &MovePlan,
        journal: &mut MoveJournalV1,
        index: usize,
    ) -> Result<(), MoveExecuteError> {
        let pair = &plan.pairs()[index];
        let source_bytes = self.read_required(pair.source())?;
        let expected = &plan.preconditions()[index];
        if u64::try_from(source_bytes.len()).ok() != Some(expected.size)
            || blake3::hash(&source_bytes) != expected.blake3
        {
            return Err(MovePlanError::SourceChanged {
                file: pair.source().clone(),
            }
            .into());
        }
        maybe_fail(FailurePoint::Backup)?;
        let source_backup = self.backups.create_prepared(
            &pair.source().profile_id().to_string(),
            pair.source().path(),
            Some(&source_bytes),
        )?;
        journal.artifacts[index].source_backup_id = Some(source_backup);
        self.persist_artifacts(guard, journal)?;

        if plan.collisions()[index] == MoveCollision::ExplicitOverwrite {
            let destination_bytes = self.read_required(pair.destination())?;
            maybe_fail(FailurePoint::Backup)?;
            let destination_backup = self.backups.create_prepared(
                &pair.destination().profile_id().to_string(),
                pair.destination().path(),
                Some(&destination_bytes),
            )?;
            journal.artifacts[index].destination_backup_id = Some(destination_backup);
            self.persist_artifacts(guard, journal)?;
        }
        Ok(())
    }

    fn stage_pair(
        &self,
        guard: &VaultMutationGuard,
        plan: &MovePlan,
        journal: &mut MoveJournalV1,
        index: usize,
    ) -> Result<(), MoveExecuteError> {
        let pair = &plan.pairs()[index];
        let source_bytes = self.read_required(pair.source())?;
        let staged_path = staged_path(pair.destination().path(), &journal.operation_id, index)?;
        journal.artifacts[index].staged_path = Some(staged_path.clone());
        self.persist_artifacts(guard, journal)?;
        let root = self.open_profile(pair.destination().profile_id())?;
        let (parent, name) = open_parent(&root, staged_path.as_path(), true)?;
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        let mut staged = parent
            .open_with(&name, &options)
            .map_err(|source| io_error("create stage for", staged_path.as_path(), source))?;
        maybe_fail(FailurePoint::StageWrite)?;
        staged
            .write_all(&source_bytes)
            .and_then(|()| staged.flush())
            .map_err(|source| io_error("write stage for", staged_path.as_path(), source))?;
        maybe_fail(FailurePoint::StageSync)?;
        staged
            .sync_all()
            .map_err(|source| io_error("sync stage for", staged_path.as_path(), source))
    }

    fn commit_destination(
        &self,
        guard: &VaultMutationGuard,
        plan: &MovePlan,
        journal: &mut MoveJournalV1,
        index: usize,
    ) -> Result<(), MoveExecuteError> {
        let pair = &plan.pairs()[index];
        let staged_path =
            journal.artifacts[index]
                .staged_path
                .as_ref()
                .ok_or(MoveJournalError::Corrupt {
                    reason: "missing staged path",
                })?;
        let root = self.open_profile(pair.destination().profile_id())?;
        let (parent, staged_name) = open_parent(&root, staged_path.as_path(), false)?;
        let destination_name = final_name(pair.destination().path())?;
        maybe_fail(FailurePoint::DestinationRename)?;
        parent
            .rename(&staged_name, &parent, destination_name)
            .map_err(|source| {
                io_error(
                    "commit destination",
                    pair.destination().path().as_path(),
                    source,
                )
            })?;
        maybe_fail(FailurePoint::DestinationDirectorySync)?;
        sync_directory(&parent, pair.destination().path().as_path())?;
        record_event(TestEvent::DestinationDirectorySynced);
        journal.artifacts[index].destination_committed = true;
        self.persist_artifacts(guard, journal)
    }

    fn remove_source(
        &self,
        guard: &VaultMutationGuard,
        plan: &MovePlan,
        journal: &mut MoveJournalV1,
        index: usize,
    ) -> Result<(), MoveExecuteError> {
        let source = plan.pairs()[index].source();
        let root = self.open_profile(source.profile_id())?;
        let (parent, name) = open_parent(&root, source.path().as_path(), false)?;
        record_event(TestEvent::SourceRemoveStarted);
        maybe_fail(FailurePoint::SourceRemove)?;
        parent
            .remove_file(&name)
            .map_err(|error| io_error("remove source", source.path().as_path(), error))?;
        maybe_fail(FailurePoint::SourceDirectorySync)?;
        sync_directory(&parent, source.path().as_path())?;
        journal.artifacts[index].source_removed = true;
        self.persist_artifacts(guard, journal)
    }

    fn complete_backups(
        &self,
        _guard: &VaultMutationGuard,
        journal: &MoveJournalV1,
    ) -> Result<(), MoveExecuteError> {
        for artifact in &journal.artifacts {
            if let Some(id) = &artifact.source_backup_id {
                self.backups.mark_state(id, BackupState::Complete)?;
            }
            if let Some(id) = &artifact.destination_backup_id {
                self.backups.mark_state(id, BackupState::Complete)?;
            }
        }
        Ok(())
    }

    fn read_required(&self, file: &VaultFileRef) -> Result<Vec<u8>, MoveExecuteError> {
        let root = self.open_profile(file.profile_id())?;
        let (parent, name) = open_parent(&root, file.path().as_path(), false)?;
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        let mut opened = parent
            .open_with(&name, &options)
            .map_err(|source| io_error("open", file.path().as_path(), source))?;
        let metadata = opened
            .metadata()
            .map_err(|source| io_error("inspect", file.path().as_path(), source))?;
        if !metadata.is_file() {
            return Err(VaultWriteError::NotFile {
                path: file.path().as_path().to_path_buf(),
            }
            .into());
        }
        let mut bytes = Vec::new();
        opened
            .read_to_end(&mut bytes)
            .map_err(|source| io_error("read", file.path().as_path(), source))?;
        Ok(bytes)
    }

    fn open_profile(&self, profile_id: &VaultProfileId) -> Result<Dir, MoveExecuteError> {
        let profile =
            self.catalog
                .get(profile_id)
                .ok_or_else(|| VaultWriteError::UnknownProfile {
                    profile_id: profile_id.to_string(),
                })?;
        Dir::open_ambient_dir(&profile.root, cap_std::ambient_authority())
            .map_err(|source| io_error("open profile", &profile.root, source))
    }
}

fn staged_path(
    destination: &VaultRelativePath,
    operation_id: &MoveOperationId,
    index: usize,
) -> Result<VaultRelativePath, MoveExecuteError> {
    let mut path = destination.as_path().to_path_buf();
    path.set_file_name(format!(".tcui-{}-{index:03}.stage", operation_id.as_str()));
    VaultRelativePath::parse(&path).map_err(|source| {
        io_error(
            "construct stage path for",
            destination.as_path(),
            io::Error::new(io::ErrorKind::InvalidInput, source),
        )
    })
}

fn final_name(path: &VaultRelativePath) -> Result<&OsStr, MoveExecuteError> {
    path.as_path().file_name().ok_or_else(|| {
        io_error(
            "resolve destination",
            path.as_path(),
            io::Error::new(io::ErrorKind::InvalidInput, "missing file name"),
        )
    })
}

fn open_parent(root: &Dir, path: &Path, create: bool) -> Result<(Dir, OsString), MoveExecuteError> {
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
        match directory.open_dir_nofollow(name) {
            Ok(child) => directory = child,
            Err(error) if create && error.kind() == io::ErrorKind::NotFound => {
                directory.create_dir(name).map_err(|source| {
                    io_error("create destination directory", &traversed, source)
                })?;
                sync_directory(&directory, &traversed)?;
                directory = directory
                    .open_dir_nofollow(name)
                    .map_err(|source| io_error("open destination directory", &traversed, source))?;
            }
            Err(source) => {
                return Err(classify_open_error(&directory, name, &traversed, source));
            }
        }
    }
    Err(io_error(
        "resolve",
        path,
        io::Error::new(io::ErrorKind::InvalidInput, "empty relative path"),
    ))
}

fn classify_open_error(
    directory: &Dir,
    name: &OsStr,
    path: &Path,
    source: io::Error,
) -> MoveExecuteError {
    match directory.symlink_metadata(name) {
        Ok(metadata) if metadata.is_symlink() => VaultWriteError::Symlink {
            path: path.to_path_buf(),
        }
        .into(),
        Ok(metadata) if !metadata.is_dir() => VaultWriteError::NotDirectory {
            path: path.to_path_buf(),
        }
        .into(),
        Ok(_) | Err(_) => io_error("open directory", path, source),
    }
}

fn sync_directory(directory: &Dir, path: &Path) -> Result<(), MoveExecuteError> {
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

fn io_error(operation: &'static str, path: &Path, source: io::Error) -> MoveExecuteError {
    MoveExecuteError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FailurePoint {
    JournalCreate,
    JournalTransition,
    Backup,
    StageWrite,
    StageSync,
    DestinationRename,
    DestinationDirectorySync,
    SourceRemove,
    SourceDirectorySync,
}

#[cfg(not(test))]
fn maybe_fail(_point: FailurePoint) -> Result<(), MoveExecuteError> {
    Ok(())
}

#[cfg(test)]
std::thread_local! {
    static FAILURE_POINT: std::cell::RefCell<Option<FailurePoint>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn maybe_fail(point: FailurePoint) -> Result<(), MoveExecuteError> {
    FAILURE_POINT.with(|configured| {
        let mut configured = configured.borrow_mut();
        if configured
            .as_ref()
            .is_some_and(|configured| *configured == point)
        {
            *configured = None;
            return Err(MoveExecuteError::InjectedFailure {
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
            Self::JournalCreate => "journal create",
            Self::JournalTransition => "journal transition",
            Self::Backup => "backup",
            Self::StageWrite => "stage write",
            Self::StageSync => "stage sync",
            Self::DestinationRename => "destination rename",
            Self::DestinationDirectorySync => "destination directory sync",
            Self::SourceRemove => "source remove",
            Self::SourceDirectorySync => "source directory sync",
        }
    }
}

#[cfg(test)]
#[derive(Clone, Copy)]
pub(crate) enum TestFailurePoint {
    JournalCreate,
    JournalTransition,
    Backup,
    StageWrite,
    StageSync,
    DestinationRename,
    DestinationDirectorySync,
    SourceRemove,
    SourceDirectorySync,
}

#[cfg(test)]
impl TestFailurePoint {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::JournalCreate => "journal-create",
            Self::JournalTransition => "journal-transition",
            Self::Backup => "backup",
            Self::StageWrite => "stage-write",
            Self::StageSync => "stage-sync",
            Self::DestinationRename => "destination-rename",
            Self::DestinationDirectorySync => "destination-directory-sync",
            Self::SourceRemove => "source-remove",
            Self::SourceDirectorySync => "source-directory-sync",
        }
    }
}

#[cfg(test)]
pub(crate) fn inject_failure(point: TestFailurePoint) {
    let point = match point {
        TestFailurePoint::JournalCreate => FailurePoint::JournalCreate,
        TestFailurePoint::JournalTransition => FailurePoint::JournalTransition,
        TestFailurePoint::Backup => FailurePoint::Backup,
        TestFailurePoint::StageWrite => FailurePoint::StageWrite,
        TestFailurePoint::StageSync => FailurePoint::StageSync,
        TestFailurePoint::DestinationRename => FailurePoint::DestinationRename,
        TestFailurePoint::DestinationDirectorySync => FailurePoint::DestinationDirectorySync,
        TestFailurePoint::SourceRemove => FailurePoint::SourceRemove,
        TestFailurePoint::SourceDirectorySync => FailurePoint::SourceDirectorySync,
    };
    FAILURE_POINT.with(|configured| *configured.borrow_mut() = Some(point));
}

#[cfg(test)]
std::thread_local! {
    static LEAVE_FOR_STARTUP_RECOVERY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
pub(crate) fn inject_interruption(point: TestFailurePoint) {
    inject_failure(point);
    LEAVE_FOR_STARTUP_RECOVERY.with(|configured| configured.set(true));
}

#[cfg(test)]
fn leave_for_startup_recovery() -> bool {
    LEAVE_FOR_STARTUP_RECOVERY.with(|configured| configured.replace(false))
}

#[cfg(not(test))]
const fn leave_for_startup_recovery() -> bool {
    false
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TestEvent {
    DestinationDirectorySynced,
    SourceRemoveStarted,
}

#[cfg(test)]
std::thread_local! {
    static TEST_EVENTS: std::cell::RefCell<Vec<TestEvent>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
fn record_event(event: TestEvent) {
    TEST_EVENTS.with(|events| events.borrow_mut().push(event));
}

#[cfg(not(test))]
enum TestEvent {
    DestinationDirectorySynced,
    SourceRemoveStarted,
}

#[cfg(not(test))]
fn record_event(_event: TestEvent) {}

#[cfg(test)]
fn clear_test_events() {
    TEST_EVENTS.with(|events| events.borrow_mut().clear());
}

#[cfg(not(test))]
fn clear_test_events() {}

#[cfg(test)]
pub(crate) fn take_test_events() -> Vec<TestEvent> {
    TEST_EVENTS.with(|events| std::mem::take(&mut *events.borrow_mut()))
}
