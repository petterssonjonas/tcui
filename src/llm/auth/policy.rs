pub(crate) fn canonical_provider_name(name: &str) -> String {
    match name.trim().to_lowercase().as_str() {
        "google ai" | "google" | "gemini" => "Gemini".to_string(),
        "openai" => "OpenAI".to_string(),
        "codex" | "openai codex" => "Codex".to_string(),
        "zen" | "opencode zen" => "OpenCode Zen".to_string(),
        "opencode go" => "OpenCode Go".to_string(),
        "openrouter" => "OpenRouter".to_string(),
        "kilo gateway" => "Kilo Gateway".to_string(),
        "berget.ai" => "Berget.ai".to_string(),
        other => other
            .split_whitespace()
            .map(|part| {
                let mut chars = part.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

pub(crate) fn is_oauth_provider(name: &str) -> bool {
    matches!(canonical_provider_name(name).as_str(), "Codex" | "Gemini")
}

pub(crate) fn trusted_provider_endpoint(name: &str, endpoint: &str) -> bool {
    let provider = canonical_provider_name(name);
    match provider.as_str() {
        "Codex" => endpoint == "https://chatgpt.com/backend-api/codex",
        "Gemini" | "Google Ai" => {
            endpoint.starts_with("https://generativelanguage.googleapis.com/")
        }
        "OpenAI" => endpoint == "https://api.openai.com/v1",
        "Anthropic" => endpoint == "https://api.anthropic.com/v1",
        "Ollama" => endpoint == "http://localhost:11434/v1",
        "OpenRouter" => endpoint == "https://openrouter.ai/api/v1",
        "Kilo Gateway" => endpoint == "https://api.kilo.ai/api/gateway",
        "Mistral" => endpoint == "https://api.mistral.ai/v1",
        "Groq" => endpoint == "https://api.groq.com/openai/v1",
        "Berget.ai" => endpoint == "https://api.berget.ai/v1",
        "OpenCode Go" => endpoint == "https://opencode.ai/zen/go/v1",
        "OpenCode Zen" => endpoint == "https://opencode.ai/zen/v1",
        _ => true,
    }
}
