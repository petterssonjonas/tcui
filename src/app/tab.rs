#![allow(dead_code)]
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tab {
    pub id: Option<i64>,
    pub name: String,
    pub provider: String,
    pub endpoint: Option<String>,
    pub model: String,
    pub api_key_ref: Option<String>,
    pub soul_name: Option<String>,
    pub agent_name: Option<String>,
    pub mcp_servers: Option<String>,
    pub tab_order: i64,
}

impl Tab {
    pub fn new(name: String, provider: String, model: String) -> Self {
        Self {
            id: None,
            name,
            provider,
            endpoint: None,
            model,
            api_key_ref: None,
            soul_name: None,
            agent_name: None,
            mcp_servers: None,
            tab_order: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum TabType {
    Chat,
    Obsidian,
    Settings,
    Logs,
}
