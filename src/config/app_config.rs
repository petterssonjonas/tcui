#![allow(dead_code)]
use color_eyre::Result;
use serde::{Deserialize, Deserializer, Serialize};
use std::path::PathBuf;

use crate::config::{McpServerConfig, WebSearchConfig};

pub const LOCAL_PROVIDER_NAME: &str = "Local Inference";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextAlignment {
    #[default]
    Left,
    Middle,
    Right,
}

impl std::fmt::Display for TextAlignment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TextAlignment::Left => write!(f, "left"),
            TextAlignment::Middle => write!(f, "middle"),
            TextAlignment::Right => write!(f, "right"),
        }
    }
}

impl std::str::FromStr for TextAlignment {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "left" => Ok(TextAlignment::Left),
            "middle" | "center" => Ok(TextAlignment::Middle),
            "right" => Ok(TextAlignment::Right),
            _ => Err(format!("unknown alignment: {}", s)),
        }
    }
}

impl TextAlignment {
    pub fn next(self) -> Self {
        match self {
            TextAlignment::Left => TextAlignment::Middle,
            TextAlignment::Middle => TextAlignment::Right,
            TextAlignment::Right => TextAlignment::Left,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            TextAlignment::Left => TextAlignment::Right,
            TextAlignment::Middle => TextAlignment::Left,
            TextAlignment::Right => TextAlignment::Middle,
        }
    }

    pub fn as_ratatui(&self) -> ratatui::layout::Alignment {
        match self {
            TextAlignment::Left => ratatui::layout::Alignment::Left,
            TextAlignment::Middle => ratatui::layout::Alignment::Center,
            TextAlignment::Right => ratatui::layout::Alignment::Right,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MarkdownMode {
    #[default]
    Off,
    Full,
    Textual,
}

impl std::fmt::Display for MarkdownMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MarkdownMode::Off => write!(f, "off"),
            MarkdownMode::Full => write!(f, "full"),
            MarkdownMode::Textual => write!(f, "textual"),
        }
    }
}

impl std::str::FromStr for MarkdownMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "off" => Ok(MarkdownMode::Off),
            "full" => Ok(MarkdownMode::Full),
            "textual" => Ok(MarkdownMode::Textual),
            _ => Err(format!("unknown markdown mode: {}", s)),
        }
    }
}

impl MarkdownMode {
    pub fn next(self) -> Self {
        match self {
            MarkdownMode::Off => MarkdownMode::Full,
            MarkdownMode::Full => MarkdownMode::Textual,
            MarkdownMode::Textual => MarkdownMode::Off,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            MarkdownMode::Off => MarkdownMode::Textual,
            MarkdownMode::Full => MarkdownMode::Off,
            MarkdownMode::Textual => MarkdownMode::Full,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            MarkdownMode::Off => "Off",
            MarkdownMode::Full => "Full",
            MarkdownMode::Textual => "Textual",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HeadingDownscale {
    #[default]
    None,
    One,
    Two,
}

impl HeadingDownscale {
    pub fn label(self) -> &'static str {
        match self {
            HeadingDownscale::None => "Original",
            HeadingDownscale::One => "Down one level",
            HeadingDownscale::Two => "Down two levels",
        }
    }

    const fn from_legacy_scale(scale: u8) -> Self {
        match scale {
            0 | 1 => HeadingDownscale::Two,
            2 => HeadingDownscale::One,
            _ => HeadingDownscale::None,
        }
    }
}

impl<'de> Deserialize<'de> for HeadingDownscale {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum HeadingDownscaleValue {
            Name(String),
            Scale(u8),
        }

        match HeadingDownscaleValue::deserialize(deserializer)? {
            HeadingDownscaleValue::Name(name) => match name.trim().to_ascii_lowercase().as_str() {
                "none" | "original" => Ok(HeadingDownscale::None),
                "one" | "down_one_level" | "down-one-level" => Ok(HeadingDownscale::One),
                "two" | "down_two_levels" | "down-two-levels" => Ok(HeadingDownscale::Two),
                other => Err(serde::de::Error::custom(format!(
                    "unknown heading downscale: {other}"
                ))),
            },
            HeadingDownscaleValue::Scale(scale) => Ok(HeadingDownscale::from_legacy_scale(scale)),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalServerType {
    #[default]
    Auto,
    Ollama,
    LlamaCpp,
    LmStudio,
    OpenAiCompat,
}

impl LocalServerType {
    pub fn label(self) -> &'static str {
        match self {
            LocalServerType::Auto => "Auto",
            LocalServerType::Ollama => "Ollama",
            LocalServerType::LlamaCpp => "llama.cpp",
            LocalServerType::LmStudio => "LM Studio",
            LocalServerType::OpenAiCompat => "OpenAI-compatible",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LocalInferenceConfig {
    pub enabled: bool,
    pub scheme: String,
    pub host: String,
    pub port: u16,
    pub server_type: LocalServerType,
    pub selected_model: String,
    pub model_directory: Option<String>,
    pub health_check_interval_seconds: u64,
    pub connect_timeout_ms: u64,
    pub request_timeout_ms: u64,
    pub api_token_env: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NtfyConfig {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub topic: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationConfig {
    pub desktop: bool,
    pub ntfy: NtfyConfig,
}

impl LocalInferenceConfig {
    pub fn base_url(&self) -> String {
        format!(
            "{}://{}:{}",
            self.scheme.trim(),
            self.host.trim(),
            self.port
        )
    }

    pub fn chat_endpoint(&self) -> String {
        format!("{}/v1", self.base_url().trim_end_matches('/'))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub theme: String,
    pub default_model: String,
    pub small_model: Option<String>,
    pub default_provider: String,
    pub user_alignment: TextAlignment,
    pub ai_alignment: TextAlignment,
    pub markdown_mode: MarkdownMode,
    pub show_selector: bool,
    pub show_chat_scrollbar: bool,
    pub collapse_thinking: bool,
    pub kitty_enhanced_text: bool,
    #[serde(default, alias = "kitty_text_max_scale")]
    pub kitty_heading_downscale: HeadingDownscale,
    pub quit_confirmation: bool,
    pub use_env_keys: bool,
    pub disabled_providers: Vec<String>,
    pub disabled_models: Vec<String>,
    pub key_file: Option<String>,
    pub image_protocol: String,
    pub vault_path: Option<String>,
    pub artifact_save_dir: Option<String>,
    pub notifications: NotificationConfig,
    pub local_inference: LocalInferenceConfig,
    pub web_search: WebSearchConfig,
    #[cfg(feature = "memory")]
    pub memory: crate::memory::MemoryConfig,
    pub providers: Vec<ProviderConfig>,
    pub mcp_servers: Vec<McpServerConfig>,
    pub tab_configs: Vec<TabConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TabConfig {
    pub name: String,
    pub provider: String,
    pub model: String,
    pub endpoint: Option<String>,
    pub soul_name: Option<String>,
    pub agent_name: Option<String>,
    pub mcp_servers: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ProviderConfig {
    pub name: String,
    pub endpoint: String,
    pub env_var: String,
    pub backend_type: String,
    pub auth_type: String,
}

impl AppConfig {
    pub fn load() -> Result<Self> {
        if let Some(path) = Self::load_repo_config_path().filter(|path| path.exists()) {
            let config = Self::load_toml(&path)?;
            Self::bootstrap_xdg_layout(&config)?;
            return Ok(config);
        }
        if let Some(path) = Self::xdg_config_path()?.filter(|path| path.exists()) {
            let config = Self::load_toml(&path)?;
            Self::bootstrap_xdg_layout(&config)?;
            return Ok(config);
        }
        if let Some(path) = Self::legacy_json_path()?.filter(|path| path.exists()) {
            let content = std::fs::read_to_string(&path)?;
            let config: AppConfig = serde_json::from_str(&content)?;
            config.save()?;
            Self::bootstrap_xdg_layout(&config)?;
            return Ok(config);
        }
        let config = Self::default();
        Self::bootstrap_xdg_layout(&config)?;
        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let config_path = Self::write_path()?;
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        } else {
            return Ok(());
        }
        let content = toml::to_string_pretty(self)?;
        std::fs::write(config_path, content)?;
        Ok(())
    }

    pub fn provider_entries(&self) -> Vec<(String, String, String, String, String)> {
        self.providers
            .iter()
            .map(|provider| {
                (
                    provider.name.clone(),
                    provider.endpoint.clone(),
                    provider.env_var.clone(),
                    provider.backend_type.clone(),
                    provider.auth_type.clone(),
                )
            })
            .collect()
    }

    pub fn provider_config(&self, provider_name: &str) -> Option<&ProviderConfig> {
        self.providers.iter().find(|provider| {
            provider.name == provider_name || provider.name.eq_ignore_ascii_case(provider_name)
        })
    }

    fn load_toml(path: &PathBuf) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&content)?)
    }

    fn write_path() -> Result<PathBuf> {
        if let Some(path) = Self::cwd_repo_config_path().filter(|path| path.exists()) {
            return Ok(path);
        }
        Ok(Self::xdg_config_path()?.unwrap_or_else(|| PathBuf::from(".").join("config.toml")))
    }

    fn load_repo_config_path() -> Option<PathBuf> {
        let mut search_roots = Vec::new();
        if let Ok(current_dir) = std::env::current_dir() {
            search_roots.push(current_dir);
        }
        if let Ok(current_exe) = std::env::current_exe() {
            let exe_is_tcui = current_exe
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(|stem| stem == "tcui")
                .unwrap_or(false);
            if exe_is_tcui {
                if let Some(parent) = current_exe.parent() {
                    search_roots.push(parent.to_path_buf());
                }
            }
        }
        Self::repo_config_path_from_roots(search_roots)
    }

    fn cwd_repo_config_path() -> Option<PathBuf> {
        std::env::current_dir()
            .ok()
            .and_then(|current_dir| Self::repo_config_path_from_roots([current_dir]))
    }

    fn repo_config_path_from_roots<I>(roots: I) -> Option<PathBuf>
    where
        I: IntoIterator<Item = PathBuf>,
    {
        for root in roots {
            for dir in root.ancestors() {
                let candidate = dir.join("config.toml");
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
        None
    }

    fn xdg_config_path() -> Result<Option<PathBuf>> {
        let dir = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        Ok(Some(dir.join("tcui").join("config.toml")))
    }

    fn legacy_json_path() -> Result<Option<PathBuf>> {
        let dir = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        Ok(Some(dir.join("tcui").join("config.json")))
    }

    fn xdg_config_root() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("tcui")
    }

    fn xdg_data_root() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("tcui")
    }

    fn bootstrap_xdg_layout(config: &Self) -> Result<()> {
        let config_root = Self::xdg_config_root();
        std::fs::create_dir_all(&config_root)?;
        for dir in ["skills", "souls", "themes", "mcp"] {
            std::fs::create_dir_all(config_root.join(dir))?;
        }

        if let Some(config_path) = Self::xdg_config_path()? {
            if !config_path.exists() {
                std::fs::write(&config_path, toml::to_string_pretty(config)?)?;
            }
        }

        std::fs::create_dir_all(Self::xdg_data_root())?;
        crate::config::key_store::KeyStore::save_keys(config, &[])?;
        Ok(())
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            theme: "system".to_string(),
            default_model: "gpt-4o".to_string(),
            small_model: None,
            default_provider: "openai".to_string(),
            user_alignment: TextAlignment::Right,
            ai_alignment: TextAlignment::Left,
            markdown_mode: MarkdownMode::Full,
            show_selector: true,
            show_chat_scrollbar: true,
            collapse_thinking: true,
            kitty_enhanced_text: true,
            kitty_heading_downscale: HeadingDownscale::None,
            quit_confirmation: true,
            use_env_keys: false,
            disabled_providers: Vec::new(),
            disabled_models: Vec::new(),
            key_file: None,
            image_protocol: "auto".to_string(),
            vault_path: None,
            artifact_save_dir: None,
            notifications: NotificationConfig::default(),
            local_inference: LocalInferenceConfig::default(),
            web_search: WebSearchConfig::default(),
            #[cfg(feature = "memory")]
            memory: crate::memory::MemoryConfig::default(),
            providers: default_providers(),
            mcp_servers: default_mcp_servers(),
            tab_configs: vec![TabConfig {
                name: "Local".to_string(),
                provider: "ollama".to_string(),
                model: "llama3.1".to_string(),
                endpoint: Some("http://localhost:11434/v1".to_string()),
                soul_name: Some("default".to_string()),
                agent_name: None,
                mcp_servers: None,
            }],
        }
    }
}

impl Default for LocalInferenceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            scheme: "http".to_string(),
            host: "127.0.0.1".to_string(),
            port: 11434,
            server_type: LocalServerType::Auto,
            selected_model: String::new(),
            model_directory: None,
            health_check_interval_seconds: 15,
            connect_timeout_ms: 2_500,
            request_timeout_ms: 120_000,
            api_token_env: None,
        }
    }
}

impl Default for NtfyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            host: String::new(),
            port: 80,
            topic: "tcui".to_string(),
        }
    }
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            desktop: true,
            ntfy: NtfyConfig::default(),
        }
    }
}

impl Default for TabConfig {
    fn default() -> Self {
        Self {
            name: "Local".to_string(),
            provider: "ollama".to_string(),
            model: "llama3.1".to_string(),
            endpoint: Some("http://localhost:11434/v1".to_string()),
            soul_name: Some("default".to_string()),
            agent_name: None,
            mcp_servers: None,
        }
    }
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            endpoint: String::new(),
            env_var: String::new(),
            backend_type: "openai".to_string(),
            auth_type: "api_key".to_string(),
        }
    }
}

fn default_providers() -> Vec<ProviderConfig> {
    [
        (
            "OpenAI",
            "https://api.openai.com/v1",
            "OPENAI_API_KEY",
            "openai",
            "api_key",
        ),
        (
            "Anthropic",
            "https://api.anthropic.com/v1",
            "ANTHROPIC_API_KEY",
            "anthropic",
            "api_key",
        ),
        (
            "Google AI",
            "https://generativelanguage.googleapis.com/v1beta/openai",
            "GOOGLE_AI_API_KEY",
            "gemini",
            "api_key",
        ),
        (
            "OpenRouter",
            "https://openrouter.ai/api/v1",
            "OPENROUTER_API_KEY",
            "openrouter",
            "api_key",
        ),
        (
            "Kilo Gateway",
            "https://api.kilo.ai/api/gateway",
            "KILO_API_KEY",
            "kilo",
            "api_key",
        ),
        (
            "Mistral",
            "https://api.mistral.ai/v1",
            "MISTRAL_API_KEY",
            "openai",
            "api_key",
        ),
        (
            "Groq",
            "https://api.groq.com/openai/v1",
            "GROQ_API_KEY",
            "openai",
            "api_key",
        ),
        (
            "Berget.ai",
            "https://api.berget.ai/v1",
            "BERGET_API_KEY",
            "openai",
            "api_key",
        ),
        (
            "OpenCode Go",
            "https://opencode.ai/zen/go/v1",
            "OPENCODE_GO_API_KEY",
            "openai",
            "api_key",
        ),
        (
            "OpenCode Zen",
            "https://opencode.ai/zen/v1",
            "ZEN_API_KEY",
            "openai",
            "api_key",
        ),
        (
            "Gemini",
            "https://generativelanguage.googleapis.com/v1beta/openai",
            "GEMINI_API_KEY",
            "gemini",
            "oauth",
        ),
        (
            "Codex",
            "https://api.openai.com/v1",
            "CODEX_API_KEY",
            "openai",
            "oauth",
        ),
    ]
    .into_iter()
    .map(
        |(name, endpoint, env_var, backend_type, auth_type)| ProviderConfig {
            name: name.to_string(),
            endpoint: endpoint.to_string(),
            env_var: env_var.to_string(),
            backend_type: backend_type.to_string(),
            auth_type: auth_type.to_string(),
        },
    )
    .collect()
}

fn default_mcp_servers() -> Vec<McpServerConfig> {
    vec![
        McpServerConfig {
            name: "Exa".to_string(),
            transport: "streamable_http".to_string(),
            url: Some("https://mcp.exa.ai/mcp".to_string()),
            enabled: false,
            ..McpServerConfig::default()
        },
        McpServerConfig {
            name: "Tavily".to_string(),
            transport: "streamable_http".to_string(),
            url: Some("https://mcp.tavily.com/mcp/".to_string()),
            enabled: false,
            ..McpServerConfig::default()
        },
        McpServerConfig {
            name: "Firecrawl".to_string(),
            transport: "stdio".to_string(),
            command: Some("npx".to_string()),
            args: Some(vec!["-y".to_string(), "firecrawl-mcp".to_string()]),
            enabled: false,
            ..McpServerConfig::default()
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn env_lock() -> &'static Mutex<()> {
        crate::test_support::env_lock()
    }

    fn unique_temp_dir(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("tcui-{label}-{}-{nanos}", std::process::id()))
    }

    #[test]
    fn prefers_repo_toml_over_xdg() {
        let _guard = env_lock().lock().expect("env lock poisoned");
        let root = unique_temp_dir("config-precedence");
        let config_home = root.join("config-home");
        let tcui_dir = config_home.join("tcui");
        std::fs::create_dir_all(&tcui_dir).expect("create config dir");
        std::fs::create_dir_all(&root).expect("create repo dir");
        std::env::set_var("XDG_CONFIG_HOME", &config_home);
        let original_dir = std::env::current_dir().expect("current dir");
        std::env::set_current_dir(&root).expect("set current dir");

        let repo_path = root.join("config.toml");
        std::fs::write(
            &repo_path,
            r#"
theme = "gruvbox"
default_model = "repo-model"
default_provider = "OpenCode Go"
"#,
        )
        .expect("write repo config");

        let xdg_path = tcui_dir.join("config.toml");
        std::fs::write(
            &xdg_path,
            r#"
theme = "solarized-dark"
default_model = "xdg-model"
default_provider = "OpenAI"
"#,
        )
        .expect("write xdg config");

        let config = AppConfig::load().expect("load config");
        assert_eq!(config.theme, "gruvbox");
        assert_eq!(config.default_model, "repo-model");
        assert_eq!(config.default_provider, "OpenCode Go");

        std::env::set_current_dir(original_dir).expect("restore current dir");
        std::fs::remove_dir_all(&root).expect("cleanup temp dir");
        std::env::remove_var("XDG_CONFIG_HOME");
    }

    #[test]
    fn repo_config_path_walks_up_from_nested_directory() {
        let root = unique_temp_dir("config-upward-search");
        let nested = root.join("target").join("debug");
        std::fs::create_dir_all(&nested).expect("create nested dir");
        let repo_path = root.join("config.toml");
        std::fs::write(&repo_path, "theme = \"gruvbox\"\n").expect("write repo config");

        let found = AppConfig::repo_config_path_from_roots([nested]).expect("repo config path");

        assert_eq!(found, repo_path);

        std::fs::remove_dir_all(&root).expect("cleanup temp dir");
    }

    #[test]
    fn legacy_kitty_text_scale_migrates_to_heading_downscale() {
        let config: AppConfig = toml::from_str(
            r#"
theme = "system"
default_model = "gpt-4o"
default_provider = "OpenAI"
kitty_text_max_scale = 2
"#,
        )
        .expect("parse legacy config");

        assert_eq!(config.kitty_heading_downscale, HeadingDownscale::One);

        let clipped: AppConfig = toml::from_str(
            r#"
theme = "system"
default_model = "gpt-4o"
default_provider = "OpenAI"
kitty_text_max_scale = 1
"#,
        )
        .expect("parse legacy clipped config");

        assert_eq!(clipped.kitty_heading_downscale, HeadingDownscale::Two);
    }
}
