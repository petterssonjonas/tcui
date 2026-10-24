use std::collections::HashMap;

use crate::ui::app_tabs::{AppTabId, AppTabs, AppView, CloseTabResult, ReplaceLauncherError};
use crate::vault::watcher::VaultWatcherError;
use crate::vault::VaultFsError;

use super::state::VaultWorkspaceState;
use super::state::{VaultContentPane, VaultFocus};

#[derive(Debug, thiserror::Error)]
pub(crate) enum VaultWorkspaceError {
    #[error(transparent)]
    FileSystem(#[from] VaultFsError),
    #[error(transparent)]
    Watcher(#[from] VaultWatcherError),
    #[error("workspace descriptor does not match state for app tab {tab_id:?}")]
    DescriptorMismatch { tab_id: AppTabId },
    #[error("cannot activate the Vault tab: {0:?}")]
    Activation(ReplaceLauncherError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VaultWorkspaceActivation {
    Replaced(AppTabId),
    FocusedExisting(AppTabId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VaultWorkspaceRemoval {
    Removed { active: AppTabId },
    UnknownTab,
    DescriptorMismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VaultWorkspacePtyPoll {
    UnknownTab,
    Idle,
    Running,
    Finished,
}

#[derive(Default)]
pub(crate) struct VaultWorkspaceRegistry {
    pub(super) states: HashMap<AppTabId, VaultWorkspaceState>,
}

impl VaultWorkspaceRegistry {
    pub(crate) fn len(&self) -> usize {
        self.states.len()
    }

    pub(crate) fn get(&self, tab_id: AppTabId) -> Option<&VaultWorkspaceState> {
        self.states.get(&tab_id)
    }

    pub(crate) fn get_mut(&mut self, tab_id: AppTabId) -> Option<&mut VaultWorkspaceState> {
        self.states.get_mut(&tab_id)
    }

    #[cfg(test)]
    pub(crate) fn get_mut_for_test(
        &mut self,
        tab_id: AppTabId,
    ) -> Option<&mut VaultWorkspaceState> {
        self.get_mut(tab_id)
    }

    pub(crate) fn activate(
        &mut self,
        tabs: &mut AppTabs,
        launcher_id: AppTabId,
    ) -> Result<VaultWorkspaceActivation, VaultWorkspaceError> {
        self.validate_bindings(tabs)?;
        if tabs
            .tab(launcher_id)
            .is_none_or(|tab| tab.view() != &AppView::Launcher)
        {
            let error = if tabs.tab(launcher_id).is_some() {
                ReplaceLauncherError::NotLauncher(launcher_id)
            } else {
                ReplaceLauncherError::Unknown(launcher_id)
            };
            return Err(VaultWorkspaceError::Activation(error));
        }
        if let Some(existing_id) = self.states.keys().next().copied() {
            let _ = tabs.select(existing_id);
            return Ok(VaultWorkspaceActivation::FocusedExisting(existing_id));
        }
        let state = VaultWorkspaceState::construct(launcher_id)?;
        tabs.replace_launcher(launcher_id, AppView::Vault)
            .map_err(VaultWorkspaceError::Activation)?;
        self.states.insert(launcher_id, state);
        Ok(VaultWorkspaceActivation::Replaced(launcher_id))
    }

    pub(crate) fn close(&mut self, tabs: &mut AppTabs, tab_id: AppTabId) -> VaultWorkspaceRemoval {
        if !self.states.contains_key(&tab_id) {
            return VaultWorkspaceRemoval::UnknownTab;
        }
        if !descriptor_matches(tabs, tab_id) {
            return VaultWorkspaceRemoval::DescriptorMismatch;
        }
        match tabs.close(tab_id) {
            CloseTabResult::Closed { active, .. } => {
                self.states.remove(&tab_id);
                VaultWorkspaceRemoval::Removed { active }
            }
            CloseTabResult::Permanent { .. } | CloseTabResult::Unknown { .. } => {
                VaultWorkspaceRemoval::DescriptorMismatch
            }
        }
    }

    pub(crate) fn poll_editor(&mut self, tab_id: AppTabId) -> VaultWorkspacePtyPoll {
        let Some(state) = self.states.get_mut(&tab_id) else {
            return VaultWorkspacePtyPoll::UnknownTab;
        };
        let Some(editor) = state.editor.as_mut() else {
            return VaultWorkspacePtyPoll::Idle;
        };
        if editor.poll_output() {
            state.editor = None;
            state.editor_path = None;
            state.editor_changed_on_disk = false;
            if state.focus == VaultFocus::Editor {
                state.focus = VaultFocus::Viewer;
                state.content_pane = VaultContentPane::Viewer;
            }
            VaultWorkspacePtyPoll::Finished
        } else {
            VaultWorkspacePtyPoll::Running
        }
    }

    pub(super) fn validate_bindings(&self, tabs: &AppTabs) -> Result<(), VaultWorkspaceError> {
        for tab_id in self.states.keys() {
            if !descriptor_matches(tabs, *tab_id) {
                return Err(VaultWorkspaceError::DescriptorMismatch { tab_id: *tab_id });
            }
        }
        for tab in tabs.tabs() {
            let AppView::Vault = tab.view() else {
                continue;
            };
            if !self.states.contains_key(&tab.id()) {
                return Err(VaultWorkspaceError::DescriptorMismatch { tab_id: tab.id() });
            }
        }
        Ok(())
    }
}

fn descriptor_matches(tabs: &AppTabs, tab_id: AppTabId) -> bool {
    tabs.tab(tab_id)
        .is_some_and(|tab| tab.view() == &AppView::Vault)
}
