use crate::config::app_config::{HeadingDownscale, LocalServerType, MarkdownMode, TextAlignment};
use crate::config::McpServerConfig;
use ratatui::layout::Rect;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GeneralFocus {
    Theme,
    UserAlignment,
    AiAlignment,
    ArtifactSaveDir,
    VaultPath,
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
    pub vault_path: Option<Rect>,
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

pub(crate) const ALIGNMENT_OPTIONS: &[TextAlignment] = &[
    TextAlignment::Left,
    TextAlignment::Middle,
    TextAlignment::Right,
];
pub(crate) const KITTY_HEADING_SIZE_OPTIONS: &[HeadingDownscale] = &[
    HeadingDownscale::None,
    HeadingDownscale::One,
    HeadingDownscale::Two,
];
pub(crate) const BACKEND_TYPE_OPTIONS: &[&str] = &[
    "openai",
    "anthropic",
    "gemini",
    "ollama",
    "openai-responses",
    "alibaba",
];
pub(crate) const PRESET_PROVIDER_NAMES: &[&str] = &[
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
pub(crate) const SEARCH_KEY_PROVIDERS: &[(&str, &str, &str)] = &[
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
    pub vault_path: String,
    pub available_models: Vec<ModelInfo>,
    pub db_providers: Vec<ProviderEntry>,
    pub show_selector: bool,
    pub show_chat_scrollbar: bool,
    pub collapse_thinking: bool,
    pub kitty_enhanced_text: bool,
    pub kitty_heading_downscale: HeadingDownscale,
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
    pub(super) fn new_add() -> Self {
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

    pub(super) fn new_edit(provider: &EditableProvider, api_key: String) -> Self {
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

    pub(super) fn can_submit(&self) -> bool {
        !self.name.trim().is_empty()
            && !self.endpoint.trim().is_empty()
            && !self.backend_type.trim().is_empty()
            && !self.api_key.trim().is_empty()
    }
}

impl EditProvidersPopupState {
    pub(super) fn new(has_providers: bool) -> Self {
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
    pub(super) fn new(provider_name: String, endpoint: String, api_key: String) -> Self {
        Self {
            provider_name,
            endpoint,
            api_key,
        }
    }

    pub(super) fn can_submit(&self) -> bool {
        !self.api_key.trim().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::settings_tab::{SettingsPopup, SettingsTab};

    fn empty_init() -> SettingsPopupInit {
        SettingsPopupInit {
            default_provider: String::new(),
            default_model: String::new(),
            small_model: String::new(),
            use_env_keys: false,
            saved_keys: Vec::new(),
            theme: "system".to_string(),
            user_alignment: TextAlignment::Left,
            ai_alignment: TextAlignment::Left,
            markdown_mode: MarkdownMode::Full,
            artifact_save_dir: String::new(),
            vault_path: String::new(),
            available_models: Vec::new(),
            db_providers: Vec::new(),
            show_selector: true,
            show_chat_scrollbar: true,
            collapse_thinking: true,
            kitty_enhanced_text: true,
            kitty_heading_downscale: HeadingDownscale::None,
            web_search_enabled: false,
            quit_confirmation: true,
            local_enabled: false,
            local_host: "127.0.0.1".to_string(),
            local_port: "11434".to_string(),
            local_server_type: LocalServerType::Auto,
            local_selected_model: String::new(),
            local_model_directory: String::new(),
            local_health_interval_seconds: "15".to_string(),
            local_connect_timeout_ms: "2500".to_string(),
            local_request_timeout_ms: "120000".to_string(),
            local_api_token_env: String::new(),
            detected_local_server: None,
            providers_tab_list: Vec::new(),
            models_provider: String::new(),
            models_available_models: Vec::new(),
            mcp_servers: Vec::new(),
        }
    }

    #[test]
    fn settings_popup_starts_on_general_tab() {
        let popup = SettingsPopup::new(empty_init());
        assert_eq!(popup.active_tab, SettingsTab::General);
        assert_eq!(popup.general_focus, GeneralFocus::Theme);
    }

    #[test]
    fn provider_form_can_submit_only_when_all_fields_present() {
        let mut form = ProviderFormState::new_add();
        assert!(!form.can_submit());

        form.name = "Test".to_string();
        assert!(!form.can_submit());

        form.endpoint = "https://api.test.com".to_string();
        assert!(!form.can_submit());

        form.backend_type = "openai".to_string();
        assert!(!form.can_submit());

        form.api_key = "sk-123".to_string();
        assert!(form.can_submit());
    }

    #[test]
    fn provider_form_can_submit_rejects_whitespace_only() {
        let mut form = ProviderFormState::new_add();
        form.name = "   ".to_string();
        form.endpoint = "  ".to_string();
        form.backend_type = "  ".to_string();
        form.api_key = "  ".to_string();
        assert!(!form.can_submit());
    }

    #[test]
    fn edit_providers_popup_focus_none_when_empty() {
        let popup = EditProvidersPopupState::new(false);
        assert_eq!(popup.focus, None);
    }

    #[test]
    fn edit_providers_popup_focus_first_when_providers_exist() {
        let popup = EditProvidersPopupState::new(true);
        assert_eq!(popup.focus, Some(EditProvidersFocus::ProviderName(0)));
    }

    #[test]
    fn preset_key_popup_can_submit_requires_api_key() {
        let popup = PresetKeyPopupState::new("Test".to_string(), "".to_string(), "".to_string());
        assert!(!popup.can_submit());

        let popup =
            PresetKeyPopupState::new("Test".to_string(), "".to_string(), "sk-123".to_string());
        assert!(popup.can_submit());
    }
}
