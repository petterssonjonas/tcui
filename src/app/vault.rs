use crossterm::event::KeyEvent;
use ratatui::layout::Position;

use super::{Action, TuiApp};
use crate::ui::app_tabs::{AppTabId, AppView};
use crate::ui::modals::terminal_editor::CloseRequest;
use crate::ui::vault_workspace::VaultInputAction;

#[derive(Debug, Clone, Copy)]
pub(crate) enum PendingVaultAction {
    Activate { launcher_id: AppTabId },
    CloseTab { tab_id: AppTabId },
}

impl TuiApp {
    pub(crate) fn open_vault_launcher(&mut self) {
        let readiness =
            crate::protection::require(crate::protection::Domain::Vault).and_then(|()| {
                self.chat_files
                    .require_native_adoption()
                    .map_err(Into::into)
            });
        if let Err(error) = readiness {
            self.show_resource_error(crate::protection::Domain::Vault, error);
            return;
        }
        if self.ui.app_tabs.active_view() == AppView::Launcher {
            self.request_vault_action(PendingVaultAction::Activate {
                launcher_id: self.ui.app_tabs.active_id(),
            });
        }
    }

    pub(crate) fn request_app_tab_close(&mut self, tab_id: AppTabId) {
        if let Some(AppView::Terminal { session_id }) =
            self.ui.app_tabs.tab(tab_id).map(|tab| tab.view())
        {
            self.request_terminal_close(*session_id);
            return;
        }
        self.request_vault_action(PendingVaultAction::CloseTab { tab_id });
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
            }
        } else if !editor.close_is_pending() {
            self.ui.pending_vault_action = None;
        }
    }
    pub(crate) fn handle_pending_vault_action_key(&mut self, key: KeyEvent) {
        if let Some(PendingVaultAction::CloseTab { tab_id }) = self.ui.pending_vault_action {
            if let Some(editor) = self
                .ui
                .vault_workspaces
                .get_mut(tab_id)
                .and_then(|state| state.editor.as_mut())
            {
                match key.code {
                    crossterm::event::KeyCode::Char('y' | 'Y')
                    | crossterm::event::KeyCode::Enter => editor.confirm_close(),
                    crossterm::event::KeyCode::Esc | crossterm::event::KeyCode::Char('n' | 'N') => {
                        editor.cancel_close()
                    }
                    _ => {}
                }
                if editor.is_done() {
                    if let Some(action) = self.ui.pending_vault_action.take() {
                        self.apply_vault_action(action);
                    }
                } else if !editor.close_is_pending() {
                    self.ui.pending_vault_action = None;
                }
                return;
            }
        }
        if let Some(editor) = self.ui.editor_popup.as_mut() {
            editor.handle_key(key);
        }
        self.resolve_pending_vault_action_after_editor_key();
    }

    fn request_vault_action(&mut self, action: PendingVaultAction) {
        if let Some(editor) = self.ui.editor_popup.as_mut() {
            if matches!(editor.request_close(), CloseRequest::ConfirmationRequired) {
                self.ui.pending_vault_action = Some(action);
                return;
            }
        }
        if let PendingVaultAction::CloseTab { tab_id } = action {
            if let Some(editor) = self
                .ui
                .vault_workspaces
                .get_mut(tab_id)
                .and_then(|state| state.editor.as_mut())
            {
                if matches!(editor.request_close(), CloseRequest::ConfirmationRequired) {
                    self.ui.pending_vault_action = Some(action);
                    return;
                }
            }
        }
        self.apply_vault_action(action);
    }

    fn apply_vault_action(&mut self, action: PendingVaultAction) {
        match action {
            PendingVaultAction::Activate { launcher_id } => {
                if let Err(error) = self
                    .ui
                    .vault_workspaces
                    .activate(&mut self.ui.app_tabs, launcher_id)
                {
                    self.ui.show_toast(error.to_string());
                }
            }
            PendingVaultAction::CloseTab { tab_id } => {
                if self.ui.vault_workspaces.get(tab_id).is_some() {
                    let _ = self
                        .ui
                        .vault_workspaces
                        .close(&mut self.ui.app_tabs, tab_id);
                } else {
                    let _ = self.ui.app_tabs.close(tab_id);
                }
            }
        }
        self.ui.focus = crate::tui::focus::Focus::Chat;
    }

    pub(crate) fn handle_vault_key(&mut self, key: KeyEvent) -> Option<Action> {
        let tab_id = self.ui.app_tabs.active_id();
        let action = self
            .ui
            .vault_workspaces
            .get_mut(tab_id)
            .map_or(VaultInputAction::None, |state| state.handle_key(key));
        self.apply_vault_input(action)
    }

    pub(crate) fn handle_vault_mouse_click(&mut self, position: Position) -> Option<Action> {
        let tab_id = self.ui.app_tabs.active_id();
        let action = self
            .ui
            .vault_workspaces
            .get_mut(tab_id)
            .and_then(|state| {
                state
                    .hit_areas
                    .action_at(position)
                    .map(|action| state.handle_mouse(action))
            })
            .unwrap_or(VaultInputAction::None);
        self.apply_vault_input(action)
    }

    pub(crate) fn handle_vault_mouse_scroll(&mut self, position: Position, down: bool) -> bool {
        self.ui
            .vault_workspaces
            .get_mut(self.ui.app_tabs.active_id())
            .is_some_and(|state| state.scroll_at(position, down))
    }

    pub(crate) fn vault_is_active(&self) -> bool {
        matches!(self.ui.app_tabs.active_view(), AppView::Vault)
    }

    fn apply_vault_input(&mut self, action: VaultInputAction) -> Option<Action> {
        match action {
            VaultInputAction::None => {}
            VaultInputAction::Notice(message) => self.ui.show_toast(message),
            VaultInputAction::Move {
                source,
                destination,
            } => self.prepare_vault_workspace_move(source, destination),
            VaultInputAction::Transfer => {
                return Some(Action::OpenNativeTransfer(crate::protection::Domain::Vault));
            }
            action @ (VaultInputAction::Edit(_)
            | VaultInputAction::Copy(_)
            | VaultInputAction::Trash(_)
            | VaultInputAction::Context(_)) => {
                let path = match &action {
                    VaultInputAction::Edit(path)
                    | VaultInputAction::Copy(path)
                    | VaultInputAction::Trash(path)
                    | VaultInputAction::Context(path) => path,
                    _ => unreachable!("native item action"),
                };
                match self.chat_files.list_items() {
                    Ok(records) => {
                        let id = records
                            .iter()
                            .find(|record| record.relative_path == *path)
                            .map(|record| record.id);
                        self.ui.vault_artifacts = records
                            .into_iter()
                            .map(crate::ui::artifact_sidebar::ArtifactEntry::vault_file)
                            .collect();
                        self.ui
                            .vault_artifacts
                            .sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
                        let Some(id) = id else {
                            self.ui.show_toast(
                                "The selected Vault item is no longer available.".into(),
                            );
                            return None;
                        };
                        let handle = crate::ui::artifact_sidebar::ArtifactHandle::Vault(id);
                        match action {
                            VaultInputAction::Edit(_) => self.edit_artifact(handle),
                            VaultInputAction::Copy(_) => self.copy_artifact(handle),
                            VaultInputAction::Trash(_) => self.ui.delete_confirm = Some(handle),
                            VaultInputAction::Context(_) => self.insert_artifact_to_chat(handle),
                            _ => unreachable!("native item action"),
                        }
                    }
                    Err(error) => {
                        self.show_resource_error(crate::protection::Domain::Vault, error.into())
                    }
                }
            }
        }
        None
    }
}
