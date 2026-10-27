use color_eyre::eyre::{eyre, Result};
use crossterm::event::{KeyEvent, MouseEvent};

use super::{Action, TuiApp};
use crate::{
    config::{prompts::PromptLayer, AppConfig},
    ui::settings_workspace::{validate_edit, SettingsIntent},
};

impl TuiApp {
    pub(crate) fn sidebar_settings_panel(&self) -> crate::tui::settings_panel::SettingsPanelState {
        crate::tui::settings_panel::SettingsPanelState::new()
    }

    /// New conversations use saved defaults without changing loaded conversations.
    pub(crate) fn apply_default_chat_selection(&mut self, tab_index: usize) {
        let selection = self.config.try_read().ok().map(|config| {
            let mut selection = config.default_chat.clone();
            selection.provider = config
                .provider_config(&selection.provider)
                .map(|provider| provider.name.clone())
                .unwrap_or_else(|| crate::llm::auth::canonical_provider_name(&selection.provider));
            if crate::llm::local::is_local_provider(&selection.provider)
                && selection.model.is_empty()
            {
                selection.model = config.local_inference.selected_model.clone();
            }
            selection
        });
        if let (Some(selection), Some(tab)) = (selection, self.ui.tabs.get_mut(tab_index)) {
            tab.tab.provider = selection.provider;
            tab.tab.model = selection.model;
            tab.tab.endpoint = None;
            tab.tab.reasoning_effort = selection.effort;
            // Saved selections are explicit, including private/manual models and delayed catalogs.
            tab.manual_model_override = true;
        }
        if tab_index == self.ui.active_tab {
            self.refresh_visible_selectors();
        }
    }

    pub(crate) fn open_settings_page(&mut self, id: Option<&str>) {
        match self.config.try_read() {
            Ok(config) => self.ui.settings_workspace.load(&config),
            Err(_) => self
                .ui
                .settings_workspace
                .fail("Configuration is busy. Reopen settings to retry; no values were changed."),
        }
        if let Some(id) = id {
            self.ui.settings_workspace.select_id(id);
        }
        self.ui.settings_v2 = None;
        self.ui.palette = None;
        self.ui
            .app_tabs
            .open_view(crate::ui::app_tabs::AppView::Settings);
    }

    fn settings_intent_action(intent: SettingsIntent) -> Option<Action> {
        match intent {
            SettingsIntent::Apply { id, value } => Some(Action::ApplySetting { id, value }),
            SettingsIntent::Command(command) => crate::tui::settings_panel::command_action(command),
            SettingsIntent::Save { draft } => Some(Action::SaveSettings { draft }),
        }
    }

    pub(crate) fn settings_workspace_key(&mut self, key: KeyEvent) -> Option<Action> {
        self.ui
            .settings_workspace
            .handle_key(key)
            .and_then(Self::settings_intent_action)
    }

    pub(crate) fn settings_workspace_mouse(&mut self, event: MouseEvent) -> Option<Action> {
        self.ui
            .settings_workspace
            .handle_mouse(event)
            .and_then(Self::settings_intent_action)
    }

    /// Save the complete live configuration, not the page's possibly stale snapshot.
    pub(crate) async fn save_settings(&mut self, draft: Option<(String, String)>) -> Result<()> {
        let result = if let Some((id, value)) = &draft {
            self.persist_setting(id, value)
        } else {
            self.config
                .try_write()
                .map_err(|_| eyre!("Configuration is busy; retry Save"))
                .and_then(|live| {
                    live.save()?;
                    Ok(live.clone())
                })
        };
        match result {
            Ok(config) => {
                if let Some((id, _)) = draft {
                    self.publish_setting(&config, &id);
                } else {
                    self.ui.settings_workspace.load(&config);
                }
                if self.ui.settings_workspace.prompt_read_error().is_some() {
                    return Ok(());
                }
                self.ui.settings_workspace.feedback = "Saved all current settings.".into();
                self.ui.settings_workspace.has_error = false;
                self.ui.show_toast("Settings saved".into());
                Ok(())
            }
            Err(error) => {
                self.ui.settings_workspace.fail(format!(
                    "Not saved: {error:#}. Draft retained; Ctrl+S retries, Esc cancels."
                ));
                Err(error)
            }
        }
    }

    /// Persist before publishing a configuration, while holding the live write lock.
    /// A failed save leaves both the old live value and the editable draft intact.
    fn persist_setting(&mut self, id: &str, value: &str) -> Result<AppConfig> {
        let mut live = self
            .config
            .try_write()
            .map_err(|_| eyre!("Configuration is busy; retry this change"))?;
        if let Some(layer) = PromptLayer::from_id(id) {
            let original = self
                .ui
                .settings_workspace
                .prompt_original(id)
                .map_err(|error| eyre!(error))?;
            layer.save(value, original)?;
            return Ok(live.clone());
        }
        let serialized = serde_json::to_value(&*live)?;
        let candidate = validate_edit(&serialized, id, value).map_err(|error| eyre!(error))?;
        if id == "default_chat" {
            let selection = &candidate.default_chat;
            if !crate::llm::local::is_local_provider(&selection.provider)
                && !crate::config::KeyStore::has_credential(&candidate, &selection.provider)?
            {
                return Err(eyre!(
                    "Connect this provider before saving a default chat selection"
                ));
            }
            let models = self.cached_models_for_provider(&selection.provider);
            let model = models.iter().find(|entry| entry.id == selection.model);
            let manual_model = self.ui.tabs.iter().any(|tab| {
                tab.manual_model_override
                    && tab.tab.provider == selection.provider
                    && tab.tab.model == selection.model
            });
            if model.is_none() && !models.is_empty() && !manual_model {
                return Err(eyre!("Model is not in this connection's catalog. Refresh Models or select it explicitly through Manual model before saving."));
            }
            let options = model
                .map(|model| Self::reasoning_options_for(&selection.provider, model))
                .unwrap_or_default();
            if let Some(effort) = &selection.effort {
                if !options.contains(effort) {
                    return Err(eyre!(
                        "Effort {effort:?} is not supported for this provider/model. Supported: {}. Use effort=auto for a model without reasoning metadata.",
                        if options.is_empty() { "none".into() } else { options.join(", ") },
                    ));
                }
            }
        }
        candidate.save()?;
        *live = candidate.clone();
        Ok(candidate)
    }

    fn publish_setting(&mut self, config: &AppConfig, id: &str) {
        self.ui.user_alignment = config.user_alignment;
        self.ui.ai_alignment = config.ai_alignment;
        self.ui.markdown_mode = config.markdown_mode;
        self.ui.show_selector = config.show_selector;
        self.ui.show_chat_scrollbar = config.show_chat_scrollbar;
        self.ui.collapse_thinking = config.collapse_thinking;
        self.ui.kitty_enhanced_text = config.kitty_enhanced_text;
        self.ui.kitty_heading_downscale = config.kitty_heading_downscale;
        self.ui.image_protocol = config.image_protocol.clone();
        self.ui.web_search_enabled = config.web_search.enabled;
        self.ui.show_memories = config.show_memories;
        self.ui.show_vault = config.show_vault;
        self.ui.tui_config = config.tui.clone();
        self.ui.keybinding_overrides = config.tui.keybinding_overrides.clone();
        if id == "theme" {
            crate::theme::set_active_theme(&config.theme);
        }
        if id == "tui.terminal_placement" {
            self.reflow_terminal_placement(config.tui.terminal_placement);
        }
        if id == "image_protocol" {
            for tab in &mut self.ui.tabs {
                tab.image_states.clear();
            }
        }
        if id.starts_with("tui.left_") || id.starts_with("tui.right_") {
            self.ui.panel_state = crate::tui::shell::PanelState::from_config(&config.tui);
            self.ui.sidebar_open = self.ui.panel_state.left != crate::tui::shell::PanelMode::Closed;
            self.ui.artifact_sidebar_open =
                self.ui.panel_state.right != crate::tui::shell::PanelMode::Closed;
        }
        if let Some(palette) = &mut self.ui.palette {
            palette.set_pinned(config.tui.pinned_commands.clone());
        }
        if id == "disabled_providers"
            || id == "disabled_models"
            || id.starts_with("local_inference.")
        {
            self.ui.disabled_providers = config.disabled_providers.iter().cloned().collect();
            self.ui.disabled_models = config.disabled_models.iter().cloned().collect();
            // Rebuild local entries without probing the server or running a process.
            self.ui
                .db_providers
                .retain(|(name, _, _, _, _)| !crate::llm::local::is_local_provider(name));
            if config.local_inference.enabled {
                self.ui
                    .db_providers
                    .push(crate::llm::local::local_provider_entry(
                        &config.local_inference,
                        None,
                    ));
            }
            self.refresh_visible_selectors();
        }
        if id == "saved_files_dir" || id.starts_with("show_") {
            self.refresh_artifact_sidebar_catalogs();
        }
        self.ui.artifact_sidebar_state.clear_hit_areas();
        self.ui.settings_workspace.applied(config, id);
    }

    pub(crate) async fn apply_settings_value(&mut self, id: &str, value: &str) -> Result<()> {
        match self.persist_setting(id, value) {
            Ok(config) => {
                self.publish_setting(&config, id);
                Ok(())
            }
            Err(error) => {
                self.ui.settings_workspace.fail(format!(
                    "Not applied: {error:#}. Draft retained; Enter retries, Esc cancels."
                ));
                // The dedicated page owns the error, rather than closing the page or losing a draft.
                self.ui.show_toast(format!("Setting not saved: {error}"));
                Ok(())
            }
        }
    }

    pub(crate) fn apply_theme_selection(&mut self, theme_name: &str) -> Result<()> {
        let key = crate::theme::canonical_theme_key(theme_name);
        let config = match self.persist_setting("theme", key) {
            Ok(config) => config,
            Err(error) => {
                self.ui.show_toast(format!("Theme not saved: {error}"));
                return Err(error);
            }
        };
        self.publish_setting(&config, "theme");
        Ok(())
    }

    pub(crate) async fn toggle_web_search(&mut self) -> Result<()> {
        let enabled = self
            .config
            .try_read()
            .map_err(|_| eyre!("Configuration is busy"))?
            .web_search
            .enabled;
        let config = self.persist_setting("web_search.enabled", &(!enabled).to_string())?;
        self.publish_setting(&config, "web_search.enabled");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prompt_draft(app: &mut TuiApp) -> (String, String) {
        use crossterm::event::{KeyCode, KeyModifiers};
        match app.settings_workspace_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)) {
            Some(Action::SaveSettings { draft: Some(draft) }) => draft,
            _ => panic!("expected an editable settings draft"),
        }
    }

    fn write_prompt(layer: PromptLayer, text: &str) {
        let original = layer.load_original().unwrap();
        layer.save(text, original.as_deref()).unwrap();
    }

    #[test]
    fn stale_prompt_drafts_preserve_external_changes_until_reopened() {
        for (original, external) in [
            (None, Some("")),
            (Some(""), None),
            (Some("original"), Some("external edit")),
        ] {
            crate::app::tests::with_test_app("settings-prompt-conflict", |app| {
                use crossterm::event::{KeyCode, KeyModifiers};
                let runtime = tokio::runtime::Runtime::new().unwrap();
                let layer = PromptLayer::Soul;
                if let Some(text) = original {
                    write_prompt(layer, text);
                }
                app.open_settings_page(Some(layer.filename()));
                app.settings_workspace_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                app.settings_workspace_key(KeyEvent::new(
                    KeyCode::Char('u'),
                    KeyModifiers::CONTROL,
                ));
                app.ui.settings_workspace.paste("retained local draft");
                let draft = prompt_draft(app);
                let path = dirs::config_dir()
                    .unwrap()
                    .join("tcui")
                    .join(layer.filename());
                if let Some(text) = external {
                    std::fs::write(&path, text).unwrap();
                } else {
                    std::fs::remove_file(&path).unwrap();
                }
                assert!(runtime
                    .block_on(app.save_settings(Some(draft.clone())))
                    .is_err());
                assert_eq!(layer.load_original().unwrap().as_deref(), external);
                assert_eq!(prompt_draft(app), draft);
                app.open_settings_page(None);
                assert!(runtime
                    .block_on(app.save_settings(Some(draft.clone())))
                    .is_err());
                assert_eq!(layer.load_original().unwrap().as_deref(), external);
                assert_eq!(prompt_draft(app), draft);

                app.settings_workspace_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
                app.settings_workspace_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                assert_eq!(
                    prompt_draft(app).1,
                    external.unwrap_or(layer.default_text())
                );
                app.settings_workspace_key(KeyEvent::new(
                    KeyCode::Char('u'),
                    KeyModifiers::CONTROL,
                ));
                app.ui.settings_workspace.paste("resolved local draft");
                let draft = prompt_draft(app);
                runtime.block_on(app.save_settings(Some(draft))).unwrap();
                assert_eq!(layer.load().unwrap(), "resolved local draft");
            });
        }
    }

    #[test]
    fn prompt_settings_save_reopen_and_apply_empty_without_normalization() {
        crate::app::tests::with_test_app("settings-prompt-round-trip", |app| {
            use crossterm::event::{KeyCode, KeyModifiers};
            let runtime = tokio::runtime::Runtime::new().unwrap();
            for layer in PromptLayer::ALL {
                app.open_settings_page(Some(layer.filename()));
                app.settings_workspace_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                assert_eq!(prompt_draft(app).1, layer.default_text());
                app.settings_workspace_key(KeyEvent::new(
                    KeyCode::Char('u'),
                    KeyModifiers::CONTROL,
                ));
                let text = "  first line\n\n\tsecond line  \r\n";
                app.ui.settings_workspace.paste(text);
                let draft = prompt_draft(app);
                assert_eq!(draft.1, text);
                runtime.block_on(app.save_settings(Some(draft))).unwrap();
                assert_eq!(layer.load().unwrap(), text);
                app.open_settings_page(Some(layer.filename()));
                app.settings_workspace_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                assert_eq!(prompt_draft(app).1, text);
                app.settings_workspace_key(KeyEvent::new(
                    KeyCode::Char('u'),
                    KeyModifiers::CONTROL,
                ));
                let Some(Action::ApplySetting { id, value }) = app
                    .settings_workspace_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL))
                else {
                    panic!("expected apply");
                };
                assert!(value.is_empty());
                runtime
                    .block_on(app.apply_settings_value(&id, &value))
                    .unwrap();
                assert_eq!(layer.load().unwrap(), "");
                app.open_settings_page(Some(layer.filename()));
                app.settings_workspace_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                assert_eq!(prompt_draft(app).1, "");
                app.settings_workspace_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
            }
        });
    }

    #[test]
    fn prompt_draft_survives_failed_atomic_save_and_settings_reload() {
        crate::app::tests::with_test_app("settings-prompt-save-failure", |app| {
            use crate::config::app_config::{inject_private_file_failure, TestFailurePoint};
            use crossterm::event::{KeyCode, KeyModifiers};
            let runtime = tokio::runtime::Runtime::new().unwrap();
            for point in [
                TestFailurePoint::Sync,
                TestFailurePoint::Rename,
                TestFailurePoint::ParentSync,
            ] {
                write_prompt(PromptLayer::Soul, "saved persona");
                app.open_settings_page(Some("SOUL.md"));
                app.settings_workspace_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                app.settings_workspace_key(KeyEvent::new(
                    KeyCode::Char('u'),
                    KeyModifiers::CONTROL,
                ));
                app.ui.settings_workspace.paste("  retained draft\n\n");
                let draft = prompt_draft(app);
                inject_private_file_failure(point);
                assert!(runtime
                    .block_on(app.save_settings(Some(draft.clone())))
                    .is_err());
                assert!(app.ui.settings_workspace.has_error);
                assert_eq!(PromptLayer::Soul.load().unwrap(), "saved persona");
                app.open_settings_page(None);
                assert_eq!(prompt_draft(app), draft);
                runtime.block_on(app.save_settings(Some(draft))).unwrap();
                assert_eq!(PromptLayer::Soul.load().unwrap(), "  retained draft\n\n");
            }
        });
    }

    #[test]
    fn saving_other_settings_never_rewrites_external_prompt_edits() {
        crate::app::tests::with_test_app("settings-prompt-live-files", |app| {
            use crossterm::event::{KeyCode, KeyModifiers};
            let runtime = tokio::runtime::Runtime::new().unwrap();
            write_prompt(PromptLayer::Agent, "original preferences");
            app.open_settings_page(None);
            write_prompt(PromptLayer::Agent, "external preferences\n");
            runtime.block_on(app.save_settings(None)).unwrap();
            assert_eq!(PromptLayer::Agent.load().unwrap(), "external preferences\n");
            app.persist_setting("collapse_thinking", "false").unwrap();
            write_prompt(PromptLayer::Agent, "newer external preferences\n");
            app.ui.settings_workspace.select_id("AGENTS.md");
            app.settings_workspace_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            assert_eq!(prompt_draft(app).1, "newer external preferences\n");
        });
    }

    #[test]
    fn prompt_read_failure_blocks_edit_and_submission_without_discarding_input() {
        crate::app::tests::with_test_app("settings-prompt-read-failure", |app| {
            use crossterm::event::{KeyCode, KeyModifiers};
            let path = dirs::config_dir().unwrap().join("tcui").join("SOUL.md");
            std::fs::create_dir_all(&path).unwrap();
            app.open_settings_page(Some("SOUL.md"));
            assert!(app.ui.settings_workspace.has_error);
            app.settings_workspace_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            assert!(app.ui.settings_workspace.has_error);
            assert!(matches!(
                app.settings_workspace_key(KeyEvent::new(
                    KeyCode::Char('s'),
                    KeyModifiers::CONTROL
                )),
                Some(Action::SaveSettings { draft: None })
            ));
            let runtime = tokio::runtime::Runtime::new().unwrap();
            assert!(runtime
                .block_on(app.send_message("preserved input\n".into()))
                .is_err());
            assert_eq!(
                app.ui.tabs[app.ui.active_tab].input_content,
                "preserved input\n"
            );
            assert!(app.requests.is_empty());
            assert!(app.ui.tabs[app.ui.active_tab].messages.is_empty());
            assert!(path.is_dir());
        });
    }

    #[test]
    fn shortcut_capture_saves_control_s_chords_instead_of_invoking_save() {
        use crossterm::event::{KeyCode, KeyModifiers};
        for (action_id, modifiers, binding) in [
            ("toggle_settings", KeyModifiers::CONTROL, "ctrl+s"),
            (
                "new_chat",
                KeyModifiers::CONTROL | KeyModifiers::ALT,
                "ctrl+alt+s",
            ),
        ] {
            crate::app::tests::with_test_app("settings-capture-save-chord", |app| {
                app.open_settings_page(Some(&format!("keybind:{action_id}")));
                app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                let action = app
                    .handle_key(KeyEvent::new(KeyCode::Char('s'), modifiers))
                    .expect("capture must submit the new binding");
                tokio::runtime::Runtime::new()
                    .expect("runtime")
                    .block_on(app.dispatch(action))
                    .expect("save captured shortcut");
                assert_eq!(
                    AppConfig::load()
                        .expect("saved shortcuts")
                        .tui
                        .keybinding_overrides
                        .get(action_id)
                        .map(String::as_str),
                    Some(binding)
                );
            });
        }
    }

    #[test]
    fn failed_shortcut_capture_retries_original_binding_with_control_s() {
        use crate::config::app_config::{inject_private_file_failure, TestFailurePoint};
        use crossterm::event::{KeyCode, KeyModifiers};
        crate::app::tests::with_test_app("settings-capture-save-retry", |app| {
            app.config
                .try_read()
                .expect("config")
                .save()
                .expect("initial save");
            app.open_settings_page(Some("keybind:new_chat"));
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            let action = app
                .handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::ALT))
                .expect("captured binding");
            let runtime = tokio::runtime::Runtime::new().expect("runtime");
            inject_private_file_failure(TestFailurePoint::Rename);
            runtime
                .block_on(app.dispatch(action))
                .expect("report save failure");
            assert!(app.ui.settings_workspace.has_error);
            assert!(!AppConfig::load()
                .expect("saved config")
                .tui
                .keybinding_overrides
                .contains_key("new_chat"));
            let retry = app
                .handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL))
                .expect("retry captured binding");
            runtime.block_on(app.dispatch(retry)).expect("retry save");
            assert_eq!(
                AppConfig::load()
                    .expect("saved shortcut")
                    .tui
                    .keybinding_overrides
                    .get("new_chat")
                    .map(String::as_str),
                Some("alt+n")
            );
            assert!(!app.ui.settings_workspace.has_error);
        });
    }

    #[test]
    fn retained_provider_drafts_validate_against_live_config_on_enter_and_save() {
        for save in [false, true] {
            crate::app::tests::with_test_app("settings-live-provider-draft", |app| {
                use crossterm::event::{KeyCode, KeyModifiers};
                app.open_settings_page(Some("default_chat"));
                app.settings_workspace_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                app.settings_workspace_key(KeyEvent::new(
                    KeyCode::Char('u'),
                    KeyModifiers::CONTROL,
                ));
                app.ui
                    .settings_workspace
                    .paste("provider=Added while draft open\nmodel=manual-model\neffort=auto");
                app.config.try_write().expect("live").providers.push(
                    crate::config::ProviderConfig {
                        name: "Added while draft open".into(),
                        ..Default::default()
                    },
                );
                crate::config::KeyStore::set(
                    &app.config.try_read().expect("config"),
                    "Added while draft open",
                    "fixture-api-key",
                )
                .expect("connect provider");
                let key = if save {
                    KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)
                } else {
                    KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)
                };
                let action = app
                    .settings_workspace_key(key)
                    .expect("stale page must submit draft");
                let runtime = tokio::runtime::Runtime::new().expect("runtime");
                match action {
                    Action::SaveSettings { draft } => runtime
                        .block_on(app.save_settings(draft))
                        .expect("save live draft"),
                    Action::ApplySetting { id, value } => runtime
                        .block_on(app.apply_settings_value(&id, &value))
                        .expect("apply live draft"),
                    _ => panic!("setting intent"),
                }
                assert_eq!(
                    AppConfig::load().expect("reload").default_chat.provider,
                    "Added while draft open"
                );
                assert!(!app.ui.settings_workspace.has_error);
            });
        }
    }

    #[test]
    fn settings_reserved_navigation_preserves_editor_draft_and_blocks_custom_actions() {
        crate::app::tests::with_test_app("settings-navigation-draft", |app| {
            use crossterm::event::{KeyCode, KeyModifiers};
            app.open_settings_page(Some("local_inference.port"));
            app.settings_workspace_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            app.settings_workspace_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
            app.ui.settings_workspace.paste("8123");
            app.ui
                .keybinding_overrides
                .insert("new_chat".into(), "alt+n".into());
            assert!(app
                .handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::ALT))
                .is_none());
            assert!(matches!(
                app.handle_key(KeyEvent::new(KeyCode::Char(','), KeyModifiers::CONTROL)),
                Some(Action::ToggleSettings)
            ));
            assert!(app
                .handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL))
                .is_none());
            assert_ne!(
                app.ui.app_tabs.active_view(),
                crate::ui::app_tabs::AppView::Settings
            );
            app.open_settings_page(None);
            assert!(
                matches!(app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)),
                Some(Action::SaveSettings { draft: Some((id, value)) }) if id == "local_inference.port" && value == "8123")
            );
        });
    }

    #[test]
    fn rejected_current_draft_reports_failure_and_keeps_text_for_retry() {
        crate::app::tests::with_test_app("settings-invalid-current-draft", |app| {
            use crossterm::event::{KeyCode, KeyModifiers};
            app.open_settings_page(Some("local_inference.port"));
            app.settings_workspace_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            app.settings_workspace_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
            app.ui.settings_workspace.paste("not-a-port");
            let Action::SaveSettings { draft } = app
                .settings_workspace_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL))
                .expect("submit draft")
            else {
                panic!("Save");
            };
            assert!(tokio::runtime::Runtime::new()
                .expect("runtime")
                .block_on(app.save_settings(draft))
                .is_err());
            assert!(app.ui.settings_workspace.has_error);
            assert!(
                matches!(app.settings_workspace_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)),
                Some(Action::SaveSettings { draft: Some((_, value)) }) if value == "not-a-port")
            );
        });
    }

    #[test]
    fn explicit_save_commits_draft_and_preserves_complete_live_configuration() {
        crate::app::tests::with_test_app("settings-explicit-save", |app| {
            app.open_settings_page(Some("local_inference.port"));
            {
                let mut live = app.config.try_write().expect("live");
                live.commands.blacklist = vec!["blocked --literal".into()];
                live.disabled_models = vec!["saved:model".into()];
            }
            app.settings_workspace_key(KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::NONE,
            ));
            app.settings_workspace_key(KeyEvent::new(
                crossterm::event::KeyCode::Char('u'),
                crossterm::event::KeyModifiers::CONTROL,
            ));
            app.ui.settings_workspace.paste("8123");
            let action = app
                .settings_workspace_key(KeyEvent::new(
                    crossterm::event::KeyCode::Char('s'),
                    crossterm::event::KeyModifiers::CONTROL,
                ))
                .expect("Save");
            let Action::SaveSettings { draft } = action else {
                panic!("Save action");
            };
            tokio::runtime::Runtime::new()
                .expect("runtime")
                .block_on(app.save_settings(draft))
                .expect("saved");
            let saved = AppConfig::load().expect("reload");
            assert_eq!(saved.local_inference.port, 8123);
            assert_eq!(saved.commands.blacklist, vec!["blocked --literal"]);
            assert_eq!(saved.disabled_models, vec!["saved:model"]);
            assert!(!app.ui.settings_workspace.has_error);
            app.config
                .try_write()
                .expect("live")
                .disabled_models
                .push("another:model".into());
            tokio::runtime::Runtime::new()
                .expect("runtime")
                .block_on(app.save_settings(None))
                .expect("save live");
            assert_eq!(AppConfig::load().expect("reload").disabled_models.len(), 2);
        });
    }

    #[test]
    fn explicit_save_failure_keeps_draft_and_reports_error() {
        crate::app::tests::with_test_app("settings-explicit-save-failure", |app| {
            app.open_settings_page(Some("local_inference.port"));
            app.settings_workspace_key(KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::NONE,
            ));
            app.settings_workspace_key(KeyEvent::new(
                crossterm::event::KeyCode::Char('u'),
                crossterm::event::KeyModifiers::CONTROL,
            ));
            app.ui.settings_workspace.paste("8123");
            let shared = app.config.clone();
            let guard = shared.try_write().expect("busy");
            let runtime = tokio::runtime::Runtime::new().expect("runtime");
            assert!(runtime
                .block_on(app.save_settings(Some(("local_inference.port".into(), "8123".into()))))
                .is_err());
            assert!(app.ui.settings_workspace.has_error);
            drop(guard);
            let action = app
                .settings_workspace_key(KeyEvent::new(
                    crossterm::event::KeyCode::Char('s'),
                    crossterm::event::KeyModifiers::CONTROL,
                ))
                .expect("retry");
            assert!(
                matches!(action, Action::SaveSettings { draft: Some((_, value)) } if value == "8123")
            );
        });
    }

    #[test]
    fn validated_changes_update_live_values_and_disk() {
        crate::app::tests::with_test_app("settings-live-persist", |app| {
            let config = app
                .persist_setting("tui.left_thin_width", "31")
                .expect("save");
            app.publish_setting(&config, "tui.left_thin_width");
            assert_eq!(app.ui.panel_state.left_thin_width, 31);
            assert_eq!(app.ui.tui_config.left_thin_width, 31);
            assert_eq!(AppConfig::load().expect("reload").tui.left_thin_width, 31);
            let config = app
                .persist_setting("collapse_thinking", "false")
                .expect("save");
            app.publish_setting(&config, "collapse_thinking");
            assert!(!app.ui.collapse_thinking);
        });
    }

    #[test]
    fn saved_defaults_apply_to_new_conversations_without_changing_existing_selection() {
        crate::app::tests::with_test_app("settings-new-chat-defaults", |app| {
            let previous = crate::config::ChatSelection {
                provider: app.ui.tabs[0].tab.provider.clone(),
                model: app.ui.tabs[0].tab.model.clone(),
                effort: app.ui.tabs[0].tab.reasoning_effort.clone(),
            };
            let previous_id = app.ui.tabs[0].active_conversation;
            let model = "default-reasoner";
            app.storage
                .save_models(
                    "Ollama",
                    &[(
                        model.into(),
                        None,
                        None,
                        None,
                        Some("high".into()),
                        vec!["low".into(), "medium".into(), "high".into()],
                    )],
                )
                .expect("catalog");
            let input = format!(r#"{{"provider":"Ollama","model":"{model}","effort":"high"}}"#);
            let config = app
                .persist_setting("default_chat", &input)
                .expect("save combination");
            app.publish_setting(&config, "default_chat");
            assert_eq!(app.ui.tabs[0].tab.provider, previous.provider);
            assert_eq!(app.ui.tabs[0].tab.model, previous.model);
            app.new_conversation(0).expect("new conversation");
            assert_eq!(app.ui.tabs[0].tab.provider, "Ollama");
            assert_eq!(app.ui.tabs[0].tab.model, model);
            assert_eq!(app.ui.tabs[0].tab.reasoning_effort.as_deref(), Some("high"));
            let new_id = app.ui.tabs[0].active_conversation;
            app.load_conversation_into_tab(0, previous_id)
                .expect("restore existing");
            assert_eq!(app.ui.tabs[0].tab.provider, previous.provider);
            assert_eq!(app.ui.tabs[0].tab.model, previous.model);
            assert_eq!(app.ui.tabs[0].tab.reasoning_effort, previous.effort);
            app.load_conversation_into_tab(0, new_id)
                .expect("restore new");
            assert_eq!(app.ui.tabs[0].tab.reasoning_effort.as_deref(), Some("high"));
            assert_eq!(
                AppConfig::load().expect("saved").default_chat,
                config.default_chat
            );
        });
    }

    fn reasoner_config(effort: Option<&str>) -> AppConfig {
        AppConfig {
            default_chat: crate::config::ChatSelection {
                provider: "Reasoner".into(),
                model: "preferred-model".into(),
                effort: effort.map(str::to_string),
            },
            providers: vec![crate::config::ProviderConfig {
                name: "Reasoner".into(),
                endpoint: "http://127.0.0.1:9/v1".into(),
                ..Default::default()
            }],
            ..AppConfig::default()
        }
    }

    fn reasoner_catalog(app: &TuiApp) {
        app.storage
            .save_models(
                "Reasoner",
                &[
                    (
                        "a-unrelated-model".into(),
                        None,
                        None,
                        None,
                        None,
                        Vec::new(),
                    ),
                    (
                        "preferred-model".into(),
                        None,
                        None,
                        None,
                        Some("high".into()),
                        vec!["low".into(), "medium".into(), "high".into()],
                    ),
                ],
            )
            .expect("model metadata");
    }

    #[test]
    fn startup_and_new_chat_keep_exact_selection_through_delayed_catalog_and_restart() {
        for effort in [None, Some("high")] {
            crate::app::tests::with_test_app_config(
                "default-chat-delayed-catalog",
                reasoner_config(effort),
                |app| {
                    assert_eq!(app.ui.tabs[0].tab.provider, "Reasoner");
                    assert_eq!(app.ui.tabs[0].tab.model, "preferred-model");
                    assert_eq!(app.ui.tabs[0].tab.reasoning_effort.as_deref(), effort);
                    reasoner_catalog(app);
                    let models = app.cached_models_for_provider("Reasoner");
                    app.apply_refreshed_provider_models("Reasoner", &models);
                    let expected = effort.unwrap_or("medium");
                    assert_eq!(app.ui.tabs[0].tab.model, "preferred-model");
                    assert_eq!(
                        app.ui.tabs[0].tab.reasoning_effort.as_deref(),
                        Some(expected)
                    );
                    app.config.try_read().expect("config").save().expect("save");
                    let existing_id = app.ui.tabs[0].active_conversation;
                    // Existing chats retain their own effort even after the saved default changes.
                    let updated = app
                        .persist_setting(
                            "default_chat",
                            r#"{"provider":"Reasoner","model":"preferred-model","effort":"low"}"#,
                        )
                        .expect("save next-chat default");
                    let mut restarted = TuiApp::new(
                        crate::storage::Storage::new().expect("reopen storage"),
                        std::sync::Arc::new(tokio::sync::RwLock::new(
                            AppConfig::load().expect("reload"),
                        )),
                        std::sync::Arc::new(crate::llm::LlmClient::new()),
                    );
                    assert_eq!(restarted.ui.tabs[0].active_conversation, existing_id);
                    assert_eq!(restarted.ui.tabs[0].tab.model, "preferred-model");
                    assert_eq!(
                        restarted.ui.tabs[0].tab.reasoning_effort.as_deref(),
                        Some(expected)
                    );
                    restarted.new_conversation(0).expect("new chat");
                    assert_eq!(
                        restarted.ui.tabs[0].tab.provider,
                        updated.default_chat.provider
                    );
                    assert_eq!(restarted.ui.tabs[0].tab.model, updated.default_chat.model);
                    assert_eq!(
                        restarted.ui.tabs[0].tab.reasoning_effort.as_deref(),
                        Some("low")
                    );
                    restarted
                        .load_conversation_into_tab(0, existing_id)
                        .expect("restore old chat");
                    assert_eq!(
                        restarted.ui.tabs[0].tab.reasoning_effort.as_deref(),
                        Some(expected)
                    );
                },
            );
        }
    }

    #[test]
    fn invalid_default_combinations_and_atomic_save_failure_preserve_live_and_saved_state() {
        use crate::config::app_config::{inject_private_file_failure, TestFailurePoint};
        crate::app::tests::with_test_app_config(
            "default-chat-transaction",
            reasoner_config(Some("high")),
            |app| {
                reasoner_catalog(app);
                let original = app.config.try_read().expect("config").clone();
                original.save().expect("initial save");
                let chat_id = app.ui.tabs[0].active_conversation;
                let chat_before =
                    serde_json::to_value(app.storage.read_chat_document(chat_id).expect("chat"))
                        .expect("serialize");
                for input in [
                    r#"{"provider":"Missing","model":"preferred-model","effort":"medium"}"#,
                    r#"{"provider":"Reasoner","model":"another-providers-model","effort":"medium"}"#,
                    r#"{"provider":"Reasoner","model":"preferred-model","effort":"xhigh"}"#,
                    r#"{"provider":"Reasoner","model":"a-unrelated-model","effort":"medium"}"#,
                ] {
                    assert!(app.persist_setting("default_chat", input).is_err());
                    assert_eq!(
                        app.config.try_read().expect("live").default_chat,
                        original.default_chat
                    );
                    assert_eq!(
                        AppConfig::load().expect("saved").default_chat,
                        original.default_chat
                    );
                }
                inject_private_file_failure(TestFailurePoint::Rename);
                assert!(app
                    .persist_setting(
                        "default_chat",
                        r#"{"provider":"Reasoner","model":"preferred-model","effort":"low"}"#
                    )
                    .is_err());
                assert_eq!(
                    app.config.try_read().expect("live").default_chat,
                    original.default_chat
                );
                assert_eq!(
                    AppConfig::load().expect("saved").default_chat,
                    original.default_chat
                );
                assert_eq!(
                    serde_json::to_value(app.storage.read_chat_document(chat_id).expect("chat"))
                        .expect("serialize"),
                    chat_before
                );
            },
        );
    }

    #[test]
    fn chat_selection_updates_and_json_exports_preserve_the_complete_document() {
        crate::app::tests::with_test_app("chat-selection-document-integrity", |app| {
            let id = app.ui.tabs[0].active_conversation;
            app.storage
                .save_message(&crate::app::Message::new(
                    id,
                    "user".into(),
                    "retained history".into(),
                ))
                .expect("save message");
            app.storage
                .update_conversation_title(id, "Retained title")
                .expect("title");
            app.storage.set_conversation_pinned(id, true).expect("pin");
            let before = app.storage.read_chat_document(id).expect("before");
            let selection = crate::config::ChatSelection {
                provider: "Ollama".into(),
                model: "manual-model".into(),
                effort: None,
            };
            app.storage
                .set_conversation_selection(id, &selection)
                .expect("selection");
            let reopened = crate::storage::Storage::new().expect("reopen storage");
            let after = reopened.read_chat_document(id).expect("after");
            assert_eq!(after.selection.as_ref(), Some(&selection));
            let mut expected = serde_json::to_value(before).expect("before JSON");
            expected["selection"] = serde_json::to_value(&selection).expect("selection JSON");
            expected["updated_at_ms"] = after.updated_at_ms.into();
            assert_eq!(serde_json::to_value(&after).expect("after JSON"), expected);
            let exported =
                crate::export::render_chat_document(&after, crate::export::OutputFormat::Json)
                    .expect("JSON export");
            let mut exported: serde_json::Value =
                serde_json::from_str(&exported).expect("export document");
            assert_eq!(exported, expected);
            exported
                .as_object_mut()
                .expect("document")
                .remove("selection");
            let legacy: crate::storage::chat_store::ChatDocument = serde_json::from_value(exported)
                .expect("legacy chat without selection remains readable");
            assert!(legacy.selection.is_none());
            let unchanged = serde_json::to_value(&after).expect("unchanged");
            app.storage
                .set_conversation_selection(id, &selection)
                .expect("same selection");
            assert_eq!(
                serde_json::to_value(app.storage.read_chat_document(id).expect("chat"))
                    .expect("JSON"),
                unchanged
            );
        });
    }

    #[test]
    fn persistence_failure_does_not_publish_or_overwrite_saved_config() {
        crate::app::tests::with_test_app("settings-save-failure", |app| {
            app.config
                .try_read()
                .expect("config")
                .save()
                .expect("initial save");
            let original_home = std::env::var_os("XDG_CONFIG_HOME").expect("config home");
            let blocked = std::path::PathBuf::from(&original_home).join("blocked");
            std::fs::write(&blocked, b"not a directory").expect("failure fixture");
            std::env::set_var("XDG_CONFIG_HOME", &blocked);
            let result = app.persist_setting("theme", "nord");
            std::env::set_var("XDG_CONFIG_HOME", original_home);
            assert!(result.is_err());
            assert_eq!(app.config.try_read().expect("live config").theme, "system");
            assert_eq!(AppConfig::load().expect("saved config").theme, "system");
        });
    }

    #[test]
    fn busy_configuration_never_falls_back_to_defaults() {
        crate::app::tests::with_test_app("settings-lock-conflict", |app| {
            let shared = app.config.clone();
            let _guard = shared.try_write().expect("lock fixture");
            assert!(app.persist_setting("theme", "nord").is_err());
        });
    }
}
