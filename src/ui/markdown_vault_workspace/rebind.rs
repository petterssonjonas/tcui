use crate::config::vault_profiles::{VaultCatalog, VaultProfileId};
use crate::ui::app_tabs::{AppTabId, AppTabs};
use crate::ui::modals::terminal_editor::CloseRequest;

use super::registry::{MarkdownVaultWorkspaceError, MarkdownVaultWorkspaceRegistry};
use super::state::{MarkdownVaultWorkspaceState, MarkdownVaultWorkspaceStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MarkdownVaultWorkspaceRebindRequest {
    Rebound,
    ConfirmationRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MarkdownVaultWorkspaceRebindCancel {
    Cancelled,
    NotPending,
    UnknownTab,
}

impl MarkdownVaultWorkspaceRegistry {
    pub(crate) fn request_rebind(
        &mut self,
        tabs: &mut AppTabs,
        catalog: &VaultCatalog,
        tab_id: AppTabId,
        profile_id: &VaultProfileId,
    ) -> Result<MarkdownVaultWorkspaceRebindRequest, MarkdownVaultWorkspaceError> {
        self.validate_bindings(tabs)?;
        self.validate_rebind_target(tab_id, profile_id)?;
        catalog.validate()?;
        if catalog.get(profile_id).is_none() {
            return Err(MarkdownVaultWorkspaceError::UnknownProfile {
                profile_id: profile_id.clone(),
            });
        }
        let state = self
            .states
            .get_mut(&tab_id)
            .ok_or(MarkdownVaultWorkspaceError::DescriptorMismatch { tab_id })?;
        if state.editor.as_mut().is_some_and(|editor| {
            matches!(editor.request_close(), CloseRequest::ConfirmationRequired)
        }) {
            state.pending_rebind_profile_id = Some(profile_id.clone());
            state.status = MarkdownVaultWorkspaceStatus::RebindConfirmationRequired {
                profile_id: profile_id.clone(),
            };
            return Ok(MarkdownVaultWorkspaceRebindRequest::ConfirmationRequired);
        }
        let replacement = MarkdownVaultWorkspaceState::construct(catalog, profile_id, tab_id)?;
        tabs.rebind_markdown_vault(tab_id, state.profile_id(), profile_id)
            .map_err(|_| MarkdownVaultWorkspaceError::DescriptorMismatch { tab_id })?;
        self.states.insert(tab_id, replacement);
        Ok(MarkdownVaultWorkspaceRebindRequest::Rebound)
    }

    pub(crate) fn confirm_rebind(
        &mut self,
        tabs: &mut AppTabs,
        catalog: &VaultCatalog,
        tab_id: AppTabId,
    ) -> Result<(), MarkdownVaultWorkspaceError> {
        self.validate_bindings(tabs)?;
        let (current_profile_id, pending_profile_id) = {
            let state = self
                .states
                .get(&tab_id)
                .ok_or(MarkdownVaultWorkspaceError::DescriptorMismatch { tab_id })?;
            let pending = state
                .pending_rebind_profile_id
                .clone()
                .ok_or(MarkdownVaultWorkspaceError::RebindNotPending { tab_id })?;
            (state.profile_id().clone(), pending)
        };
        let replacement =
            match MarkdownVaultWorkspaceState::construct(catalog, &pending_profile_id, tab_id) {
                Ok(replacement) => replacement,
                Err(error) => {
                    self.cancel_rebind(tab_id);
                    return Err(error);
                }
            };
        tabs.rebind_markdown_vault(tab_id, &current_profile_id, &pending_profile_id)
            .map_err(|_| MarkdownVaultWorkspaceError::DescriptorMismatch { tab_id })?;
        if let Some(editor) = self
            .states
            .get_mut(&tab_id)
            .and_then(|state| state.editor.as_mut())
        {
            editor.confirm_close();
        }
        self.states.insert(tab_id, replacement);
        Ok(())
    }

    pub(crate) fn cancel_rebind(&mut self, tab_id: AppTabId) -> MarkdownVaultWorkspaceRebindCancel {
        let Some(state) = self.states.get_mut(&tab_id) else {
            return MarkdownVaultWorkspaceRebindCancel::UnknownTab;
        };
        if state.pending_rebind_profile_id.take().is_none() {
            return MarkdownVaultWorkspaceRebindCancel::NotPending;
        }
        if let Some(editor) = state.editor.as_mut() {
            editor.cancel_close();
        }
        state.status = MarkdownVaultWorkspaceStatus::Ready;
        MarkdownVaultWorkspaceRebindCancel::Cancelled
    }

    fn validate_rebind_target(
        &self,
        tab_id: AppTabId,
        profile_id: &VaultProfileId,
    ) -> Result<(), MarkdownVaultWorkspaceError> {
        if self
            .states
            .get(&tab_id)
            .is_some_and(|state| state.profile_id() == profile_id)
        {
            return Err(MarkdownVaultWorkspaceError::ProfileAlreadyBoundToTab {
                tab_id,
                profile_id: profile_id.clone(),
            });
        }
        if let Some(existing_id) = self.tab_for_profile(profile_id) {
            return Err(MarkdownVaultWorkspaceError::ProfileAlreadyBoundToTab {
                tab_id: existing_id,
                profile_id: profile_id.clone(),
            });
        }
        Ok(())
    }
}
