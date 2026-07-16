use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::ui::markdown_vault::hit_areas::MarkdownVaultMouseAction;

use super::super::MarkdownVaultInputAction;
use crate::ui::markdown_vault_workspace::state::{
    MarkdownVaultContentPane, MarkdownVaultFocus, MarkdownVaultWorkspaceState,
};

impl MarkdownVaultWorkspaceState {
    pub(super) fn handle_profile_key(&mut self, key: KeyEvent) -> MarkdownVaultInputAction {
        match key.code {
            KeyCode::Esc => {
                self.profile_controls.expanded = false;
                MarkdownVaultInputAction::None
            }
            KeyCode::Up => {
                self.profile_controls.selected = self.profile_controls.selected.saturating_sub(1);
                MarkdownVaultInputAction::None
            }
            KeyCode::Down => {
                self.profile_controls.selected = self
                    .profile_controls
                    .selected
                    .saturating_add(1)
                    .min(self.profile_controls.profiles.len().saturating_sub(1));
                MarkdownVaultInputAction::None
            }
            KeyCode::Enter => self.select_profile(self.profile_controls.selected),
            KeyCode::Tab | KeyCode::BackTab => {
                self.profile_controls.expanded = false;
                self.cycle_focus(
                    key.code == KeyCode::BackTab || key.modifiers.contains(KeyModifiers::SHIFT),
                );
                MarkdownVaultInputAction::None
            }
            _ => MarkdownVaultInputAction::None,
        }
    }

    pub(super) fn select_profile(&mut self, index: usize) -> MarkdownVaultInputAction {
        self.profile_controls.expanded = false;
        let Some(profile) = self.profile_controls.profiles.get(index) else {
            return MarkdownVaultInputAction::None;
        };
        self.profile_controls.selected = index;
        if profile.id == self.profile_id {
            MarkdownVaultInputAction::None
        } else {
            MarkdownVaultInputAction::Rebind(profile.id.clone())
        }
    }

    pub(super) fn cycle_focus(&mut self, reverse: bool) {
        let editor_visible = self.editor.is_some()
            && self
                .hit_areas
                .area(MarkdownVaultMouseAction::FocusEditor)
                .is_some();
        self.focus = match (self.focus, reverse, editor_visible) {
            (MarkdownVaultFocus::Tree, false, _) => MarkdownVaultFocus::Viewer,
            (MarkdownVaultFocus::Viewer, false, true) => MarkdownVaultFocus::Editor,
            (MarkdownVaultFocus::Viewer | MarkdownVaultFocus::Editor, false, _) => {
                MarkdownVaultFocus::ProfileControls
            }
            (MarkdownVaultFocus::ProfileControls, false, _) => MarkdownVaultFocus::Tree,
            (MarkdownVaultFocus::Tree, true, _) => MarkdownVaultFocus::ProfileControls,
            (MarkdownVaultFocus::ProfileControls, true, true) => MarkdownVaultFocus::Editor,
            (MarkdownVaultFocus::ProfileControls, true, false) => MarkdownVaultFocus::Viewer,
            (MarkdownVaultFocus::Editor, true, _) => MarkdownVaultFocus::Viewer,
            (MarkdownVaultFocus::Viewer, true, _) => MarkdownVaultFocus::Tree,
        };
        match self.focus {
            MarkdownVaultFocus::Viewer => self.content_pane = MarkdownVaultContentPane::Viewer,
            MarkdownVaultFocus::Editor => self.content_pane = MarkdownVaultContentPane::Editor,
            MarkdownVaultFocus::Tree | MarkdownVaultFocus::ProfileControls => {}
        }
    }

    pub(super) fn scroll_focused(&mut self, key: KeyEvent) {
        let page = self
            .hit_areas
            .area(match self.focus {
                MarkdownVaultFocus::Tree => MarkdownVaultMouseAction::FocusTree,
                MarkdownVaultFocus::Viewer => MarkdownVaultMouseAction::FocusViewer,
                MarkdownVaultFocus::Editor => MarkdownVaultMouseAction::FocusEditor,
                MarkdownVaultFocus::ProfileControls => return,
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
            MarkdownVaultFocus::Tree => self.tree.move_selection(delta, page),
            MarkdownVaultFocus::Viewer => {
                self.viewer.scroll = self.viewer.scroll.saturating_add_signed(delta);
            }
            MarkdownVaultFocus::Editor => {
                if let Some(editor) = self.editor.as_mut() {
                    editor.handle_key(key);
                }
            }
            MarkdownVaultFocus::ProfileControls => {}
        }
    }
}
