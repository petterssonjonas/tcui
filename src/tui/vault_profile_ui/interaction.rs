use std::collections::HashSet;
use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};

use super::manager::{
    VaultProfileManagerState, VaultProfilePhase, VaultProfileUiCommand,
};
use crate::config::vault_profiles::VaultProfileId;

impl VaultProfileManagerState {
    pub(crate) fn handle_key(
        &mut self,
        key: KeyEvent,
        bound: &HashSet<VaultProfileId>,
    ) -> VaultProfileUiCommand {
        if key.kind != KeyEventKind::Press {
            return VaultProfileUiCommand::None;
        }
        match self.phase {
            VaultProfilePhase::Profiles => self.handle_profiles_key(key.code),
            VaultProfilePhase::LauncherChoice => self.handle_launcher_key(key.code),
            VaultProfilePhase::Actions => self.handle_actions_key(key.code, bound),
            VaultProfilePhase::Directory => self.handle_directory_key(key.code),
            VaultProfilePhase::Name => self.handle_name_key(key, bound),
            VaultProfilePhase::Rename => self.handle_rename_key(key, bound),
            VaultProfilePhase::ChangeFolder => self.handle_repoint_key(key.code, bound),
        }
    }

    fn handle_profiles_key(&mut self, code: KeyCode) -> VaultProfileUiCommand {
        match code {
            KeyCode::Esc => VaultProfileUiCommand::Dismiss,
            KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                VaultProfileUiCommand::None
            }
            KeyCode::Down => {
                self.selected = self
                    .selected
                    .saturating_add(1)
                    .min(self.catalog.profiles.len().saturating_sub(1));
                VaultProfileUiCommand::None
            }
            KeyCode::Char('a') | KeyCode::Char('A') => {
                let start =
                    default_picker_start(self.catalog.get_default().map(|p| p.root.as_path()));
                if let Err(error) = self.begin_add(&start) {
                    return self.fail(error);
                }
                VaultProfileUiCommand::None
            }
            KeyCode::Enter if self.selected_profile_id().is_some() => {
                self.phase = VaultProfilePhase::Actions;
                self.action_selected = 0;
                VaultProfileUiCommand::None
            }
            KeyCode::Enter
            | KeyCode::Backspace
            | KeyCode::Left
            | KeyCode::Right
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::Tab
            | KeyCode::BackTab
            | KeyCode::Delete
            | KeyCode::Insert
            | KeyCode::F(_)
            | KeyCode::Char(_)
            | KeyCode::Null
            | KeyCode::CapsLock
            | KeyCode::ScrollLock
            | KeyCode::NumLock
            | KeyCode::PrintScreen
            | KeyCode::Pause
            | KeyCode::Menu
            | KeyCode::KeypadBegin
            | KeyCode::Media(_)
            | KeyCode::Modifier(_) => VaultProfileUiCommand::None,
        }
    }

    fn handle_launcher_key(&mut self, code: KeyCode) -> VaultProfileUiCommand {
        match code {
            KeyCode::Enter => {
                let (Some(launcher_id), Some(profile_id)) =
                    (self.launcher_id(), self.selected_profile_id().cloned())
                else {
                    return VaultProfileUiCommand::None;
                };
                VaultProfileUiCommand::Activate {
                    launcher_id,
                    profile_id,
                }
            }
            KeyCode::Esc => VaultProfileUiCommand::Dismiss,
            KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                VaultProfileUiCommand::None
            }
            KeyCode::Down => {
                self.selected = self
                    .selected
                    .saturating_add(1)
                    .min(self.catalog.profiles.len().saturating_sub(1));
                VaultProfileUiCommand::None
            }
            _ => VaultProfileUiCommand::None,
        }
    }

    fn handle_actions_key(
        &mut self,
        code: KeyCode,
        bound: &HashSet<VaultProfileId>,
    ) -> VaultProfileUiCommand {
        match code {
            KeyCode::Esc => {
                self.phase = VaultProfilePhase::Profiles;
                VaultProfileUiCommand::None
            }
            KeyCode::Up => {
                self.action_selected = self.action_selected.saturating_sub(1);
                VaultProfileUiCommand::None
            }
            KeyCode::Down => {
                self.action_selected = self.action_selected.saturating_add(1).min(3);
                VaultProfileUiCommand::None
            }
            KeyCode::Enter => self.select_action(bound),
            _ => VaultProfileUiCommand::None,
        }
    }

    fn select_action(&mut self, bound: &HashSet<VaultProfileId>) -> VaultProfileUiCommand {
        let Some(id) = self.selected_profile_id().cloned() else {
            return VaultProfileUiCommand::None;
        };
        match self.action_selected {
            0 => {
                let Some(profile) = self.catalog.get(&id) else {
                    return VaultProfileUiCommand::None;
                };
                self.input.clone_from(&profile.display_name);
                self.pending_id = Some(id);
                self.phase = VaultProfilePhase::Rename;
                VaultProfileUiCommand::None
            }
            1 => {
                let Some(profile) = self.catalog.get(&id) else {
                    return VaultProfileUiCommand::None;
                };
                let start = default_picker_start(profile.root.parent());
                match super::directory_picker::VaultDirectoryPickerState::new(&start) {
                    Ok(picker) => {
                        self.picker = Some(picker);
                        self.pending_id = Some(id);
                        self.phase = VaultProfilePhase::ChangeFolder;
                        VaultProfileUiCommand::None
                    }
                    Err(error) => self.fail(error),
                }
            }
            2 => self.set_default(&id),
            3 => self.remove(&id, bound),
            _ => VaultProfileUiCommand::None,
        }
    }
}

fn default_picker_start(profile_path: Option<&Path>) -> std::path::PathBuf {
    profile_path
        .map(Path::to_path_buf)
        .or_else(dirs::home_dir)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| Path::new(".").to_path_buf())
}
