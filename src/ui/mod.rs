#![allow(dead_code)]
use std::collections::{HashMap, HashSet};

use ratatui::{
    prelude::*,
    widgets::{Block, Borders},
    Frame,
};

pub mod artifact_sidebar;
pub mod chat_tab;
pub mod components;
pub mod modals;
pub mod session_list;
pub mod settings_tab;
pub mod sidebar;
pub mod status_bar;
pub mod tab_bar;
pub mod top_bar;

use crate::config::app_config::{MarkdownMode, TextAlignment};
use crate::ui::components::terminal_capabilities::TerminalCapabilities;
use artifact_sidebar::{ArtifactEntry, ArtifactSidebar, ArtifactSidebarState};
use modals::artifact_viewer::ArtifactViewerState;
use modals::list_popup::ListPopup;
use modals::quit_confirm::QuitConfirmModal;
use modals::save_file::SaveFileDialog;
use settings_tab::SettingsPopup;
use sidebar::Sidebar;
use status_bar::{ConnectionStatus, StatusBar, StatusBarAreas};
use top_bar::TopBar;

#[derive(Debug, Clone)]
pub enum Action {
    Quit,
    SendMessage(String),
    StreamResponse(usize, usize, String),
    StreamThinking(usize, usize, String),
    StopStream(usize),
    AddMessage(usize, crate::app::message::Message),
    LoadConversation(i64),
    NewConversation(usize),
    UpdateModel(String),
    UpdateStatus(String),
    SetTitle(String),
    SwitchTab(usize),
    AddTab(crate::app::tab::Tab),
    RemoveTab(usize),
    SetProviderModels(String, Vec<crate::ui::settings_tab::ModelInfo>),
    ToggleSessionList,
    ToggleSidebar,
    ToggleArtifactSidebar,
}

pub struct UI {
    pub tabs: Vec<ChatTabState>,
    pub active_tab: usize,
    pub sidebar_open: bool,
    pub artifact_sidebar_open: bool,
    pub show_session_list: bool,
    pub active_modal: Option<Modal>,
    pub focus_input: bool,
    pub show_settings: bool,
    pub settings_popup: Option<SettingsPopup>,
    pub save_file_dialog: Option<SaveFileDialog>,
    pub artifact_viewer: Option<ArtifactViewerState>,
    pub list_popup: Option<ListPopup>,
    pub last_area: Option<Rect>,
    pub chat_area: Option<Rect>,
    pub connection_status: ConnectionStatus,
    pub connection_message: Option<String>,
    pub modal_areas: Option<ModalAreas>,
    pub settings_tab_areas: Option<Vec<Rect>>,
    pub status_bar_areas: Option<StatusBarAreas>,
    pub artifact_sidebar_state: ArtifactSidebarState,
    pub vault_artifacts: Vec<ArtifactEntry>,
    pub mcps: Vec<String>,
    pub user_alignment: TextAlignment,
    pub ai_alignment: TextAlignment,
    pub markdown_mode: MarkdownMode,
    pub show_selector: bool,
    pub show_chat_scrollbar: bool,
    pub collapse_thinking: bool,
    pub kitty_enhanced_text: bool,
    pub kitty_text_max_scale: u8,
    pub image_protocol: String,
    pub terminal_capabilities: TerminalCapabilities,
    pub web_search_enabled: bool,
    pub db_providers: Vec<(String, String, String, String, String)>,
    pub current_models: Vec<crate::ui::settings_tab::ModelInfo>,
    pub frame_tick: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct ModalAreas {
    pub yes: Rect,
    pub no: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Modal {
    QuitConfirm,
}

pub struct ChatTabState {
    pub tab: crate::app::tab::Tab,
    pub messages: Vec<crate::app::message::Message>,
    pub active_conversation: i64,
    pub conversations: Vec<ConversationEntry>,
    pub input_content: String,
    pub input_cursor: usize,
    pub input_scroll: usize,
    pub scroll_offset: usize,
    pub streaming: bool,
    pub pending_diff: Option<Diff>,
    pub generated_title: Option<String>,
    pub provider_dropdown_open: bool,
    pub model_dropdown_open: bool,
    pub provider_hit_area: Option<Rect>,
    pub model_hit_area: Option<Rect>,
    pub input_area: Option<Rect>,
    pub input_text_area: Option<Rect>,
    pub dropdown_item_areas: Vec<Rect>,
    pub dropdown_scroll_offset: usize,
    pub chat_scrollbar_area: Option<Rect>,
    pub chat_scrollbar_thumb: Option<Rect>,
    pub thinking_hit_areas: Vec<(usize, Rect)>,
    pub link_hit_areas: Vec<(Rect, String)>,
    pub thinking_fold_overrides: HashSet<usize>,
    pub scroll_to_message: Option<usize>,
    pub answer_anchor_lines: Vec<(usize, usize)>,
    pub total_rendered_lines: usize,
    pub message_viewport_height: usize,
    pub temporary_artifacts: Vec<ArtifactEntry>,
    pub image_states: HashMap<String, crate::ui::components::image_block::ImageBlockState>,
}

pub struct ConversationEntry {
    pub id: i64,
    pub title: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct Diff {
    pub path: String,
    pub old_content: String,
    pub new_content: String,
}

impl Diff {
    pub fn new(path: String, old_content: String, new_content: String) -> Self {
        Self {
            path,
            old_content,
            new_content,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum DiffStatus {
    Pending,
    Accepted,
    Rejected,
}

impl UI {
    pub fn new() -> Self {
        Self {
            tabs: vec![ChatTabState {
                tab: crate::app::tab::Tab::new(
                    "Chat".to_string(),
                    "Ollama".to_string(),
                    "llama3.1".to_string(),
                ),
                messages: vec![],
                active_conversation: 0,
                conversations: vec![],
                input_content: String::new(),
                input_cursor: 0,
                input_scroll: 0,
                scroll_offset: 0,
                streaming: false,
                pending_diff: None,
                generated_title: None,
                provider_dropdown_open: false,
                model_dropdown_open: false,
                provider_hit_area: None,
                model_hit_area: None,
                input_area: None,
                input_text_area: None,
                dropdown_item_areas: vec![],
                dropdown_scroll_offset: 0,
                chat_scrollbar_area: None,
                chat_scrollbar_thumb: None,
                thinking_hit_areas: vec![],
                link_hit_areas: vec![],
                thinking_fold_overrides: HashSet::new(),
                scroll_to_message: None,
                answer_anchor_lines: vec![],
                total_rendered_lines: 0,
                message_viewport_height: 0,
                temporary_artifacts: vec![],
                image_states: HashMap::new(),
            }],
            active_tab: 0,
            sidebar_open: false,
            artifact_sidebar_open: false,
            show_session_list: false,
            active_modal: None,
            focus_input: true,
            show_settings: false,
            settings_popup: None,
            save_file_dialog: None,
            artifact_viewer: None,
            list_popup: None,
            last_area: None,
            chat_area: None,
            connection_status: ConnectionStatus::Checking,
            connection_message: None,
            modal_areas: None,
            settings_tab_areas: None,
            status_bar_areas: None,
            artifact_sidebar_state: ArtifactSidebarState::default(),
            vault_artifacts: vec![],
            mcps: vec![],
            user_alignment: TextAlignment::Right,
            ai_alignment: TextAlignment::Left,
            markdown_mode: MarkdownMode::Off,
            show_selector: true,
            show_chat_scrollbar: true,
            collapse_thinking: true,
            kitty_enhanced_text: true,
            kitty_text_max_scale: 3,
            image_protocol: "auto".to_string(),
            terminal_capabilities: TerminalCapabilities::detect(),
            web_search_enabled: false,
            db_providers: vec![],
            current_models: vec![],
            frame_tick: 0,
        }
    }

    pub fn render(&mut self, f: &mut Frame) {
        let theme = crate::theme::active_theme();
        self.frame_tick = self.frame_tick.wrapping_add(1);
        let area = f.area();
        self.last_area = Some(area);
        self.modal_areas = None;
        self.settings_tab_areas = None;
        self.status_bar_areas = None;
        self.chat_area = None;

        let main_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(0),
                Constraint::Length(1),
            ])
            .split(area);

        // Top bar
        let top_bar = TopBar::new(
            &self.tabs,
            self.active_tab,
            self.sidebar_open,
            self.artifact_sidebar_open,
        );
        top_bar.render(f, main_layout[0]);

        let left_width = if self.sidebar_open { 24 } else { 0 };
        let show_artifact_sidebar =
            self.artifact_sidebar_open && main_layout[1].width.saturating_sub(left_width) >= 72;
        let max_artifact_width = main_layout[1]
            .width
            .saturating_sub(left_width)
            .saturating_sub(24);
        let artifact_width = if !show_artifact_sidebar {
            0
        } else if max_artifact_width < 22 {
            max_artifact_width
        } else {
            max_artifact_width.min(32)
        };
        let content_layout = [
            Rect::new(
                main_layout[1].x,
                main_layout[1].y,
                left_width,
                main_layout[1].height,
            ),
            Rect::new(
                main_layout[1].x + left_width,
                main_layout[1].y,
                main_layout[1]
                    .width
                    .saturating_sub(left_width)
                    .saturating_sub(artifact_width),
                main_layout[1].height,
            ),
            Rect::new(
                main_layout[1].right().saturating_sub(artifact_width),
                main_layout[1].y,
                artifact_width,
                main_layout[1].height,
            ),
        ];

        // Sidebar
        if self.sidebar_open {
            let active_tab = &self.tabs[self.active_tab];
            let show_new_chat =
                active_tab.messages.is_empty() && active_tab.generated_title.is_none();
            let sidebar = Sidebar::new(
                &active_tab.conversations,
                active_tab.active_conversation,
                show_new_chat,
                show_new_chat,
            );
            sidebar.render(f, content_layout[0]);
        }

        if let Some(tab_state) = self.tabs.get_mut(self.active_tab) {
            if artifact_width > 0 {
                let mut artifact_sidebar = ArtifactSidebar::new(
                    &tab_state.temporary_artifacts,
                    &self.vault_artifacts,
                    !self.vault_artifacts.is_empty(),
                    &mut self.artifact_sidebar_state,
                );
                artifact_sidebar.render(f, content_layout[2]);
            } else {
                self.artifact_sidebar_state = ArtifactSidebarState::default();
            }
        }

        self.chat_area = Some(content_layout[1]);

        // Content area
        if let Some(tab_state) = self.tabs.get_mut(self.active_tab) {
            let pane_title = if tab_state.tab.model.is_empty() {
                format!(" {} ", tab_state.tab.provider)
            } else {
                format!(" {} / {} ", tab_state.tab.provider, tab_state.tab.model)
            };
            let pane = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border))
                .title(Line::from(Span::styled(
                    pane_title,
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD),
                )))
                .style(theme.panel_style());
            let pane_area = pane.inner(content_layout[1]);
            f.render_widget(pane, content_layout[1]);

            let chat_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(0)])
                .split(pane_area);

            let streaming = tab_state.streaming;
            {
                let mut chat_tab = chat_tab::ChatTab::new(
                    tab_state,
                    chat_tab::ChatTabProps {
                        user_alignment: self.user_alignment,
                        ai_alignment: self.ai_alignment,
                        markdown_mode: self.markdown_mode,
                        collapse_thinking: self.collapse_thinking,
                        show_chat_scrollbar: self.show_chat_scrollbar,
                        kitty_enhanced_text: self.kitty_enhanced_text,
                        kitty_text_max_scale: self.kitty_text_max_scale,
                        image_protocol: &self.image_protocol,
                        terminal_capabilities: self.terminal_capabilities,
                        frame_tick: self.frame_tick,
                        providers: &self.db_providers,
                        models: &self.current_models,
                    },
                );
                chat_tab.render(f, chat_chunks[0]);
            }

            let status_bar = StatusBar {
                status: self.connection_status,
                message: self.connection_message.clone(),
                mcps: self.mcps.clone(),
                working: streaming,
                tick: self.frame_tick,
                provider: tab_state.tab.provider.clone(),
                model: tab_state.tab.model.clone(),
                show_selector: self.show_selector,
                web_search_enabled: self.web_search_enabled,
            };
            let status_areas = status_bar.render(f, main_layout[2]);
            self.status_bar_areas = Some(status_areas);
            tab_state.provider_hit_area = status_areas.provider;
            tab_state.model_hit_area = status_areas.model;

            let mut chat_tab = chat_tab::ChatTab::new(
                tab_state,
                chat_tab::ChatTabProps {
                    user_alignment: self.user_alignment,
                    ai_alignment: self.ai_alignment,
                    markdown_mode: self.markdown_mode,
                    collapse_thinking: self.collapse_thinking,
                    show_chat_scrollbar: self.show_chat_scrollbar,
                    kitty_enhanced_text: self.kitty_enhanced_text,
                    kitty_text_max_scale: self.kitty_text_max_scale,
                    image_protocol: &self.image_protocol,
                    terminal_capabilities: self.terminal_capabilities,
                    frame_tick: self.frame_tick,
                    providers: &self.db_providers,
                    models: &self.current_models,
                },
            );
            chat_tab.render_dropdowns(f);
        }

        // Settings popup overlay
        if self.show_settings {
            if let Some(ref mut settings) = self.settings_popup {
                settings.render(f);
                if let Some(popup_area) = self.last_area {
                    let popup_rect = settings_tab::SettingsPopup::popup_area(popup_area);
                    self.settings_tab_areas = Some(settings.tab_hit_areas(popup_rect));
                }
            }
        }

        if let Some(ref mut dialog) = self.save_file_dialog {
            dialog.render(f, area);
        }

        if let (Some(viewer), Some(chat_area)) = (&mut self.artifact_viewer, self.chat_area) {
            viewer.render(
                f,
                chat_area,
                modals::artifact_viewer::ArtifactViewerProps {
                    markdown_mode: self.markdown_mode,
                    kitty_enhanced_text: self.kitty_enhanced_text,
                    kitty_text_max_scale: self.kitty_text_max_scale,
                    image_protocol: &self.image_protocol,
                    terminal_capabilities: self.terminal_capabilities,
                },
            );
        }

        if let Some(ref popup) = self.list_popup {
            popup.render(f, area);
        }

        // Modal overlay (rendered on top of everything)
        if let Some(modal) = self.active_modal {
            match modal {
                Modal::QuitConfirm => {
                    let modal = QuitConfirmModal::new();
                    let areas = modal.render(f);
                    self.modal_areas = Some(ModalAreas {
                        yes: areas.yes,
                        no: areas.no,
                    });
                }
            }
        }
    }

    pub fn add_tab(&mut self, tab: crate::app::tab::Tab) {
        self.tabs.push(ChatTabState {
            tab,
            messages: vec![],
            active_conversation: 0,
            conversations: vec![],
            input_content: String::new(),
            input_cursor: 0,
            input_scroll: 0,
            scroll_offset: 0,
            streaming: false,
            pending_diff: None,
            generated_title: None,
            provider_dropdown_open: false,
            model_dropdown_open: false,
            provider_hit_area: None,
            model_hit_area: None,
            input_area: None,
            input_text_area: None,
            dropdown_item_areas: vec![],
            dropdown_scroll_offset: 0,
            chat_scrollbar_area: None,
            chat_scrollbar_thumb: None,
            thinking_hit_areas: vec![],
            link_hit_areas: vec![],
            thinking_fold_overrides: HashSet::new(),
            scroll_to_message: None,
            answer_anchor_lines: vec![],
            total_rendered_lines: 0,
            message_viewport_height: 0,
            temporary_artifacts: vec![],
            image_states: HashMap::new(),
        });
        self.active_tab = self.tabs.len() - 1;
    }

    pub fn add_chat_to_active_tab(&mut self, title: String) {
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.conversations.push(ConversationEntry {
                id: tab.active_conversation,
                title,
                created_at: String::new(),
            });
        }
    }

    pub fn remove_tab(&mut self, idx: usize) {
        if self.tabs.len() > 1 && idx < self.tabs.len() {
            self.tabs.remove(idx);
            if self.active_tab >= self.tabs.len() {
                self.active_tab = self.tabs.len() - 1;
            }
        }
    }

    pub fn add_message(&mut self, tab_id: usize, msg: crate::app::message::Message) {
        if let Some(tab) = self.tabs.get_mut(tab_id) {
            tab.messages.push(msg);
            tab.scroll_to_message = Some(tab.messages.len().saturating_sub(1));
        }
    }

    pub fn add_generated_file(&mut self, tab_id: usize, file: crate::app::GeneratedFile) {
        if let Some(tab) = self.tabs.get_mut(tab_id) {
            tab.temporary_artifacts.push(ArtifactEntry::temp_markdown(
                file.id,
                file.name,
                file.content,
            ));
        }
    }

    pub fn add_stream_content(&mut self, tab_id: usize, message_idx: usize, content: String) {
        if let Some(tab) = self.tabs.get_mut(tab_id) {
            if let Some(message) = tab.messages.get_mut(message_idx) {
                if message.role == "assistant" {
                    message.content.push_str(&content);
                    tab.scroll_to_message = Some(message_idx);
                }
            }
        }
    }

    pub fn add_stream_thinking(&mut self, tab_id: usize, message_idx: usize, content: String) {
        if let Some(tab) = self.tabs.get_mut(tab_id) {
            if let Some(message) = tab.messages.get_mut(message_idx) {
                if message.role == "assistant" {
                    message
                        .thinking_content
                        .get_or_insert_with(String::new)
                        .push_str(&content);
                    tab.scroll_to_message = Some(message_idx);
                }
            }
        }
    }

    pub fn finish_stream(&mut self, tab_id: usize) {
        if let Some(tab) = self.tabs.get_mut(tab_id) {
            tab.streaming = false;
        }
    }

    pub fn toggle_thinking_fold(&mut self, tab_id: usize, message_idx: usize) {
        if let Some(tab) = self.tabs.get_mut(tab_id) {
            if !tab.thinking_fold_overrides.insert(message_idx) {
                tab.thinking_fold_overrides.remove(&message_idx);
            }
            tab.scroll_to_message = Some(message_idx);
        }
    }

    pub fn set_status(&mut self, _status: String) {}
    pub fn set_title(&mut self, _title: String) {}
    pub fn show_diff(&mut self, _diff: Diff) {}
    pub fn update_model(&mut self, _model: String) {}
    pub fn update_status(&mut self, status: String) {
        self.connection_status = ConnectionStatus::Failed;
        self.connection_message = Some(status);
    }
}

#[cfg(test)]
mod tests {
    use super::UI;

    #[test]
    fn artifact_sidebar_starts_collapsed() {
        let ui = UI::new();
        assert!(!ui.artifact_sidebar_open);
    }
}
