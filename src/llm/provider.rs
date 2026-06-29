#![allow(dead_code)]
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum Provider {
    OpenAI,
    Anthropic,
    Gemini,
    Ollama,
    Custom,
}

impl Provider {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "openai" => Provider::OpenAI,
            "anthropic" => Provider::Anthropic,
            "gemini" => Provider::Gemini,
            "ollama" => Provider::Ollama,
            _ => Provider::Custom,
        }
    }

    pub fn endpoint(&self) -> Option<&'static str> {
        match self {
            Provider::OpenAI => Some("https://api.openai.com/v1"),
            Provider::Anthropic => Some("https://api.anthropic.com/v1"),
            Provider::Gemini => Some("https://generativelanguage.googleapis.com/v1"),
            Provider::Ollama => Some("http://localhost:11434/v1"),
            Provider::Custom => None,
        }
    }
}
