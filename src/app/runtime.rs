use futures::StreamExt;
use secrecy::ExposeSecret;
use std::io::IsTerminal;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use super::{Action, Message, TuiApp};
use crate::app::action::MouseClickAction;

impl TuiApp {
    pub(crate) fn quit_requires_confirmation(&self) -> bool {
        self.config
            .try_read()
            .map(|config| config.quit_confirmation)
            .unwrap_or(true)
    }

    pub(crate) fn quit_action(&self) -> Action {
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
                _ = tick.tick() => {
                    if let Some(editor) = self.ui.editor_popup.as_mut() {
                        let done = editor.poll_output();
                        if done {
                            let chat_draft_path = editor.take_chat_draft_path();
                            self.ui.editor_popup = None;
                            if let Some(path) = chat_draft_path {
                                self.apply_chat_draft_from_path(&path);
                            }
                        }
                    }
                }
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
            Action::ToggleWebSearch => {
                self.toggle_web_search().await?;
                let state = if self.ui.web_search_enabled {
                    "enabled"
                } else {
                    "disabled"
                };
                self.ui
                    .toast_stack
                    .push_message(format!("Web Search {state}"), self.ui.frame_tick);
                Ok(())
            }
            Action::ToggleCollapseThinking => self.toggle_collapse_thinking(),
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
            Action::ShowHelp => {
                let artifact = crate::ui::artifact_sidebar::ArtifactEntry::temp_markdown(
                    u64::MAX,
                    "Help".to_string(),
                    crate::ui::HELP_MARKDOWN.to_string(),
                );
                let mut viewer =
                    crate::ui::modals::artifact_viewer::ArtifactViewerState::new(artifact);
                viewer.view_only = true;
                self.ui.artifact_viewer = Some(viewer);
                Ok(())
            }
            Action::ShowKeybinds => {
                self.ui.show_keybinds = true;
                Ok(())
            }
            Action::DismissKeybinds => {
                self.ui.show_keybinds = false;
                Ok(())
            }
            Action::CloseListPopup => {
                self.ui.list_popup = None;
                Ok(())
            }
            Action::ToggleSidebar => {
                self.ui.panel_state.toggle_left();
                self.ui.sidebar_open =
                    self.ui.panel_state.left != crate::tui::shell::PanelMode::Closed;
                if !self.ui.sidebar_open {
                    self.ui.focus = crate::tui::focus::Focus::Chat;
                }
                Ok(())
            }
            Action::FocusInput => {
                self.ui.app_tabs.select(0);
                self.ui.focus = crate::tui::focus::Focus::Chat;
                Ok(())
            }
            Action::ToggleArtifactSidebar => {
                self.ui.panel_state.toggle_right();
                self.ui.artifact_sidebar_open =
                    self.ui.panel_state.right != crate::tui::shell::PanelMode::Closed;
                Ok(())
            }
            Action::RefreshArtifactSidebar => {
                self.refresh_artifact_sidebar_catalogs();
                Ok(())
            }
            Action::ShowSettings => {
                self.ui.palette = None;
                self.ui.settings_v2 = Some(crate::tui::settings_panel::SettingsPanelState::new());
                Ok(())
            }
            Action::CloseSettings => {
                self.ui.settings_v2 = None;
                Ok(())
            }
            Action::ToggleSettings => {
                self.ui.palette = None;
                if self.ui.settings_v2.is_some() {
                    self.ui.settings_v2 = None;
                } else {
                    self.ui.settings_v2 =
                        Some(crate::tui::settings_panel::SettingsPanelState::new());
                }
                Ok(())
            }
            Action::NewChat => self.new_conversation(self.ui.active_tab),
            Action::CloseChat => {
                let tab_id = self.ui.active_tab;
                let active_conversation = self
                    .ui
                    .tabs
                    .get(tab_id)
                    .map(|tab| tab.active_conversation)
                    .unwrap_or(0);
                if active_conversation != 0 {
                    self.storage.delete_conversation(active_conversation)?;
                }
                self.ensure_tab_has_active_conversation(tab_id)
            }
            Action::ToggleConversationPinned(conversation_id) => {
                self.toggle_conversation_pinned(conversation_id)
            }
            Action::DeleteConversation(conversation_id) => {
                self.delete_conversation_by_id(conversation_id)
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
            Action::FinalizeAssistantMessage(tab_id, message_idx, message) => {
                if let Some(tab) = self.ui.tabs.get_mut(tab_id) {
                    if let Some(existing) = tab.messages.get_mut(message_idx) {
                        *existing = message;
                    }
                    tab.scroll_to_message = Some(message_idx);
                }
                self.persist_active_conversation(tab_id)?;
                self.refresh_tab_conversations(tab_id)?;
                Ok(())
            }
            Action::StopStream(tab_id) => {
                self.ui.finish_stream(tab_id);
                self.refresh_tab_conversations(tab_id)?;
                Ok(())
            }
            Action::AddMessage(tab_id, msg) => {
                let mut stored = msg;
                if let Ok(message_id) = self.storage.save_message(&stored) {
                    stored.id = Some(message_id);
                }
                self.ui.add_message(tab_id, stored);
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
            Action::ShowToast(message) => {
                self.ui.show_toast(message);
                Ok(())
            }
            Action::SetConnectionState(status, message) => {
                self.set_connection_state(status, message);
                Ok(())
            }
            Action::SetTitle(title) => {
                if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
                    tab.generated_title = Some(title.clone());
                    if tab.active_conversation != 0 {
                        self.storage
                            .update_conversation_title(tab.active_conversation, &title)?;
                    }
                }
                self.ui.set_title(title);
                self.refresh_tab_conversations(self.ui.active_tab)?;
                Ok(())
            }
            Action::SwitchTab(idx) => {
                self.ui.active_tab = idx;
                self.ui.settings_v2 = None;
                self.refresh_visible_selectors();
                Ok(())
            }
            Action::AddTab(tab) => {
                self.ui.add_tab(tab);
                self.ui.settings_v2 = None;
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
                    tab.tab.provider = provider.clone();
                    tab.tab.model.clear();
                    tab.tab.reasoning_effort = None;
                }
                self.refresh_visible_selectors();
                self.queue_connection_check_for_active_tab();
                self.ui
                    .toast_stack
                    .push_message(format!("Switched to {provider}"), self.ui.frame_tick);
                Ok(())
            }
            Action::UpdateTabModel(tab_id, model) => {
                if let Some(tab) = self.ui.tabs.get_mut(tab_id) {
                    tab.tab.model = model.clone();
                    tab.tab.reasoning_effort = None;
                }
                self.refresh_visible_selectors();
                self.queue_connection_check_for_active_tab();
                self.ui
                    .toast_stack
                    .push_message(format!("Model: {model}"), self.ui.frame_tick);
                Ok(())
            }
            Action::SetProviderModels(provider, _models) => {
                if self
                    .ui
                    .tabs
                    .get(self.ui.active_tab)
                    .map(|tab| tab.tab.provider == provider)
                    .unwrap_or(false)
                {
                    self.refresh_visible_selectors();
                }

                Ok(())
            }
            Action::SaveApiKey(_provider, _key) => Ok(()),
            Action::MouseClick(action) => self.dispatch_mouse_click(action).await,
            Action::SaveGeneratedFile => self.save_generated_file(),
            Action::SaveExportDialog => self.save_export_dialog(),
            Action::CancelSaveDialog => {
                self.ui.save_file_dialog = None;
                self.ui.export_dialog = None;
                Ok(())
            }
            Action::ExportConversation => {
                self.open_conversation_export_dialog();
                Ok(())
            }
            Action::ExportConversationId(conversation_id) => {
                self.open_conversation_export_dialog_for(conversation_id);
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
            Action::OpenCommandPalette => {
                self.ui.palette = None;
                self.ui.settings_v2 = Some(crate::tui::settings_panel::SettingsPanelState::new());
                Ok(())
            }
            Action::OpenSettingsPanel => {
                let _focus = crate::tui::focus::Focus::SettingsPanel;
                self.ui.palette = None;
                self.ui.settings_v2 = Some(crate::tui::settings_panel::SettingsPanelState::new());
                Ok(())
            }
            Action::SetPinnedCommands(pinned) => {
                if let Some(palette) = self.ui.palette.as_mut() {
                    palette.set_pinned(pinned);
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    pub(crate) fn send_message(&mut self, content: String) -> color_eyre::Result<()> {
        if tokio::runtime::Handle::try_current().is_err() {
            return Err(color_eyre::eyre::eyre!(
                "Cannot send a message without a Tokio runtime"
            ));
        }
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

        let mut msg = Message::new(conversation_id, role, content.clone());
        if let Ok(message_id) = self.storage.save_message(&msg) {
            msg.id = Some(message_id);
        }
        self.ui.add_message(self.ui.active_tab, msg.clone());

        // On first message, create a conversation entry and generate title
        if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
            if tab.messages.len() == 1 && tab.generated_title.is_none() {
                let title = Self::generate_title(&content);
                tab.generated_title = Some(title.clone());
                self.storage
                    .update_conversation_title(tab.active_conversation, &title)?;
            }
        }
        self.refresh_tab_conversations(self.ui.active_tab)?;

        let tab_id = self.ui.active_tab;
        let Some(tab_state) = self.ui.tabs.get(tab_id) else {
            return Ok(());
        };
        let provider = tab_state.tab.provider.clone();
        let is_local_provider = crate::llm::local::is_local_provider(&provider);
        let model = tab_state.tab.model.clone();
        let reasoning_effort = tab_state.tab.reasoning_effort.clone();
        let supported_reasoning_efforts = self
            .ui
            .current_models
            .iter()
            .find(|entry| entry.id == model)
            .map(|entry| entry.supported_reasoning_efforts.clone())
            .unwrap_or_default();
        let messages = tab_state.messages.clone();
        let config_snapshot = self
            .config
            .try_read()
            .map(|config| config.clone())
            .unwrap_or_default();
        if let Some(response) = crate::reminders::maybe_handle_request(&config_snapshot, &content) {
            let mut assistant = Message::new(conversation_id, "assistant".to_string(), response);
            if let Ok(message_id) = self.storage.save_message(&assistant) {
                assistant.id = Some(message_id);
            }
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
        let local_api_key = if is_local_provider {
            config_snapshot
                .local_inference
                .api_token_env
                .as_deref()
                .and_then(|env_var| std::env::var(env_var).ok())
                .filter(|value| !value.trim().is_empty())
        } else {
            None
        };
        let credential_env_var = provider_config
            .as_ref()
            .map(|(_, env_var, _)| env_var.clone());
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
        if is_local_provider {
            self.ui.connection_status = crate::ui::status_bar::ConnectionStatus::LocalConnected;
            self.ui.connection_message = Some("Connected to Local LLM".to_string());
        } else {
            self.ui.connection_status = crate::ui::status_bar::ConnectionStatus::CloudConnected;
            self.ui.connection_message = Some(format!("{provider} / {model}"));
        }

        tokio::spawn(async move {
            let api_key = if is_local_provider {
                local_api_key
            } else {
                match credential_env_var {
                    Some(env_var) => {
                        let credential_request = crate::llm::auth::CredentialRequest::new(
                            &provider, &env_var, &endpoint,
                        );
                        match crate::llm::auth::resolve_provider_credential(credential_request)
                            .await
                        {
                            Ok(Some(credential)) => {
                                Some(credential.bearer_token().expose_secret().to_owned())
                            }
                            Ok(None) => None,
                            Err(error) => {
                                let _ = action_tx.send(Action::StreamResponse(
                                    tab_id,
                                    assistant_idx,
                                    format!("Credential unavailable: {error}"),
                                ));
                                let _ = action_tx.send(Action::StopStream(tab_id));
                                return;
                            }
                        }
                    }
                    None => None,
                }
            };
            if !is_local_provider && !provider.eq_ignore_ascii_case("Ollama") && api_key.is_none() {
                let _ = action_tx.send(Action::StreamResponse(
                    tab_id,
                    assistant_idx,
                    format!("No API key or OAuth token found for {provider}. Open Settings > Providers or set the provider env var."),
                ));
                let _ = action_tx.send(Action::StopStream(tab_id));
                return;
            }
            let event_tx = action_tx.clone();
            let mut runtime_system_prompt = system_prompt.clone();
            let mut request = crate::llm::chat::ChatRequest {
                provider,
                endpoint,
                model,
                reasoning_effort,
                supported_reasoning_efforts,
                backend_type,
                api_key,
                system_prompt: runtime_system_prompt.clone(),
                messages,
            };
            let skills =
                crate::skill_runtime::prepare(&config_snapshot, &user_request, &request).await;
            runtime_system_prompt.push_str(&skills.context);
            let now = chrono::Utc::now().format("%Y-%m-%d %H:%M UTC");
            runtime_system_prompt.push_str(&format!(
                "\n\nCurrent date and time: {now}. Use this when referencing relative dates like 'today' or 'yesterday'."
            ));
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
                            let refresh =
                                matches!(activity, crate::memory::MemoryActivity::Saved { .. });
                            memory_activities.push(activity);
                            let _ = action_tx.send(Action::SetMemoryActivities(
                                tab_id,
                                assistant_idx,
                                memory_activities.clone(),
                            ));
                            if refresh {
                                let _ = action_tx.send(Action::RefreshArtifactSidebar);
                            }
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
                crate::llm::chat::ChatStreamEvent::Title(title) => {
                    let _ = event_tx.send(Action::SetTitle(title));
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
                for issue in filtered.issues {
                    let message = match issue {
                        crate::memory::RememberDirectiveIssue::UnterminatedOpen => {
                            "Memory was not saved: <tcui:remember> was started but never closed."
                        }
                        crate::memory::RememberDirectiveIssue::ClosingWithoutOpening => {
                            "Memory was not saved: </tcui:remember> appeared without a matching start tag."
                        }
                    };
                    let _ = action_tx.send(Action::UpdateStatus(message.to_string()));
                }
                filtered.memory
            };

            match result {
                Ok(response) if !visible_answer.is_empty() || !response.thinking.is_empty() => {
                    let file_id = rand::random();
                    let mut generated_file = crate::app::GeneratedFile::from_skill_response(
                        file_id,
                        conversation_id,
                        &user_request,
                        &visible_answer,
                    );
                    if let Some(file) = generated_file.take() {
                        let configured = config_snapshot
                            .artifact_save_dir
                            .as_deref()
                            .map(std::path::Path::new)
                            .map(|path| {
                                crate::app::generated_file::expand_user_path(
                                    path,
                                    dirs::home_dir().as_deref(),
                                )
                            })
                            .unwrap_or_else(|| {
                                dirs::data_dir()
                                    .unwrap_or_else(|| std::path::PathBuf::from("."))
                                    .join("tcui/artifacts")
                            });
                        match file.clone().save_to(&configured) {
                            Ok(saved) => generated_file = Some(saved),
                            Err(error) => {
                                generated_file = Some(file);
                                let _ = action_tx.send(Action::UpdateStatus(format!(
                                    "Artifact save failed: {error}"
                                )));
                            }
                        }
                    }
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
                                let _ = action_tx.send(Action::RefreshArtifactSidebar);
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
                    let assistant_content = visible_answer.clone();
                    let mut msg =
                        Message::new(conversation_id, "assistant".to_string(), assistant_content);
                    msg.thinking_content =
                        (!response.thinking.is_empty()).then_some(response.thinking);
                    msg.token_count = response.total_tokens;
                    #[cfg(feature = "memory")]
                    let _ = crate::memory::set_activities(&mut msg, &memory_activities);
                    let _ = action_tx.send(Action::FinalizeAssistantMessage(
                        tab_id,
                        assistant_idx,
                        msg,
                    ));
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

    async fn dispatch_mouse_click(&mut self, action: MouseClickAction) -> color_eyre::Result<()> {
        match action {
            MouseClickAction::SelectPaletteItem(index) => {
                if let Some(palette) = self.ui.palette.as_mut() {
                    palette.select(index);
                }
                Ok(())
            }
            MouseClickAction::ToggleLeftHandle => {
                self.ui.panel_state.toggle_left();
                self.ui.sidebar_open =
                    self.ui.panel_state.left != crate::tui::shell::PanelMode::Closed;
                if !self.ui.sidebar_open {
                    self.ui.focus = crate::tui::focus::Focus::Chat;
                }
                Ok(())
            }
            MouseClickAction::ToggleRightHandle => {
                self.ui.panel_state.toggle_right();
                self.ui.artifact_sidebar_open =
                    self.ui.panel_state.right != crate::tui::shell::PanelMode::Closed;
                Ok(())
            }
            MouseClickAction::CloseRightSidebar => {
                self.ui.panel_state.right = crate::tui::shell::PanelMode::Closed;
                self.ui.artifact_sidebar_open = false;
                Ok(())
            }
            MouseClickAction::SetRightSidebarThin => {
                self.ui.panel_state.right = crate::tui::shell::PanelMode::Thin;
                self.ui.artifact_sidebar_open = true;
                Ok(())
            }
            MouseClickAction::OpenProviderDropdown => {
                if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
                    tab.provider_dropdown_open = !tab.provider_dropdown_open;
                    tab.model_dropdown_open = false;
                    tab.reasoning_dropdown_open = false;
                }
                Ok(())
            }
            MouseClickAction::OpenModelDropdown => {
                if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
                    tab.model_dropdown_open = !tab.model_dropdown_open;
                    tab.provider_dropdown_open = false;
                    tab.reasoning_dropdown_open = false;
                }
                Ok(())
            }
            MouseClickAction::SelectSettingsItem(index) => self.select_settings_item(index).await,
        }
    }

    async fn select_settings_item(&mut self, index: usize) -> color_eyre::Result<()> {
        let catalog = crate::tui::settings_panel::all_settings();
        let Some(settings) = self.ui.settings_v2.as_mut() else {
            return Ok(());
        };
        settings.select(index, &catalog);
        let selected_id = settings
            .selected_setting(&catalog)
            .map(|setting| setting.id);
        match settings.enter(&catalog) {
            crate::tui::settings_panel::EnterResult::OpenKeybind {
                action_id,
                action_label,
            } => {
                self.ui.keybind_capture = Some(
                    crate::tui::keybind_capture::KeybindCaptureState::new(action_id, action_label),
                );
            }
            crate::tui::settings_panel::EnterResult::RunCommand(id) => {
                if let Some(action) = crate::tui::settings_panel::command_action(id) {
                    self.ui.settings_v2 = None;
                    self.action_tx.send(action).ok();
                }
            }
            crate::tui::settings_panel::EnterResult::SelectTheme(theme) => {
                self.apply_theme_selection(theme)?;
            }
            crate::tui::settings_panel::EnterResult::SelectToastPosition(position) => {
                self.apply_toast_position_selection(position)?;
            }
            crate::tui::settings_panel::EnterResult::ToggledBool => match selected_id {
                Some("web_search") => {
                    self.toggle_web_search().await?;
                    let state = if self.ui.web_search_enabled {
                        "enabled"
                    } else {
                        "disabled"
                    };
                    self.ui
                        .toast_stack
                        .push_message(format!("Web Search {state}"), self.ui.frame_tick);
                }
                Some("collapse_thinking") => {
                    self.toggle_collapse_thinking()?;
                }
                _ => {}
            },
            crate::tui::settings_panel::EnterResult::Nothing
            | crate::tui::settings_panel::EnterResult::EnteredSubsection
            | crate::tui::settings_panel::EnterResult::RequestConfirm => {}
        }
        Ok(())
    }

    fn toggle_collapse_thinking(&mut self) -> color_eyre::Result<()> {
        let mut config = self
            .config
            .try_read()
            .map(|config| config.clone())
            .unwrap_or_default();
        config.collapse_thinking = !config.collapse_thinking;
        config.save()?;
        if let Ok(mut live_config) = self.config.try_write() {
            *live_config = config.clone();
        }
        self.ui.collapse_thinking = config.collapse_thinking;
        let state = if self.ui.collapse_thinking {
            "enabled"
        } else {
            "disabled"
        };
        self.ui
            .toast_stack
            .push_message(format!("Collapse Thinking {state}"), self.ui.frame_tick);
        Ok(())
    }
}
