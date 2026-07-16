use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crossterm::event::KeyEvent;

use super::TuiApp;
use crate::config::vault_profiles::{VaultCatalog, VaultProfileId};
use crate::tui::vault_profile_ui::{
    launcher_entry, LauncherEntry, PendingVaultAction, VaultProfileManagerState,
    VaultProfileUiCommand, VaultProfileUiError,
};
use crate::ui::app_tabs::{AppTabId, AppView};
use crate::ui::modals::terminal_editor::CloseRequest;

impl TuiApp {
    pub(crate) fn open_vault_profile_manager(&mut self) {
        let catalog = self.vault_catalog();
        let start = picker_start(&catalog);
        match VaultProfileManagerState::for_settings(catalog, &start) {
            Ok(manager) => {
                self.ui.settings_v2 = None;
                self.ui.vault_profile_manager = Some(manager);
            }
            Err(error) => self.ui.show_toast(error.to_string()),
        }
    }

    pub(crate) fn open_markdown_vault_launcher(&mut self) {
        let Some(active_tab) = self.ui.app_tabs.active_tab() else {
            return;
        };
        if active_tab.view() != &AppView::Launcher {
            return;
        }
        let launcher_id = active_tab.id();
        let catalog = self.vault_catalog();
        let start = picker_start(&catalog);
        match launcher_entry(&catalog, launcher_id, &start) {
            Ok(LauncherEntry::Add(manager) | LauncherEntry::Choose(manager)) => {
                self.ui.vault_profile_manager = Some(manager);
            }
            Ok(LauncherEntry::Activate(profile_id)) => {
                let _ = self.request_vault_action(PendingVaultAction::Activate {
                    launcher_id,
                    profile_id,
                });
            }
            Err(error) => self.ui.show_toast(error.to_string()),
        }
    }

    pub(crate) fn handle_vault_profile_key(&mut self, key: KeyEvent) {
        let Some(mut manager) = self.ui.vault_profile_manager.take() else {
            return;
        };
        let bound = self.bound_vault_profiles();
        let command = manager.handle_key(key, &bound);
        let keep_open = self.apply_vault_profile_command(&mut manager, command);
        if keep_open {
            self.ui.vault_profile_manager = Some(manager);
        }
    }

    pub(crate) fn request_app_tab_close(&mut self, tab_id: AppTabId) {
        let _ = self.request_vault_action(PendingVaultAction::CloseTab { tab_id });
    }

    pub(crate) fn resolve_pending_vault_action_after_editor_key(&mut self) {
        if self.ui.pending_vault_action.is_none() {
            return;
        }
        let Some(editor) = self.ui.editor_popup.as_ref() else {
            self.ui.pending_vault_action = None;
            return;
        };
        if editor.is_done() {
            if let Some(action) = self.ui.pending_vault_action.take() {
                self.apply_vault_action(action);
                self.ui.vault_profile_manager = None;
            }
        } else if !editor.close_is_pending() {
            self.ui.pending_vault_action = None;
        }
    }

    fn apply_vault_profile_command(
        &mut self,
        manager: &mut VaultProfileManagerState,
        command: VaultProfileUiCommand,
    ) -> bool {
        match command {
            VaultProfileUiCommand::None => true,
            VaultProfileUiCommand::Dismiss => false,
            VaultProfileUiCommand::Activate {
                launcher_id,
                profile_id,
            } => !self.request_vault_action(PendingVaultAction::Activate {
                launcher_id,
                profile_id,
            }),
            VaultProfileUiCommand::Persist {
                catalog,
                activate_profile_id,
            } => {
                if let Err(error) = self.persist_vault_catalog(&catalog) {
                    manager.set_error(error);
                    return true;
                }
                manager.commit_catalog(catalog);
                let Some(profile_id) = activate_profile_id else {
                    return true;
                };
                let Some(launcher_id) = manager.launcher_id() else {
                    return true;
                };
                !self.request_vault_action(PendingVaultAction::Activate {
                    launcher_id,
                    profile_id,
                })
            }
        }
    }

    fn request_vault_action(&mut self, action: PendingVaultAction) -> bool {
        if let Some(editor) = self.ui.editor_popup.as_mut() {
            match editor.request_close() {
                CloseRequest::ConfirmationRequired => {
                    self.ui.pending_vault_action = Some(action);
                    return false;
                }
                CloseRequest::Closed => {}
            }
        }
        self.apply_vault_action(action);
        true
    }

    fn apply_vault_action(&mut self, action: PendingVaultAction) {
        match action {
            PendingVaultAction::Activate {
                launcher_id,
                profile_id,
            } => {
                let catalog = self.vault_catalog();
                if let Err(error) = self.ui.markdown_vault_workspaces.activate(
                    &mut self.ui.app_tabs,
                    &catalog,
                    launcher_id,
                    &profile_id,
                ) {
                    self.ui.show_toast(error.to_string());
                }
                self.ui.focus = crate::tui::focus::Focus::Chat;
            }
            PendingVaultAction::CloseTab { tab_id } => {
                if self.ui.markdown_vault_workspaces.get(tab_id).is_some() {
                    let _ = self
                        .ui
                        .markdown_vault_workspaces
                        .close(&mut self.ui.app_tabs, tab_id);
                } else {
                    let _ = self.ui.app_tabs.close(tab_id);
                }
                self.ui.focus = crate::tui::focus::Focus::Chat;
            }
        }
    }

    fn persist_vault_catalog(&mut self, catalog: &VaultCatalog) -> Result<(), VaultProfileUiError> {
        let mut config =
            self.config
                .try_write()
                .map_err(|error| VaultProfileUiError::Persistence {
                    message: error.to_string(),
                })?;
        let old_vaults = std::mem::replace(&mut config.vaults, catalog.profiles.clone());
        let old_default =
            std::mem::replace(&mut config.default_vault_id, catalog.default_id.clone());
        if let Err(error) = config.save() {
            config.vaults = old_vaults;
            config.default_vault_id = old_default;
            return Err(VaultProfileUiError::Persistence {
                message: error.to_string(),
            });
        }
        Ok(())
    }

    fn vault_catalog(&self) -> VaultCatalog {
        self.config
            .try_read()
            .map(|config| VaultCatalog {
                profiles: config.vaults.clone(),
                default_id: config.default_vault_id.clone(),
            })
            .unwrap_or_default()
    }

    fn bound_vault_profiles(&self) -> HashSet<VaultProfileId> {
        self.ui
            .app_tabs
            .tabs()
            .iter()
            .filter_map(|tab| match tab.view() {
                AppView::MarkdownVault { profile_id } => Some(profile_id.clone()),
                AppView::Chat | AppView::Launcher => None,
                #[cfg(test)]
                AppView::Placeholder { .. } => None,
            })
            .collect()
    }
}

fn picker_start(catalog: &VaultCatalog) -> PathBuf {
    catalog
        .get_default()
        .and_then(|profile| profile.root.parent())
        .map(Path::to_path_buf)
        .or_else(dirs::home_dir)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| Path::new(".").to_path_buf())
}
