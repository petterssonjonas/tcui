mod document;
mod navigation;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Position;

use crate::config::vault_profiles::VaultProfileId;
use crate::ui::markdown_vault::hit_areas::MarkdownVaultMouseAction;
use crate::ui::modals::terminal_editor::TerminalEditorState;

use super::move_input::MarkdownVaultMoveMode;
use super::state::{
    MarkdownVaultContentPane, MarkdownVaultFocus, MarkdownVaultWorkspaceState,
    MarkdownVaultWorkspaceStatus,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MarkdownVaultInputAction {
    None,
    AddVault,
    Rebind(VaultProfileId),
    ConfirmRebind,
    CancelRebind,
    Move {
        source: std::path::PathBuf,
        destination: std::path::PathBuf,
    },
    Notice(String),
}

impl MarkdownVaultWorkspaceState {
    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> MarkdownVaultInputAction {
        if self.move_input.is_some() {
            return self.handle_move_input_key(key);
        }
        if matches!(
            self.status,
            MarkdownVaultWorkspaceStatus::RebindConfirmationRequired { .. }
        ) {
            return match key.code {
                KeyCode::Enter | KeyCode::Char('y' | 'Y') => {
                    MarkdownVaultInputAction::ConfirmRebind
                }
                KeyCode::Esc | KeyCode::Char('n' | 'N') => MarkdownVaultInputAction::CancelRebind,
                _ => MarkdownVaultInputAction::None,
            };
        }
        if self
            .editor
            .as_ref()
            .is_some_and(TerminalEditorState::close_is_pending)
        {
            match key.code {
                KeyCode::Enter | KeyCode::Char('y' | 'Y') => self.confirm_editor_close(),
                KeyCode::Esc | KeyCode::Char('n' | 'N') => {
                    if let Some(editor) = self.editor.as_mut() {
                        editor.cancel_close();
                    }
                }
                _ => {}
            }
            return MarkdownVaultInputAction::None;
        }
        if self.profile_controls.expanded {
            return self.handle_profile_key(key);
        }
        match key.code {
            KeyCode::Tab | KeyCode::BackTab => {
                let reverse =
                    key.code == KeyCode::BackTab || key.modifiers.contains(KeyModifiers::SHIFT);
                self.cycle_focus(reverse);
                MarkdownVaultInputAction::None
            }
            KeyCode::Esc if self.focus == MarkdownVaultFocus::Editor => {
                self.request_editor_close();
                MarkdownVaultInputAction::None
            }
            KeyCode::Char('e') if self.focus != MarkdownVaultFocus::Editor => self.open_editor(),
            KeyCode::Char('m') if self.focus == MarkdownVaultFocus::Tree => {
                self.open_move_input(MarkdownVaultMoveMode::Move)
            }
            KeyCode::Char('r') if self.focus == MarkdownVaultFocus::Tree => {
                self.open_move_input(MarkdownVaultMoveMode::Rename)
            }
            KeyCode::Enter if self.focus == MarkdownVaultFocus::Tree => self.activate_selected(),
            KeyCode::Enter if self.focus == MarkdownVaultFocus::ProfileControls => {
                if self.profile_controls.add_selected {
                    MarkdownVaultInputAction::AddVault
                } else {
                    self.profile_controls.expanded = true;
                    MarkdownVaultInputAction::None
                }
            }
            KeyCode::Left | KeyCode::Right if self.focus == MarkdownVaultFocus::ProfileControls => {
                self.profile_controls.add_selected = !self.profile_controls.add_selected;
                MarkdownVaultInputAction::None
            }
            KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown => {
                self.scroll_focused(key);
                MarkdownVaultInputAction::None
            }
            _ if self.focus == MarkdownVaultFocus::Editor => {
                if let Some(editor) = self.editor.as_mut() {
                    editor.handle_key(key);
                }
                MarkdownVaultInputAction::None
            }
            _ => MarkdownVaultInputAction::None,
        }
    }

    pub(crate) fn handle_mouse(
        &mut self,
        action: MarkdownVaultMouseAction,
    ) -> MarkdownVaultInputAction {
        if self.move_input.is_some() {
            return MarkdownVaultInputAction::None;
        }
        match action {
            MarkdownVaultMouseAction::TreeRow(index) => {
                self.tree.select(index);
                self.focus = MarkdownVaultFocus::Tree;
                self.activate_selected()
            }
            MarkdownVaultMouseAction::FocusTree => {
                self.focus = MarkdownVaultFocus::Tree;
                MarkdownVaultInputAction::None
            }
            MarkdownVaultMouseAction::FocusViewer => {
                self.focus = MarkdownVaultFocus::Viewer;
                self.content_pane = MarkdownVaultContentPane::Viewer;
                MarkdownVaultInputAction::None
            }
            MarkdownVaultMouseAction::FocusEditor => {
                if self.editor.is_some() {
                    self.focus = MarkdownVaultFocus::Editor;
                    self.content_pane = MarkdownVaultContentPane::Editor;
                }
                MarkdownVaultInputAction::None
            }
            MarkdownVaultMouseAction::EditSelected => self.open_editor(),
            MarkdownVaultMouseAction::ProfileDropdown => {
                self.focus = MarkdownVaultFocus::ProfileControls;
                self.profile_controls.add_selected = false;
                self.profile_controls.expanded = !self.profile_controls.expanded;
                MarkdownVaultInputAction::None
            }
            MarkdownVaultMouseAction::AddVault => MarkdownVaultInputAction::AddVault,
            MarkdownVaultMouseAction::ProfileOption(index) => self.select_profile(index),
            MarkdownVaultMouseAction::Confirm => {
                if matches!(
                    self.status,
                    MarkdownVaultWorkspaceStatus::RebindConfirmationRequired { .. }
                ) {
                    MarkdownVaultInputAction::ConfirmRebind
                } else {
                    self.confirm_editor_close();
                    MarkdownVaultInputAction::None
                }
            }
            MarkdownVaultMouseAction::Cancel => {
                if matches!(
                    self.status,
                    MarkdownVaultWorkspaceStatus::RebindConfirmationRequired { .. }
                ) {
                    MarkdownVaultInputAction::CancelRebind
                } else {
                    if let Some(editor) = self.editor.as_mut() {
                        editor.cancel_close();
                    }
                    MarkdownVaultInputAction::None
                }
            }
        }
    }

    pub(crate) fn scroll_at(&mut self, position: Position, down: bool) -> bool {
        if self.move_input.is_some() {
            return false;
        }
        let Some(action) = self.hit_areas.action_at(position) else {
            return false;
        };
        match action {
            MarkdownVaultMouseAction::TreeRow(_) | MarkdownVaultMouseAction::FocusTree => {
                self.focus = MarkdownVaultFocus::Tree;
            }
            MarkdownVaultMouseAction::FocusViewer | MarkdownVaultMouseAction::EditSelected => {
                self.focus = MarkdownVaultFocus::Viewer;
                self.content_pane = MarkdownVaultContentPane::Viewer;
            }
            MarkdownVaultMouseAction::FocusEditor => {
                self.focus = MarkdownVaultFocus::Editor;
                self.content_pane = MarkdownVaultContentPane::Editor;
            }
            MarkdownVaultMouseAction::ProfileDropdown
            | MarkdownVaultMouseAction::AddVault
            | MarkdownVaultMouseAction::ProfileOption(_)
            | MarkdownVaultMouseAction::Confirm
            | MarkdownVaultMouseAction::Cancel => return false,
        }
        self.scroll_focused(KeyEvent::new(
            if down { KeyCode::Down } else { KeyCode::Up },
            KeyModifiers::NONE,
        ));
        true
    }
}
