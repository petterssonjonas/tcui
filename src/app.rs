use std::collections::HashMap;
use std::io::IsTerminal;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

pub mod action;
mod artifacts;
mod chat_files;
mod connection;
mod conversations;
pub(crate) mod credential_manager;
mod ephemeral;
mod export;
pub mod generated_file;
mod input;
mod input_events;
pub mod message;
mod pending_documents;
mod popups;
mod providers;
mod request_control;
pub(crate) mod requests;
mod runtime;
mod settings;
pub mod submission;
mod submission_runtime;
pub mod tab;
mod terminal;
mod terminal_workspace;
mod update;
pub(crate) mod vault_mutation_execute;
mod vault_profiles;

#[cfg(test)]
#[path = "app/request_only_document_tests.rs"]
mod request_only_document_tests;

#[cfg(test)]
#[path = "app/vault_mutation_preview_tests.rs"]
mod vault_mutation_preview_tests;

#[cfg(test)]
#[path = "app/vault_mutation_execute_tests.rs"]
mod vault_mutation_execute_tests;

#[cfg(test)]
#[path = "app/chat_file_lifecycle.rs"]
mod chat_file_lifecycle;

#[cfg(test)]
#[path = "app/saved_file_migration_tests.rs"]
mod saved_file_migration_tests;

#[cfg(test)]
#[path = "app/markdown_vault_restart_tests.rs"]
mod markdown_vault_restart_tests;

pub use action::Action;
pub use generated_file::GeneratedFile;
pub use message::Message;
pub use tab::Tab;

pub(crate) use artifacts::local_media_sources;
pub(crate) use input::char_to_byte_index;

use crate::{config::AppConfig, llm::LlmClient, obsidian::Vault, storage::Storage};

pub struct TuiApp {
    pub storage: Storage,
    pub(crate) chat_files: crate::storage::chat_files::ChatSavedFileStore,
    pub config: Arc<tokio::sync::RwLock<AppConfig>>,
    pub ui: crate::ui::UI,
    pub vault: Option<Arc<Vault>>,
    pub action_tx: tokio::sync::mpsc::UnboundedSender<Action>,
    pub action_rx: tokio::sync::mpsc::UnboundedReceiver<Action>,
    pub system_prompt: String,
    pub ctrl_c_count: u32,
    pub last_ctrl_c: Option<std::time::Instant>,
    pub(crate) cancel_escape: Option<(submission::SubmissionId, std::time::Instant)>,
    pub(crate) requests: HashMap<submission::SubmissionId, requests::ConversationRequest>,
    pub terminal_has_focus: Arc<AtomicBool>,
    pub(crate) pending_documents:
        HashMap<crate::ui::app_tabs::AppTabId, submission::PendingDocuments>,
    pub(crate) submission_states:
        HashMap<crate::ui::app_tabs::AppTabId, submission::SubmissionState>,
    pub(crate) pending_document_discard_action:
        Option<pending_documents::PendingDocumentDiscardAction>,
    pub(crate) security_restart: Option<crate::protection::ui::SecurityRestart>,
    pub(crate) security_notice: Option<String>,
    background_tasks: std::sync::Mutex<Vec<tokio::task::JoinHandle<()>>>,
}

impl TuiApp {
    pub fn new(
        storage: Storage,
        config: Arc<tokio::sync::RwLock<AppConfig>>,
        _llm: Arc<LlmClient>,
        vault: Option<Arc<Vault>>,
    ) -> Self {
        let mut ui = crate::ui::UI::new();
        let loaded_config_snapshot = config.try_read().ok().map(|cfg| cfg.clone());
        let config_snapshot = loaded_config_snapshot.as_ref().cloned().unwrap_or_default();
        let reset_startup = crate::storage::saved_file_migration::SavedFileMigration::startup(
            loaded_config_snapshot.as_ref(),
        );
        let reset_warning = reset_startup.warning;
        #[cfg(test)]
        let chat_files = if std::env::var_os("TCUI_TEST_SAVED_FILE_MIGRATION").is_some() {
            crate::storage::chat_files::ChatSavedFileStore::new(
                reset_startup.saved_root,
                reset_startup.data_paths,
            )
        } else {
            crate::storage::chat_files::ChatSavedFileStore::discover()
        };
        #[cfg(not(test))]
        let chat_files = crate::storage::chat_files::ChatSavedFileStore::new(
            reset_startup.saved_root,
            reset_startup.data_paths,
        );
        let orphan_cleanup_error = chat_files.cleanup_orphans().err();
        let (action_tx, action_rx) = tokio::sync::mpsc::unbounded_channel();
        let system_prompt = Self::load_system_prompt();
        crate::theme::set_active_theme(&config_snapshot.theme);

        ui.user_alignment = config_snapshot.user_alignment;
        ui.ai_alignment = config_snapshot.ai_alignment;
        ui.markdown_mode = config_snapshot.markdown_mode;
        ui.show_selector = config_snapshot.show_selector;
        ui.show_chat_scrollbar = config_snapshot.show_chat_scrollbar;
        ui.collapse_thinking = config_snapshot.collapse_thinking;
        ui.kitty_enhanced_text = false;
        ui.kitty_heading_downscale = config_snapshot.kitty_heading_downscale;
        ui.image_protocol = config_snapshot.image_protocol.clone();
        ui.web_search_enabled = config_snapshot.web_search.enabled;
        ui.show_saved_files = config_snapshot.show_saved_files;
        ui.show_memories = config_snapshot.show_memories;
        ui.show_markdown_vault = config_snapshot.show_markdown_vault;
        ui.db_providers = Self::provider_entries_with_local(&config_snapshot, None);
        ui.panel_state = crate::tui::shell::PanelState::from_config(&config_snapshot.tui);
        ui.sidebar_open = ui.panel_state.left != crate::tui::shell::PanelMode::Closed;
        ui.artifact_sidebar_open = ui.panel_state.right != crate::tui::shell::PanelMode::Closed;
        ui.tui_config = config_snapshot.tui.clone();
        ui.keybinding_overrides = config_snapshot.tui.keybinding_overrides.clone();
        ui.disabled_providers = config_snapshot.disabled_providers.iter().cloned().collect();
        ui.disabled_models = config_snapshot.disabled_models.iter().cloned().collect();
        let _ = storage.sync_providers(&config_snapshot.providers);
        if let Ok(mut providers) = storage.get_providers() {
            if config_snapshot.local_inference.enabled {
                providers.push(crate::llm::local::local_provider_entry(
                    &config_snapshot.local_inference,
                    None,
                ));
            }
            ui.db_providers = providers;
        }
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

        let mut background_tasks = Vec::new();
        if std::io::stdin().is_terminal() {
            let model_action_tx = action_tx.clone();
            background_tasks.push(tokio::task::spawn_blocking(move || {
                let Ok(runtime_storage) = crate::storage::Storage::new() else {
                    return;
                };
                let rt = tokio::runtime::Handle::current();
                rt.block_on(async {
                    crate::llm::model_fetcher::refresh_all_models(
                        &runtime_storage,
                        |provider, models| {
                            let _ =
                                model_action_tx.send(Action::SetProviderModels(provider, models));
                        },
                    )
                    .await;
                });
            }));
        }

        let app = Self {
            storage,
            chat_files,
            config,
            ui,
            vault,
            action_tx,
            action_rx,
            system_prompt,
            ctrl_c_count: 0,
            last_ctrl_c: None,
            cancel_escape: None,
            requests: HashMap::new(),
            terminal_has_focus: Arc::new(AtomicBool::new(true)),
            pending_documents: HashMap::new(),
            submission_states: HashMap::new(),
            pending_document_discard_action: None,
            security_restart: None,
            security_notice: None,
            background_tasks: std::sync::Mutex::new(background_tasks),
        };
        let mut app = app;
        app.refresh_visible_selectors();
        app.initialize_chat_state();
        app.refresh_artifact_sidebar_catalogs();
        if orphan_cleanup_error.is_some() {
            app.ui
                .show_toast("Some deleted-chat files still need cleanup.".to_string());
        }
        if let Some(error) = reset_warning {
            app.ui
                .show_toast(format!("Legacy saved-file migration skipped: {error}"));
        }
        app.queue_connection_check_for_active_tab();
        app
    }

    pub(crate) fn lock_application(&mut self) -> Result<(), String> {
        self.request_security_restart(crate::protection::ui::SecurityRestart::Lock);
        Ok(())
    }

    pub(crate) fn unlock_application(&mut self) {
        self.request_security_restart(crate::protection::ui::SecurityRestart::Unlock);
    }

    pub(crate) fn initialize_application_password(&mut self) -> Result<(), String> {
        self.request_security_restart(crate::protection::ui::SecurityRestart::InitializePassword);
        Ok(())
    }

    pub(crate) fn recover_security_restart(&mut self, error: &color_eyre::eyre::Report) -> bool {
        if self.security_restart.is_none() || self.security_notice.is_some() {
            return false;
        }
        self.security_restart = None;
        self.ui.protection_blocked = false;
        self.ui.show_toast(format!(
            "Protection transition stopped: {error}\nThe application is NOT locked. Unsaved responses remain available. Fix the save error, then retry /lock, /unlock, or /security."
        ));
        true
    }

    fn has_unsaved_security_drafts(&self) -> bool {
        self.pending_documents
            .values()
            .any(|documents| !documents.is_empty())
            || self.ui.tabs.iter().any(|tab| {
                !tab.input_content.is_empty()
                    || tab
                        .input_history_draft
                        .as_ref()
                        .is_some_and(|draft| !draft.is_empty())
            })
            || self.ui.ephemeral.is_some()
            || self.ui.editor_popup.is_some()
    }

    fn has_security_process_close_risk(&self) -> bool {
        self.ui.terminals.has_running()
            || self.ui.app_tabs.tabs().iter().any(|tab| {
                self.ui
                    .markdown_vault_workspaces
                    .get(tab.id())
                    .and_then(|state| state.editor.as_ref())
                    .is_some_and(|editor| editor.has_unsaved_risk())
            })
    }

    pub(crate) fn lock_application_after_error(&mut self, error: String) {
        // Authentication has already revoked the key: do not leave caches visible behind a modal.
        let process_notice = if self.has_security_process_close_risk() {
            "\nRunning terminal and vault editor processes were closed for security; unsaved work may have been lost."
        } else {
            ""
        };
        self.security_notice = Some(format!(
            "Application locked after authentication failed: {error}{process_notice}"
        ));
        self.security_restart = Some(crate::protection::ui::SecurityRestart::Lock);
        self.ui.protection_blocked = true;
        self.pending_document_discard_action = None;
        self.ui.active_modal = None;
    }

    fn request_security_restart(&mut self, restart: crate::protection::ui::SecurityRestart) {
        if self.has_unsaved_security_drafts() || self.has_security_process_close_risk() {
            self.pending_document_discard_action =
                Some(pending_documents::PendingDocumentDiscardAction::SecurityRestart(restart));
            self.ui.active_modal = Some(crate::ui::Modal::DiscardSecurityDrafts);
        } else {
            self.security_restart = Some(restart);
            self.ui.protection_blocked = true;
        }
    }

    fn track_background_task(&self, task: tokio::task::JoinHandle<()>) {
        let mut tasks = self
            .background_tasks
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        tasks.retain(|task| !task.is_finished());
        tasks.push(task);
    }

    async fn stop_background_tasks(&mut self) {
        let tasks = std::mem::take(
            self.background_tasks
                .get_mut()
                .unwrap_or_else(|error| error.into_inner()),
        );
        for task in &tasks {
            task.abort();
        }
        // Already-running blocking storage work cannot be aborted: finish it before clearing keys.
        for task in tasks {
            let _ = task.await;
        }
    }

    #[cfg(test)]
    pub fn pending_documents_mut(&mut self) -> &mut submission::PendingDocuments {
        let tab_id = self.input_context_id();
        self.pending_documents.entry(tab_id).or_default()
    }

    #[cfg(test)]
    pub fn submission_state(&self) -> Option<&submission::SubmissionState> {
        self.submission_states.get(&self.input_context_id())
    }

    pub(crate) fn transition_submission(
        &mut self,
        tab_id: crate::ui::app_tabs::AppTabId,
        transition: fn(
            submission::SubmissionState,
        ) -> Result<
            submission::SubmissionState,
            submission::SubmissionTransitionError,
        >,
    ) -> color_eyre::Result<()> {
        let state = self
            .submission_states
            .remove(&tab_id)
            .ok_or_else(|| color_eyre::eyre::eyre!("submission state is unavailable"))?;
        self.submission_states.insert(tab_id, transition(state)?);
        Ok(())
    }

    pub(crate) fn load_system_prompt() -> String {
        std::fs::read_to_string("assets/TCUI.md")
            .unwrap_or_else(|_| include_str!("../assets/TCUI.md").to_owned())
    }

    pub(crate) fn refresh_visible_selectors(&mut self) {
        let Ok(config) = self.config.try_read() else {
            return;
        };
        self.ui.visible_providers =
            Self::filter_visible_providers(&self.ui.db_providers, &self.ui.disabled_providers);
        self.ui.visible_providers.retain(|(name, _, _, _, _)| {
            crate::llm::local::is_local_provider(name)
                || crate::config::KeyStore::has_credential(&config, name).unwrap_or(false)
        });
        drop(config);
        let Some(tab) = self.ui.tabs.get(self.ui.active_tab) else {
            self.ui.current_models.clear();
            self.ui.current_reasoning_options.clear();
            return;
        };
        let mut provider = tab.tab.provider.clone();
        let mut model = tab.tab.model.clone();
        let mut reasoning_effort = tab.tab.reasoning_effort.clone();
        let mut manual_model_override = tab.manual_model_override;
        if !self
            .ui
            .visible_providers
            .iter()
            .any(|(name, _, _, _, _)| name == &provider)
        {
            if let Some((next_provider, _, _, _, _)) = self.ui.visible_providers.first() {
                provider = next_provider.clone();
                model.clear();
            } else {
                provider.clear();
                model.clear();
            }
            manual_model_override = false;
        }
        self.ui.current_models = self.visible_models_for_provider(&provider);
        if model.trim().is_empty()
            || (!manual_model_override
                && (!self.ui.current_models.is_empty()
                    || crate::llm::auth::canonical_provider_name(&provider) == "Codex")
                && !self.ui.current_models.iter().any(|entry| entry.id == model))
        {
            model = self
                .ui
                .current_models
                .first()
                .map(|entry| entry.id.clone())
                .unwrap_or_default();
            reasoning_effort = None;
        }
        let selected_model = self
            .ui
            .current_models
            .iter()
            .find(|entry| entry.id == model);
        let default_reasoning_effort = selected_model
            .and_then(|entry| entry.default_reasoning_effort.as_ref())
            .cloned();
        self.ui.current_reasoning_options = selected_model
            .map(|entry| Self::reasoning_options_for(&provider, entry))
            .unwrap_or_default();
        if self.ui.current_reasoning_options.is_empty() {
            reasoning_effort = None;
        } else if reasoning_effort.as_ref().is_none_or(|value| {
            !self
                .ui
                .current_reasoning_options
                .iter()
                .any(|option| option == value)
        }) {
            reasoning_effort = default_reasoning_effort
                .filter(|default| self.ui.current_reasoning_options.contains(default))
                .or_else(|| {
                    self.ui
                        .current_reasoning_options
                        .iter()
                        .find(|option| option.as_str() == "medium")
                        .cloned()
                })
                .or_else(|| self.ui.current_reasoning_options.first().cloned());
        }
        if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
            if tab.tab.provider != provider {
                tab.tab.endpoint = None;
            }
            tab.tab.provider = provider;
            tab.tab.model = model;
            tab.manual_model_override = manual_model_override;
            tab.tab.reasoning_effort = reasoning_effort;
        }
    }

    pub(crate) fn initialize_chat_state(&mut self) {
        for tab_id in 0..self.ui.tabs.len() {
            if self.ensure_tab_has_active_conversation(tab_id).is_err() {
                self.ui.show_toast("Chat history unavailable.".to_string());
            }
        }
        if self.storage.created_default_key() {
            self.ui.show_toast(format!(
                "Back up {}; encrypted chats and memories cannot be recovered without it.",
                Storage::chat_key_path().display()
            ));
        }
    }

    pub(crate) fn scroll_active_chat_lines(&mut self, delta: isize) {
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

    pub(crate) fn scroll_active_chat_page(&mut self, down: bool) {
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

    pub(crate) fn jump_to_adjacent_answer(&mut self, forward: bool) {
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

    pub(crate) fn refresh_input_popup(&mut self) {
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
        let mut commands = [
            ("/fork", "Continue this conversation in an independent chat"),
            ("/temp", "Open a temporary fork in the wide right sidebar"),
            ("/temp close", "Close and delete the temporary conversation"),
            ("/cancel", "Cancel this conversation's active request"),
            (
                "/steer ",
                "Steer an active request when the transport supports it",
            ),
            ("/settings", "Open the Settings tab"),
            ("/security", "Choose app and data protection"),
            ("/unlock", "Unlock selected protected data"),
            ("/lock", "Lock application protection"),
            ("/terminal", "Open an interactive terminal"),
            ("/providers", "Manage your added provider connections"),
            ("/connect", "Add or reconnect a provider"),
            (
                "/vault",
                "Unlock and manage stored credentials and machine secrets",
            ),
            ("/vault-search ", "Search the Markdown Vault"),
            ("/model ", "Enter a provider model ID manually"),
            ("/help", "Show the help document"),
            ("/keybinds", "Show keyboard shortcuts"),
            ("/theme ", "Select and apply a theme"),
            ("/skills", "Show installed skills"),
            ("/mcp", "Show MCP servers"),
            ("/remindme ", "Schedule, list, or forget reminders"),
            ("/schedule-command ", "Schedule, list, or forget reminders"),
            ("/web", "Toggle local web search"),
            ("/quit", "Quit the app"),
        ];
        // A typed command outranks another command's descriptive substring match.
        if let Some(index) = commands
            .iter()
            .position(|(command, _)| command.trim_end() == trimmed.trim_end())
        {
            commands[..=index].rotate_right(1);
        }
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

    pub(crate) fn open_external_target(&mut self, target: &str) {
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

    pub(crate) fn generate_title(content: &str) -> String {
        let mut words = content.split_whitespace();
        let mut title = String::with_capacity(content.len().min(16 * 4) + 3);
        let mut length = 0;
        for (index, word) in words.by_ref().take(5).enumerate() {
            for character in (index > 0).then_some(' ').into_iter().chain(word.chars()) {
                if length == 16 {
                    title.push_str("...");
                    return title;
                }
                title.push(character);
                length += 1;
            }
        }
        if words.next().is_some() {
            title.push_str("...");
        }
        if title.is_empty() {
            "New Chat".to_string()
        } else {
            title
        }
    }

    pub(crate) fn load_conversation(&mut self, conv_id: i64) -> color_eyre::Result<()> {
        self.load_conversation_into_tab(self.ui.active_tab, conv_id)
    }

    pub(crate) fn provider_config(&self, provider: &str) -> Option<(String, String, String)> {
        self.ui
            .db_providers
            .iter()
            .find(|(name, _, _, _, _)| name == provider || name.eq_ignore_ascii_case(provider))
            .map(|(_, endpoint, env_var, backend_type, _)| {
                (endpoint.clone(), env_var.clone(), backend_type.clone())
            })
    }
}

#[cfg(test)]
pub(crate) mod tests;

#[cfg(test)]
#[path = "app/runtime_guard_tests.rs"]
mod runtime_guard_tests;
