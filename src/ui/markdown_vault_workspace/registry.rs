use std::collections::HashMap;

use crate::config::vault_profiles::{VaultCatalog, VaultProfileError, VaultProfileId};
use crate::tui::vault_profile_ui::{activate_profile, LauncherActivation};
use crate::ui::app_tabs::{AppTabId, AppTabs, AppView, CloseTabResult};
use crate::vault::watcher::VaultWatcherError;
use crate::vault::VaultFsError;

use super::state::MarkdownVaultWorkspaceState;
use super::state::{MarkdownVaultContentPane, MarkdownVaultFocus};

#[derive(Debug, thiserror::Error)]
pub(crate) enum MarkdownVaultWorkspaceError {
    #[error(transparent)]
    Profile(#[from] VaultProfileError),
    #[error(transparent)]
    FileSystem(#[from] VaultFsError),
    #[error(transparent)]
    Watcher(#[from] VaultWatcherError),
    #[error("unknown vault profile: {profile_id}")]
    UnknownProfile { profile_id: VaultProfileId },
    #[error("vault profile is unavailable: {profile_id}")]
    ProfileUnavailable { profile_id: VaultProfileId },
    #[error("workspace is already bound to app tab {tab_id:?}")]
    TabAlreadyBound { tab_id: AppTabId },
    #[error("vault profile {profile_id} is already bound to app tab {tab_id:?}")]
    ProfileAlreadyBoundToTab {
        tab_id: AppTabId,
        profile_id: VaultProfileId,
    },
    #[error("workspace descriptor does not match state for app tab {tab_id:?}")]
    DescriptorMismatch { tab_id: AppTabId },
    #[error("workspace rebind was not pending for app tab {tab_id:?}")]
    RebindNotPending { tab_id: AppTabId },
    #[error("vault launcher activation failed: {message}")]
    Activation { message: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MarkdownVaultWorkspaceActivation {
    Replaced(AppTabId),
    FocusedExisting(AppTabId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MarkdownVaultWorkspaceRemoval {
    Removed { active: AppTabId },
    UnknownTab,
    DescriptorMismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MarkdownVaultWorkspacePtyPoll {
    UnknownTab,
    Idle,
    Running,
    Finished,
}

#[derive(Default)]
pub(crate) struct MarkdownVaultWorkspaceRegistry {
    pub(super) states: HashMap<AppTabId, MarkdownVaultWorkspaceState>,
}

impl MarkdownVaultWorkspaceRegistry {
    pub(crate) fn len(&self) -> usize {
        self.states.len()
    }

    pub(crate) fn get(&self, tab_id: AppTabId) -> Option<&MarkdownVaultWorkspaceState> {
        self.states.get(&tab_id)
    }

    pub(crate) fn get_mut(&mut self, tab_id: AppTabId) -> Option<&mut MarkdownVaultWorkspaceState> {
        self.states.get_mut(&tab_id)
    }

    #[cfg(test)]
    pub(crate) fn get_mut_for_test(
        &mut self,
        tab_id: AppTabId,
    ) -> Option<&mut MarkdownVaultWorkspaceState> {
        self.get_mut(tab_id)
    }

    pub(crate) fn activate(
        &mut self,
        tabs: &mut AppTabs,
        catalog: &VaultCatalog,
        launcher_id: AppTabId,
        profile_id: &VaultProfileId,
    ) -> Result<MarkdownVaultWorkspaceActivation, MarkdownVaultWorkspaceError> {
        self.validate_bindings(tabs)?;
        if let Some(existing_id) = self.tab_for_profile(profile_id) {
            let activation =
                activate_profile(tabs, launcher_id, profile_id.clone()).map_err(|error| {
                    MarkdownVaultWorkspaceError::Activation {
                        message: error.to_string(),
                    }
                })?;
            return match activation {
                LauncherActivation::FocusedExisting(id) if id == existing_id => {
                    Ok(MarkdownVaultWorkspaceActivation::FocusedExisting(id))
                }
                LauncherActivation::Replaced(id) | LauncherActivation::FocusedExisting(id) => {
                    Err(MarkdownVaultWorkspaceError::DescriptorMismatch { tab_id: id })
                }
            };
        }
        if self.states.contains_key(&launcher_id) {
            return Err(MarkdownVaultWorkspaceError::TabAlreadyBound {
                tab_id: launcher_id,
            });
        }
        let state = MarkdownVaultWorkspaceState::construct(catalog, profile_id, launcher_id)?;
        let activation =
            activate_profile(tabs, launcher_id, profile_id.clone()).map_err(|error| {
                MarkdownVaultWorkspaceError::Activation {
                    message: error.to_string(),
                }
            })?;
        match activation {
            LauncherActivation::Replaced(tab_id) => {
                self.states.insert(tab_id, state);
                Ok(MarkdownVaultWorkspaceActivation::Replaced(tab_id))
            }
            LauncherActivation::FocusedExisting(tab_id) => {
                Err(MarkdownVaultWorkspaceError::DescriptorMismatch { tab_id })
            }
        }
    }

    pub(crate) fn close(
        &mut self,
        tabs: &mut AppTabs,
        tab_id: AppTabId,
    ) -> MarkdownVaultWorkspaceRemoval {
        let Some(state) = self.states.get(&tab_id) else {
            return MarkdownVaultWorkspaceRemoval::UnknownTab;
        };
        if !descriptor_matches(tabs, tab_id, state.profile_id()) {
            return MarkdownVaultWorkspaceRemoval::DescriptorMismatch;
        }
        match tabs.close(tab_id) {
            CloseTabResult::Closed { active, .. } => {
                self.states.remove(&tab_id);
                MarkdownVaultWorkspaceRemoval::Removed { active }
            }
            CloseTabResult::Permanent { .. } | CloseTabResult::Unknown { .. } => {
                MarkdownVaultWorkspaceRemoval::DescriptorMismatch
            }
        }
    }

    pub(crate) fn poll_editor(&mut self, tab_id: AppTabId) -> MarkdownVaultWorkspacePtyPoll {
        let Some(state) = self.states.get_mut(&tab_id) else {
            return MarkdownVaultWorkspacePtyPoll::UnknownTab;
        };
        let Some(editor) = state.editor.as_mut() else {
            return MarkdownVaultWorkspacePtyPoll::Idle;
        };
        if editor.poll_output() {
            state.editor = None;
            state.editor_path = None;
            state.editor_changed_on_disk = false;
            if state.focus == MarkdownVaultFocus::Editor {
                state.focus = MarkdownVaultFocus::Viewer;
                state.content_pane = MarkdownVaultContentPane::Viewer;
            }
            MarkdownVaultWorkspacePtyPoll::Finished
        } else {
            MarkdownVaultWorkspacePtyPoll::Running
        }
    }

    pub(super) fn tab_for_profile(&self, profile_id: &VaultProfileId) -> Option<AppTabId> {
        self.states
            .iter()
            .find_map(|(tab_id, state)| (state.profile_id() == profile_id).then_some(*tab_id))
    }

    pub(super) fn validate_bindings(
        &self,
        tabs: &AppTabs,
    ) -> Result<(), MarkdownVaultWorkspaceError> {
        for (tab_id, state) in &self.states {
            if !descriptor_matches(tabs, *tab_id, state.profile_id()) {
                return Err(MarkdownVaultWorkspaceError::DescriptorMismatch { tab_id: *tab_id });
            }
        }
        for tab in tabs.tabs() {
            let AppView::MarkdownVault { profile_id } = tab.view() else {
                continue;
            };
            if self
                .states
                .get(&tab.id())
                .is_none_or(|state| state.profile_id() != profile_id)
            {
                return Err(MarkdownVaultWorkspaceError::DescriptorMismatch { tab_id: tab.id() });
            }
        }
        Ok(())
    }
}

fn descriptor_matches(tabs: &AppTabs, tab_id: AppTabId, profile_id: &VaultProfileId) -> bool {
    tabs.tab(tab_id).is_some_and(|tab| {
        matches!(tab.view(), AppView::MarkdownVault { profile_id: descriptor_id } if descriptor_id == profile_id)
    })
}
