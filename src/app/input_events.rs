use ratatui::layout::Rect;

use super::{Action, Tab, TuiApp};

impl TuiApp {
    pub(crate) fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> Option<Action> {
        use crossterm::event::{KeyCode, KeyModifiers};

        if key.kind != crossterm::event::KeyEventKind::Press {
            return None;
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
            return Some(Action::ExportConversation);
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
                            self.insert_input_text(&text);
                        }
                        Some(crate::ui::modals::list_popup::ListPopupAction::ReplaceInput(
                            text,
                        )) => {
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
                    if let Some(popup) = &mut self.ui.list_popup {
                        popup.move_up();
                    }
                    None
                }
                KeyCode::Down => {
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
        } else if self.ui.show_settings {
            if self
                .ui
                .settings_popup
                .as_ref()
                .map(|settings| settings.provider_popup_active())
                .unwrap_or(false)
            {
                let popup_action = if let Some(settings) = &mut self.ui.settings_popup {
                    match key.code {
                        KeyCode::Esc => {
                            settings.close_active_provider_popup();
                            crate::ui::settings_tab::ProvidersAction::None
                        }
                        KeyCode::Tab if key.modifiers.contains(KeyModifiers::SHIFT) => {
                            settings.prev_popup_focus();
                            crate::ui::settings_tab::ProvidersAction::None
                        }
                        KeyCode::Tab => {
                            settings.next_popup_focus();
                            crate::ui::settings_tab::ProvidersAction::None
                        }
                        KeyCode::Up => {
                            settings.popup_up();
                            crate::ui::settings_tab::ProvidersAction::None
                        }
                        KeyCode::Down => {
                            settings.popup_down();
                            crate::ui::settings_tab::ProvidersAction::None
                        }
                        KeyCode::Enter | KeyCode::Char(' ') => settings.activate_provider_popup(),
                        KeyCode::Char(c)
                            if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
                        {
                            settings.type_char(c);
                            crate::ui::settings_tab::ProvidersAction::None
                        }
                        KeyCode::Backspace => {
                            settings.backspace();
                            crate::ui::settings_tab::ProvidersAction::None
                        }
                        _ => return None,
                    }
                } else {
                    crate::ui::settings_tab::ProvidersAction::None
                };
                self.apply_settings_provider_action(popup_action);
                return None;
            }

            match key.code {
                KeyCode::Esc => Some(Action::CloseSettings),
                KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(Action::CloseSettings)
                }
                KeyCode::Char(',') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(Action::CloseSettings)
                }
                KeyCode::Tab if key.modifiers.contains(KeyModifiers::SHIFT) => {
                    if let Some(settings) = &mut self.ui.settings_popup {
                        settings.prev_tab();
                    }
                    None
                }
                KeyCode::Tab => {
                    if let Some(settings) = &mut self.ui.settings_popup {
                        settings.next_tab();
                    }
                    None
                }
                KeyCode::Up => {
                    if let Some(settings) = &mut self.ui.settings_popup {
                        if settings.active_tab == crate::ui::settings_tab::SettingsTab::General {
                            if settings.general_dropdown_open.is_some() {
                                settings.general_dropdown_up();
                            } else {
                                settings.prev_focus();
                            }
                        } else if settings.active_tab
                            == crate::ui::settings_tab::SettingsTab::Providers
                        {
                            if settings.providers_dropdown_open.is_some() {
                                settings.providers_dropdown_up();
                            } else {
                                settings.prev_focus();
                            }
                        } else if settings.active_tab
                            == crate::ui::settings_tab::SettingsTab::Models
                        {
                            if settings.models_dropdown_open {
                                settings.models_dropdown_up();
                            } else {
                                settings.prev_focus();
                            }
                        } else if matches!(
                            settings.active_tab,
                            crate::ui::settings_tab::SettingsTab::Local
                                | crate::ui::settings_tab::SettingsTab::Mcp
                        ) {
                            settings.prev_focus();
                        }
                    }
                    None
                }
                KeyCode::Down => {
                    if let Some(settings) = &mut self.ui.settings_popup {
                        if settings.active_tab == crate::ui::settings_tab::SettingsTab::General {
                            if settings.general_dropdown_open.is_some() {
                                settings.general_dropdown_down();
                            } else {
                                settings.next_focus();
                            }
                        } else if settings.active_tab
                            == crate::ui::settings_tab::SettingsTab::Providers
                        {
                            if settings.providers_dropdown_open.is_some() {
                                settings.providers_dropdown_down();
                            } else {
                                settings.next_focus();
                            }
                        } else if settings.active_tab
                            == crate::ui::settings_tab::SettingsTab::Models
                        {
                            if settings.models_dropdown_open {
                                settings.models_dropdown_down();
                            } else {
                                settings.next_focus();
                            }
                        } else if matches!(
                            settings.active_tab,
                            crate::ui::settings_tab::SettingsTab::Local
                                | crate::ui::settings_tab::SettingsTab::Mcp
                        ) {
                            settings.next_focus();
                        }
                    }
                    None
                }
                KeyCode::Enter | KeyCode::Char(' ') => {
                    let mut provider_action = crate::ui::settings_tab::ProvidersAction::None;
                    let mut theme_to_apply = None;
                    let mut models_provider_to_refresh = None;
                    if let Some(ref mut settings) = self.ui.settings_popup {
                        match settings.active_tab {
                            crate::ui::settings_tab::SettingsTab::General => {
                                if settings.general_dropdown_open.is_some() {
                                    let idx = settings.general_dropdown_current_idx();
                                    let changed_theme = settings.select_general_dropdown_item(idx);
                                    if changed_theme {
                                        theme_to_apply = Some(settings.theme.clone());
                                    }
                                } else {
                                    provider_action = settings.activate_focus();
                                    if settings.general_focus
                                        == crate::ui::settings_tab::GeneralFocus::Theme
                                        && settings.general_dropdown_open.is_none()
                                    {
                                        theme_to_apply = Some(settings.theme.clone());
                                    }
                                }
                            }
                            crate::ui::settings_tab::SettingsTab::Providers => {
                                provider_action = settings.activate_focus();
                            }
                            crate::ui::settings_tab::SettingsTab::Models => {
                                if settings.models_dropdown_open {
                                    settings.select_models_provider_dropdown_item(0);
                                    if let Ok(models) =
                                        self.storage.get_models(&settings.models_provider)
                                    {
                                        settings.models_available_models = models
                                            .into_iter()
                                            .map(
                                                |(
                                                    id,
                                                    input_price,
                                                    output_price,
                                                    context_window,
                                                )| {
                                                    crate::ui::settings_tab::ModelInfo {
                                                        id,
                                                        input_price,
                                                        output_price,
                                                        context_window,
                                                    }
                                                },
                                            )
                                            .collect();
                                    }
                                    if settings.models_available_models.is_empty() {
                                        models_provider_to_refresh =
                                            Some(settings.models_provider.clone());
                                    }
                                } else {
                                    settings.activate_models_focus();
                                }
                            }
                            crate::ui::settings_tab::SettingsTab::Local => {
                                provider_action = settings.activate_focus();
                            }
                            crate::ui::settings_tab::SettingsTab::Mcp => {
                                provider_action = settings.activate_focus();
                            }
                            _ => {}
                        }
                    }
                    if let Some(theme) = theme_to_apply {
                        let _ = self.apply_theme_selection(&theme);
                    }
                    if let Some(provider) = models_provider_to_refresh {
                        self.refresh_models_for_provider(provider);
                    }
                    self.apply_settings_provider_action(provider_action);
                    None
                }
                KeyCode::Char(c)
                    if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
                {
                    if let Some(settings) = &mut self.ui.settings_popup {
                        settings.type_char(c);
                    }
                    None
                }
                KeyCode::Backspace => {
                    if let Some(settings) = &mut self.ui.settings_popup {
                        settings.backspace();
                    }
                    None
                }
                _ => None,
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
                    if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
                        let content = tab.input_content.clone();
                        if !content.is_empty() {
                            tab.input_history_index = None;
                            tab.input_history_draft = None;
                            tab.input_content.clear();
                            tab.input_cursor = 0;
                            tab.input_scroll = 0;
                            let trimmed = content.trim();
                            match trimmed {
                                "/quit" | "/exit" | "/q" => return Some(self.quit_action()),
                                "/skills" => return Some(Action::ShowSkillsPopup),
                                "/mcp" => return Some(Action::ShowMcpPopup),
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
                    }
                    None
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
            MouseEventKind::ScrollUp => {
                self.handle_mouse_scroll(mouse.column, mouse.row, mouse.modifiers, false);
                None
            }
            MouseEventKind::ScrollDown => {
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

        if self.ui.show_settings {
            if let Some(settings) = &mut self.ui.settings_popup {
                if settings.provider_popup_active() {
                    if down {
                        settings.popup_down();
                    } else {
                        settings.popup_up();
                    }
                    return;
                }

                if settings.general_dropdown_open.is_some() {
                    if down {
                        settings.general_dropdown_down();
                    } else {
                        settings.general_dropdown_up();
                    }
                    return;
                }

                if settings.providers_dropdown_open.is_some() {
                    if down {
                        settings.providers_dropdown_down();
                    } else {
                        settings.providers_dropdown_up();
                    }
                }
            }
            return;
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

        if let Some(viewer) = &self.ui.artifact_viewer {
            if let Some(chat_area) = self.ui.chat_area {
                let popup_area = crate::ui::modals::artifact_viewer::popup_area(chat_area);
                if popup_area.contains(pos) {
                    if viewer
                        .hit_areas
                        .close
                        .is_some_and(|area| area.contains(pos))
                    {
                        self.ui.artifact_viewer = None;
                        return None;
                    }
                    if viewer.hit_areas.save.is_some_and(|area| area.contains(pos)) {
                        self.prepare_artifact_save(viewer.handle().clone());
                        return None;
                    }
                    if viewer
                        .hit_areas
                        .delete
                        .is_some_and(|area| area.contains(pos))
                    {
                        self.delete_artifact(viewer.handle().clone());
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

        // Check settings popup
        if self.ui.show_settings {
            // Check if click is inside settings popup
            let popup_area = crate::ui::settings_tab::SettingsPopup::popup_area(area);
            if popup_area.contains(pos) {
                if self
                    .ui
                    .settings_popup
                    .as_ref()
                    .map(|settings| settings.provider_popup_active())
                    .unwrap_or(false)
                {
                    let action = if let Some(settings) = &mut self.ui.settings_popup {
                        settings.handle_provider_popup_click(pos)
                    } else {
                        crate::ui::settings_tab::ProvidersAction::None
                    };
                    self.apply_settings_provider_action(action);
                    return None;
                }

                // Check settings tabs
                if let Some(areas) = &self.ui.settings_tab_areas {
                    for (i, tab_area) in areas.iter().enumerate() {
                        if tab_area.contains(pos) {
                            if let Some(settings) = &mut self.ui.settings_popup {
                                settings.active_tab = match i {
                                    0 => crate::ui::settings_tab::SettingsTab::General,
                                    1 => crate::ui::settings_tab::SettingsTab::Keybindings,
                                    2 => crate::ui::settings_tab::SettingsTab::Providers,
                                    3 => crate::ui::settings_tab::SettingsTab::Models,
                                    4 => crate::ui::settings_tab::SettingsTab::Local,
                                    5 => crate::ui::settings_tab::SettingsTab::Mcp,
                                    _ => crate::ui::settings_tab::SettingsTab::General,
                                };
                            }
                            return None;
                        }
                    }
                }
                let mut models_provider_to_refresh = None;
                let mut models_dropdown_handled = false;
                let mut models_dropdown_closed = false;
                if let Some(ref mut settings) = self.ui.settings_popup {
                    if settings.provider_popup_active() {
                        let action = settings.handle_provider_popup_click(pos);
                        self.apply_settings_provider_action(action);
                        return None;
                    }
                    if settings.active_tab == crate::ui::settings_tab::SettingsTab::General {
                        let mut theme_to_apply = None;
                        if let Some(dropdown) = settings.general_dropdown_open {
                            for (i, item_area) in
                                settings.general_hit_areas.dropdown_items.iter().enumerate()
                            {
                                if item_area.contains(pos) {
                                    let changed_theme = settings.select_general_dropdown_item(i);
                                    if changed_theme {
                                        theme_to_apply = Some(settings.theme.clone());
                                    }
                                    if dropdown
                                        == crate::ui::settings_tab::GeneralDropdown::UserAlignment
                                    {
                                        if let Ok(models) =
                                            self.storage.get_models(&settings.default_provider)
                                        {
                                            settings.available_models = models
                                                .into_iter()
                                                .map(|(id, input_price, output_price, context_window)| {
                                                    crate::ui::settings_tab::ModelInfo {
                                                        id,
                                                        input_price,
                                                        output_price,
                                                        context_window,
                                                    }
                                                })
                                                .collect();
                                        }
                                    }
                                    if let Some(theme) = theme_to_apply {
                                        let _ = self.apply_theme_selection(&theme);
                                    }
                                    return None;
                                }
                            }
                            settings.close_general_dropdown();
                            return None;
                        }
                        if let Some(area) = settings.general_hit_areas.user_alignment {
                            if area.contains(pos) {
                                settings.toggle_general_dropdown(
                                    crate::ui::settings_tab::GeneralDropdown::UserAlignment,
                                );
                                settings.general_focus =
                                    crate::ui::settings_tab::GeneralFocus::UserAlignment;
                                return None;
                            }
                        }
                        if let Some(area) = settings.general_hit_areas.theme {
                            if area.contains(pos) {
                                settings.toggle_general_dropdown(
                                    crate::ui::settings_tab::GeneralDropdown::Theme,
                                );
                                settings.general_focus =
                                    crate::ui::settings_tab::GeneralFocus::Theme;
                                return None;
                            }
                        }
                        if let Some(area) = settings.general_hit_areas.ai_alignment {
                            if area.contains(pos) {
                                settings.toggle_general_dropdown(
                                    crate::ui::settings_tab::GeneralDropdown::AiAlignment,
                                );
                                settings.general_focus =
                                    crate::ui::settings_tab::GeneralFocus::AiAlignment;
                                return None;
                            }
                        }
                        if let Some(area) = settings.general_hit_areas.artifact_save_dir {
                            if area.contains(pos) {
                                settings.general_focus =
                                    crate::ui::settings_tab::GeneralFocus::ArtifactSaveDir;
                                return None;
                            }
                        }
                        if let Some(area) = settings.general_hit_areas.show_selector {
                            if area.contains(pos) {
                                settings.show_selector = !settings.show_selector;
                                settings.general_focus =
                                    crate::ui::settings_tab::GeneralFocus::ShowSelector;
                                return None;
                            }
                        }
                        if let Some(area) = settings.general_hit_areas.show_chat_scrollbar {
                            if area.contains(pos) {
                                settings.show_chat_scrollbar = !settings.show_chat_scrollbar;
                                settings.general_focus =
                                    crate::ui::settings_tab::GeneralFocus::ShowChatScrollbar;
                                return None;
                            }
                        }
                        if let Some(area) = settings.general_hit_areas.collapse_thinking {
                            if area.contains(pos) {
                                settings.collapse_thinking = !settings.collapse_thinking;
                                settings.general_focus =
                                    crate::ui::settings_tab::GeneralFocus::CollapseThinking;
                                return None;
                            }
                        }
                        if let Some(area) = settings.general_hit_areas.kitty_enhanced_text {
                            if area.contains(pos) {
                                settings.kitty_enhanced_text = !settings.kitty_enhanced_text;
                                settings.general_focus =
                                    crate::ui::settings_tab::GeneralFocus::KittyEnhancedText;
                                return None;
                            }
                        }
                        if let Some(area) = settings.general_hit_areas.kitty_text_scale {
                            if area.contains(pos) {
                                settings.toggle_general_dropdown(
                                    crate::ui::settings_tab::GeneralDropdown::KittyTextScale,
                                );
                                settings.general_focus =
                                    crate::ui::settings_tab::GeneralFocus::KittyTextScale;
                                return None;
                            }
                        }
                        if let Some(area) = settings.general_hit_areas.web_search_enabled {
                            if area.contains(pos) {
                                settings.web_search_enabled = !settings.web_search_enabled;
                                settings.general_focus =
                                    crate::ui::settings_tab::GeneralFocus::WebSearchEnabled;
                                return None;
                            }
                        }
                        if let Some(area) = settings.general_hit_areas.quit_confirmation {
                            if area.contains(pos) {
                                settings.quit_confirmation = !settings.quit_confirmation;
                                settings.general_focus =
                                    crate::ui::settings_tab::GeneralFocus::QuitConfirmation;
                                return None;
                            }
                        }
                    } else if settings.active_tab == crate::ui::settings_tab::SettingsTab::Local {
                        if let Some(area) = settings.local_hit_areas.enabled {
                            if area.contains(pos) {
                                settings.local_focus = crate::ui::settings_tab::LocalFocus::Enabled;
                                settings.local_enabled = !settings.local_enabled;
                                return None;
                            }
                        }
                        if let Some(area) = settings.local_hit_areas.host {
                            if area.contains(pos) {
                                settings.local_focus = crate::ui::settings_tab::LocalFocus::Host;
                                return None;
                            }
                        }
                        if let Some(area) = settings.local_hit_areas.port {
                            if area.contains(pos) {
                                settings.local_focus = crate::ui::settings_tab::LocalFocus::Port;
                                return None;
                            }
                        }
                        if let Some(area) = settings.local_hit_areas.server_type {
                            if area.contains(pos) {
                                settings.local_focus =
                                    crate::ui::settings_tab::LocalFocus::ServerType;
                                let _ = settings.activate_focus();
                                return None;
                            }
                        }
                        if let Some(area) = settings.local_hit_areas.selected_model {
                            if area.contains(pos) {
                                settings.local_focus =
                                    crate::ui::settings_tab::LocalFocus::SelectedModel;
                                return None;
                            }
                        }
                        if let Some(area) = settings.local_hit_areas.model_directory {
                            if area.contains(pos) {
                                settings.local_focus =
                                    crate::ui::settings_tab::LocalFocus::ModelDirectory;
                                return None;
                            }
                        }
                        if let Some(area) = settings.local_hit_areas.health_interval {
                            if area.contains(pos) {
                                settings.local_focus =
                                    crate::ui::settings_tab::LocalFocus::HealthInterval;
                                return None;
                            }
                        }
                        if let Some(area) = settings.local_hit_areas.connect_timeout {
                            if area.contains(pos) {
                                settings.local_focus =
                                    crate::ui::settings_tab::LocalFocus::ConnectTimeout;
                                return None;
                            }
                        }
                        if let Some(area) = settings.local_hit_areas.request_timeout {
                            if area.contains(pos) {
                                settings.local_focus =
                                    crate::ui::settings_tab::LocalFocus::RequestTimeout;
                                return None;
                            }
                        }
                        if let Some(area) = settings.local_hit_areas.api_token_env {
                            if area.contains(pos) {
                                settings.local_focus =
                                    crate::ui::settings_tab::LocalFocus::ApiTokenEnv;
                                return None;
                            }
                        }
                    } else if settings.active_tab == crate::ui::settings_tab::SettingsTab::Mcp {
                        for (idx, area) in &settings.mcp_hit_areas.rows {
                            if area.contains(pos) {
                                settings.mcp_focus = *idx;
                                let _ = settings.activate_focus();
                                return None;
                            }
                        }
                    } else if settings.active_tab == crate::ui::settings_tab::SettingsTab::Models {
                        if settings.models_dropdown_open {
                            let mut provider_to_refresh = None;
                            let mut handled = false;
                            for (i, area) in settings
                                .models_tab_hit_areas
                                .provider_items
                                .iter()
                                .enumerate()
                            {
                                if area.contains(pos) {
                                    settings.select_models_provider_dropdown_item(i);
                                    if let Ok(models) =
                                        self.storage.get_models(&settings.models_provider)
                                    {
                                        settings.models_available_models = models
                                            .into_iter()
                                            .map(
                                                |(
                                                    id,
                                                    input_price,
                                                    output_price,
                                                    context_window,
                                                )| {
                                                    crate::ui::settings_tab::ModelInfo {
                                                        id,
                                                        input_price,
                                                        output_price,
                                                        context_window,
                                                    }
                                                },
                                            )
                                            .collect();
                                    }
                                    if settings.models_available_models.is_empty() {
                                        provider_to_refresh =
                                            Some(settings.models_provider.clone());
                                    }
                                    handled = true;
                                    break;
                                }
                            }
                            models_provider_to_refresh = provider_to_refresh;
                            if handled {
                                models_dropdown_handled = true;
                            } else {
                                settings.models_dropdown_open = false;
                                models_dropdown_closed = true;
                            }
                        }
                        if let Some(area) = settings.models_tab_hit_areas.provider {
                            if area.contains(pos) {
                                settings.models_tab_focus =
                                    crate::ui::settings_tab::ModelsTabFocus::Provider;
                                settings.toggle_models_dropdown();
                                return None;
                            }
                        }
                        for (idx, row_area) in
                            settings.models_tab_hit_areas.model_rows.iter().enumerate()
                        {
                            if row_area.contains(pos) {
                                settings.models_tab_focus =
                                    crate::ui::settings_tab::ModelsTabFocus::Model(idx);
                                settings.activate_models_focus();
                                return None;
                            }
                        }
                    }
                }
                if let Some(provider) = models_provider_to_refresh {
                    self.refresh_models_for_provider(provider);
                }
                if models_dropdown_handled || models_dropdown_closed {
                    return None;
                }

                let mut provider_action = crate::ui::settings_tab::ProvidersAction::None;
                let mut refresh_after_settings_selection = None;
                if let Some(ref mut settings) = self.ui.settings_popup {
                    if settings.active_tab == crate::ui::settings_tab::SettingsTab::Providers {
                        if let Some(_popup) = settings.preset_key_popup.as_mut() {
                            provider_action = settings.handle_providers_click(pos);
                        } else if let Some(dropdown) = settings.providers_dropdown_open {
                            let item_areas = match dropdown {
                                crate::ui::settings_tab::ProvidersDropdown::DefaultProvider
                                | crate::ui::settings_tab::ProvidersDropdown::SmallProvider => {
                                    &settings.providers_tab_hit_areas.default_provider_items
                                }
                                crate::ui::settings_tab::ProvidersDropdown::DefaultModel
                                | crate::ui::settings_tab::ProvidersDropdown::SmallModel => {
                                    &settings.providers_tab_hit_areas.default_model_items
                                }
                            };
                            let mut handled = false;
                            for (i, area) in item_areas.iter().enumerate() {
                                if area.contains(pos) {
                                    settings.select_providers_dropdown_item(i);
                                    handled = true;
                                    break;
                                }
                            }
                            if handled {
                                if dropdown
                                    == crate::ui::settings_tab::ProvidersDropdown::DefaultProvider
                                {
                                    if let Ok(models) =
                                        self.storage.get_models(&settings.default_provider)
                                    {
                                        settings.available_models = models
                                            .into_iter()
                                            .map(
                                                |(
                                                    id,
                                                    input_price,
                                                    output_price,
                                                    context_window,
                                                )| {
                                                    crate::ui::settings_tab::ModelInfo {
                                                        id,
                                                        input_price,
                                                        output_price,
                                                        context_window,
                                                    }
                                                },
                                            )
                                            .collect();
                                    }
                                    if settings.available_models.is_empty() {
                                        refresh_after_settings_selection =
                                            Some(settings.default_provider.clone());
                                    }
                                }
                                if let Some(provider) = refresh_after_settings_selection {
                                    self.refresh_models_for_provider(provider);
                                }
                                return None;
                            } else {
                                settings.providers_dropdown_open = None;
                                return None;
                            }
                        }
                        if let Some(area) = settings.providers_tab_hit_areas.default_provider {
                            if area.contains(pos) {
                                settings.toggle_providers_dropdown(
                                    crate::ui::settings_tab::ProvidersDropdown::DefaultProvider,
                                );
                                settings.providers_tab_focus =
                                    crate::ui::settings_tab::ProvidersTabFocus::DefaultProvider;
                                return None;
                            }
                        }
                        if let Some(area) = settings.providers_tab_hit_areas.default_model {
                            if area.contains(pos) {
                                settings.toggle_providers_dropdown(
                                    crate::ui::settings_tab::ProvidersDropdown::DefaultModel,
                                );
                                settings.providers_tab_focus =
                                    crate::ui::settings_tab::ProvidersTabFocus::DefaultModel;
                                return None;
                            }
                        }
                        if let Some(area) = settings.providers_tab_hit_areas.reload_models_button {
                            if area.contains(pos) {
                                settings.providers_tab_focus =
                                    crate::ui::settings_tab::ProvidersTabFocus::ReloadModelsButton;
                                provider_action = settings.activate_focus();
                            }
                        }
                        if let Some(area) = settings.providers_tab_hit_areas.grab_env_button {
                            if area.contains(pos) {
                                settings.providers_tab_focus =
                                    crate::ui::settings_tab::ProvidersTabFocus::UseEnvToggle;
                                provider_action = settings.activate_focus();
                            }
                        }
                        if let Some(area) = settings.providers_tab_hit_areas.add_button {
                            if area.contains(pos) {
                                settings.providers_tab_focus =
                                    crate::ui::settings_tab::ProvidersTabFocus::AddProviderButton;
                                provider_action = settings.activate_focus();
                            }
                        }
                        if let Some(area) = settings.providers_tab_hit_areas.edit_button {
                            if area.contains(pos) {
                                settings.providers_tab_focus =
                                    crate::ui::settings_tab::ProvidersTabFocus::EditProvidersButton;
                                provider_action = settings.activate_focus();
                            }
                        }
                        for (idx, row_area) in settings
                            .providers_tab_hit_areas
                            .saved_key_rows
                            .iter()
                            .enumerate()
                        {
                            if row_area.contains(pos) {
                                settings.providers_tab_focus =
                                    crate::ui::settings_tab::ProvidersTabFocus::SavedKeyList(idx);
                                let _ = settings.activate_focus();
                                return None;
                            }
                        }
                        for (idx, row_area) in settings
                            .providers_tab_hit_areas
                            .oauth_rows
                            .iter()
                            .enumerate()
                        {
                            if row_area.contains(pos) {
                                settings.providers_tab_focus =
                                    crate::ui::settings_tab::ProvidersTabFocus::OAuthProvider(idx);
                                let _ = settings.activate_focus();
                                return None;
                            }
                        }
                        let mut preset_clicked = None;
                        for (idx, row_area) in settings
                            .providers_tab_hit_areas
                            .preset_rows
                            .iter()
                            .enumerate()
                        {
                            if row_area.contains(pos) {
                                preset_clicked = Some(idx);
                                break;
                            }
                        }
                        if let Some(idx) = preset_clicked {
                            settings.providers_tab_focus =
                                crate::ui::settings_tab::ProvidersTabFocus::PresetProvider(idx);
                            provider_action = settings.activate_focus();
                        }
                    }
                    if settings.active_tab == crate::ui::settings_tab::SettingsTab::Models
                        && settings.models_dropdown_open
                    {
                        return None;
                    }
                }
                self.apply_settings_provider_action(provider_action.clone());
                return None;
            } else {
                // Click outside popup - close it
                return Some(Action::CloseSettings);
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
            if let Some(scrollbar) = tab.chat_scrollbar_area {
                if scrollbar.contains(pos) {
                    let max_scroll = tab
                        .total_rendered_lines
                        .saturating_sub(tab.message_viewport_height);
                    if max_scroll > 0 && scrollbar.height > 0 {
                        let relative = pos.y.saturating_sub(scrollbar.y) as usize;
                        tab.scroll_offset =
                            ((relative * max_scroll) / scrollbar.height as usize).min(max_scroll);
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

        if self.ui.show_selector {
            let mut selected_provider = None;
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
                                    tab.tab.model = self.ui.current_models[real_idx].id.clone();
                                    tab.tab.reasoning_effort = None;
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
                    self.refresh_models_for_provider(provider);
                } else if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
                    if tab.tab.model.is_empty() {
                        if let Some(first) = self.ui.current_models.first() {
                            tab.tab.model = first.id.clone();
                        }
                    }
                }
                self.refresh_visible_selectors();
                self.queue_connection_check_for_active_tab();
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
                    self.delete_artifact(handle);
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
