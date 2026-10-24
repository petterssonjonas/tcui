use std::path::Path;

use super::move_journal::MoveJournalFileRef;
use super::move_recovery_fs::{read_optional, replace_file, RecoveryFsError};
use super::VaultRelativePath;

pub(super) fn restore_source(
    data_root: &Path,
    source: &MoveJournalFileRef,
    original: &[u8],
) -> Result<(), RollbackFailure> {
    match read_optional(data_root, source)
        .map_err(|error| fs_failure(RecoveryFailurePoint::RestoreSource, error))?
    {
        Some(current) if current == original => Ok(()),
        None => replace_file(data_root, source, original)
            .map_err(|error| fs_failure(RecoveryFailurePoint::RestoreSource, error)),
        Some(_) => Err(RollbackFailure::new(
            RecoveryFailurePoint::RestoreSource,
            Some(source.path.clone()),
        )),
    }
}

pub(super) fn restore_destination(
    data_root: &Path,
    destination: &MoveJournalFileRef,
    original: &[u8],
    moved: &[u8],
) -> Result<(), RollbackFailure> {
    match read_optional(data_root, destination)
        .map_err(|error| fs_failure(RecoveryFailurePoint::RestoreDestination, error))?
    {
        Some(current) if current == original => Ok(()),
        Some(current) if current == moved => replace_file(data_root, destination, original)
            .map_err(|error| fs_failure(RecoveryFailurePoint::RestoreDestination, error)),
        None => replace_file(data_root, destination, original)
            .map_err(|error| fs_failure(RecoveryFailurePoint::RestoreDestination, error)),
        Some(_) => Err(RollbackFailure::new(
            RecoveryFailurePoint::RestoreDestination,
            Some(destination.path.clone()),
        )),
    }
}

pub(super) fn fs_failure(step: RecoveryFailurePoint, error: RecoveryFsError) -> RollbackFailure {
    RollbackFailure::new(step, Some(error.path().clone()))
}

#[derive(Debug)]
pub(crate) struct RollbackFailure {
    pub(crate) step: RecoveryFailurePoint,
    pub(crate) path: Option<VaultRelativePath>,
}

impl RollbackFailure {
    pub(crate) const fn new(step: RecoveryFailurePoint, path: Option<VaultRelativePath>) -> Self {
        Self { step, path }
    }

    pub(crate) const fn persist_progress() -> Self {
        Self::new(RecoveryFailurePoint::PersistProgress, None)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecoveryFailurePoint {
    LoadBackup,
    RestoreSource,
    RestoreDestination,
    RemoveCreatedDestination,
    CleanStagedFile,
    PersistProgress,
}

impl RecoveryFailurePoint {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::LoadBackup => "load-backup",
            Self::RestoreSource => "restore-source",
            Self::RestoreDestination => "restore-destination",
            Self::RemoveCreatedDestination => "remove-created-destination",
            Self::CleanStagedFile => "clean-staged-file",
            Self::PersistProgress => "persist-progress",
        }
    }
}

#[cfg(not(test))]
pub(super) fn maybe_fail(
    _point: RecoveryFailurePoint,
    _path: Option<&VaultRelativePath>,
) -> Result<(), RollbackFailure> {
    Ok(())
}

#[cfg(test)]
std::thread_local! {
    static FAILURE_POINT: std::cell::RefCell<Option<RecoveryFailurePoint>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn maybe_fail(
    point: RecoveryFailurePoint,
    path: Option<&VaultRelativePath>,
) -> Result<(), RollbackFailure> {
    FAILURE_POINT.with(|configured| {
        let mut configured = configured.borrow_mut();
        if configured
            .as_ref()
            .is_some_and(|configured| *configured == point)
        {
            *configured = None;
            return Err(RollbackFailure::new(point, path.cloned()));
        }
        Ok(())
    })
}

#[cfg(test)]
pub(crate) fn inject_failure(point: RecoveryFailurePoint) {
    FAILURE_POINT.with(|configured| *configured.borrow_mut() = Some(point));
}
