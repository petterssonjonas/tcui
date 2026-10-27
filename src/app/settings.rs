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
        let selection = self.config.try_read().ok().and_then(|config| {
            let provider = self
                .ui
                .visible_providers
                .iter()
                .find(|(name, _, _, _, _)| name.eq_ignore_ascii_case(&config.default_provider))?;
            let model = if crate::llm::local::is_local_provider(&provider.0)
                && config.default_model.is_empty()
            {
                config.local_inference.selected_model.clone()
            } else {
                config.default_model.clone()
            };
            Some((provider.0.clone(), model))
        });
        if let (Some((provider, model)), Some(tab)) = (selection, self.ui.tabs.get_mut(tab_index)) {
            tab.tab.provider = provider;
            tab.tab.model = model;
            tab.tab.endpoint = None;
            tab.tab.reasoning_effort = None;
            tab.manual_model_override = false;
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
        if id == "default_model" && !candidate.default_model.is_empty() {
            let provider = candidate
                .provider_config(&candidate.default_provider)
                .map_or(candidate.default_provider.as_str(), |provider| {
                    provider.name.as_str()
                });
            let models = self.storage.get_models(provider)?;
            if !models.is_empty()
                && !models
                    .iter()
                    .any(|entry| entry.0 == candidate.default_model)
            {
                return Err(eyre!("Model is not in the saved connection's catalog. Refresh Models and choose an available model."));
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
        self.ui.connection_message = Some(format!(
            "Web {}",
            if config.web_search.enabled {
                "on"
            } else {
                "off"
            }
        ));
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
                app.open_settings_page(Some("default_provider"));
                app.settings_workspace_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                app.settings_workspace_key(KeyEvent::new(
                    KeyCode::Char('u'),
                    KeyModifiers::CONTROL,
                ));
                app.ui.settings_workspace.paste("Added while draft open");
                app.config.try_write().expect("live").providers.push(
                    crate::config::ProviderConfig {
                        name: "Added while draft open".into(),
                        ..Default::default()
                    },
                );
                let key = if save {
                    KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)
                } else {
                    KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
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
                    AppConfig::load().expect("reload").default_provider,
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
            let previous = app.ui.tabs[0].tab.provider.clone();
            let config = app
                .persist_setting("default_provider", "Ollama")
                .expect("saved connection");
            app.publish_setting(&config, "default_provider");
            let model = app
                .storage
                .get_models("Ollama")
                .expect("saved model catalog")
                .first()
                .map_or_else(|| "manual-model".to_string(), |entry| entry.0.clone());
            let config = app
                .persist_setting("default_model", &model)
                .expect("model default");
            app.publish_setting(&config, "default_model");
            assert_eq!(app.ui.tabs[0].tab.provider, previous);
            app.new_conversation(0).expect("new conversation");
            assert_eq!(app.ui.tabs[0].tab.provider, "Ollama");
            assert_eq!(app.ui.tabs[0].tab.model, model);
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
