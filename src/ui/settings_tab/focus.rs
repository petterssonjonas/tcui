use super::*;
use crate::ui::settings_tab::local::next_local_server_type;

impl SettingsPopup {
    pub fn next_tab(&mut self) {
        self.active_tab = match self.active_tab {
            SettingsTab::General => SettingsTab::Keybindings,
            SettingsTab::Keybindings => SettingsTab::Providers,
            SettingsTab::Providers => SettingsTab::Models,
            SettingsTab::Models => SettingsTab::Local,
            SettingsTab::Local => SettingsTab::Mcp,
            SettingsTab::Mcp => SettingsTab::General,
        };
        self.reset_focus();
    }

    pub fn prev_tab(&mut self) {
        self.active_tab = match self.active_tab {
            SettingsTab::General => SettingsTab::Mcp,
            SettingsTab::Keybindings => SettingsTab::General,
            SettingsTab::Providers => SettingsTab::Keybindings,
            SettingsTab::Models => SettingsTab::Providers,
            SettingsTab::Local => SettingsTab::Models,
            SettingsTab::Mcp => SettingsTab::Local,
        };
        self.reset_focus();
    }

    pub(super) fn reset_focus(&mut self) {
        self.general_focus = GeneralFocus::Theme;
        self.general_dropdown_open = None;
        self.providers_tab_focus = ProvidersTabFocus::DefaultProvider;
        self.models_tab_focus = ModelsTabFocus::Provider;
        self.models_dropdown_open = false;
        self.local_focus = LocalFocus::Enabled;
        self.mcp_focus = 0;
    }

    pub fn type_char(&mut self, c: char) {
        if let Some(popup) = self.preset_key_popup.as_mut() {
            if self.providers_tab_focus == ProvidersTabFocus::PopupApiKey {
                popup.api_key.push(c);
            }
        } else if let Some(form) = self.edit_provider_popup.as_mut() {
            Self::type_char_in_form(form, c);
        } else if let Some(form) = self.add_provider_popup.as_mut() {
            Self::type_char_in_form(form, c);
        } else if self.active_tab == SettingsTab::General {
            if self.general_focus == GeneralFocus::ArtifactSaveDir {
                self.artifact_save_dir.push(c);
            }
        } else if self.active_tab == SettingsTab::Local {
            match self.local_focus {
                LocalFocus::Host => self.local_host.push(c),
                LocalFocus::Port => {
                    if c.is_ascii_digit() {
                        self.local_port.push(c);
                    }
                }
                LocalFocus::SelectedModel => self.local_selected_model.push(c),
                LocalFocus::ModelDirectory => self.local_model_directory.push(c),
                LocalFocus::HealthInterval => {
                    if c.is_ascii_digit() {
                        self.local_health_interval_seconds.push(c);
                    }
                }
                LocalFocus::ConnectTimeout => {
                    if c.is_ascii_digit() {
                        self.local_connect_timeout_ms.push(c);
                    }
                }
                LocalFocus::RequestTimeout => {
                    if c.is_ascii_digit() {
                        self.local_request_timeout_ms.push(c);
                    }
                }
                LocalFocus::ApiTokenEnv => {
                    if c.is_ascii_alphanumeric() || c == '_' {
                        self.local_api_token_env.push(c);
                    }
                }
                LocalFocus::Enabled | LocalFocus::ServerType => {}
            }
        }
    }

    pub fn backspace(&mut self) {
        if let Some(popup) = self.preset_key_popup.as_mut() {
            if self.providers_tab_focus == ProvidersTabFocus::PopupApiKey {
                popup.api_key.pop();
            }
        } else if let Some(form) = self.edit_provider_popup.as_mut() {
            Self::backspace_in_form(form);
        } else if let Some(form) = self.add_provider_popup.as_mut() {
            Self::backspace_in_form(form);
        } else if self.active_tab == SettingsTab::General {
            if self.general_focus == GeneralFocus::ArtifactSaveDir {
                self.artifact_save_dir.pop();
            }
        } else if self.active_tab == SettingsTab::Local {
            match self.local_focus {
                LocalFocus::Host => {
                    self.local_host.pop();
                }
                LocalFocus::Port => {
                    self.local_port.pop();
                }
                LocalFocus::SelectedModel => {
                    self.local_selected_model.pop();
                }
                LocalFocus::ModelDirectory => {
                    self.local_model_directory.pop();
                }
                LocalFocus::HealthInterval => {
                    self.local_health_interval_seconds.pop();
                }
                LocalFocus::ConnectTimeout => {
                    self.local_connect_timeout_ms.pop();
                }
                LocalFocus::RequestTimeout => {
                    self.local_request_timeout_ms.pop();
                }
                LocalFocus::ApiTokenEnv => {
                    self.local_api_token_env.pop();
                }
                LocalFocus::Enabled | LocalFocus::ServerType => {}
            }
        }
    }

    pub(super) fn type_char_in_form(form: &mut ProviderFormState, c: char) {
        match form.focus {
            ProviderFormFocus::ProviderName => form.name.push(c),
            ProviderFormFocus::ProviderEndpoint => form.endpoint.push(c),
            ProviderFormFocus::ProviderApiKey => form.api_key.push(c),
            _ => {}
        }
    }

    pub(super) fn backspace_in_form(form: &mut ProviderFormState) {
        match form.focus {
            ProviderFormFocus::ProviderName => {
                form.name.pop();
            }
            ProviderFormFocus::ProviderEndpoint => {
                form.endpoint.pop();
            }
            ProviderFormFocus::ProviderApiKey => {
                form.api_key.pop();
            }
            _ => {}
        }
    }

    pub fn next_popup_focus(&mut self) {
        if self.preset_key_popup.is_some() {
            self.providers_tab_focus = match self.providers_tab_focus {
                ProvidersTabFocus::PopupApiKey => ProvidersTabFocus::PopupSaveButton,
                ProvidersTabFocus::PopupSaveButton => ProvidersTabFocus::PopupCancelButton,
                ProvidersTabFocus::PopupCancelButton => ProvidersTabFocus::PopupApiKey,
                _ => ProvidersTabFocus::PopupApiKey,
            };
        } else if let Some(form) = self.edit_provider_popup.as_mut() {
            Self::cycle_form_focus(form, true);
        } else if let Some(popup) = self.edit_providers_popup.as_mut() {
            Self::cycle_edit_popup_focus(popup, self.providers_tab_list.len(), true);
        } else if let Some(form) = self.add_provider_popup.as_mut() {
            Self::cycle_form_focus(form, true);
        }
    }

    pub fn prev_popup_focus(&mut self) {
        if self.preset_key_popup.is_some() {
            self.providers_tab_focus = match self.providers_tab_focus {
                ProvidersTabFocus::PopupApiKey => ProvidersTabFocus::PopupCancelButton,
                ProvidersTabFocus::PopupSaveButton => ProvidersTabFocus::PopupApiKey,
                ProvidersTabFocus::PopupCancelButton => ProvidersTabFocus::PopupSaveButton,
                _ => ProvidersTabFocus::PopupApiKey,
            };
        } else if let Some(form) = self.edit_provider_popup.as_mut() {
            Self::cycle_form_focus(form, false);
        } else if let Some(popup) = self.edit_providers_popup.as_mut() {
            Self::cycle_edit_popup_focus(popup, self.providers_tab_list.len(), false);
        } else if let Some(form) = self.add_provider_popup.as_mut() {
            Self::cycle_form_focus(form, false);
        }
    }

    pub(super) fn cycle_form_focus(form: &mut ProviderFormState, forward: bool) {
        form.dropdown_open = false;
        form.focus = match (form.focus, forward) {
            (ProviderFormFocus::ProviderName, true) => ProviderFormFocus::ProviderEndpoint,
            (ProviderFormFocus::ProviderEndpoint, true) => ProviderFormFocus::ProviderBackendType,
            (ProviderFormFocus::ProviderBackendType, true) => ProviderFormFocus::ProviderApiKey,
            (ProviderFormFocus::ProviderApiKey, true) => ProviderFormFocus::SubmitButton,
            (ProviderFormFocus::SubmitButton, true) => ProviderFormFocus::CancelButton,
            (ProviderFormFocus::CancelButton, true) => ProviderFormFocus::ProviderName,
            (ProviderFormFocus::ProviderName, false) => ProviderFormFocus::CancelButton,
            (ProviderFormFocus::ProviderEndpoint, false) => ProviderFormFocus::ProviderName,
            (ProviderFormFocus::ProviderBackendType, false) => ProviderFormFocus::ProviderEndpoint,
            (ProviderFormFocus::ProviderApiKey, false) => ProviderFormFocus::ProviderBackendType,
            (ProviderFormFocus::SubmitButton, false) => ProviderFormFocus::ProviderApiKey,
            (ProviderFormFocus::CancelButton, false) => ProviderFormFocus::SubmitButton,
        };
    }

    pub(super) fn cycle_edit_popup_focus(
        popup: &mut EditProvidersPopupState,
        len: usize,
        forward: bool,
    ) {
        if len == 0 {
            popup.focus = None;
            return;
        }
        popup.focus = Some(
            match (
                popup.focus.unwrap_or(EditProvidersFocus::ProviderName(0)),
                forward,
            ) {
                (EditProvidersFocus::ProviderName(idx), true) => {
                    EditProvidersFocus::DeleteButton(idx)
                }
                (EditProvidersFocus::DeleteButton(idx), true) => {
                    if idx + 1 < len {
                        EditProvidersFocus::ProviderName(idx + 1)
                    } else {
                        EditProvidersFocus::ProviderName(0)
                    }
                }
                (EditProvidersFocus::ProviderName(idx), false) => {
                    if idx == 0 {
                        EditProvidersFocus::DeleteButton(len - 1)
                    } else {
                        EditProvidersFocus::DeleteButton(idx - 1)
                    }
                }
                (EditProvidersFocus::DeleteButton(idx), false) => {
                    EditProvidersFocus::ProviderName(idx)
                }
            },
        );
    }

    pub fn popup_up(&mut self) {
        if self.preset_key_popup.is_some() {
            self.providers_tab_focus = match self.providers_tab_focus {
                ProvidersTabFocus::PopupSaveButton | ProvidersTabFocus::PopupCancelButton => {
                    ProvidersTabFocus::PopupApiKey
                }
                _ => self.providers_tab_focus,
            };
        } else if let Some(form) = self.edit_provider_popup.as_mut() {
            Self::move_backend_selection(form, false);
        } else if let Some(popup) = self.edit_providers_popup.as_mut() {
            Self::move_edit_popup_vertically(popup, self.providers_tab_list.len(), false);
        } else if let Some(form) = self.add_provider_popup.as_mut() {
            Self::move_backend_selection(form, false);
        }
    }

    pub fn popup_down(&mut self) {
        if self.preset_key_popup.is_some() {
            self.providers_tab_focus = match self.providers_tab_focus {
                ProvidersTabFocus::PopupApiKey => ProvidersTabFocus::PopupSaveButton,
                _ => self.providers_tab_focus,
            };
        } else if let Some(form) = self.edit_provider_popup.as_mut() {
            Self::move_backend_selection(form, true);
        } else if let Some(popup) = self.edit_providers_popup.as_mut() {
            Self::move_edit_popup_vertically(popup, self.providers_tab_list.len(), true);
        } else if let Some(form) = self.add_provider_popup.as_mut() {
            Self::move_backend_selection(form, true);
        }
    }

    pub(super) fn move_backend_selection(form: &mut ProviderFormState, forward: bool) {
        if !form.dropdown_open || form.focus != ProviderFormFocus::ProviderBackendType {
            return;
        }
        let idx = BACKEND_TYPE_OPTIONS
            .iter()
            .position(|backend| *backend == form.backend_type)
            .unwrap_or(0);
        let new_idx = if forward {
            (idx + 1) % BACKEND_TYPE_OPTIONS.len()
        } else if idx == 0 {
            BACKEND_TYPE_OPTIONS.len() - 1
        } else {
            idx - 1
        };
        form.backend_type = BACKEND_TYPE_OPTIONS[new_idx].to_string();
    }

    pub(super) fn move_edit_popup_vertically(
        popup: &mut EditProvidersPopupState,
        len: usize,
        forward: bool,
    ) {
        if len == 0 {
            popup.focus = None;
            return;
        }
        popup.focus = Some(
            match popup.focus.unwrap_or(EditProvidersFocus::ProviderName(0)) {
                EditProvidersFocus::ProviderName(idx) => {
                    let new_idx = if forward {
                        (idx + 1) % len
                    } else if idx == 0 {
                        len - 1
                    } else {
                        idx - 1
                    };
                    EditProvidersFocus::ProviderName(new_idx)
                }
                EditProvidersFocus::DeleteButton(idx) => {
                    let new_idx = if forward {
                        (idx + 1) % len
                    } else if idx == 0 {
                        len - 1
                    } else {
                        idx - 1
                    };
                    EditProvidersFocus::DeleteButton(new_idx)
                }
            },
        );
    }

    pub fn activate_provider_popup(&mut self) -> ProvidersAction {
        if self.preset_key_popup.is_some() {
            return self.activate_preset_key_popup();
        }
        if self.edit_provider_popup.is_some() {
            return self.activate_form_popup(true);
        }
        if let Some(popup) = self.edit_providers_popup.as_mut() {
            return match popup.focus {
                Some(EditProvidersFocus::ProviderName(idx)) => {
                    self.open_edit_provider_popup(idx);
                    ProvidersAction::None
                }
                Some(EditProvidersFocus::DeleteButton(idx)) => self
                    .providers_tab_list
                    .get(idx)
                    .map(|provider| ProvidersAction::DeleteProvider(provider.name.clone()))
                    .unwrap_or(ProvidersAction::None),
                None => ProvidersAction::None,
            };
        }
        if self.add_provider_popup.is_some() {
            return self.activate_form_popup(false);
        }
        ProvidersAction::None
    }

    pub(super) fn activate_form_popup(&mut self, is_edit: bool) -> ProvidersAction {
        let form_opt = if is_edit {
            self.edit_provider_popup.as_mut()
        } else {
            self.add_provider_popup.as_mut()
        };
        let Some(form) = form_opt else {
            return ProvidersAction::None;
        };

        if form.dropdown_open && form.focus == ProviderFormFocus::ProviderBackendType {
            form.dropdown_open = false;
            return ProvidersAction::None;
        }

        let trimmed_name = form.name.trim().to_string();
        let original_name = form.original_name.clone();
        let duplicate_name = self.providers_tab_list.iter().any(|provider| {
            provider.name == trimmed_name
                && original_name
                    .as_ref()
                    .map(|original| original != &trimmed_name)
                    .unwrap_or(true)
        });

        match form.focus {
            ProviderFormFocus::ProviderBackendType => {
                form.dropdown_open = !form.dropdown_open;
                ProvidersAction::None
            }
            ProviderFormFocus::CancelButton => {
                if is_edit {
                    self.edit_provider_popup = None;
                } else {
                    self.add_provider_popup = None;
                }
                ProvidersAction::None
            }
            ProviderFormFocus::SubmitButton => {
                if !form.can_submit() || duplicate_name {
                    return ProvidersAction::None;
                }
                let provider = EditableProvider {
                    name: form.name.trim().to_string(),
                    endpoint: form.endpoint.trim().to_string(),
                    backend_type: form.backend_type.trim().to_string(),
                };
                let api_key = form.api_key.trim().to_string();
                if is_edit {
                    let original_name = form
                        .original_name
                        .clone()
                        .unwrap_or_else(|| provider.name.clone());
                    self.edit_provider_popup = None;
                    ProvidersAction::SubmitEdit {
                        original_name,
                        provider,
                        api_key,
                    }
                } else {
                    self.add_provider_popup = None;
                    ProvidersAction::SubmitAdd { provider, api_key }
                }
            }
            _ => ProvidersAction::None,
        }
    }

    pub fn prev_focus(&mut self) {
        match self.active_tab {
            SettingsTab::General => {
                self.close_general_dropdown();
                self.general_focus = match self.general_focus {
                    GeneralFocus::Theme => GeneralFocus::QuitConfirmation,
                    GeneralFocus::UserAlignment => GeneralFocus::Theme,
                    GeneralFocus::QuitConfirmation => GeneralFocus::WebSearchEnabled,
                    GeneralFocus::WebSearchEnabled => GeneralFocus::KittyTextScale,
                    GeneralFocus::KittyTextScale => GeneralFocus::KittyEnhancedText,
                    GeneralFocus::KittyEnhancedText => GeneralFocus::CollapseThinking,
                    GeneralFocus::CollapseThinking => GeneralFocus::ShowChatScrollbar,
                    GeneralFocus::ShowChatScrollbar => GeneralFocus::ShowSelector,
                    GeneralFocus::ShowSelector => GeneralFocus::ArtifactSaveDir,
                    GeneralFocus::ArtifactSaveDir => GeneralFocus::AiAlignment,
                    GeneralFocus::AiAlignment => GeneralFocus::UserAlignment,
                };
            }
            SettingsTab::Providers => {
                if let Some(form) = self.add_provider_popup.as_mut() {
                    Self::cycle_form_focus(form, false);
                } else if let Some(popup) = self.edit_providers_popup.as_mut() {
                    Self::cycle_edit_popup_focus(popup, self.providers_tab_list.len(), false);
                } else {
                    self.move_providers_tab_focus(false);
                }
            }
            SettingsTab::Models => self.move_models_tab_focus(false),
            SettingsTab::Local => {
                self.local_focus = match self.local_focus {
                    LocalFocus::Enabled => LocalFocus::ApiTokenEnv,
                    LocalFocus::Host => LocalFocus::Enabled,
                    LocalFocus::Port => LocalFocus::Host,
                    LocalFocus::ServerType => LocalFocus::Port,
                    LocalFocus::SelectedModel => LocalFocus::ServerType,
                    LocalFocus::ModelDirectory => LocalFocus::SelectedModel,
                    LocalFocus::HealthInterval => LocalFocus::ModelDirectory,
                    LocalFocus::ConnectTimeout => LocalFocus::HealthInterval,
                    LocalFocus::RequestTimeout => LocalFocus::ConnectTimeout,
                    LocalFocus::ApiTokenEnv => LocalFocus::RequestTimeout,
                };
            }
            SettingsTab::Mcp => {
                if !self.mcp_servers.is_empty() {
                    self.mcp_focus = if self.mcp_focus == 0 {
                        self.mcp_servers.len() - 1
                    } else {
                        self.mcp_focus - 1
                    };
                }
            }
            SettingsTab::Keybindings => {}
        }
    }

    pub fn activate_focus(&mut self) -> ProvidersAction {
        match self.active_tab {
            SettingsTab::General => match self.general_focus {
                GeneralFocus::Theme => {
                    self.toggle_general_dropdown(GeneralDropdown::Theme);
                    ProvidersAction::None
                }
                GeneralFocus::UserAlignment => {
                    self.toggle_general_dropdown(GeneralDropdown::UserAlignment);
                    ProvidersAction::None
                }
                GeneralFocus::AiAlignment => {
                    self.toggle_general_dropdown(GeneralDropdown::AiAlignment);
                    ProvidersAction::None
                }
                GeneralFocus::ArtifactSaveDir => ProvidersAction::None,
                GeneralFocus::ShowSelector => {
                    self.show_selector = !self.show_selector;
                    ProvidersAction::None
                }
                GeneralFocus::ShowChatScrollbar => {
                    self.show_chat_scrollbar = !self.show_chat_scrollbar;
                    ProvidersAction::None
                }
                GeneralFocus::CollapseThinking => {
                    self.collapse_thinking = !self.collapse_thinking;
                    ProvidersAction::None
                }
                GeneralFocus::KittyEnhancedText => {
                    self.kitty_enhanced_text = !self.kitty_enhanced_text;
                    ProvidersAction::None
                }
                GeneralFocus::KittyTextScale => {
                    self.toggle_general_dropdown(GeneralDropdown::KittyTextScale);
                    ProvidersAction::None
                }
                GeneralFocus::WebSearchEnabled => {
                    self.web_search_enabled = !self.web_search_enabled;
                    ProvidersAction::None
                }
                GeneralFocus::QuitConfirmation => {
                    self.quit_confirmation = !self.quit_confirmation;
                    ProvidersAction::None
                }
            },
            SettingsTab::Providers => match self.providers_tab_focus {
                ProvidersTabFocus::UseEnvToggle => {
                    self.grab_keys_from_env();
                    ProvidersAction::ToggleUseEnv
                }
                ProvidersTabFocus::AddProviderButton => {
                    self.open_add_provider_popup();
                    ProvidersAction::None
                }
                ProvidersTabFocus::EditProvidersButton => {
                    self.open_edit_providers_popup();
                    ProvidersAction::None
                }
                ProvidersTabFocus::ReloadModelsButton => ProvidersAction::RefreshModels,
                ProvidersTabFocus::SmallProvider => {
                    self.toggle_providers_dropdown(ProvidersDropdown::SmallProvider);
                    ProvidersAction::None
                }
                ProvidersTabFocus::SmallModel => {
                    self.toggle_providers_dropdown(ProvidersDropdown::SmallModel);
                    ProvidersAction::None
                }
                ProvidersTabFocus::DefaultProvider => {
                    self.toggle_providers_dropdown(ProvidersDropdown::DefaultProvider);
                    ProvidersAction::None
                }
                ProvidersTabFocus::DefaultModel => {
                    self.toggle_providers_dropdown(ProvidersDropdown::DefaultModel);
                    ProvidersAction::None
                }
                ProvidersTabFocus::SavedKeyList(idx) => {
                    let preset = self.preset_api_key_providers();
                    let custom: Vec<_> = self
                        .db_providers
                        .iter()
                        .filter(|(n, _, _, _, auth_type)| {
                            auth_type != "oauth" && preset.iter().all(|(pn, _, _, _, _)| pn != n)
                        })
                        .collect();
                    if let Some((name, _, _, _, _)) = custom.get(idx) {
                        if self.disabled_providers.contains(name) {
                            self.disabled_providers.remove(name);
                        } else {
                            self.disabled_providers.insert(name.clone());
                        }
                    }
                    ProvidersAction::None
                }
                ProvidersTabFocus::OAuthProvider(idx) => {
                    let oauth = self.oauth_providers();
                    if let Some((name, _, _, _, _)) = oauth.get(idx) {
                        if self.disabled_providers.contains(name) {
                            self.disabled_providers.remove(name);
                        } else {
                            self.disabled_providers.insert(name.clone());
                        }
                    }
                    ProvidersAction::None
                }
                ProvidersTabFocus::PresetProvider(idx) => {
                    if let Some((name, _, _, _, _)) = self.preset_api_key_providers().get(idx) {
                        if self.disabled_providers.contains(name) {
                            self.disabled_providers.remove(name);
                        } else {
                            self.disabled_providers.insert(name.clone());
                        }
                    }
                    ProvidersAction::None
                }
                _ => ProvidersAction::None,
            },
            SettingsTab::Models => {
                self.activate_models_focus();
                ProvidersAction::None
            }
            SettingsTab::Local => {
                match self.local_focus {
                    LocalFocus::Enabled => {
                        self.local_enabled = !self.local_enabled;
                    }
                    LocalFocus::ServerType => {
                        self.local_server_type = next_local_server_type(self.local_server_type);
                    }
                    _ => {}
                }
                ProvidersAction::None
            }
            SettingsTab::Mcp => {
                if let Some(server) = self.mcp_servers.get_mut(self.mcp_focus) {
                    server.enabled = !server.enabled;
                }
                ProvidersAction::None
            }
            SettingsTab::Keybindings => ProvidersAction::None,
        }
    }

    pub fn next_focus(&mut self) {
        match self.active_tab {
            SettingsTab::General => {
                self.close_general_dropdown();
                self.general_focus = match self.general_focus {
                    GeneralFocus::Theme => GeneralFocus::UserAlignment,
                    GeneralFocus::UserAlignment => GeneralFocus::AiAlignment,
                    GeneralFocus::AiAlignment => GeneralFocus::ArtifactSaveDir,
                    GeneralFocus::ArtifactSaveDir => GeneralFocus::ShowSelector,
                    GeneralFocus::ShowSelector => GeneralFocus::ShowChatScrollbar,
                    GeneralFocus::ShowChatScrollbar => GeneralFocus::CollapseThinking,
                    GeneralFocus::CollapseThinking => GeneralFocus::KittyEnhancedText,
                    GeneralFocus::KittyEnhancedText => GeneralFocus::KittyTextScale,
                    GeneralFocus::KittyTextScale => GeneralFocus::WebSearchEnabled,
                    GeneralFocus::WebSearchEnabled => GeneralFocus::QuitConfirmation,
                    GeneralFocus::QuitConfirmation => GeneralFocus::Theme,
                };
            }
            SettingsTab::Providers => {
                if let Some(form) = self.add_provider_popup.as_mut() {
                    Self::cycle_form_focus(form, true);
                } else if let Some(popup) = self.edit_providers_popup.as_mut() {
                    Self::cycle_edit_popup_focus(popup, self.providers_tab_list.len(), true);
                } else {
                    self.move_providers_tab_focus(true);
                }
            }
            SettingsTab::Models => self.move_models_tab_focus(true),
            SettingsTab::Local => {
                self.local_focus = match self.local_focus {
                    LocalFocus::Enabled => LocalFocus::Host,
                    LocalFocus::Host => LocalFocus::Port,
                    LocalFocus::Port => LocalFocus::ServerType,
                    LocalFocus::ServerType => LocalFocus::SelectedModel,
                    LocalFocus::SelectedModel => LocalFocus::ModelDirectory,
                    LocalFocus::ModelDirectory => LocalFocus::HealthInterval,
                    LocalFocus::HealthInterval => LocalFocus::ConnectTimeout,
                    LocalFocus::ConnectTimeout => LocalFocus::RequestTimeout,
                    LocalFocus::RequestTimeout => LocalFocus::ApiTokenEnv,
                    LocalFocus::ApiTokenEnv => LocalFocus::Enabled,
                };
            }
            SettingsTab::Mcp => {
                if !self.mcp_servers.is_empty() {
                    self.mcp_focus = (self.mcp_focus + 1) % self.mcp_servers.len();
                }
            }
            SettingsTab::Keybindings => {}
        }
    }
}
