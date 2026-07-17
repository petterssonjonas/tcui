use std::collections::HashSet;
use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::config::vault_profiles::VaultProfileId;

use super::directory_picker::{DirectoryPickerAction, VaultDirectoryPickerState};
use super::manager::{
    VaultProfileManagerState, VaultProfilePhase, VaultProfileUiCommand, VaultProfileUiError,
};

impl VaultProfileManagerState {
    pub(super) fn handle_directory_key(&mut self, code: KeyCode) -> VaultProfileUiCommand {
        match code {
            KeyCode::Esc => {
                if self.catalog.profiles.is_empty() && self.launcher_id().is_some() {
                    return VaultProfileUiCommand::Dismiss;
                }
                self.phase = VaultProfilePhase::Profiles;
                VaultProfileUiCommand::None
            }
            _ => match self.forward_picker(code) {
                Ok(DirectoryPickerAction::None) => VaultProfileUiCommand::None,
                Ok(DirectoryPickerAction::Confirm) => self.confirm_directory_selection(),
                Err(error) => self.fail(error),
            },
        }
    }

    pub(super) fn handle_name_key(
        &mut self,
        key: KeyEvent,
        bound: &HashSet<VaultProfileId>,
    ) -> VaultProfileUiCommand {
        match key.code {
            KeyCode::Esc => {
                self.phase = VaultProfilePhase::Directory;
                self.error = None;
                VaultProfileUiCommand::None
            }
            KeyCode::Enter => self.submit_name(bound),
            KeyCode::Backspace => {
                self.input.pop();
                VaultProfileUiCommand::None
            }
            KeyCode::Char(character)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                self.input.push(character);
                VaultProfileUiCommand::None
            }
            _ => VaultProfileUiCommand::None,
        }
    }

    pub(super) fn handle_rename_key(
        &mut self,
        key: KeyEvent,
        bound: &HashSet<VaultProfileId>,
    ) -> VaultProfileUiCommand {
        match key.code {
            KeyCode::Esc => {
                self.phase = VaultProfilePhase::Actions;
                self.error = None;
                VaultProfileUiCommand::None
            }
            KeyCode::Enter => {
                let Some(id) = self.pending_id.clone() else {
                    return VaultProfileUiCommand::None;
                };
                let input = self.input.clone();
                self.rename(&id, &input, bound)
            }
            _ => self.handle_name_key(key, bound),
        }
    }

    pub(super) fn handle_repoint_key(
        &mut self,
        code: KeyCode,
        bound: &HashSet<VaultProfileId>,
    ) -> VaultProfileUiCommand {
        match code {
            KeyCode::Esc => {
                self.phase = VaultProfilePhase::Actions;
                self.error = None;
                VaultProfileUiCommand::None
            }
            _ => match self.forward_picker(code) {
                Ok(DirectoryPickerAction::None) => VaultProfileUiCommand::None,
                Ok(DirectoryPickerAction::Confirm) => self.confirm_repoint_selection(bound),
                Err(error) => self.fail(error),
            },
        }
    }

    fn forward_picker(
        &mut self,
        code: KeyCode,
    ) -> Result<DirectoryPickerAction, VaultProfileUiError> {
        let action = self
            .picker
            .as_mut()
            .ok_or_else(|| VaultProfileUiError::DirectoryUnavailable {
                path: Path::new(".").to_path_buf(),
                message: "picker is unavailable".to_owned(),
            })?
            .handle_key(code)?;
        self.error = None;
        Ok(action)
    }

    pub(super) fn confirm_directory_selection(&mut self) -> VaultProfileUiCommand {
        let selected = self
            .picker
            .as_ref()
            .map(VaultDirectoryPickerState::selected_directory);
        let path = match selected {
            Some(Ok(path)) => path,
            Some(Err(error)) => return self.fail(error),
            None => {
                return self.fail(VaultProfileUiError::DirectoryUnavailable {
                    path: Path::new(".").to_path_buf(),
                    message: "picker is unavailable".to_owned(),
                });
            }
        };
        match self.choose_directory(&path) {
            Ok(()) => VaultProfileUiCommand::None,
            Err(error) => self.fail(error),
        }
    }

    pub(super) fn confirm_repoint_selection(
        &mut self,
        bound: &HashSet<VaultProfileId>,
    ) -> VaultProfileUiCommand {
        let selected = self
            .picker
            .as_ref()
            .map(VaultDirectoryPickerState::selected_directory);
        let Some(id) = self.pending_id.clone() else {
            return VaultProfileUiCommand::None;
        };
        let path = match selected {
            Some(Ok(path)) => path,
            Some(Err(error)) => return self.fail(error),
            None => return VaultProfileUiCommand::None,
        };
        self.repoint(&id, &path, bound)
    }
}
