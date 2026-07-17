use std::collections::HashSet;

use ratatui::layout::{Position, Rect};

use super::directory_picker::DirectoryPickerAction;
use super::manager::{VaultProfileManagerState, VaultProfilePhase, VaultProfileUiCommand};
use crate::config::vault_profiles::VaultProfileId;

impl VaultProfileManagerState {
    pub(crate) fn handle_mouse_click(
        &mut self,
        position: Position,
        area: Rect,
        bound: &HashSet<VaultProfileId>,
    ) -> VaultProfileUiCommand {
        if !matches!(
            self.phase,
            VaultProfilePhase::Directory | VaultProfilePhase::ChangeFolder
        ) {
            return VaultProfileUiCommand::None;
        }
        let picker_area = super::render::popup_area(area).inner(ratatui::layout::Margin::new(1, 1));
        let action = match self
            .picker
            .as_mut()
            .map(|picker| picker.handle_mouse_click(picker_area, position))
        {
            Some(Ok(action)) => action,
            Some(Err(error)) => return self.fail(error),
            None => return VaultProfileUiCommand::None,
        };
        self.error = None;
        match (self.phase, action) {
            (_, DirectoryPickerAction::None) => VaultProfileUiCommand::None,
            (VaultProfilePhase::Directory, DirectoryPickerAction::Confirm) => {
                self.confirm_directory_selection()
            }
            (VaultProfilePhase::ChangeFolder, DirectoryPickerAction::Confirm) => {
                self.confirm_repoint_selection(bound)
            }
            (
                VaultProfilePhase::Profiles
                | VaultProfilePhase::Actions
                | VaultProfilePhase::Name
                | VaultProfilePhase::Rename
                | VaultProfilePhase::LauncherChoice,
                DirectoryPickerAction::Confirm,
            ) => VaultProfileUiCommand::None,
        }
    }

    pub(crate) fn handle_mouse_scroll(&mut self, position: Position, area: Rect, down: bool) {
        if !matches!(
            self.phase,
            VaultProfilePhase::Directory | VaultProfilePhase::ChangeFolder
        ) {
            return;
        }
        let picker_area = super::render::popup_area(area).inner(ratatui::layout::Margin::new(1, 1));
        if let Some(picker) = self.picker.as_mut() {
            picker.handle_mouse_scroll(picker_area, position, down);
        }
    }
}
