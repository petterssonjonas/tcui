#![allow(dead_code)]
use crate::config::app_config::{LocalServerType, MarkdownMode, TextAlignment};
use crate::config::McpServerConfig;
use ratatui::{
    layout::{Position, Rect},
    prelude::*,
    widgets::*,
    Frame,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SettingsTab {
    General,
    Keybindings,
    Providers,
    Models,
    Local,
    Mcp,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GeneralFocus {
    Theme,
    UserAlignment,
    AiAlignment,
    ArtifactSaveDir,
    ShowSelector,
    ShowChatScrollbar,
    CollapseThinking,
    KittyEnhancedText,
    KittyTextScale,
    WebSearchEnabled,
    QuitConfirmation,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GeneralDropdown {
    Theme,
    UserAlignment,
    AiAlignment,
    KittyTextScale,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LocalFocus {
    Enabled,
    Host,
    Port,
    ServerType,
    SelectedModel,
    ModelDirectory,
    HealthInterval,
    ConnectTimeout,
    RequestTimeout,
    ApiTokenEnv,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProvidersFocus {
    DefaultProvider,
    DefaultModel,
    SmallModel,
    List(usize),
    AddButton,
    EditButton,
    GrabEnvButton,
    ReloadModelsButton,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProvidersDropdown {
    DefaultProvider,
    DefaultModel,
    SmallProvider,
    SmallModel,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProvidersTabFocus {
    DefaultProvider,
    DefaultModel,
    SmallProvider,
    SmallModel,
    UseEnvToggle,
    ReloadModelsButton,
    AddProviderButton,
    EditProvidersButton,
    SavedKeyList(usize),
    PresetProvider(usize),
    OAuthProvider(usize),
    PopupApiKey,
    PopupSaveButton,
    PopupCancelButton,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ModelsTabFocus {
    Provider,
    Model(usize),
}

#[derive(Debug, Clone, Default)]
pub struct ModelsTabHitAreas {
    pub provider: Option<Rect>,
    pub provider_items: Vec<Rect>,
    pub model_rows: Vec<Rect>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProviderFormFocus {
    ProviderName,
    ProviderEndpoint,
    ProviderBackendType,
    ProviderApiKey,
    SubmitButton,
    CancelButton,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EditProvidersFocus {
    ProviderName(usize),
    DeleteButton(usize),
}

#[derive(Debug, Clone, Default)]
pub struct ProvidersTabHitAreas {
    pub default_provider: Option<Rect>,
    pub default_model: Option<Rect>,
    pub small_provider: Option<Rect>,
    pub small_model: Option<Rect>,
    pub default_provider_items: Vec<Rect>,
    pub default_model_items: Vec<Rect>,
    pub small_provider_items: Vec<Rect>,
    pub small_model_items: Vec<Rect>,
    pub use_env_toggle: Option<Rect>,
    pub grab_env_button: Option<Rect>,
    pub reload_models_button: Option<Rect>,
    pub add_button: Option<Rect>,
    pub edit_button: Option<Rect>,
    pub saved_key_rows: Vec<Rect>,
    pub oauth_rows: Vec<Rect>,
    pub preset_rows: Vec<Rect>,
    pub popup_name: Option<Rect>,
    pub popup_endpoint: Option<Rect>,
    pub popup_api_key: Option<Rect>,
    pub popup_save: Option<Rect>,
    pub popup_cancel: Option<Rect>,
}

#[derive(Debug, Clone, Default)]
pub struct LocalHitAreas {
    pub enabled: Option<Rect>,
    pub host: Option<Rect>,
    pub port: Option<Rect>,
    pub server_type: Option<Rect>,
    pub selected_model: Option<Rect>,
    pub model_directory: Option<Rect>,
    pub health_interval: Option<Rect>,
    pub connect_timeout: Option<Rect>,
    pub request_timeout: Option<Rect>,
    pub api_token_env: Option<Rect>,
}

#[derive(Debug, Clone, Default)]
pub struct McpHitAreas {
    pub rows: Vec<(usize, Rect)>,
}

#[derive(Debug, Clone, Default)]
pub struct ProviderFormHitAreas {
    pub name: Option<Rect>,
    pub endpoint: Option<Rect>,
    pub backend_type: Option<Rect>,
    pub api_key: Option<Rect>,
    pub submit_button: Option<Rect>,
    pub cancel_button: Option<Rect>,
    pub dropdown_items: Vec<Rect>,
}

#[derive(Debug, Clone, Default)]
pub struct EditProvidersHitAreas {
    pub provider_rows: Vec<(Rect, Rect)>,
}

#[derive(Debug, Clone, Default)]
pub struct GeneralHitAreas {
    pub theme: Option<Rect>,
    pub user_alignment: Option<Rect>,
    pub ai_alignment: Option<Rect>,
    pub artifact_save_dir: Option<Rect>,
    pub show_selector: Option<Rect>,
    pub show_chat_scrollbar: Option<Rect>,
    pub collapse_thinking: Option<Rect>,
    pub kitty_enhanced_text: Option<Rect>,
    pub kitty_text_scale: Option<Rect>,
    pub web_search_enabled: Option<Rect>,
    pub quit_confirmation: Option<Rect>,
    pub dropdown_items: Vec<Rect>,
}

#[derive(Debug, Clone, Default)]
pub struct PresetsOAuthHitAreas {
    pub oauth_rows: Vec<Rect>,
    pub preset_rows: Vec<Rect>,
    pub popup_name: Option<Rect>,
    pub popup_endpoint: Option<Rect>,
    pub popup_api_key: Option<Rect>,
    pub popup_save: Option<Rect>,
    pub popup_cancel: Option<Rect>,
}

#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub id: String,
    pub input_price: Option<f64>,
    pub output_price: Option<f64>,
    pub context_window: Option<u32>,
}

pub type ProviderEntry = (String, String, String, String, String);

#[derive(Debug, Clone)]
pub struct EditableProvider {
    pub name: String,
    pub endpoint: String,
    pub backend_type: String,
}

#[derive(Debug, Clone)]
pub struct ProviderFormState {
    pub title: String,
    pub submit_label: String,
    pub original_name: Option<String>,
    pub name: String,
    pub endpoint: String,
    pub backend_type: String,
    pub api_key: String,
    pub focus: ProviderFormFocus,
    pub dropdown_open: bool,
    pub hit_areas: ProviderFormHitAreas,
}

#[derive(Debug, Clone)]
pub struct EditProvidersPopupState {
    pub focus: Option<EditProvidersFocus>,
    pub hit_areas: EditProvidersHitAreas,
}

#[derive(Debug, Clone)]
pub struct PresetKeyPopupState {
    pub provider_name: String,
    pub endpoint: String,
    pub api_key: String,
}

#[derive(Debug, Clone)]
pub enum ProvidersAction {
    None,
    ToggleUseEnv,
    RefreshModels,
    SubmitAdd {
        provider: EditableProvider,
        api_key: String,
    },
    SubmitEdit {
        original_name: String,
        provider: EditableProvider,
        api_key: String,
    },
    DeleteProvider(String),
    SavePresetKey {
        provider_name: String,
        api_key: String,
    },
}

const ALIGNMENT_OPTIONS: &[TextAlignment] = &[
    TextAlignment::Left,
    TextAlignment::Middle,
    TextAlignment::Right,
];
const KITTY_TEXT_SCALE_OPTIONS: &[u8] = &[1, 2, 3, 4, 5, 6, 7];
const BACKEND_TYPE_OPTIONS: &[&str] = &[
    "openai",
    "anthropic",
    "gemini",
    "ollama",
    "openai-responses",
    "alibaba",
];
const PRESET_PROVIDER_NAMES: &[&str] = &[
    "OpenAI",
    "Anthropic",
    "Google AI",
    "Ollama",
    "OpenRouter",
    "Kilo Gateway",
    "Mistral",
    "Groq",
    "Berget.ai",
    "OpenCode Go",
    "OpenCode Zen",
];
const SEARCH_KEY_PROVIDERS: &[(&str, &str, &str)] = &[
    ("Exa Search", "https://api.exa.ai/search", "EXA_API_KEY"),
    (
        "Tavily Search",
        "https://api.tavily.com/search",
        "TAVILY_API_KEY",
    ),
    (
        "Firecrawl Search",
        "https://api.firecrawl.dev/v2/search",
        "FIRECRAWL_API_KEY",
    ),
];

#[derive(Debug, Clone)]
pub struct SettingsPopup {
    pub default_provider: String,
    pub default_model: String,
    pub small_model: String,
    pub active_tab: SettingsTab,
    pub use_env_keys: bool,
    pub saved_keys: Vec<(String, String)>,
    pub theme: String,
    pub user_alignment: TextAlignment,
    pub ai_alignment: TextAlignment,
    pub markdown_mode: MarkdownMode,
    pub artifact_save_dir: String,
    pub general_focus: GeneralFocus,
    pub general_hit_areas: GeneralHitAreas,
    pub general_dropdown_open: Option<GeneralDropdown>,
    pub available_models: Vec<ModelInfo>,
    pub db_providers: Vec<ProviderEntry>,
    pub show_selector: bool,
    pub show_chat_scrollbar: bool,
    pub collapse_thinking: bool,
    pub kitty_enhanced_text: bool,
    pub kitty_text_max_scale: u8,
    pub web_search_enabled: bool,
    pub quit_confirmation: bool,
    pub local_enabled: bool,
    pub local_host: String,
    pub local_port: String,
    pub local_server_type: LocalServerType,
    pub local_selected_model: String,
    pub local_model_directory: String,
    pub local_health_interval_seconds: String,
    pub local_connect_timeout_ms: String,
    pub local_request_timeout_ms: String,
    pub local_api_token_env: String,
    pub detected_local_server: Option<LocalServerType>,
    pub local_focus: LocalFocus,
    pub local_hit_areas: LocalHitAreas,
    pub mcp_servers: Vec<McpServerConfig>,
    pub mcp_focus: usize,
    pub mcp_hit_areas: McpHitAreas,
    pub providers_tab_list: Vec<EditableProvider>,
    pub providers_tab_focus: ProvidersTabFocus,
    pub providers_dropdown_open: Option<ProvidersDropdown>,
    pub dropdown_scroll_offset: usize,
    pub providers_tab_hit_areas: ProvidersTabHitAreas,
    pub models_provider: String,
    pub models_available_models: Vec<ModelInfo>,
    pub models_tab_focus: ModelsTabFocus,
    pub models_dropdown_open: bool,
    pub models_dropdown_scroll_offset: usize,
    pub models_tab_hit_areas: ModelsTabHitAreas,
    pub add_provider_popup: Option<ProviderFormState>,
    pub edit_providers_popup: Option<EditProvidersPopupState>,
    pub edit_provider_popup: Option<ProviderFormState>,

    pub preset_key_popup: Option<PresetKeyPopupState>,
    pub disabled_providers: std::collections::HashSet<String>,
    pub disabled_models: std::collections::HashSet<String>,
}

pub struct SettingsPopupInit {
    pub default_provider: String,
    pub default_model: String,
    pub small_model: String,
    pub use_env_keys: bool,
    pub saved_keys: Vec<(String, String)>,
    pub theme: String,
    pub user_alignment: TextAlignment,
    pub ai_alignment: TextAlignment,
    pub markdown_mode: MarkdownMode,
    pub artifact_save_dir: String,
    pub available_models: Vec<ModelInfo>,
    pub db_providers: Vec<ProviderEntry>,
    pub show_selector: bool,
    pub show_chat_scrollbar: bool,
    pub collapse_thinking: bool,
    pub kitty_enhanced_text: bool,
    pub kitty_text_max_scale: u8,
    pub web_search_enabled: bool,
    pub quit_confirmation: bool,
    pub local_enabled: bool,
    pub local_host: String,
    pub local_port: String,
    pub local_server_type: LocalServerType,
    pub local_selected_model: String,
    pub local_model_directory: String,
    pub local_health_interval_seconds: String,
    pub local_connect_timeout_ms: String,
    pub local_request_timeout_ms: String,
    pub local_api_token_env: String,
    pub detected_local_server: Option<LocalServerType>,
    pub providers_tab_list: Vec<EditableProvider>,
    pub models_provider: String,
    pub models_available_models: Vec<ModelInfo>,
    pub mcp_servers: Vec<McpServerConfig>,
}

impl ProviderFormState {
    fn new_add() -> Self {
        Self {
            title: " Add Provider ".to_string(),
            submit_label: "Add".to_string(),
            original_name: None,
            name: String::new(),
            endpoint: String::new(),
            backend_type: "openai".to_string(),
            api_key: String::new(),
            focus: ProviderFormFocus::ProviderName,
            dropdown_open: false,
            hit_areas: ProviderFormHitAreas::default(),
        }
    }

    fn new_edit(provider: &EditableProvider, api_key: String) -> Self {
        Self {
            title: " Edit Provider ".to_string(),
            submit_label: "Save".to_string(),
            original_name: Some(provider.name.clone()),
            name: provider.name.clone(),
            endpoint: provider.endpoint.clone(),
            backend_type: provider.backend_type.clone(),
            api_key,
            focus: ProviderFormFocus::ProviderName,
            dropdown_open: false,
            hit_areas: ProviderFormHitAreas::default(),
        }
    }

    fn can_submit(&self) -> bool {
        !self.name.trim().is_empty()
            && !self.endpoint.trim().is_empty()
            && !self.backend_type.trim().is_empty()
            && !self.api_key.trim().is_empty()
    }
}

impl EditProvidersPopupState {
    fn new(has_providers: bool) -> Self {
        Self {
            focus: if has_providers {
                Some(EditProvidersFocus::ProviderName(0))
            } else {
                None
            },
            hit_areas: EditProvidersHitAreas::default(),
        }
    }
}

impl PresetKeyPopupState {
    fn new(provider_name: String, endpoint: String, api_key: String) -> Self {
        Self {
            provider_name,
            endpoint,
            api_key,
        }
    }

    fn can_submit(&self) -> bool {
        !self.api_key.trim().is_empty()
    }
}

impl SettingsPopup {
    pub fn new(init: SettingsPopupInit) -> Self {
        Self {
            default_provider: init.default_provider,
            default_model: init.default_model,
            small_model: init.small_model,
            active_tab: SettingsTab::General,
            general_focus: GeneralFocus::Theme,
            use_env_keys: init.use_env_keys,
            saved_keys: init.saved_keys,
            theme: init.theme,
            user_alignment: init.user_alignment,
            ai_alignment: init.ai_alignment,
            markdown_mode: init.markdown_mode,
            artifact_save_dir: init.artifact_save_dir,
            general_hit_areas: GeneralHitAreas::default(),
            general_dropdown_open: None,
            available_models: init.available_models,
            db_providers: init.db_providers,
            show_selector: init.show_selector,
            show_chat_scrollbar: init.show_chat_scrollbar,
            collapse_thinking: init.collapse_thinking,
            kitty_enhanced_text: init.kitty_enhanced_text,
            kitty_text_max_scale: init.kitty_text_max_scale,
            web_search_enabled: init.web_search_enabled,
            quit_confirmation: init.quit_confirmation,
            local_enabled: init.local_enabled,
            local_host: init.local_host,
            local_port: init.local_port,
            local_server_type: init.local_server_type,
            local_selected_model: init.local_selected_model,
            local_model_directory: init.local_model_directory,
            local_health_interval_seconds: init.local_health_interval_seconds,
            local_connect_timeout_ms: init.local_connect_timeout_ms,
            local_request_timeout_ms: init.local_request_timeout_ms,
            local_api_token_env: init.local_api_token_env,
            detected_local_server: init.detected_local_server,
            local_focus: LocalFocus::Enabled,
            local_hit_areas: LocalHitAreas::default(),
            mcp_servers: init.mcp_servers,
            mcp_focus: 0,
            mcp_hit_areas: McpHitAreas::default(),
            providers_tab_list: init.providers_tab_list,
            providers_tab_focus: ProvidersTabFocus::DefaultProvider,
            providers_dropdown_open: None,
            dropdown_scroll_offset: 0,
            providers_tab_hit_areas: ProvidersTabHitAreas::default(),
            models_provider: init.models_provider,
            models_available_models: init.models_available_models,
            models_tab_focus: ModelsTabFocus::Provider,
            models_dropdown_open: false,
            models_dropdown_scroll_offset: 0,
            models_tab_hit_areas: ModelsTabHitAreas::default(),
            add_provider_popup: None,
            edit_providers_popup: None,
            edit_provider_popup: None,

            preset_key_popup: None,
            disabled_providers: std::collections::HashSet::new(),
            disabled_models: std::collections::HashSet::new(),
        }
    }

    pub fn render(&mut self, f: &mut Frame) {
        let area = f.area();
        let popup_area = Self::centered_rect(65, 75, area);

        f.render_widget(Clear, area);
        f.render_widget(
            Block::default().style(Style::default().bg(Color::Black)),
            area,
        );

        let block = Block::default()
            .title(" Settings ")
            .title_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .style(Style::default().bg(Color::Black));

        let inner = block.inner(popup_area);
        f.render_widget(Clear, popup_area);
        f.render_widget(block, popup_area);

        let tab_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)])
            .split(inner);

        let tabs = [
            "General",
            "Keybindings",
            "Providers",
            "Models",
            "Local",
            "MCP",
        ];
        let tab_titles: Vec<Line> = tabs
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let style = if self.active_tab == Self::tab_from_index(i) {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Gray)
                };
                Line::styled((*t).to_string(), style)
            })
            .collect();

        let tabs_widget = Tabs::new(tab_titles)
            .select(self.tab_index())
            .divider(Span::styled("│", Style::default().fg(Color::DarkGray)))
            .padding("", "")
            .style(Style::default().bg(Color::Black));
        f.render_widget(tabs_widget, tab_layout[0]);

        match self.active_tab {
            SettingsTab::General => self.render_general(f, tab_layout[1]),
            SettingsTab::Keybindings => self.render_keybindings(f, tab_layout[1]),
            SettingsTab::Providers => self.render_providers(f, tab_layout[1]),
            SettingsTab::Models => self.render_models(f, tab_layout[1]),
            SettingsTab::Local => self.render_local(f, tab_layout[1]),
            SettingsTab::Mcp => self.render_mcp(f, tab_layout[1]),
        }

        if let Some(edit_popup) = &mut self.edit_providers_popup {
            Self::render_edit_providers_popup(f, popup_area, &self.providers_tab_list, edit_popup);
        }

        if let Some(form) = &mut self.add_provider_popup {
            Self::render_provider_form_popup(f, popup_area, form);
        }

        if let Some(form) = &mut self.edit_provider_popup {
            Self::render_provider_form_popup(f, popup_area, form);
        }
    }

    fn render_general(&mut self, f: &mut Frame, area: Rect) {
        self.general_hit_areas = GeneralHitAreas::default();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
            ])
            .margin(1)
            .split(area);

        let theme_focused =
            self.active_tab == SettingsTab::General && self.general_focus == GeneralFocus::Theme;
        let theme_widget = Paragraph::new(format!("{} ▼", crate::theme::theme_label(&self.theme)))
            .block(
                Block::default()
                    .title(" Theme ")
                    .borders(Borders::ALL)
                    .border_style(if theme_focused {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    }),
            )
            .style(if theme_focused {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::Gray)
            });
        self.general_hit_areas.theme = Some(chunks[0]);
        f.render_widget(theme_widget, chunks[0]);

        let user_align_focused = self.active_tab == SettingsTab::General
            && self.general_focus == GeneralFocus::UserAlignment;
        let user_align = Paragraph::new(format!("{} ▼", self.user_alignment))
            .block(
                Block::default()
                    .title(" User alignment ")
                    .borders(Borders::ALL)
                    .border_style(if user_align_focused {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    }),
            )
            .style(if user_align_focused {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::Gray)
            });
        self.general_hit_areas.user_alignment = Some(chunks[1]);
        f.render_widget(user_align, chunks[1]);

        let ai_align_focused = self.active_tab == SettingsTab::General
            && self.general_focus == GeneralFocus::AiAlignment;
        let ai_align = Paragraph::new(format!("{} ▼", self.ai_alignment))
            .block(
                Block::default()
                    .title(" AI alignment ")
                    .borders(Borders::ALL)
                    .border_style(if ai_align_focused {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    }),
            )
            .style(if ai_align_focused {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::Gray)
            });
        self.general_hit_areas.ai_alignment = Some(chunks[2]);
        f.render_widget(ai_align, chunks[2]);

        let artifact_save_focused = self.active_tab == SettingsTab::General
            && self.general_focus == GeneralFocus::ArtifactSaveDir;
        let artifact_save_widget = Paragraph::new(self.artifact_save_dir.as_str())
            .block(
                Block::default()
                    .title(" Artifact save dir ")
                    .borders(Borders::ALL)
                    .border_style(if artifact_save_focused {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    }),
            )
            .style(if artifact_save_focused {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::Gray)
            });
        self.general_hit_areas.artifact_save_dir = Some(chunks[3]);
        f.render_widget(artifact_save_widget, chunks[3]);

        let selector_focused = self.active_tab == SettingsTab::General
            && self.general_focus == GeneralFocus::ShowSelector;
        let selector_lines = vec![Line::from(vec![
            Span::raw(if self.show_selector { "[✓] " } else { "[ ] " }),
            Span::styled(
                "Show provider/model selector",
                if selector_focused {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                },
            ),
        ])];
        let selector_widget = Paragraph::new(selector_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(if selector_focused {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default().fg(Color::DarkGray)
                }),
        );
        self.general_hit_areas.show_selector = Some(chunks[4]);
        f.render_widget(selector_widget, chunks[4]);

        let collapse_focused = self.active_tab == SettingsTab::General
            && self.general_focus == GeneralFocus::ShowChatScrollbar;
        let scrollbar_lines = vec![Line::from(vec![
            Span::raw(if self.show_chat_scrollbar {
                "[✓] "
            } else {
                "[ ] "
            }),
            Span::styled(
                "Show chat scrollbar",
                if collapse_focused {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                },
            ),
        ])];
        let scrollbar_widget = Paragraph::new(scrollbar_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(if collapse_focused {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default().fg(Color::DarkGray)
                }),
        );
        self.general_hit_areas.show_chat_scrollbar = Some(chunks[5]);
        f.render_widget(scrollbar_widget, chunks[5]);

        let collapse_focused = self.active_tab == SettingsTab::General
            && self.general_focus == GeneralFocus::CollapseThinking;
        let collapse_lines = vec![Line::from(vec![
            Span::raw(if self.collapse_thinking {
                "[✓] "
            } else {
                "[ ] "
            }),
            Span::styled(
                "Fold thinking by default",
                if collapse_focused {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                },
            ),
        ])];
        let collapse_widget = Paragraph::new(collapse_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(if collapse_focused {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default().fg(Color::DarkGray)
                }),
        );
        self.general_hit_areas.collapse_thinking = Some(chunks[6]);
        f.render_widget(collapse_widget, chunks[6]);

        let kitty_toggle_focused = self.active_tab == SettingsTab::General
            && self.general_focus == GeneralFocus::KittyEnhancedText;
        let kitty_toggle = Paragraph::new(vec![Line::from(vec![
            Span::raw(if self.kitty_enhanced_text {
                "[✓] "
            } else {
                "[ ] "
            }),
            Span::styled(
                "Use kitty enhanced text",
                if kitty_toggle_focused {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                },
            ),
        ])])
        .block(Block::default().borders(Borders::ALL).border_style(
            if kitty_toggle_focused {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default().fg(Color::DarkGray)
            },
        ));
        self.general_hit_areas.kitty_enhanced_text = Some(chunks[7]);
        f.render_widget(kitty_toggle, chunks[7]);

        let kitty_scale_focused = self.active_tab == SettingsTab::General
            && self.general_focus == GeneralFocus::KittyTextScale;
        let kitty_scale = Paragraph::new(format!("{} ▼", self.kitty_text_max_scale))
            .block(
                Block::default()
                    .title(" Kitty max heading scale ")
                    .borders(Borders::ALL)
                    .border_style(if kitty_scale_focused {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    }),
            )
            .style(if kitty_scale_focused {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::Gray)
            });
        self.general_hit_areas.kitty_text_scale = Some(chunks[8]);
        f.render_widget(kitty_scale, chunks[8]);

        let web_toggle_focused = self.active_tab == SettingsTab::General
            && self.general_focus == GeneralFocus::WebSearchEnabled;
        let web_toggle = Paragraph::new(vec![Line::from(vec![
            Span::raw(if self.web_search_enabled {
                "[✓] "
            } else {
                "[ ] "
            }),
            Span::styled(
                "Enable web search",
                if web_toggle_focused {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                },
            ),
        ])])
        .block(Block::default().borders(Borders::ALL).border_style(
            if web_toggle_focused {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default().fg(Color::DarkGray)
            },
        ));
        self.general_hit_areas.web_search_enabled = Some(chunks[9]);
        f.render_widget(web_toggle, chunks[9]);

        let quit_focused = self.active_tab == SettingsTab::General
            && self.general_focus == GeneralFocus::QuitConfirmation;
        let quit_toggle =
            Paragraph::new(vec![Line::from(vec![
                Span::raw(if self.quit_confirmation {
                    "[✓] "
                } else {
                    "[ ] "
                }),
                Span::styled(
                    "Confirm before quit",
                    if quit_focused {
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    },
                ),
            ])])
            .block(Block::default().borders(Borders::ALL).border_style(
                if quit_focused {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default().fg(Color::DarkGray)
                },
            ));
        self.general_hit_areas.quit_confirmation = Some(chunks[10]);
        f.render_widget(quit_toggle, chunks[10]);

        if let Some(dropdown) = self.general_dropdown_open {
            match dropdown {
                GeneralDropdown::Theme => {
                    let items = crate::theme::theme_labels();
                    let list_items: Vec<ListItem> = items
                        .iter()
                        .map(|label| {
                            let style = if *label == crate::theme::theme_label(&self.theme) {
                                Style::default().fg(Color::Black).bg(Color::Cyan)
                            } else {
                                Style::default().fg(Color::White)
                            };
                            ListItem::new(*label).style(style)
                        })
                        .collect();
                    let dropdown_area =
                        Self::dropdown_area_below(chunks[0], items.len() as u16 + 2);
                    let list = List::new(list_items).block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(Color::Cyan))
                            .style(Style::default().bg(Color::Black)),
                    );
                    f.render_widget(Clear, dropdown_area);
                    f.render_widget(list, dropdown_area);
                    self.general_hit_areas.dropdown_items = (0..items.len())
                        .map(|i| Rect {
                            x: dropdown_area.x + 1,
                            y: dropdown_area.y + 1 + i as u16,
                            width: dropdown_area.width - 2,
                            height: 1,
                        })
                        .collect();
                }
                GeneralDropdown::UserAlignment => {
                    let items = ["left", "middle", "right"];
                    let list_items: Vec<ListItem> = items
                        .iter()
                        .map(|name| {
                            let style = if *name == self.user_alignment.to_string().as_str() {
                                Style::default().fg(Color::Black).bg(Color::Cyan)
                            } else {
                                Style::default().fg(Color::White)
                            };
                            ListItem::new(*name).style(style)
                        })
                        .collect();
                    let dropdown_area = Self::dropdown_area_below(chunks[1], 5);
                    let list = List::new(list_items).block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(Color::Cyan))
                            .style(Style::default().bg(Color::Black)),
                    );
                    f.render_widget(Clear, dropdown_area);
                    f.render_widget(list, dropdown_area);

                    let item_height = 1;
                    let mut item_areas = Vec::new();
                    for i in 0..items.len() {
                        item_areas.push(Rect {
                            x: dropdown_area.x + 1,
                            y: dropdown_area.y + 1 + (i as u16 * item_height),
                            width: dropdown_area.width - 2,
                            height: item_height,
                        });
                    }
                    self.general_hit_areas.dropdown_items = item_areas;
                }
                GeneralDropdown::AiAlignment => {
                    let items = ["left", "middle", "right"];
                    let list_items: Vec<ListItem> = items
                        .iter()
                        .map(|name| {
                            let style = if *name == self.ai_alignment.to_string().as_str() {
                                Style::default().fg(Color::Black).bg(Color::Cyan)
                            } else {
                                Style::default().fg(Color::White)
                            };
                            ListItem::new(*name).style(style)
                        })
                        .collect();
                    let dropdown_area = Self::dropdown_area_below(chunks[2], 5);
                    let list = List::new(list_items).block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(Color::Cyan))
                            .style(Style::default().bg(Color::Black)),
                    );
                    f.render_widget(Clear, dropdown_area);
                    f.render_widget(list, dropdown_area);

                    let item_height = 1;
                    let mut item_areas = Vec::new();
                    for i in 0..items.len() {
                        item_areas.push(Rect {
                            x: dropdown_area.x + 1,
                            y: dropdown_area.y + 1 + (i as u16 * item_height),
                            width: dropdown_area.width - 2,
                            height: item_height,
                        });
                    }
                    self.general_hit_areas.dropdown_items = item_areas;
                }
                GeneralDropdown::KittyTextScale => {
                    let dropdown_area = Self::dropdown_area_below(chunks[7], 9);
                    let list_items: Vec<ListItem> = KITTY_TEXT_SCALE_OPTIONS
                        .iter()
                        .map(|scale| {
                            let style = if *scale == self.kitty_text_max_scale {
                                Style::default().fg(Color::Black).bg(Color::Cyan)
                            } else {
                                Style::default().fg(Color::White)
                            };
                            ListItem::new(scale.to_string()).style(style)
                        })
                        .collect();
                    let list = List::new(list_items).block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(Color::Cyan))
                            .style(Style::default().bg(Color::Black)),
                    );
                    f.render_widget(Clear, dropdown_area);
                    f.render_widget(list, dropdown_area);

                    self.general_hit_areas.dropdown_items = (0..KITTY_TEXT_SCALE_OPTIONS.len())
                        .map(|i| Rect {
                            x: dropdown_area.x + 1,
                            y: dropdown_area.y + 1 + i as u16,
                            width: dropdown_area.width - 2,
                            height: 1,
                        })
                        .collect();
                }
            }
        }
    }

    fn render_local(&mut self, f: &mut Frame, area: Rect) {
        self.local_hit_areas = LocalHitAreas::default();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(2),
            ])
            .margin(1)
            .split(area);

        let enabled_focused =
            self.active_tab == SettingsTab::Local && self.local_focus == LocalFocus::Enabled;
        let enabled = Paragraph::new(vec![Line::from(vec![
            Span::raw(if self.local_enabled { "[✓] " } else { "[ ] " }),
            Span::styled(
                "Enable local inference",
                if enabled_focused {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                },
            ),
        ])])
        .block(Block::default().borders(Borders::ALL).border_style(
            if enabled_focused {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default().fg(Color::DarkGray)
            },
        ));
        self.local_hit_areas.enabled = Some(chunks[0]);
        f.render_widget(enabled, chunks[0]);

        Self::render_local_text_field(
            f,
            chunks[1],
            " Host ",
            &self.local_host,
            self.local_focus == LocalFocus::Host,
            &mut self.local_hit_areas.host,
        );
        Self::render_local_text_field(
            f,
            chunks[2],
            " Port ",
            &self.local_port,
            self.local_focus == LocalFocus::Port,
            &mut self.local_hit_areas.port,
        );
        Self::render_local_text_field(
            f,
            chunks[3],
            " Server Type ",
            self.local_server_type.label(),
            self.local_focus == LocalFocus::ServerType,
            &mut self.local_hit_areas.server_type,
        );
        Self::render_local_text_field(
            f,
            chunks[4],
            " Selected Model ",
            &self.local_selected_model,
            self.local_focus == LocalFocus::SelectedModel,
            &mut self.local_hit_areas.selected_model,
        );
        Self::render_local_text_field(
            f,
            chunks[5],
            " Model Directory ",
            &self.local_model_directory,
            self.local_focus == LocalFocus::ModelDirectory,
            &mut self.local_hit_areas.model_directory,
        );
        Self::render_local_text_field(
            f,
            chunks[6],
            " Health Interval (s) ",
            &self.local_health_interval_seconds,
            self.local_focus == LocalFocus::HealthInterval,
            &mut self.local_hit_areas.health_interval,
        );
        Self::render_local_text_field(
            f,
            chunks[7],
            " Connect Timeout (ms) ",
            &self.local_connect_timeout_ms,
            self.local_focus == LocalFocus::ConnectTimeout,
            &mut self.local_hit_areas.connect_timeout,
        );
        Self::render_local_text_field(
            f,
            chunks[8],
            " Request Timeout (ms) ",
            &self.local_request_timeout_ms,
            self.local_focus == LocalFocus::RequestTimeout,
            &mut self.local_hit_areas.request_timeout,
        );
        Self::render_local_text_field(
            f,
            chunks[9],
            " API Token Env ",
            &self.local_api_token_env,
            self.local_focus == LocalFocus::ApiTokenEnv,
            &mut self.local_hit_areas.api_token_env,
        );

        let detected_label = self
            .detected_local_server
            .map(LocalServerType::label)
            .unwrap_or("Not checked");
        f.render_widget(
            Paragraph::new(format!(
                "Detected: {detected_label}    server command management is disabled"
            ))
            .style(Style::default().fg(Color::DarkGray)),
            chunks[10],
        );
    }

    fn render_local_text_field(
        f: &mut Frame,
        area: Rect,
        title: &str,
        value: &str,
        focused: bool,
        hit_area: &mut Option<Rect>,
    ) {
        *hit_area = Some(area);
        f.render_widget(
            Paragraph::new(if value.trim().is_empty() { " " } else { value })
                .block(
                    Block::default()
                        .title(title)
                        .borders(Borders::ALL)
                        .border_style(if focused {
                            Style::default().fg(Color::Cyan)
                        } else {
                            Style::default().fg(Color::DarkGray)
                        }),
                )
                .style(if focused {
                    Style::default().fg(Color::White)
                } else {
                    Style::default().fg(Color::Gray)
                }),
            area,
        );
    }

    fn dropdown_area_below(anchor: Rect, height: u16) -> Rect {
        Rect::new(anchor.x, anchor.y + anchor.height, anchor.width, height)
    }

    fn render_mcp(&mut self, f: &mut Frame, area: Rect) {
        self.mcp_hit_areas = McpHitAreas::default();
        let visible = area.height.saturating_sub(2) as usize;
        if visible == 0 {
            return;
        }
        self.mcp_focus = self.mcp_focus.min(self.mcp_servers.len().saturating_sub(1));
        let start = self
            .mcp_focus
            .saturating_sub(visible.saturating_sub(1))
            .min(self.mcp_servers.len().saturating_sub(visible));
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints(
                self.mcp_servers
                    .iter()
                    .skip(start)
                    .take(visible)
                    .map(|_| Constraint::Length(1))
                    .collect::<Vec<_>>(),
            )
            .margin(1)
            .split(area);

        for ((idx, server), row) in self
            .mcp_servers
            .iter()
            .enumerate()
            .skip(start)
            .zip(rows.iter())
        {
            let focused = idx == self.mcp_focus;
            let line = Line::from(vec![
                Span::raw(if server.enabled { "[x] " } else { "[ ] " }),
                Span::styled(
                    server.name.clone(),
                    if focused {
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    },
                ),
            ]);
            f.render_widget(
                Paragraph::new(line).style(if focused {
                    Style::default().bg(Color::DarkGray)
                } else {
                    Style::default()
                }),
                *row,
            );
            self.mcp_hit_areas.rows.push((idx, *row));
        }
    }

    fn render_providers(&mut self, f: &mut Frame, area: Rect) {
        self.providers_tab_hit_areas = ProvidersTabHitAreas::default();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(3),
                Constraint::Length(3),
            ])
            .margin(1)
            .split(area);

        let provider_focused = self.providers_tab_focus == ProvidersTabFocus::DefaultProvider;
        let provider_text = format!(
            "{} ▼",
            if self.default_provider.is_empty() {
                "Select provider".to_string()
            } else {
                self.default_provider.clone()
            }
        );
        let provider_widget = Paragraph::new(provider_text)
            .block(
                Block::default()
                    .title(" Default Provider ")
                    .borders(Borders::ALL)
                    .border_style(if provider_focused {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    }),
            )
            .style(if provider_focused {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::Gray)
            });
        self.providers_tab_hit_areas.default_provider = Some(chunks[0]);
        f.render_widget(provider_widget, chunks[0]);

        let model_focused = self.providers_tab_focus == ProvidersTabFocus::DefaultModel;
        let model_text = format!(
            "{} ▼",
            if self.default_model.is_empty() {
                "Select model".to_string()
            } else {
                self.default_model.clone()
            }
        );
        let model_widget = Paragraph::new(model_text)
            .block(
                Block::default()
                    .title(" Default Model ")
                    .borders(Borders::ALL)
                    .border_style(if model_focused {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    }),
            )
            .style(if model_focused {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::Gray)
            });
        self.providers_tab_hit_areas.default_model = Some(chunks[1]);
        f.render_widget(model_widget, chunks[1]);

        // Button Row: [Grab Keys] [Add provider] [Edit] [Reload]
        let button_row = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(16),
                Constraint::Length(18),
                Constraint::Length(20),
                Constraint::Length(20),
                Constraint::Min(0),
            ])
            .split(chunks[2]);

        let env_focused = self.providers_tab_focus == ProvidersTabFocus::UseEnvToggle;
        let env_button = Paragraph::new(" Grab keys ")
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(if env_focused {
                        Style::default().fg(Color::Magenta)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    })
                    .style(if env_focused {
                        Style::default().bg(Color::Magenta)
                    } else {
                        Style::default()
                    }),
            )
            .style(if env_focused {
                Style::default()
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Magenta)
            })
            .alignment(Alignment::Center);
        self.providers_tab_hit_areas.grab_env_button = Some(button_row[0]);
        f.render_widget(env_button, button_row[0]);

        let add_focused = self.providers_tab_focus == ProvidersTabFocus::AddProviderButton;
        let add_button = Paragraph::new(" Add provider ")
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(if add_focused {
                        Style::default().fg(Color::Green)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    })
                    .style(if add_focused {
                        Style::default().bg(Color::Green)
                    } else {
                        Style::default()
                    }),
            )
            .style(if add_focused {
                Style::default()
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Green)
            })
            .alignment(Alignment::Center);
        self.providers_tab_hit_areas.add_button = Some(button_row[1]);
        f.render_widget(add_button, button_row[1]);

        let edit_focused = self.providers_tab_focus == ProvidersTabFocus::EditProvidersButton;
        let edit_button = Paragraph::new(" Edit providers ")
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(if edit_focused {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    })
                    .style(if edit_focused {
                        Style::default().bg(Color::Cyan)
                    } else {
                        Style::default()
                    }),
            )
            .style(if edit_focused {
                Style::default()
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Cyan)
            })
            .alignment(Alignment::Center);
        self.providers_tab_hit_areas.edit_button = Some(button_row[2]);
        f.render_widget(edit_button, button_row[2]);

        let reload_focused = self.providers_tab_focus == ProvidersTabFocus::ReloadModelsButton;
        let reload_button = Paragraph::new(" Reload Models ")
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(if reload_focused {
                        Style::default().fg(Color::Yellow)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    })
                    .style(if reload_focused {
                        Style::default().bg(Color::Yellow)
                    } else {
                        Style::default()
                    }),
            )
            .style(if reload_focused {
                Style::default()
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Yellow)
            })
            .alignment(Alignment::Center);
        self.providers_tab_hit_areas.reload_models_button = Some(button_row[3]);
        f.render_widget(reload_button, button_row[3]);

        if let Some(dropdown) = self.providers_dropdown_open {
            if dropdown == ProvidersDropdown::DefaultModel
                || dropdown == ProvidersDropdown::SmallModel
            {
                self.render_model_list_inline(f, chunks[3], dropdown);
                let explanation = Paragraph::new(vec![Line::from(Span::styled(
                    "Use Up/Down to scroll, Enter to select, Esc or click outside to close.",
                    Style::default().fg(Color::DarkGray),
                ))])
                .alignment(Alignment::Center);
                f.render_widget(explanation, chunks[4]);
                return;
            }
        }

        let saved_block = Block::default()
            .title(" Saved Providers ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray));
        let saved_inner = saved_block.inner(chunks[3]);
        f.render_widget(saved_block, chunks[3]);

        let mut row_idx = 0;

        let oauth_providers = self.oauth_providers();
        for (name, _, _, _, _) in &oauth_providers {
            if row_idx >= saved_inner.height as usize {
                break;
            }
            let row_area = Rect::new(
                saved_inner.x,
                saved_inner.y + row_idx as u16,
                saved_inner.width,
                1,
            );
            let enabled = !self.disabled_providers.contains(name);
            let toggle = if enabled { "[✓]" } else { "[ ]" };
            let connected = self.has_saved_key(name);
            let color = if enabled && connected {
                Color::Green
            } else if !enabled {
                Color::DarkGray
            } else {
                Color::Yellow
            };
            let is_focused = self.providers_tab_focus == ProvidersTabFocus::OAuthProvider(row_idx);
            self.providers_tab_hit_areas.oauth_rows.push(row_area);
            f.render_widget(
                Paragraph::new(format!("{} {}", toggle, name)).style(if is_focused {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(color)
                }),
                row_area,
            );
            row_idx += 1;
        }

        let preset_providers = self.preset_api_key_providers();
        for (idx, (name, _, _, _, _)) in preset_providers.iter().enumerate() {
            if row_idx >= saved_inner.height as usize {
                break;
            }
            let row_area = Rect::new(
                saved_inner.x,
                saved_inner.y + row_idx as u16,
                saved_inner.width,
                1,
            );
            let enabled = !self.disabled_providers.contains(name);
            let toggle = if enabled { "[✓]" } else { "[ ]" };
            let connected = self.has_saved_key(name);
            let color = if enabled && connected {
                Color::Green
            } else if !enabled {
                Color::DarkGray
            } else {
                Color::Yellow
            };
            let is_focused = self.providers_tab_focus == ProvidersTabFocus::PresetProvider(idx);
            self.providers_tab_hit_areas.preset_rows.push(row_area);
            f.render_widget(
                Paragraph::new(format!("{} {}", toggle, name)).style(if is_focused {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(color)
                }),
                row_area,
            );
            row_idx += 1;
        }

        let custom_providers: Vec<_> = self
            .db_providers
            .iter()
            .filter(|(n, _, _, _, auth_type)| {
                auth_type != "oauth" && preset_providers.iter().all(|(pn, _, _, _, _)| pn != n)
            })
            .collect();
        for (idx, (name, _, _, _, _)) in custom_providers.iter().enumerate() {
            if row_idx >= saved_inner.height as usize {
                break;
            }
            let row_area = Rect::new(
                saved_inner.x,
                saved_inner.y + row_idx as u16,
                saved_inner.width,
                1,
            );
            let enabled = !self.disabled_providers.contains(name);
            let toggle = if enabled { "[✓]" } else { "[ ]" };
            let connected = self.has_saved_key(name);
            let color = if enabled && connected {
                Color::Green
            } else if !enabled {
                Color::DarkGray
            } else {
                Color::Yellow
            };
            let is_focused = self.providers_tab_focus == ProvidersTabFocus::SavedKeyList(idx);
            self.providers_tab_hit_areas.saved_key_rows.push(row_area);
            f.render_widget(
                Paragraph::new(format!("{} {}", toggle, name)).style(if is_focused {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(color)
                }),
                row_area,
            );
            row_idx += 1;
        }

        let explanation = Paragraph::new(vec![
            Line::from(Span::styled(
                "Keys are read from: environment variables, .env, ~/.env, or \"api_key_<provider>\" settings.",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(Span::styled(
                "Format: PROVIDER_API_KEY  (e.g. OPENAI_API_KEY, ANTHROPIC_API_KEY)",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(Span::styled(
                "OAuth providers (Gemini, Codex): login via CLI first, tokens read from ~/.gemini.json, ~/.codex.json",
                Style::default().fg(Color::DarkGray),
            )),
        ]).alignment(Alignment::Center);
        f.render_widget(explanation, chunks[4]);

        self.render_providers_dropdowns(f, chunks);
    }

    fn render_models(&mut self, f: &mut Frame, area: Rect) {
        self.models_tab_hit_areas = ModelsTabHitAreas::default();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(2),
            ])
            .margin(1)
            .split(area);

        let provider_focused = self.models_tab_focus == ModelsTabFocus::Provider;
        let provider_widget = Paragraph::new(format!("{} ▼", self.models_provider))
            .block(
                Block::default()
                    .title(" Provider ")
                    .borders(Borders::ALL)
                    .border_style(if provider_focused {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    }),
            )
            .style(if provider_focused {
                Style::default().fg(Color::Black).bg(Color::Cyan)
            } else {
                Style::default().fg(Color::White)
            });
        self.models_tab_hit_areas.provider = Some(chunks[0]);
        f.render_widget(provider_widget, chunks[0]);

        let list_block = Block::default()
            .title(" Models For Selected Provider ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray));
        let list_inner = list_block.inner(chunks[1]);
        f.render_widget(list_block, chunks[1]);

        for (idx, model) in self.models_available_models.iter().enumerate() {
            if idx >= list_inner.height as usize {
                break;
            }
            let row_area = Rect::new(list_inner.x, list_inner.y + idx as u16, list_inner.width, 1);
            let enabled = !self
                .disabled_models
                .contains(&Self::disabled_model_key(&self.models_provider, &model.id));
            let toggle = if enabled { "[✓]" } else { "[ ]" };
            let focused = self.models_tab_focus == ModelsTabFocus::Model(idx);
            self.models_tab_hit_areas.model_rows.push(row_area);
            f.render_widget(
                Paragraph::new(format!("{} {}", toggle, model.id)).style(if focused {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else if enabled {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::DarkGray)
                }),
                row_area,
            );
        }

        let help =
            Paragraph::new("Toggle providers/models here to hide them from the chat selectors.")
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::DarkGray));
        f.render_widget(help, chunks[2]);

        self.render_models_dropdown(f, chunks[0]);
    }

    fn render_models_dropdown(&mut self, f: &mut Frame, anchor: Rect) {
        if !self.models_dropdown_open {
            return;
        }
        const VISIBLE_ITEMS: usize = 8;
        const SCROLLBAR_WIDTH: u16 = 1;
        let provider_names = self.all_enabled_provider_names();
        let total = provider_names.len();
        let max_visible = VISIBLE_ITEMS.min(total);
        let offset = self
            .models_dropdown_scroll_offset
            .min(total.saturating_sub(max_visible));
        self.models_dropdown_scroll_offset = offset;
        let visible_names: Vec<_> = provider_names
            .iter()
            .skip(offset)
            .take(max_visible)
            .collect();
        let items: Vec<ListItem> = visible_names
            .iter()
            .map(|name| {
                let style = if *name == &self.models_provider {
                    Style::default().fg(Color::Black).bg(Color::Cyan)
                } else {
                    Style::default().fg(Color::White)
                };
                ListItem::new(name.as_str()).style(style)
            })
            .collect();
        let content_height = max_visible as u16;
        let dropdown_area = Self::dropdown_area_below(anchor, content_height + 2);
        let content_width = dropdown_area.width - 2 - SCROLLBAR_WIDTH;
        let viewport = Rect::new(
            dropdown_area.x + 1,
            dropdown_area.y + 1,
            content_width,
            content_height,
        );
        let list = List::new(items).style(Style::default().bg(Color::Black));
        f.render_widget(Clear, dropdown_area);
        f.render_widget(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .style(Style::default().bg(Color::Black)),
            dropdown_area,
        );
        f.render_widget(list, viewport);
        self.models_tab_hit_areas.provider_items.clear();
        for i in 0..max_visible {
            self.models_tab_hit_areas.provider_items.push(Rect {
                x: viewport.x,
                y: viewport.y + i as u16,
                width: viewport.width,
                height: 1,
            });
        }
    }

    fn render_model_list_inline(&mut self, f: &mut Frame, area: Rect, dropdown: ProvidersDropdown) {
        const VISIBLE: usize = 6;
        const SB_W: u16 = 1;
        let total = self.available_models.len();
        let max_visible = VISIBLE.min(total);
        let offset = self
            .dropdown_scroll_offset
            .min(total.saturating_sub(max_visible));
        self.dropdown_scroll_offset = offset;

        let visible_models: Vec<_> = self
            .available_models
            .iter()
            .skip(offset)
            .take(max_visible)
            .collect();

        let block = Block::default()
            .title(format!(
                " {} ",
                if dropdown == ProvidersDropdown::DefaultModel {
                    "Default Model"
                } else {
                    "Small Model"
                }
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .style(Style::default().bg(Color::Black));
        let inner = block.inner(area);
        f.render_widget(Clear, area);
        f.render_widget(block, area);

        let list_width = inner.width - SB_W;
        for (i, m) in visible_models.iter().enumerate() {
            let row = Rect::new(inner.x, inner.y + i as u16, list_width, 1);
            let is_selected = if dropdown == ProvidersDropdown::DefaultModel {
                m.id == self.default_model
            } else {
                m.id == self.small_model_name()
            };
            let price = match (m.input_price, m.output_price) {
                (Some(inp), Some(out)) => format!("${:.2}/${:.2}", inp, out),
                (Some(inp), None) => format!("${:.2}/-", inp),
                (None, Some(out)) => format!("-/${:.2}", out),
                _ => String::new(),
            };
            let label = if price.is_empty() {
                m.id.clone()
            } else {
                format!("{:30} {}", m.id, price)
            };
            f.render_widget(
                Paragraph::new(label).style(if is_selected {
                    Style::default().fg(Color::Black).bg(Color::Cyan)
                } else {
                    Style::default().fg(Color::White)
                }),
                row,
            );
            let target = if dropdown == ProvidersDropdown::DefaultModel {
                &mut self.providers_tab_hit_areas.default_model_items
            } else {
                &mut self.providers_tab_hit_areas.small_model_items
            };
            if i >= target.len() {
                target.push(row);
            } else {
                target[i] = row;
            }
        }

        if total > max_visible {
            let sb_x = inner.x + list_width;
            let sb_area = Rect::new(sb_x, inner.y, SB_W, inner.height);
            let thumb_h =
                ((max_visible as f64 / total as f64) * inner.height as f64).max(1.0) as u16;
            let thumb_y = ((offset as f64 / total as f64) * inner.height as f64) as u16;
            f.render_widget(
                Paragraph::new("").style(Style::default().bg(Color::DarkGray)),
                sb_area,
            );
            f.render_widget(
                Paragraph::new("").style(Style::default().bg(Color::White)),
                Rect::new(
                    sb_x,
                    sb_area.y + thumb_y.min(inner.height.saturating_sub(1)),
                    SB_W,
                    thumb_h,
                ),
            );
        }
    }

    fn render_providers_dropdowns(&mut self, f: &mut Frame, chunks: std::rc::Rc<[Rect]>) {
        let Some(dropdown) = self.providers_dropdown_open else {
            return;
        };
        if dropdown == ProvidersDropdown::DefaultModel || dropdown == ProvidersDropdown::SmallModel
        {
            return;
        }

        const VISIBLE_ITEMS: usize = 8;
        const SCROLLBAR_WIDTH: u16 = 1;

        match dropdown {
            ProvidersDropdown::DefaultProvider | ProvidersDropdown::SmallProvider => {
                let current_provider = if dropdown == ProvidersDropdown::DefaultProvider {
                    &self.default_provider
                } else {
                    &self.small_model_provider()
                };
                let provider_names: Vec<String> = self.all_enabled_provider_names();
                let total = provider_names.len();
                let max_visible = VISIBLE_ITEMS.min(total);

                let offset = self
                    .dropdown_scroll_offset
                    .min(total.saturating_sub(max_visible));
                self.dropdown_scroll_offset = offset;

                let visible_names: Vec<_> = provider_names
                    .iter()
                    .skip(offset)
                    .take(max_visible)
                    .collect();

                let items: Vec<ListItem> = visible_names
                    .iter()
                    .map(|name| {
                        let style = if *name == current_provider {
                            Style::default().fg(Color::Black).bg(Color::Cyan)
                        } else {
                            Style::default().fg(Color::White)
                        };
                        ListItem::new(name.as_str()).style(style)
                    })
                    .collect();

                let anchor = if dropdown == ProvidersDropdown::DefaultProvider {
                    chunks[0]
                } else {
                    chunks[2]
                };
                let content_height = max_visible as u16;
                let dropdown_area = Self::dropdown_area_below(anchor, content_height + 2);

                let content_width = dropdown_area.width - 2 - SCROLLBAR_WIDTH;
                let viewport = Rect::new(
                    dropdown_area.x + 1,
                    dropdown_area.y + 1,
                    content_width,
                    content_height,
                );

                let list = List::new(items).style(Style::default().bg(Color::Black));
                f.render_widget(Clear, dropdown_area);
                f.render_widget(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Cyan))
                        .style(Style::default().bg(Color::Black)),
                    dropdown_area,
                );
                f.render_widget(list, viewport);

                if total > max_visible {
                    let sb_x = dropdown_area.x + dropdown_area.width - 2;
                    let sb_area =
                        Rect::new(sb_x, dropdown_area.y + 1, SCROLLBAR_WIDTH, content_height);
                    let thumb_h = ((max_visible as f64 / total as f64) * content_height as f64)
                        .max(1.0) as u16;
                    let thumb_y = ((offset as f64 / total as f64) * content_height as f64) as u16;
                    let thumb = Rect::new(
                        sb_x,
                        sb_area.y + thumb_y.min(content_height.saturating_sub(1)),
                        SCROLLBAR_WIDTH,
                        thumb_h,
                    );
                    f.render_widget(
                        Paragraph::new("").style(Style::default().bg(Color::DarkGray)),
                        sb_area,
                    );
                    f.render_widget(
                        Paragraph::new("").style(Style::default().bg(Color::White)),
                        thumb,
                    );
                }

                self.providers_tab_hit_areas.default_provider_items.clear();
                for i in 0..max_visible {
                    self.providers_tab_hit_areas
                        .default_provider_items
                        .push(Rect {
                            x: viewport.x,
                            y: viewport.y + i as u16,
                            width: viewport.width,
                            height: 1,
                        });
                }
            }
            ProvidersDropdown::DefaultModel | ProvidersDropdown::SmallModel => {
                let total = self.available_models.len();
                let max_visible = VISIBLE_ITEMS.min(total);

                let offset = self
                    .dropdown_scroll_offset
                    .min(total.saturating_sub(max_visible));
                self.dropdown_scroll_offset = offset;

                let visible_models: Vec<_> = self
                    .available_models
                    .iter()
                    .skip(offset)
                    .take(max_visible)
                    .collect();

                let items: Vec<ListItem> = visible_models
                    .iter()
                    .map(|m| {
                        let is_selected = if dropdown == ProvidersDropdown::DefaultModel {
                            m.id == self.default_model
                        } else {
                            m.id == self.small_model_name()
                        };
                        let price_text = match (m.input_price, m.output_price) {
                            (Some(inp), Some(out)) => format!("${:.2}/${:.2}", inp, out),
                            (Some(inp), None) => format!("${:.2}/-", inp),
                            (None, Some(out)) => format!("-/${:.2}", out),
                            (None, None) => String::new(),
                        };
                        let label = if price_text.is_empty() {
                            m.id.clone()
                        } else {
                            format!("{:30} {}", m.id, price_text)
                        };
                        let style = if is_selected {
                            Style::default().fg(Color::Black).bg(Color::Cyan)
                        } else {
                            Style::default().fg(Color::White)
                        };
                        ListItem::new(label).style(style)
                    })
                    .collect();

                let anchor = if dropdown == ProvidersDropdown::DefaultModel {
                    chunks[1]
                } else {
                    chunks[3]
                };
                let content_height = max_visible as u16;
                let dropdown_area = Self::dropdown_area_below(anchor, content_height + 2);

                let content_width = dropdown_area.width - 2 - SCROLLBAR_WIDTH;
                let viewport = Rect::new(
                    dropdown_area.x + 1,
                    dropdown_area.y + 1,
                    content_width,
                    content_height,
                );

                let list = List::new(items).style(Style::default().bg(Color::Black));
                f.render_widget(Clear, dropdown_area);
                f.render_widget(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Cyan))
                        .style(Style::default().bg(Color::Black)),
                    dropdown_area,
                );
                f.render_widget(list, viewport);

                if total > max_visible {
                    let sb_x = dropdown_area.x + dropdown_area.width - 2;
                    let sb_area =
                        Rect::new(sb_x, dropdown_area.y + 1, SCROLLBAR_WIDTH, content_height);
                    let thumb_h = ((max_visible as f64 / total as f64) * content_height as f64)
                        .max(1.0) as u16;
                    let thumb_y = ((offset as f64 / total as f64) * content_height as f64) as u16;
                    let thumb = Rect::new(
                        sb_x,
                        sb_area.y + thumb_y.min(content_height.saturating_sub(1)),
                        SCROLLBAR_WIDTH,
                        thumb_h,
                    );
                    f.render_widget(
                        Paragraph::new("").style(Style::default().bg(Color::DarkGray)),
                        sb_area,
                    );
                    f.render_widget(
                        Paragraph::new("").style(Style::default().bg(Color::White)),
                        thumb,
                    );
                }

                let target_vec = if dropdown == ProvidersDropdown::DefaultModel {
                    &mut self.providers_tab_hit_areas.default_model_items
                } else {
                    &mut self.providers_tab_hit_areas.small_model_items
                };
                target_vec.clear();
                for i in 0..max_visible {
                    target_vec.push(Rect {
                        x: viewport.x,
                        y: viewport.y + i as u16,
                        width: viewport.width,
                        height: 1,
                    });
                }
            }
        }
    }

    fn render_provider_form_popup(f: &mut Frame, parent_area: Rect, form: &mut ProviderFormState) {
        form.hit_areas = ProviderFormHitAreas::default();
        let popup_area = Self::centered_rect_in(52, 60, parent_area);
        let block = Block::default()
            .title(form.title.as_str())
            .title_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .style(Style::default().bg(Color::Black));
        let inner = block.inner(popup_area);

        f.render_widget(Clear, popup_area);
        f.render_widget(block, popup_area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
            ])
            .margin(1)
            .split(inner);

        let name_focused = form.focus == ProviderFormFocus::ProviderName;
        form.hit_areas.name = Some(chunks[0]);
        f.render_widget(
            Paragraph::new(form.name.clone())
                .block(
                    Block::default()
                        .title(" Provider Name ")
                        .borders(Borders::ALL)
                        .border_style(if name_focused {
                            Style::default().fg(Color::Cyan)
                        } else {
                            Style::default().fg(Color::DarkGray)
                        }),
                )
                .style(if name_focused {
                    Style::default().fg(Color::White)
                } else {
                    Style::default().fg(Color::Gray)
                }),
            chunks[0],
        );

        let endpoint_focused = form.focus == ProviderFormFocus::ProviderEndpoint;
        form.hit_areas.endpoint = Some(chunks[1]);
        f.render_widget(
            Paragraph::new(form.endpoint.clone())
                .block(
                    Block::default()
                        .title(" Endpoint Base URL ")
                        .borders(Borders::ALL)
                        .border_style(if endpoint_focused {
                            Style::default().fg(Color::Cyan)
                        } else {
                            Style::default().fg(Color::DarkGray)
                        }),
                )
                .style(if endpoint_focused {
                    Style::default().fg(Color::White)
                } else {
                    Style::default().fg(Color::Gray)
                }),
            chunks[1],
        );

        let backend_focused = form.focus == ProviderFormFocus::ProviderBackendType;
        form.hit_areas.backend_type = Some(chunks[2]);
        f.render_widget(
            Paragraph::new(format!("{} ▼", Self::backend_label(&form.backend_type)))
                .block(
                    Block::default()
                        .title(" SDK Backend Type ")
                        .borders(Borders::ALL)
                        .border_style(if backend_focused {
                            Style::default().fg(Color::Cyan)
                        } else {
                            Style::default().fg(Color::DarkGray)
                        }),
                )
                .style(if backend_focused {
                    Style::default().fg(Color::White)
                } else {
                    Style::default().fg(Color::Gray)
                }),
            chunks[2],
        );

        let api_key_focused = form.focus == ProviderFormFocus::ProviderApiKey;
        let api_key_display = if api_key_focused {
            form.api_key.clone()
        } else {
            mask_key(&form.api_key)
        };
        form.hit_areas.api_key = Some(chunks[3]);
        f.render_widget(
            Paragraph::new(api_key_display)
                .block(
                    Block::default()
                        .title(" API Key ")
                        .borders(Borders::ALL)
                        .border_style(if api_key_focused {
                            Style::default().fg(Color::Cyan)
                        } else {
                            Style::default().fg(Color::DarkGray)
                        }),
                )
                .style(if api_key_focused {
                    Style::default().fg(Color::White)
                } else {
                    Style::default().fg(Color::Gray)
                }),
            chunks[3],
        );

        let buttons = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(12),
                Constraint::Length(12),
                Constraint::Min(0),
            ])
            .split(chunks[4]);

        let can_submit = form.can_submit();
        let submit_focused = form.focus == ProviderFormFocus::SubmitButton;
        form.hit_areas.submit_button = Some(buttons[0]);
        f.render_widget(
            Paragraph::new(format!(" {} ", form.submit_label))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(if submit_focused {
                            Style::default().fg(Color::Green)
                        } else {
                            Style::default().fg(Color::DarkGray)
                        }),
                )
                .style(if can_submit && submit_focused {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Green)
                        .add_modifier(Modifier::BOLD)
                } else if can_submit {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::DarkGray)
                })
                .alignment(Alignment::Center),
            buttons[0],
        );

        let cancel_focused = form.focus == ProviderFormFocus::CancelButton;
        form.hit_areas.cancel_button = Some(buttons[1]);
        f.render_widget(
            Paragraph::new(" Cancel ")
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(if cancel_focused {
                            Style::default().fg(Color::Red)
                        } else {
                            Style::default().fg(Color::DarkGray)
                        }),
                )
                .style(if cancel_focused {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Red)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Red)
                })
                .alignment(Alignment::Center),
            buttons[1],
        );

        if form.dropdown_open {
            let count = BACKEND_TYPE_OPTIONS.len() as u16;
            let dropdown_area = Rect::new(
                chunks[2].x,
                chunks[2].y + chunks[2].height,
                chunks[2].width,
                count + 2,
            );
            let items: Vec<ListItem> = BACKEND_TYPE_OPTIONS
                .iter()
                .map(|backend| {
                    let style = if *backend == form.backend_type {
                        Style::default().fg(Color::Black).bg(Color::Cyan)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    ListItem::new(Self::backend_label(backend)).style(style)
                })
                .collect();
            f.render_widget(Clear, dropdown_area);
            f.render_widget(
                List::new(items).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Cyan))
                        .style(Style::default().bg(Color::Black)),
                ),
                dropdown_area,
            );
            for i in 0..count {
                form.hit_areas.dropdown_items.push(Rect::new(
                    dropdown_area.x + 1,
                    dropdown_area.y + 1 + i,
                    dropdown_area.width - 2,
                    1,
                ));
            }
        }
    }

    fn render_edit_providers_popup(
        f: &mut Frame,
        parent_area: Rect,
        providers: &[EditableProvider],
        popup: &mut EditProvidersPopupState,
    ) {
        popup.hit_areas = EditProvidersHitAreas::default();

        let popup_area = Self::centered_rect_in(48, 60, parent_area);
        let block = Block::default()
            .title(" Edit Providers ")
            .title_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .style(Style::default().bg(Color::Black));
        let inner = block.inner(popup_area);
        f.render_widget(Clear, popup_area);
        f.render_widget(block, popup_area);

        if providers.is_empty() {
            f.render_widget(
                Paragraph::new("No providers saved yet.")
                    .style(Style::default().fg(Color::DarkGray))
                    .alignment(Alignment::Center),
                inner,
            );
            return;
        }

        let max_visible = inner.height as usize;
        for (i, provider) in providers.iter().enumerate().take(max_visible) {
            let row_y = inner.y + i as u16;
            let content_width = inner.width.saturating_sub(5);
            let name_area = Rect::new(inner.x, row_y, content_width, 1);
            let delete_area = Rect::new(inner.x + content_width, row_y, 4, 1);
            let name_focused = popup.focus == Some(EditProvidersFocus::ProviderName(i));
            let delete_focused = popup.focus == Some(EditProvidersFocus::DeleteButton(i));

            f.render_widget(
                Paragraph::new(provider.name.clone()).style(if name_focused {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                }),
                name_area,
            );
            f.render_widget(
                Paragraph::new("[X]").style(if delete_focused {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Red)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Red)
                }),
                delete_area,
            );

            popup.hit_areas.provider_rows.push((name_area, delete_area));
        }
    }

    fn render_keybindings(&self, f: &mut Frame, area: Rect) {
        let bindings = vec![
            ("Ctrl+T", "New Tab"),
            ("Ctrl+N", "New Chat"),
            ("Ctrl+W", "Close Tab"),
            ("Ctrl+Shift+W", "Close Chat"),
            ("Ctrl+B", "Toggle Sidebar"),
            ("Ctrl+,", "Toggle Settings"),
            ("Ctrl+Q", "Quit"),
            ("Ctrl+C", "Cancel / Quit (press twice)"),
            ("Enter", "Send message"),
            ("/quit, /exit, /q", "Quit via chat"),
            ("/theme", "Choose and apply a theme"),
            ("/skills", "Show installed skills"),
            ("/mcp", "Show MCP servers"),
            ("/vault <query>", "Search the configured vault"),
            ("/web, /web on, /web off", "Toggle local web search"),
            ("@obsidian", "Search, read, or update the configured vault"),
            ("@websearch", "Use local web search"),
            ("@save", "Create a sidebar markdown artifact"),
        ];

        let lines: Vec<Line> = bindings
            .iter()
            .map(|&(key, desc)| {
                Line::from(vec![
                    Span::styled(
                        format!("{:22}", key),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(desc),
                ])
            })
            .collect();

        let paragraph = Paragraph::new(lines)
            .block(
                Block::default()
                    .title(" Keyboard Shortcuts ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::DarkGray)),
            )
            .wrap(Wrap { trim: true });

        f.render_widget(paragraph, area);
    }

    pub fn next_tab(&mut self) {
        self.active_tab = match self.active_tab {
            SettingsTab::General => SettingsTab::Keybindings,
            SettingsTab::Keybindings => SettingsTab::Providers,
            SettingsTab::Providers => SettingsTab::Models,
            SettingsTab::Models => SettingsTab::Local,
            SettingsTab::Local => SettingsTab::Mcp,
            SettingsTab::Mcp => SettingsTab::General,
        };
        self.reset_focus();
    }

    pub fn prev_tab(&mut self) {
        self.active_tab = match self.active_tab {
            SettingsTab::General => SettingsTab::Mcp,
            SettingsTab::Keybindings => SettingsTab::General,
            SettingsTab::Providers => SettingsTab::Keybindings,
            SettingsTab::Models => SettingsTab::Providers,
            SettingsTab::Local => SettingsTab::Models,
            SettingsTab::Mcp => SettingsTab::Local,
        };
        self.reset_focus();
    }

    fn reset_focus(&mut self) {
        self.general_focus = GeneralFocus::Theme;
        self.general_dropdown_open = None;
        self.providers_tab_focus = ProvidersTabFocus::DefaultProvider;
        self.models_tab_focus = ModelsTabFocus::Provider;
        self.models_dropdown_open = false;
        self.local_focus = LocalFocus::Enabled;
        self.mcp_focus = 0;
    }
    pub fn type_char(&mut self, c: char) {
        if let Some(popup) = self.preset_key_popup.as_mut() {
            if self.providers_tab_focus == ProvidersTabFocus::PopupApiKey {
                popup.api_key.push(c);
            }
        } else if let Some(form) = self.edit_provider_popup.as_mut() {
            Self::type_char_in_form(form, c);
        } else if let Some(form) = self.add_provider_popup.as_mut() {
            Self::type_char_in_form(form, c);
        } else if self.active_tab == SettingsTab::General {
            if self.general_focus == GeneralFocus::ArtifactSaveDir {
                self.artifact_save_dir.push(c);
            }
        } else if self.active_tab == SettingsTab::Local {
            match self.local_focus {
                LocalFocus::Host => self.local_host.push(c),
                LocalFocus::Port => {
                    if c.is_ascii_digit() {
                        self.local_port.push(c);
                    }
                }
                LocalFocus::SelectedModel => self.local_selected_model.push(c),
                LocalFocus::ModelDirectory => self.local_model_directory.push(c),
                LocalFocus::HealthInterval => {
                    if c.is_ascii_digit() {
                        self.local_health_interval_seconds.push(c);
                    }
                }
                LocalFocus::ConnectTimeout => {
                    if c.is_ascii_digit() {
                        self.local_connect_timeout_ms.push(c);
                    }
                }
                LocalFocus::RequestTimeout => {
                    if c.is_ascii_digit() {
                        self.local_request_timeout_ms.push(c);
                    }
                }
                LocalFocus::ApiTokenEnv => {
                    if c.is_ascii_alphanumeric() || c == '_' {
                        self.local_api_token_env.push(c);
                    }
                }
                LocalFocus::Enabled | LocalFocus::ServerType => {}
            }
        }
    }

    pub fn backspace(&mut self) {
        if let Some(popup) = self.preset_key_popup.as_mut() {
            if self.providers_tab_focus == ProvidersTabFocus::PopupApiKey {
                popup.api_key.pop();
            }
        } else if let Some(form) = self.edit_provider_popup.as_mut() {
            Self::backspace_in_form(form);
        } else if let Some(form) = self.add_provider_popup.as_mut() {
            Self::backspace_in_form(form);
        } else if self.active_tab == SettingsTab::General {
            if self.general_focus == GeneralFocus::ArtifactSaveDir {
                self.artifact_save_dir.pop();
            }
        } else if self.active_tab == SettingsTab::Local {
            match self.local_focus {
                LocalFocus::Host => {
                    self.local_host.pop();
                }
                LocalFocus::Port => {
                    self.local_port.pop();
                }
                LocalFocus::SelectedModel => {
                    self.local_selected_model.pop();
                }
                LocalFocus::ModelDirectory => {
                    self.local_model_directory.pop();
                }
                LocalFocus::HealthInterval => {
                    self.local_health_interval_seconds.pop();
                }
                LocalFocus::ConnectTimeout => {
                    self.local_connect_timeout_ms.pop();
                }
                LocalFocus::RequestTimeout => {
                    self.local_request_timeout_ms.pop();
                }
                LocalFocus::ApiTokenEnv => {
                    self.local_api_token_env.pop();
                }
                LocalFocus::Enabled | LocalFocus::ServerType => {}
            }
        }
    }

    fn type_char_in_form(form: &mut ProviderFormState, c: char) {
        match form.focus {
            ProviderFormFocus::ProviderName => form.name.push(c),
            ProviderFormFocus::ProviderEndpoint => form.endpoint.push(c),
            ProviderFormFocus::ProviderApiKey => form.api_key.push(c),
            _ => {}
        }
    }

    fn backspace_in_form(form: &mut ProviderFormState) {
        match form.focus {
            ProviderFormFocus::ProviderName => {
                form.name.pop();
            }
            ProviderFormFocus::ProviderEndpoint => {
                form.endpoint.pop();
            }
            ProviderFormFocus::ProviderApiKey => {
                form.api_key.pop();
            }
            _ => {}
        }
    }

    pub fn provider_popup_active(&self) -> bool {
        self.add_provider_popup.is_some()
            || self.edit_providers_popup.is_some()
            || self.edit_provider_popup.is_some()
            || self.preset_key_popup.is_some()
    }

    pub fn close_active_provider_popup(&mut self) -> bool {
        self.preset_key_popup.take().is_some()
            || self.edit_provider_popup.take().is_some()
            || self.edit_providers_popup.take().is_some()
            || self.add_provider_popup.take().is_some()
    }

    pub fn next_popup_focus(&mut self) {
        if self.preset_key_popup.is_some() {
            self.providers_tab_focus = match self.providers_tab_focus {
                ProvidersTabFocus::PopupApiKey => ProvidersTabFocus::PopupSaveButton,
                ProvidersTabFocus::PopupSaveButton => ProvidersTabFocus::PopupCancelButton,
                ProvidersTabFocus::PopupCancelButton => ProvidersTabFocus::PopupApiKey,
                _ => ProvidersTabFocus::PopupApiKey,
            };
        } else if let Some(form) = self.edit_provider_popup.as_mut() {
            Self::cycle_form_focus(form, true);
        } else if let Some(popup) = self.edit_providers_popup.as_mut() {
            Self::cycle_edit_popup_focus(popup, self.providers_tab_list.len(), true);
        } else if let Some(form) = self.add_provider_popup.as_mut() {
            Self::cycle_form_focus(form, true);
        }
    }

    pub fn prev_popup_focus(&mut self) {
        if self.preset_key_popup.is_some() {
            self.providers_tab_focus = match self.providers_tab_focus {
                ProvidersTabFocus::PopupApiKey => ProvidersTabFocus::PopupCancelButton,
                ProvidersTabFocus::PopupSaveButton => ProvidersTabFocus::PopupApiKey,
                ProvidersTabFocus::PopupCancelButton => ProvidersTabFocus::PopupSaveButton,
                _ => ProvidersTabFocus::PopupApiKey,
            };
        } else if let Some(form) = self.edit_provider_popup.as_mut() {
            Self::cycle_form_focus(form, false);
        } else if let Some(popup) = self.edit_providers_popup.as_mut() {
            Self::cycle_edit_popup_focus(popup, self.providers_tab_list.len(), false);
        } else if let Some(form) = self.add_provider_popup.as_mut() {
            Self::cycle_form_focus(form, false);
        }
    }

    fn cycle_form_focus(form: &mut ProviderFormState, forward: bool) {
        form.dropdown_open = false;
        form.focus = match (form.focus, forward) {
            (ProviderFormFocus::ProviderName, true) => ProviderFormFocus::ProviderEndpoint,
            (ProviderFormFocus::ProviderEndpoint, true) => ProviderFormFocus::ProviderBackendType,
            (ProviderFormFocus::ProviderBackendType, true) => ProviderFormFocus::ProviderApiKey,
            (ProviderFormFocus::ProviderApiKey, true) => ProviderFormFocus::SubmitButton,
            (ProviderFormFocus::SubmitButton, true) => ProviderFormFocus::CancelButton,
            (ProviderFormFocus::CancelButton, true) => ProviderFormFocus::ProviderName,
            (ProviderFormFocus::ProviderName, false) => ProviderFormFocus::CancelButton,
            (ProviderFormFocus::ProviderEndpoint, false) => ProviderFormFocus::ProviderName,
            (ProviderFormFocus::ProviderBackendType, false) => ProviderFormFocus::ProviderEndpoint,
            (ProviderFormFocus::ProviderApiKey, false) => ProviderFormFocus::ProviderBackendType,
            (ProviderFormFocus::SubmitButton, false) => ProviderFormFocus::ProviderApiKey,
            (ProviderFormFocus::CancelButton, false) => ProviderFormFocus::SubmitButton,
        };
    }

    fn cycle_edit_popup_focus(popup: &mut EditProvidersPopupState, len: usize, forward: bool) {
        if len == 0 {
            popup.focus = None;
            return;
        }
        popup.focus = Some(
            match (
                popup.focus.unwrap_or(EditProvidersFocus::ProviderName(0)),
                forward,
            ) {
                (EditProvidersFocus::ProviderName(idx), true) => {
                    EditProvidersFocus::DeleteButton(idx)
                }
                (EditProvidersFocus::DeleteButton(idx), true) => {
                    if idx + 1 < len {
                        EditProvidersFocus::ProviderName(idx + 1)
                    } else {
                        EditProvidersFocus::ProviderName(0)
                    }
                }
                (EditProvidersFocus::ProviderName(idx), false) => {
                    if idx == 0 {
                        EditProvidersFocus::DeleteButton(len - 1)
                    } else {
                        EditProvidersFocus::DeleteButton(idx - 1)
                    }
                }
                (EditProvidersFocus::DeleteButton(idx), false) => {
                    EditProvidersFocus::ProviderName(idx)
                }
            },
        );
    }

    pub fn popup_up(&mut self) {
        if self.preset_key_popup.is_some() {
            self.providers_tab_focus = match self.providers_tab_focus {
                ProvidersTabFocus::PopupSaveButton | ProvidersTabFocus::PopupCancelButton => {
                    ProvidersTabFocus::PopupApiKey
                }
                _ => self.providers_tab_focus,
            };
        } else if let Some(form) = self.edit_provider_popup.as_mut() {
            Self::move_backend_selection(form, false);
        } else if let Some(popup) = self.edit_providers_popup.as_mut() {
            Self::move_edit_popup_vertically(popup, self.providers_tab_list.len(), false);
        } else if let Some(form) = self.add_provider_popup.as_mut() {
            Self::move_backend_selection(form, false);
        }
    }

    pub fn popup_down(&mut self) {
        if self.preset_key_popup.is_some() {
            self.providers_tab_focus = match self.providers_tab_focus {
                ProvidersTabFocus::PopupApiKey => ProvidersTabFocus::PopupSaveButton,
                _ => self.providers_tab_focus,
            };
        } else if let Some(form) = self.edit_provider_popup.as_mut() {
            Self::move_backend_selection(form, true);
        } else if let Some(popup) = self.edit_providers_popup.as_mut() {
            Self::move_edit_popup_vertically(popup, self.providers_tab_list.len(), true);
        } else if let Some(form) = self.add_provider_popup.as_mut() {
            Self::move_backend_selection(form, true);
        }
    }

    fn move_backend_selection(form: &mut ProviderFormState, forward: bool) {
        if !form.dropdown_open || form.focus != ProviderFormFocus::ProviderBackendType {
            return;
        }
        let idx = BACKEND_TYPE_OPTIONS
            .iter()
            .position(|backend| *backend == form.backend_type)
            .unwrap_or(0);
        let new_idx = if forward {
            (idx + 1) % BACKEND_TYPE_OPTIONS.len()
        } else if idx == 0 {
            BACKEND_TYPE_OPTIONS.len() - 1
        } else {
            idx - 1
        };
        form.backend_type = BACKEND_TYPE_OPTIONS[new_idx].to_string();
    }

    fn move_edit_popup_vertically(popup: &mut EditProvidersPopupState, len: usize, forward: bool) {
        if len == 0 {
            popup.focus = None;
            return;
        }
        popup.focus = Some(
            match popup.focus.unwrap_or(EditProvidersFocus::ProviderName(0)) {
                EditProvidersFocus::ProviderName(idx) => {
                    let new_idx = if forward {
                        (idx + 1) % len
                    } else if idx == 0 {
                        len - 1
                    } else {
                        idx - 1
                    };
                    EditProvidersFocus::ProviderName(new_idx)
                }
                EditProvidersFocus::DeleteButton(idx) => {
                    let new_idx = if forward {
                        (idx + 1) % len
                    } else if idx == 0 {
                        len - 1
                    } else {
                        idx - 1
                    };
                    EditProvidersFocus::DeleteButton(new_idx)
                }
            },
        );
    }

    pub fn activate_provider_popup(&mut self) -> ProvidersAction {
        if self.preset_key_popup.is_some() {
            return self.activate_preset_key_popup();
        }
        if self.edit_provider_popup.is_some() {
            return self.activate_form_popup(true);
        }
        if let Some(popup) = self.edit_providers_popup.as_mut() {
            return match popup.focus {
                Some(EditProvidersFocus::ProviderName(idx)) => {
                    self.open_edit_provider_popup(idx);
                    ProvidersAction::None
                }
                Some(EditProvidersFocus::DeleteButton(idx)) => self
                    .providers_tab_list
                    .get(idx)
                    .map(|provider| ProvidersAction::DeleteProvider(provider.name.clone()))
                    .unwrap_or(ProvidersAction::None),
                None => ProvidersAction::None,
            };
        }
        if self.add_provider_popup.is_some() {
            return self.activate_form_popup(false);
        }
        ProvidersAction::None
    }

    fn activate_form_popup(&mut self, is_edit: bool) -> ProvidersAction {
        let form_opt = if is_edit {
            self.edit_provider_popup.as_mut()
        } else {
            self.add_provider_popup.as_mut()
        };
        let Some(form) = form_opt else {
            return ProvidersAction::None;
        };

        if form.dropdown_open && form.focus == ProviderFormFocus::ProviderBackendType {
            form.dropdown_open = false;
            return ProvidersAction::None;
        }

        let trimmed_name = form.name.trim().to_string();
        let original_name = form.original_name.clone();
        let duplicate_name = self.providers_tab_list.iter().any(|provider| {
            provider.name == trimmed_name
                && original_name
                    .as_ref()
                    .map(|original| original != &trimmed_name)
                    .unwrap_or(true)
        });

        match form.focus {
            ProviderFormFocus::ProviderBackendType => {
                form.dropdown_open = !form.dropdown_open;
                ProvidersAction::None
            }
            ProviderFormFocus::CancelButton => {
                if is_edit {
                    self.edit_provider_popup = None;
                } else {
                    self.add_provider_popup = None;
                }
                ProvidersAction::None
            }
            ProviderFormFocus::SubmitButton => {
                if !form.can_submit() || duplicate_name {
                    return ProvidersAction::None;
                }
                let provider = EditableProvider {
                    name: form.name.trim().to_string(),
                    endpoint: form.endpoint.trim().to_string(),
                    backend_type: form.backend_type.trim().to_string(),
                };
                let api_key = form.api_key.trim().to_string();
                if is_edit {
                    let original_name = form
                        .original_name
                        .clone()
                        .unwrap_or_else(|| provider.name.clone());
                    self.edit_provider_popup = None;
                    ProvidersAction::SubmitEdit {
                        original_name,
                        provider,
                        api_key,
                    }
                } else {
                    self.add_provider_popup = None;
                    ProvidersAction::SubmitAdd { provider, api_key }
                }
            }
            _ => ProvidersAction::None,
        }
    }

    pub fn handle_provider_popup_click(&mut self, pos: Position) -> ProvidersAction {
        if self.preset_key_popup.is_some() {
            return self.handle_preset_key_popup_click(pos);
        }
        if let Some(form) = self.edit_provider_popup.as_mut() {
            if let Some(action) = Self::handle_form_click(form, pos) {
                return match action {
                    FormClickAction::Activate => self.activate_form_popup(true),
                    FormClickAction::Noop => ProvidersAction::None,
                };
            }
            return ProvidersAction::None;
        }
        if let Some(popup) = self.edit_providers_popup.as_mut() {
            for (idx, (name_area, delete_area)) in popup.hit_areas.provider_rows.iter().enumerate()
            {
                if name_area.contains(pos) {
                    popup.focus = Some(EditProvidersFocus::ProviderName(idx));
                    self.open_edit_provider_popup(idx);
                    return ProvidersAction::None;
                }
                if delete_area.contains(pos) {
                    popup.focus = Some(EditProvidersFocus::DeleteButton(idx));
                    if let Some(provider) = self.providers_tab_list.get(idx) {
                        return ProvidersAction::DeleteProvider(provider.name.clone());
                    }
                }
            }
            return ProvidersAction::None;
        }
        if let Some(form) = self.add_provider_popup.as_mut() {
            if let Some(action) = Self::handle_form_click(form, pos) {
                return match action {
                    FormClickAction::Activate => self.activate_form_popup(false),
                    FormClickAction::Noop => ProvidersAction::None,
                };
            }
            return ProvidersAction::None;
        }
        ProvidersAction::None
    }

    fn handle_form_click(form: &mut ProviderFormState, pos: Position) -> Option<FormClickAction> {
        if form.dropdown_open {
            for (idx, area) in form.hit_areas.dropdown_items.iter().enumerate() {
                if area.contains(pos) {
                    form.backend_type = BACKEND_TYPE_OPTIONS[idx].to_string();
                    form.dropdown_open = false;
                    form.focus = ProviderFormFocus::ProviderBackendType;
                    return Some(FormClickAction::Noop);
                }
            }
        }
        if let Some(area) = form.hit_areas.name {
            if area.contains(pos) {
                form.focus = ProviderFormFocus::ProviderName;
                form.dropdown_open = false;
                return Some(FormClickAction::Noop);
            }
        }
        if let Some(area) = form.hit_areas.endpoint {
            if area.contains(pos) {
                form.focus = ProviderFormFocus::ProviderEndpoint;
                form.dropdown_open = false;
                return Some(FormClickAction::Noop);
            }
        }
        if let Some(area) = form.hit_areas.backend_type {
            if area.contains(pos) {
                form.focus = ProviderFormFocus::ProviderBackendType;
                form.dropdown_open = !form.dropdown_open;
                return Some(FormClickAction::Noop);
            }
        }
        if let Some(area) = form.hit_areas.api_key {
            if area.contains(pos) {
                form.focus = ProviderFormFocus::ProviderApiKey;
                form.dropdown_open = false;
                return Some(FormClickAction::Noop);
            }
        }
        if let Some(area) = form.hit_areas.submit_button {
            if area.contains(pos) {
                form.focus = ProviderFormFocus::SubmitButton;
                return Some(FormClickAction::Activate);
            }
        }
        if let Some(area) = form.hit_areas.cancel_button {
            if area.contains(pos) {
                form.focus = ProviderFormFocus::CancelButton;
                return Some(FormClickAction::Activate);
            }
        }
        None
    }

    pub fn open_add_provider_popup(&mut self) {
        self.add_provider_popup = Some(ProviderFormState::new_add());
    }

    pub fn open_edit_providers_popup(&mut self) {
        self.edit_providers_popup = Some(EditProvidersPopupState::new(
            !self.providers_tab_list.is_empty(),
        ));
    }

    pub fn open_edit_provider_popup(&mut self, idx: usize) {
        if let Some(provider) = self.providers_tab_list.get(idx) {
            let api_key = self
                .saved_keys
                .iter()
                .find(|(name, _)| name == &provider.name)
                .map(|(_, key)| key.clone())
                .unwrap_or_default();
            self.edit_provider_popup = Some(ProviderFormState::new_edit(provider, api_key));
        }
    }

    pub fn open_preset_key_popup(&mut self, idx: usize) {
        if let Some((name, endpoint, _, _, _)) = self.preset_api_key_providers().get(idx) {
            let api_key = self
                .saved_keys
                .iter()
                .find(|(provider_name, _)| provider_name == name)
                .map(|(_, key)| key.clone())
                .unwrap_or_default();
            self.preset_key_popup = Some(PresetKeyPopupState::new(
                name.clone(),
                endpoint.clone(),
                api_key,
            ));
            self.providers_tab_focus = ProvidersTabFocus::PopupApiKey;
        }
    }

    pub fn apply_add_provider(&mut self, provider: EditableProvider, api_key: String) {
        let env_var = Self::env_var_for_name(&provider.name);
        self.db_providers.push((
            provider.name.clone(),
            provider.endpoint.clone(),
            env_var,
            provider.backend_type.clone(),
            "api_key".to_string(),
        ));
        self.providers_tab_list.push(provider.clone());
        self.providers_tab_list.sort_by(|a, b| a.name.cmp(&b.name));
        self.db_providers.sort_by(|a, b| a.0.cmp(&b.0));
        self.set_saved_key(provider.name, api_key);
        self.add_provider_popup = None;
    }

    pub fn apply_update_provider(
        &mut self,
        original_name: &str,
        provider: EditableProvider,
        api_key: String,
    ) {
        if let Some(entry) = self
            .db_providers
            .iter_mut()
            .find(|(name, _, _, _, _)| name == original_name)
        {
            let auth_type = entry.4.clone();
            *entry = (
                provider.name.clone(),
                provider.endpoint.clone(),
                Self::env_var_for_name(&provider.name),
                provider.backend_type.clone(),
                auth_type,
            );
        }
        if let Some(editable) = self
            .providers_tab_list
            .iter_mut()
            .find(|existing| existing.name == original_name)
        {
            *editable = provider.clone();
        }
        self.providers_tab_list.sort_by(|a, b| a.name.cmp(&b.name));
        self.db_providers.sort_by(|a, b| a.0.cmp(&b.0));

        self.saved_keys.retain(|(name, _)| name != original_name);
        self.set_saved_key(provider.name.clone(), api_key);
        if self.default_provider == original_name {
            self.default_provider = provider.name;
            self.default_model.clear();
        }
        self.edit_provider_popup = None;
    }

    pub fn apply_preset_key_save(&mut self, provider_name: String, api_key: String) {
        self.set_saved_key(provider_name, api_key);
        self.preset_key_popup = None;
    }

    pub fn remove_provider_by_name(&mut self, name: &str) {
        self.db_providers
            .retain(|(provider_name, _, _, _, _)| provider_name != name);
        self.providers_tab_list
            .retain(|provider| provider.name != name);
        self.saved_keys
            .retain(|(provider_name, _)| provider_name != name);

        if self.default_provider == name {
            self.default_provider = self
                .saved_keys
                .first()
                .map(|(provider_name, _)| provider_name.clone())
                .unwrap_or_default();
            self.default_model.clear();
        }

        if let Some(popup) = self.edit_providers_popup.as_mut() {
            let len = self.providers_tab_list.len();
            popup.focus = if len == 0 {
                None
            } else {
                match popup.focus.unwrap_or(EditProvidersFocus::ProviderName(0)) {
                    EditProvidersFocus::ProviderName(idx) => {
                        Some(EditProvidersFocus::ProviderName(idx.min(len - 1)))
                    }
                    EditProvidersFocus::DeleteButton(idx) => {
                        Some(EditProvidersFocus::DeleteButton(idx.min(len - 1)))
                    }
                }
            };
        }

        if self
            .edit_provider_popup
            .as_ref()
            .and_then(|form| form.original_name.as_ref())
            .map(|original| original == name)
            .unwrap_or(false)
        {
            self.edit_provider_popup = None;
        }
    }

    fn set_saved_key(&mut self, provider: String, api_key: String) {
        self.saved_keys.retain(|(name, _)| name != &provider);
        if !api_key.is_empty() {
            self.saved_keys.push((provider, api_key));
            self.saved_keys.sort_by(|a, b| a.0.cmp(&b.0));
        }
    }

    pub fn oauth_providers(&self) -> Vec<ProviderEntry> {
        self.db_providers
            .iter()
            .filter(|(_, _, _, _, auth_type)| auth_type == "oauth")
            .cloned()
            .collect()
    }

    pub fn preset_api_key_providers(&self) -> Vec<ProviderEntry> {
        let mut providers: Vec<ProviderEntry> = self
            .db_providers
            .iter()
            .filter(|(name, _, _, _, auth_type)| {
                auth_type == "api_key" && PRESET_PROVIDER_NAMES.contains(&name.as_str())
            })
            .cloned()
            .collect();
        providers.extend(
            SEARCH_KEY_PROVIDERS
                .iter()
                .map(|(name, endpoint, env_var)| {
                    (
                        (*name).to_string(),
                        (*endpoint).to_string(),
                        (*env_var).to_string(),
                        "search".to_string(),
                        "api_key".to_string(),
                    )
                }),
        );
        providers
    }

    fn has_saved_key(&self, provider_name: &str) -> bool {
        self.saved_keys
            .iter()
            .any(|(name, key)| name == provider_name && !key.trim().is_empty())
    }

    fn env_var_for_name(name: &str) -> String {
        format!("{}_API_KEY", name.to_uppercase().replace(' ', "_"))
    }

    pub fn small_model_provider(&self) -> String {
        self.small_model.split(':').next().unwrap_or("").to_string()
    }

    pub fn small_model_name(&self) -> String {
        self.small_model.split(':').nth(1).unwrap_or("").to_string()
    }

    pub fn small_model_tuple(&self) -> Option<(String, String, String, String, String, String)> {
        let prov = self.small_model_provider();
        let model = self.small_model_name();
        if prov.is_empty() {
            return None;
        }
        let (endpoint, env_var, backend_type, _auth) = self
            .db_providers
            .iter()
            .find(|(n, _, _, _, _)| n == &prov)
            .map(|(_, ep, ev, bt, au)| (ep.clone(), ev.clone(), bt.clone(), au.clone()))
            .unwrap_or_default();
        Some((prov.clone(), endpoint, env_var, backend_type, prov, model))
    }

    pub fn toggle_providers_dropdown(&mut self, dropdown: ProvidersDropdown) {
        if self.providers_dropdown_open == Some(dropdown) {
            self.providers_dropdown_open = None;
        } else {
            self.providers_dropdown_open = Some(dropdown);
            self.dropdown_scroll_offset = 0;
        }
    }

    pub fn all_enabled_provider_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .db_providers
            .iter()
            .filter(|(name, _, _, _, _)| !self.disabled_providers.contains(name))
            .map(|(name, _, _, _, _)| name.clone())
            .collect();
        names.sort();
        names
    }

    pub fn disabled_model_key(provider: &str, model: &str) -> String {
        format!("{provider}:{model}")
    }

    pub fn toggle_models_dropdown(&mut self) {
        self.models_dropdown_open = !self.models_dropdown_open;
        if self.models_dropdown_open {
            self.models_dropdown_scroll_offset = 0;
        }
    }

    pub fn select_models_provider_dropdown_item(&mut self, idx: usize) {
        let real_idx = idx + self.models_dropdown_scroll_offset;
        let provider_names = self.all_enabled_provider_names();
        if let Some(provider) = provider_names.get(real_idx) {
            self.models_provider = provider.clone();
            self.models_tab_focus = ModelsTabFocus::Provider;
        }
        self.models_dropdown_open = false;
        self.models_dropdown_scroll_offset = 0;
    }

    pub fn models_dropdown_up(&mut self) {
        if self.models_dropdown_scroll_offset > 0 {
            self.models_dropdown_scroll_offset -= 1;
        }
    }

    pub fn models_dropdown_down(&mut self) {
        let total = self.all_enabled_provider_names().len();
        let max_visible = 8.min(total);
        let max_offset = total.saturating_sub(max_visible);
        if self.models_dropdown_scroll_offset < max_offset {
            self.models_dropdown_scroll_offset += 1;
        }
    }

    pub fn move_models_tab_focus(&mut self, forward: bool) {
        if self.models_dropdown_open {
            if forward {
                self.models_dropdown_down();
            } else {
                self.models_dropdown_up();
            }
            return;
        }
        let count = self.models_available_models.len();
        self.models_tab_focus = match self.models_tab_focus {
            ModelsTabFocus::Provider if forward && count > 0 => ModelsTabFocus::Model(0),
            ModelsTabFocus::Provider => ModelsTabFocus::Provider,
            ModelsTabFocus::Model(idx) if forward && idx + 1 < count => {
                ModelsTabFocus::Model(idx + 1)
            }
            ModelsTabFocus::Model(_) if forward => ModelsTabFocus::Provider,
            ModelsTabFocus::Model(0) => ModelsTabFocus::Provider,
            ModelsTabFocus::Model(idx) => ModelsTabFocus::Model(idx - 1),
        };
    }

    pub fn activate_models_focus(&mut self) {
        match self.models_tab_focus {
            ModelsTabFocus::Provider => self.toggle_models_dropdown(),
            ModelsTabFocus::Model(idx) => {
                if let Some(model) = self.models_available_models.get(idx) {
                    let key = Self::disabled_model_key(&self.models_provider, &model.id);
                    if self.disabled_models.contains(&key) {
                        self.disabled_models.remove(&key);
                    } else {
                        self.disabled_models.insert(key);
                    }
                }
            }
        }
    }

    pub fn select_providers_dropdown_item(&mut self, idx: usize) {
        let real_idx = idx + self.dropdown_scroll_offset;
        let provider_names = self.all_enabled_provider_names();
        match self.providers_dropdown_open {
            Some(ProvidersDropdown::DefaultProvider) => {
                if real_idx < provider_names.len() {
                    self.default_provider = provider_names[real_idx].clone();
                    self.default_model.clear();
                }
            }
            Some(ProvidersDropdown::SmallProvider) => {
                if real_idx < provider_names.len() {
                    let current_model = self.small_model_name();
                    let new_prov = &provider_names[real_idx];
                    self.small_model = format!("{}:{}", new_prov, current_model);
                }
            }
            Some(ProvidersDropdown::DefaultModel) => {
                if let Some(model) = self.available_models.get(real_idx) {
                    self.default_model = model.id.clone();
                }
            }
            Some(ProvidersDropdown::SmallModel) => {
                if let Some(model) = self.available_models.get(real_idx) {
                    let current_prov = self.small_model_provider();
                    self.small_model = format!("{}:{}", current_prov, model.id);
                }
            }
            None => {}
        }
        self.providers_dropdown_open = None;
        self.dropdown_scroll_offset = 0;
    }

    pub fn providers_dropdown_up(&mut self) {
        if self.dropdown_scroll_offset > 0 {
            self.dropdown_scroll_offset -= 1;
        }
    }

    pub fn providers_dropdown_down(&mut self) {
        let total = match self.providers_dropdown_open {
            Some(ProvidersDropdown::DefaultProvider) | Some(ProvidersDropdown::SmallProvider) => {
                self.all_enabled_provider_names().len()
            }
            Some(ProvidersDropdown::DefaultModel) | Some(ProvidersDropdown::SmallModel) => {
                self.available_models.len()
            }
            None => return,
        };
        let max_visible = 8.min(total);
        let max_offset = total.saturating_sub(max_visible);
        if self.dropdown_scroll_offset < max_offset {
            self.dropdown_scroll_offset += 1;
        }
    }

    pub fn handle_providers_click(&mut self, pos: Position) -> ProvidersAction {
        if let Some(area) = self.providers_tab_hit_areas.popup_api_key {
            if area.contains(pos) {
                self.providers_tab_focus = ProvidersTabFocus::PopupApiKey;
                return ProvidersAction::None;
            }
        }
        if let Some(area) = self.providers_tab_hit_areas.popup_save {
            if area.contains(pos) {
                self.providers_tab_focus = ProvidersTabFocus::PopupSaveButton;
                return self.activate_preset_key_popup();
            }
        }
        if let Some(area) = self.providers_tab_hit_areas.popup_cancel {
            if area.contains(pos) {
                self.providers_tab_focus = ProvidersTabFocus::PopupCancelButton;
                self.preset_key_popup = None;
                return ProvidersAction::None;
            }
        }
        ProvidersAction::None
    }

    pub fn select_general_dropdown_item(&mut self, idx: usize) -> bool {
        match self.general_dropdown_open {
            Some(GeneralDropdown::Theme) => {
                if let Some(theme) = crate::theme::theme_keys().get(idx) {
                    let changed = crate::theme::canonical_theme_key(&self.theme) != *theme;
                    self.theme = (*theme).to_string();
                    self.general_dropdown_open = None;
                    return changed;
                }
            }
            Some(GeneralDropdown::UserAlignment) => {
                if let Some(alignment) = ALIGNMENT_OPTIONS.get(idx).copied() {
                    self.user_alignment = alignment;
                }
            }
            Some(GeneralDropdown::AiAlignment) => {
                if let Some(alignment) = ALIGNMENT_OPTIONS.get(idx).copied() {
                    self.ai_alignment = alignment;
                }
            }
            Some(GeneralDropdown::KittyTextScale) => {
                if let Some(scale) = KITTY_TEXT_SCALE_OPTIONS.get(idx).copied() {
                    self.kitty_text_max_scale = scale;
                }
            }
            None => {}
        }
        self.general_dropdown_open = None;
        false
    }

    pub fn general_dropdown_len(&self) -> usize {
        match self.general_dropdown_open {
            Some(GeneralDropdown::Theme) => crate::theme::theme_keys().len(),
            Some(GeneralDropdown::UserAlignment) => ALIGNMENT_OPTIONS.len(),
            Some(GeneralDropdown::AiAlignment) => ALIGNMENT_OPTIONS.len(),
            Some(GeneralDropdown::KittyTextScale) => KITTY_TEXT_SCALE_OPTIONS.len(),
            None => 0,
        }
    }

    pub fn general_dropdown_current_idx(&self) -> usize {
        match self.general_dropdown_open {
            Some(GeneralDropdown::Theme) => crate::theme::theme_keys()
                .iter()
                .position(|theme| *theme == crate::theme::canonical_theme_key(&self.theme))
                .unwrap_or(0),
            Some(GeneralDropdown::UserAlignment) => ALIGNMENT_OPTIONS
                .iter()
                .position(|a| *a == self.user_alignment)
                .unwrap_or(0),
            Some(GeneralDropdown::AiAlignment) => ALIGNMENT_OPTIONS
                .iter()
                .position(|a| *a == self.ai_alignment)
                .unwrap_or(0),
            Some(GeneralDropdown::KittyTextScale) => KITTY_TEXT_SCALE_OPTIONS
                .iter()
                .position(|scale| *scale == self.kitty_text_max_scale)
                .unwrap_or(0),
            None => 0,
        }
    }

    pub fn general_dropdown_up(&mut self) {
        let len = self.general_dropdown_len();
        if len == 0 {
            return;
        }
        let idx = self.general_dropdown_current_idx();
        let new_idx = if idx == 0 { len - 1 } else { idx - 1 };
        self.select_general_dropdown_item(new_idx);
        if let Some(dropdown) = self.focus_to_dropdown() {
            self.general_dropdown_open = Some(dropdown);
        }
    }

    pub fn general_dropdown_down(&mut self) {
        let len = self.general_dropdown_len();
        if len == 0 {
            return;
        }
        let idx = self.general_dropdown_current_idx();
        let new_idx = (idx + 1) % len;
        self.select_general_dropdown_item(new_idx);
        if let Some(dropdown) = self.focus_to_dropdown() {
            self.general_dropdown_open = Some(dropdown);
        }
    }

    fn focus_to_dropdown(&self) -> Option<GeneralDropdown> {
        match self.general_focus {
            GeneralFocus::Theme => Some(GeneralDropdown::Theme),
            GeneralFocus::UserAlignment => Some(GeneralDropdown::UserAlignment),
            GeneralFocus::AiAlignment => Some(GeneralDropdown::AiAlignment),
            GeneralFocus::ArtifactSaveDir => None,
            GeneralFocus::ShowSelector => None,
            GeneralFocus::ShowChatScrollbar => None,
            GeneralFocus::CollapseThinking => None,
            GeneralFocus::KittyEnhancedText => None,
            GeneralFocus::KittyTextScale => Some(GeneralDropdown::KittyTextScale),
            GeneralFocus::WebSearchEnabled => None,
            GeneralFocus::QuitConfirmation => None,
        }
    }

    pub fn grab_keys_from_env(&mut self) {
        for (provider, _, var_name, _, auth_type) in &self.db_providers {
            if auth_type == "oauth" {
                continue;
            }
            if let Ok(val) = std::env::var(var_name) {
                if !val.is_empty() {
                    self.saved_keys.retain(|(p, _)| p != provider);
                    self.saved_keys.push((provider.clone(), val));
                }
            }
        }
        for (provider, _, var_name) in SEARCH_KEY_PROVIDERS {
            if let Ok(val) = std::env::var(var_name) {
                if !val.is_empty() {
                    self.saved_keys.retain(|(p, _)| p != provider);
                    self.saved_keys.push(((*provider).to_string(), val));
                }
            }
        }

        let home = dirs::home_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        for path in [".env".to_string(), format!("{}/.env", home)] {
            if let Ok(content) = std::fs::read_to_string(&path) {
                for line in content.lines() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    for (provider, _, var_name, _, auth_type) in &self.db_providers {
                        if auth_type == "oauth" {
                            continue;
                        }
                        let prefix = format!("{}=", var_name);
                        if let Some(val) = line.strip_prefix(&prefix) {
                            let val = val.trim().trim_matches('"').trim_matches('\'').to_string();
                            if !val.is_empty() {
                                self.saved_keys.retain(|(p, _)| p != provider);
                                self.saved_keys.push((provider.clone(), val));
                            }
                        }
                    }
                    for (provider, _, var_name) in SEARCH_KEY_PROVIDERS {
                        let prefix = format!("{}=", var_name);
                        if let Some(val) = line.strip_prefix(&prefix) {
                            let val = val.trim().trim_matches('"').trim_matches('\'').to_string();
                            if !val.is_empty() {
                                self.saved_keys.retain(|(p, _)| p != provider);
                                self.saved_keys.push(((*provider).to_string(), val));
                            }
                        }
                    }
                }
            }
        }

        self.check_oauth_tokens();
        self.saved_keys.sort_by(|a, b| a.0.cmp(&b.0));
    }

    pub fn prev_focus(&mut self) {
        match self.active_tab {
            SettingsTab::General => {
                self.close_general_dropdown();
                self.general_focus = match self.general_focus {
                    GeneralFocus::Theme => GeneralFocus::QuitConfirmation,
                    GeneralFocus::UserAlignment => GeneralFocus::Theme,
                    GeneralFocus::QuitConfirmation => GeneralFocus::WebSearchEnabled,
                    GeneralFocus::WebSearchEnabled => GeneralFocus::KittyTextScale,
                    GeneralFocus::KittyTextScale => GeneralFocus::KittyEnhancedText,
                    GeneralFocus::KittyEnhancedText => GeneralFocus::CollapseThinking,
                    GeneralFocus::CollapseThinking => GeneralFocus::ShowChatScrollbar,
                    GeneralFocus::ShowChatScrollbar => GeneralFocus::ShowSelector,
                    GeneralFocus::ShowSelector => GeneralFocus::ArtifactSaveDir,
                    GeneralFocus::ArtifactSaveDir => GeneralFocus::AiAlignment,
                    GeneralFocus::AiAlignment => GeneralFocus::UserAlignment,
                };
            }
            SettingsTab::Providers => {
                if let Some(form) = self.add_provider_popup.as_mut() {
                    Self::cycle_form_focus(form, false);
                } else if let Some(popup) = self.edit_providers_popup.as_mut() {
                    Self::cycle_edit_popup_focus(popup, self.providers_tab_list.len(), false);
                } else {
                    self.move_providers_tab_focus(false);
                }
            }
            SettingsTab::Models => self.move_models_tab_focus(false),
            SettingsTab::Local => {
                self.local_focus = match self.local_focus {
                    LocalFocus::Enabled => LocalFocus::ApiTokenEnv,
                    LocalFocus::Host => LocalFocus::Enabled,
                    LocalFocus::Port => LocalFocus::Host,
                    LocalFocus::ServerType => LocalFocus::Port,
                    LocalFocus::SelectedModel => LocalFocus::ServerType,
                    LocalFocus::ModelDirectory => LocalFocus::SelectedModel,
                    LocalFocus::HealthInterval => LocalFocus::ModelDirectory,
                    LocalFocus::ConnectTimeout => LocalFocus::HealthInterval,
                    LocalFocus::RequestTimeout => LocalFocus::ConnectTimeout,
                    LocalFocus::ApiTokenEnv => LocalFocus::RequestTimeout,
                };
            }
            SettingsTab::Mcp => {
                if !self.mcp_servers.is_empty() {
                    self.mcp_focus = if self.mcp_focus == 0 {
                        self.mcp_servers.len() - 1
                    } else {
                        self.mcp_focus - 1
                    };
                }
            }
            SettingsTab::Keybindings => {}
        }
    }

    pub fn activate_focus(&mut self) -> ProvidersAction {
        match self.active_tab {
            SettingsTab::General => match self.general_focus {
                GeneralFocus::Theme => {
                    self.toggle_general_dropdown(GeneralDropdown::Theme);
                    ProvidersAction::None
                }
                GeneralFocus::UserAlignment => {
                    self.toggle_general_dropdown(GeneralDropdown::UserAlignment);
                    ProvidersAction::None
                }
                GeneralFocus::AiAlignment => {
                    self.toggle_general_dropdown(GeneralDropdown::AiAlignment);
                    ProvidersAction::None
                }
                GeneralFocus::ArtifactSaveDir => ProvidersAction::None,
                GeneralFocus::ShowSelector => {
                    self.show_selector = !self.show_selector;
                    ProvidersAction::None
                }
                GeneralFocus::ShowChatScrollbar => {
                    self.show_chat_scrollbar = !self.show_chat_scrollbar;
                    ProvidersAction::None
                }
                GeneralFocus::CollapseThinking => {
                    self.collapse_thinking = !self.collapse_thinking;
                    ProvidersAction::None
                }
                GeneralFocus::KittyEnhancedText => {
                    self.kitty_enhanced_text = !self.kitty_enhanced_text;
                    ProvidersAction::None
                }
                GeneralFocus::KittyTextScale => {
                    self.toggle_general_dropdown(GeneralDropdown::KittyTextScale);
                    ProvidersAction::None
                }
                GeneralFocus::WebSearchEnabled => {
                    self.web_search_enabled = !self.web_search_enabled;
                    ProvidersAction::None
                }
                GeneralFocus::QuitConfirmation => {
                    self.quit_confirmation = !self.quit_confirmation;
                    ProvidersAction::None
                }
            },
            SettingsTab::Providers => match self.providers_tab_focus {
                ProvidersTabFocus::UseEnvToggle => {
                    self.grab_keys_from_env();
                    ProvidersAction::ToggleUseEnv
                }
                ProvidersTabFocus::AddProviderButton => {
                    self.open_add_provider_popup();
                    ProvidersAction::None
                }
                ProvidersTabFocus::EditProvidersButton => {
                    self.open_edit_providers_popup();
                    ProvidersAction::None
                }
                ProvidersTabFocus::ReloadModelsButton => ProvidersAction::RefreshModels,
                ProvidersTabFocus::SmallProvider => {
                    self.toggle_providers_dropdown(ProvidersDropdown::SmallProvider);
                    ProvidersAction::None
                }
                ProvidersTabFocus::SmallModel => {
                    self.toggle_providers_dropdown(ProvidersDropdown::SmallModel);
                    ProvidersAction::None
                }
                ProvidersTabFocus::DefaultProvider => {
                    self.toggle_providers_dropdown(ProvidersDropdown::DefaultProvider);
                    ProvidersAction::None
                }
                ProvidersTabFocus::DefaultModel => {
                    self.toggle_providers_dropdown(ProvidersDropdown::DefaultModel);
                    ProvidersAction::None
                }
                ProvidersTabFocus::SavedKeyList(idx) => {
                    let preset = self.preset_api_key_providers();
                    let custom: Vec<_> = self
                        .db_providers
                        .iter()
                        .filter(|(n, _, _, _, auth_type)| {
                            auth_type != "oauth" && preset.iter().all(|(pn, _, _, _, _)| pn != n)
                        })
                        .collect();
                    if let Some((name, _, _, _, _)) = custom.get(idx) {
                        if self.disabled_providers.contains(name) {
                            self.disabled_providers.remove(name);
                        } else {
                            self.disabled_providers.insert(name.clone());
                        }
                    }
                    ProvidersAction::None
                }
                ProvidersTabFocus::OAuthProvider(idx) => {
                    let oauth = self.oauth_providers();
                    if let Some((name, _, _, _, _)) = oauth.get(idx) {
                        if self.disabled_providers.contains(name) {
                            self.disabled_providers.remove(name);
                        } else {
                            self.disabled_providers.insert(name.clone());
                        }
                    }
                    ProvidersAction::None
                }
                ProvidersTabFocus::PresetProvider(idx) => {
                    if let Some((name, _, _, _, _)) = self.preset_api_key_providers().get(idx) {
                        if self.disabled_providers.contains(name) {
                            self.disabled_providers.remove(name);
                        } else {
                            self.disabled_providers.insert(name.clone());
                        }
                    }
                    ProvidersAction::None
                }
                _ => ProvidersAction::None,
            },
            SettingsTab::Models => {
                self.activate_models_focus();
                ProvidersAction::None
            }
            SettingsTab::Local => {
                match self.local_focus {
                    LocalFocus::Enabled => {
                        self.local_enabled = !self.local_enabled;
                    }
                    LocalFocus::ServerType => {
                        self.local_server_type = next_local_server_type(self.local_server_type);
                    }
                    _ => {}
                }
                ProvidersAction::None
            }
            SettingsTab::Mcp => {
                if let Some(server) = self.mcp_servers.get_mut(self.mcp_focus) {
                    server.enabled = !server.enabled;
                }
                ProvidersAction::None
            }
            SettingsTab::Keybindings => ProvidersAction::None,
        }
    }

    pub fn toggle_general_dropdown(&mut self, dropdown: GeneralDropdown) {
        if self.general_dropdown_open == Some(dropdown) {
            self.general_dropdown_open = None;
        } else {
            self.general_dropdown_open = Some(dropdown);
        }
    }

    pub fn close_general_dropdown(&mut self) {
        self.general_dropdown_open = None;
    }

    pub fn check_oauth_tokens(&mut self) -> Vec<String> {
        let mut found = Vec::new();

        for provider_name in self
            .oauth_providers()
            .into_iter()
            .map(|(name, _, _, _, _)| name)
        {
            if crate::llm::auth::read_oauth_token(&provider_name).is_some() {
                self.saved_keys.retain(|(name, _)| name != &provider_name);
                self.saved_keys
                    .push((provider_name.to_string(), "OAuth token found".to_string()));
                found.push(provider_name.to_string());
            }
        }

        found
    }

    pub fn provider_endpoint(&self, provider_name: &str) -> Option<String> {
        self.db_providers
            .iter()
            .find(|(name, _, _, _, _)| name == provider_name)
            .map(|(_, endpoint, _, _, _)| endpoint.clone())
    }

    fn tab_index(&self) -> usize {
        match self.active_tab {
            SettingsTab::General => 0,
            SettingsTab::Keybindings => 1,
            SettingsTab::Providers => 2,
            SettingsTab::Models => 3,
            SettingsTab::Local => 4,
            SettingsTab::Mcp => 5,
        }
    }

    fn tab_from_index(i: usize) -> SettingsTab {
        match i {
            0 => SettingsTab::General,
            1 => SettingsTab::Keybindings,
            2 => SettingsTab::Providers,
            3 => SettingsTab::Models,
            4 => SettingsTab::Local,
            5 => SettingsTab::Mcp,
            _ => SettingsTab::General,
        }
    }

    pub fn tab_hit_areas(&self, area: Rect) -> Vec<Rect> {
        let inner = Rect::new(area.x + 1, area.y + 1, area.width - 2, area.height - 2);
        let tab_row = Rect::new(inner.x, inner.y, inner.width, 3);
        let tabs = [
            "General",
            "Keybindings",
            "Providers",
            "Models",
            "Local",
            "MCP",
        ];
        let mut areas = Vec::new();
        let mut current_x = tab_row.x;
        for t in tabs {
            let width = t.len() as u16;
            areas.push(Rect::new(current_x, tab_row.y, width, tab_row.height));
            current_x += width + 1;
        }
        areas
    }

    pub fn popup_area(area: Rect) -> Rect {
        Self::centered_rect(65, 75, area)
    }

    fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
        let popup_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage((100 - percent_y) / 2),
                Constraint::Percentage(percent_y),
                Constraint::Percentage((100 - percent_y) / 2),
            ])
            .split(r);

        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage((100 - percent_x) / 2),
                Constraint::Percentage(percent_x),
                Constraint::Percentage((100 - percent_x) / 2),
            ])
            .split(popup_layout[1])[1]
    }

    fn centered_rect_in(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
        let popup_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage((100 - percent_y) / 2),
                Constraint::Percentage(percent_y),
                Constraint::Percentage((100 - percent_y) / 2),
            ])
            .split(r);

        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage((100 - percent_x) / 2),
                Constraint::Percentage(percent_x),
                Constraint::Percentage((100 - percent_x) / 2),
            ])
            .split(popup_layout[1])[1]
    }

    fn backend_label(value: &str) -> String {
        match value {
            "openai" => "OpenAI".to_string(),
            "anthropic" => "Anthropic".to_string(),
            "gemini" => "Gemini".to_string(),
            "ollama" => "Ollama".to_string(),
            "openai-responses" => "OpenAI Responses".to_string(),
            "alibaba" => "Alibaba".to_string(),
            _ => value.to_string(),
        }
    }

    pub fn move_providers_tab_focus(&mut self, forward: bool) {
        if self.providers_dropdown_open.is_some() {
            if forward {
                self.providers_dropdown_down();
            } else {
                self.providers_dropdown_up();
            }
            return;
        }
        let order = [
            ProvidersTabFocus::DefaultProvider,
            ProvidersTabFocus::DefaultModel,
            ProvidersTabFocus::UseEnvToggle,
            ProvidersTabFocus::AddProviderButton,
            ProvidersTabFocus::EditProvidersButton,
            ProvidersTabFocus::ReloadModelsButton,
        ];
        let current_idx = order.iter().position(|f| *f == self.providers_tab_focus);
        let new_focus = match current_idx {
            Some(idx) if forward => {
                let next = (idx + 1) % order.len();
                order[next]
            }
            Some(idx) => {
                let prev = if idx == 0 { order.len() - 1 } else { idx - 1 };
                order[prev]
            }
            None => ProvidersTabFocus::DefaultProvider,
        };
        self.providers_tab_focus = new_focus;
    }

    pub fn next_focus(&mut self) {
        match self.active_tab {
            SettingsTab::General => {
                self.close_general_dropdown();
                self.general_focus = match self.general_focus {
                    GeneralFocus::Theme => GeneralFocus::UserAlignment,
                    GeneralFocus::UserAlignment => GeneralFocus::AiAlignment,
                    GeneralFocus::AiAlignment => GeneralFocus::ArtifactSaveDir,
                    GeneralFocus::ArtifactSaveDir => GeneralFocus::ShowSelector,
                    GeneralFocus::ShowSelector => GeneralFocus::ShowChatScrollbar,
                    GeneralFocus::ShowChatScrollbar => GeneralFocus::CollapseThinking,
                    GeneralFocus::CollapseThinking => GeneralFocus::KittyEnhancedText,
                    GeneralFocus::KittyEnhancedText => GeneralFocus::KittyTextScale,
                    GeneralFocus::KittyTextScale => GeneralFocus::WebSearchEnabled,
                    GeneralFocus::WebSearchEnabled => GeneralFocus::QuitConfirmation,
                    GeneralFocus::QuitConfirmation => GeneralFocus::Theme,
                };
            }
            SettingsTab::Providers => {
                if let Some(form) = self.add_provider_popup.as_mut() {
                    Self::cycle_form_focus(form, true);
                } else if let Some(popup) = self.edit_providers_popup.as_mut() {
                    Self::cycle_edit_popup_focus(popup, self.providers_tab_list.len(), true);
                } else {
                    self.move_providers_tab_focus(true);
                }
            }
            SettingsTab::Models => self.move_models_tab_focus(true),
            SettingsTab::Local => {
                self.local_focus = match self.local_focus {
                    LocalFocus::Enabled => LocalFocus::Host,
                    LocalFocus::Host => LocalFocus::Port,
                    LocalFocus::Port => LocalFocus::ServerType,
                    LocalFocus::ServerType => LocalFocus::SelectedModel,
                    LocalFocus::SelectedModel => LocalFocus::ModelDirectory,
                    LocalFocus::ModelDirectory => LocalFocus::HealthInterval,
                    LocalFocus::HealthInterval => LocalFocus::ConnectTimeout,
                    LocalFocus::ConnectTimeout => LocalFocus::RequestTimeout,
                    LocalFocus::RequestTimeout => LocalFocus::ApiTokenEnv,
                    LocalFocus::ApiTokenEnv => LocalFocus::Enabled,
                };
            }
            SettingsTab::Mcp => {
                if !self.mcp_servers.is_empty() {
                    self.mcp_focus = (self.mcp_focus + 1) % self.mcp_servers.len();
                }
            }
            SettingsTab::Keybindings => {}
        }
    }

    fn activate_preset_key_popup(&mut self) -> ProvidersAction {
        let Some(popup) = &self.preset_key_popup else {
            return ProvidersAction::None;
        };

        match self.providers_tab_focus {
            ProvidersTabFocus::PopupSaveButton if popup.can_submit() => {
                let provider_name = popup.provider_name.clone();
                let api_key = popup.api_key.trim().to_string();
                self.preset_key_popup = None;
                ProvidersAction::SavePresetKey {
                    provider_name,
                    api_key,
                }
            }
            ProvidersTabFocus::PopupCancelButton => {
                self.preset_key_popup = None;
                ProvidersAction::None
            }
            _ => ProvidersAction::None,
        }
    }

    fn handle_preset_key_popup_click(&mut self, pos: Position) -> ProvidersAction {
        if let Some(area) = self.providers_tab_hit_areas.popup_api_key {
            if area.contains(pos) {
                self.providers_tab_focus = ProvidersTabFocus::PopupApiKey;
                return ProvidersAction::None;
            }
        }
        if let Some(area) = self.providers_tab_hit_areas.popup_save {
            if area.contains(pos) {
                self.providers_tab_focus = ProvidersTabFocus::PopupSaveButton;
                return self.activate_preset_key_popup();
            }
        }
        if let Some(area) = self.providers_tab_hit_areas.popup_cancel {
            if area.contains(pos) {
                self.providers_tab_focus = ProvidersTabFocus::PopupCancelButton;
                self.preset_key_popup = None;
                return ProvidersAction::None;
            }
        }
        ProvidersAction::None
    }
}

#[derive(Debug, Clone, Copy)]
enum FormClickAction {
    Noop,
    Activate,
}

fn mask_key(key: &str) -> String {
    if key.is_empty() {
        String::new()
    } else if key.len() > 8 {
        format!("{}...{}", &key[..4], &key[key.len() - 4..])
    } else {
        "••••".to_string()
    }
}

fn next_local_server_type(current: LocalServerType) -> LocalServerType {
    match current {
        LocalServerType::Auto => LocalServerType::Ollama,
        LocalServerType::Ollama => LocalServerType::LlamaCpp,
        LocalServerType::LlamaCpp => LocalServerType::LmStudio,
        LocalServerType::LmStudio => LocalServerType::OpenAiCompat,
        LocalServerType::OpenAiCompat => LocalServerType::Auto,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn popup_with_mcps(count: usize) -> SettingsPopup {
        SettingsPopup::new(SettingsPopupInit {
            default_provider: String::new(),
            default_model: String::new(),
            small_model: String::new(),
            use_env_keys: false,
            saved_keys: vec![],
            theme: "system".to_string(),
            user_alignment: TextAlignment::Left,
            ai_alignment: TextAlignment::Left,
            markdown_mode: MarkdownMode::Full,
            artifact_save_dir: String::new(),
            available_models: vec![],
            db_providers: vec![],
            show_selector: true,
            show_chat_scrollbar: true,
            collapse_thinking: true,
            kitty_enhanced_text: false,
            kitty_text_max_scale: 3,
            web_search_enabled: false,
            quit_confirmation: true,
            local_enabled: false,
            local_host: String::new(),
            local_port: String::new(),
            local_server_type: LocalServerType::Auto,
            local_selected_model: String::new(),
            local_model_directory: String::new(),
            local_health_interval_seconds: String::new(),
            local_connect_timeout_ms: String::new(),
            local_request_timeout_ms: String::new(),
            local_api_token_env: String::new(),
            detected_local_server: None,
            providers_tab_list: vec![],
            models_provider: String::new(),
            models_available_models: vec![],
            mcp_servers: (0..count)
                .map(|idx| McpServerConfig {
                    name: format!("server-{idx}"),
                    enabled: idx % 2 == 0,
                    ..McpServerConfig::default()
                })
                .collect(),
        })
    }

    #[test]
    fn mcp_settings_render_and_hit_rows_fit_80_by_24() {
        // Given
        let mut popup = popup_with_mcps(20);
        popup.active_tab = SettingsTab::Mcp;
        popup.mcp_focus = 15;
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");

        // When
        terminal
            .draw(|frame| popup.render(frame))
            .expect("render MCP settings");

        // Then
        assert!(popup
            .mcp_hit_areas
            .rows
            .iter()
            .any(|(idx, area)| *idx == 15 && area.height == 1));
        assert!(popup
            .mcp_hit_areas
            .rows
            .iter()
            .all(|(_, area)| area.right() <= 80 && area.bottom() <= 24));
        let rendered: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(rendered.contains("MCP"));
        assert!(rendered.contains("server-15"));
    }

    #[test]
    fn tab_hit_areas_include_models_before_local_and_mcp() {
        // Given
        let popup = popup_with_mcps(0);
        let area = SettingsPopup::popup_area(Rect::new(0, 0, 100, 30));

        // When
        let hit_areas = popup.tab_hit_areas(area);

        // Then
        assert_eq!(hit_areas.len(), 6);
        assert!(hit_areas[3].x < hit_areas[4].x);
        assert!(hit_areas[4].x < hit_areas[5].x);
    }

    #[test]
    fn activating_preset_provider_row_toggles_provider_enabled_state() {
        // Given
        let mut popup = popup_with_mcps(0);
        popup.active_tab = SettingsTab::Providers;
        popup.db_providers = vec![(
            "OpenAI".to_string(),
            "https://api.openai.com/v1".to_string(),
            "OPENAI_API_KEY".to_string(),
            "openai".to_string(),
            "api_key".to_string(),
        )];
        popup.providers_tab_focus = ProvidersTabFocus::PresetProvider(0);

        // When
        let action = popup.activate_focus();

        // Then
        assert!(matches!(action, ProvidersAction::None));
        assert!(popup.disabled_providers.contains("OpenAI"));
        assert!(popup.preset_key_popup.is_none());
    }

    #[test]
    fn keybindings_help_keeps_vault_route_and_omits_local_route() {
        // Given
        let mut popup = popup_with_mcps(0);
        popup.active_tab = SettingsTab::Keybindings;
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("test terminal");

        // When
        terminal
            .draw(|frame| popup.render(frame))
            .expect("render keybindings");
        let rendered: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();

        // Then
        assert!(rendered.contains("/vault <query>"));
        assert!(!rendered.contains("/local"));
    }

    #[test]
    fn providers_settings_hide_small_model_controls() {
        let mut popup = popup_with_mcps(0);
        popup.active_tab = SettingsTab::Providers;
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("test terminal");

        terminal
            .draw(|frame| popup.render(frame))
            .expect("render providers");
        let rendered: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();

        assert!(!rendered.contains("Small Provider"));
        assert!(!rendered.contains("Small Model"));
    }

    #[test]
    fn models_tab_renders_provider_and_model_rows() {
        let mut popup = popup_with_mcps(0);
        popup.active_tab = SettingsTab::Models;
        popup.models_provider = "Codex".to_string();
        popup.models_available_models = vec![
            ModelInfo {
                id: "gpt-5.5".to_string(),
                input_price: None,
                output_price: None,
                context_window: Some(400_000),
            },
            ModelInfo {
                id: "gpt-5.4".to_string(),
                input_price: None,
                output_price: None,
                context_window: Some(256_000),
            },
        ];
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("test terminal");

        terminal
            .draw(|frame| popup.render(frame))
            .expect("render models tab");
        let rendered: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();

        assert!(rendered.contains("Models"));
        assert!(rendered.contains("Models For Selected Provider"));
        assert!(rendered.contains("gpt-5.5"));
        assert!(rendered.contains("gpt-5.4"));
    }
}
