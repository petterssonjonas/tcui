use std::collections::HashSet;

use super::{Action, TuiApp};
use crate::config::AppConfig;

pub(crate) type ModelRow = (String, Option<f64>, Option<f64>, Option<u32>);

impl TuiApp {
    pub(crate) fn provider_entries_with_local(
        config: &AppConfig,
        detected: Option<crate::config::LocalServerType>,
    ) -> Vec<(String, String, String, String, String)> {
        let mut providers = config.provider_entries();
        if config.local_inference.enabled {
            providers.push(crate::llm::local::local_provider_entry(
                &config.local_inference,
                detected,
            ));
        }
        providers
    }

    pub(crate) fn filter_visible_providers(
        providers: &[(String, String, String, String, String)],
        disabled: &HashSet<String>,
    ) -> Vec<(String, String, String, String, String)> {
        providers
            .iter()
            .filter(|(name, _, _, _, _)| !disabled.contains(name))
            .cloned()
            .collect()
    }

    pub(crate) fn model_disable_key(provider: &str, model: &str) -> String {
        format!("{provider}:{model}")
    }

    pub(crate) fn visible_models_for_provider(
        &self,
        provider: &str,
    ) -> Vec<crate::ui::settings_tab::ModelInfo> {
        let mut models = self.cached_models_for_provider(provider);
        models.retain(|model| {
            !self
                .ui
                .disabled_models
                .contains(&Self::model_disable_key(provider, &model.id))
        });
        models
    }

    pub(crate) fn reasoning_options_for(provider: &str, model: &str) -> Vec<String> {
        let provider = crate::llm::auth::canonical_provider_name(provider);
        if model.starts_with("gpt-5")
            && matches!(
                provider.as_str(),
                "Codex" | "OpenAI" | "OpenRouter" | "Groq" | "Mistral"
            )
        {
            return ["none", "low", "medium", "high", "xhigh"]
                .into_iter()
                .map(str::to_string)
                .collect();
        }
        Vec::new()
    }

    pub(crate) fn cached_models_for_provider(
        &self,
        provider: &str,
    ) -> Vec<crate::ui::settings_tab::ModelInfo> {
        let mut models = match self.storage.get_models(provider) {
            Ok(models) => models
                .into_iter()
                .map(|(id, input_price, output_price, context_window)| {
                    crate::ui::settings_tab::ModelInfo {
                        id,
                        input_price,
                        output_price,
                        context_window,
                    }
                })
                .collect::<Vec<_>>(),
            Err(_) => Vec::new(),
        };
        for fallback in crate::llm::model_fetcher::provider_model_fallback(provider) {
            if !models.iter().any(|existing| existing.id == fallback.id) {
                models.push(fallback);
            }
        }
        models.sort_by(|left, right| left.id.cmp(&right.id));
        models
    }

    pub(crate) fn refresh_models_for_provider(&self, provider: String) {
        if crate::llm::local::is_local_provider(&provider) {
            let action_tx = self.action_tx.clone();
            let config_snapshot = self
                .config
                .try_read()
                .map(|config| config.clone())
                .unwrap_or_default();
            tokio::spawn(async move {
                let Ok(probe) = crate::llm::local::probe(&config_snapshot.local_inference).await
                else {
                    return;
                };
                let models: Vec<ModelRow> = probe
                    .models
                    .iter()
                    .map(|model| {
                        (
                            model.id.clone(),
                            model.input_price,
                            model.output_price,
                            model.context_window,
                        )
                    })
                    .collect();
                if let Ok(storage) = crate::storage::Storage::new() {
                    let _ = storage.save_models(crate::config::LOCAL_PROVIDER_NAME, &models);
                }
                let _ = action_tx.send(Action::SetProviderModels(
                    crate::config::LOCAL_PROVIDER_NAME.to_string(),
                    probe.models,
                ));
            });
            return;
        }

        let Some((endpoint, env_var, backend_type)) = self.provider_config(&provider) else {
            return;
        };
        let action_tx = self.action_tx.clone();

        tokio::task::spawn_blocking(move || {
            let Ok(storage) = crate::storage::Storage::new() else {
                return;
            };
            let rt = tokio::runtime::Handle::current();
            rt.block_on(async {
                crate::llm::model_fetcher::refresh_provider_models(
                    &storage,
                    &provider,
                    &endpoint,
                    &env_var,
                    &backend_type,
                )
                .await;
            });

            let Ok(models) = storage.get_models(&provider) else {
                return;
            };
            if models.is_empty() {
                return;
            }

            let model_infos = models
                .into_iter()
                .map(|(id, input_price, output_price, context_window)| {
                    crate::ui::settings_tab::ModelInfo {
                        id,
                        input_price,
                        output_price,
                        context_window,
                    }
                })
                .collect();
            let _ = action_tx.send(Action::SetProviderModels(provider, model_infos));
        });
    }
}
