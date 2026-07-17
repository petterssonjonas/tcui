use std::path::Path;

use crossterm::event::KeyCode;
use ratatui::layout::{Position, Rect};

use super::{DirectoryPickerAction, PickerFocus, VaultDirectoryPickerState};
use crate::tui::vault_profile_ui::VaultProfileUiError;

impl VaultDirectoryPickerState {
    pub(in crate::tui::vault_profile_ui) fn handle_key(
        &mut self,
        code: KeyCode,
    ) -> Result<DirectoryPickerAction, VaultProfileUiError> {
        match code {
            KeyCode::Enter => self.activate_focus(),
            KeyCode::Tab => {
                self.focus = match self.focus {
                    PickerFocus::Path => PickerFocus::List,
                    PickerFocus::List => PickerFocus::Add,
                    PickerFocus::Add => PickerFocus::Path,
                };
                Ok(DirectoryPickerAction::None)
            }
            KeyCode::BackTab => {
                self.focus = match self.focus {
                    PickerFocus::Path => PickerFocus::Add,
                    PickerFocus::List => PickerFocus::Path,
                    PickerFocus::Add => PickerFocus::List,
                };
                Ok(DirectoryPickerAction::None)
            }
            KeyCode::Up => {
                self.select_previous();
                Ok(DirectoryPickerAction::None)
            }
            KeyCode::Down => {
                self.select_next();
                Ok(DirectoryPickerAction::None)
            }
            KeyCode::PageUp => {
                self.select_by(-8);
                Ok(DirectoryPickerAction::None)
            }
            KeyCode::PageDown => {
                self.select_by(8);
                Ok(DirectoryPickerAction::None)
            }
            KeyCode::Home if self.focus == PickerFocus::Path => {
                self.path_input.home();
                Ok(DirectoryPickerAction::None)
            }
            KeyCode::End if self.focus == PickerFocus::Path => {
                self.path_input.end();
                Ok(DirectoryPickerAction::None)
            }
            KeyCode::Home => {
                self.focus = PickerFocus::List;
                self.selected = Some(0);
                Ok(DirectoryPickerAction::None)
            }
            KeyCode::End => {
                self.focus = PickerFocus::List;
                self.selected = self.entries.len().checked_sub(1);
                Ok(DirectoryPickerAction::None)
            }
            KeyCode::Left if self.focus == PickerFocus::Path => {
                self.path_input.left();
                Ok(DirectoryPickerAction::None)
            }
            KeyCode::Right if self.focus == PickerFocus::Path => {
                self.path_input.right();
                Ok(DirectoryPickerAction::None)
            }
            KeyCode::Backspace => {
                self.focus = PickerFocus::Path;
                self.path_pristine = false;
                self.path_input.backspace();
                Ok(DirectoryPickerAction::None)
            }
            KeyCode::Delete => {
                self.focus = PickerFocus::Path;
                self.path_pristine = false;
                self.path_input.delete();
                Ok(DirectoryPickerAction::None)
            }
            KeyCode::Char(character) => {
                self.focus = PickerFocus::Path;
                if self.path_pristine {
                    self.path_input.clear();
                    self.path_pristine = false;
                }
                self.path_input.insert(character.encode_utf8(&mut [0; 4]));
                Ok(DirectoryPickerAction::None)
            }
            _ => Ok(DirectoryPickerAction::None),
        }
    }

    pub(in crate::tui::vault_profile_ui) fn handle_mouse_click(
        &mut self,
        area: Rect,
        position: Position,
    ) -> Result<DirectoryPickerAction, VaultProfileUiError> {
        let Some(areas) = super::view::picker_areas(area) else {
            return Ok(DirectoryPickerAction::None);
        };
        if areas.path.contains(position) {
            self.focus = PickerFocus::Path;
            return Ok(DirectoryPickerAction::None);
        }
        if areas.add.contains(position) {
            self.focus = PickerFocus::Add;
            return Ok(DirectoryPickerAction::Confirm);
        }
        if areas.list.contains(position) {
            let row = usize::from(position.y.saturating_sub(areas.list.y));
            let index = self.visible_start(usize::from(areas.list.height)) + row;
            if index < self.entries.len() {
                self.selected = Some(index);
                self.focus = PickerFocus::List;
            }
        }
        Ok(DirectoryPickerAction::None)
    }

    pub(in crate::tui::vault_profile_ui) fn handle_mouse_scroll(
        &mut self,
        area: Rect,
        position: Position,
        down: bool,
    ) {
        let Some(areas) = super::view::picker_areas(area) else {
            return;
        };
        if !areas.list.contains(position) {
            return;
        }
        if down {
            self.select_next();
        } else {
            self.select_previous();
        }
    }

    fn activate_focus(&mut self) -> Result<DirectoryPickerAction, VaultProfileUiError> {
        match self.focus {
            PickerFocus::Path => {
                let path = super::expand_tilde(
                    Path::new(self.path_input.query()),
                    dirs::home_dir().as_deref(),
                );
                self.set_directory(&path)?;
                Ok(DirectoryPickerAction::None)
            }
            PickerFocus::List => {
                let path = self.selected_directory()?;
                self.set_directory(&path)?;
                Ok(DirectoryPickerAction::None)
            }
            PickerFocus::Add => Ok(DirectoryPickerAction::Confirm),
        }
    }

    fn select_previous(&mut self) {
        self.focus = PickerFocus::List;
        self.selected = Some(match self.selected {
            Some(index) => index.saturating_sub(1),
            None => self.entries.len().saturating_sub(1),
        });
    }

    fn select_next(&mut self) {
        self.focus = PickerFocus::List;
        self.selected = Some(match self.selected {
            Some(index) => index
                .saturating_add(1)
                .min(self.entries.len().saturating_sub(1)),
            None => 0,
        });
    }

    fn select_by(&mut self, delta: isize) {
        self.focus = PickerFocus::List;
        let current = self.selected.unwrap_or(0);
        self.selected = Some(if delta.is_negative() {
            current.saturating_sub(delta.unsigned_abs())
        } else {
            current
                .saturating_add(delta.unsigned_abs())
                .min(self.entries.len().saturating_sub(1))
        });
    }
}
