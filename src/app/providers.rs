use std::collections::HashSet;

use super::{Action, TuiApp};
use crate::config::AppConfig;

pub(crate) type ModelRow = (
    String,
    Option<f64>,
    Option<f64>,
    Option<u32>,
    Option<String>,
    Vec<String>,
);

fn refreshed_selection(
    current: &str,
    manual_override: bool,
    models: &[&crate::ui::ModelInfo],
) -> Option<String> {
    if manual_override || models.iter().any(|model| model.id == current) {
        return None;
    }
    Some(
        models
            .first()
            .map(|model| model.id.clone())
            .unwrap_or_default(),
    )
}
impl TuiApp {
    pub(crate) fn apply_refreshed_provider_models(
        &mut self,
        provider: &str,
        models: &[crate::ui::ModelInfo],
    ) {
        let visible_models: Vec<_> = models
            .iter()
            .filter(|model| {
                !self
                    .ui
                    .disabled_models
                    .contains(&Self::model_disable_key(provider, &model.id))
            })
            .collect();
        let mut changed_selection = None;
        for (index, tab) in self.ui.tabs.iter_mut().enumerate() {
            if tab.tab.provider != provider {
                continue;
            }
            if let Some(model) =
                refreshed_selection(&tab.tab.model, tab.manual_model_override, &visible_models)
            {
                if index == self.ui.active_tab && !tab.tab.model.is_empty() {
                    changed_selection = Some((tab.tab.model.clone(), model.clone()));
                }
                tab.tab.model = model;
                tab.tab.reasoning_effort = None;
            }
        }
        self.refresh_visible_selectors();
        if let Some((previous, model)) = changed_selection {
            self.ui.show_toast(if model.is_empty() {
                format!("{previous} is no longer available. Choose a model with /model.")
            } else {
                format!("{previous} is no longer available. Selected {model}.")
            });
        }
    }

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

    pub(crate) fn visible_models_for_provider(&self, provider: &str) -> Vec<crate::ui::ModelInfo> {
        let mut models = self.cached_models_for_provider(provider);
        models.retain(|model| {
            !self
                .ui
                .disabled_models
                .contains(&Self::model_disable_key(provider, &model.id))
        });
        models
    }

    pub(crate) fn reasoning_options_for(
        provider: &str,
        model: &crate::ui::ModelInfo,
    ) -> Vec<String> {
        let provider = crate::llm::auth::canonical_provider_name(provider);
        if provider == "Codex" {
            return model.supported_reasoning_efforts.clone();
        }
        if model.id.starts_with("gpt-5")
            && matches!(
                provider.as_str(),
                "OpenAI" | "OpenRouter" | "Groq" | "Mistral"
            )
        {
            return ["none", "low", "medium", "high", "xhigh"]
                .into_iter()
                .map(str::to_string)
                .collect();
        }
        Vec::new()
    }

    pub(crate) fn cached_models_for_provider(&self, provider: &str) -> Vec<crate::ui::ModelInfo> {
        let mut models = match self.storage.get_models(provider) {
            Ok(models) => models
                .into_iter()
                .map(
                    |(
                        id,
                        input_price,
                        output_price,
                        context_window,
                        default_reasoning_effort,
                        supported_reasoning_efforts,
                    )| crate::ui::ModelInfo {
                        id,
                        input_price,
                        output_price,
                        context_window,
                        default_reasoning_effort,
                        supported_reasoning_efforts,
                    },
                )
                .collect::<Vec<_>>(),
            Err(_) => Vec::new(),
        };
        if crate::llm::auth::canonical_provider_name(provider) != "Codex" {
            models.sort_by(|left, right| left.id.cmp(&right.id));
        }
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
            self.track_background_task(tokio::spawn(async move {
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
                            model.default_reasoning_effort.clone(),
                            model.supported_reasoning_efforts.clone(),
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
            }));
            return;
        }

        let Some((endpoint, env_var, backend_type)) = self.provider_config(&provider) else {
            return;
        };
        let action_tx = self.action_tx.clone();

        self.track_background_task(tokio::task::spawn_blocking(move || {
            let Ok(storage) = crate::storage::Storage::new() else {
                return;
            };
            let rt = tokio::runtime::Handle::current();
            let result = rt.block_on(async {
                crate::llm::model_fetcher::refresh_provider_models(
                    &storage,
                    &provider,
                    &endpoint,
                    &env_var,
                    &backend_type,
                )
                .await
            });
            let action = match result {
                Ok(models) => Action::SetProviderModels(provider, models),
                Err(message) => Action::ShowToast(format!("{provider}: {message}")),
            };
            let _ = action_tx.send(action);
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;

    fn model_info(id: &str) -> crate::ui::ModelInfo {
        crate::ui::ModelInfo {
            id: id.to_string(),
            input_price: None,
            output_price: None,
            context_window: None,
            default_reasoning_effort: None,
            supported_reasoning_efforts: Vec::new(),
        }
    }

    #[test]
    fn provider_entries_with_local_includes_local_when_enabled() {
        let mut config = AppConfig::default();
        config.local_inference.enabled = true;
        config.local_inference.host = "localhost".to_string();
        config.local_inference.port = 11434;
        config.local_inference.selected_model = "llama3.1".to_string();

        let entries = TuiApp::provider_entries_with_local(&config, None);

        assert!(entries
            .iter()
            .any(|(name, _, _, _, _)| name == crate::config::LOCAL_PROVIDER_NAME));
    }

    #[test]
    fn provider_entries_with_local_omits_local_when_disabled() {
        let mut config = AppConfig::default();
        config.local_inference.enabled = false;

        let entries = TuiApp::provider_entries_with_local(&config, None);

        assert!(!entries
            .iter()
            .any(|(name, _, _, _, _)| name == crate::config::LOCAL_PROVIDER_NAME));
    }

    #[test]
    fn filter_visible_providers_excludes_disabled() {
        let providers = vec![
            (
                "OpenAI".to_string(),
                "".to_string(),
                "".to_string(),
                "".to_string(),
                "".to_string(),
            ),
            (
                "Anthropic".to_string(),
                "".to_string(),
                "".to_string(),
                "".to_string(),
                "".to_string(),
            ),
        ];
        let mut disabled = HashSet::new();
        disabled.insert("OpenAI".to_string());

        let visible = TuiApp::filter_visible_providers(&providers, &disabled);

        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].0, "Anthropic");
    }

    #[test]
    fn model_disable_key_uses_colon_separator() {
        assert_eq!(
            TuiApp::model_disable_key("OpenAI", "gpt-4o"),
            "OpenAI:gpt-4o"
        );
    }

    #[test]
    fn reasoning_options_for_gpt5_models() {
        let model = model_info("gpt-5-chess");
        let options = TuiApp::reasoning_options_for("OpenAI", &model);
        assert_eq!(options, vec!["none", "low", "medium", "high", "xhigh"]);
    }

    #[test]
    fn reasoning_options_empty_for_non_gpt5() {
        let model = model_info("gpt-4o");
        let options = TuiApp::reasoning_options_for("OpenAI", &model);
        assert!(options.is_empty());
    }

    #[test]
    fn codex_reasoning_options_follow_selected_model_metadata() {
        let model = crate::ui::ModelInfo {
            id: "gpt-5.6-sol".to_string(),
            input_price: None,
            output_price: None,
            context_window: Some(272_000),
            default_reasoning_effort: Some("medium".to_string()),
            supported_reasoning_efforts: vec![
                "low".to_string(),
                "medium".to_string(),
                "high".to_string(),
            ],
        };

        let options = TuiApp::reasoning_options_for("Codex", &model);

        assert_eq!(options, vec!["low", "medium", "high"]);
    }
}
