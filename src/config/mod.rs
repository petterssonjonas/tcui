pub mod app_config;
pub mod key_store;
pub mod mcp_config;
pub mod search;
pub mod soul;
pub mod tui;

pub use app_config::{
    AppConfig, LocalInferenceConfig, LocalServerType, ProviderConfig, LOCAL_PROVIDER_NAME,
};
pub use key_store::KeyStore;
pub use mcp_config::McpServerConfig;
pub use search::WebSearchConfig;
#[allow(unused_imports)]
pub use tui::{PanelMode, StatusWidgetPlacement, ToastPosition, TuiConfig};

#[cfg(test)]
mod tests;
