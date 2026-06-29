pub mod app_config;
pub mod key_store;
pub mod mcp_config;
pub mod search;
pub mod soul;

pub use app_config::{
    AppConfig, LocalInferenceConfig, LocalServerType, ProviderConfig, LOCAL_PROVIDER_NAME,
};
pub use key_store::KeyStore;
pub use mcp_config::McpServerConfig;
pub use search::WebSearchConfig;

#[cfg(test)]
mod tests;
