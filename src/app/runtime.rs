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
        if self.quit_requires_confirmation()
            || self.has_any_pending_documents()
            || self.ui.terminals.has_running()
            || self
                .ui
                .resource_editor
                .as_ref()
                .is_some_and(|editor| editor.is_dirty())
        {
            Action::ShowQuitConfirm
        } else {
            Action::Quit
        }
    }

    pub async fn run(
        &mut self,
        terminal: &mut ratatui::Terminal<crate::Backend>,
        reader: &mut crossterm::event::EventStream,
    ) -> color_eyre::Result<()> {
        if !std::io::stdin().is_terminal() {
            terminal.draw(|f| {
                self.ui.render(f);
            })?;
            return Ok(());
        }

        let mut tick = tokio::time::interval(std::time::Duration::from_millis(33));

        loop {
            if self.security_restart.is_some() {
                crate::clear_terminal_surface(terminal)?;
                break;
            }
            terminal.draw(|f| {
                self.ui.render(f);
            })?;

            tokio::select! {
                _ = tick.tick() => {
                    self.poll_credential_login().await;
                    if self.security_restart.is_some() {
                        continue;
                    }
                    self.poll_terminals();
                    for notice in self.ui.vault_workspaces.reconcile_tick() {
                        self.ui.show_toast(notice);
                    }
                    let active_app_tab_id = self.ui.app_tabs.active_id();
                    let _ = self
                        .ui
                        .vault_workspaces
                        .poll_editor(active_app_tab_id);
                    if let Some(editor) = self.ui.editor_popup.as_mut() {
                        let done = editor.poll_output();
                        if done {
                            let chat_draft = editor.take_chat_draft();
                            self.ui.editor_popup = None;
                            if let Some(draft) = chat_draft {
                                self.apply_chat_draft_from_path(draft.path());
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
                                    if let Err(error) = self.dispatch(action).await {
                                        self.ui.show_toast(format!("Action failed: {error}"));
                                    }
                                }
                            }
                        }
                        Some(Ok(crossterm::event::Event::Mouse(mouse))) => {
                            self.cancel_escape = None;
                            if let Some(action) = self.handle_mouse(mouse) {
                                if matches!(action, Action::Quit) {
                                    break;
                                }
                                if let Err(error) = self.dispatch(action).await {
                                    self.ui.show_toast(format!("Action failed: {error}"));
                                }
                            }
                        }
                        Some(Ok(crossterm::event::Event::FocusGained)) => {
                            self.terminal_has_focus.store(true, Ordering::Relaxed);
                        }
                        Some(Ok(crossterm::event::Event::FocusLost)) => {
                            self.cancel_escape = None;
                            self.terminal_has_focus.store(false, Ordering::Relaxed);
                        }
                        Some(Ok(crossterm::event::Event::Paste(text))) => {
                            self.cancel_escape = None;
                            if let Some(manager) = self.ui.credential_manager.as_mut() {
                                manager.paste(&text);
                            } else if self.ui.command_review.is_some() || self.ui.terminals.close_pending.is_some()
                                || self.ui.active_modal.is_some() || self.ui.editor_popup.is_some()
                                || self.ui.artifact_viewer.is_some() || self.ui.save_file_dialog.is_some()
                                || self.ui.export_dialog.is_some()
                                || self.ui.pending_vault_mutation.is_some() || self.ui.list_popup.is_some()
                                || self.ui.delete_confirm.is_some() || self.ui.show_keybinds || self.ui.show_session_list
                            {
                                // Modal input must never reach a hidden PTY or chat composer.
                            } else if let Some(editor) = self.ui.resource_editor.as_mut() {
                                editor.insert(&text);
                            } else if let Some(settings) = self.ui.settings_v2.as_mut() {
                                for character in text.chars().filter(|character| !character.is_control()) {
                                    settings.insert_char(character);
                                }
                            } else if let Some(palette) = self.ui.palette.as_mut() {
                                for character in text.chars().filter(|character| !character.is_control()) {
                                    palette.insert_char(character);
                                }
                            } else if self.ui.app_tabs.active_view() == crate::ui::app_tabs::AppView::Settings {
                                self.ui.settings_workspace.paste(&text);
                            } else if !self.terminal_paste(&text) && self.ui.app_tabs.active_view().is_chat() {
                                self.insert_input_text(&text);
                            }
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
                        if let Err(error) = self.dispatch(action).await {
                            self.ui.show_toast(format!("Action failed: {error}"));
                        }
                    }
                }
            }
        }
        self.quiesce_for_security_restart().await
    }

    pub(crate) async fn quiesce_for_security_restart(&mut self) -> color_eyre::Result<()> {
        // Producers must be joined before draining their final events or revoking shared keys.
        self.stop_credential_login().await;
        self.stop_background_tasks().await;
        crate::llm::auth::cancel_all_refreshes().await;
        let mut result = Ok(());
        while let Ok(action) = self.action_rx.try_recv() {
            let event_result = match action {
                Action::StreamResponse(id, content) => {
                    self.append_request_content(id, &content, false)
                }
                Action::StreamThinking(id, content) => {
                    self.append_request_content(id, &content, true)
                }
                #[cfg(feature = "memory")]
                Action::SetMemoryActivities(id, activities) => {
                    self.set_request_memory_activities(id, &activities)
                }
                Action::FinalizeAssistantMessage(completion) => {
                    self.finalize_assistant_message(completion)
                }
                Action::StopStream(id) => self.finish_request(id),
                Action::PersistHostActionReceipts(id) => self.persist_host_action_receipts(id),
                _ => Ok(()),
            };
            result = result.and(event_result);
        }
        let ids: Vec<_> = self.requests.keys().copied().collect();
        for id in ids {
            result = result.and(self.cancel_request(id));
        }
        // Ordinary save failures retain the application, keys, and every remaining snapshot.
        result?;
        self.close_ephemeral()?;
        self.shutdown_terminals();
        Ok(())
    }

    pub async fn dispatch(&mut self, action: Action) -> color_eyre::Result<()> {
        if self.security_restart.is_some() {
            return Err(color_eyre::eyre::eyre!(
                "Application protection transition is pending."
            ));
        }
        match action {
            Action::OpenSecurity => {
                self.request_security_restart(crate::protection::ui::SecurityRestart::Configure);
                Ok(())
            }
            Action::UnlockApplication => {
                self.request_security_restart(crate::protection::ui::SecurityRestart::Unlock);
                Ok(())
            }
            Action::OpenNativeTransfer(domain) => {
                self.request_security_restart(crate::protection::ui::SecurityRestart::Transfer(
                    domain,
                ));
                Ok(())
            }
            Action::SaveResourceEditor => {
                self.save_resource_editor();
                Ok(())
            }
            Action::CopyResourceEditor => {
                self.copy_resource_editor();
                Ok(())
            }
            Action::CopyArtifact(handle) => {
                self.copy_artifact(handle);
                Ok(())
            }
            Action::ConfirmProtectedContext => self.confirm_protected_context(),
            Action::LockApplication => self
                .lock_application()
                .map_err(color_eyre::eyre::Error::msg),
            Action::OpenProviders => {
                self.open_credential_manager(false);
                Ok(())
            }
            Action::OpenCredentialVault => {
                self.open_credential_manager(true);
                Ok(())
            }
            Action::OpenSearchCredentials => {
                self.open_search_credentials();
                Ok(())
            }
            Action::OpenPushoverCredential { application } => {
                self.open_pushover_credential(application);
                Ok(())
            }
            Action::SetManualModel(model) => {
                self.open_manual_model(&model);
                Ok(())
            }
            Action::Quit => Ok(()),
            Action::ConfirmQuit => {
                self.discard_all_pending_documents();
                self.action_tx.send(Action::Quit).ok();
                Ok(())
            }
            Action::CancelModal => {
                self.ui.active_modal = None;
                self.ui.pending_protected_context = None;
                self.cancel_pending_document_discard();
                self.ctrl_c_count = 0;
                Ok(())
            }
            Action::ShowQuitConfirm => {
                if self.has_any_pending_documents() {
                    self.request_pending_document_discard(
                        crate::app::pending_documents::PendingDocumentDiscardAction::Quit,
                    );
                    self.ctrl_c_count = 1;
                    self.last_ctrl_c = Some(std::time::Instant::now());
                } else if self.quit_requires_confirmation()
                    || self.has_security_process_close_risk()
                    || self
                        .ui
                        .resource_editor
                        .as_ref()
                        .is_some_and(|editor| editor.is_dirty())
                {
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
                if self.ui.ephemeral.is_some() {
                    return self.close_ephemeral();
                }
                self.ui.panel_state.toggle_right();
                self.ui.artifact_sidebar_open =
                    self.ui.panel_state.right != crate::tui::shell::PanelMode::Closed;
                Ok(())
            }
            Action::RefreshArtifactSidebar => {
                self.refresh_artifact_sidebar_catalogs();
                Ok(())
            }
            Action::ShowSettings | Action::OpenSettingsPanel => {
                self.ui.settings_v2 = None;
                self.ui.palette = None;
                self.open_settings_page(None);
                Ok(())
            }
            Action::OpenSettingsPage(id) => {
                self.ui.settings_v2 = None;
                self.ui.palette = None;
                self.open_settings_page(Some(&id));
                Ok(())
            }
            Action::ApplySetting { id, value } => {
                if let Err(error) = self.apply_settings_value(&id, &value).await {
                    self.ui
                        .show_toast(format!("Setting was not saved: {error}"));
                }
                Ok(())
            }
            Action::SaveSettings { draft } => {
                if let Err(error) = self.save_settings(draft).await {
                    self.ui
                        .show_toast(format!("Settings were not saved: {error}"));
                }
                Ok(())
            }
            Action::CloseSettings => {
                self.ui.settings_v2 = None;
                self.ui.app_tabs.select(0);
                Ok(())
            }
            Action::ToggleSettings => {
                self.ui.palette = None;
                self.ui.settings_v2 = None;
                if self.ui.app_tabs.active_view() == crate::ui::app_tabs::AppView::Settings {
                    self.ui.app_tabs.select(0);
                } else {
                    self.open_settings_page(None);
                }
                Ok(())
            }
            Action::NewTab => {
                self.ui.app_tabs.add_launcher();
                self.ui.focus = crate::tui::focus::Focus::Chat;
                Ok(())
            }
            Action::NewChat => {
                self.focus_chat_pane(self.ui.main_chat_index());
                let tab_id = self.ui.active_tab;
                if self.request_pending_document_discard(
                    crate::app::pending_documents::PendingDocumentDiscardAction::NewConversation(
                        tab_id,
                    ),
                ) {
                    Ok(())
                } else {
                    self.new_conversation(tab_id)
                }
            }
            Action::CloseChat => {
                if self.request_pending_document_discard(
                    crate::app::pending_documents::PendingDocumentDiscardAction::CloseConversation,
                ) {
                    Ok(())
                } else {
                    self.close_active_conversation()
                }
            }
            Action::ToggleConversationPinned(conversation_id) => {
                self.toggle_conversation_pinned(conversation_id)
            }
            Action::DeleteConversation(conversation_id) => {
                self.delete_conversation_by_id(conversation_id)
            }
            Action::SendMessage(msg) => self.send_message(msg).await,
            Action::ForkConversation => self.fork_active_conversation(),
            Action::OpenEphemeral => self.open_ephemeral(),
            Action::CloseEphemeral => self.close_ephemeral(),
            Action::FocusChatPane(index) => {
                self.focus_chat_pane(index);
                Ok(())
            }
            Action::CancelRequest => self.cancel_active_request(),
            Action::SteerRequest(instruction) => {
                if let Err(error) = self.steer_active_request(instruction) {
                    self.ui.show_toast(error.to_string());
                }
                Ok(())
            }
            Action::QueuePendingDocument(input) => self.queue_pending_document(input),
            Action::InsertArtifactToChat(handle) => {
                self.insert_artifact_to_chat(handle);
                Ok(())
            }
            Action::RemovePendingDocument(document_id) => {
                self.remove_pending_document(document_id);
                Ok(())
            }
            Action::ConfirmPendingDocumentDiscard => self.confirm_pending_document_discard(),
            Action::ConfirmChatDraftExport => {
                self.open_chat_draft_editor_after_consent();
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
            Action::CancelVaultMutation => self.cancel_pending_vault_mutation(),
            Action::ConfirmVaultMutation => self.confirm_pending_vault_mutation().await,
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
            Action::StreamResponse(request_id, content) => {
                self.append_request_content(request_id, &content, false)
            }
            Action::StreamThinking(request_id, content) => {
                self.append_request_content(request_id, &content, true)
            }
            #[cfg(feature = "memory")]
            Action::SetMemoryActivities(request_id, activities) => {
                self.set_request_memory_activities(request_id, &activities)
            }
            Action::FinalizeAssistantMessage(completion) => {
                self.finalize_assistant_message(completion)
            }
            Action::StopStream(request_id) => self.finish_request(request_id),
            Action::PersistHostActionReceipts(id) => self.persist_host_action_receipts(id),
            Action::AddMessage(tab_id, msg) => {
                let mut stored = msg;
                if let Ok(message_id) = self.storage.save_message(&stored) {
                    stored.id = Some(message_id);
                }
                self.ui.add_message(tab_id, stored);
                Ok(())
            }
            Action::AddGeneratedFile(tab_id, file) => {
                match self.persist_generated_file(tab_id, file) {
                    Ok(filename) => {
                        self.ui.connection_status =
                            crate::ui::status_bar::ConnectionStatus::CloudConnected;
                        self.ui.connection_message = Some(format!("Saved as {filename}"));
                    }
                    Err(error) => {
                        self.ui.connection_status = crate::ui::status_bar::ConnectionStatus::Failed;
                        self.ui.connection_message =
                            Some(format!("Generated file persistence failed: {error}"));
                    }
                }
                Ok(())
            }
            Action::LoadConversation(conv_id) => {
                self.focus_chat_pane(self.ui.main_chat_index());
                if self.request_pending_document_discard(
                    crate::app::pending_documents::PendingDocumentDiscardAction::LoadConversation(
                        conv_id,
                    ),
                ) {
                    Ok(())
                } else {
                    self.load_conversation(conv_id)
                }
            }
            Action::NewConversation(tab_id) => {
                if self.request_pending_document_discard(
                    crate::app::pending_documents::PendingDocumentDiscardAction::NewConversation(
                        tab_id,
                    ),
                ) {
                    Ok(())
                } else {
                    self.new_conversation(tab_id)
                }
            }
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
            Action::SwitchTab(idx) => {
                if self.request_pending_document_discard(
                    crate::app::pending_documents::PendingDocumentDiscardAction::SwitchTab(idx),
                ) {
                    Ok(())
                } else {
                    self.ui.active_tab = idx;
                    self.ui.settings_v2 = None;
                    self.refresh_visible_selectors();
                    Ok(())
                }
            }
            Action::AddTab(tab) => {
                self.close_ephemeral()?;
                self.ui.add_tab(tab);
                self.ui.settings_v2 = None;
                self.refresh_visible_selectors();
                Ok(())
            }
            Action::RemoveTab(idx) => {
                if self
                    .ui
                    .ephemeral
                    .as_ref()
                    .is_some_and(|pane| pane.tab_index == idx)
                {
                    return self.close_ephemeral();
                }
                self.close_ephemeral()?;
                if self.request_pending_document_discard(
                    crate::app::pending_documents::PendingDocumentDiscardAction::RemoveTab(idx),
                ) {
                    Ok(())
                } else {
                    self.ui.remove_tab(idx);
                    self.refresh_visible_selectors();
                    Ok(())
                }
            }
            Action::ToggleSessionList => {
                self.ui.show_session_list = !self.ui.show_session_list;
                Ok(())
            }
            Action::UpdateTabProvider(tab_id, provider) => {
                if let Some(tab) = self.ui.tabs.get_mut(tab_id) {
                    tab.tab.provider = provider.clone();
                    tab.tab.endpoint = None;
                    tab.tab.model.clear();
                    tab.manual_model_override = false;
                    tab.tab.reasoning_effort = None;
                }
                self.refresh_visible_selectors();
                if tab_id != self.ui.active_tab {
                    self.persist_tab_chat_selection(tab_id)?;
                }
                self.queue_connection_check_for_active_tab();
                self.ui
                    .toast_stack
                    .push_message(format!("Switched to {provider}"), self.ui.frame_tick);
                Ok(())
            }
            Action::UpdateTabModel(tab_id, model) => {
                if let Some(tab) = self.ui.tabs.get_mut(tab_id) {
                    tab.tab.model = model.clone();
                    tab.manual_model_override = !model.trim().is_empty();
                    tab.tab.reasoning_effort = None;
                }
                self.refresh_visible_selectors();
                if tab_id != self.ui.active_tab {
                    self.persist_tab_chat_selection(tab_id)?;
                }
                self.queue_connection_check_for_active_tab();
                self.ui
                    .toast_stack
                    .push_message(format!("Model: {model}"), self.ui.frame_tick);
                Ok(())
            }
            Action::SetProviderModels(provider, models) => {
                self.apply_refreshed_provider_models(&provider, &models);
                Ok(())
            }
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
                for (provider, _, _, _, _) in &self.ui.db_providers {
                    if self
                        .ui
                        .visible_providers
                        .iter()
                        .any(|entry| entry.0 == *provider)
                    {
                        self.refresh_models_for_provider(provider.clone());
                    }
                }
                Ok(())
            }
            Action::OpenCommandPalette => {
                self.ui.palette = None;
                self.ui.settings_v2 = Some(self.sidebar_settings_panel());
                Ok(())
            }
            Action::OpenTerminal => {
                self.open_terminal().await;
                Ok(())
            }
            Action::ApproveCommandReview => {
                self.approve_command_review().await;
                Ok(())
            }
            Action::TransferCommandReview => {
                self.transfer_command_review();
                Ok(())
            }
            Action::OpenVaultLauncher => {
                self.open_vault_launcher();
                Ok(())
            }
            #[cfg(feature = "memory")]
            Action::OpenMemoriesLauncher => {
                self.open_memories_launcher();
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
        if self.active_request_id().is_some() {
            self.replace_input_content(content);
            self.ui.show_toast(
                "This conversation already has an active request. Use /cancel or press Esc twice."
                    .into(),
            );
            return Ok(());
        }
        if tokio::runtime::Handle::try_current().is_err() {
            return Err(color_eyre::eyre::eyre!(
                "Cannot send a message without a Tokio runtime"
            ));
        }
        let system_prompt = match Self::load_system_prompt() {
            Ok(prompt) => prompt,
            Err(error) => {
                self.replace_input_content(content);
                return Err(error);
            }
        };
        let completed_vault_result = self
            .ui
            .pending_vault_mutation
            .as_ref()
            .and_then(crate::app::submission::PendingVaultMutation::completed_submission)
            .cloned();
        if self.ui.pending_vault_mutation.is_some() && completed_vault_result.is_none() {
            return Err(color_eyre::eyre::eyre!(
                "Confirm or cancel the pending Vault mutation first"
            ));
        }
        let completed_vault_mutation = completed_vault_result.is_some();
        let app_tab_id = self.input_context_id();
        let (content, documents) = match self.ui.pending_vault_mutation.as_ref() {
            Some(pending) if completed_vault_mutation => (
                pending.submission.instruction().to_owned(),
                pending.submission.documents().clone(),
            ),
            Some(_) | None => (
                content,
                self.pending_documents
                    .get(&app_tab_id)
                    .cloned()
                    .unwrap_or_default(),
            ),
        };
        let submission = crate::app::submission::InputSubmission::new(&content, documents);
        let prepared_turn = crate::app::submission::PreparedUserTurn::prepare(&submission)?;
        let canonical_content = prepared_turn.content().to_owned();
        let ephemeral_context = match self.ephemeral_context_for_request(&content) {
            Ok(context) => context,
            Err(error) => {
                self.replace_input_content(content);
                return Err(error);
            }
        };
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
        self.persist_tab_chat_selection(tab_id)?;
        let Some(tab_state) = self.ui.tabs.get(tab_id) else {
            return Ok(());
        };
        let provider = tab_state.tab.provider.clone();
        let is_local_provider = crate::llm::local::is_local_provider(&provider);
        let model = tab_state.tab.model.clone();
        let reasoning_effort = tab_state.tab.reasoning_effort.clone();
        if let Some(entry) = self
            .ui
            .current_models
            .iter()
            .find(|entry| entry.id == model)
        {
            if reasoning_effort.as_ref().is_some_and(|effort| {
                !Self::reasoning_options_for(&provider, entry).contains(effort)
            }) {
                return Err(color_eyre::eyre::eyre!(
                    "Selected reasoning effort is unsupported for {provider}/{model}; choose a supported effort before sending"
                ));
            }
        }
        let supported_reasoning_efforts = self
            .ui
            .current_models
            .iter()
            .find(|entry| entry.id == model)
            .map(|entry| entry.supported_reasoning_efforts.clone())
            .unwrap_or_default();
        let mut messages = prepared_turn.expand_messages(&tab_state.messages, conversation_id);
        if let Some(context) = &ephemeral_context {
            if let Some(message) = messages.last_mut() {
                message.content.push_str(context);
            }
        }
        let tab_endpoint = tab_state.tab.endpoint.clone();
        let config_snapshot = self.config.read().await.clone();
        if provider.is_empty()
            || !self
                .ui
                .visible_providers
                .iter()
                .any(|entry| entry.0 == provider)
        {
            self.transition_submission(
                app_tab_id,
                crate::app::submission::SubmissionState::return_to_draft,
            )?;
            self.replace_input_content(content);
            self.open_credential_manager(false);
            self.ui
                .show_toast("Add a provider with /connect before sending.".into());
            return Ok(());
        }
        if model.trim().is_empty() {
            self.transition_submission(
                app_tab_id,
                crate::app::submission::SubmissionState::return_to_draft,
            )?;
            self.ui
                .show_toast("Choose a model, or enter its ID with /model <id>.".into());
            return Err(color_eyre::eyre::eyre!("Choose a model before sending."));
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
            crate::config::KeyStore::get(&config_snapshot, crate::config::LOCAL_PROVIDER_NAME)
                .ok()
                .flatten()
        } else {
            None
        };
        let credential_env_var = provider_config
            .as_ref()
            .map(|(_, env_var, _)| env_var.clone());
        let action_tx = self.action_tx.clone();
        let user_request = content.clone();
        let save_requested = crate::skills::mentions(&content)
            .iter()
            .any(|mention| mention == "save");
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
            history: self.storage.chat_reader(),
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
            user_request: content.clone(),
            #[cfg(feature = "memory")]
            memory_instruction: content,
            is_local_provider,
            vault_result: completed_vault_result,
        };
        enum LaunchWork {
            Ready(Box<crate::app::submission_runtime::PreparedLaunch>),
            Deferred(Box<crate::app::submission_runtime::RequestPreparation>),
        }
        impl LaunchWork {
            async fn prepare(
                self,
            ) -> color_eyre::Result<crate::app::submission_runtime::PreparedLaunch> {
                match self {
                    Self::Ready(prepared) => Ok(*prepared),
                    Self::Deferred(preparation) => {
                        match crate::app::submission_runtime::prepare_launch(*preparation).await? {
                            crate::app::submission_runtime::PreparationOutcome::Ready(prepared) => {
                                Ok(*prepared)
                            }
                            crate::app::submission_runtime::PreparationOutcome::VaultMutation(
                                _,
                            ) => Err(color_eyre::eyre::eyre!(
                                "Use a separate @vault request to confirm Vault mutations"
                            )),
                        }
                    }
                }
            }
        }
        let host_execution = crate::skill_runtime::host_execution::requested(&user_request);
        let work = if host_execution {
            // Own cancellation before agent tool decisions or host actions begin.
            LaunchWork::Deferred(Box::new(preparation))
        } else {
            let preparation_outcome =
                match crate::app::submission_runtime::prepare_launch(preparation).await {
                    Ok(prepared) => prepared,
                    Err(error) => {
                        self.transition_submission(
                            app_tab_id,
                            crate::app::submission::SubmissionState::return_to_draft,
                        )?;
                        return Err(error);
                    }
                };
            match preparation_outcome {
                crate::app::submission_runtime::PreparationOutcome::Ready(prepared) => {
                    LaunchWork::Ready(prepared)
                }
                crate::app::submission_runtime::PreparationOutcome::VaultMutation(kind) => {
                    let pending = match crate::app::submission::PendingVaultMutation::new(
                        prepared_turn,
                        kind,
                    ) {
                        Ok(pending) => pending,
                        Err(error) => {
                            self.transition_submission(
                                app_tab_id,
                                crate::app::submission::SubmissionState::return_to_draft,
                            )?;
                            return Err(error.into());
                        }
                    };
                    self.transition_submission(
                        app_tab_id,
                        crate::app::submission::SubmissionState::await_confirmation,
                    )?;
                    self.ui.pending_vault_mutation = Some(pending);
                    return Ok(());
                }
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
        let mut user_message = Message::new(
            conversation_id,
            "user".to_string(),
            canonical_content.clone(),
        );
        if let Some(context) = &ephemeral_context {
            user_message.content.push_str(context);
        }
        match crate::app::vault_mutation_execute::persist_user_message(
            &self.storage,
            &user_message,
            completed_vault_mutation,
        ) {
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
                let title = Self::generate_title(&canonical_content);
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

        let submission_id = prepared_turn.submission_id();
        let route = crate::app::requests::RequestRoute {
            request_id: submission_id,
            origin_conversation_id: conversation_id,
            destination_conversation_id: conversation_id,
        };
        let history = self.ui.tabs[tab_id].messages.clone();
        self.register_request(route, tab_id, history)?;
        if let Some(request) = self.requests.get_mut(&submission_id) {
            request.hide_answer = save_requested;
        }
        if is_local_provider {
            self.ui.connection_status = crate::ui::status_bar::ConnectionStatus::LocalConnected;
            self.ui.connection_message = Some("Connected to Local LLM".to_string());
        } else {
            self.ui.connection_status = crate::ui::status_bar::ConnectionStatus::CloudConnected;
            self.ui.connection_message = Some(format!("{provider} / {model}"));
        }
        let host_receipts = self
            .requests
            .get(&submission_id)
            .map(|request| request.host_receipts.clone())
            .ok_or_else(|| color_eyre::eyre::eyre!("Request disappeared before agent launch"))?;
        let completion = crate::app::requests::StreamCompletion {
            request_id: submission_id,
            sender: action_tx.clone(),
        };
        let handle = tokio::spawn(async move {
            // Capture the guard before spawning so even an unpolled abort reports termination.
            let _completion = completion;
            let prepared = match crate::skill_runtime::host_receipts::capture(
                host_receipts,
                submission_id,
                action_tx.clone(),
                work.prepare(),
            )
            .await
            {
                Ok(prepared) => prepared,
                Err(error) => {
                    let message = format!("Request preparation failed: {error}");
                    let _ = action_tx.send(Action::UpdateStatus(message.clone()));
                    let _ = action_tx.send(Action::FinalizeAssistantMessage(
                        crate::app::action::AssistantFinalization {
                            request_id: submission_id,
                            message: Message::new(conversation_id, "assistant".to_owned(), message),
                            generated_file:
                                crate::app::action::GeneratedFileFinalization::NotRequested,
                        },
                    ));
                    let _ = action_tx.send(Action::SubmissionFailedAfterLaunch(
                        app_tab_id,
                        submission_id,
                    ));
                    return;
                }
            };
            for notice in prepared.notices {
                let _ = action_tx.send(Action::UpdateStatus(notice));
            }
            #[cfg(feature = "memory")]
            let memory_activities = prepared.memory_activities;
            #[cfg(feature = "memory")]
            if memory_activities.iter().any(|activity| {
                matches!(
                    activity,
                    crate::memory::MemoryActivity::Saved { .. }
                        | crate::memory::MemoryActivity::Removed { .. }
                )
            }) {
                let _ = action_tx.send(Action::RefreshArtifactSidebar);
            }
            #[cfg(feature = "memory")]
            let _ = action_tx.send(Action::SetMemoryActivities(
                submission_id,
                memory_activities.clone(),
            ));
            let event_tx = action_tx.clone();
            let redactions = prepared.redactions;
            let private_output = !redactions.is_empty();
            let request = prepared.request;
            let mut visible_answer = String::new();
            let provider_future =
                crate::llm::chat::stream_prepared_chat(request, |event| match event {
                    crate::llm::chat::ChatStreamEvent::Answer(content) => {
                        if !content.is_empty() {
                            visible_answer.push_str(&content);
                            if !private_output {
                                let _ =
                                    event_tx.send(Action::StreamResponse(submission_id, content));
                            }
                        }
                    }
                    crate::llm::chat::ChatStreamEvent::Thinking(content) => {
                        if !private_output {
                            let _ = event_tx.send(Action::StreamThinking(submission_id, content));
                        }
                    }
                });
            let result = if host_execution {
                crate::diagnostics::sensitive_provider_round(provider_future).await
            } else {
                provider_future.await
            }
            .map(|mut response| {
                if private_output {
                    // Buffer sensitive replies so keys split across chunks cannot reach the UI.
                    visible_answer = redactions.redact(&visible_answer);
                    response.thinking = redactions.redact(&response.thinking);
                    let _ = event_tx.send(Action::StreamResponse(
                        submission_id,
                        visible_answer.clone(),
                    ));
                    if !response.thinking.is_empty() {
                        let _ = event_tx.send(Action::StreamThinking(
                            submission_id,
                            response.thinking.clone(),
                        ));
                    }
                }
                response
            });

            match result {
                Ok(response)
                    if save_requested
                        || !visible_answer.is_empty()
                        || !response.thinking.is_empty() =>
                {
                    let assistant_content = visible_answer.clone();
                    let mut msg =
                        Message::new(conversation_id, "assistant".to_string(), assistant_content);
                    msg.thinking_content =
                        (!response.thinking.is_empty()).then_some(response.thinking);
                    msg.token_count = response.total_tokens;
                    #[cfg(feature = "memory")]
                    let _ = crate::memory::set_activities(&mut msg, &memory_activities);
                    let generated_file = if save_requested {
                        crate::app::GeneratedFile::from_skill_response(
                            rand::random(),
                            conversation_id,
                            &user_request,
                            &visible_answer,
                        )
                        .map_or(
                            crate::app::action::GeneratedFileFinalization::Invalid,
                            crate::app::action::GeneratedFileFinalization::Ready,
                        )
                    } else {
                        crate::app::action::GeneratedFileFinalization::NotRequested
                    };
                    let _ = action_tx.send(Action::FinalizeAssistantMessage(
                        crate::app::action::AssistantFinalization {
                            request_id: submission_id,
                            message: msg,
                            generated_file,
                        },
                    ));
                    let _ = action_tx.send(Action::StopStream(submission_id));
                    if !save_requested && !terminal_has_focus.load(Ordering::Relaxed) {
                        let _ = crate::notifications::notify_finished(
                            &config_snapshot,
                            &user_request,
                            &visible_answer,
                        )
                        .await;
                    }
                }
                Ok(_) => {}
                Err(err) => {
                    let message = redactions.redact(&format!("Provider request failed: {err}"));
                    let _ = action_tx.send(Action::UpdateStatus(message.clone()));
                    let final_message =
                        Message::new(conversation_id, "assistant".to_string(), message);
                    #[cfg(feature = "memory")]
                    let final_message = {
                        let mut message = final_message;
                        let _ = crate::memory::set_activities(&mut message, &memory_activities);
                        message
                    };
                    let _ = action_tx.send(Action::FinalizeAssistantMessage(
                        crate::app::action::AssistantFinalization {
                            request_id: submission_id,
                            message: final_message,
                            generated_file:
                                crate::app::action::GeneratedFileFinalization::NotRequested,
                        },
                    ));
                    let _ = action_tx.send(Action::SubmissionFailedAfterLaunch(
                        app_tab_id,
                        submission_id,
                    ));
                }
            }
        });
        if let Some(request) = self.requests.get_mut(&submission_id) {
            request.abort = Some(handle.abort_handle());
        }
        self.track_background_task(handle);

        self.transition_submission(app_tab_id, crate::app::submission::SubmissionState::launch)?;
        self.discard_pending_documents();
        if completed_vault_mutation {
            self.ui.pending_vault_mutation = None;
        }

        Ok(())
    }

    pub(crate) fn cancel_pending_vault_mutation(&mut self) -> color_eyre::Result<()> {
        let Some(pending) = self.ui.pending_vault_mutation.take() else {
            return Ok(());
        };
        if !pending.can_cancel() {
            self.ui.pending_vault_mutation = Some(pending);
            return Err(color_eyre::eyre::eyre!(
                "confirmed Vault mutation cannot be cancelled"
            ));
        }
        if pending.is_workspace() {
            return Ok(());
        }
        let app_tab_id = self.input_context_id();
        self.transition_submission(
            app_tab_id,
            crate::app::submission::SubmissionState::return_to_draft,
        )?;
        self.pending_documents
            .insert(app_tab_id, pending.submission.documents().clone());
        self.replace_input_content(pending.submission.instruction().to_owned());
        Ok(())
    }

    pub(crate) async fn confirm_pending_vault_mutation(&mut self) -> color_eyre::Result<()> {
        use crate::app::submission::VaultMutationConfirmationState;
        use crate::app::vault_mutation_execute::VaultMutationExecutionError;

        let mut pending = self
            .ui
            .pending_vault_mutation
            .take()
            .ok_or_else(|| color_eyre::eyre::eyre!("Vault mutation preview is unavailable"))?;
        match &pending.state {
            VaultMutationConfirmationState::CompletedAwaitingChat { .. } => {
                let instruction = pending.submission.instruction().to_owned();
                self.ui.pending_vault_mutation = Some(pending);
                return self.send_message(instruction).await;
            }
            VaultMutationConfirmationState::RecoveryRequired {
                recovery_status, ..
            } => {
                let error = color_eyre::eyre::eyre!(recovery_status.clone());
                self.ui.pending_vault_mutation = Some(pending);
                return Err(error);
            }
            VaultMutationConfirmationState::Reviewing { .. }
            | VaultMutationConfirmationState::ReadyForExecution
            | VaultMutationConfirmationState::ExecutionFailed { .. } => {}
        }
        let data_root = crate::storage::paths::TcuiDataPaths::discover().root;
        if matches!(
            pending.state,
            VaultMutationConfirmationState::Reviewing { .. }
        ) {
            if let Err(error) = pending.confirm(&data_root) {
                self.ui.pending_vault_mutation = Some(pending);
                return Err(error.into());
            }
            if !pending.is_workspace() {
                let app_tab_id = self.input_context_id();
                self.transition_submission(
                    app_tab_id,
                    crate::app::submission::SubmissionState::confirm,
                )?;
            }
        }
        let changes = crate::app::vault_mutation_execute::affected_paths(&pending.kind);
        let targets = match self.ui.vault_workspaces.mutation_targets(&changes) {
            Ok(targets) => targets,
            Err(error) => {
                let message = error.to_string();
                pending.state = VaultMutationConfirmationState::ExecutionFailed {
                    error: message.clone(),
                };
                self.ui.pending_vault_mutation = Some(pending);
                return Err(color_eyre::eyre::eyre!(message));
            }
        };
        match crate::app::vault_mutation_execute::execute(&pending.kind, &data_root) {
            Ok(submission) => {
                let _ = self.ui.vault_workspaces.refresh_mutation_targets(&targets);
                if pending.is_workspace() {
                    self.ui.show_toast("Vault file moved".to_string());
                    return Ok(());
                }
                let operation_id = submission.operation_id().to_owned();
                pending.state = VaultMutationConfirmationState::CompletedAwaitingChat {
                    operation_id,
                    submission,
                };
                let instruction = pending.submission.instruction().to_owned();
                self.ui.pending_vault_mutation = Some(pending);
                self.send_message(instruction).await
            }
            Err(VaultMutationExecutionError::Failed { message }) => {
                pending.state = VaultMutationConfirmationState::ExecutionFailed {
                    error: message.to_owned(),
                };
                self.ui.pending_vault_mutation = Some(pending);
                Err(color_eyre::eyre::eyre!(message))
            }
            Err(VaultMutationExecutionError::RecoveryRequired { operation_id }) => {
                let recovery_status = format!(
                    "Recovery required for operation {operation_id}; restart TCUI to retry native recovery"
                );
                pending.state = VaultMutationConfirmationState::RecoveryRequired {
                    operation_id,
                    recovery_status: recovery_status.clone(),
                };
                self.ui.pending_vault_mutation = Some(pending);
                Err(color_eyre::eyre::eyre!(recovery_status))
            }
        }
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
                if self.ui.ephemeral.is_some() {
                    return self.close_ephemeral();
                }
                self.ui.panel_state.toggle_right();
                self.ui.artifact_sidebar_open =
                    self.ui.panel_state.right != crate::tui::shell::PanelMode::Closed;
                Ok(())
            }
            MouseClickAction::CloseRightSidebar => {
                self.close_ephemeral()?;
                self.ui.panel_state.right = crate::tui::shell::PanelMode::Closed;
                self.ui.artifact_sidebar_open = false;
                Ok(())
            }
            MouseClickAction::SetRightSidebarThin => {
                self.close_ephemeral()?;
                self.ui.panel_state.right = crate::tui::shell::PanelMode::Thin;
                self.ui.artifact_sidebar_open = true;
                Ok(())
            }
            MouseClickAction::OpenProviderDropdown => {
                if self.ui.visible_providers.is_empty() {
                    self.open_credential_manager(false);
                    return Ok(());
                }
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
        match settings.enter(&catalog) {
            crate::tui::settings_panel::EnterResult::NavigateSetting(id) => {
                self.ui.settings_v2 = None;
                self.open_settings_page(Some(id));
            }
            crate::tui::settings_panel::EnterResult::RunCommand(id) => {
                if let Some(action) = crate::tui::settings_panel::command_action(id) {
                    self.ui.settings_v2 = None;
                    self.action_tx.send(action).ok();
                }
            }
            crate::tui::settings_panel::EnterResult::Nothing => {}
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
