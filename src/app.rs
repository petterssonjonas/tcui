use std::collections::HashSet;
use std::io::IsTerminal;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

pub mod action;
mod artifacts;
mod connection;
mod conversations;
mod export;
pub mod generated_file;
mod input;
mod input_events;
pub mod message;
mod popups;
mod providers;
mod runtime;
mod settings;
pub mod tab;
mod terminal;

pub use action::Action;
pub use generated_file::GeneratedFile;
pub use message::Message;
pub use tab::Tab;

pub(crate) use artifacts::local_media_sources;
pub(crate) use input::char_to_byte_index;
#[cfg(test)]
pub(crate) use ratatui::layout::Rect;

use crate::{config::AppConfig, llm::LlmClient, obsidian::Vault, storage::Storage};

pub struct TuiApp {
    pub storage: Arc<Storage>,
    pub config: Arc<tokio::sync::RwLock<AppConfig>>,
    #[allow(dead_code)]
    pub key_store: Arc<crate::config::KeyStore>,
    pub ui: crate::ui::UI,
    #[allow(dead_code)]
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
        ui.kitty_heading_downscale = config_snapshot.kitty_heading_downscale;
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
        app.initialize_chat_state();
        app.refresh_artifact_sidebar_catalogs();
        app.sync_message_media(app.ui.active_tab);
        app.queue_connection_check_for_active_tab();
        app
    }

    pub fn queue_update_check(&self) {
        if tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        let action_tx = self.action_tx.clone();
        tokio::spawn(async move {
            if let Ok(Some(release)) = crate::updater::available_release().await {
                let _ = action_tx.send(Action::ShowToast(format!(
                    "Update {} available. Run `tcui upgrade` to update.",
                    release.version
                )));
            }
        });
    }

    pub(crate) fn load_system_prompt() -> String {
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

    pub(crate) fn refresh_visible_selectors(&mut self) {
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

    pub(crate) fn initialize_chat_state(&mut self) {
        for tab_id in 0..self.ui.tabs.len() {
            if self.ensure_tab_has_active_conversation(tab_id).is_err() {
                self.ui.show_toast("Chat history unavailable.".to_string());
            }
        }
        if self.storage.created_default_key() {
            self.ui.show_toast(format!(
                "Back up {}; encrypted chats, memories, and saved provider keys cannot be recovered without it.",
                Storage::chat_key_path().display()
            ));
        }
    }

    pub(crate) fn sync_message_media(&mut self, tab_id: usize) {
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
mod tests;
