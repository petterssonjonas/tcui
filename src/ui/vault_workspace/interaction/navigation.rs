use crossterm::event::{KeyCode, KeyEvent};

use crate::ui::vault::hit_areas::VaultMouseAction;

use crate::ui::vault_workspace::state::{VaultContentPane, VaultFocus, VaultWorkspaceState};

impl VaultWorkspaceState {
    pub(super) fn cycle_focus(&mut self, reverse: bool) {
        let editor_visible =
            self.editor.is_some() && self.hit_areas.area(VaultMouseAction::FocusEditor).is_some();
        self.focus = match (self.focus, reverse, editor_visible) {
            (VaultFocus::Tree, false, _) => VaultFocus::Viewer,
            (VaultFocus::Viewer, false, true) => VaultFocus::Editor,
            (VaultFocus::Viewer | VaultFocus::Editor, false, _) => VaultFocus::Tree,
            (VaultFocus::Tree, true, true) => VaultFocus::Editor,
            (VaultFocus::Tree, true, false) => VaultFocus::Viewer,
            (VaultFocus::Editor, true, _) => VaultFocus::Viewer,
            (VaultFocus::Viewer, true, _) => VaultFocus::Tree,
        };
        match self.focus {
            VaultFocus::Viewer => self.content_pane = VaultContentPane::Viewer,
            VaultFocus::Editor => self.content_pane = VaultContentPane::Editor,
            VaultFocus::Tree => {}
        }
    }

    pub(super) fn scroll_focused(&mut self, key: KeyEvent) {
        let page = self
            .hit_areas
            .area(match self.focus {
                VaultFocus::Tree => VaultMouseAction::FocusTree,
                VaultFocus::Viewer => VaultMouseAction::FocusViewer,
                VaultFocus::Editor => VaultMouseAction::FocusEditor,
            })
            .map_or(8, |area| usize::from(area.height).max(1));
        let delta = match key.code {
            KeyCode::Up => -1,
            KeyCode::Down => 1,
            KeyCode::PageUp => -isize::try_from(page).unwrap_or(isize::MAX),
            KeyCode::PageDown => isize::try_from(page).unwrap_or(isize::MAX),
            _ => return,
        };
        match self.focus {
            VaultFocus::Tree => self.tree.move_selection(delta, page),
            VaultFocus::Viewer => {
                self.viewer.scroll = self.viewer.scroll.saturating_add_signed(delta);
            }
            VaultFocus::Editor => {
                if let Some(editor) = self.editor.as_mut() {
                    editor.handle_key(key);
                }
            }
        }
    }
}
