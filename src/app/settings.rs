use super::TuiApp;
use crate::config::AppConfig;

impl TuiApp {
    pub(crate) fn fetch_models_for_settings(
        &self,
        config: &AppConfig,
    ) -> Vec<crate::ui::settings_tab::ModelInfo> {
        let provider = if config.default_provider.is_empty() {
            return Vec::new();
        } else {
            &config.default_provider
        };

        if crate::llm::local::is_local_provider(provider) {
            return self
                .storage
                .get_models(crate::config::LOCAL_PROVIDER_NAME)
                .unwrap_or_default()
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
        }

        self.storage
            .get_models(provider)
            .unwrap_or_default()
            .into_iter()
            .map(|(id, input_price, output_price, context_window)| {
                crate::ui::settings_tab::ModelInfo {
                    id,
                    input_price,
                    output_price,
                    context_window,
                }
            })
            .collect()
    }

    pub(crate) fn apply_theme_selection(&mut self, theme_name: &str) -> color_eyre::Result<()> {
        let key = crate::theme::canonical_theme_key(theme_name).to_string();
        let label = crate::theme::theme_label(&key);
        crate::theme::set_active_theme(&key);

        let mut config = self
            .config
            .try_read()
            .map(|config| config.clone())
            .unwrap_or_default();
        config.theme = key.clone();
        config.save()?;
        if let Ok(mut live_config) = self.config.try_write() {
            *live_config = config;
        }
        if let Some(settings) = &mut self.ui.settings_popup {
            settings.theme = key.clone();
        }
        self.ui.connection_message = Some(format!("Theme: {label}"));
        Ok(())
    }

    pub(crate) fn load_settings_popup_state(
        &self,
        config: &AppConfig,
    ) -> crate::ui::settings_tab::SettingsPopup {
        let db_providers = Self::provider_entries_with_local(config, None);
        let saved_keys = crate::config::KeyStore::load_keys(config)
            .unwrap_or_default()
            .into_iter()
            .map(|(provider, key)| {
                let display_name = db_providers
                    .iter()
                    .find(|(name, _, _, _, _)| {
                        crate::llm::auth::canonical_provider_name(name)
                            == crate::llm::auth::canonical_provider_name(&provider)
                    })
                    .map(|(name, _, _, _, _)| name.clone())
                    .unwrap_or(provider);
                (display_name, key)
            })
            .collect();
        let available_models = self.fetch_models_for_settings(config);

        let providers_tab_list: Vec<crate::ui::settings_tab::EditableProvider> = db_providers
            .iter()
            .filter(|(name, _, _, _, _)| !crate::llm::local::is_local_provider(name))
            .map(
                |(name, endpoint, _, backend_type, _)| crate::ui::settings_tab::EditableProvider {
                    name: name.clone(),
                    endpoint: endpoint.clone(),
                    backend_type: backend_type.clone(),
                },
            )
            .collect();

        let small_model = config.small_model.clone().unwrap_or_default();
        let models_provider = if config.default_provider.is_empty() {
            String::new()
        } else {
            config.default_provider.clone()
        };
        let models_available_models = self.fetch_models_for_settings(config);
        let mut popup = crate::ui::settings_tab::SettingsPopup::new(
            crate::ui::settings_tab::SettingsPopupInit {
                default_provider: config.default_provider.clone(),
                default_model: config.default_model.clone(),
                small_model,
                use_env_keys: config.use_env_keys,
                saved_keys,
                user_alignment: config.user_alignment,
                ai_alignment: config.ai_alignment,
                theme: config.theme.clone(),
                markdown_mode: config.markdown_mode,
                artifact_save_dir: config.artifact_save_dir.clone().unwrap_or_default(),
                vault_path: config.vault_path.clone().unwrap_or_default(),
                available_models,
                db_providers,
                show_selector: config.show_selector,
                show_chat_scrollbar: config.show_chat_scrollbar,
                collapse_thinking: config.collapse_thinking,
                kitty_enhanced_text: config.kitty_enhanced_text,
                kitty_heading_downscale: config.kitty_heading_downscale,
                web_search_enabled: config.web_search.enabled,
                quit_confirmation: config.quit_confirmation,
                local_enabled: config.local_inference.enabled,
                local_host: config.local_inference.host.clone(),
                local_port: config.local_inference.port.to_string(),
                local_server_type: config.local_inference.server_type,
                local_selected_model: config.local_inference.selected_model.clone(),
                local_model_directory: config
                    .local_inference
                    .model_directory
                    .clone()
                    .unwrap_or_default(),
                local_health_interval_seconds: config
                    .local_inference
                    .health_check_interval_seconds
                    .to_string(),
                local_connect_timeout_ms: config.local_inference.connect_timeout_ms.to_string(),
                local_request_timeout_ms: config.local_inference.request_timeout_ms.to_string(),
                local_api_token_env: config
                    .local_inference
                    .api_token_env
                    .clone()
                    .unwrap_or_default(),
                detected_local_server: None,
                providers_tab_list,
                models_provider,
                models_available_models,
                mcp_servers: crate::mcp::merged_configs(&config.mcp_servers),
            },
        );
        popup.check_oauth_tokens();
        for name in &config.disabled_providers {
            if !name.trim().is_empty() {
                popup.disabled_providers.insert(name.trim().to_string());
            }
        }
        for name in &config.disabled_models {
            if !name.trim().is_empty() {
                popup.disabled_models.insert(name.trim().to_string());
            }
        }
        popup
    }

    pub(crate) async fn save_settings_popup_state(
        &mut self,
        settings: &crate::ui::settings_tab::SettingsPopup,
    ) -> color_eyre::Result<()> {
        let mut config = self
            .config
            .try_read()
            .map(|config| config.clone())
            .unwrap_or_default();
        config.use_env_keys = settings.use_env_keys;
        config.user_alignment = settings.user_alignment;
        config.ai_alignment = settings.ai_alignment;
        config.theme = crate::theme::canonical_theme_key(&settings.theme).to_string();
        config.markdown_mode = settings.markdown_mode;
        config.artifact_save_dir = (!settings.artifact_save_dir.trim().is_empty())
            .then_some(settings.artifact_save_dir.trim().to_string());
        config.vault_path = (!settings.vault_path.trim().is_empty())
            .then_some(settings.vault_path.trim().to_string());
        config.default_provider = settings.default_provider.clone();
        config.default_model = settings.default_model.clone();
        config.small_model =
            (!settings.small_model.trim().is_empty()).then_some(settings.small_model.clone());
        config.show_selector = settings.show_selector;
        config.show_chat_scrollbar = settings.show_chat_scrollbar;
        config.collapse_thinking = settings.collapse_thinking;
        config.kitty_enhanced_text = settings.kitty_enhanced_text;
        config.kitty_heading_downscale = settings.kitty_heading_downscale;
        config.quit_confirmation = settings.quit_confirmation;
        config.web_search.enabled = settings.web_search_enabled;
        config.mcp_servers = settings.mcp_servers.clone();
        config.local_inference.enabled = settings.local_enabled;
        config.local_inference.host = settings.local_host.trim().to_string();
        config.local_inference.port = settings
            .local_port
            .trim()
            .parse::<u16>()
            .unwrap_or(config.local_inference.port.max(1));
        config.local_inference.server_type = settings.local_server_type;
        config.local_inference.selected_model = settings.local_selected_model.trim().to_string();
        config.local_inference.model_directory =
            (!settings.local_model_directory.trim().is_empty())
                .then_some(settings.local_model_directory.trim().to_string());
        config.local_inference.health_check_interval_seconds = settings
            .local_health_interval_seconds
            .trim()
            .parse::<u64>()
            .unwrap_or(config.local_inference.health_check_interval_seconds.max(1));
        config.local_inference.connect_timeout_ms = settings
            .local_connect_timeout_ms
            .trim()
            .parse::<u64>()
            .unwrap_or(config.local_inference.connect_timeout_ms.max(250));
        config.local_inference.request_timeout_ms = settings
            .local_request_timeout_ms
            .trim()
            .parse::<u64>()
            .unwrap_or(config.local_inference.request_timeout_ms.max(500));
        config.local_inference.api_token_env = (!settings.local_api_token_env.trim().is_empty())
            .then_some(settings.local_api_token_env.trim().to_string());
        config.disabled_providers = settings.disabled_providers.iter().cloned().collect();
        config.disabled_models = settings.disabled_models.iter().cloned().collect();
        config.providers = settings
            .db_providers
            .iter()
            .filter(|(name, _, _, _, _)| !crate::llm::local::is_local_provider(name))
            .map(|(name, endpoint, env_var, backend_type, auth_type)| {
                crate::config::ProviderConfig {
                    name: name.clone(),
                    endpoint: endpoint.clone(),
                    env_var: env_var.clone(),
                    backend_type: backend_type.clone(),
                    auth_type: auth_type.clone(),
                }
            })
            .collect();

        config.save()?;
        crate::config::KeyStore::save_keys(
            &config,
            &settings
                .saved_keys
                .iter()
                .filter(|(provider, _)| !crate::llm::auth::is_oauth_provider(provider))
                .map(|(provider, key)| {
                    (
                        if provider.ends_with(" Search") {
                            provider.clone()
                        } else {
                            crate::llm::auth::canonical_provider_name(provider)
                        },
                        key.clone(),
                    )
                })
                .collect::<Vec<_>>(),
        )?;
        self.storage.sync_providers(&config.providers)?;
        self.ui.db_providers = Self::provider_entries_with_local(&config, None);
        self.ui.disabled_providers = settings.disabled_providers.clone();
        self.ui.disabled_models = settings.disabled_models.clone();
        if let Ok(mut live_config) = self.config.try_write() {
            *live_config = config;
        }
        self.refresh_visible_selectors();
        crate::theme::set_active_theme(&settings.theme);
        Ok(())
    }

    pub(crate) async fn toggle_web_search(&mut self) -> color_eyre::Result<()> {
        let mut config = self
            .config
            .try_read()
            .map(|config| config.clone())
            .unwrap_or_default();
        config.web_search.enabled = !config.web_search.enabled;
        config.save()?;
        if let Ok(mut live_config) = self.config.try_write() {
            *live_config = config.clone();
        }
        self.ui.web_search_enabled = config.web_search.enabled;
        self.ui.connection_status = crate::ui::status_bar::ConnectionStatus::CloudConnected;
        self.ui.connection_message = Some(format!(
            "Web {}",
            if config.web_search.enabled {
                "on"
            } else {
                "off"
            }
        ));
        Ok(())
    }

    pub(crate) fn apply_settings_provider_action(
        &mut self,
        action: crate::ui::settings_tab::ProvidersAction,
    ) {
        match action {
            crate::ui::settings_tab::ProvidersAction::None => {}
            crate::ui::settings_tab::ProvidersAction::ToggleUseEnv => {
                if let Some(settings) = &mut self.ui.settings_popup {
                    if settings.use_env_keys {
                        settings.grab_keys_from_env();
                    }
                }
            }
            crate::ui::settings_tab::ProvidersAction::RefreshModels => {
                let Ok(storage) = crate::storage::Storage::new() else {
                    return;
                };
                tokio::task::spawn_blocking(move || {
                    let rt = tokio::runtime::Handle::current();
                    rt.block_on(async {
                        crate::llm::model_fetcher::refresh_all_models(&storage).await;
                    });
                });
            }
            crate::ui::settings_tab::ProvidersAction::SubmitAdd { provider, api_key } => {
                if let Some(settings) = &mut self.ui.settings_popup {
                    settings.apply_add_provider(provider.clone(), api_key.clone());
                }
                let name = provider.name.clone();
                let endpoint = provider.endpoint.clone();
                let env_var = format!("{}_API_KEY", provider.name.to_uppercase().replace(' ', "_"));
                let backend_type = provider.backend_type.clone();
                tokio::task::spawn_blocking(move || {
                    let rt = tokio::runtime::Handle::current();
                    rt.block_on(async {
                        if let Ok(storage) = crate::storage::Storage::new() {
                            crate::llm::model_fetcher::refresh_provider_models(
                                &storage,
                                &name,
                                &endpoint,
                                &env_var,
                                &backend_type,
                            )
                            .await;
                        }
                    });
                });
            }
            crate::ui::settings_tab::ProvidersAction::SubmitEdit {
                original_name,
                provider,
                api_key,
            } => {
                if let Some(settings) = &mut self.ui.settings_popup {
                    settings.apply_update_provider(
                        &original_name,
                        provider.clone(),
                        api_key.clone(),
                    );
                }
                let name = provider.name.clone();
                let endpoint = provider.endpoint.clone();
                let env_var = format!("{}_API_KEY", provider.name.to_uppercase().replace(' ', "_"));
                let backend_type = provider.backend_type.clone();
                tokio::task::spawn_blocking(move || {
                    let rt = tokio::runtime::Handle::current();
                    rt.block_on(async {
                        if let Ok(storage) = crate::storage::Storage::new() {
                            crate::llm::model_fetcher::refresh_provider_models(
                                &storage,
                                &name,
                                &endpoint,
                                &env_var,
                                &backend_type,
                            )
                            .await;
                        }
                    });
                });
            }
            crate::ui::settings_tab::ProvidersAction::DeleteProvider(name) => {
                if let Some(settings) = &mut self.ui.settings_popup {
                    settings.remove_provider_by_name(&name);
                }
            }
            crate::ui::settings_tab::ProvidersAction::SavePresetKey {
                provider_name,
                api_key,
            } => {
                if crate::llm::auth::is_oauth_provider(&provider_name) {
                    return;
                }
                if let Some(settings) = &mut self.ui.settings_popup {
                    settings.apply_preset_key_save(provider_name, api_key);
                }
            }
        }
    }
}
