use super::TuiApp;

impl TuiApp {
    pub(crate) fn sidebar_settings_panel(&self) -> crate::tui::settings_panel::SettingsPanelState {
        crate::tui::settings_panel::SettingsPanelState::new().with_sidebar_visibility(
            self.ui.show_saved_files,
            self.ui.show_memories,
            self.ui.show_markdown_vault,
        )
    }

    pub(crate) fn toggle_sidebar_visibility(&mut self, id: &str) -> color_eyre::Result<()> {
        let mut config = self
            .config
            .try_read()
            .map(|config| config.clone())
            .unwrap_or_default();
        match id {
            "show_saved_files" => config.show_saved_files = !config.show_saved_files,
            "show_memories" => config.show_memories = !config.show_memories,
            "show_markdown_vault" => {
                config.show_markdown_vault = !config.show_markdown_vault;
            }
            _ => color_eyre::eyre::bail!("unknown sidebar visibility setting: {id}"),
        }
        config.save()?;
        if let Ok(mut live_config) = self.config.try_write() {
            *live_config = config.clone();
        }
        self.ui.show_saved_files = config.show_saved_files;
        self.ui.show_memories = config.show_memories;
        self.ui.show_markdown_vault = config.show_markdown_vault;
        self.ui.artifact_sidebar_state.clear_hit_areas();
        Ok(())
    }

    pub(crate) fn apply_theme_selection(&mut self, theme_name: &str) -> color_eyre::Result<()> {
        let key = crate::theme::canonical_theme_key(theme_name).to_string();
        let label = crate::theme::theme_label(&key);
        crate::theme::set_active_theme(&key);

        let mut config = self
            .config
            .try_read()
            .map(|config| config.clone())
            .unwrap_or_default();
        config.theme = key.clone();
        config.save()?;
        if let Ok(mut live_config) = self.config.try_write() {
            *live_config = config;
        }
        self.ui.connection_message = Some(format!("Theme: {label}"));
        Ok(())
    }

    pub(crate) fn apply_toast_position_selection(
        &mut self,
        position: crate::config::ToastPosition,
    ) -> color_eyre::Result<()> {
        let mut config = self
            .config
            .try_read()
            .map(|config| config.clone())
            .unwrap_or_default();
        config.tui.toast_position = position;
        config.save()?;
        if let Ok(mut live_config) = self.config.try_write() {
            *live_config = config.clone();
        }
        self.ui.tui_config.toast_position = position;
        self.ui.show_toast(format!("Toast position: {position:?}"));
        Ok(())
    }

    pub(crate) async fn toggle_web_search(&mut self) -> color_eyre::Result<()> {
        let mut config = self
            .config
            .try_read()
            .map(|config| config.clone())
            .unwrap_or_default();
        config.web_search.enabled = !config.web_search.enabled;
        config.save()?;
        if let Ok(mut live_config) = self.config.try_write() {
            *live_config = config.clone();
        }
        self.ui.web_search_enabled = config.web_search.enabled;
        self.ui.connection_status = crate::ui::status_bar::ConnectionStatus::CloudConnected;
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
