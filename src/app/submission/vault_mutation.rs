use std::path::Path;

use crate::config::vault_profiles::VaultCatalog;
use crate::vault::mutation_plan::{
    VaultMutationKind, VaultMutationPlanError, VaultMutationPreview,
};

use super::PreparedUserTurn;

#[derive(Debug, Clone)]
pub(crate) struct PendingVaultMutation {
    pub(crate) submission: PreparedUserTurn,
    pub(crate) kind: VaultMutationKind,
    pub(crate) preview: VaultMutationPreview,
    pub(crate) scroll: usize,
    pub(crate) state: VaultMutationConfirmationState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum VaultMutationConfirmationState {
    Reviewing { error: Option<String> },
    ReadyForExecution,
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
        })
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
        matches!(self.state, VaultMutationConfirmationState::Reviewing { .. })
    }

    pub(crate) fn can_confirm(&self) -> bool {
        self.preview.confirm_enabled
            && matches!(self.state, VaultMutationConfirmationState::Reviewing { .. })
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
