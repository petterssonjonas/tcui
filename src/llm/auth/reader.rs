use std::path::PathBuf;

use super::canonical_provider_name;

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
            home.join(".gemini")
                .join("antigravity-cli")
                .join("antigravity-oauth-token"),
            home.join(".gemini")
                .join("antigravity")
                .join("session.json"),
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
                        .pointer("/token/access_token")
                        .and_then(serde_json::Value::as_str)
                })
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

#[cfg(test)]
pub(crate) fn read_codex_account_id() -> Option<String> {
    let home = dirs::home_dir()?;
    [
        home.join(".codex").join("auth.json"),
        home.join(".codex.json"),
    ]
    .into_iter()
    .find_map(|path| {
        let content = std::fs::read_to_string(path).ok()?;
        let value = serde_json::from_str::<serde_json::Value>(&content).ok()?;
        value
            .pointer("/tokens/account_id")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|account_id| !account_id.is_empty())
            .map(str::to_string)
    })
}

pub(super) fn read_env_file(env_var: &str) -> Option<String> {
    let home = dirs::home_dir();
    let paths = [PathBuf::from(".env"), home.unwrap_or_default().join(".env")];

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
