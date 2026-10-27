#![allow(dead_code)]
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: Option<i64>,
    pub conversation_id: i64,
    pub role: String,
    pub content: String,
    pub thinking_content: Option<String>,
    pub tool_calls: Option<String>,
    pub tool_result: Option<String>,
    pub tool_source: Option<String>,
    pub images: Option<String>,
    pub diff_data: Option<String>,
    pub token_count: Option<i64>,
}

impl Message {
    pub fn new(conversation_id: i64, role: String, content: String) -> Self {
        Self {
            id: None,
            conversation_id,
            role,
            content,
            thinking_content: None,
            tool_calls: None,
            tool_result: None,
            tool_source: None,
            images: None,
            diff_data: None,
            token_count: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum MessageRole {
    User,
    Assistant,
    System,
    Tool,
}

impl From<&str> for MessageRole {
    fn from(s: &str) -> Self {
        match s {
            "user" => MessageRole::User,
            "assistant" => MessageRole::Assistant,
            "system" => MessageRole::System,
            "tool" => MessageRole::Tool,
            _ => MessageRole::User,
        }
    }
}

impl std::fmt::Display for MessageRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MessageRole::User => write!(f, "user"),
            MessageRole::Assistant => write!(f, "assistant"),
            MessageRole::System => write!(f, "system"),
            MessageRole::Tool => write!(f, "tool"),
        }
    }
}
