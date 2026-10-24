mod document;
mod navigation;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Position;

use crate::ui::modals::terminal_editor::TerminalEditorState;
use crate::ui::vault::hit_areas::VaultMouseAction;

use super::move_input::VaultMoveMode;
use super::state::{VaultContentPane, VaultFocus, VaultWorkspaceState};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum VaultInputAction {
    None,
    Move {
        source: std::path::PathBuf,
        destination: std::path::PathBuf,
    },
    Notice(String),
    Edit(std::path::PathBuf),
    Copy(std::path::PathBuf),
    Trash(std::path::PathBuf),
    Context(std::path::PathBuf),
    Transfer,
}

impl VaultWorkspaceState {
    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> VaultInputAction {
        if self.move_input.is_some() {
            return self.handle_move_input_key(key);
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
            return VaultInputAction::None;
        }
        match key.code {
            KeyCode::Tab | KeyCode::BackTab => {
                let reverse =
                    key.code == KeyCode::BackTab || key.modifiers.contains(KeyModifiers::SHIFT);
                self.cycle_focus(reverse);
                VaultInputAction::None
            }
            KeyCode::Esc if self.focus == VaultFocus::Editor => {
                self.request_editor_close();
                VaultInputAction::None
            }
            KeyCode::Char('e') if self.focus != VaultFocus::Editor => self.open_editor(),
            KeyCode::Char('i') if self.focus != VaultFocus::Editor => VaultInputAction::Transfer,
            KeyCode::Char('c' | 'd' | '+') | KeyCode::Delete
                if self.focus != VaultFocus::Editor =>
            {
                let Some(node) = self.tree.selected_node() else {
                    return VaultInputAction::Notice("Select a file first".into());
                };
                if node.entry_kind != crate::vault::VaultEntryKind::File {
                    return VaultInputAction::Notice("Select a regular file, not a folder".into());
                }
                match key.code {
                    KeyCode::Char('c') => VaultInputAction::Copy(node.path.clone()),
                    KeyCode::Char('d') | KeyCode::Delete => {
                        VaultInputAction::Trash(node.path.clone())
                    }
                    _ => VaultInputAction::Context(node.path.clone()),
                }
            }
            KeyCode::Char('m') if self.focus == VaultFocus::Tree => {
                self.open_move_input(VaultMoveMode::Move)
            }
            KeyCode::Char('r') if self.focus == VaultFocus::Tree => {
                self.open_move_input(VaultMoveMode::Rename)
            }
            KeyCode::Enter if self.focus == VaultFocus::Tree => self.activate_selected(),
            KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown => {
                self.scroll_focused(key);
                VaultInputAction::None
            }
            _ if self.focus == VaultFocus::Editor => {
                if let Some(editor) = self.editor.as_mut() {
                    editor.handle_key(key);
                }
                VaultInputAction::None
            }
            _ => VaultInputAction::None,
        }
    }

    pub(crate) fn handle_mouse(&mut self, action: VaultMouseAction) -> VaultInputAction {
        if self.move_input.is_some() {
            return VaultInputAction::None;
        }
        match action {
            VaultMouseAction::TreeRow(index) => {
                self.tree.select(index);
                self.focus = VaultFocus::Tree;
                self.activate_selected()
            }
            VaultMouseAction::FocusTree => {
                self.focus = VaultFocus::Tree;
                VaultInputAction::None
            }
            VaultMouseAction::FocusViewer => {
                self.focus = VaultFocus::Viewer;
                self.content_pane = VaultContentPane::Viewer;
                VaultInputAction::None
            }
            VaultMouseAction::FocusEditor => {
                if self.editor.is_some() {
                    self.focus = VaultFocus::Editor;
                    self.content_pane = VaultContentPane::Editor;
                }
                VaultInputAction::None
            }
            VaultMouseAction::EditSelected => self.open_editor(),
            VaultMouseAction::Confirm => {
                self.confirm_editor_close();
                VaultInputAction::None
            }
            VaultMouseAction::Cancel => {
                if let Some(editor) = self.editor.as_mut() {
                    editor.cancel_close();
                }
                VaultInputAction::None
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
            VaultMouseAction::TreeRow(_) | VaultMouseAction::FocusTree => {
                self.focus = VaultFocus::Tree;
            }
            VaultMouseAction::FocusViewer | VaultMouseAction::EditSelected => {
                self.focus = VaultFocus::Viewer;
                self.content_pane = VaultContentPane::Viewer;
            }
            VaultMouseAction::FocusEditor => {
                self.focus = VaultFocus::Editor;
                self.content_pane = VaultContentPane::Editor;
            }
            VaultMouseAction::Confirm | VaultMouseAction::Cancel => return false,
        }
        self.scroll_focused(KeyEvent::new(
            if down { KeyCode::Down } else { KeyCode::Up },
            KeyModifiers::NONE,
        ));
        true
    }
}
