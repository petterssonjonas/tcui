use super::{Action, TuiApp};
use crate::app::action::MouseClickAction;

impl TuiApp {
    pub(crate) fn focus_chat_pane(&mut self, index: usize) -> bool {
        let changed = self.ui.active_tab != index;
        if !self.ui.focus_chat_pane(index) {
            return false;
        }
        self.ui.terminals.focused = false;
        if changed {
            self.cancel_escape = None;
            self.ui.list_popup = None;
            self.refresh_visible_selectors();
        }
        true
    }

    fn action_for_slash_command(&self, text: &str) -> Option<Action> {
        let trimmed = text.trim();
        let command = trimmed.split_whitespace().next()?;
        match command {
            "/fork" => return Some(Action::ForkConversation),
            "/temp" => {
                return match trimmed[command.len()..].trim() {
                    "" => Some(Action::OpenEphemeral),
                    "close" => Some(Action::CloseEphemeral),
                    _ => None,
                };
            }
            "/cancel" => return Some(Action::CancelRequest),
            "/steer" => {
                return Some(Action::SteerRequest(
                    trimmed[command.len()..].trim().to_string(),
                ))
            }
            _ => {}
        }
        match text.trim() {
            "/quit" | "/exit" | "/q" => Some(self.quit_action()),
            "/skills" => Some(Action::ShowSkillsPopup),
            "/mcp" => Some(Action::ShowMcpPopup),
            "/settings" => Some(Action::OpenSettingsPanel),
            "/terminal" => Some(Action::OpenTerminal),
            "/providers" | "/connect" => Some(Action::OpenProviders),
            "/vault" => Some(Action::OpenCredentialVault),
            "/vault-search" => Some(Action::ShowLocalSearch(String::new())),
            "/model" => Some(Action::SetManualModel(String::new())),
            "/help" => Some(Action::ShowHelp),
            "/keybinds" => Some(Action::ShowKeybinds),
            "/web" => Some(Action::ToggleWebSearch),
            _ => None,
        }
    }

    fn submit_composer_input(&mut self) -> Option<Action> {
        let tab = self.ui.tabs.get(self.ui.active_tab)?;
        let trimmed = tab.input_content.trim();
        if let Some(action) = self.action_for_slash_command(trimmed) {
            if !matches!(action, Action::SteerRequest(_)) {
                self.take_input_submission();
            }
            return Some(action);
        }
        let local_action = match trimmed.split_whitespace().next() {
            Some("/vault") => Some(Action::OpenCredentialVault),
            Some("/connect" | "/providers") => Some(Action::OpenProviders),
            _ => trimmed
                .strip_prefix("/vault-search ")
                .map(|query| Action::ShowLocalSearch(query.trim().to_string()))
                .or_else(|| {
                    trimmed
                        .strip_prefix("/model ")
                        .map(|model| Action::SetManualModel(model.trim().to_string()))
                }),
        };
        if let Some(action) = local_action {
            self.take_input_submission();
            return Some(action);
        }
        if trimmed == "/theme" {
            self.take_input_submission();
            self.show_theme_popup("");
            return None;
        }
        if let Some(theme_name) = trimmed.strip_prefix("/theme ") {
            let theme_name = theme_name.trim().to_string();
            self.take_input_submission();
            let _ = self.apply_theme_selection(&theme_name);
            return None;
        }
        if let Some(value) = trimmed.strip_prefix("/web ") {
            let value = value.trim();
            if value.eq_ignore_ascii_case("on") || value.eq_ignore_ascii_case("off") {
                let enabled = value.eq_ignore_ascii_case("on");
                let current = self
                    .config
                    .try_read()
                    .map(|config| config.web_search.enabled)
                    .unwrap_or(false);
                self.take_input_submission();
                return (enabled != current).then_some(Action::ToggleWebSearch);
            }
        }
        if self.active_request_id().is_some() {
            self.ui.show_toast(
                "This conversation already has an active request. Use /cancel or wait; your draft is preserved."
                    .to_string(),
            );
            return None;
        }
        self.take_input_submission().map(Action::SendMessage)
    }

    pub(crate) fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> Option<Action> {
        if key.kind != crossterm::event::KeyEventKind::Press {
            return None;
        }
        let previous_escape = self.cancel_escape.take();
        self.handle_key_inner(key, previous_escape)
    }

    fn handle_key_inner(
        &mut self,
        key: crossterm::event::KeyEvent,
        previous_escape: super::request_control::CancelEscape,
    ) -> Option<Action> {
        use crossterm::event::{KeyCode, KeyModifiers};

        self.ui.text_selection.cancel();
        if self.ui.command_review.is_some() || self.ui.terminals.close_pending.is_some() {
            return self.terminal_key(key).flatten();
        }

        if self.ui.credential_manager.is_some() {
            self.handle_credential_manager_key(key);
            return None;
        }

        if self.ui.show_keybinds {
            return match key.code {
                KeyCode::Esc | KeyCode::Enter => Some(Action::DismissKeybinds),
                _ => None,
            };
        }

        if let Some(pending) = self.ui.pending_vault_mutation.as_mut() {
            return match key.code {
                KeyCode::Esc | KeyCode::Char('n' | 'N') if pending.can_cancel() => {
                    Some(Action::CancelVaultMutation)
                }
                KeyCode::Enter | KeyCode::Char('y' | 'Y') if pending.can_confirm() => {
                    Some(Action::ConfirmVaultMutation)
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    pending.scroll_up(1);
                    None
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    pending.scroll_down(1);
                    None
                }
                KeyCode::PageUp => {
                    pending.scroll_up(8);
                    None
                }
                KeyCode::PageDown => {
                    pending.scroll_down(8);
                    None
                }
                KeyCode::Home => {
                    pending.scroll_up(usize::MAX);
                    None
                }
                KeyCode::End => {
                    pending.scroll_down(usize::MAX);
                    None
                }
                _ => None,
            };
        }

        if self.ui.pending_vault_action.is_some() {
            if let Some(editor) = self.ui.editor_popup.as_mut() {
                editor.handle_key(key);
            }
            self.resolve_pending_vault_action_after_editor_key();
            return None;
        }

        if self.ui.vault_profile_manager.is_some() {
            self.handle_vault_profile_key(key);
            return None;
        }

        if let Some(palette) = self.ui.palette.as_mut() {
            return match key.code {
                KeyCode::Esc => {
                    self.ui.palette = None;
                    None
                }
                KeyCode::Enter => {
                    let action = palette.selected_action();
                    self.ui.palette = None;
                    action
                }
                KeyCode::Up => {
                    palette.move_up();
                    None
                }
                KeyCode::Down => {
                    palette.move_down();
                    None
                }
                KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if palette.toggle_pin() {
                        let pinned = palette.pinned().to_vec();
                        if let Ok(mut config) = self.config.try_write() {
                            config.tui.pinned_commands = pinned;
                            let _ = config.save();
                        }
                    }
                    None
                }
                KeyCode::Char(c)
                    if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
                {
                    palette.insert_char(c);
                    None
                }
                KeyCode::Backspace => {
                    palette.backspace();
                    None
                }
                _ => None,
            };
        }

        if let Some(settings) = self.ui.settings_v2.as_mut() {
            let catalog = crate::tui::settings_panel::all_settings();
            return match key.code {
                KeyCode::Esc => {
                    if settings.esc() {
                        self.ui.settings_v2 = None;
                    }
                    None
                }
                KeyCode::Enter => {
                    match settings.enter(&catalog) {
                        crate::tui::settings_panel::EnterResult::RunCommand(id) => {
                            self.ui.settings_v2 = None;
                            return crate::tui::settings_panel::command_action(id);
                        }
                        crate::tui::settings_panel::EnterResult::NavigateSetting(id) => {
                            self.ui.settings_v2 = None;
                            return Some(Action::OpenSettingsPage(id.to_string()));
                        }
                        crate::tui::settings_panel::EnterResult::Nothing => {}
                    }
                    None
                }
                KeyCode::Up => {
                    settings.move_up();
                    None
                }
                KeyCode::Down => {
                    settings.move_down(&catalog);
                    None
                }
                KeyCode::Tab => {
                    settings.toggle_descriptions();
                    None
                }
                KeyCode::Char(c)
                    if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
                {
                    settings.insert_char(c);
                    None
                }
                KeyCode::Backspace => {
                    settings.backspace();
                    None
                }
                _ => None,
            };
        }

        if let Some(dialog) = &mut self.ui.export_dialog {
            return match key.code {
                KeyCode::Esc => {
                    self.ui.export_dialog = None;
                    None
                }
                KeyCode::Tab => {
                    dialog.cycle_focus(!key.modifiers.contains(KeyModifiers::SHIFT));
                    None
                }
                KeyCode::Left | KeyCode::Up => {
                    dialog.cycle_focus(false);
                    None
                }
                KeyCode::Right | KeyCode::Down => {
                    dialog.cycle_focus(true);
                    None
                }
                KeyCode::Enter => match dialog.focus {
                    crate::ui::modals::export_dialog::ExportDialogFocus::Markdown => {
                        dialog.format = crate::export::OutputFormat::Markdown;
                        None
                    }
                    crate::ui::modals::export_dialog::ExportDialogFocus::Json => {
                        dialog.format = crate::export::OutputFormat::Json;
                        None
                    }
                    crate::ui::modals::export_dialog::ExportDialogFocus::Cancel => {
                        self.ui.export_dialog = None;
                        None
                    }
                    _ => Some(Action::SaveExportDialog),
                },
                KeyCode::Char(c)
                    if dialog.focus
                        == crate::ui::modals::export_dialog::ExportDialogFocus::Path
                        && (key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT) =>
                {
                    dialog.directory_input.push(c);
                    None
                }
                KeyCode::Backspace
                    if dialog.focus
                        == crate::ui::modals::export_dialog::ExportDialogFocus::Path =>
                {
                    dialog.directory_input.pop();
                    None
                }
                _ => None,
            };
        }

        if self.ui.active_modal.is_none()
            && self.ui.delete_confirm.is_none()
            && self.ui.save_file_dialog.is_none()
            && self.ui.artifact_viewer.is_none()
            && self.ui.editor_popup.is_none()
            && self.ui.list_popup.is_none()
            && !self.ui.show_session_list
        {
            if let Some(action) = self.terminal_key(key) {
                return action;
            }
            if key.code == KeyCode::Char('p') && key.modifiers.contains(KeyModifiers::CONTROL) {
                return Some(Action::OpenCommandPalette);
            }
            if let Some(action) =
                crate::ui::settings_workspace::keybinding_action(key, &self.ui.keybinding_overrides)
            {
                return Some(action);
            }
            if self.ui.app_tabs.active_view() == crate::ui::app_tabs::AppView::Settings {
                return match key.code {
                    KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        self.request_app_tab_close(self.ui.app_tabs.active_id());
                        None
                    }
                    KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        Some(self.quit_action())
                    }
                    _ => self.settings_workspace_key(key),
                };
            }
            if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('e') {
                self.open_chat_draft_editor();
                return None;
            }
        }

        if let Some(dialog) = &mut self.ui.save_file_dialog {
            return match key.code {
                KeyCode::Esc => Some(Action::CancelSaveDialog),
                KeyCode::Enter => Some(Action::SaveGeneratedFile),
                KeyCode::Char(c)
                    if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
                {
                    dialog.path_input.push(c);
                    None
                }
                KeyCode::Backspace => {
                    dialog.path_input.pop();
                    None
                }
                _ => None,
            };
        }

        if self.ui.editor_popup.is_some() {
            if let Some(editor) = self.ui.editor_popup.as_mut() {
                editor.handle_key(key);
            }
            return None;
        }

        if let Some(viewer) = &mut self.ui.artifact_viewer {
            let viewer_handle = viewer.handle().clone();
            return match key.code {
                KeyCode::Esc => {
                    self.ui.artifact_viewer = None;
                    None
                }
                KeyCode::Up => {
                    viewer.scroll = viewer.scroll.saturating_sub(1);
                    None
                }
                KeyCode::Down => {
                    viewer.scroll = viewer.scroll.saturating_add(1);
                    None
                }
                KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.prepare_artifact_save(viewer_handle);
                    None
                }
                KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(Action::ExportConversation)
                }
                _ => None,
            };
        }

        if self.ui.active_modal.is_none()
            && self.ui.delete_confirm.is_none()
            && !self.ui.show_session_list
            && key.code == KeyCode::Enter
            && !key.modifiers.contains(KeyModifiers::SHIFT)
            && !self
                .ui
                .list_popup
                .as_ref()
                .is_some_and(|popup| popup.selected.is_some())
        {
            let skill_command =
                self.ui.tabs.get(self.ui.active_tab).is_some_and(|tab| {
                    tab.input_content.split_whitespace().next() == Some("/skill")
                });
            if skill_command {
                self.ui.list_popup = None;
                return self.submit_composer_input();
            }
            let local_action = self.ui.tabs.get(self.ui.active_tab).and_then(|tab| {
                match tab.input_content.split_whitespace().next() {
                    Some("/vault") => Some(Action::OpenCredentialVault),
                    Some("/connect" | "/providers") => Some(Action::OpenProviders),
                    Some("/fork" | "/temp" | "/cancel" | "/steer") => {
                        self.action_for_slash_command(&tab.input_content)
                    }
                    _ => None,
                }
            });
            if let Some(action) = local_action {
                if !matches!(action, Action::SteerRequest(_)) {
                    self.take_input_submission();
                }
                self.ui.list_popup = None;
                return Some(action);
            }
        }

        if self.ui.list_popup.is_some()
            && self.ui.active_modal.is_none()
            && self.ui.delete_confirm.is_none()
        {
            let live_input = self
                .ui
                .list_popup
                .as_ref()
                .is_some_and(|popup| popup.live_input);
            let visible_rows = self
                .ui
                .last_area
                .and_then(|area| {
                    self.ui
                        .list_popup
                        .as_ref()
                        .map(|popup| popup.popup_area_in(area))
                })
                .map(|area| area.height.saturating_sub(2) as usize)
                .unwrap_or(8);
            return match key.code {
                KeyCode::Esc => Some(Action::CloseListPopup),
                KeyCode::Enter => {
                    let action = self
                        .ui
                        .list_popup
                        .as_ref()
                        .and_then(|popup| popup.selected_action());
                    self.ui.list_popup = None;
                    match action {
                        Some(crate::ui::modals::list_popup::ListPopupAction::InsertText(text)) => {
                            if let Some(action) = self.action_for_slash_command(&text) {
                                if !matches!(action, Action::SteerRequest(_)) {
                                    self.replace_input_content(String::new());
                                }
                                return Some(action);
                            }
                            self.insert_input_text(&text);
                        }
                        Some(crate::ui::modals::list_popup::ListPopupAction::ReplaceInput(
                            text,
                        )) => {
                            if let Some(action) = self.action_for_slash_command(&text) {
                                if !matches!(action, Action::SteerRequest(_)) {
                                    self.replace_input_content(String::new());
                                }
                                return Some(action);
                            }
                            self.replace_input_content(text);
                        }
                        Some(crate::ui::modals::list_popup::ListPopupAction::SetTheme(theme)) => {
                            let _ = self.apply_theme_selection(&theme);
                        }
                        None => {}
                    }
                    None
                }
                KeyCode::Up => {
                    if live_input {
                        if key
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL)
                            && self.browse_input_history(false)
                        {
                            return None;
                        }
                        if let Some(popup) = &mut self.ui.list_popup {
                            popup.move_up();
                        }
                        return None;
                    }
                    if self.browse_input_history(false) {
                        return None;
                    }
                    if let Some(popup) = &mut self.ui.list_popup {
                        popup.move_up();
                    }
                    None
                }
                KeyCode::Down => {
                    if live_input {
                        if key
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL)
                            && self.browse_input_history(true)
                        {
                            return None;
                        }
                        if let Some(popup) = &mut self.ui.list_popup {
                            popup.move_down(visible_rows);
                        }
                        return None;
                    }
                    if self.browse_input_history(true) {
                        return None;
                    }
                    if let Some(popup) = &mut self.ui.list_popup {
                        popup.move_down(visible_rows);
                    }
                    None
                }
                KeyCode::Char(c)
                    if live_input
                        && (key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT) =>
                {
                    self.insert_input_char(c);
                    None
                }
                KeyCode::Backspace if live_input => {
                    self.backspace_input_char();
                    None
                }
                _ => None,
            };
        }

        // Modal input handling takes priority
        if self.ui.active_modal.is_some() {
            let modal = self.ui.active_modal;
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    self.ui.active_modal = None;
                    Some(match modal {
                        Some(crate::ui::Modal::QuitConfirm) => Action::ConfirmQuit,
                        Some(crate::ui::Modal::DiscardPendingDocuments) => {
                            Action::ConfirmPendingDocumentDiscard
                        }
                        None => Action::CancelModal,
                    })
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    self.ui.active_modal = None;
                    Some(Action::CancelModal)
                }
                _ => None,
            }
        } else if self.ui.delete_confirm.is_some() {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                    if let Some(handle) = self.ui.delete_confirm.take() {
                        self.delete_artifact(handle);
                    }
                    None
                }
                _ => {
                    self.ui.delete_confirm = None;
                    None
                }
            }
        } else {
            if self.markdown_vault_is_active() {
                return match key.code {
                    KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => {
                        Some(self.quit_action())
                    }
                    KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        Some(self.quit_action())
                    }
                    KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        Some(Action::OpenCommandPalette)
                    }
                    KeyCode::Char(',') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        Some(Action::ToggleSettings)
                    }
                    KeyCode::Char('t') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        self.ui.app_tabs.add_placeholder();
                        self.ui.focus = crate::tui::focus::Focus::Chat;
                        None
                    }
                    KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        let active = self.ui.app_tabs.active_index();
                        self.request_app_tab_close(active);
                        None
                    }
                    _ => self.handle_markdown_vault_key(key),
                };
            }
            if !self.ui.app_tabs.active_view().is_chat() {
                return match key.code {
                    KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => {
                        Some(self.quit_action())
                    }
                    KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        Some(self.quit_action())
                    }
                    KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        Some(Action::OpenCommandPalette)
                    }
                    KeyCode::Char(',') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        Some(Action::ToggleSettings)
                    }
                    KeyCode::Char('t') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        self.ui.app_tabs.add_placeholder();
                        self.ui.focus = crate::tui::focus::Focus::Chat;
                        None
                    }
                    KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        let active = self.ui.app_tabs.active_index();
                        self.request_app_tab_close(active);
                        None
                    }
                    _ => None,
                };
            }

            if self.ui.ephemeral.is_some()
                && key.modifiers == KeyModifiers::ALT
                && matches!(key.code, KeyCode::Left | KeyCode::Right)
            {
                let pane = self.ui.ephemeral.as_ref()?;
                let index = if key.code == KeyCode::Left {
                    pane.main_tab_index
                } else {
                    pane.tab_index
                };
                return Some(Action::FocusChatPane(index));
            }

            if self.ui.focus == crate::tui::focus::Focus::LeftSidebar {
                match key.code {
                    KeyCode::Esc | KeyCode::Tab => {
                        self.ui.focus = crate::tui::focus::Focus::Chat;
                        return None;
                    }
                    KeyCode::Up | KeyCode::Down => {
                        let index = self.ui.main_chat_index();
                        if let Some(tab) = self.ui.tabs.get_mut(index) {
                            tab.sidebar_state
                                .move_selection(&tab.conversations, key.code == KeyCode::Down);
                        }
                        return None;
                    }
                    KeyCode::Enter => {
                        let index = self.ui.main_chat_index();
                        let action = self
                            .ui
                            .tabs
                            .get(index)
                            .map(|tab| tab.sidebar_state.selected_action());
                        self.focus_chat_pane(index);
                        return action.map(|action| match action {
                            crate::ui::sidebar::SidebarAction::NewChat => {
                                self.ui.focus = crate::tui::focus::Focus::Chat;
                                Action::NewChat
                            }
                            crate::ui::sidebar::SidebarAction::LoadConversation(id) => {
                                Action::LoadConversation(id)
                            }
                            crate::ui::sidebar::SidebarAction::TogglePinned(id) => {
                                Action::ToggleConversationPinned(id)
                            }
                            crate::ui::sidebar::SidebarAction::ExportConversation(id) => {
                                Action::ExportConversationId(id)
                            }
                            crate::ui::sidebar::SidebarAction::DeleteConversation(id) => {
                                Action::DeleteConversation(id)
                            }
                        });
                    }
                    _ if !key.modifiers.contains(KeyModifiers::CONTROL) => return None,
                    _ => {}
                }
            }

            match key.code {
                KeyCode::Esc if key.modifiers.is_empty() => {
                    if self.ui.show_session_list {
                        return Some(Action::ToggleSessionList);
                    }
                    let (armed, action) = super::request_control::cancel_escape_action(
                        previous_escape,
                        self.active_request_id(),
                        std::time::Instant::now(),
                    );
                    self.cancel_escape = armed;
                    action
                }
                KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => {
                    Some(self.quit_action())
                }
                KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(self.quit_action())
                }
                KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(Action::ToggleSidebar)
                }
                KeyCode::Char(']') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(Action::ToggleArtifactSidebar)
                }
                KeyCode::Left if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(Action::ToggleSidebar)
                }
                KeyCode::Right if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(Action::ToggleArtifactSidebar)
                }
                KeyCode::Char('t') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.ui.app_tabs.add_placeholder();
                    self.ui.focus = crate::tui::focus::Focus::Chat;
                    None
                }
                KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    let index = self.ui.main_chat_index();
                    self.focus_chat_pane(index);
                    Some(Action::NewChat)
                }
                KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if key.modifiers.contains(KeyModifiers::SHIFT) {
                        Some(Action::CloseChat)
                    } else {
                        None
                    }
                }
                KeyCode::Char(',') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(Action::ToggleSettings)
                }
                KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(Action::RefreshModels)
                }
                KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(Action::ToggleSettings)
                }
                KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(Action::OpenCommandPalette)
                }
                KeyCode::Tab if self.ui.sidebar_open => {
                    self.ui.focus = crate::tui::focus::Focus::LeftSidebar;
                    None
                }
                KeyCode::Up if key.modifiers == KeyModifiers::ALT => {
                    self.scroll_active_chat_lines(-3);
                    None
                }
                KeyCode::Down if key.modifiers == KeyModifiers::ALT => {
                    self.scroll_active_chat_lines(3);
                    None
                }
                KeyCode::PageUp if key.modifiers == KeyModifiers::ALT => {
                    self.scroll_active_chat_page(false);
                    None
                }
                KeyCode::PageDown if key.modifiers == KeyModifiers::ALT => {
                    self.scroll_active_chat_page(true);
                    None
                }
                KeyCode::Up if key.modifiers == KeyModifiers::SHIFT => {
                    self.jump_to_adjacent_answer(false);
                    None
                }
                KeyCode::Down if key.modifiers == KeyModifiers::SHIFT => {
                    self.jump_to_adjacent_answer(true);
                    None
                }
                KeyCode::Left => {
                    self.move_input_cursor_left();
                    None
                }
                KeyCode::Right => {
                    self.move_input_cursor_right();
                    None
                }
                KeyCode::Up => {
                    self.browse_input_history(false);
                    None
                }
                KeyCode::Down => {
                    self.browse_input_history(true);
                    None
                }
                KeyCode::Home => {
                    self.move_input_cursor_home();
                    None
                }
                KeyCode::End => {
                    self.move_input_cursor_end();
                    None
                }
                KeyCode::Delete => {
                    self.delete_input_char();
                    None
                }
                KeyCode::Enter => {
                    if key.modifiers.contains(KeyModifiers::SHIFT) {
                        self.insert_input_newline();
                        None
                    } else {
                        self.submit_composer_input()
                    }
                }
                KeyCode::Char(c) => {
                    if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT {
                        self.insert_input_char(c);
                    }
                    None
                }
                KeyCode::Backspace => {
                    let last_chip = self
                        .ui
                        .tabs
                        .get(self.ui.active_tab)
                        .filter(|tab| tab.input_content.is_empty())
                        .and_then(|tab| tab.pending_document_chips.last())
                        .map(crate::ui::pending_documents::PendingDocumentChip::id);
                    match last_chip {
                        Some(id) => Some(Action::RemovePendingDocument(id)),
                        None => {
                            self.backspace_input_char();
                            None
                        }
                    }
                }
                _ => None,
            }
        }
    }

    pub(crate) fn handle_mouse(&mut self, mouse: crossterm::event::MouseEvent) -> Option<Action> {
        use crossterm::event::{MouseButton, MouseEventKind};

        self.cancel_escape = None;
        if self.ui.command_review.is_some() || self.ui.terminals.close_pending.is_some() {
            return self.terminal_mouse(mouse).flatten();
        }
        if self.ui.active_modal.is_none()
            && self.ui.palette.is_none()
            && self.ui.settings_v2.is_none()
            && self.ui.credential_manager.is_none()
            && self.ui.vault_profile_manager.is_none()
            && self.ui.pending_vault_mutation.is_none()
            && self.ui.editor_popup.is_none()
            && self.ui.artifact_viewer.is_none()
            && self.ui.save_file_dialog.is_none()
            && self.ui.export_dialog.is_none()
            && self.ui.list_popup.is_none()
            && self.ui.delete_confirm.is_none()
            && !self.ui.show_keybinds
            && !self.ui.show_session_list
        {
            if let Some(action) = self.terminal_mouse(mouse) {
                return action;
            }
            if self.ui.app_tabs.active_view() == crate::ui::app_tabs::AppView::Settings
                && mouse.row > self.ui.last_area.map_or(0, |area| area.y)
            {
                return self.settings_workspace_mouse(mouse);
            }
        }
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let pos = ratatui::layout::Position::new(mouse.column, mouse.row);
                let bounds = if self.ui.credential_manager.is_some() {
                    self.ui.last_area.map(|area| {
                        crate::tui::components::centered_rect(86, 82, area)
                            .inner(ratatui::layout::Margin::new(1, 1))
                    })
                } else {
                    self.ui
                        .ephemeral
                        .as_ref()
                        .and_then(|pane| pane.area)
                        .filter(|area| area.contains(pos))
                        .or_else(|| self.ui.chat_area.filter(|area| area.contains(pos)))
                        .or(self.ui.last_area)
                }
                .unwrap_or_default();
                self.ui.text_selection.begin(pos, bounds);
                None
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                self.ui
                    .text_selection
                    .drag(ratatui::layout::Position::new(mouse.column, mouse.row));
                None
            }
            MouseEventKind::Up(MouseButton::Left) => {
                let pos = ratatui::layout::Position::new(mouse.column, mouse.row);
                if !self.ui.text_selection.is_active() {
                    return None;
                }
                if let Some(text) = self.ui.text_selection.finish(pos) {
                    if !text.is_empty() {
                        let result = crate::clipboard::copy(&text);
                        self.clipboard_notice(result);
                    }
                    None
                } else if self.ui.credential_manager.is_none() {
                    self.handle_mouse_click(mouse.column, mouse.row)
                } else {
                    None
                }
            }
            MouseEventKind::Down(MouseButton::Right) => {
                self.handle_key(crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Esc,
                    crossterm::event::KeyModifiers::NONE,
                ))
            }
            MouseEventKind::ScrollUp => {
                self.ui.text_selection.cancel();
                if self.ui.credential_manager.is_some() {
                    return None;
                }
                if self.ui.editor_popup.is_some() {
                    if let Some(editor) = self.ui.editor_popup.as_mut() {
                        editor.send_scroll(false);
                    }
                    return None;
                }
                self.handle_mouse_scroll(mouse.column, mouse.row, mouse.modifiers, false);
                None
            }
            MouseEventKind::ScrollDown => {
                self.ui.text_selection.cancel();
                if self.ui.credential_manager.is_some() {
                    return None;
                }
                if self.ui.editor_popup.is_some() {
                    if let Some(editor) = self.ui.editor_popup.as_mut() {
                        editor.send_scroll(true);
                    }
                    return None;
                }
                self.handle_mouse_scroll(mouse.column, mouse.row, mouse.modifiers, true);
                None
            }
            _ => None,
        }
    }

    fn clipboard_notice(&mut self, result: std::io::Result<()>) {
        self.ui.show_toast(match result {
            Ok(()) => "Copy sent to clipboard".to_string(),
            Err(error) => format!("Copy failed: {error}"),
        });
    }

    pub(crate) fn handle_mouse_scroll(
        &mut self,
        col: u16,
        row: u16,
        modifiers: crossterm::event::KeyModifiers,
        down: bool,
    ) {
        let pos = ratatui::layout::Position::new(col, row);
        if let (Some(manager), Some(area)) =
            (self.ui.vault_profile_manager.as_mut(), self.ui.last_area)
        {
            manager.handle_mouse_scroll(pos, area, down);
            return;
        }
        if let Some(popup) = &mut self.ui.list_popup {
            let visible_rows = self
                .ui
                .last_area
                .map(|area| popup.popup_area_in(area))
                .map(|area| area.height.saturating_sub(2) as usize)
                .unwrap_or(1);
            if down {
                popup.move_down(visible_rows);
            } else {
                popup.move_up();
            }
            return;
        }
        if self.ui.active_modal.is_some()
            || self.ui.command_review.is_some()
            || self.ui.terminals.close_pending.is_some()
            || self.ui.delete_confirm.is_some()
            || self.ui.palette.is_some()
            || self.ui.settings_v2.is_some()
            || self.ui.credential_manager.is_some()
            || self.ui.pending_vault_mutation.is_some()
            || self.ui.pending_vault_action.is_some()
            || self.ui.save_file_dialog.is_some()
            || self.ui.export_dialog.is_some()
            || self.ui.editor_popup.is_some()
            || self.ui.show_keybinds
            || self.ui.show_session_list
        {
            return;
        }

        if self.ui.app_tabs.active_view().is_chat() && self.ui.sidebar_open {
            let index = self.ui.main_chat_index();
            if let Some(tab) = self.ui.tabs.get_mut(index) {
                if tab.sidebar_state.viewport_contains(pos) {
                    tab.sidebar_state.scroll(&tab.conversations, down);
                    self.ui.focus = crate::tui::focus::Focus::LeftSidebar;
                    return;
                }
            }
        }

        if self.markdown_vault_is_active() {
            let _ = self.handle_markdown_vault_mouse_scroll(pos, down);
            return;
        }

        if !self.ui.app_tabs.active_view().is_chat() {
            return;
        }

        if let Some(viewer) = &mut self.ui.artifact_viewer {
            if let Some(area) = self.ui.chat_area {
                if area.contains(pos) {
                    if down {
                        viewer.scroll = viewer.scroll.saturating_add(1);
                    } else {
                        viewer.scroll = viewer.scroll.saturating_sub(1);
                    }
                    return;
                }
            }
        }

        if let Some(section) = self.ui.artifact_sidebar_state.section_at(pos) {
            if let Some(tab) = self.ui.tabs.get(self.ui.active_tab) {
                match section {
                    crate::ui::artifact_sidebar::ArtifactSection::Temporary => {
                        let visible = self
                            .ui
                            .artifact_sidebar_state
                            .temp_body
                            .map(crate::ui::artifact_sidebar::ArtifactSidebar::visible_rows)
                            .unwrap_or(1);
                        self.ui.artifact_sidebar_state.scroll(
                            section,
                            down,
                            tab.temporary_artifacts.len(),
                            visible,
                        );
                    }
                    crate::ui::artifact_sidebar::ArtifactSection::Saved => {
                        let visible = self
                            .ui
                            .artifact_sidebar_state
                            .saved_body
                            .map(crate::ui::artifact_sidebar::ArtifactSidebar::visible_rows)
                            .unwrap_or(1);
                        self.ui.artifact_sidebar_state.scroll(
                            section,
                            down,
                            self.ui.saved_artifacts.len(),
                            visible,
                        );
                    }
                    crate::ui::artifact_sidebar::ArtifactSection::Memories => {
                        let visible = self
                            .ui
                            .artifact_sidebar_state
                            .memory_body
                            .map(crate::ui::artifact_sidebar::ArtifactSidebar::visible_rows)
                            .unwrap_or(1);
                        self.ui.artifact_sidebar_state.scroll(
                            section,
                            down,
                            self.ui.memory_artifacts.len(),
                            visible,
                        );
                    }
                    crate::ui::artifact_sidebar::ArtifactSection::Vault => {
                        let visible = self
                            .ui
                            .artifact_sidebar_state
                            .vault_body
                            .map(crate::ui::artifact_sidebar::ArtifactSidebar::visible_vault_rows)
                            .unwrap_or(1);
                        self.ui.artifact_sidebar_state.scroll(
                            section,
                            down,
                            self.ui.vault_artifacts.len(),
                            visible,
                        );
                    }
                }
            }
            return;
        }

        let dropdown_hit = self.ui.tabs.get(self.ui.active_tab).is_some_and(|tab| {
            tab.dropdown_item_areas
                .iter()
                .any(|area| area.contains(pos))
        });
        if !dropdown_hit {
            if let Some(index) = self.ui.chat_pane_at(pos) {
                self.focus_chat_pane(index);
            } else if self.ui.ephemeral.is_some() {
                return;
            }
        }

        if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
            if tab.provider_dropdown_open || tab.model_dropdown_open || tab.reasoning_dropdown_open
            {
                let total = if tab.provider_dropdown_open {
                    self.ui.visible_providers.len()
                } else if tab.model_dropdown_open {
                    self.ui.current_models.len()
                } else {
                    self.ui.current_reasoning_options.len()
                };
                if total == 0 {
                    return;
                }
                const VISIBLE_ITEMS: usize = 6;
                let max_offset = total.saturating_sub(VISIBLE_ITEMS.min(total));
                if down {
                    tab.dropdown_scroll_offset = (tab.dropdown_scroll_offset + 1).min(max_offset);
                } else {
                    tab.dropdown_scroll_offset = tab.dropdown_scroll_offset.saturating_sub(1);
                }
            } else {
                if modifiers.contains(crossterm::event::KeyModifiers::SHIFT) {
                    self.jump_to_adjacent_answer(down);
                    return;
                }
                const SCROLL_STEP: usize = 3;
                if down {
                    tab.scroll_offset = tab.scroll_offset.saturating_add(SCROLL_STEP);
                } else {
                    tab.scroll_offset = tab.scroll_offset.saturating_sub(SCROLL_STEP);
                }
                tab.scroll_to_message = None;
            }
        }
    }

    pub(crate) fn handle_mouse_click(&mut self, col: u16, row: u16) -> Option<Action> {
        let pos = ratatui::layout::Position::new(col, row);
        let area = self.ui.last_area?;

        if let Some(areas) = self.ui.pending_vault_mutation_areas {
            if areas.cancel.contains(pos)
                && self
                    .ui
                    .pending_vault_mutation
                    .as_ref()
                    .is_some_and(crate::app::submission::PendingVaultMutation::can_cancel)
            {
                return Some(Action::CancelVaultMutation);
            }
            if areas.confirm.is_some_and(|area| area.contains(pos)) {
                return Some(Action::ConfirmVaultMutation);
            }
            return None;
        }

        if self.ui.vault_profile_manager.is_some() {
            self.handle_vault_profile_mouse_click(pos, area);
            return None;
        }

        if self.ui.pending_vault_action.is_some() {
            return None;
        }

        if self.ui.delete_confirm.is_some() {
            if let Some(areas) = self.ui.delete_confirm_areas {
                if areas.yes.contains(pos) {
                    if let Some(handle) = self.ui.delete_confirm.take() {
                        self.delete_artifact(handle);
                    }
                    return None;
                }
                if areas.no.contains(pos) {
                    self.ui.delete_confirm = None;
                    return None;
                }
            }
            self.ui.delete_confirm = None;
            return None;
        }

        if let Some(dialog) = &self.ui.save_file_dialog {
            let popup_area = crate::ui::modals::save_file::SaveFileDialog::popup_area(area);
            if popup_area.contains(pos) {
                if dialog.hit_areas.save.is_some_and(|hit| hit.contains(pos)) {
                    return Some(Action::SaveGeneratedFile);
                }
                if dialog.hit_areas.cancel.is_some_and(|hit| hit.contains(pos)) {
                    return Some(Action::CancelSaveDialog);
                }
                return None;
            }
            return Some(Action::CancelSaveDialog);
        }

        if let Some(dialog) = &mut self.ui.export_dialog {
            let popup_area = crate::ui::modals::export_dialog::ExportDialog::popup_area(area);
            if popup_area.contains(pos) {
                if dialog
                    .hit_areas
                    .markdown
                    .is_some_and(|hit| hit.contains(pos))
                {
                    dialog.focus = crate::ui::modals::export_dialog::ExportDialogFocus::Markdown;
                    dialog.format = crate::export::OutputFormat::Markdown;
                    return None;
                }
                if dialog.hit_areas.json.is_some_and(|hit| hit.contains(pos)) {
                    dialog.focus = crate::ui::modals::export_dialog::ExportDialogFocus::Json;
                    dialog.format = crate::export::OutputFormat::Json;
                    return None;
                }
                if dialog.hit_areas.export.is_some_and(|hit| hit.contains(pos)) {
                    dialog.focus = crate::ui::modals::export_dialog::ExportDialogFocus::Export;
                    return Some(Action::SaveExportDialog);
                }
                if dialog.hit_areas.cancel.is_some_and(|hit| hit.contains(pos)) {
                    self.ui.export_dialog = None;
                    return None;
                }
                return None;
            }
            self.ui.export_dialog = None;
            return None;
        }

        if self.ui.editor_popup.is_some() {
            if let Some(chat_area) = self.ui.chat_area {
                let popup_area = crate::ui::modals::editor_popup::popup_area_pub(chat_area);
                if popup_area.contains(pos) {
                    if self
                        .ui
                        .editor_popup
                        .as_ref()
                        .is_some_and(|e| e.close_area().is_some_and(|area| area.contains(pos)))
                    {
                        if let Some(editor) = self.ui.editor_popup.as_mut() {
                            let _ = editor.request_close();
                        }
                    }
                    return None;
                }
            }
            return None;
        }

        if self.ui.artifact_viewer.is_some() {
            if let Some(chat_area) = self.ui.chat_area {
                let popup_area = crate::ui::modals::artifact_viewer::popup_area(chat_area);
                if popup_area.contains(pos) {
                    let (close, edit, delete, insert_to_chat, handle) =
                        self.ui.artifact_viewer.as_ref().map(|viewer| {
                            (
                                viewer.hit_areas.close,
                                viewer.hit_areas.edit,
                                viewer.hit_areas.delete,
                                viewer.hit_areas.insert_to_chat,
                                viewer.handle().clone(),
                            )
                        })?;
                    if close.is_some_and(|area| area.contains(pos)) {
                        self.ui.artifact_viewer = None;
                        return None;
                    }
                    if edit.is_some_and(|area| area.contains(pos)) {
                        self.ui.artifact_viewer = None;
                        self.edit_artifact(handle);
                        return None;
                    }
                    if delete.is_some_and(|area| area.contains(pos)) {
                        self.ui.delete_confirm = Some(handle);
                        return None;
                    }
                    if insert_to_chat.is_some_and(|area| area.contains(pos)) {
                        return Some(Action::InsertArtifactToChat(handle));
                    }
                    return None;
                }
            }
            self.ui.artifact_viewer = None;
            return None;
        }

        if self.ui.list_popup.is_some() {
            let popup_area = self
                .ui
                .list_popup
                .as_ref()
                .map(|popup| popup.popup_area_in(area))
                .unwrap_or_else(|| crate::ui::modals::list_popup::ListPopup::popup_area(area));
            if popup_area.contains(pos) {
                let action = self
                    .ui
                    .list_popup
                    .as_mut()
                    .and_then(|popup| popup.action_at(area, pos));
                self.ui.list_popup = None;
                match action {
                    Some(crate::ui::modals::list_popup::ListPopupAction::InsertText(text)) => {
                        if let Some(action) = self.action_for_slash_command(&text) {
                            if !matches!(action, Action::SteerRequest(_)) {
                                self.replace_input_content(String::new());
                            }
                            return Some(action);
                        }
                        self.insert_input_text(&text);
                    }
                    Some(crate::ui::modals::list_popup::ListPopupAction::ReplaceInput(text)) => {
                        if let Some(action) = self.action_for_slash_command(&text) {
                            if !matches!(action, Action::SteerRequest(_)) {
                                self.replace_input_content(String::new());
                            }
                            return Some(action);
                        }
                        self.replace_input_content(text);
                    }
                    Some(crate::ui::modals::list_popup::ListPopupAction::SetTheme(theme)) => {
                        let _ = self.apply_theme_selection(&theme);
                    }
                    None => {}
                }
                return None;
            }
            self.ui.list_popup = None;
            return None;
        }

        if self.ui.show_keybinds {
            return Some(Action::DismissKeybinds);
        }

        if self.ui.palette.is_some()
            && !crate::tui::components::centered_rect(70, 60, area).contains(pos)
        {
            self.ui.palette = None;
            return None;
        }

        if let Some(settings) = self.ui.settings_v2.as_ref() {
            if !settings.popup_area(area).contains(pos) {
                self.ui.settings_v2 = None;
                return None;
            }
        }

        if self.ui.active_modal.is_some() {
            let modal = self.ui.active_modal;
            if let Some(modal_areas) = self.ui.modal_areas {
                if modal_areas.yes.contains(pos) {
                    self.ui.active_modal = None;
                    return Some(match modal {
                        Some(crate::ui::Modal::QuitConfirm) => Action::ConfirmQuit,
                        Some(crate::ui::Modal::DiscardPendingDocuments) => {
                            Action::ConfirmPendingDocumentDiscard
                        }
                        None => Action::CancelModal,
                    });
                }
                if modal_areas.no.contains(pos) {
                    self.ui.active_modal = None;
                    return Some(Action::CancelModal);
                }
            }
            self.ui.active_modal = None;
            return Some(Action::CancelModal);
        }
        if self
            .ui
            .ephemeral
            .as_ref()
            .and_then(|pane| pane.close_area)
            .is_some_and(|area| area.contains(pos))
            && self.ui.palette.is_none()
            && self.ui.settings_v2.is_none()
            && self.ui.command_review.is_none()
            && self.ui.terminals.close_pending.is_none()
            && self.ui.credential_manager.is_none()
            && !self.ui.show_session_list
        {
            return Some(Action::CloseEphemeral);
        }

        if let Some((_, mouse_action)) = self
            .ui
            .mouse_hit_areas
            .iter()
            .rev()
            .find(|(hit_area, _)| hit_area.contains(pos))
            .copied()
        {
            if (self.ui.palette.is_some()
                && !matches!(mouse_action, crate::ui::MouseAction::PaletteItem(_)))
                || (self.ui.settings_v2.is_some()
                    && !matches!(mouse_action, crate::ui::MouseAction::SettingsItem(_)))
                || self.ui.show_session_list
                || self.ui.command_review.is_some()
                || self.ui.terminals.close_pending.is_some()
                || self.ui.credential_manager.is_some()
            {
                return None;
            }
            match mouse_action {
                crate::ui::MouseAction::AppTabSelect(index) => {
                    self.ui.app_tabs.select(index);
                    self.ui.terminals.focused = self.ui.app_tabs.tab(index).is_some_and(|tab| {
                        matches!(tab.view(), crate::ui::app_tabs::AppView::Terminal { .. })
                    });
                    self.ui.focus = crate::tui::focus::Focus::Chat;
                    return None;
                }
                crate::ui::MouseAction::AppTabAdd => {
                    self.ui.app_tabs.add_placeholder();
                    self.ui.focus = crate::tui::focus::Focus::Chat;
                    return None;
                }
                crate::ui::MouseAction::AppTabClose(index) => {
                    self.request_app_tab_close(index);
                    return None;
                }
                crate::ui::MouseAction::Sidebar(action) => {
                    let index = self.ui.main_chat_index();
                    self.focus_chat_pane(index);
                    if let Some(tab) = self.ui.tabs.get_mut(index) {
                        tab.sidebar_state.select_action(action);
                    }
                    self.ui.focus = crate::tui::focus::Focus::LeftSidebar;
                    return Some(match action {
                        crate::ui::sidebar::SidebarAction::NewChat => {
                            self.ui.focus = crate::tui::focus::Focus::Chat;
                            Action::NewChat
                        }
                        crate::ui::sidebar::SidebarAction::LoadConversation(id) => {
                            Action::LoadConversation(id)
                        }
                        crate::ui::sidebar::SidebarAction::TogglePinned(id) => {
                            Action::ToggleConversationPinned(id)
                        }
                        crate::ui::sidebar::SidebarAction::ExportConversation(id) => {
                            Action::ExportConversationId(id)
                        }
                        crate::ui::sidebar::SidebarAction::DeleteConversation(id) => {
                            Action::DeleteConversation(id)
                        }
                    });
                }
                crate::ui::MouseAction::PaletteItem(index) => {
                    if let Some(palette) = self.ui.palette.as_mut() {
                        palette.select(index);
                        let action = palette.selected_action();
                        self.ui.palette = None;
                        return action;
                    }
                }
                crate::ui::MouseAction::LeftHandle => {
                    return Some(Action::MouseClick(MouseClickAction::ToggleLeftHandle));
                }
                crate::ui::MouseAction::RightHandle => {
                    return Some(Action::MouseClick(MouseClickAction::ToggleRightHandle));
                }
                crate::ui::MouseAction::RightHandleClose => {
                    return Some(Action::MouseClick(MouseClickAction::CloseRightSidebar));
                }
                crate::ui::MouseAction::RightHandleThin => {
                    return Some(Action::MouseClick(MouseClickAction::SetRightSidebarThin));
                }
                crate::ui::MouseAction::ProviderDropdown => {
                    return Some(Action::MouseClick(MouseClickAction::OpenProviderDropdown));
                }
                crate::ui::MouseAction::ModelDropdown => {
                    return Some(Action::MouseClick(MouseClickAction::OpenModelDropdown));
                }
                crate::ui::MouseAction::SettingsItem(index) => {
                    return Some(Action::MouseClick(MouseClickAction::SelectSettingsItem(
                        index,
                    )));
                }
                crate::ui::MouseAction::Launcher(action) => {
                    return match action {
                        crate::ui::launcher::LauncherAction::MarkdownVault => {
                            Some(Action::OpenMarkdownVaultLauncher)
                        }
                        crate::ui::launcher::LauncherAction::Terminal => Some(Action::OpenTerminal),
                        crate::ui::launcher::LauncherAction::Settings => {
                            Some(Action::OpenSettingsPanel)
                        }
                        crate::ui::launcher::LauncherAction::MemoryManagement
                        | crate::ui::launcher::LauncherAction::System
                        | crate::ui::launcher::LauncherAction::Research
                        | crate::ui::launcher::LauncherAction::Todo
                        | crate::ui::launcher::LauncherAction::Kanban
                        | crate::ui::launcher::LauncherAction::News
                        | crate::ui::launcher::LauncherAction::Pomodoro => None,
                    };
                }
            }
        }
        if self.ui.palette.is_some()
            || self.ui.settings_v2.is_some()
            || self.ui.credential_manager.is_some()
            || self.ui.command_review.is_some()
            || self.ui.terminals.close_pending.is_some()
            || self.ui.show_session_list
        {
            return None;
        }

        if self.markdown_vault_is_active() {
            return self.handle_markdown_vault_mouse_click(pos);
        }

        if !self.ui.app_tabs.active_view().is_chat() {
            return None;
        }

        let dropdown_hit = self.ui.tabs.get(self.ui.active_tab).is_some_and(|tab| {
            tab.dropdown_item_areas
                .iter()
                .any(|area| area.contains(pos))
        });
        if !dropdown_hit {
            if let Some(index) = self.ui.chat_pane_at(pos) {
                self.focus_chat_pane(index);
                self.ui.focus = crate::tui::focus::Focus::Chat;
            }
        }

        let mut clicked_link = None;
        if let Some(tab) = self.ui.tabs.get(self.ui.active_tab) {
            for (message_idx, hit_area) in &tab.thinking_hit_areas {
                if hit_area.contains(pos) {
                    self.ui
                        .toggle_thinking_fold(self.ui.active_tab, *message_idx);
                    return None;
                }
            }
            for (hit_area, url) in &tab.link_hit_areas {
                if hit_area.contains(pos) {
                    clicked_link = Some(url.clone());
                    break;
                }
            }
        }
        if let Some(url) = clicked_link {
            self.open_external_target(&url);
            return None;
        }

        if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
            // When a dropdown is open, skip scrollbar/input checks so clicks on
            // overlapping dropdown items reach the dropdown handler below instead
            // of being swallowed by the input-area handler.
            let dropdown_open = tab.provider_dropdown_open
                || tab.model_dropdown_open
                || tab.reasoning_dropdown_open;
            if !dropdown_open {
                if let Some(scrollbar) = tab.chat_scrollbar_area {
                    if scrollbar.contains(pos) {
                        let max_scroll = tab
                            .total_rendered_lines
                            .saturating_sub(tab.message_viewport_height);
                        if max_scroll > 0 && scrollbar.height > 0 {
                            let relative = pos.y.saturating_sub(scrollbar.y) as usize;
                            tab.scroll_offset = ((relative * max_scroll)
                                / scrollbar.height as usize)
                                .min(max_scroll);
                            tab.scroll_to_message = None;
                        }
                        return None;
                    }
                }
                if tab.input_area.is_some_and(|area| area.contains(pos)) {
                    self.ui.focus = crate::tui::focus::Focus::Chat;
                    self.set_input_cursor_from_click(pos);
                    return None;
                }
            }
        }

        if let Some(id) = self.ui.tabs.get(self.ui.active_tab).and_then(|tab| {
            tab.pending_document_chip_hit_areas
                .iter()
                .find_map(|(id, area)| area.contains(pos).then_some(*id))
        }) {
            return Some(Action::RemovePendingDocument(id));
        }

        if self.ui.show_selector {
            let mut selected_provider = None;
            let mut selected_model = None;
            let mut handled_selector = false;
            let mut refresh_models_and_reasoning = false;

            if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
                if tab.provider_dropdown_open
                    || tab.model_dropdown_open
                    || tab.reasoning_dropdown_open
                {
                    for (i, item_area) in tab.dropdown_item_areas.iter().enumerate() {
                        if item_area.contains(pos) {
                            let real_idx = i + tab.dropdown_scroll_offset;
                            if tab.provider_dropdown_open {
                                if real_idx < self.ui.visible_providers.len() {
                                    let new_provider =
                                        self.ui.visible_providers[real_idx].0.clone();
                                    if tab.tab.provider != new_provider {
                                        tab.tab.provider = new_provider.clone();
                                        tab.tab.endpoint = None;
                                        tab.tab.model.clear();
                                        tab.manual_model_override = false;
                                        tab.tab.reasoning_effort = None;
                                        selected_provider = Some(new_provider);
                                    }
                                }
                                tab.provider_dropdown_open = false;
                            } else if tab.model_dropdown_open {
                                if real_idx < self.ui.current_models.len() {
                                    let model = self.ui.current_models[real_idx].id.clone();
                                    tab.tab.model = model.clone();
                                    tab.manual_model_override = false;
                                    tab.tab.reasoning_effort = None;
                                    selected_model = Some(model);
                                }
                                tab.model_dropdown_open = false;
                                refresh_models_and_reasoning = true;
                            } else if tab.reasoning_dropdown_open {
                                if real_idx < self.ui.current_reasoning_options.len() {
                                    tab.tab.reasoning_effort =
                                        Some(self.ui.current_reasoning_options[real_idx].clone());
                                }
                                tab.reasoning_dropdown_open = false;
                            }
                            tab.dropdown_scroll_offset = 0;
                            handled_selector = true;
                            break;
                        }
                    }
                    if !handled_selector {
                        tab.provider_dropdown_open = false;
                        tab.model_dropdown_open = false;
                        tab.reasoning_dropdown_open = false;
                        handled_selector = true;
                    }
                }
                if !handled_selector {
                    if let Some(area) = tab.provider_hit_area {
                        if area.contains(pos) {
                            tab.provider_dropdown_open = !tab.provider_dropdown_open;
                            tab.model_dropdown_open = false;
                            tab.reasoning_dropdown_open = false;
                            handled_selector = true;
                        }
                    }
                    if let Some(area) = tab.model_hit_area {
                        if area.contains(pos) {
                            tab.model_dropdown_open = !tab.model_dropdown_open;
                            tab.provider_dropdown_open = false;
                            tab.reasoning_dropdown_open = false;
                            handled_selector = true;
                        }
                    }
                    if let Some(area) = tab.reasoning_hit_area {
                        if area.contains(pos) {
                            tab.reasoning_dropdown_open = !tab.reasoning_dropdown_open;
                            tab.provider_dropdown_open = false;
                            tab.model_dropdown_open = false;
                            handled_selector = true;
                        }
                    }
                }
            }
            if refresh_models_and_reasoning {
                self.refresh_visible_selectors();
            }

            if let Some(provider) = selected_provider {
                let models = self.visible_models_for_provider(&provider);
                self.ui.current_models = models.clone();
                if models.is_empty() {
                    self.refresh_models_for_provider(provider.clone());
                } else if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
                    if tab.tab.model.is_empty() {
                        if let Some(first) = self.ui.current_models.first() {
                            tab.tab.model = first.id.clone();
                        }
                    }
                }
                self.refresh_visible_selectors();
                self.queue_connection_check_for_active_tab();
                self.ui.show_toast(format!("Switched to {provider}"));
            }

            if let Some(model) = selected_model {
                self.ui.show_toast(format!("Model: {model}"));
            }

            if handled_selector {
                return None;
            }
        }

        if self.ui.palette.is_none() && self.ui.settings_v2.is_none() {
            if let Some((message_index, block_index, action)) =
                self.ui.tabs.get(self.ui.active_tab).and_then(|tab| {
                    tab.command_hit_areas.iter().find_map(
                        |(message_index, block_index, action, area)| {
                            area.contains(pos)
                                .then_some((*message_index, *block_index, *action))
                        },
                    )
                })
            {
                match action {
                    crate::ui::chat_tab::CommandBlockAction::Run => {
                        self.open_command_review(message_index, block_index);
                    }
                    crate::ui::chat_tab::CommandBlockAction::Copy => {
                        if let Some(block) =
                            self.command_block_for_message(message_index, block_index)
                        {
                            let result = crate::clipboard::copy(&block.code);
                            self.clipboard_notice(result);
                        }
                    }
                }
                return None;
            }
            if let Some((tab, index)) = self.ui.tabs.get(self.ui.active_tab).and_then(|tab| {
                tab.copy_hit_areas
                    .iter()
                    .find_map(|(index, area)| area.contains(pos).then_some((tab, *index)))
            }) {
                if let Some(message) = tab
                    .messages
                    .get(index)
                    .filter(|message| message.role == "assistant")
                {
                    let result = crate::clipboard::copy(&message.content);
                    self.clipboard_notice(result);
                }
                return None;
            }
        }

        if let Some(action) = self.ui.artifact_sidebar_state.action_at(pos) {
            match action {
                crate::ui::artifact_sidebar::ArtifactSidebarAction::ToggleSection(section) => {
                    self.ui.artifact_sidebar_state.toggle(section);
                }
                crate::ui::artifact_sidebar::ArtifactSidebarAction::ToggleVaultDir(path) => {
                    self.ui.artifact_sidebar_state.toggle_vault_dir(&path);
                }
                crate::ui::artifact_sidebar::ArtifactSidebarAction::Open(handle) => {
                    self.open_artifact(handle);
                }
                crate::ui::artifact_sidebar::ArtifactSidebarAction::Edit(handle) => {
                    self.edit_artifact(handle);
                }
                crate::ui::artifact_sidebar::ArtifactSidebarAction::Save(handle) => {
                    self.prepare_artifact_save(handle);
                }
                crate::ui::artifact_sidebar::ArtifactSidebarAction::Delete(handle) => {
                    self.ui.delete_confirm = Some(handle);
                }
            }
            return None;
        }

        if let Some(areas) = self.ui.status_bar_areas {
            if areas.web_search.is_some_and(|area| area.contains(pos)) {
                return Some(Action::ToggleWebSearch);
            }
        }

        None
    }
}

#[cfg(test)]
#[path = "ephemeral_input_tests.rs"]
mod ephemeral_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::submission::{InputSubmission, PendingDocuments, PreparedUserTurn};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn register_request(app: &mut TuiApp) -> crate::app::submission::SubmissionId {
        let id = PreparedUserTurn::prepare(&InputSubmission::new(
            "request",
            PendingDocuments::default(),
        ))
        .expect("prepare request")
        .submission_id();
        let conversation = app.ui.tabs[0].active_conversation;
        app.register_request(
            crate::app::requests::RequestRoute {
                request_id: id,
                origin_conversation_id: conversation,
                destination_conversation_id: conversation,
            },
            0,
            app.ui.tabs[0].messages.clone(),
        )
        .expect("register request");
        id
    }

    fn arm_escape(app: &mut TuiApp) {
        let id = register_request(app);
        app.cancel_escape = Some((id, std::time::Instant::now()));
    }

    #[test]
    fn slash_controls_parse_tokens_and_never_become_model_messages() {
        crate::app::tests::with_test_app("request-slash-controls", |app| {
            for (input, command) in [
                (" /fork ", "fork"),
                ("/cancel", "cancel"),
                ("/steer", "steer"),
                ("/steer \t change direction\nnow ", "steer"),
            ] {
                app.replace_input_content(input.into());
                let action = app.handle_key(key(KeyCode::Enter));
                match (command, action) {
                    ("fork", Some(Action::ForkConversation))
                    | ("cancel", Some(Action::CancelRequest)) => {
                        assert!(app.ui.tabs[0].input_content.is_empty());
                    }
                    ("steer", Some(Action::SteerRequest(instruction))) => {
                        let expected = input.trim().strip_prefix("/steer").unwrap().trim();
                        assert_eq!(instruction, expected);
                        assert_eq!(app.ui.tabs[0].input_content, input);
                    }
                    (_, action) => panic!("Unexpected control action: {action:?}"),
                }
            }
            assert!(app.ui.tabs[0].messages.is_empty());
        });
    }

    #[test]
    fn slash_skill_submits_with_one_enter_despite_an_empty_completion_popup() {
        crate::app::tests::with_test_app("slash-skill-submit", |app| {
            let input = "/skill recall what is this machine for?";
            app.replace_input_content(input.to_owned());
            assert!(matches!(
                app.handle_key(key(KeyCode::Enter)),
                Some(Action::SendMessage(content)) if content == input
            ));
            assert!(app.ui.tabs[0].input_content.is_empty());
        });
    }

    #[test]
    fn slash_control_prefixes_do_not_match_other_commands() {
        crate::app::tests::with_test_app("request-slash-boundaries", |app| {
            for input in ["/forked", "/cancelled", "/steering", "hello /cancel"] {
                app.ui.tabs[0].input_content = input.into();
                assert!(matches!(
                    app.handle_key(key(KeyCode::Enter)),
                    Some(Action::SendMessage(content)) if content == input
                ));
            }
        });
    }

    #[test]
    fn non_escape_key_clears_cancel_chord() {
        crate::app::tests::with_test_app("request-chord-non-escape", |app| {
            arm_escape(app);
            app.handle_key(key(KeyCode::Char('x')));
            assert!(app.cancel_escape.is_none());
            assert_eq!(app.ui.tabs[0].input_content, "x");
        });
    }

    #[test]
    fn modal_escape_dismisses_without_arming_or_cancelling() {
        crate::app::tests::with_test_app("request-chord-modal", |app| {
            arm_escape(app);
            app.ui.active_modal = Some(crate::ui::Modal::QuitConfirm);
            assert!(matches!(
                app.handle_key(key(KeyCode::Esc)),
                Some(Action::CancelModal)
            ));
            assert!(app.ui.active_modal.is_none());
            assert!(app.cancel_escape.is_none());
        });
    }

    #[test]
    fn keybind_overlay_escape_keeps_overlay_priority() {
        crate::app::tests::with_test_app("request-chord-keybinds", |app| {
            arm_escape(app);
            app.ui.show_keybinds = true;
            assert!(matches!(
                app.handle_key(key(KeyCode::Esc)),
                Some(Action::DismissKeybinds)
            ));
            assert!(app.cancel_escape.is_none());
        });
    }

    #[test]
    fn list_popup_escape_keeps_overlay_priority() {
        crate::app::tests::with_test_app("request-chord-list-popup", |app| {
            arm_escape(app);
            app.ui.list_popup = Some(crate::ui::modals::list_popup::ListPopup::new(
                "Items",
                "Empty",
                Vec::new(),
            ));
            assert!(matches!(
                app.handle_key(key(KeyCode::Esc)),
                Some(Action::CloseListPopup)
            ));
            assert!(app.cancel_escape.is_none());
        });
    }

    #[test]
    fn terminal_confirmation_escape_dismisses_without_arming() {
        crate::app::tests::with_test_app("request-chord-terminal", |app| {
            arm_escape(app);
            app.ui.terminals.close_pending = Some(42);
            assert!(app.handle_key(key(KeyCode::Esc)).is_none());
            assert!(app.ui.terminals.close_pending.is_none());
            assert!(app.cancel_escape.is_none());
        });
    }

    #[test]
    fn session_overlay_escape_closes_without_arming() {
        crate::app::tests::with_test_app("request-chord-sessions", |app| {
            arm_escape(app);
            app.ui.show_session_list = true;
            assert!(matches!(
                app.handle_key(key(KeyCode::Esc)),
                Some(Action::ToggleSessionList)
            ));
            assert!(app.cancel_escape.is_none());
        });
    }

    #[test]
    fn escape_in_a_different_view_clears_chord() {
        crate::app::tests::with_test_app("request-chord-view", |app| {
            arm_escape(app);
            app.ui
                .app_tabs
                .open_view(crate::ui::app_tabs::AppView::Settings);
            assert!(!matches!(
                app.handle_key(key(KeyCode::Esc)),
                Some(Action::CancelRequest)
            ));
            assert!(app.cancel_escape.is_none());
        });
    }

    #[test]
    fn double_escape_targets_displayed_request_and_consumes_chord() {
        crate::app::tests::with_test_app("request-double-escape", |app| {
            let id = register_request(app);
            assert!(app.handle_key(key(KeyCode::Esc)).is_none());
            assert_eq!(app.cancel_escape.map(|(armed, _)| armed), Some(id));
            assert!(matches!(
                app.handle_key(key(KeyCode::Esc)),
                Some(Action::CancelRequest)
            ));
            assert!(app.cancel_escape.is_none());
        });
    }

    #[test]
    fn busy_send_keeps_composer_cursor_history_and_documents() {
        crate::app::tests::with_test_app("request-busy-draft", |app| {
            register_request(app);
            app.pending_documents_mut()
                .insert(
                    crate::app::submission::PendingDocument::snapshot(
                        "source",
                        "draft.md",
                        b"queued document",
                    )
                    .expect("snapshot document"),
                )
                .expect("queue document");
            for draft in ["draft question", "/unknown-command", "/web invalid"] {
                app.ui.tabs[0].input_content = draft.into();
                app.ui.tabs[0].input_cursor = 4;
                app.ui.tabs[0].input_scroll = 2;
                app.ui.tabs[0].input_history_index = Some(0);
                app.ui.tabs[0].input_history_draft = Some("older draft".into());
                assert!(app.handle_key(key(KeyCode::Enter)).is_none());
                assert_eq!(app.ui.tabs[0].input_content, draft);
                assert_eq!(app.ui.tabs[0].input_cursor, 4);
                assert_eq!(app.ui.tabs[0].input_scroll, 2);
                assert_eq!(app.ui.tabs[0].input_history_index, Some(0));
                assert_eq!(
                    app.ui.tabs[0].input_history_draft.as_deref(),
                    Some("older draft")
                );
                assert_eq!(app.pending_documents_mut().len(), 1);
            }
        });
    }

    #[test]
    fn switching_conversation_disarms_escape_without_cancelling_background_request() {
        crate::app::tests::with_test_app("request-chord-conversation", |app| {
            let id = register_request(app);
            let original = app.ui.tabs[0].active_conversation;
            app.handle_key(key(KeyCode::Esc));
            let other = app
                .storage
                .create_conversation(0)
                .expect("create other conversation");
            app.load_conversation_into_tab(0, other)
                .expect("switch conversation");
            assert!(app.cancel_escape.is_none());
            assert!(app.handle_key(key(KeyCode::Esc)).is_none());
            assert!(!app.requests.get(&id).expect("background request").cancelled);
            app.load_conversation_into_tab(0, original)
                .expect("return to conversation");
            assert!(app.handle_key(key(KeyCode::Esc)).is_none());
            assert_eq!(app.cancel_escape.map(|(armed, _)| armed), Some(id));
        });
    }

    #[test]
    fn focused_terminal_escape_is_not_a_chat_cancel_chord() {
        crate::app::tests::with_test_app("request-chord-focused-terminal", |app| {
            arm_escape(app);
            let (terminal, _probe) =
                crate::ui::modals::terminal_editor::terminal_editor_test_support::live_editor();
            let id = app
                .ui
                .terminals
                .insert(crate::ui::terminal_workspace::TerminalSession {
                    terminal,
                    cwd: std::path::PathBuf::from("/tmp"),
                    host: "test-host".into(),
                });
            app.ui.terminals.split_session = Some(id);
            app.ui.terminals.focused = true;
            assert!(app.handle_key(key(KeyCode::Esc)).is_none());
            assert!(app.handle_key(key(KeyCode::Esc)).is_none());
            assert!(app.cancel_escape.is_none());
            assert!(app.ui.tabs[0].streaming);
        });
    }
}
