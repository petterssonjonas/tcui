pub(crate) fn canonical_provider_name(name: &str) -> String {
    match name.trim().to_lowercase().as_str() {
        "google ai" | "google" | "gemini" => "Gemini".to_string(),
        "openai" => "OpenAI".to_string(),
        "codex" | "openai codex" | "openai oauth" => "Codex".to_string(),
        "zen" | "opencode zen" => "OpenCode Zen".to_string(),
        "opencode go" => "OpenCode Go".to_string(),
        "openrouter" => "OpenRouter".to_string(),
        "openrouter oauth" => "OpenRouter OAuth".to_string(),
        "anthropic agent sdk" => "Anthropic Agent SDK".to_string(),
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

pub(crate) fn trusted_provider_endpoint(name: &str, endpoint: &str) -> bool {
    match canonical_provider_name(name).as_str() {
        // OAuth grants are bound to their native service, unlike user-saved API keys.
        "Codex" => endpoint.trim_end_matches('/') == "https://chatgpt.com/backend-api/codex",
        "OpenRouter OAuth" => endpoint.trim_end_matches('/') == "https://openrouter.ai/api/v1",
        "Anthropic Agent SDK" => endpoint.trim_end_matches('/') == "https://api.anthropic.com/v1",
        _ => reqwest::Url::parse(endpoint).is_ok_and(|url| {
            matches!(url.scheme(), "http" | "https")
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none()
        }),
    }
}
