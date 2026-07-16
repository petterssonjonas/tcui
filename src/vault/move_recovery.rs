use std::path::Path;
use std::time::Duration;

use crate::config::vault_profiles::VaultCatalog;

use super::backup::BackupStore;
use super::move_journal::{JournalStore, MoveJournalError, MoveJournalState, MoveJournalV1};
#[cfg(test)]
pub(crate) use super::move_rollback::inject_failure;
use super::move_rollback::MoveRollback;
pub(crate) use super::move_rollback::{RecoveryFailurePoint, RollbackFailure};
use super::{VaultMutationGuard, VaultMutationLock, VaultRelativePath, VaultWriteError};

const SECONDS_PER_DAY: u64 = 24 * 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MoveRecoveryConfig {
    pub(crate) retention_days: u64,
}

impl Default for MoveRecoveryConfig {
    fn default() -> Self {
        Self { retention_days: 7 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecoveryStatus {
    RolledBack,
    RecoveryRequired,
    Pruned,
    Retained,
}

impl RecoveryStatus {
    const fn label(self) -> &'static str {
        match self {
            Self::RolledBack => "rolled_back",
            Self::RecoveryRequired => "recovery_required",
            Self::Pruned => "pruned",
            Self::Retained => "retained",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecoveryRecord {
    pub(crate) operation_id: String,
    pub(crate) status: RecoveryStatus,
    pub(crate) paths: Vec<VaultRelativePath>,
    pub(crate) failed_step: Option<RecoveryFailurePoint>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct MoveRecoveryReport {
    records: Vec<RecoveryRecord>,
}

impl MoveRecoveryReport {
    pub(crate) fn blocked(&self) -> bool {
        self.records()
            .iter()
            .any(|record| record.status == RecoveryStatus::RecoveryRequired)
    }

    pub(crate) fn records(&self) -> &[RecoveryRecord] {
        &self.records
    }

    pub(crate) fn log_lines(&self) -> Vec<String> {
        self.records
            .iter()
            .map(|record| {
                let paths = record
                    .paths
                    .iter()
                    .map(render_relative_path)
                    .collect::<Vec<_>>()
                    .join(",");
                let step = record
                    .failed_step
                    .map_or("none", RecoveryFailurePoint::label);
                format!(
                    "vault.move_recovery operation_id={} status={} step={} paths={paths}",
                    record.operation_id,
                    record.status.label(),
                    step
                )
            })
            .collect()
    }
}

pub(crate) struct MoveRecoveryService<'a> {
    catalog: &'a VaultCatalog,
    mutation_lock: VaultMutationLock,
    journals: JournalStore,
    backups: BackupStore,
    config: MoveRecoveryConfig,
}

impl<'a> MoveRecoveryService<'a> {
    pub(crate) fn at_data_root(
        catalog: &'a VaultCatalog,
        data_root: &Path,
        config: MoveRecoveryConfig,
    ) -> Self {
        Self {
            catalog,
            mutation_lock: VaultMutationLock::at_data_root(data_root),
            journals: JournalStore::at_data_root(data_root),
            backups: BackupStore::new(data_root.join("vault-backups")),
            config,
        }
    }

    pub(crate) fn recover_startup(&self) -> Result<MoveRecoveryReport, VaultWriteError> {
        let guard = self.mutation_lock.try_acquire()?;
        let roots = self
            .journals
            .operation_roots()
            .map_err(redacted_store_error)?;
        let mut report = MoveRecoveryReport::default();
        for root in roots {
            match self.journals.load_operation_root(&root) {
                Ok(mut journal) => self.process_known(&guard, &root, &mut journal, &mut report),
                Err(_) => report.records.push(RecoveryRecord {
                    operation_id: operation_directory_name(&root),
                    status: RecoveryStatus::RecoveryRequired,
                    paths: Vec::new(),
                    failed_step: None,
                }),
            }
        }
        Ok(report)
    }

    fn process_known(
        &self,
        guard: &VaultMutationGuard,
        operation_root: &Path,
        journal: &mut MoveJournalV1,
        report: &mut MoveRecoveryReport,
    ) {
        match journal.state {
            MoveJournalState::Complete | MoveJournalState::RolledBack => {
                self.apply_retention(operation_root, journal, report);
            }
            MoveJournalState::Planned
            | MoveJournalState::Staging
            | MoveJournalState::DestinationsCommitted
            | MoveJournalState::SourcesRemoving
            | MoveJournalState::RollingBack
            | MoveJournalState::RecoveryRequired => {
                self.recover_known(guard, journal, report);
            }
        }
    }

    fn recover_known(
        &self,
        guard: &VaultMutationGuard,
        journal: &mut MoveJournalV1,
        report: &mut MoveRecoveryReport,
    ) {
        let operation_id = journal.operation_id.to_string();
        let paths = journal_paths(journal);
        let prepared = match journal.state {
            MoveJournalState::Planned
            | MoveJournalState::Staging
            | MoveJournalState::DestinationsCommitted
            | MoveJournalState::SourcesRemoving
            | MoveJournalState::RecoveryRequired => self
                .journals
                .transition(journal, MoveJournalState::RollingBack),
            MoveJournalState::RollingBack => Ok(()),
            MoveJournalState::Complete | MoveJournalState::RolledBack => {
                Err(MoveJournalError::InvalidTransition {
                    from: journal.state,
                    to: MoveJournalState::RollingBack,
                })
            }
        };
        let rollback = prepared
            .map_err(|_| RollbackFailure::new(RecoveryFailurePoint::PersistProgress, None))
            .and_then(|()| {
                MoveRollback::new(self.catalog, &self.journals, &self.backups)
                    .rollback_guarded(guard, journal)
            })
            .and_then(|()| {
                self.journals
                    .transition(journal, MoveJournalState::RolledBack)
                    .map_err(|_| RollbackFailure::new(RecoveryFailurePoint::PersistProgress, None))
            });
        match rollback {
            Ok(()) => report.records.push(RecoveryRecord {
                operation_id,
                status: RecoveryStatus::RolledBack,
                paths,
                failed_step: None,
            }),
            Err(failure) => {
                let _ = mark_recovery_required(&self.journals, journal);
                report.records.push(RecoveryRecord {
                    operation_id,
                    status: RecoveryStatus::RecoveryRequired,
                    paths: failure.path.into_iter().collect(),
                    failed_step: Some(failure.step),
                });
            }
        }
    }

    fn apply_retention(
        &self,
        operation_root: &Path,
        journal: &MoveJournalV1,
        report: &mut MoveRecoveryReport,
    ) {
        let age = Duration::from_secs(self.config.retention_days.saturating_mul(SECONDS_PER_DAY));
        let status = match self.journals.operation_is_older_than(operation_root, age) {
            Ok(true) => match self.journals.remove_operation_root(operation_root) {
                Ok(()) => RecoveryStatus::Pruned,
                Err(_) => RecoveryStatus::Retained,
            },
            Ok(false) | Err(_) => return,
        };
        report.records.push(RecoveryRecord {
            operation_id: journal.operation_id.to_string(),
            status,
            paths: Vec::new(),
            failed_step: None,
        });
    }
}

fn mark_recovery_required(
    journals: &JournalStore,
    journal: &mut MoveJournalV1,
) -> Result<(), MoveJournalError> {
    match journal.state {
        MoveJournalState::RecoveryRequired => Ok(()),
        MoveJournalState::Planned
        | MoveJournalState::Staging
        | MoveJournalState::DestinationsCommitted
        | MoveJournalState::SourcesRemoving
        | MoveJournalState::RollingBack => {
            journals.transition(journal, MoveJournalState::RecoveryRequired)
        }
        MoveJournalState::Complete | MoveJournalState::RolledBack => {
            Err(MoveJournalError::InvalidTransition {
                from: journal.state,
                to: MoveJournalState::RecoveryRequired,
            })
        }
    }
}

fn journal_paths(journal: &MoveJournalV1) -> Vec<VaultRelativePath> {
    let mut paths = Vec::new();
    for (pair, artifact) in journal.plan.pairs.iter().zip(&journal.artifacts) {
        paths.push(pair.source.path.clone());
        paths.push(pair.destination.path.clone());
        if let Some(staged) = &artifact.staged_path {
            paths.push(staged.clone());
        }
    }
    paths
}

fn operation_directory_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| {
            name.strip_prefix("move-").is_some_and(|hex| {
                hex.len() == 32
                    && hex
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            })
        })
        .map_or_else(|| "unknown".to_owned(), ToOwned::to_owned)
}

fn render_relative_path(path: &VaultRelativePath) -> String {
    path.as_path()
        .to_string_lossy()
        .chars()
        .flat_map(char::escape_default)
        .collect()
}

fn redacted_store_error(_error: MoveJournalError) -> VaultWriteError {
    VaultWriteError::MoveJournalBlocked {
        reason: "Vault move operation store could not be scanned".to_owned(),
    }
}
