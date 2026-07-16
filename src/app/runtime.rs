use futures::StreamExt;
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
                self.ui.settings_v2 = Some(self.sidebar_settings_panel());
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
                    self.ui.settings_v2 = Some(self.sidebar_settings_panel());
                }
                Ok(())
            }
            Action::NewChat => self.new_conversation(self.ui.active_tab),
            Action::CloseChat => {
                let active_conversation = self
                    .ui
                    .tabs
                    .get(self.ui.active_tab)
                    .map(|tab| tab.active_conversation)
                    .unwrap_or(0);
                if active_conversation != 0 {
                    self.delete_conversation_by_id(active_conversation)?;
                } else {
                    self.ensure_tab_has_active_conversation(self.ui.active_tab)?;
                }
                Ok(())
            }
            Action::ToggleConversationPinned(conversation_id) => {
                self.toggle_conversation_pinned(conversation_id)
            }
            Action::DeleteConversation(conversation_id) => {
                self.delete_conversation_by_id(conversation_id)
            }
            Action::SendMessage(msg) => self.send_message(msg).await,
            Action::QueuePendingDocument(input) => {
                let document = input.snapshot()?;
                let status = format!(
                    "Queued {} ({} lines, {} bytes, {})",
                    document.display_name(),
                    document.line_count(),
                    document.byte_count(),
                    document.digest().prefix(),
                );
                let pending_count = {
                    let pending = self.pending_documents_mut();
                    pending.insert(document)?;
                    pending.len()
                };
                let status = format!("{status}; {pending_count} pending");
                self.ui.update_status(status);
                Ok(())
            }
            Action::RemovePendingDocument(document_id) => {
                self.pending_documents_mut().remove(document_id);
                Ok(())
            }
            Action::AwaitSubmissionConfirmation(app_tab_id) => self.transition_submission(
                app_tab_id,
                crate::app::submission::SubmissionState::await_confirmation,
            ),
            Action::ConfirmSubmission(app_tab_id) => self.transition_submission(
                app_tab_id,
                crate::app::submission::SubmissionState::confirm,
            ),
            Action::SubmissionFailedAfterLaunch(app_tab_id, submission_id) => {
                if let Some(state) = self.submission_states.remove(&app_tab_id) {
                    let next = if state.submission_id() == submission_id
                        && matches!(
                            &state,
                            crate::app::submission::SubmissionState::Launched { .. }
                        ) {
                        state.fail_after_launch()?
                    } else {
                        state
                    };
                    self.submission_states.insert(app_tab_id, next);
                }
                Ok(())
            }
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
                if let Err(error) = self.persist_generated_file(tab_id, file) {
                    self.ui.connection_status = crate::ui::status_bar::ConnectionStatus::Failed;
                    self.ui.connection_message =
                        Some(format!("Chat file persistence failed: {error}"));
                }
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
                self.ui.settings_v2 = Some(self.sidebar_settings_panel());
                Ok(())
            }
            Action::OpenSettingsPanel => {
                let _focus = crate::tui::focus::Focus::SettingsPanel;
                self.ui.palette = None;
                self.ui.settings_v2 = Some(self.sidebar_settings_panel());
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

    pub(crate) async fn send_message(&mut self, content: String) -> color_eyre::Result<()> {
        if tokio::runtime::Handle::try_current().is_err() {
            return Err(color_eyre::eyre::eyre!(
                "Cannot send a message without a Tokio runtime"
            ));
        }
        let app_tab_id = self.ui.app_tabs.active_id();
        let documents = self
            .pending_documents
            .get(&app_tab_id)
            .cloned()
            .unwrap_or_default();
        let submission = crate::app::submission::InputSubmission::new(&content, documents)?;
        let instruction = submission.instruction().to_string();
        let preparing =
            crate::app::submission::SubmissionState::draft(submission.clone()).begin_preparing()?;
        self.submission_states.insert(app_tab_id, preparing);

        let conversation_id = self
            .ui
            .tabs
            .get(self.ui.active_tab)
            .map(|t| t.active_conversation)
            .unwrap_or(0);

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
        let prepared_turn = crate::app::submission::PreparedUserTurn::prepare(&submission);
        let messages = prepared_turn.expand_messages(&tab_state.messages, conversation_id);
        let tab_endpoint = tab_state.tab.endpoint.clone();
        if model.trim().is_empty() {
            self.transition_submission(
                app_tab_id,
                crate::app::submission::SubmissionState::return_to_draft,
            )?;
            return Err(color_eyre::eyre::eyre!("Choose a model before sending."));
        }
        let config_snapshot = self.config.read().await.clone();
        if let Some(response) =
            crate::reminders::maybe_handle_request(&config_snapshot, &instruction)
        {
            self.transition_submission(
                app_tab_id,
                crate::app::submission::SubmissionState::finish_preparing,
            )?;
            let created_conversation = conversation_id == 0;
            let conversation_id = if created_conversation {
                match self.storage.create_conversation(tab_id as i64) {
                    Ok(conversation_id) => conversation_id,
                    Err(error) => {
                        self.transition_submission(
                            app_tab_id,
                            crate::app::submission::SubmissionState::return_to_draft,
                        )?;
                        return Err(error);
                    }
                }
            } else {
                conversation_id
            };
            let mut user_message =
                Message::new(conversation_id, "user".to_string(), instruction.clone());
            match self.storage.save_message(&user_message) {
                Ok(message_id) => user_message.id = Some(message_id),
                Err(error) => {
                    if created_conversation {
                        let _ = self.storage.delete_conversation(conversation_id);
                    }
                    self.transition_submission(
                        app_tab_id,
                        crate::app::submission::SubmissionState::return_to_draft,
                    )?;
                    return Err(error);
                }
            }
            self.transition_submission(
                app_tab_id,
                crate::app::submission::SubmissionState::commit,
            )?;
            if let Some(tab) = self.ui.tabs.get_mut(tab_id) {
                tab.active_conversation = conversation_id;
            }
            self.ui.add_message(tab_id, user_message);
            if let Some(tab) = self.ui.tabs.get_mut(tab_id) {
                if tab.messages.len() == 1 && tab.generated_title.is_none() {
                    let title = Self::generate_title(&instruction);
                    tab.generated_title = Some(title.clone());
                    self.storage
                        .update_conversation_title(conversation_id, &title)?;
                }
            }
            let mut assistant = Message::new(conversation_id, "assistant".to_string(), response);
            assistant.id = Some(self.storage.save_message(&assistant)?);
            self.ui.add_message(tab_id, assistant);
            self.refresh_tab_conversations(tab_id)?;
            self.transition_submission(
                app_tab_id,
                crate::app::submission::SubmissionState::launch,
            )?;
            self.pending_documents.remove(&app_tab_id);
            return Ok(());
        }
        let endpoint = if is_local_provider {
            Some(config_snapshot.local_inference.chat_endpoint())
        } else {
            tab_endpoint.or_else(|| {
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
        let user_request = instruction.clone();
        let terminal_has_focus = Arc::clone(&self.terminal_has_focus);

        let Some(endpoint) = endpoint else {
            self.transition_submission(
                app_tab_id,
                crate::app::submission::SubmissionState::return_to_draft,
            )?;
            self.ui.connection_status = crate::ui::status_bar::ConnectionStatus::Failed;
            self.ui.connection_message = Some(format!("Missing endpoint for {provider}"));
            return Err(color_eyre::eyre::eyre!(
                "No endpoint configured for {provider}."
            ));
        };
        let preparation = crate::app::submission_runtime::RequestPreparation {
            conversation_id,
            provider: provider.clone(),
            endpoint,
            model: model.clone(),
            reasoning_effort,
            supported_reasoning_efforts,
            backend_type,
            local_api_key,
            credential_env_var,
            system_prompt,
            messages,
            config: config_snapshot.clone(),
            instruction: instruction.clone(),
            is_local_provider,
        };
        let prepared = match crate::app::submission_runtime::prepare_launch(preparation).await {
            Ok(prepared) => prepared,
            Err(error) => {
                self.transition_submission(
                    app_tab_id,
                    crate::app::submission::SubmissionState::return_to_draft,
                )?;
                return Err(error);
            }
        };
        self.transition_submission(
            app_tab_id,
            crate::app::submission::SubmissionState::finish_preparing,
        )?;

        let created_conversation = conversation_id == 0;
        let conversation_id = if created_conversation {
            match self.storage.create_conversation(tab_id as i64) {
                Ok(conversation_id) => conversation_id,
                Err(error) => {
                    self.transition_submission(
                        app_tab_id,
                        crate::app::submission::SubmissionState::return_to_draft,
                    )?;
                    return Err(error);
                }
            }
        } else {
            conversation_id
        };
        let mut user_message =
            Message::new(conversation_id, "user".to_string(), instruction.clone());
        match self.storage.save_message(&user_message) {
            Ok(message_id) => user_message.id = Some(message_id),
            Err(error) => {
                if created_conversation {
                    let _ = self.storage.delete_conversation(conversation_id);
                }
                self.transition_submission(
                    app_tab_id,
                    crate::app::submission::SubmissionState::return_to_draft,
                )?;
                return Err(error);
            }
        }
        self.transition_submission(app_tab_id, crate::app::submission::SubmissionState::commit)?;
        if let Some(tab) = self.ui.tabs.get_mut(tab_id) {
            tab.active_conversation = conversation_id;
        }
        self.ui.add_message(tab_id, user_message);
        if let Some(tab) = self.ui.tabs.get_mut(tab_id) {
            if tab.messages.len() == 1 && tab.generated_title.is_none() {
                let title = Self::generate_title(&instruction);
                tab.generated_title = Some(title.clone());
                if let Err(error) = self
                    .storage
                    .update_conversation_title(conversation_id, &title)
                {
                    let _ = action_tx.send(Action::UpdateStatus(format!(
                        "Conversation title update failed: {error}"
                    )));
                }
            }
        }
        if let Err(error) = self.refresh_tab_conversations(tab_id) {
            let _ = action_tx.send(Action::UpdateStatus(format!(
                "Conversation list refresh failed: {error}"
            )));
        }

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
        if is_local_provider {
            self.ui.connection_status = crate::ui::status_bar::ConnectionStatus::LocalConnected;
            self.ui.connection_message = Some("Connected to Local LLM".to_string());
        } else {
            self.ui.connection_status = crate::ui::status_bar::ConnectionStatus::CloudConnected;
            self.ui.connection_message = Some(format!("{provider} / {model}"));
        }
        for notice in prepared.notices {
            let _ = action_tx.send(Action::UpdateStatus(notice));
        }
        #[cfg(feature = "memory")]
        let mut memory_activities = prepared.memory_activities;
        #[cfg(feature = "memory")]
        let _ = action_tx.send(Action::SetMemoryActivities(
            tab_id,
            assistant_idx,
            memory_activities.clone(),
        ));
        let event_tx = action_tx.clone();
        let request = prepared.request;
        let submission_id = prepared_turn.submission_id();
        let _handle = tokio::spawn(async move {
            let mut visible_answer = String::new();
            #[cfg(feature = "memory")]
            let mut remember_filter = crate::memory::RememberFilter::default();
            let result = crate::llm::chat::stream_prepared_chat(request, |event| match event {
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
                    let generated_file = crate::app::GeneratedFile::from_skill_response(
                        file_id,
                        conversation_id,
                        &user_request,
                        &visible_answer,
                    );
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
                    let _ = action_tx.send(Action::SubmissionFailedAfterLaunch(
                        app_tab_id,
                        submission_id,
                    ));
                }
            }
            let _ = action_tx.send(Action::StopStream(tab_id));
        });

        self.transition_submission(app_tab_id, crate::app::submission::SubmissionState::launch)?;
        self.pending_documents.remove(&app_tab_id);

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
                Some(id @ ("show_saved_files" | "show_memories" | "show_markdown_vault")) => {
                    self.toggle_sidebar_visibility(id)?;
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
