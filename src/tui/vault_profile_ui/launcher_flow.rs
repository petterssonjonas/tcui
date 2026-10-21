use std::path::Path;

use crate::config::vault_profiles::{VaultCatalog, VaultProfileId};
use crate::ui::app_tabs::{AppTabId, AppTabs, AppView};

use super::manager::{
    LauncherActivation, LauncherEntry, VaultProfileManagerState, VaultProfilePhase,
    VaultProfileUiError,
};

pub(crate) fn launcher_entry(
    catalog: &VaultCatalog,
    launcher_id: AppTabId,
    start: &Path,
) -> Result<LauncherEntry, VaultProfileUiError> {
    match catalog.profiles.len() {
        0 => {
            let mut manager =
                VaultProfileManagerState::for_launcher(catalog.clone(), launcher_id, start)?;
            manager.begin_add(start)?;
            Ok(LauncherEntry::Add(manager))
        }
        1 => catalog
            .profiles
            .keys()
            .next()
            .cloned()
            .map(LauncherEntry::Activate)
            .ok_or(VaultProfileUiError::LauncherUnavailable),
        _ => {
            let mut manager =
                VaultProfileManagerState::for_launcher(catalog.clone(), launcher_id, start)?;
            manager.phase = VaultProfilePhase::LauncherChoice;
            if let Some(default_id) = catalog.default_id.as_ref() {
                manager.selected = manager
                    .profile_ids()
                    .iter()
                    .position(|id| *id == default_id)
                    .unwrap_or(0);
            }
            Ok(LauncherEntry::Choose(manager))
        }
    }
}

pub(crate) fn activate_profile(
    tabs: &mut AppTabs,
    launcher_id: AppTabId,
    profile_id: VaultProfileId,
) -> Result<LauncherActivation, VaultProfileUiError> {
    let existing = tabs.tabs().iter().find_map(|tab| match tab.view() {
        AppView::MarkdownVault {
            profile_id: open_id,
        } if open_id == &profile_id => Some(tab.id()),
        AppView::Chat
        | AppView::Launcher
        | AppView::MarkdownVault { .. }
        | AppView::Terminal { .. }
        | AppView::Settings => None,
        #[cfg(test)]
        AppView::Placeholder { .. } => None,
    });
    if let Some(existing_id) = existing {
        let _ = tabs.close(launcher_id);
        let _ = tabs.select(existing_id);
        return Ok(LauncherActivation::FocusedExisting(existing_id));
    }
    let replaced = tabs
        .replace_launcher(launcher_id, AppView::MarkdownVault { profile_id })
        .map_err(|_| VaultProfileUiError::LauncherUnavailable)?;
    let _ = tabs.select(replaced);
    Ok(LauncherActivation::Replaced(replaced))
}
