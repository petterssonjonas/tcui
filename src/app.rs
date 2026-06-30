#![allow(dead_code)]
use futures::StreamExt;
use ratatui::layout::Rect;
use std::collections::HashSet;
use std::io::IsTerminal;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub mod action;
pub mod generated_file;
pub mod message;
pub mod tab;

pub use action::Action;
pub use generated_file::GeneratedFile;
pub use message::Message;
pub use tab::Tab;

use crate::{config::AppConfig, llm::LlmClient, obsidian::Vault, storage::Storage};

pub struct TuiApp {
    pub storage: Arc<Storage>,
    pub config: Arc<tokio::sync::RwLock<AppConfig>>,
    pub key_store: Arc<crate::config::KeyStore>,
    pub ui: crate::ui::UI,
    pub llm: Arc<LlmClient>,
    pub vault: Option<Arc<Vault>>,
    pub action_tx: tokio::sync::mpsc::UnboundedSender<Action>,
    pub action_rx: tokio::sync::mpsc::UnboundedReceiver<Action>,
    pub system_prompt: String,
    pub ctrl_c_count: u32,
    pub last_ctrl_c: Option<std::time::Instant>,
    pub terminal_has_focus: Arc<AtomicBool>,
}

impl TuiApp {
    pub fn new(
        storage: Arc<Storage>,
        config: Arc<tokio::sync::RwLock<AppConfig>>,
        llm: Arc<LlmClient>,
        vault: Option<Arc<Vault>>,
    ) -> Self {
        let key_store = Arc::new(crate::config::KeyStore::new());
        let mut ui = crate::ui::UI::new();
        let (action_tx, action_rx) = tokio::sync::mpsc::unbounded_channel();
        let system_prompt = Self::load_system_prompt();
        let config_snapshot = config.try_read().map(|cfg| cfg.clone()).unwrap_or_default();
        crate::theme::set_active_theme(&config_snapshot.theme);

        ui.user_alignment = config_snapshot.user_alignment;
        ui.ai_alignment = config_snapshot.ai_alignment;
        ui.markdown_mode = config_snapshot.markdown_mode;
        ui.show_selector = config_snapshot.show_selector;
        ui.show_chat_scrollbar = config_snapshot.show_chat_scrollbar;
        ui.collapse_thinking = config_snapshot.collapse_thinking;
        ui.kitty_enhanced_text = config_snapshot.kitty_enhanced_text;
        ui.kitty_text_max_scale = config_snapshot.kitty_text_max_scale.clamp(1, 7);
        ui.image_protocol = config_snapshot.image_protocol.clone();
        ui.web_search_enabled = config_snapshot.web_search.enabled;
        ui.db_providers = Self::provider_entries_with_local(&config_snapshot, None);
        ui.disabled_providers = config_snapshot.disabled_providers.iter().cloned().collect();
        ui.disabled_models = config_snapshot.disabled_models.iter().cloned().collect();
        let _ = storage.sync_providers(&config_snapshot.providers);
        ui.visible_providers =
            Self::filter_visible_providers(&ui.db_providers, &ui.disabled_providers);

        let resolve_provider_name = |candidate: &str| {
            ui.db_providers
                .iter()
                .find(|(name, _, _, _, _)| name.eq_ignore_ascii_case(candidate.trim()))
                .map(|(name, _, _, _, _)| name.clone())
                .unwrap_or_else(|| crate::llm::auth::canonical_provider_name(candidate))
        };
        let default_provider = resolve_provider_name(&config_snapshot.default_provider);
        let default_model = if default_provider == crate::config::LOCAL_PROVIDER_NAME
            && config_snapshot.default_model.trim().is_empty()
        {
            config_snapshot.local_inference.selected_model.clone()
        } else {
            config_snapshot.default_model.clone()
        };

        if let Some(active_tab) = ui.tabs.get_mut(ui.active_tab) {
            active_tab.tab.provider = default_provider.clone();
            active_tab.tab.model = default_model.clone();
        }

        if std::io::stdin().is_terminal() {
            tokio::task::spawn_blocking(move || {
                let Ok(runtime_storage) = crate::storage::Storage::new() else {
                    return;
                };
                let rt = tokio::runtime::Handle::current();
                rt.block_on(async {
                    crate::llm::model_fetcher::refresh_all_models(&runtime_storage).await;
                });
            });
        }

        let app = Self {
            storage,
            config,
            key_store,
            ui,
            llm,
            vault,
            action_tx,
            action_rx,
            system_prompt,
            ctrl_c_count: 0,
            last_ctrl_c: None,
            terminal_has_focus: Arc::new(AtomicBool::new(true)),
        };
        let mut app = app;
        app.refresh_visible_selectors();
        app.refresh_vault_artifacts();
        app.sync_message_media(app.ui.active_tab);
        app.queue_connection_check_for_active_tab();
        app
    }

    fn load_system_prompt() -> String {
        let paths = [
            std::path::PathBuf::from("assets/TCUI.md"),
            std::path::PathBuf::from("/home/jp/Code/TermChatUI/assets/TCUI.md"),
        ];
        for path in &paths {
            if let Ok(content) = std::fs::read_to_string(path) {
                return content;
            }
        }
        // Default prompt if file not found
        "You are TCUI, a helpful terminal assistant.".to_string()
    }

    fn provider_entries_with_local(
        config: &AppConfig,
        detected: Option<crate::config::LocalServerType>,
    ) -> Vec<(String, String, String, String, String)> {
        let mut providers = config.provider_entries();
        if config.local_inference.enabled {
            providers.push(crate::llm::local::local_provider_entry(
                &config.local_inference,
                detected,
            ));
        }
        providers
    }

    fn filter_visible_providers(
        providers: &[(String, String, String, String, String)],
        disabled: &HashSet<String>,
    ) -> Vec<(String, String, String, String, String)> {
        providers
            .iter()
            .filter(|(name, _, _, _, _)| !disabled.contains(name))
            .cloned()
            .collect()
    }

    fn model_disable_key(provider: &str, model: &str) -> String {
        format!("{provider}:{model}")
    }

    fn visible_models_for_provider(
        &self,
        provider: &str,
    ) -> Vec<crate::ui::settings_tab::ModelInfo> {
        let mut models = self.cached_models_for_provider(provider);
        models.retain(|model| {
            !self
                .ui
                .disabled_models
                .contains(&Self::model_disable_key(provider, &model.id))
        });
        models
    }

    fn reasoning_options_for(provider: &str, model: &str) -> Vec<String> {
        let provider = crate::llm::auth::canonical_provider_name(provider);
        if model.starts_with("gpt-5")
            && matches!(
                provider.as_str(),
                "Codex" | "OpenAI" | "OpenRouter" | "Groq" | "Mistral"
            )
        {
            return ["none", "low", "medium", "high", "xhigh"]
                .into_iter()
                .map(str::to_string)
                .collect();
        }
        Vec::new()
    }

    fn refresh_visible_selectors(&mut self) {
        self.ui.visible_providers =
            Self::filter_visible_providers(&self.ui.db_providers, &self.ui.disabled_providers);
        let Some(tab) = self.ui.tabs.get(self.ui.active_tab) else {
            self.ui.current_models.clear();
            self.ui.current_reasoning_options.clear();
            return;
        };
        let mut provider = tab.tab.provider.clone();
        let mut model = tab.tab.model.clone();
        let mut reasoning_effort = tab.tab.reasoning_effort.clone();
        if !self
            .ui
            .visible_providers
            .iter()
            .any(|(name, _, _, _, _)| name == &provider)
        {
            if let Some((next_provider, _, _, _, _)) = self.ui.visible_providers.first() {
                provider = next_provider.clone();
                model.clear();
            }
        }
        self.ui.current_models = self.visible_models_for_provider(&provider);
        if model.trim().is_empty()
            || (!self.ui.current_models.is_empty()
                && !self.ui.current_models.iter().any(|entry| entry.id == model))
        {
            if let Some(first_model) = self.ui.current_models.first() {
                model = first_model.id.clone();
            }
        }
        self.ui.current_reasoning_options = Self::reasoning_options_for(&provider, &model);
        if self.ui.current_reasoning_options.is_empty() {
            reasoning_effort = None;
        } else if reasoning_effort.as_ref().is_none_or(|value| {
            !self
                .ui
                .current_reasoning_options
                .iter()
                .any(|option| option == value)
        }) {
            reasoning_effort = Some("medium".to_string());
        }
        if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
            tab.tab.provider = provider;
            tab.tab.model = model;
            tab.tab.reasoning_effort = reasoning_effort;
        }
    }

    fn set_connection_state(
        &mut self,
        status: crate::ui::status_bar::ConnectionStatus,
        message: Option<String>,
    ) {
        self.ui.connection_status = status;
        self.ui.connection_message = message;
    }

    fn queue_connection_check_for_active_tab(&self) {
        if tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        let Some(tab) = self.ui.tabs.get(self.ui.active_tab) else {
            return;
        };
        let config_snapshot = self
            .config
            .try_read()
            .map(|config| config.clone())
            .unwrap_or_default();
        let provider = tab.tab.provider.clone();
        let action_tx = self.action_tx.clone();
        let cloud_api_key =
            config_snapshot
                .provider_config(&provider)
                .and_then(|provider_config| {
                    crate::llm::auth::read_provider_api_key(
                        &provider_config.name,
                        &provider_config.env_var,
                        &self.storage,
                    )
                });
        tokio::spawn(async move {
            let _ = action_tx.send(Action::SetConnectionState(
                crate::ui::status_bar::ConnectionStatus::Checking,
                Some("Checking connection...".to_string()),
            ));
            if crate::llm::local::is_local_provider(&provider) {
                match crate::llm::local::probe(&config_snapshot.local_inference).await {
                    Ok(probe) => {
                        let models: Vec<(String, Option<f64>, Option<f64>, Option<u32>)> = probe
                            .models
                            .iter()
                            .map(|model| {
                                (
                                    model.id.clone(),
                                    model.input_price,
                                    model.output_price,
                                    model.context_window,
                                )
                            })
                            .collect();
                        if let Ok(storage) = crate::storage::Storage::new() {
                            let _ =
                                storage.save_models(crate::config::LOCAL_PROVIDER_NAME, &models);
                        }
                        let _ = action_tx.send(Action::SetProviderModels(
                            crate::config::LOCAL_PROVIDER_NAME.to_string(),
                            probe.models.clone(),
                        ));
                        let (status, message) = Self::local_probe_state(&probe);
                        let _ = action_tx.send(Action::SetConnectionState(status, message));
                    }
                    Err(err) => {
                        let _ = action_tx.send(Action::SetConnectionState(
                            crate::ui::status_bar::ConnectionStatus::Failed,
                            Some(format!("Not connected to Local LLM: {err}")),
                        ));
                    }
                }
                return;
            }

            match Self::check_cloud_connection(
                &provider,
                &config_snapshot,
                cloud_api_key.as_deref(),
            )
            .await
            {
                Ok(()) => {
                    let _ = action_tx.send(Action::SetConnectionState(
                        crate::ui::status_bar::ConnectionStatus::CloudConnected,
                        None,
                    ));
                }
                Err(_) => {
                    let _ = action_tx.send(Action::SetConnectionState(
                        crate::ui::status_bar::ConnectionStatus::Failed,
                        Some("Not connected, check settings".to_string()),
                    ));
                }
            }
        });
    }

    fn local_probe_state(
        probe: &crate::llm::local::LocalProbe,
    ) -> (crate::ui::status_bar::ConnectionStatus, Option<String>) {
        match probe.selected_model_loaded {
            Some(false) if probe.server_type == crate::config::LocalServerType::Ollama => (
                crate::ui::status_bar::ConnectionStatus::LocalModelUnloaded,
                Some("Local model unloaded".to_string()),
            ),
            _ => (
                crate::ui::status_bar::ConnectionStatus::LocalConnected,
                Some(format!("Connected to {}", probe.status_label)),
            ),
        }
    }

    async fn check_cloud_connection(
        provider: &str,
        config: &AppConfig,
        api_key: Option<&str>,
    ) -> color_eyre::Result<()> {
        let Some(provider_config) = config.provider_config(provider) else {
            return Err(color_eyre::eyre::eyre!("Missing provider config"));
        };
        if provider_config.auth_type == "oauth" && api_key.is_some_and(|key| !key.trim().is_empty())
        {
            return Ok(());
        }
        let endpoint = provider_config.endpoint.trim_end_matches('/');
        let url = format!("{endpoint}/models");
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()?;
        let mut request = client.get(url);
        if let Some(api_key) = api_key.filter(|key| !key.trim().is_empty()) {
            request = request.bearer_auth(api_key);
        }
        if provider_config.name == "OpenRouter" {
            request = request
                .header("HTTP-Referer", "https://github.com/jp/TermChatUI")
                .header("X-Title", "TermChatUI");
        }
        request.send().await?.error_for_status()?;
        Ok(())
    }

    fn quit_requires_confirmation(&self) -> bool {
        self.config
            .try_read()
            .map(|config| config.quit_confirmation)
            .unwrap_or(true)
    }

    fn quit_action(&self) -> Action {
        if self.quit_requires_confirmation() {
            Action::ShowQuitConfirm
        } else {
            Action::Quit
        }
    }

    pub async fn run(
        &mut self,
        terminal: &mut ratatui::Terminal<crate::Backend>,
    ) -> color_eyre::Result<()> {
        if !std::io::stdin().is_terminal() {
            terminal.draw(|f| {
                self.ui.render(f);
            })?;
            return Ok(());
        }

        let mut reader = crossterm::event::EventStream::new();
        let mut tick = tokio::time::interval(std::time::Duration::from_millis(33));

        loop {
            terminal.draw(|f| {
                self.ui.render(f);
            })?;

            tokio::select! {
                _ = tick.tick() => {}
                maybe_event = reader.next() => {
                    match maybe_event {
                        Some(Ok(crossterm::event::Event::Key(key))) => {
                            if key.kind == crossterm::event::KeyEventKind::Press {
                                if let Some(action) = self.handle_key(key) {
                                    if matches!(action, Action::Quit) {
                                        break;
                                    }
                                    let _ = self.dispatch(action).await;
                                }
                            }
                        }
                        Some(Ok(crossterm::event::Event::Mouse(mouse))) => {
                            if let Some(action) = self.handle_mouse(mouse) {
                                if matches!(action, Action::Quit) {
                                    break;
                                }
                                let _ = self.dispatch(action).await;
                            }
                        }
                        Some(Ok(crossterm::event::Event::FocusGained)) => {
                            self.terminal_has_focus.store(true, Ordering::Relaxed);
                        }
                        Some(Ok(crossterm::event::Event::FocusLost)) => {
                            self.terminal_has_focus.store(false, Ordering::Relaxed);
                        }
                        Some(Ok(crossterm::event::Event::Resize(_, _))) => {}
                        _ => {}
                    }
                }
                maybe_action = self.action_rx.recv() => {
                    if let Some(action) = maybe_action {
                        if matches!(action, Action::Quit) {
                            break;
                        }
                        let _ = self.dispatch(action).await;
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn dispatch(&mut self, action: Action) -> color_eyre::Result<()> {
        match action {
            Action::Quit => Ok(()),
            Action::ConfirmQuit => {
                self.action_tx.send(Action::Quit).ok();
                Ok(())
            }
            Action::CancelModal => {
                self.ui.active_modal = None;
                self.ctrl_c_count = 0;
                Ok(())
            }
            Action::ShowQuitConfirm => {
                if self.quit_requires_confirmation() {
                    self.ui.active_modal = Some(crate::ui::Modal::QuitConfirm);
                    self.ctrl_c_count = 1;
                    self.last_ctrl_c = Some(std::time::Instant::now());
                } else {
                    self.action_tx.send(Action::Quit).ok();
                }
                Ok(())
            }
            Action::ToggleWebSearch => self.toggle_web_search().await,
            Action::ShowSkillsPopup => {
                self.show_skills_popup();
                Ok(())
            }
            Action::ShowMcpPopup => {
                self.show_mcp_popup();
                Ok(())
            }
            Action::ShowLocalSearch(query) => {
                self.show_local_search_popup(&query);
                Ok(())
            }
            Action::CloseListPopup => {
                self.ui.list_popup = None;
                Ok(())
            }
            Action::ToggleSidebar => {
                self.ui.sidebar_open = !self.ui.sidebar_open;
                Ok(())
            }
            Action::ToggleArtifactSidebar => {
                self.ui.artifact_sidebar_open = !self.ui.artifact_sidebar_open;
                Ok(())
            }
            Action::ShowSettings => {
                let config = self.config.read().await;
                let settings = self.load_settings_popup_state(&config);
                self.ui.settings_popup = Some(settings);
                self.ui.show_settings = true;
                Ok(())
            }
            Action::CloseSettings => {
                if let Some(settings) = self.ui.settings_popup.clone() {
                    let _ = self.save_settings_popup_state(&settings).await;
                    self.ui.user_alignment = settings.user_alignment;
                    self.ui.ai_alignment = settings.ai_alignment;
                    self.ui.markdown_mode = settings.markdown_mode;
                    self.ui.show_selector = settings.show_selector;
                    self.ui.show_chat_scrollbar = settings.show_chat_scrollbar;
                    self.ui.collapse_thinking = settings.collapse_thinking;
                    self.ui.kitty_enhanced_text = settings.kitty_enhanced_text;
                    self.ui.kitty_text_max_scale = settings.kitty_text_max_scale;
                    self.ui.web_search_enabled = settings.web_search_enabled;
                    for tab in &mut self.ui.tabs {
                        tab.thinking_fold_overrides.clear();
                    }
                }
                self.ui.show_settings = false;
                self.queue_connection_check_for_active_tab();
                Ok(())
            }
            Action::ToggleSettings => {
                if self.ui.show_settings {
                    if let Some(settings) = self.ui.settings_popup.clone() {
                        let _ = self.save_settings_popup_state(&settings).await;
                        self.ui.user_alignment = settings.user_alignment;
                        self.ui.ai_alignment = settings.ai_alignment;
                        self.ui.markdown_mode = settings.markdown_mode;
                        self.ui.show_selector = settings.show_selector;
                        self.ui.show_chat_scrollbar = settings.show_chat_scrollbar;
                        self.ui.collapse_thinking = settings.collapse_thinking;
                        self.ui.kitty_enhanced_text = settings.kitty_enhanced_text;
                        self.ui.kitty_text_max_scale = settings.kitty_text_max_scale;
                        self.ui.web_search_enabled = settings.web_search_enabled;
                        for tab in &mut self.ui.tabs {
                            tab.thinking_fold_overrides.clear();
                        }
                    }
                    self.ui.show_settings = false;
                    self.queue_connection_check_for_active_tab();
                } else {
                    let config = self.config.read().await;
                    let settings = self.load_settings_popup_state(&config);
                    self.ui.settings_popup = Some(settings);
                    self.ui.show_settings = true;
                }
                Ok(())
            }
            Action::NewChat => {
                if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
                    let conv_id = self
                        .storage
                        .create_conversation(self.ui.active_tab as i64)?;
                    tab.active_conversation = conv_id;
                    tab.messages.clear();
                    tab.thinking_fold_overrides.clear();
                    tab.thinking_hit_areas.clear();
                    tab.scroll_offset = 0;
                    tab.scroll_to_message = None;
                    tab.input_content.clear();
                    tab.input_cursor = 0;
                    tab.input_scroll = 0;
                    tab.generated_title = None;
                    tab.temporary_artifacts.clear();
                }
                Ok(())
            }
            Action::CloseChat => {
                if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
                    tab.conversations
                        .retain(|c| c.id != tab.active_conversation);
                    tab.temporary_artifacts.clear();
                    if tab.conversations.is_empty() {
                        // Create a new empty conversation
                        let conv_id = self
                            .storage
                            .create_conversation(self.ui.active_tab as i64)?;
                        tab.active_conversation = conv_id;
                        tab.messages.clear();
                        tab.thinking_fold_overrides.clear();
                        tab.thinking_hit_areas.clear();
                        tab.scroll_offset = 0;
                        tab.scroll_to_message = None;
                        tab.input_cursor = 0;
                        tab.input_scroll = 0;
                        tab.generated_title = None;
                        tab.temporary_artifacts.clear();
                    } else {
                        tab.active_conversation =
                            tab.conversations.last().map(|c| c.id).unwrap_or(0);
                    }
                    tab.messages.clear();
                    tab.thinking_fold_overrides.clear();
                    tab.thinking_hit_areas.clear();
                    tab.scroll_offset = 0;
                    tab.scroll_to_message = None;
                    tab.input_content.clear();
                    tab.input_cursor = 0;
                    tab.input_scroll = 0;
                }
                Ok(())
            }
            Action::SendMessage(msg) => self.send_message(msg),
            Action::StreamResponse(tab_id, message_idx, content) => {
                self.ui.add_stream_content(tab_id, message_idx, content);
                self.sync_message_media(tab_id);
                Ok(())
            }
            Action::StreamThinking(tab_id, message_idx, content) => {
                self.ui.add_stream_thinking(tab_id, message_idx, content);
                Ok(())
            }
            #[cfg(feature = "memory")]
            Action::SetMemoryActivities(tab_id, message_idx, activities) => {
                if let Some(message) = self
                    .ui
                    .tabs
                    .get_mut(tab_id)
                    .and_then(|tab| tab.messages.get_mut(message_idx))
                {
                    let _ = crate::memory::set_activities(message, &activities);
                }
                Ok(())
            }
            Action::StopStream(tab_id) => {
                self.ui.finish_stream(tab_id);
                Ok(())
            }
            Action::AddMessage(tab_id, msg) => {
                let _ = self.storage.save_message(&msg);
                self.ui.add_message(tab_id, msg);
                self.sync_message_media(tab_id);
                Ok(())
            }
            Action::AddGeneratedFile(tab_id, file) => {
                self.ui.add_generated_file(tab_id, file);
                Ok(())
            }
            Action::LoadConversation(conv_id) => self.load_conversation(conv_id),
            Action::NewConversation(tab_id) => self.new_conversation(tab_id),
            Action::UpdateModel(model) => {
                self.ui.update_model(model);
                Ok(())
            }
            Action::UpdateStatus(status) => {
                self.ui.update_status(status);
                Ok(())
            }
            Action::SetConnectionState(status, message) => {
                self.set_connection_state(status, message);
                Ok(())
            }
            Action::SetTitle(title) => {
                self.ui.set_title(title);
                Ok(())
            }
            Action::SwitchTab(idx) => {
                self.ui.active_tab = idx;
                self.ui.show_settings = false;
                self.refresh_visible_selectors();
                Ok(())
            }
            Action::AddTab(tab) => {
                self.ui.add_tab(tab);
                self.ui.show_settings = false;
                self.refresh_visible_selectors();
                Ok(())
            }
            Action::RemoveTab(idx) => {
                self.ui.remove_tab(idx);
                self.refresh_visible_selectors();
                Ok(())
            }
            Action::ToggleSessionList => {
                self.ui.show_session_list = !self.ui.show_session_list;
                Ok(())
            }
            Action::UpdateTabProvider(tab_id, provider) => {
                if let Some(tab) = self.ui.tabs.get_mut(tab_id) {
                    tab.tab.provider = provider;
                    tab.tab.model.clear();
                    tab.tab.reasoning_effort = None;
                }
                self.refresh_visible_selectors();
                self.queue_connection_check_for_active_tab();
                Ok(())
            }
            Action::UpdateTabModel(tab_id, model) => {
                if let Some(tab) = self.ui.tabs.get_mut(tab_id) {
                    tab.tab.model = model;
                    tab.tab.reasoning_effort = None;
                }
                self.refresh_visible_selectors();
                self.queue_connection_check_for_active_tab();
                Ok(())
            }
            Action::SetProviderModels(provider, models) => {
                if self
                    .ui
                    .tabs
                    .get(self.ui.active_tab)
                    .map(|tab| tab.tab.provider == provider)
                    .unwrap_or(false)
                {
                    self.refresh_visible_selectors();
                }

                if let Some(settings) = &mut self.ui.settings_popup {
                    if settings.default_provider == provider {
                        settings.available_models = models.clone();
                    }
                    if settings.models_provider == provider {
                        settings.models_available_models = models.clone();
                    }
                }
                Ok(())
            }
            Action::SaveApiKey(_provider, _key) => Ok(()),
            Action::MouseClick(_col, _row) => Ok(()),
            Action::SaveGeneratedFile => self.save_generated_file(),
            Action::CancelSaveDialog => {
                self.ui.save_file_dialog = None;
                Ok(())
            }
            Action::RefreshModels => {
                let storage = crate::storage::Storage::new()?;
                tokio::task::spawn_blocking(move || {
                    let rt = tokio::runtime::Handle::current();
                    rt.block_on(async {
                        crate::llm::model_fetcher::refresh_all_models(&storage).await;
                    });
                });
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> Option<Action> {
        use crossterm::event::{KeyCode, KeyModifiers};

        if key.kind != crossterm::event::KeyEventKind::Press {
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

    fn handle_mouse(&mut self, mouse: crossterm::event::MouseEvent) -> Option<Action> {
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

    fn handle_mouse_scroll(
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
                    crate::ui::artifact_sidebar::ArtifactSection::Vault => {
                        let visible = self
                            .ui
                            .artifact_sidebar_state
                            .vault_body
                            .map(crate::ui::artifact_sidebar::ArtifactSidebar::visible_rows)
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

    fn handle_mouse_click(&mut self, col: u16, row: u16) -> Option<Action> {
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
                    if settings.active_tab == crate::ui::settings_tab::SettingsTab::Models {
                        if settings.models_dropdown_open {
                            return None;
                        }
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
            let sidebar_width = 24u16;
            let sidebar_area = Rect::new(area.x, area.y + 1, sidebar_width, area.height - 2);
            let active_tab = self.ui.tabs.get(self.ui.active_tab)?;
            let show_new_chat =
                active_tab.messages.is_empty() && active_tab.generated_title.is_none();
            let sidebar = crate::ui::sidebar::Sidebar::new(
                &active_tab.conversations,
                active_tab.active_conversation,
                show_new_chat,
                show_new_chat,
            );

            // Check "New Chat..." card
            if let Some(card_area) = sidebar.new_chat_card_area(sidebar_area) {
                if card_area.contains(pos) {
                    return Some(Action::SwitchTab(self.ui.active_tab));
                }
            }

            for (conversation_id, hit_area) in sidebar.conversation_item_areas(sidebar_area) {
                if hit_area.contains(pos) {
                    return Some(Action::LoadConversation(conversation_id));
                }
            }

            // Check + New Chat button
            let new_chat_btn = sidebar.new_chat_button_area(sidebar_area);
            if new_chat_btn.contains(pos) {
                return Some(Action::NewChat);
            }

            // Check Settings button
            let settings_btn = sidebar.settings_area(sidebar_area);
            if settings_btn.contains(pos) {
                return Some(Action::ShowSettings);
            }
        }

        if let Some(action) = self.ui.artifact_sidebar_state.action_at(pos) {
            match action {
                crate::ui::artifact_sidebar::ArtifactSidebarAction::Open(handle) => {
                    self.open_artifact(handle);
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

    fn insert_input_text(&mut self, text: &str) {
        if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
            let cursor = tab.input_cursor.min(tab.input_content.chars().count());
            let byte = char_to_byte_index(&tab.input_content, cursor);
            let needs_space = cursor > 0
                && !tab.input_content[..byte].ends_with(char::is_whitespace)
                && !text.starts_with(char::is_whitespace);
            if needs_space {
                tab.input_content.insert(byte, ' ');
                tab.input_cursor += 1;
            }
            let insert_at = char_to_byte_index(&tab.input_content, tab.input_cursor);
            tab.input_content.insert_str(insert_at, text);
            tab.input_cursor += text.chars().count();
        }
        self.refresh_input_popup();
    }

    fn send_message(&mut self, content: String) -> color_eyre::Result<()> {
        let mut conversation_id = self
            .ui
            .tabs
            .get(self.ui.active_tab)
            .map(|t| t.active_conversation)
            .unwrap_or(0);
        if conversation_id == 0 {
            conversation_id = self
                .storage
                .create_conversation(self.ui.active_tab as i64)?;
            if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
                tab.active_conversation = conversation_id;
            }
        }
        let role = "user".to_string();

        let msg = Message::new(conversation_id, role, content.clone());
        let _ = self.storage.save_message(&msg);

        let msg_clone = msg.clone();
        self.ui.add_message(self.ui.active_tab, msg_clone);

        // On first message, create a conversation entry and generate title
        if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
            if tab.messages.len() == 1 && tab.generated_title.is_none() {
                let title = Self::generate_title(&content);
                tab.generated_title = Some(title.clone());
                // Add to sidebar conversations
                tab.conversations.push(crate::ui::ConversationEntry {
                    id: tab.active_conversation,
                    title: title.clone(),
                    created_at: String::new(),
                });
            }
        }

        let tab_id = self.ui.active_tab;
        let Some(tab_state) = self.ui.tabs.get(tab_id) else {
            return Ok(());
        };
        let provider = tab_state.tab.provider.clone();
        let is_local_provider = crate::llm::local::is_local_provider(&provider);
        let model = tab_state.tab.model.clone();
        let reasoning_effort = tab_state.tab.reasoning_effort.clone();
        let messages = tab_state.messages.clone();
        let config_snapshot = self
            .config
            .try_read()
            .map(|config| config.clone())
            .unwrap_or_default();
        if let Some(response) = crate::reminders::maybe_handle_request(&config_snapshot, &content) {
            let assistant = Message::new(conversation_id, "assistant".to_string(), response);
            let _ = self.storage.save_message(&assistant);
            self.ui.add_message(tab_id, assistant);
            return Ok(());
        }
        let endpoint = if is_local_provider {
            Some(config_snapshot.local_inference.chat_endpoint())
        } else {
            tab_state.tab.endpoint.clone().or_else(|| {
                self.provider_config(&provider)
                    .map(|(endpoint, _, _)| endpoint)
            })
        };
        let provider_config = self.provider_config(&provider);
        let backend_type = if is_local_provider {
            crate::llm::local::backend_type_label(config_snapshot.local_inference.server_type)
                .to_string()
        } else {
            provider_config
                .as_ref()
                .map(|(_, _, backend_type)| backend_type.clone())
                .unwrap_or_else(|| "openai".to_string())
        };
        let api_key = if is_local_provider {
            config_snapshot
                .local_inference
                .api_token_env
                .as_deref()
                .and_then(|env_var| std::env::var(env_var).ok())
                .filter(|value| !value.trim().is_empty())
        } else {
            provider_config.as_ref().and_then(|(_, env_var, _)| {
                crate::llm::auth::read_provider_api_key(&provider, env_var, &self.storage)
            })
        };
        let action_tx = self.action_tx.clone();
        let system_prompt = self.system_prompt.clone();
        let user_request = content.clone();
        let terminal_has_focus = Arc::clone(&self.terminal_has_focus);

        let assistant_msg = Message::new(conversation_id, "assistant".to_string(), String::new());
        self.ui.add_message(tab_id, assistant_msg);
        let assistant_idx = self
            .ui
            .tabs
            .get(tab_id)
            .map(|tab| tab.messages.len().saturating_sub(1))
            .unwrap_or(0);
        if let Some(tab) = self.ui.tabs.get_mut(tab_id) {
            tab.streaming = true;
        }

        let Some(endpoint) = endpoint else {
            self.ui.connection_status = crate::ui::status_bar::ConnectionStatus::Failed;
            self.ui.connection_message = Some(format!("Missing endpoint for {provider}"));
            let _ = action_tx.send(Action::StreamResponse(
                tab_id,
                assistant_idx,
                format!("No endpoint configured for {provider}."),
            ));
            let _ = action_tx.send(Action::StopStream(tab_id));
            return Ok(());
        };

        if !is_local_provider && !provider.eq_ignore_ascii_case("Ollama") && api_key.is_none() {
            self.ui.connection_status = crate::ui::status_bar::ConnectionStatus::Failed;
            self.ui.connection_message = Some(format!("Missing credentials for {provider}"));
            let _ = action_tx.send(Action::StreamResponse(
                tab_id,
                assistant_idx,
                format!("No API key or OAuth token found for {provider}. Open Settings > Providers or set the provider env var."),
            ));
            let _ = action_tx.send(Action::StopStream(tab_id));
            return Ok(());
        }

        if is_local_provider {
            self.ui.connection_status = crate::ui::status_bar::ConnectionStatus::LocalConnected;
            self.ui.connection_message = Some("Connected to Local LLM".to_string());
        } else {
            self.ui.connection_status = crate::ui::status_bar::ConnectionStatus::CloudConnected;
            self.ui.connection_message = Some(format!("{provider} / {model}"));
        }

        tokio::spawn(async move {
            let event_tx = action_tx.clone();
            let mut runtime_system_prompt = system_prompt.clone();
            let mut request = crate::llm::chat::ChatRequest {
                provider,
                endpoint,
                model,
                reasoning_effort,
                backend_type,
                api_key,
                system_prompt: runtime_system_prompt.clone(),
                messages,
            };
            let skills =
                crate::skill_runtime::prepare(&config_snapshot, &user_request, &request).await;
            runtime_system_prompt.push_str(&skills.context);
            request.messages.extend(skills.messages);
            for notice in skills.notices {
                let _ = action_tx.send(Action::UpdateStatus(notice));
            }
            match crate::search::maybe_search(&config_snapshot, &user_request, false).await {
                Ok(Some(context)) => {
                    request
                        .messages
                        .push(crate::search::untrusted_context_message(&context));
                }
                Ok(None) => {}
                Err(err) => {
                    let _ = action_tx.send(Action::UpdateStatus(format!(
                        "Web search unavailable: {err}"
                    )));
                }
            }
            #[cfg(feature = "memory")]
            let mut memory_activities = Vec::new();
            #[cfg(feature = "memory")]
            if config_snapshot.memory.enabled {
                memory_activities.push(crate::memory::MemoryActivity::Recalling);
                let _ = action_tx.send(Action::SetMemoryActivities(
                    tab_id,
                    assistant_idx,
                    memory_activities.clone(),
                ));
                let recall_config = config_snapshot.clone();
                let recall_query = user_request.clone();
                match tokio::task::spawn_blocking(move || {
                    crate::memory::recall(&recall_config, &recall_query)
                })
                .await
                {
                    Ok(Ok(recall)) if !recall.context.is_empty() => {
                        memory_activities.clear();
                        memory_activities.push(crate::memory::MemoryActivity::Recalled {
                            titles: recall.titles,
                        });
                        crate::memory::append_recall(&mut request.messages, &recall.context);
                    }
                    Ok(Ok(_)) => memory_activities.clear(),
                    Ok(Err(error)) => {
                        memory_activities.clear();
                        memory_activities.push(crate::memory::MemoryActivity::Failed {
                            message: error.to_string(),
                        });
                    }
                    Err(error) => {
                        memory_activities.clear();
                        memory_activities.push(crate::memory::MemoryActivity::Failed {
                            message: error.to_string(),
                        });
                    }
                }
                let _ = action_tx.send(Action::SetMemoryActivities(
                    tab_id,
                    assistant_idx,
                    memory_activities.clone(),
                ));
                let operation_config = config_snapshot.clone();
                let operation_request = user_request.clone();
                match tokio::task::spawn_blocking(move || {
                    crate::memory::run_skill_operation(&operation_config, &operation_request)
                })
                .await
                {
                    Ok(Ok(Some(operation))) => {
                        if !operation.context.is_empty() {
                            request.messages.push(Message::new(
                                conversation_id,
                                "user".to_string(),
                                operation.context,
                            ));
                        }
                        if let Some(activity) = operation.activity {
                            memory_activities.push(activity);
                            let _ = action_tx.send(Action::SetMemoryActivities(
                                tab_id,
                                assistant_idx,
                                memory_activities.clone(),
                            ));
                        }
                    }
                    Ok(Ok(None)) => {}
                    Ok(Err(error)) => {
                        memory_activities.push(crate::memory::MemoryActivity::Failed {
                            message: error.to_string(),
                        });
                        let _ = action_tx.send(Action::SetMemoryActivities(
                            tab_id,
                            assistant_idx,
                            memory_activities.clone(),
                        ));
                    }
                    Err(error) => {
                        memory_activities.push(crate::memory::MemoryActivity::Failed {
                            message: error.to_string(),
                        });
                        let _ = action_tx.send(Action::SetMemoryActivities(
                            tab_id,
                            assistant_idx,
                            memory_activities.clone(),
                        ));
                    }
                }
                if config_snapshot.memory.auto_capture {
                    runtime_system_prompt.push_str(crate::memory::AUTO_CAPTURE_POLICY);
                }
            }
            request.system_prompt = runtime_system_prompt;
            let mut visible_answer = String::new();
            #[cfg(feature = "memory")]
            let mut remember_filter = crate::memory::RememberFilter::default();
            let result = crate::llm::chat::stream_chat(request, |event| match event {
                crate::llm::chat::ChatStreamEvent::Answer(content) => {
                    let visible = {
                        #[cfg(feature = "memory")]
                        {
                            remember_filter.push(&content)
                        }
                        #[cfg(not(feature = "memory"))]
                        {
                            content
                        }
                    };
                    if !visible.is_empty() {
                        visible_answer.push_str(&visible);
                        let _ =
                            event_tx.send(Action::StreamResponse(tab_id, assistant_idx, visible));
                    }
                }
                crate::llm::chat::ChatStreamEvent::Thinking(content) => {
                    let _ = event_tx.send(Action::StreamThinking(tab_id, assistant_idx, content));
                }
            })
            .await;

            #[cfg(feature = "memory")]
            let remembered = {
                let filtered = remember_filter.finish();
                if !filtered.visible.is_empty() {
                    visible_answer.push_str(&filtered.visible);
                    let _ = action_tx.send(Action::StreamResponse(
                        tab_id,
                        assistant_idx,
                        filtered.visible,
                    ));
                }
                filtered.memory
            };

            match result {
                Ok(response) if !visible_answer.is_empty() || !response.thinking.is_empty() => {
                    let file_id = rand::random();
                    let generated_file = crate::app::GeneratedFile::maybe_from_response(
                        file_id,
                        conversation_id,
                        &user_request,
                        &visible_answer,
                    )
                    .or_else(|| {
                        crate::app::GeneratedFile::from_skill_response(
                            file_id,
                            conversation_id,
                            &user_request,
                            &visible_answer,
                        )
                    });
                    #[cfg(feature = "memory")]
                    if let Some(fact) = remembered {
                        memory_activities.push(crate::memory::MemoryActivity::Saving);
                        let _ = action_tx.send(Action::SetMemoryActivities(
                            tab_id,
                            assistant_idx,
                            memory_activities.clone(),
                        ));
                        let capture_config = config_snapshot.clone();
                        let capture = tokio::task::spawn_blocking(move || {
                            crate::memory::capture(&capture_config, &fact)
                        })
                        .await;
                        memory_activities.retain(|activity| {
                            !matches!(activity, crate::memory::MemoryActivity::Saving)
                        });
                        match capture {
                            Ok(Ok(crate::memory::WriteOutcome::Saved { title, path })) => {
                                memory_activities
                                    .push(crate::memory::MemoryActivity::Saved { title, path });
                            }
                            Ok(Ok(crate::memory::WriteOutcome::AlreadyKnown { title })) => {
                                memory_activities
                                    .push(crate::memory::MemoryActivity::AlreadyKnown { title });
                            }
                            Ok(Err(error)) => {
                                memory_activities.push(crate::memory::MemoryActivity::Failed {
                                    message: error.to_string(),
                                });
                            }
                            Err(error) => {
                                memory_activities.push(crate::memory::MemoryActivity::Failed {
                                    message: error.to_string(),
                                });
                            }
                        }
                        let _ = action_tx.send(Action::SetMemoryActivities(
                            tab_id,
                            assistant_idx,
                            memory_activities.clone(),
                        ));
                    }
                    if let Ok(storage) = crate::storage::Storage::new() {
                        let assistant_content = visible_answer.clone();
                        let mut msg = Message::new(
                            conversation_id,
                            "assistant".to_string(),
                            assistant_content,
                        );
                        msg.thinking_content =
                            (!response.thinking.is_empty()).then_some(response.thinking);
                        msg.token_count = response.total_tokens;
                        #[cfg(feature = "memory")]
                        let _ = crate::memory::set_activities(&mut msg, &memory_activities);
                        let _ = storage.save_message(&msg);
                    }
                    if !terminal_has_focus.load(Ordering::Relaxed) {
                        let _ = crate::notifications::notify_finished(
                            &config_snapshot,
                            &user_request,
                            &visible_answer,
                        )
                        .await;
                    }
                    if let Some(file) = generated_file {
                        let _ = action_tx.send(Action::AddGeneratedFile(tab_id, file));
                    }
                }
                Ok(_) => {}
                Err(err) => {
                    let message = format!("Provider request failed: {err}");
                    let _ = action_tx.send(Action::UpdateStatus(message.clone()));
                    let _ = action_tx.send(Action::StreamResponse(tab_id, assistant_idx, message));
                }
            }
            let _ = action_tx.send(Action::StopStream(tab_id));
        });

        Ok(())
    }

    fn provider_config(&self, provider: &str) -> Option<(String, String, String)> {
        self.ui
            .db_providers
            .iter()
            .find(|(name, _, _, _, _)| name == provider || name.eq_ignore_ascii_case(provider))
            .map(|(_, endpoint, env_var, backend_type, _)| {
                (endpoint.clone(), env_var.clone(), backend_type.clone())
            })
    }

    fn save_generated_file(&mut self) -> color_eyre::Result<()> {
        let Some(dialog) = self.ui.save_file_dialog.clone() else {
            return Ok(());
        };

        let trimmed_path = dialog.path_input.trim();
        if trimmed_path.is_empty() {
            self.ui.connection_status = crate::ui::status_bar::ConnectionStatus::Failed;
            self.ui.connection_message = Some("Choose a path before saving.".to_string());
            return Ok(());
        }

        let path = std::path::PathBuf::from(trimmed_path);
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        match &dialog.artifact.content {
            Some(content) => std::fs::write(&path, content.as_bytes())?,
            None => {
                let Some(source) = &dialog.artifact.path else {
                    return Ok(());
                };
                std::fs::copy(source, &path)?;
            }
        }

        self.ui.save_file_dialog = None;
        self.promote_temporary_artifact_if_vault_path(&dialog.artifact.handle, &path);
        self.ui.connection_status = crate::ui::status_bar::ConnectionStatus::CloudConnected;
        self.ui.connection_message = Some(format!("Saved {}", path.display()));
        Ok(())
    }

    fn refresh_vault_artifacts(&mut self) {
        self.ui.vault_artifacts = self
            .vault
            .as_ref()
            .and_then(|vault| {
                vault.list_files(None).ok().map(|paths| {
                    paths
                        .into_iter()
                        .map(|path| {
                            crate::ui::artifact_sidebar::ArtifactEntry::vault_file(
                                &vault.root,
                                &path,
                            )
                        })
                        .collect()
                })
            })
            .unwrap_or_default();
    }

    fn sync_message_media(&mut self, tab_id: usize) {
        let Some(tab) = self.ui.tabs.get_mut(tab_id) else {
            return;
        };

        let existing: HashSet<_> = tab
            .temporary_artifacts
            .iter()
            .filter_map(|artifact| match &artifact.handle {
                crate::ui::artifact_sidebar::ArtifactHandle::Media(source) => Some(source.clone()),
                _ => None,
            })
            .collect();
        let mut discovered = Vec::new();
        for message in &tab.messages {
            for source in local_media_sources(&message.content) {
                if existing.contains(&source) {
                    continue;
                }
                if let Some(artifact) =
                    crate::ui::artifact_sidebar::ArtifactEntry::temp_media(&source)
                {
                    discovered.push(artifact);
                }
            }
        }
        tab.temporary_artifacts.extend(discovered);
    }

    fn prepare_artifact_save(&mut self, handle: crate::ui::artifact_sidebar::ArtifactHandle) {
        let Some(artifact) = self.find_artifact(&handle) else {
            return;
        };
        if artifact.is_markdown() && self.vault.is_some() {
            if self.save_temp_artifact_to_vault(&artifact).is_ok() {
                self.ui.connection_status = crate::ui::status_bar::ConnectionStatus::CloudConnected;
                self.ui.connection_message = Some(format!("Saved {} to vault", artifact.name));
            }
            return;
        }

        let base_dir = self
            .config
            .try_read()
            .ok()
            .and_then(|cfg| cfg.artifact_save_dir.clone())
            .map(std::path::PathBuf::from)
            .or_else(dirs::download_dir)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        self.ui.save_file_dialog = Some(crate::ui::modals::save_file::SaveFileDialog::new(
            &artifact, base_dir, "Save",
        ));
    }

    fn save_temp_artifact_to_vault(
        &mut self,
        artifact: &crate::ui::artifact_sidebar::ArtifactEntry,
    ) -> color_eyre::Result<()> {
        let Some(vault) = &self.vault else {
            return Ok(());
        };
        let content = artifact.content.clone().unwrap_or_default();
        vault.write_file(std::path::Path::new(&artifact.name), &content)?;
        self.remove_temporary_artifact(&artifact.handle);
        self.refresh_vault_artifacts();
        Ok(())
    }

    fn promote_temporary_artifact_if_vault_path(
        &mut self,
        handle: &crate::ui::artifact_sidebar::ArtifactHandle,
        saved_path: &std::path::Path,
    ) {
        let Some(vault) = &self.vault else {
            return;
        };
        if saved_path.starts_with(&vault.root) {
            self.remove_temporary_artifact(handle);
            self.refresh_vault_artifacts();
        }
    }

    fn open_artifact(&mut self, handle: crate::ui::artifact_sidebar::ArtifactHandle) {
        let Some(mut artifact) = self.find_artifact(&handle) else {
            return;
        };
        if artifact.content.is_none() {
            if let Some(path) = artifact.path.as_ref() {
                artifact.content = std::fs::read_to_string(path).ok();
            }
        }
        self.ui.artifact_viewer =
            Some(crate::ui::modals::artifact_viewer::ArtifactViewerState::new(artifact));
    }

    fn delete_artifact(&mut self, handle: crate::ui::artifact_sidebar::ArtifactHandle) {
        match &handle {
            crate::ui::artifact_sidebar::ArtifactHandle::Vault(path) => {
                if let Some(vault) = &self.vault {
                    let full_path = vault.root.join(path);
                    let _ = std::fs::remove_file(full_path);
                    self.refresh_vault_artifacts();
                }
            }
            _ => {
                self.remove_temporary_artifact(&handle);
            }
        }
        if self
            .ui
            .artifact_viewer
            .as_ref()
            .is_some_and(|viewer| viewer.handle() == &handle)
        {
            self.ui.artifact_viewer = None;
        }
    }

    fn remove_temporary_artifact(&mut self, handle: &crate::ui::artifact_sidebar::ArtifactHandle) {
        if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
            tab.temporary_artifacts
                .retain(|artifact| &artifact.handle != handle);
        }
    }

    fn find_artifact(
        &self,
        handle: &crate::ui::artifact_sidebar::ArtifactHandle,
    ) -> Option<crate::ui::artifact_sidebar::ArtifactEntry> {
        self.ui
            .tabs
            .get(self.ui.active_tab)
            .and_then(|tab| {
                tab.temporary_artifacts
                    .iter()
                    .find(|artifact| &artifact.handle == handle)
                    .cloned()
            })
            .or_else(|| {
                self.ui
                    .vault_artifacts
                    .iter()
                    .find(|artifact| &artifact.handle == handle)
                    .cloned()
            })
    }

    fn cached_models_for_provider(
        &self,
        provider: &str,
    ) -> Vec<crate::ui::settings_tab::ModelInfo> {
        let mut models = match self.storage.get_models(provider) {
            Ok(models) => models
                .into_iter()
                .map(|(id, input_price, output_price, context_window)| {
                    crate::ui::settings_tab::ModelInfo {
                        id,
                        input_price,
                        output_price,
                        context_window,
                    }
                })
                .collect::<Vec<_>>(),
            Err(_) => Vec::new(),
        };
        for fallback in crate::llm::model_fetcher::provider_model_fallback(provider) {
            if !models.iter().any(|existing| existing.id == fallback.id) {
                models.push(fallback);
            }
        }
        models.sort_by(|left, right| left.id.cmp(&right.id));
        models
    }

    fn refresh_models_for_provider(&self, provider: String) {
        if crate::llm::local::is_local_provider(&provider) {
            let action_tx = self.action_tx.clone();
            let config_snapshot = self
                .config
                .try_read()
                .map(|config| config.clone())
                .unwrap_or_default();
            tokio::spawn(async move {
                let Ok(probe) = crate::llm::local::probe(&config_snapshot.local_inference).await
                else {
                    return;
                };
                let models: Vec<(String, Option<f64>, Option<f64>, Option<u32>)> = probe
                    .models
                    .iter()
                    .map(|model| {
                        (
                            model.id.clone(),
                            model.input_price,
                            model.output_price,
                            model.context_window,
                        )
                    })
                    .collect();
                if let Ok(storage) = crate::storage::Storage::new() {
                    let _ = storage.save_models(crate::config::LOCAL_PROVIDER_NAME, &models);
                }
                let _ = action_tx.send(Action::SetProviderModels(
                    crate::config::LOCAL_PROVIDER_NAME.to_string(),
                    probe.models,
                ));
            });
            return;
        }

        let Some((endpoint, env_var, backend_type)) = self.provider_config(&provider) else {
            return;
        };
        let action_tx = self.action_tx.clone();

        tokio::task::spawn_blocking(move || {
            let Ok(storage) = crate::storage::Storage::new() else {
                return;
            };
            let rt = tokio::runtime::Handle::current();
            rt.block_on(async {
                crate::llm::model_fetcher::refresh_provider_models(
                    &storage,
                    &provider,
                    &endpoint,
                    &env_var,
                    &backend_type,
                )
                .await;
            });

            let Ok(models) = storage.get_models(&provider) else {
                return;
            };
            if models.is_empty() {
                return;
            }

            let model_infos = models
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
            let _ = action_tx.send(Action::SetProviderModels(provider, model_infos));
        });
    }

    fn generate_title(content: &str) -> String {
        let words: Vec<&str> = content.split_whitespace().collect();
        let title_words: Vec<&str> = words.iter().take(5).copied().collect();
        let title = title_words.join(" ");
        let title = if title.len() > 16 {
            format!("{}...", &title[..16])
        } else if words.len() > 5 {
            format!("{}...", title)
        } else {
            title
        };
        if title.is_empty() {
            "New Chat".to_string()
        } else {
            title
        }
    }

    fn load_conversation(&mut self, conv_id: i64) -> color_eyre::Result<()> {
        let messages = self.storage.get_messages(conv_id)?;
        if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
            tab.messages.clear();
            tab.thinking_fold_overrides.clear();
            tab.thinking_hit_areas.clear();
            tab.temporary_artifacts.clear();
            tab.scroll_offset = 0;
            tab.scroll_to_message = None;
            tab.active_conversation = conv_id;
            for msg in messages {
                tab.messages.push(msg);
            }
        }
        self.sync_message_media(self.ui.active_tab);
        Ok(())
    }

    fn new_conversation(&mut self, tab_id: usize) -> color_eyre::Result<()> {
        if let Some(tab) = self.ui.tabs.get_mut(tab_id) {
            let conv_id = self.storage.create_conversation(tab_id as i64)?;
            tab.active_conversation = conv_id;
            tab.messages.clear();
            tab.thinking_fold_overrides.clear();
            tab.thinking_hit_areas.clear();
            tab.temporary_artifacts.clear();
            tab.scroll_offset = 0;
            tab.scroll_to_message = None;
            tab.generated_title = None;
        }
        Ok(())
    }

    fn fetch_models_for_settings(
        &self,
        config: &AppConfig,
    ) -> Vec<crate::ui::settings_tab::ModelInfo> {
        let provider = if config.default_provider.is_empty() {
            return Vec::new();
        } else {
            &config.default_provider
        };

        if crate::llm::local::is_local_provider(provider) {
            return self
                .storage
                .get_models(crate::config::LOCAL_PROVIDER_NAME)
                .unwrap_or_default()
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

        self.storage
            .get_models(provider)
            .unwrap_or_default()
            .into_iter()
            .map(|(id, input_price, output_price, context_window)| {
                crate::ui::settings_tab::ModelInfo {
                    id,
                    input_price,
                    output_price,
                    context_window,
                }
            })
            .collect()
    }

    fn show_skills_popup(&mut self) {
        let items = crate::skills::SkillCatalog::discover()
            .map(|catalog| {
                catalog
                    .list()
                    .iter()
                    .map(|skill| {
                        let mut description: String = skill.description.chars().take(32).collect();
                        if skill.description.chars().nth(32).is_some() {
                            description.push_str("...");
                        }
                        let origin = match &skill.origin {
                            crate::skills::SkillOrigin::Builtin => "built-in",
                            crate::skills::SkillOrigin::External(_) => "external",
                        };
                        crate::ui::modals::list_popup::ListPopupItem {
                            label: format!("@{} - {} [{origin}]", skill.name, description),
                            action: Some(
                                crate::ui::modals::list_popup::ListPopupAction::InsertText(
                                    format!("@{} ", skill.name),
                                ),
                            ),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.ui.list_popup = Some(crate::ui::modals::list_popup::ListPopup::selectable(
            "Skills",
            "No skills found.",
            items,
        ));
    }

    fn show_skill_popup(&mut self, name: &str) {
        let content = crate::skills::SkillCatalog::discover()
            .and_then(|catalog| catalog.load(name))
            .ok()
            .flatten()
            .map(|skill| skill.source.lines().map(str::to_string).collect::<Vec<_>>())
            .unwrap_or_default();
        self.ui.list_popup = Some(crate::ui::modals::list_popup::ListPopup::new(
            format!("@{name}"),
            "Skill not found.",
            content,
        ));
    }

    fn show_mcp_popup(&mut self) {
        let config = self
            .config
            .try_read()
            .map(|config| config.clone())
            .unwrap_or_default();
        let items = crate::mcp::merged_configs(&config.mcp_servers)
            .iter()
            .map(|server| {
                let status = if server.enabled { "on" } else { "off" };
                if let Some(url) = &server.url {
                    format!("{}  [{}]  {}", server.name, status, url)
                } else if let Some(command) = &server.command {
                    let args = server.args.clone().unwrap_or_default().join(" ");
                    format!("{}  [{}]  {} {}", server.name, status, command, args)
                } else {
                    format!("{}  [{}]", server.name, status)
                }
            })
            .collect();
        self.ui.list_popup = Some(crate::ui::modals::list_popup::ListPopup::new(
            "MCP Servers",
            "No MCP servers configured.",
            items,
        ));
    }

    fn show_theme_popup(&mut self, filter: &str) {
        let query = filter.trim().to_ascii_lowercase();
        let items = crate::theme::theme_keys()
            .into_iter()
            .filter_map(|key| {
                let label = crate::theme::theme_label(key);
                let haystack = format!("{key} {label}").to_ascii_lowercase();
                haystack.contains(&query).then_some(
                    crate::ui::modals::list_popup::ListPopupItem::action(
                        format!("{label}  [{key}]"),
                        crate::ui::modals::list_popup::ListPopupAction::SetTheme(key.to_string()),
                    ),
                )
            })
            .collect();
        self.ui.list_popup = Some(
            crate::ui::modals::list_popup::ListPopup::anchored_selectable(
                "Themes",
                "No matching themes.",
                items,
                self.current_input_anchor(),
            ),
        );
    }

    fn show_local_search_popup(&mut self, query: &str) {
        let items = self
            .vault
            .as_ref()
            .and_then(|vault| vault.search(query).ok())
            .map(|paths| {
                paths
                    .into_iter()
                    .map(|path| path.display().to_string())
                    .collect()
            })
            .unwrap_or_default();
        self.ui.list_popup = Some(crate::ui::modals::list_popup::ListPopup::new(
            format!("Local Search: {}", query.trim()),
            "No local matches. Configure vault_path to enable local search.",
            items,
        ));
    }

    fn current_input_anchor(&self) -> Option<Rect> {
        self.ui
            .tabs
            .get(self.ui.active_tab)
            .and_then(|tab| tab.input_area)
    }

    fn insert_input_char(&mut self, character: char) {
        if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
            let cursor = tab.input_cursor.min(tab.input_content.chars().count());
            let byte = char_to_byte_index(&tab.input_content, cursor);
            tab.input_content.insert(byte, character);
            tab.input_cursor = cursor + 1;
        }
        self.refresh_input_popup();
    }

    fn backspace_input_char(&mut self) {
        if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
            let cursor = tab.input_cursor.min(tab.input_content.chars().count());
            if cursor == 0 {
                return;
            }
            let start = char_to_byte_index(&tab.input_content, cursor - 1);
            let end = char_to_byte_index(&tab.input_content, cursor);
            tab.input_content.replace_range(start..end, "");
            tab.input_cursor = cursor - 1;
        }
        self.refresh_input_popup();
    }

    fn delete_input_char(&mut self) {
        if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
            let cursor = tab.input_cursor.min(tab.input_content.chars().count());
            if cursor >= tab.input_content.chars().count() {
                return;
            }
            let start = char_to_byte_index(&tab.input_content, cursor);
            let end = char_to_byte_index(&tab.input_content, cursor + 1);
            tab.input_content.replace_range(start..end, "");
        }
        self.refresh_input_popup();
    }

    fn move_input_cursor_left(&mut self) {
        if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
            tab.input_cursor = tab.input_cursor.saturating_sub(1);
        }
        self.refresh_input_popup();
    }

    fn move_input_cursor_right(&mut self) {
        if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
            let len = tab.input_content.chars().count();
            tab.input_cursor = (tab.input_cursor + 1).min(len);
        }
        self.refresh_input_popup();
    }

    fn move_input_cursor_home(&mut self) {
        if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
            tab.input_cursor = 0;
        }
        self.refresh_input_popup();
    }

    fn move_input_cursor_end(&mut self) {
        if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
            tab.input_cursor = tab.input_content.chars().count();
        }
        self.refresh_input_popup();
    }

    fn scroll_active_chat_lines(&mut self, delta: isize) {
        let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) else {
            return;
        };
        let max_scroll = tab
            .total_rendered_lines
            .saturating_sub(tab.message_viewport_height);
        let next = if delta.is_negative() {
            tab.scroll_offset.saturating_sub(delta.unsigned_abs())
        } else {
            tab.scroll_offset.saturating_add(delta as usize)
        };
        tab.scroll_offset = next.min(max_scroll);
        tab.scroll_to_message = None;
    }

    fn scroll_active_chat_page(&mut self, down: bool) {
        let page = self
            .ui
            .tabs
            .get(self.ui.active_tab)
            .map(|tab| tab.message_viewport_height.saturating_sub(1).max(1))
            .unwrap_or(1);
        let delta = if down {
            page as isize
        } else {
            -(page as isize)
        };
        self.scroll_active_chat_lines(delta);
    }

    fn jump_to_adjacent_answer(&mut self, forward: bool) {
        let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) else {
            return;
        };
        let assistant_indices: Vec<usize> = tab
            .answer_anchor_lines
            .iter()
            .filter(|(message_idx, _)| {
                tab.messages
                    .get(*message_idx)
                    .map(|message| message.role == "assistant")
                    .unwrap_or(false)
            })
            .map(|(message_idx, _)| *message_idx)
            .collect();
        if assistant_indices.is_empty() {
            return;
        }
        let current = tab
            .answer_anchor_lines
            .iter()
            .filter(|(message_idx, line)| {
                tab.messages
                    .get(*message_idx)
                    .map(|message| message.role == "assistant")
                    .unwrap_or(false)
                    && *line <= tab.scroll_offset
            })
            .map(|(message_idx, _)| *message_idx)
            .next_back()
            .or_else(|| assistant_indices.first().copied());
        let next = match current.and_then(|message_idx| {
            assistant_indices
                .iter()
                .position(|candidate| *candidate == message_idx)
        }) {
            Some(position) if forward => assistant_indices
                .get((position + 1).min(assistant_indices.len() - 1))
                .copied(),
            Some(position) => assistant_indices.get(position.saturating_sub(1)).copied(),
            None => assistant_indices.first().copied(),
        };
        if let Some(message_idx) = next {
            tab.scroll_to_message = Some(message_idx);
        }
    }

    fn set_input_cursor_from_click(&mut self, position: ratatui::layout::Position) {
        let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) else {
            return;
        };
        let Some(area) = tab.input_text_area else {
            return;
        };
        if !area.contains(position) {
            return;
        }
        let relative = position.x.saturating_sub(area.x) as usize;
        let len = tab.input_content.chars().count();
        tab.input_cursor = (tab.input_scroll + relative).min(len);
        self.refresh_input_popup();
    }

    fn replace_input_content(&mut self, content: String) {
        if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
            tab.input_content = content;
            tab.input_cursor = tab.input_content.chars().count();
            tab.input_scroll = 0;
        }
        self.refresh_input_popup();
    }

    fn refresh_input_popup(&mut self) {
        let (input, cursor) = self
            .ui
            .tabs
            .get(self.ui.active_tab)
            .map(|tab| (tab.input_content.clone(), tab.input_cursor))
            .unwrap_or_default();
        let cursor = cursor.min(input.chars().count());
        let cursor_byte = char_to_byte_index(&input, cursor);
        let active_prefix = &input[..cursor_byte];
        let trimmed = active_prefix.trim_start();
        let token_start = active_prefix
            .char_indices()
            .rev()
            .find(|(_, character)| character.is_whitespace())
            .map(|(index, character)| index + character.len_utf8())
            .unwrap_or(0);
        if let Some(query) = active_prefix[token_start..].strip_prefix('@') {
            let query = query.to_ascii_lowercase();
            let prefix = &active_prefix[..token_start];
            let suffix = &input[cursor_byte..];
            let items = crate::skills::SkillCatalog::discover()
                .map(|catalog| {
                    catalog
                        .list()
                        .iter()
                        .filter(|skill| {
                            format!("{} {}", skill.name, skill.description)
                                .to_ascii_lowercase()
                                .contains(&query)
                        })
                        .map(|skill| {
                            crate::ui::modals::list_popup::ListPopupItem::action(
                                format!("@{} - {}", skill.name, skill.description),
                                crate::ui::modals::list_popup::ListPopupAction::ReplaceInput(
                                    format!("{prefix}@{} {suffix}", skill.name),
                                ),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            self.ui.list_popup = Some(
                crate::ui::modals::list_popup::ListPopup::anchored_selectable(
                    "Skills",
                    "No matching skills.",
                    items,
                    self.current_input_anchor(),
                ),
            );
            return;
        }
        if !trimmed.starts_with('/') {
            self.ui.list_popup = None;
            return;
        }

        if let Some(theme_query) = trimmed.strip_prefix("/theme") {
            if theme_query.is_empty() || theme_query.starts_with(char::is_whitespace) {
                self.show_theme_popup(theme_query);
                return;
            }
        }

        let filter = trimmed.trim_start_matches('/').to_ascii_lowercase();
        let commands = [
            ("/theme ", "Select and apply a theme"),
            ("/skills", "Show installed skills"),
            ("/mcp", "Show MCP servers"),
            ("/remindme ", "Schedule, list, or forget reminders"),
            ("/schedule-command ", "Schedule, list, or forget reminders"),
            ("/vault ", "Search the configured vault"),
            ("/web", "Toggle local web search"),
            ("/quit", "Quit the app"),
        ];
        let items = commands
            .into_iter()
            .filter_map(|(command, description)| {
                let command_name = command.trim_end();
                let haystack = format!("{command_name} {description}").to_ascii_lowercase();
                haystack.contains(&filter).then_some(
                    crate::ui::modals::list_popup::ListPopupItem::action(
                        format!("{command_name:<8} {description}"),
                        crate::ui::modals::list_popup::ListPopupAction::ReplaceInput(
                            command.to_string(),
                        ),
                    ),
                )
            })
            .collect();
        self.ui.list_popup = Some(
            crate::ui::modals::list_popup::ListPopup::anchored_selectable(
                "Commands",
                "No matching commands.",
                items,
                self.current_input_anchor(),
            ),
        );
    }

    fn apply_theme_selection(&mut self, theme_name: &str) -> color_eyre::Result<()> {
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
        if let Some(settings) = &mut self.ui.settings_popup {
            settings.theme = key.clone();
        }
        self.ui.connection_message = Some(format!("Theme: {label}"));
        Ok(())
    }

    fn load_settings_popup_state(
        &self,
        config: &AppConfig,
    ) -> crate::ui::settings_tab::SettingsPopup {
        let db_providers = Self::provider_entries_with_local(config, None);
        let saved_keys = crate::config::KeyStore::load_keys(config)
            .unwrap_or_default()
            .into_iter()
            .map(|(provider, key)| {
                let display_name = db_providers
                    .iter()
                    .find(|(name, _, _, _, _)| {
                        crate::llm::auth::canonical_provider_name(name)
                            == crate::llm::auth::canonical_provider_name(&provider)
                    })
                    .map(|(name, _, _, _, _)| name.clone())
                    .unwrap_or(provider);
                (display_name, key)
            })
            .collect();
        let available_models = self.fetch_models_for_settings(config);

        let providers_tab_list: Vec<crate::ui::settings_tab::EditableProvider> = db_providers
            .iter()
            .filter(|(name, _, _, _, _)| !crate::llm::local::is_local_provider(name))
            .map(
                |(name, endpoint, _, backend_type, _)| crate::ui::settings_tab::EditableProvider {
                    name: name.clone(),
                    endpoint: endpoint.clone(),
                    backend_type: backend_type.clone(),
                },
            )
            .collect();

        let small_model = config.small_model.clone().unwrap_or_default();
        let models_provider = if config.default_provider.is_empty() {
            String::new()
        } else {
            config.default_provider.clone()
        };
        let models_available_models = self.fetch_models_for_settings(config);
        let mut popup = crate::ui::settings_tab::SettingsPopup::new(
            crate::ui::settings_tab::SettingsPopupInit {
                default_provider: config.default_provider.clone(),
                default_model: config.default_model.clone(),
                small_model,
                use_env_keys: config.use_env_keys,
                saved_keys,
                user_alignment: config.user_alignment,
                ai_alignment: config.ai_alignment,
                theme: config.theme.clone(),
                markdown_mode: config.markdown_mode,
                artifact_save_dir: config.artifact_save_dir.clone().unwrap_or_default(),
                available_models,
                db_providers,
                show_selector: config.show_selector,
                show_chat_scrollbar: config.show_chat_scrollbar,
                collapse_thinking: config.collapse_thinking,
                kitty_enhanced_text: config.kitty_enhanced_text,
                kitty_text_max_scale: config.kitty_text_max_scale.clamp(1, 7),
                web_search_enabled: config.web_search.enabled,
                quit_confirmation: config.quit_confirmation,
                local_enabled: config.local_inference.enabled,
                local_host: config.local_inference.host.clone(),
                local_port: config.local_inference.port.to_string(),
                local_server_type: config.local_inference.server_type,
                local_selected_model: config.local_inference.selected_model.clone(),
                local_model_directory: config
                    .local_inference
                    .model_directory
                    .clone()
                    .unwrap_or_default(),
                local_health_interval_seconds: config
                    .local_inference
                    .health_check_interval_seconds
                    .to_string(),
                local_connect_timeout_ms: config.local_inference.connect_timeout_ms.to_string(),
                local_request_timeout_ms: config.local_inference.request_timeout_ms.to_string(),
                local_api_token_env: config
                    .local_inference
                    .api_token_env
                    .clone()
                    .unwrap_or_default(),
                detected_local_server: None,
                providers_tab_list,
                models_provider,
                models_available_models,
                mcp_servers: crate::mcp::merged_configs(&config.mcp_servers),
            },
        );
        popup.check_oauth_tokens();
        for name in &config.disabled_providers {
            if !name.trim().is_empty() {
                popup.disabled_providers.insert(name.trim().to_string());
            }
        }
        for name in &config.disabled_models {
            if !name.trim().is_empty() {
                popup.disabled_models.insert(name.trim().to_string());
            }
        }
        popup
    }

    async fn save_settings_popup_state(
        &mut self,
        settings: &crate::ui::settings_tab::SettingsPopup,
    ) -> color_eyre::Result<()> {
        let mut config = self
            .config
            .try_read()
            .map(|config| config.clone())
            .unwrap_or_default();
        config.use_env_keys = settings.use_env_keys;
        config.user_alignment = settings.user_alignment;
        config.ai_alignment = settings.ai_alignment;
        config.theme = crate::theme::canonical_theme_key(&settings.theme).to_string();
        config.markdown_mode = settings.markdown_mode;
        config.artifact_save_dir = (!settings.artifact_save_dir.trim().is_empty())
            .then_some(settings.artifact_save_dir.trim().to_string());
        config.default_provider = settings.default_provider.clone();
        config.default_model = settings.default_model.clone();
        config.small_model =
            (!settings.small_model.trim().is_empty()).then_some(settings.small_model.clone());
        config.show_selector = settings.show_selector;
        config.show_chat_scrollbar = settings.show_chat_scrollbar;
        config.collapse_thinking = settings.collapse_thinking;
        config.kitty_enhanced_text = settings.kitty_enhanced_text;
        config.kitty_text_max_scale = settings.kitty_text_max_scale.clamp(1, 7);
        config.quit_confirmation = settings.quit_confirmation;
        config.web_search.enabled = settings.web_search_enabled;
        config.mcp_servers = settings.mcp_servers.clone();
        config.local_inference.enabled = settings.local_enabled;
        config.local_inference.host = settings.local_host.trim().to_string();
        config.local_inference.port = settings
            .local_port
            .trim()
            .parse::<u16>()
            .unwrap_or(config.local_inference.port.max(1));
        config.local_inference.server_type = settings.local_server_type;
        config.local_inference.selected_model = settings.local_selected_model.trim().to_string();
        config.local_inference.model_directory =
            (!settings.local_model_directory.trim().is_empty())
                .then_some(settings.local_model_directory.trim().to_string());
        config.local_inference.health_check_interval_seconds = settings
            .local_health_interval_seconds
            .trim()
            .parse::<u64>()
            .unwrap_or(config.local_inference.health_check_interval_seconds.max(1));
        config.local_inference.connect_timeout_ms = settings
            .local_connect_timeout_ms
            .trim()
            .parse::<u64>()
            .unwrap_or(config.local_inference.connect_timeout_ms.max(250));
        config.local_inference.request_timeout_ms = settings
            .local_request_timeout_ms
            .trim()
            .parse::<u64>()
            .unwrap_or(config.local_inference.request_timeout_ms.max(500));
        config.local_inference.api_token_env = (!settings.local_api_token_env.trim().is_empty())
            .then_some(settings.local_api_token_env.trim().to_string());
        config.disabled_providers = settings.disabled_providers.iter().cloned().collect();
        config.disabled_models = settings.disabled_models.iter().cloned().collect();
        config.providers = settings
            .db_providers
            .iter()
            .filter(|(name, _, _, _, _)| !crate::llm::local::is_local_provider(name))
            .map(|(name, endpoint, env_var, backend_type, auth_type)| {
                crate::config::ProviderConfig {
                    name: name.clone(),
                    endpoint: endpoint.clone(),
                    env_var: env_var.clone(),
                    backend_type: backend_type.clone(),
                    auth_type: auth_type.clone(),
                }
            })
            .collect();

        config.save()?;
        crate::config::KeyStore::save_keys(
            &config,
            &settings
                .saved_keys
                .iter()
                .filter(|(provider, _)| !crate::llm::auth::is_oauth_provider(provider))
                .map(|(provider, key)| {
                    (
                        if provider.ends_with(" Search") {
                            provider.clone()
                        } else {
                            crate::llm::auth::canonical_provider_name(provider)
                        },
                        key.clone(),
                    )
                })
                .collect::<Vec<_>>(),
        )?;
        self.storage.sync_providers(&config.providers)?;
        self.ui.db_providers = Self::provider_entries_with_local(&config, None);
        self.ui.disabled_providers = settings.disabled_providers.clone();
        self.ui.disabled_models = settings.disabled_models.clone();
        if let Ok(mut live_config) = self.config.try_write() {
            *live_config = config;
        }
        self.refresh_visible_selectors();
        crate::theme::set_active_theme(&settings.theme);
        Ok(())
    }

    async fn toggle_web_search(&mut self) -> color_eyre::Result<()> {
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

    fn open_external_target(&mut self, target: &str) {
        let target = target.trim();
        if target.is_empty() {
            return;
        }
        if let Some(skill) = target.strip_prefix("skill:") {
            self.show_skill_popup(skill);
            return;
        }
        let command = if cfg!(target_os = "macos") {
            ("open", vec![target])
        } else {
            ("xdg-open", vec![target])
        };
        let status = std::process::Command::new(command.0)
            .args(command.1)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        self.ui.connection_status = crate::ui::status_bar::ConnectionStatus::CloudConnected;
        self.ui.connection_message = Some(match status {
            Ok(_) => format!("Opened {}", target),
            Err(_) => format!("Could not open {}", target),
        });
    }

    fn apply_settings_provider_action(&mut self, action: crate::ui::settings_tab::ProvidersAction) {
        match action {
            crate::ui::settings_tab::ProvidersAction::None => {}
            crate::ui::settings_tab::ProvidersAction::ToggleUseEnv => {
                if let Some(settings) = &mut self.ui.settings_popup {
                    if settings.use_env_keys {
                        settings.grab_keys_from_env();
                    }
                }
            }
            crate::ui::settings_tab::ProvidersAction::RefreshModels => {
                let Ok(storage) = crate::storage::Storage::new() else {
                    return;
                };
                tokio::task::spawn_blocking(move || {
                    let rt = tokio::runtime::Handle::current();
                    rt.block_on(async {
                        crate::llm::model_fetcher::refresh_all_models(&storage).await;
                    });
                });
            }
            crate::ui::settings_tab::ProvidersAction::SubmitAdd { provider, api_key } => {
                if let Some(settings) = &mut self.ui.settings_popup {
                    settings.apply_add_provider(provider.clone(), api_key.clone());
                }
                let name = provider.name.clone();
                let endpoint = provider.endpoint.clone();
                let env_var = format!("{}_API_KEY", provider.name.to_uppercase().replace(' ', "_"));
                let backend_type = provider.backend_type.clone();
                tokio::task::spawn_blocking(move || {
                    let rt = tokio::runtime::Handle::current();
                    rt.block_on(async {
                        if let Ok(storage) = crate::storage::Storage::new() {
                            crate::llm::model_fetcher::refresh_provider_models(
                                &storage,
                                &name,
                                &endpoint,
                                &env_var,
                                &backend_type,
                            )
                            .await;
                        }
                    });
                });
            }
            crate::ui::settings_tab::ProvidersAction::SubmitEdit {
                original_name,
                provider,
                api_key,
            } => {
                if let Some(settings) = &mut self.ui.settings_popup {
                    settings.apply_update_provider(
                        &original_name,
                        provider.clone(),
                        api_key.clone(),
                    );
                }
                let name = provider.name.clone();
                let endpoint = provider.endpoint.clone();
                let env_var = format!("{}_API_KEY", provider.name.to_uppercase().replace(' ', "_"));
                let backend_type = provider.backend_type.clone();
                tokio::task::spawn_blocking(move || {
                    let rt = tokio::runtime::Handle::current();
                    rt.block_on(async {
                        if let Ok(storage) = crate::storage::Storage::new() {
                            crate::llm::model_fetcher::refresh_provider_models(
                                &storage,
                                &name,
                                &endpoint,
                                &env_var,
                                &backend_type,
                            )
                            .await;
                        }
                    });
                });
            }
            crate::ui::settings_tab::ProvidersAction::DeleteProvider(name) => {
                if let Some(settings) = &mut self.ui.settings_popup {
                    settings.remove_provider_by_name(&name);
                }
            }
            crate::ui::settings_tab::ProvidersAction::SavePresetKey {
                provider_name,
                api_key,
            } => {
                if crate::llm::auth::is_oauth_provider(&provider_name) {
                    return;
                }
                if let Some(settings) = &mut self.ui.settings_popup {
                    settings.apply_preset_key_save(provider_name, api_key);
                }
            }
        }
    }
}

fn local_media_sources(content: &str) -> Vec<String> {
    let mut sources = Vec::new();
    let mut seen = HashSet::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("![") && trimmed.ends_with(')') {
            if let Some(start) = trimmed.find("](") {
                let source = trimmed[start + 2..trimmed.len().saturating_sub(1)]
                    .trim()
                    .trim_matches('<')
                    .trim_matches('>');
                if crate::ui::components::image_block::is_local_image_source(source)
                    && seen.insert(source.to_string())
                {
                    sources.push(source.to_string());
                }
            }
            continue;
        }

        if crate::ui::components::image_block::is_local_image_source(trimmed)
            && seen.insert(trimmed.to_string())
        {
            sources.push(trimmed.to_string());
        }
    }

    sources
}

fn char_to_byte_index(text: &str, char_idx: usize) -> usize {
    if char_idx == 0 {
        return 0;
    }
    text.char_indices()
        .nth(char_idx)
        .map(|(idx, _)| idx)
        .unwrap_or(text.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn env_lock() -> &'static Mutex<()> {
        crate::test_support::env_lock()
    }

    fn unique_temp_dir(label: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("tcui-{label}-{}-{nanos}", std::process::id()))
    }

    fn with_test_app(label: &str, test: impl FnOnce(&mut TuiApp)) {
        let _guard = env_lock().lock().expect("env lock poisoned");
        let root = unique_temp_dir(label);
        let data_home = root.join("data-home");
        let config_home = root.join("config-home");
        std::fs::create_dir_all(&data_home).expect("create data dir");
        std::fs::create_dir_all(&config_home).expect("create config dir");
        std::env::set_var("XDG_DATA_HOME", &data_home);
        std::env::set_var("XDG_CONFIG_HOME", &config_home);

        let storage = Arc::new(Storage::new().expect("create storage"));
        let mut app = TuiApp::new(
            storage,
            Arc::new(tokio::sync::RwLock::new(AppConfig::default())),
            Arc::new(LlmClient::new()),
            None,
        );
        test(&mut app);

        std::fs::remove_dir_all(&root).expect("cleanup temp dir");
        std::env::remove_var("XDG_DATA_HOME");
        std::env::remove_var("XDG_CONFIG_HOME");
    }

    #[test]
    fn mouse_wheel_moves_open_list_popup_before_settings() {
        with_test_app("popup-wheel", |app| {
            // Given
            app.ui.last_area = Some(Rect::new(0, 0, 80, 24));
            app.ui.show_settings = true;
            app.ui.list_popup = Some(crate::ui::modals::list_popup::ListPopup::selectable(
                "Skills",
                "Empty",
                (0..20)
                    .map(|idx| {
                        crate::ui::modals::list_popup::ListPopupItem::insert(format!(
                            "@skill-{idx} "
                        ))
                    })
                    .collect(),
            ));

            // When
            for _ in 0..15 {
                app.handle_mouse(crossterm::event::MouseEvent {
                    kind: crossterm::event::MouseEventKind::ScrollDown,
                    column: 0,
                    row: 0,
                    modifiers: crossterm::event::KeyModifiers::NONE,
                });
            }

            // Then
            let popup = app.ui.list_popup.as_ref().expect("list popup remains open");
            assert_eq!(popup.selected, Some(15));
            assert_eq!(popup.scroll, 2);
            assert_eq!(app.ui.tabs[0].scroll_offset, 0);
        });
    }

    #[test]
    fn slash_routes_keep_vault_and_remove_local() {
        with_test_app("slash-routes", |app| {
            // Given
            app.ui.tabs[0].input_content = "/vault release notes".to_string();

            // When
            let vault_action = app.handle_key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::NONE,
            ));

            // Then
            assert!(matches!(
                vault_action,
                Some(Action::ShowLocalSearch(query)) if query == "release notes"
            ));

            app.ui.tabs[0].input_content = "/local".to_string();
            assert!(matches!(
                app.handle_key(crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Enter,
                    crossterm::event::KeyModifiers::NONE,
                )),
                Some(Action::SendMessage(message)) if message == "/local"
            ));
        });
    }

    #[test]
    fn skills_popup_labels_description_and_origin_but_inserts_only_name() {
        with_test_app("skills-popup", |app| {
            // Given / When
            app.show_skills_popup();

            // Then
            let popup = app.ui.list_popup.as_ref().expect("skills popup");
            let save = popup
                .items
                .iter()
                .find(|item| item.label.starts_with("@save "))
                .expect("save skill");
            assert!(save.label.contains("Markdown artifact"));
            assert!(save.label.contains("[built-in]"));
            assert_eq!(
                save.action,
                Some(crate::ui::modals::list_popup::ListPopupAction::InsertText(
                    "@save ".to_string()
                ))
            );
        });
    }

    #[test]
    fn startup_uses_saved_default_provider_and_model() {
        let _guard = env_lock().lock().expect("env lock poisoned");
        let root = unique_temp_dir("startup-defaults");
        let data_home = root.join("data-home");
        let config_home = root.join("config-home");
        std::fs::create_dir_all(&root).expect("create root dir");
        std::fs::create_dir_all(&data_home).expect("create data dir");
        std::fs::create_dir_all(&config_home).expect("create config dir");
        std::env::set_var("XDG_DATA_HOME", &data_home);
        std::env::set_var("XDG_CONFIG_HOME", &config_home);
        let original_dir = std::env::current_dir().expect("current dir");
        std::env::set_current_dir(&root).expect("set current dir");

        std::fs::write(
            root.join("config.toml"),
            r#"
default_provider = "OpenCode Go"
default_model = "deepseek-v4-flash"
"#,
        )
        .expect("write config");

        let storage = Arc::new(Storage::new().expect("create storage"));

        let mut app = TuiApp::new(
            storage,
            Arc::new(tokio::sync::RwLock::new(
                AppConfig::load().expect("load config"),
            )),
            Arc::new(LlmClient::new()),
            None,
        );

        assert_eq!(app.ui.tabs[0].tab.provider, "OpenCode Go");
        assert_eq!(app.ui.tabs[0].tab.model, "deepseek-v4-flash");
        app.ui.show_settings = true;
        assert!(matches!(
            app.handle_key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyModifiers::NONE,
            )),
            Some(Action::CloseSettings)
        ));

        std::env::set_current_dir(original_dir).expect("restore current dir");
        std::fs::remove_dir_all(&root).expect("cleanup temp dir");
        std::env::remove_var("XDG_DATA_HOME");
        std::env::remove_var("XDG_CONFIG_HOME");
    }

    #[test]
    fn startup_adds_local_inference_provider_when_enabled() {
        let _guard = env_lock().lock().expect("env lock poisoned");
        let root = unique_temp_dir("startup-local");
        let data_home = root.join("data-home");
        let config_home = root.join("config-home");
        std::fs::create_dir_all(&root).expect("create root dir");
        std::fs::create_dir_all(&data_home).expect("create data dir");
        std::fs::create_dir_all(&config_home).expect("create config dir");
        std::env::set_var("XDG_DATA_HOME", &data_home);
        std::env::set_var("XDG_CONFIG_HOME", &config_home);
        let original_dir = std::env::current_dir().expect("current dir");
        std::env::set_current_dir(&root).expect("set current dir");

        std::fs::write(
            root.join("config.toml"),
            r#"
default_provider = "Local Inference"
default_model = ""

[local_inference]
enabled = true
host = "127.0.0.1"
port = 11434
server_type = "auto"
selected_model = "llama3.1"
"#,
        )
        .expect("write config");

        let storage = Arc::new(Storage::new().expect("create storage"));
        let app = TuiApp::new(
            storage,
            Arc::new(tokio::sync::RwLock::new(
                AppConfig::load().expect("load config"),
            )),
            Arc::new(LlmClient::new()),
            None,
        );

        assert!(app
            .ui
            .db_providers
            .iter()
            .any(|(name, _, _, _, _)| name == crate::config::LOCAL_PROVIDER_NAME));
        assert_eq!(
            app.ui.tabs[0].tab.provider,
            crate::config::LOCAL_PROVIDER_NAME
        );
        assert_eq!(app.ui.tabs[0].tab.model, "llama3.1");

        std::env::set_current_dir(original_dir).expect("restore current dir");
        std::fs::remove_dir_all(&root).expect("cleanup temp dir");
        std::env::remove_var("XDG_DATA_HOME");
        std::env::remove_var("XDG_CONFIG_HOME");
    }

    #[test]
    fn typing_slash_opens_command_popup() {
        with_test_app("slash-popup", |app| {
            app.handle_key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('/'),
                crossterm::event::KeyModifiers::NONE,
            ));

            let popup = app.ui.list_popup.as_ref().expect("command popup");
            assert_eq!(popup.title, "Commands");
            assert!(popup.items.iter().any(|item| item.label.contains("/theme")));
            assert!(popup
                .items
                .iter()
                .any(|item| item.label.contains("/remindme")));
        });
    }

    #[test]
    fn slash_popup_allows_continued_typing() {
        with_test_app("slash-popup-typing", |app| {
            app.handle_key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('/'),
                crossterm::event::KeyModifiers::NONE,
            ));
            app.handle_key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('t'),
                crossterm::event::KeyModifiers::NONE,
            ));

            let input = &app.ui.tabs[0].input_content;
            let popup = app.ui.list_popup.as_ref().expect("command popup");
            assert_eq!(input, "/t");
            assert_eq!(popup.title, "Commands");
        });
    }

    #[test]
    fn arrow_keys_move_cursor_and_insert_in_place() {
        with_test_app("input-cursor", |app| {
            app.handle_key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('a'),
                crossterm::event::KeyModifiers::NONE,
            ));
            app.handle_key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('c'),
                crossterm::event::KeyModifiers::NONE,
            ));
            app.handle_key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Left,
                crossterm::event::KeyModifiers::NONE,
            ));
            app.handle_key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('b'),
                crossterm::event::KeyModifiers::NONE,
            ));

            let tab = &app.ui.tabs[0];
            assert_eq!(tab.input_content, "abc");
            assert_eq!(tab.input_cursor, 2);
        });
    }

    #[test]
    fn alt_scrolls_lines_and_shift_jumps_answers() {
        with_test_app("answer-jump", |app| {
            let tab = &mut app.ui.tabs[0];
            tab.scroll_offset = 4;
            tab.total_rendered_lines = 60;
            tab.message_viewport_height = 10;
            tab.messages = vec![
                crate::app::message::Message::new(1, "user".to_string(), "Q1".to_string()),
                crate::app::message::Message::new(1, "assistant".to_string(), "A1".to_string()),
                crate::app::message::Message::new(1, "user".to_string(), "Q2".to_string()),
                crate::app::message::Message::new(1, "assistant".to_string(), "A2".to_string()),
            ];
            tab.answer_anchor_lines = vec![(1, 3), (3, 20)];

            app.handle_key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Down,
                crossterm::event::KeyModifiers::ALT,
            ));
            assert_eq!(app.ui.tabs[0].scroll_offset, 7);

            app.handle_key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Down,
                crossterm::event::KeyModifiers::SHIFT,
            ));
            assert_eq!(app.ui.tabs[0].scroll_to_message, Some(3));
        });
    }

    #[cfg(feature = "memory")]
    #[test]
    fn inline_at_completion_discovers_memory_skills() {
        with_test_app("memory-skill-popup", |app| {
            // Given
            app.ui.tabs[0].input_content = "Use @rem".to_string();
            app.ui.tabs[0].input_cursor = app.ui.tabs[0].input_content.chars().count();

            // When
            app.refresh_input_popup();

            // Then
            let popup = app.ui.list_popup.as_ref().expect("skill completion popup");
            assert_eq!(popup.title, "Skills");
            assert!(popup
                .items
                .iter()
                .any(|item| item.label.contains("@remember")));
        });
    }

    #[test]
    fn theme_command_persists_selection() {
        with_test_app("theme-command", |app| {
            if let Some(tab) = app.ui.tabs.get_mut(0) {
                tab.input_content = "/theme nord".to_string();
            }

            let action = app.handle_key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::NONE,
            ));

            assert!(action.is_none());
            let live_theme = app
                .config
                .try_read()
                .map(|config| config.theme.clone())
                .unwrap_or_default();
            assert_eq!(live_theme, "nord");
        });
    }

    #[test]
    fn slash_remindme_is_handled_without_model_streaming() {
        with_test_app("slash-remindme", |app| {
            std::env::set_var("TCUI_REMINDER_SYSTEMD_RUN", "true");

            app.send_message("/remindme in 10m | Stretch".to_string())
                .expect("schedule reminder");

            let tab = &app.ui.tabs[0];
            assert_eq!(tab.messages.len(), 2);
            assert_eq!(tab.messages[0].role, "user");
            assert_eq!(tab.messages[1].role, "assistant");
            assert!(tab.messages[1]
                .content
                .contains("Scheduled one-shot reminder"));

            std::env::remove_var("TCUI_REMINDER_SYSTEMD_RUN");
        });
    }

    #[tokio::test]
    async fn oauth_connection_check_skips_models_probe() {
        let mut config = AppConfig::default();
        config.default_provider = "Codex".to_string();

        let result = TuiApp::check_cloud_connection("Codex", &config, Some("token")).await;

        assert!(result.is_ok());
    }
}
