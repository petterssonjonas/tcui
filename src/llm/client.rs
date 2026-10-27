#![allow(dead_code)]
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct LlmClient {
    config: Arc<RwLock<ClientConfig>>,
}

#[derive(Clone)]
pub struct ClientConfig {
    pub api_keys: std::collections::HashMap<String, String>,
}

impl LlmClient {
    pub fn new() -> Self {
        Self {
            config: Arc::new(RwLock::new(ClientConfig {
                api_keys: std::collections::HashMap::new(),
            })),
        }
    }

    pub async fn set_api_key(&self, provider: String, key: String) {
        let mut config = self.config.write().await;
        config.api_keys.insert(provider, key);
    }

    pub fn get_config(&self) -> Arc<RwLock<ClientConfig>> {
        self.config.clone()
    }
}

impl Default for LlmClient {
    fn default() -> Self {
        Self::new()
    }
}
