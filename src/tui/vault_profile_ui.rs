mod directory_picker;
mod form_interaction;
mod interaction;
mod launcher_flow;
mod manager;
mod mouse_interaction;
mod mutations;
mod render;

pub(crate) use launcher_flow::{activate_profile, launcher_entry};
pub(crate) use manager::{
    LauncherActivation, LauncherEntry, PendingVaultAction, VaultProfileManagerState,
    VaultProfileUiCommand, VaultProfileUiError,
};

#[cfg(test)]
pub(crate) use manager::VaultProfilePhase;

#[cfg(test)]
#[path = "vault_profile_ui/tests.rs"]
mod vault_profile_ui;
