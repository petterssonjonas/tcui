use ratatui::layout::Rect;

use super::{Action, Tab, TuiApp};
use crate::app::action::MouseClickAction;

impl TuiApp {
    fn action_for_slash_command(&self, text: &str) -> Option<Action> {
        match text.trim() {
            "/quit" | "/exit" | "/q" => Some(self.quit_action()),
            "/skills" => Some(Action::ShowSkillsPopup),
            "/mcp" => Some(Action::ShowMcpPopup),
            "/settings" => Some(Action::OpenSettingsPanel),
            "/help" => Some(Action::ShowHelp),
            "/keybinds" => Some(Action::ShowKeybinds),
            "/web" => Some(Action::ToggleWebSearch),
            _ => None,
        }
    }

    pub(crate) fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> Option<Action> {
        use crossterm::event::{KeyCode, KeyModifiers};

        if key.kind != crossterm::event::KeyEventKind::Press {
            return None;
        }

        // TODO: Keybind editing is disabled in the settings UI (no keybind entries in
        // all_settings). This capture flow is preserved and persisted to config, but
        // normal key dispatch does not yet consult keybinding_overrides. Re-enable
        // keybind entries in all_settings() once dispatch is wired through the binding map.
        if let Some(mut capture) = self.ui.keybind_capture.take() {
            let mut effective_bindings = std::collections::BTreeMap::new();
            for setting in crate::tui::settings_panel::all_settings() {
                if let crate::tui::settings_panel::SettingType::Keybind {
                    action_id,
                    default_binding,
                    reserved: _,
                } = setting.setting_type
                {
                    effective_bindings.insert(action_id.to_string(), default_binding.to_string());
                }
            }
            effective_bindings.extend(self.ui.keybinding_overrides.clone());
            let result = capture.capture_with_overrides(key, &effective_bindings);
            match result {
                crate::tui::keybind_capture::CaptureResult::Captured(_) => {
                    let action_id = capture.action_id.clone();
                    let action_label = capture.action_label.clone();
                    if let Some(binding) = capture.confirm() {
                        self.ui
                            .keybinding_overrides
                            .insert(action_id.clone(), binding.clone());
                        self.ui
                            .show_toast(format!("Bound {action_label} → {binding}"));
                        if let Ok(mut config) = self.config.try_write() {
                            config.tui.keybinding_overrides = self.ui.keybinding_overrides.clone();
                            let _ = config.save();
                        }
                    }
                }
                crate::tui::keybind_capture::CaptureResult::Cleared => {
                    let action_id = capture.action_id.clone();
                    self.ui.keybinding_overrides.remove(&action_id);
                    if let Ok(mut config) = self.config.try_write() {
                        config.tui.keybinding_overrides = self.ui.keybinding_overrides.clone();
                        let _ = config.save();
                    }
                }
                crate::tui::keybind_capture::CaptureResult::Conflict(_)
                | crate::tui::keybind_capture::CaptureResult::Waiting => {
                    self.ui.keybind_capture = Some(capture);
                }
                crate::tui::keybind_capture::CaptureResult::Cancelled => {}
            }
            return None;
        }

        if self.ui.show_keybinds {
            return match key.code {
                KeyCode::Esc | KeyCode::Enter => Some(Action::DismissKeybinds),
                _ => None,
            };
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
                    if settings.confirm.is_some() {
                        settings.confirm_reset();
                        return None;
                    }
                    let selected_id = settings
                        .selected_setting(&catalog)
                        .map(|setting| setting.id);
                    match settings.enter(&catalog) {
                        crate::tui::settings_panel::EnterResult::OpenKeybind {
                            action_id,
                            action_label,
                        } => {
                            self.ui.keybind_capture =
                                Some(crate::tui::keybind_capture::KeybindCaptureState::new(
                                    action_id,
                                    action_label,
                                ));
                        }
                        crate::tui::settings_panel::EnterResult::SelectTheme(theme) => {
                            let _ = self.apply_theme_selection(theme);
                        }
                        crate::tui::settings_panel::EnterResult::SelectToastPosition(position) => {
                            let _ = self.apply_toast_position_selection(position);
                        }
                        crate::tui::settings_panel::EnterResult::ToggledBool => match selected_id {
                            Some("web_search") => return Some(Action::ToggleWebSearch),
                            Some("collapse_thinking") => {
                                return Some(Action::ToggleCollapseThinking);
                            }
                            _ => {}
                        },
                        crate::tui::settings_panel::EnterResult::Nothing
                        | crate::tui::settings_panel::EnterResult::EnteredSubsection
                        | crate::tui::settings_panel::EnterResult::RequestConfirm => {}
                    }
                    None
                }
                KeyCode::Char(' ') => {
                    let selected_id = settings
                        .selected_setting(&catalog)
                        .map(|setting| setting.id);
                    if !settings.toggle_bool(&catalog) {
                        settings.insert_char(' ');
                    } else {
                        match selected_id {
                            Some("web_search") => return Some(Action::ToggleWebSearch),
                            Some("collapse_thinking") => {
                                return Some(Action::ToggleCollapseThinking);
                            }
                            _ => {}
                        }
                    }
                    None
                }
                KeyCode::Left | KeyCode::Right if settings.confirm.is_some() => {
                    settings.toggle_confirm_selection();
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

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('e') {
            self.open_chat_draft_editor();
            return None;
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

        if self.ui.list_popup.is_some() {
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
                                self.replace_input_content(String::new());
                                return Some(action);
                            }
                            self.insert_input_text(&text);
                        }
                        Some(crate::ui::modals::list_popup::ListPopupAction::ReplaceInput(
                            text,
                        )) => {
                            if let Some(action) = self.action_for_slash_command(&text) {
                                self.replace_input_content(String::new());
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
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    self.ui.active_modal = None;
                    Some(Action::ConfirmQuit)
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
            match key.code {
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
                    let tab = Tab::new("New Chat".to_string(), String::new(), String::new());
                    Some(Action::AddTab(tab))
                }
                KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(Action::NewChat)
                }
                KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if key.modifiers.contains(KeyModifiers::SHIFT) {
                        Some(Action::CloseChat)
                    } else {
                        Some(Action::RemoveTab(self.ui.active_tab))
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
                        if let Some(content) = self.take_input_submission() {
                            let trimmed = content.trim();
                            match trimmed {
                                "/quit" | "/exit" | "/q" => return Some(self.quit_action()),
                                "/skills" => return Some(Action::ShowSkillsPopup),
                                "/mcp" => return Some(Action::ShowMcpPopup),
                                "/settings" => return Some(Action::OpenSettingsPanel),
                                "/help" => return Some(Action::ShowHelp),
                                "/keybinds" => return Some(Action::ShowKeybinds),
                                "/web" => return Some(Action::ToggleWebSearch),
                                _ => {
                                    if trimmed == "/theme" {
                                        self.show_theme_popup("");
                                        return None;
                                    }
                                    if let Some(theme_name) = trimmed.strip_prefix("/theme ") {
                                        let _ = self.apply_theme_selection(theme_name.trim());
                                        return None;
                                    }
                                    if let Some(query) = trimmed.strip_prefix("/vault ") {
                                        return Some(Action::ShowLocalSearch(
                                            query.trim().to_string(),
                                        ));
                                    }
                                    if let Some(value) = trimmed.strip_prefix("/web ") {
                                        let value = value.trim().to_lowercase();
                                        if value == "on" || value == "off" {
                                            let enabled = value == "on";
                                            let current = self
                                                .config
                                                .try_read()
                                                .map(|config| config.web_search.enabled)
                                                .unwrap_or(false);
                                            if enabled != current {
                                                return Some(Action::ToggleWebSearch);
                                            }
                                            return None;
                                        }
                                    }
                                    return Some(Action::SendMessage(content));
                                }
                            }
                        }
                        None
                    }
                }
                KeyCode::Char(c) => {
                    if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT {
                        self.insert_input_char(c);
                    }
                    None
                }
                KeyCode::Backspace => {
                    self.backspace_input_char();
                    None
                }
                _ => None,
            }
        }
    }

    pub(crate) fn handle_mouse(&mut self, mouse: crossterm::event::MouseEvent) -> Option<Action> {
        use crossterm::event::{MouseButton, MouseEventKind};

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.handle_mouse_click(mouse.column, mouse.row)
            }
            MouseEventKind::Down(MouseButton::Right) => {
                self.handle_key(crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Esc,
                    crossterm::event::KeyModifiers::NONE,
                ))
            }
            MouseEventKind::ScrollUp => {
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

    pub(crate) fn handle_mouse_scroll(
        &mut self,
        col: u16,
        row: u16,
        modifiers: crossterm::event::KeyModifiers,
        down: bool,
    ) {
        let pos = ratatui::layout::Position::new(col, row);
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
                        if let Some(mut editor) = self.ui.editor_popup.take() {
                            editor.close();
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
                    let (close, edit, delete, handle) =
                        self.ui.artifact_viewer.as_ref().map(|viewer| {
                            (
                                viewer.hit_areas.close,
                                viewer.hit_areas.edit,
                                viewer.hit_areas.delete,
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
                        self.insert_input_text(&text);
                    }
                    Some(crate::ui::modals::list_popup::ListPopupAction::ReplaceInput(text)) => {
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

        if self.ui.settings_v2.is_some()
            && !crate::tui::components::centered_rect(70, 60, area).contains(pos)
        {
            self.ui.settings_v2 = None;
            return None;
        }

        if let Some((_, mouse_action)) = self
            .ui
            .mouse_hit_areas
            .iter()
            .rev()
            .find(|(hit_area, _)| hit_area.contains(pos))
            .copied()
        {
            match mouse_action {
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
            }
        }

        // Check modal first (takes priority)
        if self.ui.active_modal.is_some() {
            if let Some(modal_areas) = self.ui.modal_areas {
                if modal_areas.yes.contains(pos) {
                    self.ui.active_modal = None;
                    return Some(Action::ConfirmQuit);
                }
                if modal_areas.no.contains(pos) {
                    self.ui.active_modal = None;
                    return Some(Action::CancelModal);
                }
            }
            self.ui.active_modal = None;
            return Some(Action::CancelModal);
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
                    self.set_input_cursor_from_click(pos);
                    return None;
                }
            }
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
                                        tab.tab.model.clear();
                                        tab.tab.reasoning_effort = None;
                                        selected_provider = Some(new_provider);
                                    }
                                }
                                tab.provider_dropdown_open = false;
                            } else if tab.model_dropdown_open {
                                if real_idx < self.ui.current_models.len() {
                                    let model = self.ui.current_models[real_idx].id.clone();
                                    tab.tab.model = model.clone();
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

        // Top bar layout
        let top_bar_area = Rect::new(area.x, area.y, area.width, 1);
        let top_bar = crate::ui::top_bar::TopBar::new(
            &self.ui.tabs,
            self.ui.active_tab,
            self.ui.sidebar_open,
            self.ui.artifact_sidebar_open,
        );

        // Check hamburger button
        let hamburger = top_bar.hamburger_area(top_bar_area);
        if hamburger.contains(pos) {
            return Some(Action::ToggleSidebar);
        }

        let settings = top_bar.settings_area(top_bar_area);
        if settings.contains(pos) {
            return Some(Action::ShowSettings);
        }

        let artifact_toggle = top_bar.artifact_toggle_area(top_bar_area);
        if artifact_toggle.contains(pos) {
            return Some(Action::ToggleArtifactSidebar);
        }

        // Check close button
        let close = top_bar.close_area(top_bar_area);
        if close.contains(pos) {
            return Some(self.quit_action());
        }

        // Check tabs using accurate hit areas
        for hit in top_bar.tab_hit_areas(top_bar_area) {
            if hit.area.contains(pos) {
                return Some(Action::SwitchTab(hit.index));
            }
        }

        // Sidebar layout (only if open)
        if self.ui.sidebar_open {
            let sidebar_width = crate::ui::sidebar::SIDEBAR_WIDTH;
            let sidebar_area = Rect::new(area.x, area.y + 1, sidebar_width, area.height - 2);
            let active_tab = self.ui.tabs.get(self.ui.active_tab)?;
            let sidebar = crate::ui::sidebar::Sidebar::new(
                &active_tab.conversations,
                active_tab.active_conversation,
            );

            for target in sidebar.hit_targets(sidebar_area) {
                if target.area.contains(pos) {
                    return Some(match target.action {
                        crate::ui::sidebar::SidebarAction::NewChat => Action::NewChat,
                        crate::ui::sidebar::SidebarAction::LoadConversation(conversation_id) => {
                            Action::LoadConversation(conversation_id)
                        }
                        crate::ui::sidebar::SidebarAction::TogglePinned(conversation_id) => {
                            Action::ToggleConversationPinned(conversation_id)
                        }
                        crate::ui::sidebar::SidebarAction::ExportConversation(conversation_id) => {
                            Action::ExportConversationId(conversation_id)
                        }
                        crate::ui::sidebar::SidebarAction::DeleteConversation(conversation_id) => {
                            Action::DeleteConversation(conversation_id)
                        }
                    });
                }
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
