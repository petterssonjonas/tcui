use std::path::Path;

use crate::config::vault_profiles::VaultCatalog;
use crate::vault::mutation_plan::{
    VaultMutationKind, VaultMutationPlanError, VaultMutationPreview,
};

use super::PreparedUserTurn;
use crate::app::vault_mutation_execute::VaultMutationToolResult;

#[derive(Debug, Clone)]
pub(crate) struct PendingVaultMutation {
    pub(crate) submission: PreparedUserTurn,
    pub(crate) kind: VaultMutationKind,
    pub(crate) preview: VaultMutationPreview,
    pub(crate) scroll: usize,
    pub(crate) state: VaultMutationConfirmationState,
    origin: VaultMutationOrigin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VaultMutationOrigin {
    Chat,
    Workspace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum VaultMutationConfirmationState {
    Reviewing {
        error: Option<String>,
    },
    ReadyForExecution,
    ExecutionFailed {
        error: String,
    },
    CompletedAwaitingChat {
        operation_id: String,
        submission: VaultMutationToolResult,
    },
    RecoveryRequired {
        operation_id: String,
        recovery_status: String,
    },
}

impl PendingVaultMutation {
    pub(crate) fn new(
        submission: PreparedUserTurn,
        kind: VaultMutationKind,
        catalog: &VaultCatalog,
    ) -> Result<Self, VaultMutationPlanError> {
        let preview = kind.preview(catalog)?;
        Ok(Self {
            submission,
            kind,
            preview,
            scroll: 0,
            state: VaultMutationConfirmationState::Reviewing { error: None },
            origin: VaultMutationOrigin::Chat,
        })
    }

    pub(crate) fn new_workspace(
        submission: PreparedUserTurn,
        kind: VaultMutationKind,
        catalog: &VaultCatalog,
    ) -> Result<Self, VaultMutationPlanError> {
        let preview = kind.preview(catalog)?;
        Ok(Self {
            submission,
            kind,
            preview,
            scroll: 0,
            state: VaultMutationConfirmationState::Reviewing { error: None },
            origin: VaultMutationOrigin::Workspace,
        })
    }

    pub(crate) fn is_workspace(&self) -> bool {
        self.origin == VaultMutationOrigin::Workspace
    }

    pub(crate) fn confirm(
        &mut self,
        catalog: &VaultCatalog,
        data_root: &Path,
    ) -> Result<(), VaultMutationPlanError> {
        if !self.preview.confirm_enabled {
            let error = VaultMutationPlanError::UnapprovedCollision;
            self.state = VaultMutationConfirmationState::Reviewing {
                error: Some(error.to_string()),
            };
            return Err(error);
        }
        match self.kind.revalidate(catalog, data_root) {
            Ok(()) => {
                self.state = VaultMutationConfirmationState::ReadyForExecution;
                Ok(())
            }
            Err(error) => {
                self.state = VaultMutationConfirmationState::Reviewing {
                    error: Some(error.to_string()),
                };
                Err(error)
            }
        }
    }

    pub(crate) fn can_cancel(&self) -> bool {
        matches!(
            self.state,
            VaultMutationConfirmationState::Reviewing { .. }
                | VaultMutationConfirmationState::ExecutionFailed { .. }
                | VaultMutationConfirmationState::RecoveryRequired { .. }
        )
    }

    pub(crate) fn can_confirm(&self) -> bool {
        match self.state {
            VaultMutationConfirmationState::Reviewing { .. } => self.preview.confirm_enabled,
            VaultMutationConfirmationState::ExecutionFailed { .. }
            | VaultMutationConfirmationState::CompletedAwaitingChat { .. } => true,
            VaultMutationConfirmationState::ReadyForExecution
            | VaultMutationConfirmationState::RecoveryRequired { .. } => false,
        }
    }

    pub(crate) fn completed_submission(&self) -> Option<&VaultMutationToolResult> {
        match &self.state {
            VaultMutationConfirmationState::CompletedAwaitingChat { submission, .. } => {
                Some(submission)
            }
            VaultMutationConfirmationState::Reviewing { .. }
            | VaultMutationConfirmationState::ReadyForExecution
            | VaultMutationConfirmationState::ExecutionFailed { .. }
            | VaultMutationConfirmationState::RecoveryRequired { .. } => None,
        }
    }

    pub(crate) fn scroll_down(&mut self, rows: usize) {
        self.scroll = self
            .scroll
            .saturating_add(rows)
            .min(self.preview.rows.len().saturating_sub(1));
    }

    pub(crate) fn scroll_up(&mut self, rows: usize) {
        self.scroll = self.scroll.saturating_sub(rows);
    }
}
