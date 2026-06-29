use crate::storage::Storage;

pub(crate) fn read_provider_api_key(
    name: &str,
    env_var: &str,
    storage: &Storage,
) -> Option<String> {
    let endpoint = storage.get_provider_endpoint(name).ok().flatten()?;
    if !trusted_provider_endpoint(name, &endpoint) {
        return None;
    }

    std::env::var(env_var)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| read_env_file(env_var))
        .or_else(|| {
            if is_oauth_provider(name) {
                None
            } else {
                let config = crate::config::AppConfig::load().ok()?;
                crate::config::KeyStore::get(&config, &canonical_provider_name(name))
                    .ok()
                    .flatten()
                    .filter(|value| !value.trim().is_empty())
            }
        })
        .or_else(|| read_oauth_token(name))
}

pub(crate) fn read_oauth_token(name: &str) -> Option<String> {
    let home = dirs::home_dir()?;
    let provider = canonical_provider_name(name);
    let paths = match provider.as_str() {
        "Codex" => vec![
            home.join(".codex").join("auth.json"),
            home.join(".codex.json"),
        ],
        "Gemini" => vec![
            home.join(".gemini.json"),
            home.join(".gemini").join("oauth_creds.json"),
            home.join(".config").join("gemini").join("oauth_creds.json"),
        ],
        _ => return None,
    };

    paths.into_iter().find_map(|path| {
        let content = std::fs::read_to_string(path).ok()?;
        let value = serde_json::from_str::<serde_json::Value>(&content).ok()?;
        match provider.as_str() {
            "Codex" => value
                .pointer("/tokens/access_token")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|token| !token.is_empty())
                .map(str::to_string),
            "Gemini" => value
                .get("access_token")
                .and_then(serde_json::Value::as_str)
                .or_else(|| {
                    value
                        .pointer("/tokens/access_token")
                        .and_then(serde_json::Value::as_str)
                })
                .map(str::trim)
                .filter(|token| !token.is_empty())
                .map(str::to_string),
            _ => None,
        }
    })
}

pub(crate) fn is_oauth_provider(name: &str) -> bool {
    matches!(canonical_provider_name(name).as_str(), "Codex" | "Gemini")
}

pub(crate) fn trusted_provider_endpoint(name: &str, endpoint: &str) -> bool {
    let provider = canonical_provider_name(name);
    match provider.as_str() {
        "Codex" => endpoint == "https://api.openai.com/v1",
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

fn read_env_file(env_var: &str) -> Option<String> {
    let home = dirs::home_dir();
    let paths = [
        std::path::PathBuf::from(".env"),
        home.unwrap_or_default().join(".env"),
    ];

    paths.into_iter().find_map(|path| {
        let content = std::fs::read_to_string(path).ok()?;
        content.lines().find_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }

            line.strip_prefix(&format!("{env_var}="))
                .map(|value| {
                    value
                        .trim()
                        .trim_matches('"')
                        .trim_matches('\'')
                        .to_string()
                })
                .filter(|value| !value.is_empty())
        })
    })
}

pub(crate) fn redact_secrets(text: &str) -> String {
    text.split_whitespace()
        .map(|word| {
            if looks_like_secret(word) || looks_like_sensitive_assignment(word) {
                "[redacted]"
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn looks_like_secret(word: &str) -> bool {
    let trimmed =
        word.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_' && c != '.');
    trimmed.starts_with("Bearer")
        || trimmed.starts_with("sk-")
        || trimmed.starts_with("sk_")
        || trimmed.starts_with("ya29.")
        || trimmed.starts_with("eyJ")
        || trimmed.contains("sk-")
        || trimmed.contains("sk_")
        || trimmed.contains("ya29.")
        || trimmed.contains("eyJ")
        || (trimmed.len() >= 48
            && trimmed
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'))
}

fn looks_like_sensitive_assignment(word: &str) -> bool {
    let lower = word.to_ascii_lowercase();
    let has_sensitive_key = [
        "api_key",
        "apikey",
        "access_token",
        "authorization",
        "bearer",
        "token",
        "secret",
    ]
    .iter()
    .any(|key| lower.contains(key));
    has_sensitive_key
        && (word.contains('=') || word.contains(':') || word.contains('?') || word.contains('&'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn env_lock() -> &'static Mutex<()> {
        crate::test_support::env_lock()
    }

    fn unique_temp_dir(label: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("tcui-{label}-{}-{nanos}", std::process::id()))
    }

    #[test]
    fn redacts_embedded_key_value_and_jsonish_secrets() {
        let text = r#"api_key=sk-test url=https://x.test?a=1&access_token=ya29.secret {"authorization":"Bearer eyJabc"}"#;
        let redacted = redact_secrets(text);
        assert!(!redacted.contains("sk-test"));
        assert!(!redacted.contains("ya29.secret"));
        assert!(!redacted.contains("eyJabc"));
    }

    #[test]
    fn reads_gemini_token_from_dot_gemini_directory() {
        let _guard = env_lock().lock().expect("env lock poisoned");
        let root = unique_temp_dir("gemini-oauth");
        let gemini_dir = root.join(".gemini");
        std::fs::create_dir_all(&gemini_dir).expect("create gemini dir");
        std::fs::write(
            gemini_dir.join("oauth_creds.json"),
            r#"{"access_token":"test-token"}"#,
        )
        .expect("write oauth creds");
        std::env::set_var("HOME", &root);

        let token = read_oauth_token("Gemini");

        assert_eq!(token.as_deref(), Some("test-token"));

        std::env::remove_var("HOME");
        std::fs::remove_dir_all(root).expect("cleanup temp dir");
    }
}
