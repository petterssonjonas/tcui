use color_eyre::eyre::eyre;
use serde::Deserialize;

use crate::{
    config::{LocalInferenceConfig, LocalServerType, LOCAL_PROVIDER_NAME},
    ui::settings_tab::ModelInfo,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LocalCapabilities {
    pub can_list_models: bool,
    pub can_load_model: bool,
    pub can_unload_model: bool,
    pub can_report_loaded_model: bool,
    pub can_report_unloaded_state: bool,
    pub can_chat: bool,
    pub can_stream: bool,
}

#[derive(Debug, Clone)]
pub struct LocalProbe {
    pub server_type: LocalServerType,
    #[allow(dead_code)]
    pub capabilities: LocalCapabilities,
    pub models: Vec<ModelInfo>,
    pub selected_model_loaded: Option<bool>,
    pub status_label: String,
}

#[derive(Debug, Deserialize)]
struct OpenAiModelsResponse {
    data: Vec<OpenAiModel>,
}

#[derive(Debug, Deserialize)]
struct OpenAiModel {
    id: String,
}

#[derive(Debug, Deserialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaModel>,
}

#[derive(Debug, Deserialize)]
struct OllamaModel {
    name: String,
}

#[derive(Debug, Deserialize)]
struct OllamaPsResponse {
    models: Vec<OllamaModel>,
}

#[derive(Debug, Deserialize)]
struct LmStudioModelsResponse {
    data: Vec<LmStudioModel>,
}

#[derive(Debug, Deserialize)]
struct LmStudioModel {
    id: String,
    #[serde(default)]
    loaded_instances: Vec<serde_json::Value>,
}

pub fn is_local_provider(provider: &str) -> bool {
    provider.eq_ignore_ascii_case(LOCAL_PROVIDER_NAME)
}

pub fn local_provider_entry(
    config: &LocalInferenceConfig,
    detected: Option<LocalServerType>,
) -> (String, String, String, String, String) {
    (
        LOCAL_PROVIDER_NAME.to_string(),
        config.chat_endpoint(),
        config.api_token_env.clone().unwrap_or_default(),
        backend_type_label(detected.unwrap_or(config.server_type)).to_string(),
        "none".to_string(),
    )
}

pub fn backend_type_label(server_type: LocalServerType) -> &'static str {
    match server_type {
        LocalServerType::Auto
        | LocalServerType::Ollama
        | LocalServerType::LlamaCpp
        | LocalServerType::LmStudio
        | LocalServerType::OpenAiCompat => "openai",
    }
}

pub async fn probe(config: &LocalInferenceConfig) -> color_eyre::Result<LocalProbe> {
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_millis(
            config.connect_timeout_ms.max(250),
        ))
        .timeout(std::time::Duration::from_millis(
            config.request_timeout_ms.max(500),
        ))
        .build()?;

    let mut candidates = vec![config.server_type];
    if config.server_type == LocalServerType::Auto {
        candidates = vec![
            LocalServerType::Ollama,
            LocalServerType::LmStudio,
            LocalServerType::LlamaCpp,
            LocalServerType::OpenAiCompat,
        ];
    }

    let mut last_err = None;
    for candidate in candidates {
        match probe_type(&client, config, candidate).await {
            Ok(probe) => return Ok(probe),
            Err(err) => last_err = Some(err),
        }
    }

    Err(last_err.unwrap_or_else(|| eyre!("Local inference server is not reachable.")))
}

async fn probe_type(
    client: &reqwest::Client,
    config: &LocalInferenceConfig,
    server_type: LocalServerType,
) -> color_eyre::Result<LocalProbe> {
    match server_type {
        LocalServerType::Auto => Err(eyre!("auto probe is resolved before probe_type")),
        LocalServerType::Ollama => probe_ollama(client, config).await,
        LocalServerType::LmStudio => probe_lmstudio(client, config).await,
        LocalServerType::LlamaCpp => probe_llamacpp(client, config).await,
        LocalServerType::OpenAiCompat => probe_openai_compat(client, config).await,
    }
}

async fn probe_ollama(
    client: &reqwest::Client,
    config: &LocalInferenceConfig,
) -> color_eyre::Result<LocalProbe> {
    let base = config.base_url();
    let tags: OllamaTagsResponse = client
        .get(format!("{}/api/tags", base))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let running = match client.get(format!("{}/api/ps", base)).send().await {
        Ok(response) => match response.error_for_status() {
            Ok(response) => response.json::<OllamaPsResponse>().await.ok(),
            Err(_) => None,
        },
        Err(_) => None,
    };
    let models = tags
        .models
        .into_iter()
        .map(|model| ModelInfo {
            id: model.name,
            input_price: None,
            output_price: None,
        })
        .collect::<Vec<_>>();
    let selected_model = resolve_selected_model(&config.selected_model, &models);
    let loaded = if selected_model.is_empty() {
        None
    } else {
        running.map(|response| {
            response
                .models
                .into_iter()
                .any(|model| model.name == selected_model)
        })
    };
    Ok(LocalProbe {
        server_type: LocalServerType::Ollama,
        capabilities: LocalCapabilities {
            can_list_models: true,
            can_report_loaded_model: true,
            can_report_unloaded_state: true,
            can_chat: true,
            can_stream: true,
            ..LocalCapabilities::default()
        },
        models,
        selected_model_loaded: loaded,
        status_label: "Ollama".to_string(),
    })
}

async fn probe_lmstudio(
    client: &reqwest::Client,
    config: &LocalInferenceConfig,
) -> color_eyre::Result<LocalProbe> {
    let base = config.base_url();
    let response: LmStudioModelsResponse = client
        .get(format!("{}/api/v1/models", base))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let selected_model = config.selected_model.trim().to_string();
    let mut selected_loaded = None;
    let models = response
        .data
        .into_iter()
        .map(|model| {
            if !selected_model.is_empty() && model.id == selected_model {
                selected_loaded = Some(!model.loaded_instances.is_empty());
            }
            ModelInfo {
                id: model.id,
                input_price: None,
                output_price: None,
            }
        })
        .collect::<Vec<_>>();
    Ok(LocalProbe {
        server_type: LocalServerType::LmStudio,
        capabilities: LocalCapabilities {
            can_list_models: true,
            can_load_model: true,
            can_unload_model: true,
            can_report_loaded_model: true,
            can_report_unloaded_state: true,
            can_chat: true,
            can_stream: true,
        },
        models,
        selected_model_loaded: selected_loaded,
        status_label: "LM Studio".to_string(),
    })
}

async fn probe_llamacpp(
    client: &reqwest::Client,
    config: &LocalInferenceConfig,
) -> color_eyre::Result<LocalProbe> {
    let base = config.base_url();
    let health = client.get(format!("{}/health", base)).send().await?;
    if !health.status().is_success() {
        return Err(eyre!(
            "llama.cpp health endpoint returned {}",
            health.status()
        ));
    }
    let models = fetch_openai_models(client, format!("{}/v1/models", base)).await?;
    Ok(LocalProbe {
        server_type: LocalServerType::LlamaCpp,
        capabilities: LocalCapabilities {
            can_list_models: !models.is_empty(),
            can_chat: true,
            can_stream: true,
            ..LocalCapabilities::default()
        },
        models,
        selected_model_loaded: None,
        status_label: "llama.cpp".to_string(),
    })
}

async fn probe_openai_compat(
    client: &reqwest::Client,
    config: &LocalInferenceConfig,
) -> color_eyre::Result<LocalProbe> {
    let models = fetch_openai_models(client, format!("{}/v1/models", config.base_url())).await?;
    Ok(LocalProbe {
        server_type: LocalServerType::OpenAiCompat,
        capabilities: LocalCapabilities {
            can_list_models: !models.is_empty(),
            can_chat: true,
            can_stream: true,
            ..LocalCapabilities::default()
        },
        models,
        selected_model_loaded: None,
        status_label: "Local LLM".to_string(),
    })
}

async fn fetch_openai_models(
    client: &reqwest::Client,
    url: String,
) -> color_eyre::Result<Vec<ModelInfo>> {
    let response: OpenAiModelsResponse = client
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(response
        .data
        .into_iter()
        .map(|model| ModelInfo {
            id: model.id,
            input_price: None,
            output_price: None,
        })
        .collect())
}

fn resolve_selected_model(configured: &str, models: &[ModelInfo]) -> String {
    let trimmed = configured.trim();
    if !trimmed.is_empty() {
        return trimmed.to_string();
    }
    models
        .first()
        .map(|model| model.id.clone())
        .unwrap_or_default()
}
