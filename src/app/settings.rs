use color_eyre::eyre::{eyre, Result};
use crossterm::event::{KeyEvent, MouseEvent};

use super::{Action, TuiApp};
use crate::{
    config::AppConfig,
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

    /// Persist before publishing a configuration, while holding the live write lock.
    /// A failed save leaves both the old live value and the editable draft intact.
    fn persist_setting(&mut self, id: &str, value: &str) -> Result<AppConfig> {
        let mut live = self
            .config
            .try_write()
            .map_err(|_| eyre!("Configuration is busy; retry this change"))?;
        let serialized = serde_json::to_value(&*live)?;
        let mut candidate = validate_edit(&serialized, id, value).map_err(|error| eyre!(error))?;
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
        candidate.vault_path = live.vault_path.clone();
        candidate.artifact_save_dir = candidate.saved_files_dir.clone();
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
        self.ui.show_saved_files = config.show_saved_files;
        self.ui.show_memories = config.show_memories;
        self.ui.show_markdown_vault = config.show_markdown_vault;
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
                    "Not applied: {error}. Draft retained; Enter retries, Esc cancels."
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
