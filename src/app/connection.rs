use super::{providers::ModelRow, Action, TuiApp};
use crate::config::AppConfig;

impl TuiApp {
    pub(crate) fn set_connection_state(
        &mut self,
        status: crate::ui::status_bar::ConnectionStatus,
        message: Option<String>,
    ) {
        self.ui.connection_status = status;
        self.ui.connection_message = message;
    }

    pub(crate) fn queue_connection_check_for_active_tab(&self) {
        if tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        let Some(tab) = self.ui.tabs.get(self.ui.active_tab) else {
            return;
        };
        let config_snapshot = self
            .config
            .try_read()
            .map(|config| config.clone())
            .unwrap_or_default();
        let provider = tab.tab.provider.clone();
        let action_tx = self.action_tx.clone();
        let cloud_api_key =
            config_snapshot
                .provider_config(&provider)
                .and_then(|provider_config| {
                    crate::llm::auth::read_provider_api_key(
                        &provider_config.name,
                        &provider_config.env_var,
                        &self.storage,
                    )
                });
        tokio::spawn(async move {
            let _ = action_tx.send(Action::SetConnectionState(
                crate::ui::status_bar::ConnectionStatus::Checking,
                Some("Checking connection...".to_string()),
            ));
            if crate::llm::local::is_local_provider(&provider) {
                match crate::llm::local::probe(&config_snapshot.local_inference).await {
                    Ok(probe) => {
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
                            let _ =
                                storage.save_models(crate::config::LOCAL_PROVIDER_NAME, &models);
                        }
                        let _ = action_tx.send(Action::SetProviderModels(
                            crate::config::LOCAL_PROVIDER_NAME.to_string(),
                            probe.models.clone(),
                        ));
                        let (status, message) = Self::local_probe_state(&probe);
                        let _ = action_tx.send(Action::SetConnectionState(status, message));
                    }
                    Err(err) => {
                        let _ = action_tx.send(Action::SetConnectionState(
                            crate::ui::status_bar::ConnectionStatus::Failed,
                            Some(format!("Not connected to Local LLM: {err}")),
                        ));
                    }
                }
                return;
            }

            match Self::check_cloud_connection(
                &provider,
                &config_snapshot,
                cloud_api_key.as_deref(),
            )
            .await
            {
                Ok(()) => {
                    let _ = action_tx.send(Action::SetConnectionState(
                        crate::ui::status_bar::ConnectionStatus::CloudConnected,
                        None,
                    ));
                }
                Err(_) => {
                    let _ = action_tx.send(Action::SetConnectionState(
                        crate::ui::status_bar::ConnectionStatus::Failed,
                        Some("Not connected, check settings".to_string()),
                    ));
                }
            }
        });
    }

    pub(crate) fn local_probe_state(
        probe: &crate::llm::local::LocalProbe,
    ) -> (crate::ui::status_bar::ConnectionStatus, Option<String>) {
        match probe.selected_model_loaded {
            Some(false) if probe.server_type == crate::config::LocalServerType::Ollama => (
                crate::ui::status_bar::ConnectionStatus::LocalModelUnloaded,
                Some("Local model unloaded".to_string()),
            ),
            _ => (
                crate::ui::status_bar::ConnectionStatus::LocalConnected,
                Some(format!("Connected to {}", probe.status_label)),
            ),
        }
    }

    pub(crate) async fn check_cloud_connection(
        provider: &str,
        config: &AppConfig,
        api_key: Option<&str>,
    ) -> color_eyre::Result<()> {
        let Some(provider_config) = config.provider_config(provider) else {
            return Err(color_eyre::eyre::eyre!("Missing provider config"));
        };
        if provider_config.auth_type == "oauth" && api_key.is_some_and(|key| !key.trim().is_empty())
        {
            return Ok(());
        }
        let endpoint = provider_config.endpoint.trim_end_matches('/');
        let url = format!("{endpoint}/models");
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()?;
        let mut request = client.get(url);
        if let Some(api_key) = api_key.filter(|key| !key.trim().is_empty()) {
            request = request.bearer_auth(api_key);
        }
        if provider_config.name == "OpenRouter" {
            request = request
                .header("HTTP-Referer", "https://github.com/jp/TermChatUI")
                .header("X-Title", "TermChatUI");
        }
        request.send().await?.error_for_status()?;
        Ok(())
    }
}
