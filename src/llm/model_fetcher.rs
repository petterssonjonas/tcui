use crate::{llm::auth::read_provider_api_key, ui::settings_tab::ModelInfo};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct OpenRouterModel {
    id: String,
    #[allow(dead_code)]
    name: Option<String>,
    pricing: Option<OpenRouterPricing>,
    #[serde(default)]
    context_length: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterPricing {
    prompt: Option<String>,
    completion: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterModelsResponse {
    data: Vec<OpenRouterModel>,
}

#[derive(Debug, Deserialize)]
struct OpenAIModel {
    id: String,
    #[serde(default)]
    context_length: Option<u32>,
    #[serde(default)]
    context_window: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct OpenAIModelsResponse {
    data: Vec<OpenAIModel>,
}

#[derive(Debug, Deserialize)]
struct KiloModel {
    id: String,
    #[serde(default)]
    pricing: Option<KiloPricing>,
    #[serde(default)]
    context_length: Option<u32>,
    #[serde(default)]
    context_window: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct KiloPricing {
    #[serde(default)]
    input: Option<f64>,
    #[serde(default)]
    output: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct KiloModelsResponse {
    data: Vec<KiloModel>,
}

pub async fn fetch_models(
    provider: &str,
    endpoint: &str,
    api_key: Option<&str>,
    backend_type: &str,
) -> Vec<ModelInfo> {
    let url = format!("{}/models", endpoint);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let mut request = client.get(&url);
    if let Some(key) = api_key {
        if !key.is_empty() {
            if !crate::llm::auth::trusted_provider_endpoint(provider, endpoint) {
                return Vec::new();
            }
            request = request.header("Authorization", format!("Bearer {}", key));
        }
    }
    if provider == "OpenRouter" {
        request = request
            .header("HTTP-Referer", "https://github.com/jp/TermChatUI")
            .header("X-Title", "TermChatUI");
    }

    match request.send().await {
        Ok(response) if response.status().is_success() => {
            let text = match response.text().await {
                Ok(t) => t,
                Err(_) => return Vec::new(),
            };

            match backend_type {
                "openrouter" => parse_openrouter_models(&text),
                "kilo" => parse_kilo_models(&text),
                _ => parse_openai_models(&text),
            }
        }
        _ => Vec::new(),
    }
}

fn parse_openai_models(text: &str) -> Vec<ModelInfo> {
    let parsed: Result<OpenAIModelsResponse, _> = serde_json::from_str(text);
    match parsed {
        Ok(resp) => resp
            .data
            .into_iter()
            .map(|m| ModelInfo {
                id: m.id,
                input_price: None,
                output_price: None,
                context_window: m.context_length.or(m.context_window),
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

fn parse_openrouter_models(text: &str) -> Vec<ModelInfo> {
    let parsed: Result<OpenRouterModelsResponse, _> = serde_json::from_str(text);
    match parsed {
        Ok(resp) => resp
            .data
            .into_iter()
            .map(|m| {
                let input_price = m
                    .pricing
                    .as_ref()
                    .and_then(|p| p.prompt.as_ref())
                    .and_then(|s| s.parse::<f64>().ok())
                    .map(|v| v * 1_000_000.0);
                let output_price = m
                    .pricing
                    .as_ref()
                    .and_then(|p| p.completion.as_ref())
                    .and_then(|s| s.parse::<f64>().ok())
                    .map(|v| v * 1_000_000.0);
                ModelInfo {
                    id: m.id,
                    input_price,
                    output_price,
                    context_window: m.context_length,
                }
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

fn parse_kilo_models(text: &str) -> Vec<ModelInfo> {
    let parsed: Result<KiloModelsResponse, _> = serde_json::from_str(text);
    match parsed {
        Ok(resp) => resp
            .data
            .into_iter()
            .map(|m| ModelInfo {
                id: m.id,
                input_price: m.pricing.as_ref().and_then(|p| p.input),
                output_price: m.pricing.as_ref().and_then(|p| p.output),
                context_window: m.context_length.or(m.context_window),
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

pub async fn refresh_provider_models(
    storage: &crate::storage::Storage,
    name: &str,
    endpoint: &str,
    env_var: &str,
    backend_type: &str,
) {
    let api_key = read_provider_api_key(name, env_var, storage);
    let mut models = fetch_models(name, endpoint, api_key.as_deref(), backend_type).await;
    if models.is_empty() {
        models = provider_model_fallback(name);
    }
    if !models.is_empty() {
        let db_models: Vec<crate::storage::db::ModelRow> = models
            .into_iter()
            .map(|m| (m.id, m.input_price, m.output_price, m.context_window))
            .collect();
        let _ = storage.save_models(name, &db_models);
    }
}

pub async fn refresh_all_models(storage: &crate::storage::Storage) {
    let providers = match storage.get_providers() {
        Ok(p) => p,
        Err(_) => return,
    };

    for (name, endpoint, env_var, backend_type, _auth_type) in providers {
        let needs_refresh =
            if let Ok(Some(fetched_at)) = storage.get_provider_models_fetched_at(&name) {
                if let Ok(parsed) =
                    chrono::NaiveDateTime::parse_from_str(&fetched_at, "%Y-%m-%d %H:%M:%S")
                {
                    let now = chrono::Utc::now().naive_utc();
                    (now - parsed).num_days() >= 3
                } else {
                    true
                }
            } else {
                true
            };

        if !needs_refresh {
            continue;
        }

        let api_key = read_provider_api_key(&name, &env_var, storage);

        let mut models = fetch_models(&name, &endpoint, api_key.as_deref(), &backend_type).await;
        if models.is_empty() {
            models = provider_model_fallback(&name);
        }
        if models.is_empty() {
            continue;
        }

        let db_models: Vec<crate::storage::db::ModelRow> = models
            .into_iter()
            .map(|m| (m.id, m.input_price, m.output_price, m.context_window))
            .collect();
        let _ = storage.save_models(&name, &db_models);
    }
}

pub(crate) fn provider_model_fallback(provider: &str) -> Vec<ModelInfo> {
    match crate::llm::auth::canonical_provider_name(provider).as_str() {
        "Codex" => codex_model_fallback(),
        _ => Vec::new(),
    }
}

fn codex_model_fallback() -> Vec<ModelInfo> {
    let mut model_ids = Vec::new();

    if let Some(home) = dirs::home_dir() {
        if let Ok(config) = std::fs::read_to_string(home.join(".codex").join("config.toml")) {
            if let Ok(parsed) = config.parse::<toml::Value>() {
                if let Some(model) = parsed.get("model").and_then(toml::Value::as_str) {
                    let model = model.trim();
                    if !model.is_empty() {
                        model_ids.push(model.to_string());
                    }
                }
            }
        }
    }

    for model in ["gpt-5.5", "gpt-5.4", "gpt-5.4-mini"] {
        if !model_ids.iter().any(|existing| existing == model) {
            model_ids.push(model.to_string());
        }
    }

    model_ids
        .into_iter()
        .map(|id| ModelInfo {
            id,
            input_price: None,
            output_price: None,
            context_window: None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_fallback_includes_current_public_models() {
        let models = provider_model_fallback("Codex");
        let ids: Vec<&str> = models.iter().map(|model| model.id.as_str()).collect();

        assert!(ids.contains(&"gpt-5.5"));
        assert!(ids.contains(&"gpt-5.4"));
        assert!(ids.contains(&"gpt-5.4-mini"));
    }
}
