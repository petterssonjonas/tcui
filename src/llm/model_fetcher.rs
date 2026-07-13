use crate::{
    llm::auth::{Credential, CredentialRequest, resolve_provider_credential},
    ui::ModelInfo,
};
use secrecy::ExposeSecret;
use serde::Deserialize;

const CODEX_MODELS_ENDPOINT: &str = "https://chatgpt.com/backend-api/codex/models";

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
struct CodexModel {
    slug: String,
    #[serde(default)]
    visibility: String,
    #[serde(default = "default_true")]
    supported_in_api: bool,
    #[serde(default)]
    priority: i32,
    #[serde(default)]
    context_window: Option<u32>,
    #[serde(default)]
    default_reasoning_level: Option<String>,
    #[serde(default)]
    supported_reasoning_levels: Vec<CodexReasoningEffort>,
}

#[derive(Debug, Deserialize)]
struct CodexReasoningEffort {
    effort: String,
}

#[derive(Debug, Deserialize)]
struct CodexModelsResponse {
    models: Vec<CodexModel>,
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
    credential: Option<&Credential>,
    backend_type: &str,
) -> Vec<ModelInfo> {
    if crate::llm::auth::canonical_provider_name(provider) == "Codex" {
        return fetch_codex_models(credential).await;
    }

    let url = format!("{}/models", endpoint);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let mut request = client.get(&url);
    if let Some(credential) = credential {
        if !crate::llm::auth::trusted_provider_endpoint(provider, endpoint) {
            return Vec::new();
        }
        request = request.bearer_auth(credential.bearer_token().expose_secret());
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

async fn fetch_codex_models(credential: Option<&Credential>) -> Vec<ModelInfo> {
    let Some(credential) = credential else {
        return Vec::new();
    };
    if !credential.is_codex_oauth() {
        return Vec::new();
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    let mut request = client
        .get(CODEX_MODELS_ENDPOINT)
        .query(&[("client_version", codex_client_version())])
        .bearer_auth(credential.bearer_token().expose_secret());
    if let Some(account_id) = credential.account_id() {
        request = request.header("ChatGPT-Account-ID", account_id);
    }

    match request.send().await {
        Ok(response) if response.status().is_success() => match response.text().await {
            Ok(text) => parse_codex_models(&text),
            Err(_) => Vec::new(),
        },
        _ => Vec::new(),
    }
}

pub(crate) fn codex_client_version() -> String {
    let Some(home) = dirs::home_dir() else {
        return env!("CARGO_PKG_VERSION").to_string();
    };
    let candidates = [
        (home.join(".codex").join("version.json"), "/latest_version"),
        (
            home.join(".codex").join("models_cache.json"),
            "/client_version",
        ),
    ];
    candidates
        .into_iter()
        .find_map(|(path, pointer)| {
            let content = std::fs::read_to_string(path).ok()?;
            serde_json::from_str::<serde_json::Value>(&content)
                .ok()?
                .pointer(pointer)
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|version| !version.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string())
}

fn default_true() -> bool {
    true
}

fn parse_codex_models(text: &str) -> Vec<ModelInfo> {
    let Ok(mut response) = serde_json::from_str::<CodexModelsResponse>(text) else {
        return Vec::new();
    };
    response.models.sort_by_key(|model| model.priority);
    let mut models = Vec::new();
    for model in response.models {
        if !model.supported_in_api || matches!(model.visibility.as_str(), "hide" | "hidden") {
            continue;
        }
        if models
            .iter()
            .any(|existing: &ModelInfo| existing.id == model.slug)
        {
            continue;
        }
        models.push(ModelInfo {
            id: model.slug,
            input_price: None,
            output_price: None,
            context_window: model.context_window,
            default_reasoning_effort: model.default_reasoning_level,
            supported_reasoning_efforts: model
                .supported_reasoning_levels
                .into_iter()
                .map(|preset| preset.effort)
                .filter(|effort| !effort.trim().is_empty())
                .collect(),
        });
    }
    models
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
                default_reasoning_effort: None,
                supported_reasoning_efforts: Vec::new(),
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
                    default_reasoning_effort: None,
                    supported_reasoning_efforts: Vec::new(),
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
                default_reasoning_effort: None,
                supported_reasoning_efforts: Vec::new(),
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
    let request = CredentialRequest::new(name, env_var, endpoint);
    let credential = resolve_provider_credential(request).await.ok().flatten();
    let mut models = fetch_models(name, endpoint, credential.as_ref(), backend_type).await;
    if models.is_empty() {
        models = provider_model_fallback(name);
    }
    if !models.is_empty() {
        let db_models: Vec<crate::storage::db::ModelRow> = models
            .into_iter()
            .map(|m| {
                (
                    m.id,
                    m.input_price,
                    m.output_price,
                    m.context_window,
                    m.default_reasoning_effort,
                    m.supported_reasoning_efforts,
                )
            })
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
        let needs_refresh = crate::llm::auth::canonical_provider_name(&name) == "Codex"
            || if let Ok(Some(fetched_at)) = storage.get_provider_models_fetched_at(&name) {
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

        let request = CredentialRequest::new(&name, &env_var, &endpoint);
        let credential = resolve_provider_credential(request).await.ok().flatten();
        let mut models = fetch_models(&name, &endpoint, credential.as_ref(), &backend_type).await;
        if models.is_empty() {
            models = provider_model_fallback(&name);
        }
        if models.is_empty() {
            continue;
        }

        let db_models: Vec<crate::storage::db::ModelRow> = models
            .into_iter()
            .map(|m| {
                (
                    m.id,
                    m.input_price,
                    m.output_price,
                    m.context_window,
                    m.default_reasoning_effort,
                    m.supported_reasoning_efforts,
                )
            })
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
            default_reasoning_effort: None,
            supported_reasoning_efforts: Vec::new(),
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

    #[test]
    fn codex_catalog_keeps_visible_api_models_in_priority_order() {
        let models = parse_codex_models(
            r#"{
                "models": [
                    {"slug":"gpt-5.6-sol","visibility":"list","supported_in_api":true,"priority":20,"context_window":272000},
                    {"slug":"hidden-model","visibility":"hide","supported_in_api":true,"priority":0},
                    {"slug":"gpt-5.6-luna","visibility":"list","supported_in_api":true,"priority":10,"context_window":200000},
                    {"slug":"unsupported-model","visibility":"list","supported_in_api":false,"priority":1}
                ]
            }"#,
        );

        assert_eq!(
            models
                .iter()
                .map(|model| (model.id.as_str(), model.context_window))
                .collect::<Vec<_>>(),
            [
                ("gpt-5.6-luna", Some(200_000)),
                ("gpt-5.6-sol", Some(272_000))
            ]
        );
    }

    #[test]
    fn codex_catalog_retains_model_reasoning_metadata() -> Result<(), Box<dyn std::error::Error>> {
        let models = parse_codex_models(
            r#"{
                "models": [{
                    "slug":"gpt-5.6-sol",
                    "visibility":"list",
                    "supported_in_api":true,
                    "priority":10,
                    "default_reasoning_level":"medium",
                    "supported_reasoning_levels":[
                        {"effort":"low","description":"Fast"},
                        {"effort":"medium","description":"Balanced"},
                        {"effort":"future","description":"Server-defined"}
                    ]
                }]
            }"#,
        );
        let model = models.first().ok_or("missing parsed Codex model")?;

        assert_eq!(model.default_reasoning_effort.as_deref(), Some("medium"));
        assert_eq!(
            model.supported_reasoning_efforts,
            ["low", "medium", "future"]
        );
        Ok(())
    }

    #[test]
    fn codex_client_version_uses_codex_release_metadata() {
        let _guard = crate::test_support::env_lock()
            .lock()
            .expect("env lock poisoned");
        let root = std::env::temp_dir().join(format!(
            "tcui-codex-version-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let codex_dir = root.join(".codex");
        std::fs::create_dir_all(&codex_dir).expect("create codex dir");
        std::fs::write(
            codex_dir.join("version.json"),
            r#"{"latest_version":"0.144.1"}"#,
        )
        .expect("write version metadata");
        std::env::set_var("HOME", &root);

        let version = codex_client_version();

        assert_eq!(version, "0.144.1");

        std::env::remove_var("HOME");
        std::fs::remove_dir_all(root).expect("cleanup temp dir");
    }
}
