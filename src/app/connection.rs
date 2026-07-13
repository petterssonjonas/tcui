use super::{Action, TuiApp, providers::ModelRow};
use secrecy::ExposeSecret;

const CODEX_CREDENTIAL_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(100);

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
        let provider_config = self.provider_config(&provider);
        let action_tx = self.action_tx.clone();
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
                                    model.default_reasoning_effort.clone(),
                                    model.supported_reasoning_efforts.clone(),
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

            let Some((endpoint, env_var, _)) = provider_config else {
                let _ = action_tx.send(Action::SetConnectionState(
                    crate::ui::status_bar::ConnectionStatus::Failed,
                    Some("Not connected, check settings".to_string()),
                ));
                return;
            };
            let credential =
                match Self::resolve_connection_credential(&provider, &env_var, &endpoint).await {
                    Ok(credential) => credential,
                    Err(error) => {
                        let _ = action_tx.send(Action::SetConnectionState(
                            crate::ui::status_bar::ConnectionStatus::Failed,
                            Some(error.to_string()),
                        ));
                        return;
                    }
                };

            match Self::check_cloud_connection(&provider, &endpoint, credential.as_ref()).await {
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

    async fn resolve_connection_credential(
        provider: &str,
        env_var: &str,
        endpoint: &str,
    ) -> Result<Option<crate::llm::auth::Credential>, crate::llm::auth::CredentialError> {
        let request = crate::llm::auth::CredentialRequest::new(provider, env_var, endpoint);
        match crate::llm::auth::resolve_provider_credential(request).await {
            Err(error) if Self::should_retry_connection_credential(provider, &error) => {
                tokio::time::sleep(CODEX_CREDENTIAL_RETRY_DELAY).await;
                let retry = crate::llm::auth::CredentialRequest::new(provider, env_var, endpoint);
                crate::llm::auth::resolve_provider_credential(retry).await
            }
            result => result,
        }
    }

    fn should_retry_connection_credential(
        provider: &str,
        error: &crate::llm::auth::CredentialError,
    ) -> bool {
        crate::llm::auth::canonical_provider_name(provider) == "Codex"
            && matches!(
                error,
                crate::llm::auth::CredentialError::CodexCredentialUnavailable
            )
    }

    pub(crate) async fn check_cloud_connection(
        provider: &str,
        endpoint: &str,
        credential: Option<&crate::llm::auth::Credential>,
    ) -> color_eyre::Result<()> {
        if !crate::llm::auth::trusted_provider_endpoint(provider, endpoint) {
            return Err(color_eyre::eyre::eyre!("Provider endpoint is not trusted"));
        }
        if crate::llm::auth::canonical_provider_name(provider) == "Codex" && credential.is_some() {
            return Ok(());
        }
        Self::probe_cloud_connection(provider, endpoint, credential).await
    }

    async fn probe_cloud_connection(
        provider: &str,
        endpoint: &str,
        credential: Option<&crate::llm::auth::Credential>,
    ) -> color_eyre::Result<()> {
        let endpoint = endpoint.trim_end_matches('/');
        let url = format!("{endpoint}/models");
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()?;
        let mut request = client.get(url);
        if let Some(credential) = credential {
            request = request.bearer_auth(credential.bearer_token().expose_secret());
        }
        if provider == "OpenRouter" {
            request = request
                .header("HTTP-Referer", "https://github.com/jp/TermChatUI")
                .header("X-Title", "TermChatUI");
        }
        request.send().await?.error_for_status()?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) async fn probe_cloud_connection_for_test(
        provider: &str,
        endpoint: &str,
        credential: &crate::llm::auth::Credential,
    ) -> color_eyre::Result<()> {
        Self::probe_cloud_connection(provider, endpoint, Some(credential)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe_with(
        status_label: &str,
        selected_model_loaded: Option<bool>,
    ) -> crate::llm::local::LocalProbe {
        crate::llm::local::LocalProbe {
            server_type: crate::config::LocalServerType::Ollama,
            capabilities: crate::llm::local::LocalCapabilities::default(),
            models: Vec::new(),
            selected_model_loaded,
            status_label: status_label.to_string(),
        }
    }

    #[test]
    fn local_probe_state_reports_connected_when_model_loaded() {
        let probe = probe_with("Ollama", Some(true));
        let (status, message) = TuiApp::local_probe_state(&probe);
        assert_eq!(
            status,
            crate::ui::status_bar::ConnectionStatus::LocalConnected
        );
        assert_eq!(message, Some("Connected to Ollama".to_string()));
    }

    #[test]
    fn local_probe_state_reports_unloaded_for_ollama_when_not_loaded() {
        let probe = probe_with("Ollama", Some(false));
        let (status, message) = TuiApp::local_probe_state(&probe);
        assert_eq!(
            status,
            crate::ui::status_bar::ConnectionStatus::LocalModelUnloaded
        );
        assert_eq!(message, Some("Local model unloaded".to_string()));
    }

    #[test]
    fn local_probe_state_connected_when_loaded_state_unknown() {
        let probe = probe_with("LM Studio", None);
        let (status, message) = TuiApp::local_probe_state(&probe);
        assert_eq!(
            status,
            crate::ui::status_bar::ConnectionStatus::LocalConnected
        );
        assert_eq!(message, Some("Connected to LM Studio".to_string()));
    }
}

#[cfg(test)]
#[path = "connection_tests.rs"]
mod connection_tests;
