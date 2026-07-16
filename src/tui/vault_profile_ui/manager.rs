use std::path::{Path, PathBuf};

use crate::config::vault_profiles::{VaultCatalog, VaultProfileError, VaultProfileId};
use crate::ui::app_tabs::AppTabId;

use super::directory_picker::VaultDirectoryPickerState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VaultProfilePhase {
    Profiles,
    Actions,
    Directory,
    Name,
    Rename,
    ChangeFolder,
    LauncherChoice,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum VaultProfileUiError {
    #[error(transparent)]
    Profile(#[from] VaultProfileError),
    #[error("Close or rebind the open vault tab before changing this profile: {id}")]
    ProfileIsOpen { id: VaultProfileId },
    #[error("Directory is unavailable: {path}: {message}")]
    DirectoryUnavailable { path: PathBuf, message: String },
    #[error("Selection is not a directory: {path}")]
    NotDirectory { path: PathBuf },
    #[error("The selected launcher is no longer available")]
    LauncherUnavailable,
    #[error("Could not save vault profiles: {message}")]
    Persistence { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum VaultProfileUiCommand {
    None,
    Dismiss,
    Persist {
        catalog: VaultCatalog,
        activate_profile_id: Option<VaultProfileId>,
    },
    Activate {
        launcher_id: AppTabId,
        profile_id: VaultProfileId,
    },
}

#[derive(Debug)]
pub(crate) enum LauncherEntry {
    Add(VaultProfileManagerState),
    Activate(VaultProfileId),
    Choose(VaultProfileManagerState),
}

impl PartialEq for LauncherEntry {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Activate(left), Self::Activate(right)) => left == right,
            (Self::Add(_), Self::Add(_)) | (Self::Choose(_), Self::Choose(_)) => true,
            (Self::Add(_) | Self::Activate(_) | Self::Choose(_), _) => false,
        }
    }
}

impl Eq for LauncherEntry {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ManagerOrigin {
    Settings,
    Launcher(AppTabId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PendingVaultAction {
    Activate {
        launcher_id: AppTabId,
        profile_id: VaultProfileId,
    },
    CloseTab {
        tab_id: AppTabId,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LauncherActivation {
    Replaced(AppTabId),
    FocusedExisting(AppTabId),
}

#[derive(Debug, Clone)]
pub(crate) struct VaultProfileManagerState {
    pub(super) catalog: VaultCatalog,
    pub(super) origin: ManagerOrigin,
    pub(super) phase: VaultProfilePhase,
    pub(super) selected: usize,
    pub(super) action_selected: usize,
    pub(super) input: String,
    pub(super) pending_path: Option<PathBuf>,
    pub(super) pending_id: Option<VaultProfileId>,
    pub(super) picker: Option<VaultDirectoryPickerState>,
    pub(super) error: Option<VaultProfileUiError>,
}

impl VaultProfileManagerState {
    pub(crate) fn for_settings(
        catalog: VaultCatalog,
        start: &Path,
    ) -> Result<Self, VaultProfileUiError> {
        let picker = VaultDirectoryPickerState::new(start)?;
        Ok(Self::base(catalog, ManagerOrigin::Settings, picker))
    }

    pub(super) fn for_launcher(
        catalog: VaultCatalog,
        launcher_id: AppTabId,
        start: &Path,
    ) -> Result<Self, VaultProfileUiError> {
        let picker = VaultDirectoryPickerState::new(start)?;
        Ok(Self::base(
            catalog,
            ManagerOrigin::Launcher(launcher_id),
            picker,
        ))
    }

    fn base(
        catalog: VaultCatalog,
        origin: ManagerOrigin,
        picker: VaultDirectoryPickerState,
    ) -> Self {
        Self {
            catalog,
            origin,
            phase: VaultProfilePhase::Profiles,
            selected: 0,
            action_selected: 0,
            input: String::new(),
            pending_path: None,
            pending_id: None,
            picker: Some(picker),
            error: None,
        }
    }

    #[cfg(test)]
    pub(crate) const fn phase(&self) -> VaultProfilePhase {
        self.phase
    }

    #[cfg(test)]
    pub(crate) fn input(&self) -> &str {
        &self.input
    }

    #[cfg(test)]
    pub(crate) fn error(&self) -> Option<String> {
        self.error.as_ref().map(ToString::to_string)
    }

    #[cfg(test)]
    pub(crate) const fn last_error(&self) -> Option<&VaultProfileUiError> {
        self.error.as_ref()
    }

    #[cfg(test)]
    pub(crate) const fn catalog(&self) -> &VaultCatalog {
        &self.catalog
    }

    #[cfg(test)]
    pub(crate) fn set_input(&mut self, input: &str) {
        self.input.clear();
        self.input.push_str(input);
    }

    #[cfg(test)]
    pub(crate) fn cancel(&mut self) -> VaultProfileUiCommand {
        VaultProfileUiCommand::Dismiss
    }

    pub(crate) const fn launcher_id(&self) -> Option<AppTabId> {
        match self.origin {
            ManagerOrigin::Settings => None,
            ManagerOrigin::Launcher(id) => Some(id),
        }
    }

    pub(crate) fn selected_profile_id(&self) -> Option<&VaultProfileId> {
        self.profile_ids().get(self.selected).copied()
    }

    pub(super) fn profile_ids(&self) -> Vec<&VaultProfileId> {
        let mut profiles = self.catalog.profiles.values().collect::<Vec<_>>();
        profiles.sort_by(|left, right| {
            left.name_key
                .as_str()
                .cmp(right.name_key.as_str())
                .then_with(|| left.id.to_string().cmp(&right.id.to_string()))
        });
        profiles.into_iter().map(|profile| &profile.id).collect()
    }

    pub(super) fn fail(&mut self, error: VaultProfileUiError) -> VaultProfileUiCommand {
        self.error = Some(error);
        VaultProfileUiCommand::None
    }
}
