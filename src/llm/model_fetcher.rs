use crate::{
    llm::auth::{resolve_provider_credential, Credential, CredentialRequest},
    ui::ModelInfo,
};
use secrecy::ExposeSecret;
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
struct CodexModel {
    slug: String,
    #[serde(default)]
    visibility: String,
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
    if !crate::llm::local::is_local_provider(provider) && credential.is_none() {
        return Vec::new();
    }
    if crate::llm::auth::canonical_provider_name(provider) == "Codex" {
        return fetch_codex_models(credential).await;
    }

    let url = format!("{}/models", endpoint);

    let Ok(client) = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()
    else {
        return Vec::new();
    };

    let mut request = client.get(&url);
    if let Some(credential) = credential {
        if !credential.permits_endpoint(provider, endpoint) {
            return Vec::new();
        }
        if matches!(backend_type, "anthropic" | "anthropic_sdk") {
            request = request
                .header("x-api-key", credential.bearer_token().expose_secret())
                .header("anthropic-version", "2023-06-01");
        } else {
            request = request.bearer_auth(credential.bearer_token().expose_secret());
        }
    }
    if matches!(provider, "OpenRouter" | "OpenRouter OAuth") {
        request = request
            .header("HTTP-Referer", "https://github.com/jp/TermChatUI")
            .header("X-Title", "TermChatUI");
    }
    if provider == "OpenCode Go" {
        request = request.header("User-Agent", concat!("tcui/", env!("CARGO_PKG_VERSION")));
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
    let Ok(client) = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()
    else {
        return Vec::new();
    };
    let Some(account_id) = credential.account_id().filter(|id| !id.trim().is_empty()) else {
        return Vec::new();
    };
    let Ok(endpoint) = crate::llm::codex_responses::codex_api_base() else {
        return Vec::new();
    };
    let Ok(headers) = crate::llm::codex_responses::codex_client_headers(
        credential.bearer_token().expose_secret(),
        account_id,
    ) else {
        return Vec::new();
    };
    let request = client
        .get(format!("{endpoint}/models"))
        .query(&[(
            "client_version",
            crate::llm::codex_responses::CODEX_CLIENT_VERSION,
        )])
        .headers(headers)
        .header(reqwest::header::ACCEPT, "application/json");

    match request.send().await {
        Ok(response) if response.status().is_success() => match response.text().await {
            Ok(text) => parse_codex_models(&text),
            Err(_) => Vec::new(),
        },
        _ => Vec::new(),
    }
}

fn parse_codex_models(text: &str) -> Vec<ModelInfo> {
    let Ok(mut response) = serde_json::from_str::<CodexModelsResponse>(text) else {
        return Vec::new();
    };
    response.models.sort_by_key(|model| model.priority);
    let mut models = Vec::new();
    for model in response.models {
        // `supported_in_api` limits public API-key users, not ChatGPT OAuth.
        // https://github.com/openai/codex/blob/rust-v0.163.0/codex-rs/protocol/src/openai_models.rs
        if model.slug.trim().is_empty() || matches!(model.visibility.as_str(), "hide" | "hidden") {
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
) -> Result<Vec<ModelInfo>, &'static str> {
    let request = CredentialRequest::new(name, env_var, endpoint);
    let credential = resolve_provider_credential(request)
        .await
        .map_err(|_| "Unlock the credential vault or reconnect the provider.")?;
    let models = fetch_models(name, endpoint, credential.as_ref(), backend_type).await;
    if models.is_empty() {
        return Err("No model catalog received. Check the connection and refresh models again.");
    }
    let db_models: Vec<crate::storage::db::ModelRow> = models
        .iter()
        .map(|m| {
            (
                m.id.clone(),
                m.input_price,
                m.output_price,
                m.context_window,
                m.default_reasoning_effort.clone(),
                m.supported_reasoning_efforts.clone(),
            )
        })
        .collect();
    storage
        .save_models(name, &db_models)
        .map_err(|_| "Could not save the refreshed model catalog.")?;
    Ok(models)
}

pub async fn refresh_all_models(
    storage: &crate::storage::Storage,
    mut on_refresh: impl FnMut(String, Vec<ModelInfo>),
) {
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

        if let Ok(models) =
            refresh_provider_models(storage, &name, &endpoint, &env_var, &backend_type).await
        {
            on_refresh(name, models);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_catalog_keeps_visible_oauth_models_in_priority_order() {
        let models = parse_codex_models(
            r#"{
                "models": [
                    {"slug":"gpt-5.6-sol","visibility":"list","supported_in_api":true,"priority":20,"context_window":272000},
                    {"slug":"hidden-model","visibility":"hide","supported_in_api":true,"priority":0},
                    {"slug":"gpt-5.6-luna","visibility":"list","supported_in_api":true,"priority":10,"context_window":200000},
                    {"slug":"oauth-only-model","visibility":"list","supported_in_api":false,"priority":1}
                ]
            }"#,
        );

        assert_eq!(
            models
                .iter()
                .map(|model| (model.id.as_str(), model.context_window))
                .collect::<Vec<_>>(),
            [
                ("oauth-only-model", None),
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
}

#[cfg(test)]
#[path = "model_fetcher_tests.rs"]
mod discovery_tests;
