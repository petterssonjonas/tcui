use crate::config::vault_profiles::VaultCatalog;

use super::backup::{BackupStore, LoadedBackup, StandaloneBackupId};
use super::move_journal::{JournalStore, MoveJournalFileRef, MoveJournalV1};
use super::move_recovery_fs::{remove_if_matches, remove_regular_if_present};
#[cfg(test)]
pub(crate) use super::move_rollback_support::inject_failure;
use super::move_rollback_support::{fs_failure, maybe_fail, restore_destination, restore_source};
pub(crate) use super::move_rollback_support::{RecoveryFailurePoint, RollbackFailure};
use super::{VaultMutationGuard, VaultRelativePath};

pub(crate) struct MoveRollback<'a> {
    catalog: &'a VaultCatalog,
    journals: &'a JournalStore,
    backups: &'a BackupStore,
}

impl<'a> MoveRollback<'a> {
    pub(crate) const fn new(
        catalog: &'a VaultCatalog,
        journals: &'a JournalStore,
        backups: &'a BackupStore,
    ) -> Self {
        Self {
            catalog,
            journals,
            backups,
        }
    }

    pub(crate) fn rollback_guarded(
        &self,
        _guard: &VaultMutationGuard,
        journal: &mut MoveJournalV1,
    ) -> Result<(), RollbackFailure> {
        self.restore_sources(journal)?;
        self.restore_overwritten_destinations(journal)?;
        self.remove_created_destinations(journal)?;
        self.clean_staged_files(journal)
    }

    fn restore_sources(&self, journal: &mut MoveJournalV1) -> Result<(), RollbackFailure> {
        for index in (0..journal.artifacts.len()).rev() {
            if journal.artifacts[index].source_restored {
                continue;
            }
            let source = &journal.plan.pairs[index].source;
            maybe_fail(RecoveryFailurePoint::RestoreSource, Some(&source.path))?;
            if let Some(id) = &journal.artifacts[index].source_backup_id {
                let bytes = self.load_backup(id, source)?;
                restore_source(self.catalog, source, &bytes)?;
            }
            journal.artifacts[index].source_restored = true;
            self.persist_progress(journal, RecoveryFailurePoint::RestoreSource, &source.path)?;
        }
        Ok(())
    }

    fn restore_overwritten_destinations(
        &self,
        journal: &mut MoveJournalV1,
    ) -> Result<(), RollbackFailure> {
        for index in (0..journal.artifacts.len()).rev() {
            if journal.artifacts[index].overwritten_destination_restored {
                continue;
            }
            let pair = &journal.plan.pairs[index];
            maybe_fail(
                RecoveryFailurePoint::RestoreDestination,
                Some(&pair.destination.path),
            )?;
            if pair.overwrite {
                if let Some(id) = &journal.artifacts[index].destination_backup_id {
                    let original = self.load_backup(id, &pair.destination)?;
                    let moved = self.source_bytes(journal, index)?;
                    restore_destination(self.catalog, &pair.destination, &original, &moved)?;
                }
            }
            journal.artifacts[index].overwritten_destination_restored = true;
            self.persist_progress(
                journal,
                RecoveryFailurePoint::RestoreDestination,
                &pair.destination.path,
            )?;
        }
        Ok(())
    }

    fn remove_created_destinations(
        &self,
        journal: &mut MoveJournalV1,
    ) -> Result<(), RollbackFailure> {
        for index in (0..journal.artifacts.len()).rev() {
            if journal.artifacts[index].created_destination_removed {
                continue;
            }
            let pair = &journal.plan.pairs[index];
            maybe_fail(
                RecoveryFailurePoint::RemoveCreatedDestination,
                Some(&pair.destination.path),
            )?;
            if !pair.overwrite {
                if let Some(moved) = self.source_bytes_optional(journal, index)? {
                    remove_if_matches(self.catalog, &pair.destination, &moved).map_err(
                        |error| fs_failure(RecoveryFailurePoint::RemoveCreatedDestination, error),
                    )?;
                }
            }
            journal.artifacts[index].created_destination_removed = true;
            self.persist_progress(
                journal,
                RecoveryFailurePoint::RemoveCreatedDestination,
                &pair.destination.path,
            )?;
        }
        Ok(())
    }

    fn clean_staged_files(&self, journal: &mut MoveJournalV1) -> Result<(), RollbackFailure> {
        for index in (0..journal.artifacts.len()).rev() {
            if journal.artifacts[index].staged_file_cleaned {
                continue;
            }
            let pair = &journal.plan.pairs[index];
            let staged = journal.artifacts[index].staged_path.clone();
            let report_path = staged.as_ref().unwrap_or(&pair.destination.path).clone();
            maybe_fail(RecoveryFailurePoint::CleanStagedFile, Some(&report_path))?;
            if let Some(staged) = staged {
                let staged_ref = MoveJournalFileRef {
                    profile_id: pair.destination.profile_id.clone(),
                    path: staged,
                };
                remove_regular_if_present(self.catalog, &staged_ref)
                    .map_err(|error| fs_failure(RecoveryFailurePoint::CleanStagedFile, error))?;
            }
            journal.artifacts[index].staged_file_cleaned = true;
            self.persist_progress(journal, RecoveryFailurePoint::CleanStagedFile, &report_path)?;
        }
        Ok(())
    }

    fn source_bytes(
        &self,
        journal: &MoveJournalV1,
        index: usize,
    ) -> Result<Vec<u8>, RollbackFailure> {
        self.source_bytes_optional(journal, index)?.ok_or_else(|| {
            RollbackFailure::new(
                RecoveryFailurePoint::RestoreSource,
                Some(journal.plan.pairs[index].source.path.clone()),
            )
        })
    }

    fn source_bytes_optional(
        &self,
        journal: &MoveJournalV1,
        index: usize,
    ) -> Result<Option<Vec<u8>>, RollbackFailure> {
        let Some(id) = &journal.artifacts[index].source_backup_id else {
            return Ok(None);
        };
        self.load_backup(id, &journal.plan.pairs[index].source)
            .map(Some)
    }

    fn load_backup(
        &self,
        id: &StandaloneBackupId,
        target: &MoveJournalFileRef,
    ) -> Result<Vec<u8>, RollbackFailure> {
        let LoadedBackup { envelope, contents } = self.backups.load(id).map_err(|_| {
            RollbackFailure::new(RecoveryFailurePoint::LoadBackup, Some(target.path.clone()))
        })?;
        if envelope.profile_id != target.profile_id.to_string() || envelope.path != target.path {
            return Err(RollbackFailure::new(
                RecoveryFailurePoint::LoadBackup,
                Some(target.path.clone()),
            ));
        }
        contents.ok_or_else(|| {
            RollbackFailure::new(RecoveryFailurePoint::LoadBackup, Some(target.path.clone()))
        })
    }

    fn persist_progress(
        &self,
        journal: &MoveJournalV1,
        step: RecoveryFailurePoint,
        path: &VaultRelativePath,
    ) -> Result<(), RollbackFailure> {
        self.journals
            .persist(journal)
            .map_err(|_| RollbackFailure::new(step, Some(path.clone())))
    }
}
