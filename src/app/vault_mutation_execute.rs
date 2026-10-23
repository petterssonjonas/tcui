use std::fmt;
use std::path::Path;

use crate::app::message::Message;
use crate::config::vault_profiles::{VaultCatalog, VaultProfileId};
use crate::storage::Storage;
use crate::vault::move_execute::{MoveExecuteError, MoveExecutor};
use crate::vault::mutation_plan::VaultMutationKind;
use crate::vault::watcher::VaultWatchChange;
use crate::vault::{
    VaultProfileRegistration, VaultRelativePath, VaultWriteError, VaultWriteService,
};

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct VaultMutationAffectedPath {
    pub(crate) profile_id: VaultProfileId,
    pub(crate) path: VaultRelativePath,
    pub(crate) change: VaultWatchChange,
}

impl fmt::Debug for VaultMutationAffectedPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VaultMutationAffectedPath")
            .field("profile_id", &self.profile_id)
            .field("path", &self.path)
            .field("change", &self.change)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct VaultMutationToolResult {
    operation_id: String,
    mutation: &'static str,
    affected: Vec<VaultMutationAffectedPath>,
}

impl VaultMutationToolResult {
    pub(crate) fn operation_id(&self) -> &str {
        &self.operation_id
    }

    pub(crate) fn message(&self, conversation_id: i64) -> Message {
        let affected = self
            .affected
            .iter()
            .map(|item| {
                serde_json::json!({
                    "profile_id": item.profile_id.to_string(),
                    "path": item.path.display().to_string(),
                    "change": format!("{:?}", item.change).to_ascii_lowercase(),
                })
            })
            .collect::<Vec<_>>();
        let result = serde_json::json!({
            "status": "complete",
            "operation_id": self.operation_id,
            "mutation": self.mutation,
            "affected": affected,
        })
        .to_string();
        let mut message = Message::new(conversation_id, "tool".to_string(), result.clone());
        message.tool_source = Some("vault".to_string());
        message.tool_result = Some(result);
        message
    }
}

impl fmt::Debug for VaultMutationToolResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VaultMutationToolResult")
            .field("operation_id", &self.operation_id)
            .field("mutation", &self.mutation)
            .field("affected", &self.affected)
            .finish()
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum VaultMutationExecutionError {
    #[error("{message}")]
    Failed { message: &'static str },
    #[error("Vault recovery is required for operation {operation_id}; run @vault restore {operation_id}")]
    RecoveryRequired { operation_id: String },
}

pub(crate) fn execute(
    kind: &VaultMutationKind,
    catalog: &VaultCatalog,
    data_root: &Path,
) -> Result<VaultMutationToolResult, VaultMutationExecutionError> {
    let affected = affected_paths(kind);
    match kind {
        VaultMutationKind::Write(plan) => {
            let service = write_service(catalog, data_root)?;
            let outcome = service
                .write(&plan.profile_id, &plan.path, plan.replacement.as_bytes())
                .map_err(map_write_error)?;
            Ok(VaultMutationToolResult {
                operation_id: outcome.backup_id.as_str().to_owned(),
                mutation: "write",
                affected,
            })
        }
        VaultMutationKind::Restore(plan) => {
            let service = write_service(catalog, data_root)?;
            let outcome = service
                .restore(&plan.profile_id, &plan.path, &plan.backup_id)
                .map_err(map_write_error)?;
            Ok(VaultMutationToolResult {
                operation_id: outcome.replaced_content_backup_id.as_str().to_owned(),
                mutation: "restore",
                affected,
            })
        }
        VaultMutationKind::Move(plan) => {
            let outcome = MoveExecutor::at_data_root(catalog, data_root)
                .execute(plan.clone())
                .map_err(map_move_error)?;
            Ok(VaultMutationToolResult {
                operation_id: outcome.operation_id.to_string(),
                mutation: "move",
                affected,
            })
        }
    }
}

pub(crate) fn affected_paths(kind: &VaultMutationKind) -> Vec<VaultMutationAffectedPath> {
    match kind {
        VaultMutationKind::Write(plan) => file_refresh(&plan.profile_id, &plan.path),
        VaultMutationKind::Restore(plan) => file_refresh(&plan.profile_id, &plan.path),
        VaultMutationKind::Move(plan) => plan
            .pairs()
            .iter()
            .flat_map(|pair| {
                [
                    VaultMutationAffectedPath {
                        profile_id: pair.source().profile_id().clone(),
                        path: pair.source().path().clone(),
                        change: VaultWatchChange::Remove,
                    },
                    VaultMutationAffectedPath {
                        profile_id: pair.destination().profile_id().clone(),
                        path: pair.destination().path().clone(),
                        change: VaultWatchChange::Create,
                    },
                ]
            })
            .collect(),
    }
}

fn file_refresh(
    profile_id: &VaultProfileId,
    path: &VaultRelativePath,
) -> Vec<VaultMutationAffectedPath> {
    vec![VaultMutationAffectedPath {
        profile_id: profile_id.clone(),
        path: path.clone(),
        change: VaultWatchChange::Create,
    }]
}

fn write_service(
    catalog: &VaultCatalog,
    data_root: &Path,
) -> Result<VaultWriteService<VaultProfileId>, VaultMutationExecutionError> {
    let registrations = catalog
        .profiles
        .values()
        .map(|profile| VaultProfileRegistration::new(profile.id.clone(), profile.root.clone()));
    #[cfg(test)]
    {
        VaultWriteService::at_data_root(registrations, data_root).map_err(map_write_error)
    }
    #[cfg(not(test))]
    {
        let _ = data_root;
        VaultWriteService::new(registrations).map_err(map_write_error)
    }
}

fn map_write_error(error: VaultWriteError) -> VaultMutationExecutionError {
    match error {
        VaultWriteError::MoveRecoveryRequired { operation_id } => {
            VaultMutationExecutionError::RecoveryRequired { operation_id }
        }
        VaultWriteError::VaultBusy => VaultMutationExecutionError::Failed {
            message: "Vault is busy with another mutation",
        },
        VaultWriteError::UnknownProfile { .. }
        | VaultWriteError::Symlink { .. }
        | VaultWriteError::NotFile { .. }
        | VaultWriteError::NotDirectory { .. }
        | VaultWriteError::InvalidUtf8 { .. }
        | VaultWriteError::BackupTargetMismatch
        | VaultWriteError::InvalidDataRoot
        | VaultWriteError::UnsupportedMoveJournalVersion { .. }
        | VaultWriteError::MoveJournalBlocked { .. }
        | VaultWriteError::Backup(_)
        | VaultWriteError::EditorTarget(_)
        | VaultWriteError::Io { .. } => VaultMutationExecutionError::Failed {
            message: "Vault write failed; no partial success was reported",
        },
        #[cfg(test)]
        VaultWriteError::InjectedFailure { .. } => VaultMutationExecutionError::Failed {
            message: "Vault write failed; no partial success was reported",
        },
    }
}

fn map_move_error(error: MoveExecuteError) -> VaultMutationExecutionError {
    match error {
        MoveExecuteError::RecoveryRequired { operation_id } => {
            VaultMutationExecutionError::RecoveryRequired {
                operation_id: operation_id.to_string(),
            }
        }
        MoveExecuteError::Write(VaultWriteError::MoveRecoveryRequired { operation_id }) => {
            VaultMutationExecutionError::RecoveryRequired { operation_id }
        }
        MoveExecuteError::Write(VaultWriteError::VaultBusy) => {
            VaultMutationExecutionError::Failed {
                message: "Vault is busy with another mutation",
            }
        }
        MoveExecuteError::Write(_)
        | MoveExecuteError::Plan(_)
        | MoveExecuteError::Backup(_)
        | MoveExecuteError::UnknownJournalVersion { .. }
        | MoveExecuteError::UnapprovedCollision { .. }
        | MoveExecuteError::Io { .. }
        | MoveExecuteError::Journal(_) => VaultMutationExecutionError::Failed {
            message: "Vault move failed; rollback completed or no disk work started",
        },
        #[cfg(test)]
        MoveExecuteError::InjectedFailure { .. } => VaultMutationExecutionError::Failed {
            message: "Vault move failed; rollback completed or no disk work started",
        },
    }
}

pub(crate) fn persist_user_message(
    storage: &Storage,
    message: &Message,
    _completed_vault_mutation: bool,
) -> color_eyre::Result<i64> {
    #[cfg(test)]
    if _completed_vault_mutation && CHAT_PERSISTENCE_FAILURE.with(std::cell::Cell::take) {
        return Err(color_eyre::eyre::eyre!(
            "injected Vault Chat persistence failure"
        ));
    }
    storage.save_message(message)
}

#[cfg(test)]
thread_local! {
    static CHAT_PERSISTENCE_FAILURE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
pub(crate) fn inject_chat_persistence_failure_once() {
    CHAT_PERSISTENCE_FAILURE.set(true);
}
