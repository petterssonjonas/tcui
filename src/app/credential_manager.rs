use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use zeroize::Zeroize;

use super::TuiApp;
use crate::config::key_store::{CredentialStorage, KeyStore, SecretEntry, SecretKind};
use crate::config::{AppConfig, ProviderConfig, SearchBackend};
use crate::notifications::pushover::{self, APP_KEY_ID, USER_KEY_ID};

const PUSHOVER_PRESETS: [(&str, &str); 2] = [
    (USER_KEY_ID, "Pushover user key"),
    (APP_KEY_ID, "Pushover TCUI application key"),
];

fn is_pushover_secret(id: &str) -> bool {
    pushover::is_credential_id(id)
}

/// Nonsecret editor intent; safe to carry across application teardown.
pub(crate) enum PushoverCredentialTarget {
    User,
    Application(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Providers,
    SearchKeys,
    RemoveSearchKey,
    Vault,
    Presets,
    PushoverSaved,
    Name,
    Endpoint,
    Storage,
    Unlock,
    NewPassword,
    ConfirmPassword,
    ApiKey,
    OAuth,
    OAuthResponse,
    SecretLabel,
    SecretValue,
    RemoveProvider,
    RemoveSecret,
    Model,
}

#[derive(Clone, Copy)]
enum AfterUnlock {
    Vault,
    Provider,
    Storage,
    Pushover,
}

#[derive(Default)]
struct SearchServiceState {
    saved: Option<bool>,
    enabled: bool,
    transport: String,
}

pub(crate) struct CredentialManager {
    screen: Screen,
    selected: usize,
    input: String,
    password: String,
    error: Option<String>,
    notice: String,
    providers: Vec<ProviderConfig>,
    presets: Vec<ProviderConfig>,
    secrets: Vec<SecretEntry>,
    draft: Option<ProviderConfig>,
    storage: CredentialStorage,
    closing: bool,
    search_service: Option<SearchBackend>,
    search_services: [SearchServiceState; 3],
    pushover_edit: bool,
    pushover_target: Option<PushoverCredentialTarget>,
    after_unlock: AfterUnlock,
    secret_id: String,
    secret_label: String,
    secret_kind: SecretKind,
    secret_value: String,
    oauth_url: Option<String>,
    oauth_events: Option<tokio::sync::mpsc::UnboundedReceiver<crate::llm::auth::tui::LoginEvent>>,
    oauth_result: Option<tokio::task::JoinHandle<Result<(), String>>>,
    oauth_cancel: Option<crate::llm::auth::tui::OAuthCancellationHandle>,
    oauth_response: Option<tokio::sync::mpsc::Sender<zeroize::Zeroizing<String>>>,
}

impl Drop for CredentialManager {
    fn drop(&mut self) {
        if let Some(cancel) = &self.oauth_cancel {
            cancel.cancel();
        }
        if let Some(task) = &self.oauth_result {
            task.abort();
        }
        self.input.zeroize();
        self.password.zeroize();
        self.secret_value.zeroize();
    }
}

impl CredentialManager {
    fn new(config: &AppConfig, vault: bool) -> Self {
        Self {
            screen: if vault {
                Screen::Vault
            } else {
                Screen::Providers
            },
            selected: 0,
            input: String::new(),
            password: String::new(),
            error: None,
            notice: String::new(),
            providers: connected_providers(config),
            presets: crate::config::app_config::provider_presets(),
            secrets: Vec::new(),
            draft: None,
            storage: CredentialStorage::Encrypted,
            closing: false,
            search_service: None,
            search_services: std::array::from_fn(|_| SearchServiceState::default()),
            pushover_edit: false,
            pushover_target: None,
            after_unlock: AfterUnlock::Vault,
            secret_id: String::new(),
            secret_label: String::new(),
            secret_kind: SecretKind::Password,
            secret_value: String::new(),
            oauth_url: None,
            oauth_events: None,
            oauth_result: None,
            oauth_cancel: None,
            oauth_response: None,
        }
    }

    fn go(&mut self, screen: Screen) {
        self.screen = screen;
        self.selected = 0;
        self.input.zeroize();
        self.error = None;
    }

    fn choose_storage(&mut self, config: &AppConfig) -> Result<(), String> {
        self.storage = KeyStore::vault_status(config)
            .map_err(|error| error.to_string())?
            .storage;
        self.go(Screen::Storage);
        self.selected = usize::from(self.storage == CredentialStorage::Plaintext);
        Ok(())
    }

    fn preset_choices(&self) -> Vec<&ProviderConfig> {
        self.presets.iter().collect()
    }

    fn accepts_input(&self) -> bool {
        matches!(
            self.screen,
            Screen::Name
                | Screen::Endpoint
                | Screen::Unlock
                | Screen::NewPassword
                | Screen::ConfirmPassword
                | Screen::ApiKey
                | Screen::SecretLabel
                | Screen::SecretValue
                | Screen::Model
                | Screen::OAuthResponse
        )
    }

    fn masked(&self) -> bool {
        matches!(
            self.screen,
            Screen::Unlock
                | Screen::NewPassword
                | Screen::ConfirmPassword
                | Screen::ApiKey
                | Screen::SecretValue
                | Screen::OAuthResponse
        )
    }

    fn list_len(&self) -> usize {
        match self.screen {
            Screen::Providers => self.providers.len() + 1,
            Screen::SearchKeys => SearchBackend::SERVICES.len(),
            Screen::Vault => self.secrets.len() + 7 + PUSHOVER_PRESETS.len(),
            Screen::Presets => self.preset_choices().len() + 1,
            Screen::Storage => 2,
            _ => 1,
        }
    }

    pub(crate) fn paste(&mut self, text: &str) {
        if self.screen == Screen::OAuth {
            self.go(Screen::OAuthResponse);
        }
        if !self.accepts_input() {
            return;
        }
        if self.screen == Screen::OAuthResponse
            && self.input.len().saturating_add(text.len()) > 4096
        {
            self.error = Some("Response URL exceeds 4096 bytes.".into());
            return;
        }
        if self.input.len().saturating_add(text.len()) > 1_048_576 {
            self.error = Some("Input exceeds 1 MiB.".into());
            return;
        }
        if self.screen == Screen::SecretValue {
            self.input.extend(
                text.chars()
                    .filter(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t')),
            );
        } else {
            self.input.extend(text.chars().filter(|c| !c.is_control()));
        }
    }

    pub(crate) fn render(&self, frame: &mut Frame, area: Rect) {
        if self.closing {
            return;
        }
        let popup = crate::tui::components::centered_rect(86, 82, area);
        frame.render_widget(Clear, popup);
        let title = if self.pushover_edit {
            " Pushover TCUI-keystore keys "
        } else if self.search_service.is_some()
            || matches!(self.screen, Screen::SearchKeys | Screen::RemoveSearchKey)
        {
            " Web search API keys "
        } else if matches!(
            self.screen,
            Screen::Vault
                | Screen::SecretLabel
                | Screen::SecretValue
                | Screen::RemoveSecret
                | Screen::Unlock
                | Screen::NewPassword
                | Screen::ConfirmPassword
        ) {
            " Credential vault "
        } else {
            " Providers / Connect "
        };
        let block = Block::default().title(title).borders(Borders::ALL);
        let inner = block.inner(popup);
        frame.render_widget(block, popup);
        let mut lines = Vec::new();
        let mut rows = Vec::new();
        let help = match self.screen {
            Screen::Providers => {
                lines.push(Line::from("Only connections you added appear here."));
                rows.push("+ Add connection".to_string());
                rows.extend(
                    self.providers
                        .iter()
                        .map(|p| format!("{}  [{}]", provider_label(p), p.auth_type)),
                );
                "↑/↓ select · Enter add/reconnect · m set model · d remove · v vault · Esc close"
            }
            Screen::SearchKeys => {
                lines.push(Line::from(self.notice.as_str()));
                lines.push(Line::from("Saving a key enables that MCP profile. h selects its official hosted transport."));
                for (service, state) in SearchBackend::SERVICES.iter().zip(&self.search_services) {
                    rows.push(format!(
                        "{} · {} · {} · {}",
                        service.label(),
                        match state.saved {
                            Some(true) => "key saved",
                            Some(false) => "no key",
                            None => "storage unavailable",
                        },
                        if state.enabled { "enabled" } else { "disabled" },
                        state.transport,
                    ));
                }
                "Enter key · e enable/disable · h hosted preset · d delete key · v vault · Esc close"
            }
            Screen::RemoveSearchKey => {
                lines.push(Line::from(
                    "Delete the selected search API key and disable its MCP profile?",
                ));
                "y confirm deletion · Esc cancel"
            }
            Screen::Presets => {
                lines.push(Line::from(
                    "Choose a preset, or provide a custom OpenAI-compatible endpoint.",
                ));
                rows.extend(
                    self.preset_choices()
                        .iter()
                        .map(|p| provider_label(p).to_string()),
                );
                rows.push("Custom endpoint + API key".into());
                "↑/↓ select · Enter continue · Esc cancel"
            }
            Screen::Vault => {
                lines.push(Line::from(self.notice.as_str()));
                lines.push(Line::from("Defaults affect new provider credentials only. Pushover has its own storage setting; other machine secrets always stay encrypted."));
                rows.extend(
                    [
                        "Unlock / initialize vault",
                        "Lock vault",
                        "Default for NEW credentials: encrypted vault",
                        "Default for NEW credentials: plaintext SQLite (NOT encrypted)",
                        "+ Machine password",
                        "+ SSH private key",
                        "Import legacy credentials (explicit)",
                    ]
                    .into_iter()
                    .map(str::to_string),
                );
                rows.extend(self.secrets.iter().map(|s| {
                    format!(
                        "{}  [{}]",
                        s.label,
                        if s.kind == SecretKind::Password {
                            "password"
                        } else {
                            "SSH key"
                        }
                    )
                }));
                rows.extend(
                    PUSHOVER_PRESETS
                        .iter()
                        .map(|(_, label)| format!("+ Set / replace {label}")),
                );
                "↑/↓ select · Enter edit/apply · c copy · d delete · s search keys · p providers · Esc close"
            }
            Screen::Storage => {
                lines.push(Line::from("Choose storage for this connection only. Other saved credentials are not converted. Plaintext credentials remain readable while locked."));
                rows.extend(
                    [
                        "Password-encrypted tcui-credentials vault",
                        "Plaintext application SQLite database (NOT encrypted)",
                    ]
                    .into_iter()
                    .map(str::to_string),
                );
                "↑/↓ select · Enter choose · Esc cancel"
            }
            Screen::Name => {
                lines.push(Line::from("Custom connection name (unique):"));
                "Enter continue · Esc cancel"
            }
            Screen::Endpoint => {
                lines.push(Line::from("Endpoint URL: HTTPS, or HTTP on loopback only. No URL credentials, query or fragment."));
                "Enter continue · Esc cancel"
            }
            Screen::Unlock => {
                lines.push(Line::from("Unlock tcui-credentials. Cancel keeps encrypted connections unavailable; the app remains usable."));
                "Enter unlock · Esc cancel"
            }
            Screen::NewPassword => {
                lines.push(Line::from("Create a vault password (at least 8 characters). Keep it safe: lost passwords cannot be recovered."));
                "Enter continue · Esc cancel"
            }
            Screen::ConfirmPassword => {
                lines.push(Line::from("Confirm the new vault password:"));
                "Enter initialize · Esc cancel"
            }
            Screen::ApiKey => {
                lines.push(Line::from(format!(
                    "API key for {}:",
                    self.search_service
                        .map(|service| std::borrow::Cow::Borrowed(service.label()))
                        .or_else(|| self.draft.as_ref().map(provider_label))
                        .unwrap_or_else(|| "provider".into())
                )));
                lines.push(Line::from("Saved only in your selected credential storage. Never read from environment files."));
                if self
                    .draft
                    .as_ref()
                    .is_some_and(|provider| provider.backend_type == "anthropic_sdk")
                {
                    lines.push(Line::from("Use an Anthropic Console API key. This uses Console credits, not a Claude subscription."));
                }
                "Enter save credential · Esc cancel"
            }
            Screen::OAuth | Screen::OAuthResponse => {
                lines.push(Line::from(
                    "Open the sign-in URL in your browser. No browser is launched on this machine.",
                ));
                lines.push(Line::from(self.notice.as_str()));
                if let Some(url) = &self.oauth_url {
                    lines.push(Line::from(url.as_str()));
                }
                if self.screen == Screen::OAuthResponse {
                    lines.push(Line::from("Paste the complete final response URL (masked), even if localhost cannot load:"));
                    "Enter submit response URL · Ctrl+U clear · Esc cancel"
                } else {
                    "c copy sign-in URL · p enter response URL · paste final URL · Esc cancel"
                }
            }
            Screen::SecretLabel => {
                lines.push(Line::from(
                    "Machine secret label (secrets always use the encrypted vault):",
                ));
                "Enter continue · Esc cancel"
            }
            Screen::SecretValue => {
                lines.push(Line::from(format!(
                    "Secret for {} (masked):",
                    self.secret_label
                )));
                if is_pushover_secret(&self.secret_id) {
                    lines.push(Line::from("Enter the 30-character Pushover key."));
                    lines.push(Line::from(
                        if self.storage == CredentialStorage::Encrypted {
                            "Storage: password-encrypted TCUI keystore."
                        } else {
                            "WARNING: storage is PLAINTEXT SQLite; readable while locked."
                        },
                    ));
                    lines.push(Line::from(self.notice.as_str()));
                    lines.push(Line::from(
                        "Model key reads disclose keys to the configured AI provider.",
                    ));
                    lines.push(Line::from(
                        "Scheduled jobs keep private plaintext copies; see settings.",
                    ));
                } else {
                    lines.push(Line::from("Paste a complete multiline SSH private key. Existing value can be replaced with Ctrl+U."));
                }
                "Enter save · Shift+Enter newline · Ctrl+U clear · Esc cancel"
            }
            Screen::PushoverSaved => {
                lines.push(Line::from(format!(
                    "Saved {} in the {} TCUI keystore.",
                    self.secret_label,
                    if self.storage == CredentialStorage::Encrypted {
                        "encrypted"
                    } else {
                        "PLAINTEXT"
                    }
                )));
                lines.push(Line::from(
                    "Other application tokens are unchanged. No key is stored in settings.",
                ));
                "Esc return to Pushover settings"
            }
            Screen::RemoveProvider => {
                lines.push(Line::from(
                    "Remove this connection and its saved credentials? This cannot be undone.",
                ));
                "y confirm removal · Esc cancel"
            }
            Screen::RemoveSecret => {
                lines.push(Line::from(
                    "Permanently delete the selected machine secret? This cannot be undone.",
                ));
                "y confirm deletion · Esc cancel"
            }
            Screen::Model => {
                lines.push(Line::from("Enter the model ID from your provider. This also supports custom endpoints without a model-list API."));
                "Enter select model · Esc cancel"
            }
        };
        lines.push(Line::from(""));
        let available = inner.height.saturating_sub(7) as usize;
        let start = self.selected.saturating_sub(available.saturating_sub(1));
        for (index, row) in rows.iter().enumerate().skip(start).take(available) {
            lines.push(Line::styled(
                format!("{} {row}", if index == self.selected { "›" } else { " " }),
                if index == self.selected {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                },
            ));
        }
        let mut remaining = inner;
        if self.accepts_input() {
            let heading = Paragraph::new(lines).wrap(Wrap { trim: false });
            let heading_height = inner.height.saturating_sub(6);
            frame.render_widget(
                heading,
                Rect::new(inner.x, inner.y, inner.width, heading_height),
            );
            remaining.y = remaining.y.saturating_add(heading_height);
            remaining.height = remaining.height.saturating_sub(heading_height);
            lines = Vec::new();
            let visible = if self.masked() {
                std::borrow::Cow::Owned("•".repeat(self.input.chars().count().min(64)))
            } else {
                std::borrow::Cow::Borrowed(self.input.as_str())
            };
            if remaining.width > 2 && remaining.height > 0 {
                let layout = crate::ui::chat_tab::input_layout(
                    &visible,
                    visible.chars().count(),
                    0,
                    remaining.width.saturating_sub(2) as usize,
                    1,
                    false,
                );
                let text = layout
                    .visible_lines
                    .first()
                    .map(String::as_str)
                    .unwrap_or_default();
                frame.render_widget(
                    Paragraph::new(format!("> {text}")),
                    Rect::new(remaining.x, remaining.y, remaining.width, 1),
                );
                frame.set_cursor_position((remaining.x + 2 + layout.cursor_x, remaining.y));
                remaining.y += 1;
                remaining.height -= 1;
            }
            if self.screen == Screen::SecretValue {
                lines.push(Line::from(format!(
                    "{} bytes · {} lines",
                    self.input.len(),
                    self.input.lines().count()
                )));
            }
        }
        if let Some(error) = &self.error {
            lines.push(Line::styled(
                error.as_str(),
                Style::default().fg(Color::Red),
            ));
        }
        lines.push(Line::from(""));
        lines.push(Line::styled(help, Style::default().fg(Color::DarkGray)));
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), remaining);
    }
}

fn connected_providers(config: &AppConfig) -> Vec<ProviderConfig> {
    config
        .providers
        .iter()
        .filter(|provider| KeyStore::has_credential(config, &provider.name).unwrap_or(false))
        .cloned()
        .collect()
}

fn provider_label(provider: &ProviderConfig) -> std::borrow::Cow<'_, str> {
    crate::ui::provider_display_name(&provider.name)
}

fn validate_endpoint(endpoint: &str) -> Result<(), &'static str> {
    let url = reqwest::Url::parse(endpoint).map_err(|_| "Enter a valid absolute endpoint URL.")?;
    if endpoint.split_once("://").is_some_and(|(_, rest)| {
        rest.split('/')
            .next()
            .is_some_and(|authority| authority.contains('@'))
    }) {
        return Err("Endpoint must not contain URL credentials.");
    }
    let loopback = url.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    if url.host_str().is_none()
        || !(url.scheme() == "https" || (url.scheme() == "http" && loopback))
    {
        return Err("Endpoint must use HTTPS, or HTTP on localhost / a loopback IP.");
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("Endpoint must not contain URL credentials, a query or a fragment.");
    }
    Ok(())
}

impl TuiApp {
    pub(crate) async fn stop_credential_login(&mut self) {
        let Some(manager) = self.ui.credential_manager.as_mut() else {
            return;
        };
        if let Some(cancel) = manager.oauth_cancel.take() {
            cancel.cancel();
        }
        if let Some(task) = manager.oauth_result.take() {
            task.abort();
            let _ = task.await;
        }
        manager.oauth_response = None;
        manager.oauth_events = None;
    }

    pub(crate) fn open_credential_manager(&mut self, vault: bool) {
        let Ok(config) = self.config.try_read().map(|c| c.clone()) else {
            return;
        };
        let mut manager = CredentialManager::new(&config, vault);
        if vault {
            self.refresh_vault_manager(&config, &mut manager);
        }
        self.ui.palette = None;
        self.ui.settings_v2 = None;
        self.ui.credential_manager = Some(manager);
    }

    pub(crate) fn open_pushover_credential(&mut self, application: bool) {
        let Ok(config) = self.config.try_read().map(|config| config.clone()) else {
            self.ui
                .settings_workspace
                .fail("Configuration is busy. Retry opening the key editor.");
            return;
        };
        let target = if application {
            PushoverCredentialTarget::Application(config.notifications.pushover.application.clone())
        } else {
            PushoverCredentialTarget::User
        };
        self.show_pushover_credential(&config, target);
    }

    fn show_pushover_credential(&mut self, config: &AppConfig, target: PushoverCredentialTarget) {
        let mut manager = CredentialManager::new(config, true);
        manager.pushover_edit = true;
        manager.storage = if config.notifications.pushover.encrypt_credentials {
            CredentialStorage::Encrypted
        } else {
            CredentialStorage::Plaintext
        };
        manager.notice = if config.notifications.pushover.use_vault {
            "Delivery source: TCUI keystore."
        } else {
            "Delivery source: environment/file; keystore keys are not selected."
        }
        .into();
        let result = (|| {
            let (id, label) = match &target {
                PushoverCredentialTarget::User => (USER_KEY_ID.into(), "Pushover user key".into()),
                PushoverCredentialTarget::Application(application) => (
                    pushover::application_key_id(application).map_err(|error| error.to_string())?,
                    format!("Pushover application token ({application})"),
                ),
            };
            manager.secret_id = id;
            manager.secret_label = label;
            manager.secret_kind = SecretKind::Password;
            manager.pushover_target = Some(target);
            self.begin_pushover_input(config, &mut manager)
        })();
        if let Err(error) = result {
            manager.error = Some(error);
        }
        self.ui.palette = None;
        self.ui.settings_v2 = None;
        self.ui.credential_manager = Some(manager);
    }

    fn begin_pushover_input(
        &mut self,
        config: &AppConfig,
        manager: &mut CredentialManager,
    ) -> Result<(), String> {
        manager.notice = if config.notifications.pushover.use_vault {
            "Delivery source: TCUI keystore."
        } else {
            "Delivery source: environment/file; keystore keys are not selected."
        }
        .into();
        let existing = KeyStore::pushover_storage(config, &manager.secret_id)
            .map_err(|error| error.to_string())?;
        let needs_unlock = manager.storage == CredentialStorage::Encrypted
            || existing == Some(CredentialStorage::Encrypted);
        if needs_unlock
            && !KeyStore::vault_status(config)
                .map_err(|error| error.to_string())?
                .unlocked
        {
            self.request_vault_unlock(config, manager, AfterUnlock::Pushover)
        } else {
            manager.go(Screen::SecretValue);
            Ok(())
        }
    }

    pub(crate) fn take_pushover_credential_restart(&mut self) -> Option<PushoverCredentialTarget> {
        use crate::protection::ui::SecurityRestart;
        if !matches!(
            self.security_restart,
            Some(SecurityRestart::Unlock | SecurityRestart::InitializePassword)
        ) {
            return None;
        }
        let manager = self.ui.credential_manager.as_mut()?;
        if manager.closing && matches!(manager.after_unlock, AfterUnlock::Pushover) {
            manager.pushover_target.take()
        } else {
            None
        }
    }

    pub(crate) fn resume_pushover_credential(&mut self, target: PushoverCredentialTarget) {
        let Ok(config) = self.config.try_read().map(|config| config.clone()) else {
            return;
        };
        if !KeyStore::vault_status(&config).is_ok_and(|status| status.unlocked) {
            return;
        }
        let id = match &target {
            PushoverCredentialTarget::User => "pushover_user_key",
            PushoverCredentialTarget::Application(_) => "pushover_app_key",
        };
        self.open_settings_page(Some(id));
        self.show_pushover_credential(&config, target);
    }

    pub(crate) fn open_search_credentials(&mut self) {
        let Ok(config) = self.config.try_read().map(|config| config.clone()) else {
            return;
        };
        let mut manager = CredentialManager::new(&config, false);
        manager.go(Screen::SearchKeys);
        self.refresh_search_keys(&config, &mut manager);
        self.ui.palette = None;
        self.ui.settings_v2 = None;
        self.ui.credential_manager = Some(manager);
    }

    fn search_key_name(config: &AppConfig, service: SearchBackend) -> Result<&'static str, String> {
        let name = crate::mcp::profile_by_skill(service.id())
            .and_then(|profile| profile.key_store_name)
            .ok_or("Search service credential profile is unavailable.")?;
        if config
            .providers
            .iter()
            .any(|provider| provider.name.eq_ignore_ascii_case(name))
        {
            return Err("A chat connection uses this reserved search credential name. Remove or rename that connection first.".into());
        }
        Ok(name)
    }

    fn refresh_search_keys(&self, config: &AppConfig, manager: &mut CredentialManager) {
        let servers = crate::mcp::merged_configs(&config.mcp_servers);
        for (index, service) in SearchBackend::SERVICES.into_iter().enumerate() {
            let saved = Self::search_key_name(config, service).and_then(|name| {
                KeyStore::has_credential(config, name).map_err(|error| error.to_string())
            });
            manager.search_services[index].saved = match saved {
                Ok(saved) => Some(saved),
                Err(error) => {
                    manager.error = Some(error);
                    None
                }
            };
            if let Some(server) = servers.iter().find(|server| {
                crate::mcp::lookup_profile(&server.name)
                    .is_some_and(|profile| profile.skill == service.id())
            }) {
                manager.search_services[index].enabled = server.enabled;
                manager.search_services[index]
                    .transport
                    .clone_from(&server.transport);
            }
        }
        manager.notice = "Choose storage for each key. Existing credentials are never converted by adding a connection. Environment keys are never imported. Select the automatic source in Web search settings.".into();
    }

    fn set_search_profile(
        &mut self,
        service: SearchBackend,
        enabled: bool,
        hosted_preset: bool,
    ) -> Result<AppConfig, String> {
        let profile = crate::mcp::profile_by_skill(service.id())
            .ok_or("Search service profile is unavailable.")?;
        let mut current = self
            .config
            .try_write()
            .map_err(|_| "Configuration is busy. Retry.")?;
        let mut updated = current.clone();
        updated.mcp_servers = crate::mcp::merged_configs(&updated.mcp_servers);
        let server = updated
            .mcp_servers
            .iter_mut()
            .find(|server| {
                crate::mcp::lookup_profile(&server.name)
                    .is_some_and(|candidate| candidate.skill == service.id())
            })
            .ok_or("Search service configuration is unavailable.")?;
        if hosted_preset {
            *server = profile.config(enabled);
        } else {
            server.enabled = enabled;
        }
        updated
            .save()
            .map_err(|_| "Could not save MCP search configuration. Retry.")?;
        self.ui.settings_workspace.load(&updated);
        *current = updated.clone();
        Ok(updated)
    }

    pub(crate) fn open_manual_model(&mut self, model: &str) {
        let Ok(config) = self.config.try_read().map(|c| c.clone()) else {
            return;
        };
        let provider = self
            .ui
            .tabs
            .get(self.ui.active_tab)
            .map(|tab| tab.tab.provider.as_str())
            .unwrap_or_default();
        let Some(provider) = config
            .providers
            .iter()
            .find(|entry| entry.name == provider)
            .cloned()
        else {
            self.open_credential_manager(false);
            return;
        };
        let mut manager = CredentialManager::new(&config, false);
        manager.draft = Some(provider);
        manager.go(Screen::Model);
        manager.input = model.to_string();
        if !model.is_empty() {
            if let Err(error) = self.submit_credential_input(&config, &mut manager) {
                manager.error = Some(error);
            }
        }
        self.ui.credential_manager = Some(manager);
    }

    fn refresh_vault_manager(&self, config: &AppConfig, manager: &mut CredentialManager) {
        manager.providers = connected_providers(config);
        match KeyStore::vault_status(config) {
            Ok(status) => {
                manager.notice = format!(
                    "Vault: {} · new credential default: {}{}",
                    if !status.initialized {
                        "not initialized"
                    } else if status.unlocked {
                        "unlocked"
                    } else {
                        "locked"
                    },
                    if status.storage == CredentialStorage::Encrypted {
                        "encrypted vault"
                    } else {
                        "plaintext SQLite"
                    },
                    if status.legacy_present {
                        " · legacy credentials available for explicit import"
                    } else {
                        ""
                    }
                );
                manager.secrets.clear();
                if status.unlocked {
                    match KeyStore::list_secrets(config) {
                        Ok(secrets) => manager.secrets = secrets,
                        Err(error) => manager.error = Some(error.to_string()),
                    }
                }
            }
            Err(error) => manager.error = Some(error.to_string()),
        }
    }

    fn request_application_password(
        &mut self,
        manager: &mut CredentialManager,
        initialize: bool,
    ) -> Result<(), String> {
        if initialize {
            self.initialize_application_password()?;
        } else {
            self.unlock_application();
        }
        manager.input.zeroize();
        manager.password.zeroize();
        manager.secret_value.zeroize();
        manager.draft = None;
        manager.closing = true;
        Ok(())
    }

    fn request_vault_unlock(
        &mut self,
        config: &AppConfig,
        manager: &mut CredentialManager,
        after: AfterUnlock,
    ) -> Result<(), String> {
        let status = KeyStore::vault_status(config).map_err(|error| error.to_string())?;
        manager.after_unlock = after;
        if crate::protection::configured().map_err(|failure| {
            let error = failure.to_string();
            manager.input.zeroize();
            self.lock_application_after_error(error.clone());
            error
        })? {
            return self.request_application_password(manager, !status.initialized);
        }
        manager.go(if status.initialized {
            Screen::Unlock
        } else {
            Screen::NewPassword
        });
        Ok(())
    }

    fn begin_provider_credentials(config: &AppConfig, manager: &mut CredentialManager) {
        if let Some(provider) = &manager.draft {
            crate::llm::auth::invalidate_provider_refreshes(&provider.name);
        }
        if manager
            .draft
            .as_ref()
            .is_some_and(|p| p.auth_type == "oauth")
        {
            let Some(provider) = manager.draft.as_ref().map(|p| p.name.clone()) else {
                return;
            };
            let (events_tx, events_rx) = tokio::sync::mpsc::unbounded_channel();
            let (cancellation, cancel) = crate::llm::auth::tui::oauth_cancellation();
            let (response_tx, response_rx) = tokio::sync::mpsc::channel(1);
            let snapshot = config.clone();
            let storage = manager.storage;
            manager.oauth_result = Some(tokio::spawn(async move {
                crate::llm::auth::tui::login(
                    &provider,
                    snapshot,
                    storage,
                    events_tx,
                    cancellation,
                    response_rx,
                )
                .await
            }));
            manager.oauth_events = Some(events_rx);
            manager.oauth_cancel = Some(cancel);
            manager.oauth_response = Some(response_tx);
            manager.oauth_url = None;
            manager.notice.clear();
            manager.go(Screen::OAuth);
        } else {
            manager.go(Screen::ApiKey);
        }
    }

    fn after_vault_unlock(
        &mut self,
        config: &AppConfig,
        manager: &mut CredentialManager,
    ) -> Result<(), String> {
        match manager.after_unlock {
            AfterUnlock::Provider => {
                Self::begin_provider_credentials(config, manager);
            }
            AfterUnlock::Storage => {
                KeyStore::set_storage_default(config, CredentialStorage::Encrypted)
                    .map_err(|e| e.to_string())?;
                manager.go(Screen::Vault);
                self.refresh_vault_manager(config, manager);
            }
            AfterUnlock::Vault => {
                manager.go(Screen::Vault);
                self.refresh_vault_manager(config, manager);
            }
            AfterUnlock::Pushover => manager.go(Screen::SecretValue),
        }
        if !matches!(manager.after_unlock, AfterUnlock::Provider) {
            self.refresh_visible_selectors();
            if tokio::runtime::Handle::try_current().is_ok() {
                for provider in &manager.providers {
                    if !config
                        .disabled_providers
                        .iter()
                        .any(|name| name == &provider.name)
                    {
                        self.refresh_models_for_provider(provider.name.clone());
                    }
                }
            }
        }
        self.queue_connection_check_for_active_tab();
        Ok(())
    }

    fn persist_connection(&mut self, manager: &mut CredentialManager) -> Result<(), String> {
        let provider = manager
            .draft
            .as_ref()
            .ok_or("No provider selected.")?
            .clone();
        let mut config = self
            .config
            .try_write()
            .map_err(|_| "Configuration is busy. Please retry.")?;
        let mut updated = config.clone();
        if let Some(existing) = updated
            .providers
            .iter_mut()
            .find(|p| p.name.eq_ignore_ascii_case(&provider.name))
        {
            *existing = provider.clone();
        } else {
            updated.providers.push(provider.clone());
        }
        updated
            .disabled_providers
            .retain(|name| !name.eq_ignore_ascii_case(&provider.name));
        updated
            .save()
            .map_err(|_| "Could not save provider configuration. Retry saving this connection.")?;
        self.storage
            .sync_providers(&updated.providers)
            .map_err(|_| "Could not update provider catalog. Retry saving this connection.")?;
        *config = updated.clone();
        drop(config);
        manager.providers = connected_providers(&updated);
        self.ui.db_providers = Self::provider_entries_with_local(&updated, None);
        self.ui.disabled_providers = updated.disabled_providers.iter().cloned().collect();
        if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
            tab.tab.provider = provider.name.clone();
            tab.tab.endpoint = None;
            tab.tab.model.clear();
            tab.manual_model_override = false;
        }
        self.refresh_visible_selectors();
        if tokio::runtime::Handle::try_current().is_ok() {
            self.refresh_models_for_provider(provider.name);
        }
        self.queue_connection_check_for_active_tab();
        manager.draft = None;
        manager.go(Screen::Providers);
        Ok(())
    }

    pub(crate) fn handle_credential_manager_key(&mut self, key: KeyEvent) {
        if self.security_restart.is_some() {
            return;
        }
        let Some(mut manager) = self.ui.credential_manager.take() else {
            return;
        };
        if manager.closing {
            self.ui.credential_manager = Some(manager);
            return;
        }
        if key.code == KeyCode::Esc
            || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
        {
            if let Some(task) = &manager.oauth_result {
                if let Some(cancel) = &manager.oauth_cancel {
                    cancel.cancel();
                }
                task.abort();
                manager.closing = true;
                self.ui.credential_manager = Some(manager);
            }
            return;
        }
        let Ok(config) = self.config.try_read().map(|c| c.clone()) else {
            manager.error = Some("Configuration is busy. Please retry.".into());
            self.ui.credential_manager = Some(manager);
            return;
        };
        let result = self.credential_manager_key(&config, &mut manager, key);
        if let Err(error) = result {
            manager.error = Some(error);
        }
        self.ui.credential_manager = Some(manager);
    }

    fn credential_manager_key(
        &mut self,
        config: &AppConfig,
        manager: &mut CredentialManager,
        key: KeyEvent,
    ) -> Result<(), String> {
        if key.code == KeyCode::Char('p') && manager.screen == Screen::OAuth {
            manager.go(Screen::OAuthResponse);
            return Ok(());
        }
        if manager.accepts_input() {
            match key.code {
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    manager.input.zeroize()
                }
                KeyCode::Char(c)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    if manager.screen == Screen::OAuthResponse
                        && manager.input.len().saturating_add(c.len_utf8()) > 4096
                    {
                        return Err("Response URL exceeds 4096 bytes.".into());
                    }
                    manager.input.push(c)
                }
                KeyCode::Backspace => {
                    manager.input.pop();
                }
                KeyCode::Enter
                    if key.modifiers.contains(KeyModifiers::SHIFT)
                        && manager.screen == Screen::SecretValue =>
                {
                    manager.input.push('\n')
                }
                KeyCode::Enter => return self.submit_credential_input(config, manager),
                _ => {}
            }
            return Ok(());
        }
        match key.code {
            KeyCode::Char('c') if manager.screen == Screen::OAuth => {
                let url = manager
                    .oauth_url
                    .as_deref()
                    .ok_or("The sign-in URL is not available yet.")?;
                crate::clipboard::copy(url).map_err(|_| "Could not copy the sign-in URL.")?;
                manager.notice = "Sign-in URL copied to clipboard. Your clipboard may retain this temporary URL.".into();
            }
            KeyCode::Up => manager.selected = manager.selected.saturating_sub(1),
            KeyCode::Down => {
                manager.selected = (manager.selected + 1).min(manager.list_len().saturating_sub(1))
            }
            KeyCode::Char('v')
                if matches!(manager.screen, Screen::Providers | Screen::SearchKeys) =>
            {
                manager.go(Screen::Vault);
                self.refresh_vault_manager(config, manager);
            }
            KeyCode::Char('p') if manager.screen == Screen::Vault => manager.go(Screen::Providers),
            KeyCode::Char('s') if matches!(manager.screen, Screen::Vault | Screen::Providers) => {
                manager.search_service = None;
                manager.draft = None;
                manager.go(Screen::SearchKeys);
                self.refresh_search_keys(config, manager);
            }
            KeyCode::Char('e' | 'h') if manager.screen == Screen::SearchKeys => {
                let service = SearchBackend::SERVICES[manager.selected];
                let hosted = key.code == KeyCode::Char('h');
                let enabled = manager.search_services[manager.selected].enabled;
                let updated = self.set_search_profile(
                    service,
                    if hosted { enabled } else { !enabled },
                    hosted,
                )?;
                self.refresh_search_keys(&updated, manager);
            }
            KeyCode::Char('d') if manager.screen == Screen::SearchKeys => {
                manager.search_service = Some(SearchBackend::SERVICES[manager.selected]);
                manager.go(Screen::RemoveSearchKey);
            }
            KeyCode::Char('y') if manager.screen == Screen::RemoveSearchKey => {
                let service = manager
                    .search_service
                    .ok_or("Select a search service first.")?;
                KeyStore::remove(config, Self::search_key_name(config, service)?)
                    .map_err(|error| error.to_string())?;
                let updated = self
                    .set_search_profile(service, false, false)
                    .map_err(|error| {
                        format!("Key removed, but its profile could not be disabled: {error}")
                    })?;
                manager.search_service = None;
                manager.go(Screen::SearchKeys);
                self.refresh_search_keys(&updated, manager);
            }
            KeyCode::Char('m') if manager.screen == Screen::Providers && manager.selected > 0 => {
                manager.draft = manager.providers.get(manager.selected - 1).cloned();
                manager.go(Screen::Model);
            }
            KeyCode::Char('d') if manager.screen == Screen::Providers && manager.selected > 0 => {
                manager.screen = Screen::RemoveProvider
            }
            KeyCode::Char('d')
                if manager.screen == Screen::Vault
                    && (7..7 + manager.secrets.len()).contains(&manager.selected) =>
            {
                manager.screen = Screen::RemoveSecret
            }
            KeyCode::Char('c')
                if manager.screen == Screen::Vault
                    && (7..7 + manager.secrets.len()).contains(&manager.selected) =>
            {
                let secret = manager
                    .secrets
                    .get(manager.selected - 7)
                    .ok_or("Select a secret first.")?;
                let value = zeroize::Zeroizing::new(
                    KeyStore::get_secret(config, &secret.id)
                        .map_err(|e| e.to_string())?
                        .ok_or("Secret no longer exists.")?,
                );
                crate::clipboard::copy(&value)
                    .map_err(|_| "Could not copy the secret to the clipboard.")?;
                manager.notice = "Secret copied to clipboard. Your clipboard may retain it.".into();
            }
            KeyCode::Char('y') if manager.screen == Screen::RemoveProvider => {
                let provider = manager
                    .providers
                    .get(manager.selected.saturating_sub(1))
                    .ok_or("Select a connection first.")?;
                crate::llm::auth::invalidate_provider_refreshes(&provider.name);
                let mut current = self
                    .config
                    .try_write()
                    .map_err(|_| "Configuration is busy.")?;
                KeyStore::remove(config, &provider.name).map_err(|e| e.to_string())?;
                let mut updated = current.clone();
                updated.providers.retain(|p| p.name != provider.name);
                updated
                    .disabled_providers
                    .retain(|name| name != &provider.name);
                updated
                    .save()
                    .map_err(|_| "Could not save provider removal. Please retry.")?;
                self.storage
                    .sync_providers(&updated.providers)
                    .map_err(|_| "Could not update provider catalog.")?;
                *current = updated.clone();
                drop(current);
                manager.providers = connected_providers(&updated);
                self.ui.db_providers = Self::provider_entries_with_local(&updated, None);
                self.ui.disabled_providers = updated.disabled_providers.iter().cloned().collect();
                self.refresh_visible_selectors();
                self.queue_connection_check_for_active_tab();
                manager.go(Screen::Providers);
            }
            KeyCode::Char('y') if manager.screen == Screen::RemoveSecret => {
                let secret = manager
                    .secrets
                    .get(manager.selected.saturating_sub(7))
                    .ok_or("Select a secret first.")?;
                KeyStore::remove_secret(config, &secret.id).map_err(|e| e.to_string())?;
                manager.go(Screen::Vault);
                self.refresh_vault_manager(config, manager);
            }
            KeyCode::Enter => self.activate_credential_selection(config, manager)?,
            _ => {}
        }
        Ok(())
    }

    fn activate_credential_selection(
        &mut self,
        config: &AppConfig,
        manager: &mut CredentialManager,
    ) -> Result<(), String> {
        match manager.screen {
            Screen::SearchKeys => {
                manager.search_service = Some(SearchBackend::SERVICES[manager.selected]);
                manager.draft = None;
                manager.choose_storage(config)?;
            }
            Screen::Providers if manager.selected == 0 => manager.go(Screen::Presets),
            Screen::Providers => {
                manager.draft = manager.providers.get(manager.selected - 1).cloned();
                manager.choose_storage(config)?;
            }
            Screen::Presets => {
                let provider = manager
                    .preset_choices()
                    .get(manager.selected)
                    .map(|p| (*p).clone());
                manager.draft = provider;
                if manager.draft.is_some() {
                    manager.choose_storage(config)?;
                } else {
                    manager.go(Screen::Name);
                }
            }
            Screen::Storage => {
                manager.storage = if manager.selected == 0 {
                    CredentialStorage::Encrypted
                } else {
                    CredentialStorage::Plaintext
                };
                if let Some(provider) = &mut manager.draft {
                    provider.name =
                        KeyStore::connection_id(config, &provider.name, manager.storage)
                            .map_err(|e| e.to_string())?;
                }
                if manager.storage == CredentialStorage::Encrypted
                    && !KeyStore::vault_status(config)
                        .map_err(|e| e.to_string())?
                        .unlocked
                {
                    self.request_vault_unlock(config, manager, AfterUnlock::Provider)?;
                } else {
                    Self::begin_provider_credentials(config, manager);
                }
            }
            Screen::Vault => match manager.selected {
                0 => self.request_vault_unlock(config, manager, AfterUnlock::Vault)?,
                1 => {
                    self.lock_application()?;
                    self.refresh_vault_manager(config, manager);
                    self.queue_connection_check_for_active_tab();
                }
                2 => {
                    if KeyStore::vault_status(config)
                        .map_err(|e| e.to_string())?
                        .unlocked
                    {
                        KeyStore::set_storage_default(config, CredentialStorage::Encrypted)
                            .map_err(|e| e.to_string())?;
                        self.refresh_vault_manager(config, manager);
                    } else {
                        self.request_vault_unlock(config, manager, AfterUnlock::Storage)?;
                    }
                }
                3 => {
                    KeyStore::set_storage_default(config, CredentialStorage::Plaintext)
                        .map_err(|e| e.to_string())?;
                    self.refresh_vault_manager(config, manager);
                }
                4 | 5 => {
                    if !KeyStore::vault_status(config)
                        .map_err(|e| e.to_string())?
                        .unlocked
                    {
                        return Err("Unlock or initialize the vault first.".into());
                    }
                    manager.secret_kind = if manager.selected == 4 {
                        SecretKind::Password
                    } else {
                        SecretKind::SshKey
                    };
                    manager.secret_id.clear();
                    manager.secret_value.zeroize();
                    manager.go(Screen::SecretLabel);
                }
                6 => {
                    let count = KeyStore::migrate_legacy(config).map_err(|e| e.to_string())?;
                    self.refresh_vault_manager(config, manager);
                    self.refresh_visible_selectors();
                    self.queue_connection_check_for_active_tab();
                    manager
                        .notice
                        .push_str(&format!(" · imported {count} credential(s)"));
                }
                index if index >= 7 + manager.secrets.len() => {
                    manager.storage = if config.notifications.pushover.encrypt_credentials {
                        CredentialStorage::Encrypted
                    } else {
                        CredentialStorage::Plaintext
                    };
                    let (id, label) = PUSHOVER_PRESETS[index - 7 - manager.secrets.len()];
                    manager.secret_id = id.into();
                    manager.secret_label = label.into();
                    manager.secret_kind = SecretKind::Password;
                    manager.secret_value.zeroize();
                    // Replacement starts blank: never reveal the saved key, even temporarily.
                    self.begin_pushover_input(config, manager)?;
                }
                index => {
                    let secret = manager
                        .secrets
                        .get(index - 7)
                        .ok_or("Select a secret first.")?;
                    manager.secret_id = secret.id.clone();
                    manager.secret_label = secret.label.clone();
                    manager.secret_kind = secret.kind;
                    if is_pushover_secret(&manager.secret_id) {
                        manager.secret_value.zeroize();
                        manager.storage = if config.notifications.pushover.encrypt_credentials {
                            CredentialStorage::Encrypted
                        } else {
                            CredentialStorage::Plaintext
                        };
                        self.begin_pushover_input(config, manager)?;
                        return Ok(());
                    }
                    manager.secret_value = KeyStore::get_secret(config, &secret.id)
                        .map_err(|e| e.to_string())?
                        .ok_or("Secret no longer exists.")?;
                    manager.go(Screen::SecretLabel);
                    manager.input = manager.secret_label.clone();
                }
            },
            _ => {}
        }
        Ok(())
    }

    fn submit_credential_input(
        &mut self,
        config: &AppConfig,
        manager: &mut CredentialManager,
    ) -> Result<(), String> {
        match manager.screen {
            Screen::Name => {
                let name = manager.input.trim();
                if name.is_empty() || name.len() > 80 || name.chars().any(char::is_control) {
                    return Err(
                        "Use a nonempty name of at most 80 bytes, without control characters."
                            .into(),
                    );
                }
                if name.eq_ignore_ascii_case(crate::config::LOCAL_PROVIDER_NAME)
                    || name.contains("::")
                    || config
                        .providers
                        .iter()
                        .chain(manager.presets.iter())
                        .any(|p| p.name.eq_ignore_ascii_case(name))
                    || crate::mcp::profiles().iter().any(|profile| {
                        profile
                            .key_store_name
                            .is_some_and(|key| key.eq_ignore_ascii_case(name))
                    })
                {
                    return Err("That name is already used or reserved for a preset.".into());
                }
                manager.draft = Some(ProviderConfig {
                    name: name.to_string(),
                    endpoint: String::new(),
                    env_var: String::new(),
                    backend_type: "openai".into(),
                    auth_type: "api_key".into(),
                });
                manager.go(Screen::Endpoint);
            }
            Screen::Endpoint => {
                validate_endpoint(manager.input.trim())?;
                if let Some(provider) = &mut manager.draft {
                    provider.endpoint = manager.input.trim().to_string();
                }
                manager.choose_storage(config)?;
            }
            Screen::Unlock => {
                if crate::protection::configured().map_err(|failure| {
                    let error = failure.to_string();
                    manager.input.zeroize();
                    self.lock_application_after_error(error.clone());
                    error
                })? {
                    let initialized = KeyStore::vault_status(config)
                        .map_err(|error| error.to_string())?
                        .initialized;
                    return self.request_application_password(manager, !initialized);
                }
                let result = KeyStore::unlock_vault(config, &manager.input);
                manager.input.zeroize();
                if let Err(failure) = result {
                    let error = failure.to_string();
                    self.lock_application_after_error(error.clone());
                    return Err(error);
                }
                self.after_vault_unlock(config, manager)?;
            }
            Screen::NewPassword => {
                if manager.input.chars().count() < 8 {
                    return Err("Use at least 8 characters for the vault password.".into());
                }
                manager.password = std::mem::take(&mut manager.input);
                manager.go(Screen::ConfirmPassword);
            }
            Screen::ConfirmPassword => {
                if manager.input != manager.password {
                    return Err("Passwords do not match. Clear with Ctrl+U and retry.".into());
                }
                KeyStore::initialize_vault(config, &manager.password).map_err(|e| e.to_string())?;
                manager.password.zeroize();
                manager.input.zeroize();
                self.after_vault_unlock(config, manager)?;
            }
            Screen::ApiKey => {
                if manager.input.trim().is_empty() {
                    return Err("Enter an API key.".into());
                }
                if let Some(service) = manager.search_service {
                    KeyStore::set_with_storage(
                        config,
                        Self::search_key_name(config, service)?,
                        manager.input.trim(),
                        manager.storage,
                    )
                    .map_err(|error| error.to_string())?;
                    let updated =
                        self.set_search_profile(service, true, false)
                            .map_err(|error| {
                                format!(
                                    "Key saved, but its MCP profile could not be enabled: {error}"
                                )
                            })?;
                    manager.search_service = None;
                    manager.go(Screen::SearchKeys);
                    self.refresh_search_keys(&updated, manager);
                    return Ok(());
                }
                let provider = manager.draft.as_ref().ok_or("Select a provider first.")?;
                KeyStore::set_with_storage(
                    config,
                    &provider.name,
                    manager.input.trim(),
                    manager.storage,
                )
                .map_err(|e| e.to_string())?;
                self.persist_connection(manager)?;
            }
            Screen::OAuthResponse => {
                let value = manager.input.trim();
                if value.is_empty() {
                    return Err("Paste the complete response URL.".into());
                }
                let sender = manager
                    .oauth_response
                    .as_ref()
                    .ok_or("Sign-in is no longer active.")?;
                sender
                    .try_send(zeroize::Zeroizing::new(value.to_owned()))
                    .map_err(|_| "Sign-in is no longer accepting a response URL.")?;
                manager.go(Screen::OAuth);
                manager.notice = "Validating response and saving connection…".into();
            }
            Screen::SecretLabel => {
                let label = manager.input.trim();
                if label.is_empty() || label.len() > 120 {
                    return Err("Enter a label of 1–120 bytes.".into());
                }
                manager.secret_label = label.to_string();
                if manager.secret_id.is_empty() {
                    manager.secret_id = format!("{:032x}", rand::random::<u128>());
                }
                manager.go(Screen::SecretValue);
                manager.input = std::mem::take(&mut manager.secret_value);
            }
            Screen::SecretValue => {
                if manager.input.trim().is_empty() {
                    return Err("Enter a secret value.".into());
                }
                if is_pushover_secret(&manager.secret_id) {
                    KeyStore::set_pushover_secret(
                        config,
                        &manager.secret_id,
                        &manager.secret_label,
                        &manager.input,
                        manager.storage,
                    )
                    .map_err(|error| error.to_string())?;
                } else {
                    KeyStore::set_secret(
                        config,
                        &manager.secret_id,
                        &manager.secret_label,
                        manager.secret_kind,
                        &manager.input,
                    )
                    .map_err(|error| error.to_string())?;
                }
                if manager.pushover_edit {
                    manager.go(Screen::PushoverSaved);
                } else {
                    manager.go(Screen::Vault);
                    self.refresh_vault_manager(config, manager);
                }
            }
            Screen::Model => {
                let model = manager.input.trim();
                if model.is_empty() || model.len() > 256 || model.chars().any(char::is_control) {
                    return Err(
                        "Enter a model ID of 1–256 bytes, without control characters.".into(),
                    );
                }
                let provider = manager.draft.as_ref().ok_or("Select a connection first.")?;
                if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
                    tab.tab.provider = provider.name.clone();
                    tab.tab.endpoint = None;
                    tab.tab.model = model.to_string();
                    tab.manual_model_override = true;
                }
                self.refresh_visible_selectors();
                manager.draft = None;
                manager.go(Screen::Providers);
            }
            _ => {}
        }
        Ok(())
    }

    pub(crate) async fn poll_credential_login(&mut self) {
        let Some(mut manager) = self.ui.credential_manager.take() else {
            return;
        };
        if manager.closing {
            if let Some(task) = manager.oauth_result.take() {
                task.abort();
                let _ = task.await;
            }
            return;
        }
        if let Some(events) = manager.oauth_events.as_mut() {
            while let Ok(event) = events.try_recv() {
                match event {
                    crate::llm::auth::tui::LoginEvent::AuthorizationUrl(url) => {
                        manager.oauth_url = Some(url)
                    }
                    crate::llm::auth::tui::LoginEvent::Progress(message) => {
                        manager.notice = message
                    }
                }
            }
        }
        if manager
            .oauth_result
            .as_ref()
            .is_some_and(|task| task.is_finished())
        {
            if let Some(task) = manager.oauth_result.take() {
                manager.oauth_cancel = None;
                manager.oauth_events = None;
                manager.oauth_response = None;
                manager.oauth_url = None;
                match task.await {
                    Ok(Ok(())) => {
                        if let Err(error) = self.persist_connection(&mut manager) {
                            manager.go(Screen::Storage);
                            manager.error = Some(error);
                        }
                    }
                    Ok(Err(error)) => {
                        manager.go(Screen::Storage);
                        manager.error = Some(error);
                    }
                    Err(_) => {
                        manager.go(Screen::Storage);
                        manager.error = Some("Sign-in could not finish. Please retry.".into());
                    }
                }
            }
        }
        self.ui.credential_manager = Some(manager);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlays_hide_chat_cursor_and_credential_input_owns_its_cursor() {
        super::super::tests::with_test_app_config("overlay-cursor", AppConfig::default(), |app| {
            use ratatui::backend::Backend;
            let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30))
                .expect("terminal");
            terminal.draw(|frame| app.ui.render(frame)).expect("chat");
            assert!(terminal.backend().cursor_visible());
            app.ui.settings_v2 = Some(crate::tui::settings_panel::SettingsPanelState::new());
            terminal
                .draw(|frame| app.ui.render(frame))
                .expect("settings");
            assert!(!terminal.backend().cursor_visible());
            app.open_credential_manager(false);
            terminal
                .draw(|frame| app.ui.render(frame))
                .expect("providers");
            assert!(!terminal.backend().cursor_visible());
            let manager = app.ui.credential_manager.as_mut().expect("manager");
            manager.go(Screen::ApiKey);
            manager.input = "secret".into();
            terminal
                .draw(|frame| app.ui.render(frame))
                .expect("masked entry");
            assert!(terminal.backend().cursor_visible());
            let cursor = terminal
                .backend_mut()
                .get_cursor_position()
                .expect("cursor");
            assert_eq!(
                terminal.backend().buffer()[(cursor.x - 1, cursor.y)].symbol(),
                "•"
            );
            app.ui.credential_manager = None;
            app.ui.active_modal = Some(crate::ui::Modal::QuitConfirm);
            terminal
                .draw(|frame| app.ui.render(frame))
                .expect("confirmation");
            assert!(!terminal.backend().cursor_visible());
        });
    }

    #[test]
    fn oauth_modal_text_can_be_drag_selected_without_clicking_underlying_chat() {
        super::super::tests::with_test_app_config("oauth-selection", AppConfig::default(), |app| {
            use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
            let url = "https://example.test/authorize?state=selection";
            app.open_credential_manager(false);
            let manager = app.ui.credential_manager.as_mut().expect("manager");
            manager.go(Screen::OAuth);
            manager.oauth_url = Some(url.into());
            let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30))
                .expect("terminal");
            terminal.draw(|frame| app.ui.render(frame)).expect("OAuth");
            let buffer = terminal.backend().buffer();
            let index = buffer
                .content
                .windows(5)
                .position(|cells| {
                    cells
                        .iter()
                        .map(|cell| cell.symbol())
                        .eq(["h", "t", "t", "p", "s"])
                })
                .expect("visible URL");
            let start = Position::new(index as u16 % 100, index as u16 / 100);
            let end = Position::new(start.x + url.len() as u16 - 1, start.y);
            for (kind, position) in [
                (MouseEventKind::Down(MouseButton::Left), start),
                (MouseEventKind::Drag(MouseButton::Left), end),
            ] {
                app.handle_mouse(MouseEvent {
                    kind,
                    column: position.x,
                    row: position.y,
                    modifiers: KeyModifiers::NONE,
                });
            }
            terminal
                .draw(|frame| app.ui.render(frame))
                .expect("highlight");
            assert!(terminal.backend().buffer()[(start.x, start.y)]
                .modifier
                .contains(Modifier::REVERSED));
            assert_eq!(app.ui.text_selection.finish(end).as_deref(), Some(url));
            assert!(app.ui.credential_manager.is_some());
            assert!(app.ui.tabs[0].messages.is_empty());
        });
    }

    #[test]
    fn endpoint_rejects_credentials_and_insecure_remote_hosts() {
        for endpoint in [
            "http://example.com/v1",
            "https://user:secret@example.com",
            "https://example.com?key=secret",
            "https://example.com#secret",
            "file:///tmp/key",
        ] {
            assert!(validate_endpoint(endpoint).is_err());
        }
        for endpoint in [
            "https://example.com/v1",
            "http://localhost:11434/v1",
            "http://127.0.0.2:8080",
            "http://[::1]:8080/v1",
        ] {
            assert!(validate_endpoint(endpoint).is_ok());
        }
    }

    #[test]
    fn pasted_ssh_key_preserves_newlines_without_touching_chat() {
        let mut manager = CredentialManager::new(&AppConfig::default(), true);
        manager.go(Screen::SecretValue);
        manager.paste(
            "-----BEGIN OPENSSH PRIVATE KEY-----\nbody\n-----END OPENSSH PRIVATE KEY-----\n",
        );
        assert_eq!(manager.input.lines().count(), 3);
        assert!(manager.masked());
    }

    #[test]
    fn pasted_oauth_response_is_masked_and_submitted_without_chat_history() {
        super::super::tests::with_test_app_config(
            "oauth-paste-response",
            AppConfig::default(),
            |app| {
                app.open_credential_manager(false);
                let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
                let manager = app.ui.credential_manager.as_mut().unwrap();
                manager.go(Screen::OAuth);
                manager.oauth_response = Some(sender);
                manager.paste(
                    "http://localhost:1455/auth/callback?code=private-code&state=private-state",
                );
                assert!(matches!(manager.screen, Screen::OAuthResponse));
                assert!(manager.masked());
                let mut terminal =
                    ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 40)).unwrap();
                terminal
                    .draw(|frame| manager.render(frame, frame.area()))
                    .unwrap();
                let text = terminal
                    .backend()
                    .buffer()
                    .content
                    .iter()
                    .map(|cell| cell.symbol())
                    .collect::<String>();
                assert!(!text.contains("private-code"));
                press(app, KeyCode::Enter);
                let response = receiver.try_recv().unwrap();
                assert!(response.contains("code=private-code"));
                let manager = app.ui.credential_manager.as_ref().unwrap();
                assert!(manager.input.is_empty());
                assert!(matches!(manager.screen, Screen::OAuth));
                assert!(app.ui.tabs[0].input_content.is_empty());
                assert!(app.ui.tabs[0].messages.is_empty());
            },
        );
    }

    #[test]
    fn oauth_response_entry_preserves_copy_shortcut_and_rejects_oversized_paste() {
        let mut manager = CredentialManager::new(&AppConfig::default(), false);
        manager.go(Screen::OAuth);
        assert!(!manager.accepts_input());
        manager.paste(&"x".repeat(4097));
        assert!(manager.input.is_empty());
        assert!(manager.error.as_deref().unwrap().contains("4096"));
    }

    #[test]
    fn credential_render_never_shows_secret_input() {
        let mut manager = CredentialManager::new(&AppConfig::default(), false);
        manager.go(Screen::ApiKey);
        manager.input = "not-visible-secret".into();
        let backend = ratatui::backend::TestBackend::new(100, 30);
        let mut terminal = ratatui::Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| manager.render(frame, frame.area()))
            .expect("render");
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(!text.contains("not-visible-secret"));
        assert!(text.contains("•"));
    }

    fn press(app: &mut TuiApp, code: KeyCode) {
        assert!(app
            .handle_key(KeyEvent::new(code, KeyModifiers::NONE))
            .is_none());
    }

    fn paste(app: &mut TuiApp, value: &str) {
        app.ui
            .credential_manager
            .as_mut()
            .expect("credential manager")
            .paste(value);
    }

    #[test]
    fn search_key_setup_enables_only_its_profile_without_creating_a_chat_provider() {
        let mut config = AppConfig::default();
        config.mcp_servers = vec![crate::config::McpServerConfig {
            name: "Exa".into(),
            transport: "streamable_http".into(),
            url: Some("http://127.0.0.1:43123/custom-mcp".into()),
            ..Default::default()
        }];
        super::super::tests::with_test_app_config("search-key-plaintext", config, |app| {
            app.open_search_credentials();
            press(app, KeyCode::Enter);
            press(app, KeyCode::Down);
            press(app, KeyCode::Enter);
            paste(app, "search-plaintext-canary");
            press(app, KeyCode::Enter);
            let config = app.config.blocking_read().clone();
            assert!(config.providers.is_empty());
            assert!(app.ui.visible_providers.is_empty());
            let exa = config
                .mcp_servers
                .iter()
                .find(|server| server.name == "Exa")
                .expect("Exa");
            assert!(exa.enabled);
            assert_eq!(
                exa.url.as_deref(),
                Some("http://127.0.0.1:43123/custom-mcp")
            );
            assert_eq!(
                KeyStore::get(&config, "Exa Search")
                    .expect("saved key")
                    .as_deref(),
                Some("search-plaintext-canary")
            );
            assert_eq!(
                KeyStore::vault_status(&config).expect("storage").storage,
                CredentialStorage::Encrypted
            );
            assert!(app.ui.tabs[0].messages.is_empty());
            assert!(app.ui.tabs[0].input_content.is_empty());
            press(app, KeyCode::Char('d'));
            press(app, KeyCode::Char('y'));
            assert!(KeyStore::get(&config, "Exa Search")
                .expect("removed key")
                .is_none());
            assert!(
                !app.config
                    .blocking_read()
                    .mcp_servers
                    .iter()
                    .find(|server| server.name == "Exa")
                    .expect("Exa")
                    .enabled
            );
        });
    }

    #[test]
    fn encrypted_search_key_setup_requires_unlock_before_retrieval() {
        super::super::tests::with_test_app_config(
            "search-key-encrypted",
            AppConfig::default(),
            |app| {
                app.open_search_credentials();
                press(app, KeyCode::Down);
                press(app, KeyCode::Enter);
                press(app, KeyCode::Enter);
                paste(app, "search-vault-password");
                press(app, KeyCode::Enter);
                paste(app, "search-vault-password");
                press(app, KeyCode::Enter);
                paste(app, "encrypted-tavily-canary");
                press(app, KeyCode::Enter);
                let config = app.config.blocking_read().clone();
                assert!(config.providers.is_empty());
                assert_eq!(
                    KeyStore::vault_status(&config).expect("storage").storage,
                    CredentialStorage::Encrypted
                );
                KeyStore::lock_vault(&config).expect("lock");
                assert!(KeyStore::get(&config, "Tavily Search").is_err());
                KeyStore::unlock_vault(&config, "search-vault-password").expect("unlock");
                assert_eq!(
                    KeyStore::get(&config, "Tavily Search")
                        .expect("key")
                        .as_deref(),
                    Some("encrypted-tavily-canary")
                );
            },
        );
    }

    #[cfg(unix)]
    #[test]
    fn pushover_settings_store_select_replace_and_mask_encrypted_keys() {
        fn open_key(app: &mut TuiApp, runtime: &tokio::runtime::Runtime, id: &str) {
            app.open_settings_page(Some(id));
            let action = app
                .settings_workspace_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
                .expect("Pushover settings key action");
            runtime
                .block_on(app.dispatch(action))
                .expect("open masked key editor");
        }
        fn select_application(app: &mut TuiApp, runtime: &tokio::runtime::Runtime, name: &str) {
            app.open_settings_page(Some("notifications.pushover.application"));
            assert!(app
                .settings_workspace_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
                .is_none());
            app.settings_workspace_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
            app.ui.settings_workspace.paste(name);
            let action = app
                .settings_workspace_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
                .expect("save application selection");
            runtime
                .block_on(app.dispatch(action))
                .expect("persist selection");
            assert_eq!(
                AppConfig::load()
                    .expect("saved settings")
                    .notifications
                    .pushover
                    .application,
                name
            );
        }
        super::super::tests::with_test_app_config(
            "pushover-settings-keys",
            AppConfig::default(),
            |app| {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("runtime");
                open_key(app, &runtime, "pushover_user_key");
                assert!(
                    app.ui.credential_manager.as_ref().expect("manager").screen
                        == Screen::NewPassword
                );
                paste(app, "long pushover vault password");
                press(app, KeyCode::Enter);
                paste(app, "long pushover vault password");
                press(app, KeyCode::Enter);
                assert!(
                    app.ui.credential_manager.as_ref().expect("manager").screen
                        == Screen::SecretValue
                );
                let mut config = app.config.blocking_read().clone();
                KeyStore::set_storage_default(&config, CredentialStorage::Plaintext)
                    .expect("plaintext preference does not affect Pushover");
                let named_id = pushover::application_key_id("work_alerts").expect("named identity");
                for (index, (id, key)) in [
                    (USER_KEY_ID, "u".repeat(30)),
                    (APP_KEY_ID, "a".repeat(30)),
                    (named_id.as_str(), "n".repeat(30)),
                ]
                .into_iter()
                .enumerate()
                {
                    if index != 0 {
                        press(app, KeyCode::Esc);
                        if index == 2 {
                            select_application(app, &runtime, "work_alerts");
                        }
                        open_key(app, &runtime, "pushover_app_key");
                    }
                    let manager = app.ui.credential_manager.as_ref().expect("manager");
                    assert!(manager.masked());
                    assert!(manager.input.is_empty());
                    paste(app, "invalid-secret-canary");
                    press(app, KeyCode::Enter);
                    assert!(app
                        .ui
                        .credential_manager
                        .as_ref()
                        .expect("manager")
                        .error
                        .is_some());
                    assert!(KeyStore::get_secret(&config, id)
                        .expect("not saved")
                        .is_none());
                    app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
                    paste(app, &key);
                    let mut terminal =
                        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30))
                            .expect("terminal");
                    terminal
                        .draw(|frame| {
                            app.ui
                                .credential_manager
                                .as_ref()
                                .expect("manager")
                                .render(frame, frame.area())
                        })
                        .expect("masked render");
                    assert!(!terminal.backend().to_string().contains(&key));
                    press(app, KeyCode::Enter);
                    assert_eq!(
                        KeyStore::get_secret(&config, id).expect("saved").as_deref(),
                        Some(key.as_str())
                    );
                }
                press(app, KeyCode::Esc);
                open_key(app, &runtime, "pushover_user_key");
                assert!(app
                    .ui
                    .credential_manager
                    .as_ref()
                    .expect("manager")
                    .input
                    .is_empty());
                paste(app, &"r".repeat(30));
                press(app, KeyCode::Enter);
                press(app, KeyCode::Esc);
                open_key(app, &runtime, "pushover_app_key");
                assert!(app
                    .ui
                    .credential_manager
                    .as_ref()
                    .expect("manager")
                    .input
                    .is_empty());
                paste(app, &"b".repeat(30));
                press(app, KeyCode::Enter);
                assert_eq!(
                    KeyStore::list_secrets(&config)
                        .expect("all application keys")
                        .len(),
                    3
                );
                assert_eq!(
                    KeyStore::get_secret(&config, USER_KEY_ID).expect("replacement"),
                    Some("r".repeat(30))
                );
                assert_eq!(
                    KeyStore::get_secret(&config, APP_KEY_ID).expect("preserved default token"),
                    Some("a".repeat(30))
                );
                assert_eq!(
                    KeyStore::get_secret(&config, &named_id).expect("replaced selected token"),
                    Some("b".repeat(30))
                );
                press(app, KeyCode::Esc);
                select_application(app, &runtime, "tcui");
                config = app.config.blocking_read().clone();
                config.notifications.pushover.enabled = true;
                config.notifications.pushover.use_vault = true;
                use std::os::unix::fs::PermissionsExt;
                // The application fixture owns HOME and holds the shared environment lock.
                let root =
                    std::path::PathBuf::from(std::env::var_os("HOME").expect("fixture HOME"));
                let program = root.join("curl");
                struct RestorePath(Option<std::ffi::OsString>);
                impl Drop for RestorePath {
                    fn drop(&mut self) {
                        match &self.0 {
                            Some(path) => std::env::set_var("PATH", path),
                            None => std::env::remove_var("PATH"),
                        }
                    }
                }
                let _restore = RestorePath(std::env::var_os("PATH"));
                std::env::set_var("PATH", &root);
                std::fs::write(&program,
                    "#!/bin/sh\n[ \"$1\" = --disable ] || exit 2\nprintf '%s\\n' \"$@\" > \"$0.argv\"\n/usr/bin/env > \"$0.env\"\n/bin/cat > \"$0.stdin\"\nprintf '%s' '{\"status\":1}'\n"
                ).expect("local curl fixture");
                std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700))
                    .expect("fixture executable");
                config.commands.whitelist = vec![program.to_string_lossy().into_owned()];
                let snapshot = pushover::prepare(&config)
                    .expect("unlocked snapshot")
                    .expect("enabled");
                KeyStore::lock_vault(&config).expect("lock");
                assert!(KeyStore::get_secret(&config, USER_KEY_ID).is_err());
                assert!(pushover::prepare(&config).is_err());
                runtime
                    .block_on(snapshot.send(
                        &config,
                        "reminder",
                        "captured credentials after cold lock",
                    ))
                    .expect("snapshot dispatch must not reopen the vault");
                let input: serde_json::Value = serde_json::from_slice(
                    &std::fs::read(program.with_extension("stdin")).expect("stdin evidence"),
                )
                .expect("Pushover JSON");
                assert_eq!(
                    input,
                    serde_json::json!({
                        "token": "a".repeat(30), "user": "r".repeat(30),
                        "title": "reminder", "message": "captured credentials after cold lock",
                    })
                );
                for extension in ["argv", "env"] {
                    let text = std::fs::read_to_string(program.with_extension(extension))
                        .expect("transport evidence");
                    assert!(!text.contains(&"r".repeat(30)) && !text.contains(&"a".repeat(30)));
                }
                open_key(app, &runtime, "pushover_app_key");
                assert!(
                    app.ui.credential_manager.as_ref().expect("manager").screen == Screen::Unlock
                );
                paste(app, "long pushover vault password");
                press(app, KeyCode::Enter);
                assert!(
                    app.ui.credential_manager.as_ref().expect("manager").screen
                        == Screen::SecretValue
                );
                assert!(app
                    .ui
                    .credential_manager
                    .as_ref()
                    .expect("manager")
                    .input
                    .is_empty());
                paste(app, &"z".repeat(30));
                press(app, KeyCode::Esc);
                assert_eq!(
                    KeyStore::get_secret(&config, APP_KEY_ID).expect("cancel preserves token"),
                    Some("a".repeat(30))
                );
                assert_eq!(
                    KeyStore::get_secret(&config, &named_id).expect("encrypted named key persists"),
                    Some("b".repeat(30))
                );
                let serialized =
                    serde_json::to_string(&AppConfig::load().expect("persisted settings"))
                        .expect("configuration");
                for key in ["r", "a", "b"] {
                    assert!(!serialized.contains(&key.repeat(30)));
                }
            },
        );
    }

    #[test]
    fn pushover_settings_save_plaintext_while_locked_and_gate_encrypted_replacements() {
        fn toggle_storage(app: &mut TuiApp, runtime: &tokio::runtime::Runtime) {
            app.open_settings_page(Some("notifications.pushover.encrypt_credentials"));
            let action = app
                .settings_workspace_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
                .expect("storage toggle");
            runtime
                .block_on(app.dispatch(action))
                .expect("save storage selection");
        }
        let mut initial = AppConfig::default();
        initial.notifications.pushover.enabled = true;
        initial.notifications.pushover.use_vault = true;
        super::super::tests::with_test_app_config("pushover-plaintext-editor", initial, |app| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            assert!(
                app.config
                    .blocking_read()
                    .notifications
                    .pushover
                    .encrypt_credentials
            );
            toggle_storage(app, &runtime);
            let config = app.config.blocking_read().clone();
            assert!(!config.notifications.pushover.encrypt_credentials);
            assert!(!KeyStore::vault_status(&config).unwrap().initialized);
            for (application, key) in [(false, "u".repeat(30)), (true, "a".repeat(30))] {
                app.open_pushover_credential(application);
                let manager = app.ui.credential_manager.as_ref().unwrap();
                assert!(manager.screen == Screen::SecretValue && manager.masked());
                assert!(manager.input.is_empty());
                assert_eq!(manager.storage, CredentialStorage::Plaintext);
                paste(app, &key);
                let mut terminal =
                    ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
                terminal
                    .draw(|frame| {
                        app.ui
                            .credential_manager
                            .as_ref()
                            .unwrap()
                            .render(frame, frame.area())
                    })
                    .unwrap();
                assert!(!terminal.backend().to_string().contains(&key));
                press(app, KeyCode::Enter);
                assert!(
                    app.ui.credential_manager.as_ref().unwrap().screen == Screen::PushoverSaved
                );
                press(app, KeyCode::Esc);
            }
            let keys = pushover::credentials_for_agent(&config, None).unwrap();
            assert_eq!(
                (keys.user_key.as_str(), keys.app_key.as_str()),
                ("u".repeat(30).as_str(), "a".repeat(30).as_str())
            );
            KeyStore::initialize_vault(&config, "pushover-storage-password").unwrap();
            let sibling = pushover::application_key_id("encrypted_sibling").unwrap();
            KeyStore::set_secret(
                &config,
                &sibling,
                "Sibling",
                SecretKind::Password,
                &"s".repeat(30),
            )
            .unwrap();
            KeyStore::lock_vault(&config).unwrap();
            app.open_pushover_credential(false);
            assert!(app.ui.credential_manager.as_ref().unwrap().screen == Screen::SecretValue);
            paste(app, &"r".repeat(30));
            press(app, KeyCode::Enter);
            assert_eq!(
                pushover::credentials_for_agent(&config, None)
                    .unwrap()
                    .user_key
                    .as_str(),
                "r".repeat(30)
            );
            press(app, KeyCode::Esc);
            app.config
                .blocking_write()
                .notifications
                .pushover
                .application = "encrypted_sibling".into();
            app.open_pushover_credential(true);
            assert!(app.ui.credential_manager.as_ref().unwrap().screen == Screen::Unlock);
            press(app, KeyCode::Esc);
            assert_eq!(
                KeyStore::pushover_storage(&config, &sibling).unwrap(),
                Some(CredentialStorage::Encrypted)
            );
            KeyStore::unlock_vault(&config, "pushover-storage-password").unwrap();
            app.open_pushover_credential(true);
            assert!(app.ui.credential_manager.as_ref().unwrap().screen == Screen::SecretValue);
            paste(app, &"p".repeat(30));
            press(app, KeyCode::Enter);
            press(app, KeyCode::Esc);
            assert_eq!(
                KeyStore::pushover_storage(&config, &sibling).unwrap(),
                Some(CredentialStorage::Plaintext)
            );
            assert_eq!(
                KeyStore::get_pushover_secret(&config, APP_KEY_ID)
                    .unwrap()
                    .unwrap()
                    .as_str(),
                "a".repeat(30)
            );
            toggle_storage(app, &runtime);
            assert!(
                app.config
                    .blocking_read()
                    .notifications
                    .pushover
                    .encrypt_credentials
            );
            app.open_pushover_credential(false);
            assert_eq!(
                app.ui.credential_manager.as_ref().unwrap().storage,
                CredentialStorage::Encrypted
            );
            paste(app, &"e".repeat(30));
            press(app, KeyCode::Enter);
            KeyStore::lock_vault(&config).unwrap();
            assert_eq!(
                KeyStore::pushover_storage(&config, USER_KEY_ID).unwrap(),
                Some(CredentialStorage::Encrypted)
            );
            assert!(KeyStore::get_pushover_secret(&config, USER_KEY_ID).is_err());
            assert_eq!(
                KeyStore::get_pushover_secret(&config, APP_KEY_ID)
                    .unwrap()
                    .unwrap()
                    .as_str(),
                "a".repeat(30)
            );
        });
    }

    #[test]
    fn search_key_manager_cannot_overwrite_or_delete_an_ambiguous_chat_credential() {
        let mut config = AppConfig::default();
        config.providers.push(ProviderConfig {
            name: "Exa Search".into(),
            endpoint: "https://chat.example.test/v1".into(),
            env_var: String::new(),
            backend_type: "openai".into(),
            auth_type: "api_key".into(),
        });
        super::super::tests::with_test_app_config("search-key-collision", config, |app| {
            let config = app.config.blocking_read().clone();
            KeyStore::set_storage_preference(&config, CredentialStorage::Plaintext)
                .expect("plaintext");
            KeyStore::set(&config, "Exa Search", "chat-only-canary").expect("chat credential");
            app.open_search_credentials();
            press(app, KeyCode::Enter);
            press(app, KeyCode::Enter);
            paste(app, "must-not-overwrite");
            press(app, KeyCode::Enter);
            assert!(app
                .ui
                .credential_manager
                .as_ref()
                .expect("manager")
                .error
                .is_some());
            assert_eq!(
                KeyStore::get(&config, "Exa Search")
                    .expect("chat key")
                    .as_deref(),
                Some("chat-only-canary")
            );
            app.open_search_credentials();
            press(app, KeyCode::Char('d'));
            press(app, KeyCode::Char('y'));
            assert_eq!(
                KeyStore::get(&config, "Exa Search")
                    .expect("chat key retained")
                    .as_deref(),
                Some("chat-only-canary")
            );
        });
    }

    #[test]
    fn provider_manager_saves_only_after_key_submission_and_removes_connection() {
        super::super::tests::with_test_app_config(
            "provider-manager-key",
            AppConfig::default(),
            |app| {
                app.open_credential_manager(false);
                assert!(app.ui.visible_providers.is_empty());
                assert!(app
                    .ui
                    .credential_manager
                    .as_ref()
                    .expect("manager")
                    .providers
                    .is_empty());
                press(app, KeyCode::Enter);
                press(app, KeyCode::Enter);
                assert!(matches!(
                    app.ui.credential_manager.as_ref().expect("manager").screen,
                    Screen::Storage
                ));
                press(app, KeyCode::Down);
                press(app, KeyCode::Enter);
                assert!(matches!(
                    app.ui.credential_manager.as_ref().expect("manager").screen,
                    Screen::ApiKey
                ));
                assert!(app.config.blocking_read().providers.is_empty());
                paste(app, "explicit-provider-secret");
                press(app, KeyCode::Enter);
                let config = app.config.blocking_read().clone();
                assert_eq!(config.providers.len(), 1);
                assert_eq!(config.providers[0].name, "OpenAI");
                assert_eq!(
                    KeyStore::get(&config, "OpenAI").expect("key").as_deref(),
                    Some("explicit-provider-secret")
                );
                assert_eq!(
                    KeyStore::vault_status(&config).expect("status").storage,
                    CredentialStorage::Encrypted
                );
                assert_eq!(app.ui.visible_providers.len(), 1);
                assert!(app.ui.tabs[0].messages.is_empty());
                assert!(app.ui.tabs[0].input_content.is_empty());
                press(app, KeyCode::Down);
                press(app, KeyCode::Char('d'));
                press(app, KeyCode::Char('y'));
                assert!(app.config.blocking_read().providers.is_empty());
                assert!(KeyStore::get(&config, "OpenAI")
                    .expect("removed key")
                    .is_none());
                assert!(app.ui.visible_providers.is_empty());
            },
        );
    }

    #[test]
    fn custom_connection_validates_endpoint_and_accepts_manual_model() {
        super::super::tests::with_test_app_config(
            "provider-manager-custom",
            AppConfig::default(),
            |app| {
                app.open_credential_manager(false);
                press(app, KeyCode::Enter);
                let manager = app.ui.credential_manager.as_mut().expect("manager");
                manager.selected = manager.preset_choices().len();
                press(app, KeyCode::Enter);
                paste(app, "Private inference");
                press(app, KeyCode::Enter);
                paste(app, "http://remote.example/v1");
                press(app, KeyCode::Enter);
                assert!(app
                    .ui
                    .credential_manager
                    .as_ref()
                    .expect("manager")
                    .error
                    .is_some());
                app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
                paste(app, "http://127.0.0.1:8765/v1");
                press(app, KeyCode::Enter);
                press(app, KeyCode::Down);
                press(app, KeyCode::Enter);
                paste(app, "custom-secret");
                press(app, KeyCode::Enter);
                let config = app.config.blocking_read().clone();
                assert_eq!(config.providers[0].endpoint, "http://127.0.0.1:8765/v1");
                press(app, KeyCode::Down);
                press(app, KeyCode::Char('m'));
                paste(app, "private/model");
                press(app, KeyCode::Enter);
                assert_eq!(app.ui.tabs[0].tab.model, "private/model");
                assert_eq!(app.ui.tabs[0].tab.provider, "Private inference");
                assert!(app.ui.tabs[0].messages.is_empty());
            },
        );
    }

    #[test]
    fn vault_initialization_and_machine_key_crud_preserve_chat_during_manual_unlock() {
        super::super::tests::with_test_app_config(
            "credential-vault-ui",
            AppConfig::default(),
            |app| {
                app.open_credential_manager(true);
                press(app, KeyCode::Enter);
                assert!(matches!(
                    app.ui.credential_manager.as_ref().expect("manager").screen,
                    Screen::NewPassword
                ));
                paste(app, "long explicit vault password");
                press(app, KeyCode::Enter);
                paste(app, "long explicit vault password");
                press(app, KeyCode::Enter);
                let config = app.config.blocking_read().clone();
                assert!(KeyStore::vault_status(&config).expect("status").unlocked);
                app.ui
                    .credential_manager
                    .as_mut()
                    .expect("manager")
                    .selected = 5;
                press(app, KeyCode::Enter);
                paste(app, "Production SSH");
                press(app, KeyCode::Enter);
                let original = "-----BEGIN OPENSSH PRIVATE KEY-----\nline one\nline two\n-----END OPENSSH PRIVATE KEY-----\n";
                paste(app, original);
                press(app, KeyCode::Enter);
                let secrets = KeyStore::list_secrets(&config).expect("secret list");
                assert_eq!(secrets.len(), 1);
                assert_eq!(secrets[0].kind, SecretKind::SshKey);
                assert_eq!(
                    KeyStore::get_secret(&config, &secrets[0].id)
                        .expect("secret")
                        .as_deref(),
                    Some(original)
                );
                app.ui
                    .credential_manager
                    .as_mut()
                    .expect("manager")
                    .selected = 7;
                press(app, KeyCode::Enter);
                app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
                paste(app, "Renamed SSH");
                press(app, KeyCode::Enter);
                app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
                paste(app, "replacement\nmultiline\nkey");
                press(app, KeyCode::Enter);
                assert_eq!(
                    KeyStore::list_secrets(&config).expect("edited secret")[0].label,
                    "Renamed SSH"
                );
                assert_eq!(
                    KeyStore::get_secret(&config, &secrets[0].id)
                        .expect("edited value")
                        .as_deref(),
                    Some("replacement\nmultiline\nkey")
                );
                app.ui
                    .credential_manager
                    .as_mut()
                    .expect("manager")
                    .selected = 7;
                press(app, KeyCode::Char('d'));
                press(app, KeyCode::Char('y'));
                assert!(KeyStore::list_secrets(&config)
                    .expect("removed secret")
                    .is_empty());
                KeyStore::lock_vault(&config).expect("simulate locked session");
                assert!(!KeyStore::vault_status(&config).expect("locked").unlocked);
                app.open_credential_manager(true);
                press(app, KeyCode::Enter);
                assert!(matches!(
                    app.ui
                        .credential_manager
                        .as_ref()
                        .expect("manual unlock")
                        .screen,
                    Screen::Unlock
                ));
                paste(app, "long explicit vault password");
                press(app, KeyCode::Enter);
                assert!(KeyStore::vault_status(&config).expect("unlocked").unlocked);
                press(app, KeyCode::Esc);
                assert!(app.ui.credential_manager.is_none());
                assert!(app.ui.tabs[0].messages.is_empty());
                assert!(app.ui.tabs[0].input_content.is_empty());
            },
        );
    }

    #[test]
    fn configured_catalog_without_saved_credentials_is_not_an_added_connection() {
        super::super::tests::with_test_app_config(
            "provider-manager-catalog",
            AppConfig::default(),
            |app| {
                app.config.blocking_write().providers =
                    crate::config::app_config::provider_presets();
                app.open_credential_manager(false);
                let manager = app.ui.credential_manager.as_ref().expect("manager");
                assert!(manager.providers.is_empty());
                assert!(manager
                    .preset_choices()
                    .iter()
                    .any(|provider| provider.name == "OpenRouter"));
                assert!(manager
                    .preset_choices()
                    .iter()
                    .any(|provider| provider.name == "OpenRouter OAuth"));
                assert!(manager
                    .preset_choices()
                    .iter()
                    .any(|provider| provider.name == "Anthropic Agent SDK"));
            },
        );
    }

    #[test]
    fn selected_markdown_search_completion_takes_precedence_over_vault_command() {
        super::super::tests::with_test_app_config(
            "credential-completion",
            AppConfig::default(),
            |app| {
                app.replace_input_content("/vault".into());
                let popup = app.ui.list_popup.as_mut().expect("command completion");
                let index = popup
                    .items
                    .iter()
                    .position(|item| item.label.starts_with("/vault-search"))
                    .expect("Markdown search completion");
                popup.selected = Some(index);
                let action = app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                assert!(
                    matches!(action, Some(super::super::Action::ShowLocalSearch(query)) if query.is_empty())
                );
                assert!(app.ui.credential_manager.is_none());
                assert!(app.ui.tabs[0].input_content.is_empty());
            },
        );
    }

    #[test]
    fn busy_configuration_never_exposes_unconnected_catalog_entries() {
        super::super::tests::with_test_app_config(
            "credential-busy-config",
            AppConfig::default(),
            |app| {
                app.ui.db_providers = AppConfig {
                    providers: crate::config::app_config::provider_presets(),
                    ..AppConfig::default()
                }
                .provider_entries();
                assert!(app.ui.visible_providers.is_empty());
                let config = app.config.clone();
                let _guard = config.try_write().expect("hold config lock");
                app.refresh_visible_selectors();
                assert!(app.ui.visible_providers.is_empty());
            },
        );
    }

    #[test]
    fn credential_command_arguments_never_become_chat_messages() {
        super::super::tests::with_test_app_config(
            "credential-command-secrets",
            AppConfig::default(),
            |app| {
                for command in [
                    "/vault not-a-chat-secret",
                    "/connect not-a-chat-secret",
                    "/providers not-a-chat-secret",
                ] {
                    app.replace_input_content(command.to_string());
                    let action = app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                    assert!(matches!(
                        action,
                        Some(
                            super::super::Action::OpenCredentialVault
                                | super::super::Action::OpenProviders
                        )
                    ));
                    assert!(app.ui.tabs[0].messages.is_empty());
                    assert!(app.ui.tabs[0].input_content.is_empty());
                }
            },
        );
    }

    #[test]
    fn encrypted_provider_metadata_remains_visible_when_manual_unlock_is_cancelled() {
        super::super::tests::with_test_app_config(
            "credential-locked-provider",
            AppConfig::default(),
            |app| {
                let mut config = app.config.blocking_read().clone();
                config
                    .providers
                    .push(crate::config::app_config::provider_presets().remove(0));
                KeyStore::initialize_vault(&config, "locked provider password")
                    .expect("initialize");
                KeyStore::set(&config, "OpenAI", "encrypted-provider-secret")
                    .expect("store credential");
                *app.config.blocking_write() = config.clone();
                app.ui.db_providers = TuiApp::provider_entries_with_local(&config, None);
                KeyStore::lock_vault(&config).expect("lock");
                app.refresh_visible_selectors();
                app.open_credential_manager(true);
                press(app, KeyCode::Enter);
                press(app, KeyCode::Esc);
                assert!(app.ui.credential_manager.is_none());
                assert_eq!(app.ui.visible_providers.len(), 1);
                assert!(KeyStore::get(&config, "OpenAI").is_err());
                app.open_credential_manager(false);
                assert_eq!(
                    app.ui
                        .credential_manager
                        .as_ref()
                        .expect("manager")
                        .providers
                        .len(),
                    1
                );
            },
        );
    }

    #[test]
    fn adding_plaintext_provider_keeps_locked_encrypted_connection_and_labels() {
        super::super::tests::with_test_app_config(
            "provider-manager-mixed",
            AppConfig::default(),
            |app| {
                let mut config = app.config.blocking_read().clone();
                config
                    .providers
                    .push(crate::config::app_config::provider_presets().remove(0));
                KeyStore::initialize_vault(&config, "mixed-provider-password").expect("initialize");
                KeyStore::set(&config, "OpenAI", "encrypted-original")
                    .expect("encrypted connection");
                KeyStore::lock_vault(&config).expect("restart locked");
                *app.config.blocking_write() = config.clone();
                app.ui.db_providers = TuiApp::provider_entries_with_local(&config, None);
                app.open_credential_manager(false);
                press(app, KeyCode::Enter);
                press(app, KeyCode::Enter);
                press(app, KeyCode::Down);
                press(app, KeyCode::Enter);
                paste(app, "plaintext-second");
                press(app, KeyCode::Enter);
                let updated = app.config.blocking_read().clone();
                assert_eq!(updated.providers.len(), 2);
                assert!(updated
                    .providers
                    .iter()
                    .any(|provider| provider.name == "OpenAI"));
                assert!(updated
                    .providers
                    .iter()
                    .any(|provider| provider.name == "OpenAI::plaintext"));
                assert!(KeyStore::get(&updated, "OpenAI").is_err());
                assert_eq!(
                    KeyStore::get(&updated, "OpenAI::plaintext")
                        .expect("plaintext")
                        .as_deref(),
                    Some("plaintext-second")
                );
                let manager = app.ui.credential_manager.as_ref().expect("manager");
                assert_eq!(manager.providers.len(), 2);
                assert_eq!(provider_label(&manager.providers[0]), "OpenAI (encrypted)");
                assert_eq!(
                    crate::ui::provider_display_name("OpenAI"),
                    "OpenAI (encrypted)"
                );
                assert_eq!(
                    crate::ui::provider_display_name("OpenAI::plaintext"),
                    "OpenAI"
                );
                KeyStore::unlock_vault(&updated, "mixed-provider-password").expect("unlock");
                assert_eq!(
                    KeyStore::get(&updated, "OpenAI")
                        .expect("encrypted preserved")
                        .as_deref(),
                    Some("encrypted-original")
                );
            },
        );
    }

    #[test]
    fn encrypted_creation_prompts_before_credentials_without_converting_plaintext() {
        super::super::tests::with_test_app_config(
            "provider-manager-unlock-new",
            AppConfig::default(),
            |app| {
                let config = app.config.blocking_read().clone();
                KeyStore::set_with_storage(
                    &config,
                    "Existing",
                    "plaintext-original",
                    CredentialStorage::Plaintext,
                )
                .expect("existing plaintext");
                KeyStore::initialize_vault(&config, "creation-password").expect("initialize");
                KeyStore::lock_vault(&config).expect("lock");
                app.open_credential_manager(false);
                press(app, KeyCode::Enter);
                press(app, KeyCode::Enter);
                press(app, KeyCode::Enter);
                assert!(matches!(
                    app.ui.credential_manager.as_ref().expect("manager").screen,
                    Screen::Unlock
                ));
                assert_eq!(
                    KeyStore::get(&config, "Existing")
                        .expect("plaintext untouched")
                        .as_deref(),
                    Some("plaintext-original")
                );
                paste(app, "wrong-password");
                press(app, KeyCode::Enter);
                let manager = app.ui.credential_manager.as_ref().expect("manager");
                assert!(manager.error.is_some());
                assert!(matches!(manager.screen, Screen::Unlock));
                assert!(!KeyStore::has_credential(&config, "OpenAI").expect("not created"));
            },
        );
    }

    #[test]
    fn configured_pushover_editor_resumes_only_nonsecret_target_after_reconstruction() {
        fn rebuild(config: &AppConfig) -> TuiApp {
            TuiApp::new(
                crate::storage::Storage::new().expect("reconstructed storage"),
                std::sync::Arc::new(tokio::sync::RwLock::new(config.clone())),
                std::sync::Arc::new(crate::llm::LlmClient::new()),
            )
        }
        use crate::protection::{ui::SecurityRestart, ProtectionPolicy};
        const PASSWORD: &str = "configured-pushover-password";
        for application in [false, true] {
            super::super::tests::with_test_app_config(
                "pushover-protected-editor-restart",
                AppConfig::default(),
                |fixture| {
                    crate::protection::configure(ProtectionPolicy::default(), "")
                        .expect("configured plaintext policy");
                    let mut config = fixture.config.blocking_read().clone();
                    config.notifications.pushover.application = "work_alerts".into();
                    config.save().expect("application selection");
                    let id = if application {
                        pushover::application_key_id("work_alerts").expect("selected application")
                    } else {
                        USER_KEY_ID.into()
                    };
                    let mut first = rebuild(&config);
                    first.open_pushover_credential(application);
                    assert_eq!(
                        first.security_restart,
                        Some(SecurityRestart::InitializePassword)
                    );
                    let manager = first
                        .ui
                        .credential_manager
                        .as_ref()
                        .expect("pending editor");
                    assert!(
                        manager.closing
                            && manager.input.is_empty()
                            && manager.password.is_empty()
                            && manager.secret_value.is_empty()
                    );
                    let target = first
                        .take_pushover_credential_restart()
                        .expect("nonsecret target");
                    assert!(match &target {
                        PushoverCredentialTarget::User => !application,
                        PushoverCredentialTarget::Application(name) =>
                            application && name == "work_alerts",
                    });
                    drop(first);
                    crate::protection::initialize_password(PASSWORD)
                        .expect("standalone initialization");
                    let mut second = rebuild(&config);
                    second.resume_pushover_credential(target);
                    let manager = second
                        .ui
                        .credential_manager
                        .as_ref()
                        .expect("resumed editor");
                    assert!(manager.screen == Screen::SecretValue && manager.masked());
                    assert!(
                        manager.input.is_empty()
                            && manager.password.is_empty()
                            && manager.secret_value.is_empty()
                    );
                    assert_eq!(manager.secret_id, id);
                    assert_eq!(
                        second.ui.settings_workspace.category,
                        crate::tui::settings_panel::SettingCategory::Pushover
                    );
                    paste(&mut second, &"i".repeat(30));
                    press(&mut second, KeyCode::Enter);
                    let sibling_id =
                        pushover::application_key_id("other_alerts").expect("sibling identity");
                    KeyStore::set_secret(
                        &config,
                        &sibling_id,
                        "Sibling app",
                        SecretKind::Password,
                        &"s".repeat(30),
                    )
                    .expect("encrypted sibling");
                    crate::protection::lock().expect("revoke session keys");
                    second.open_pushover_credential(application);
                    assert_eq!(second.security_restart, Some(SecurityRestart::Unlock));
                    let target = second
                        .take_pushover_credential_restart()
                        .expect("unlock target");
                    drop(second);
                    crate::protection::unlock(PASSWORD).expect("standalone unlock");
                    let mut third = rebuild(&config);
                    third.resume_pushover_credential(target);
                    let manager = third
                        .ui
                        .credential_manager
                        .as_ref()
                        .expect("blank replacement");
                    assert!(
                        manager.screen == Screen::SecretValue
                            && manager.masked()
                            && manager.input.is_empty()
                    );
                    assert_eq!(manager.secret_id, id);
                    paste(&mut third, &"r".repeat(30));
                    press(&mut third, KeyCode::Enter);
                    assert_eq!(
                        KeyStore::get_secret(&config, &id).expect("replacement"),
                        Some("r".repeat(30))
                    );
                    assert_eq!(
                        KeyStore::get_secret(&config, &sibling_id).expect("preserved sibling"),
                        Some("s".repeat(30))
                    );
                    let saved = serde_json::to_string(&AppConfig::load().expect("saved config"))
                        .expect("serialized settings");
                    assert!(!saved.contains(&"r".repeat(30)) && !saved.contains(&"s".repeat(30)));
                    drop(third);
                    crate::protection::lock().expect("clear test session");
                    assert!(KeyStore::get_secret(&config, &id).is_err());
                },
            );
        }
    }

    #[cfg(feature = "memory")]
    #[test]
    fn configured_unlock_leaves_live_keys_intact_and_blocks_cached_content_before_gate() {
        use crate::protection::ui::SecurityRestart;
        use crate::protection::ProtectionPolicy;

        super::super::tests::with_test_app_config(
            "credential-unlock-before-teardown",
            AppConfig::default(),
            |app| {
                crate::protection::configure(
                    ProtectionPolicy {
                        app_password: true,
                        memories: true,
                        vault: true,
                        ..ProtectionPolicy::default()
                    },
                    "protected-cache-password",
                )
                .expect("configure protected domains");
                let marker = "PRIVATE42";
                let store =
                    crate::memory::MemoryStore::open_default().expect("native memory store");
                store
                    .write(std::path::Path::new("cached-memory.md"), marker, false)
                    .expect("protected memory");
                let path = store.active_documents().expect("active memory")[0]
                    .0
                    .clone();
                let stored = std::fs::read(&path).expect("encrypted fixture");
                app.refresh_memory_artifacts().expect("memory catalog");
                let handle = app
                    .ui
                    .memory_artifacts
                    .iter()
                    .find(|artifact| {
                        matches!(
                            artifact.handle,
                            crate::ui::artifact_sidebar::ArtifactHandle::Memory(_)
                        )
                    })
                    .expect("native memory artifact")
                    .handle
                    .clone();
                app.open_artifact(handle.clone());
                let mut terminal =
                    ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30))
                        .expect("terminal");
                terminal
                    .draw(|frame| app.ui.render(frame))
                    .expect("render loaded memory");
                let loaded = terminal
                    .backend()
                    .buffer()
                    .content
                    .iter()
                    .flat_map(|cell| cell.symbol().chars())
                    .filter(|character| !character.is_whitespace())
                    .collect::<String>();
                assert!(
                    loaded.contains(marker),
                    "fixture must contain an already-visible decrypted memory: {loaded:?}; viewer={}, chat_area={:?}",
                    app.ui.artifact_viewer.is_some(), app.ui.chat_area
                );

                app.open_credential_manager(true);
                let manager = app.ui.credential_manager.as_mut().expect("manager");
                manager.go(Screen::Unlock);
                manager.input = "wrong-password".into();
                press(app, KeyCode::Enter);
                assert_eq!(app.security_restart, Some(SecurityRestart::Unlock));
                assert!(app.ui.protection_blocked);
                assert!(crate::protection::is_unlocked().expect("live session"),
                    "password authentication must wait until requests are persisted and the app is dropped");
                assert!(app
                    .ui
                    .credential_manager
                    .as_ref()
                    .expect("manager")
                    .input
                    .is_empty());

                // The standalone gate may fail only after runtime teardown. Keep the stale cache
                // here deliberately to prove pending transitions cannot render or reopen it.
                assert!(crate::protection::unlock("wrong-password").is_err());
                assert!(crate::protection::startup_requires_unlock().expect("mandatory gate"));
                press(app, KeyCode::Esc);
                assert_eq!(app.security_restart, Some(SecurityRestart::Unlock));
                assert!(app.ui.protection_blocked);
                assert!(
                    app.ui.artifact_viewer.is_some(),
                    "retain the already-open decrypted viewer"
                );
                terminal
                    .draw(|frame| app.ui.render(frame))
                    .expect("render blocked transition");
                let blocked = terminal
                    .backend()
                    .buffer()
                    .content
                    .iter()
                    .flat_map(|cell| cell.symbol().chars())
                    .filter(|character| !character.is_whitespace())
                    .collect::<String>();
                assert!(!blocked.contains(marker));
                app.ui.artifact_viewer = None;
                app.open_artifact(handle.clone());
                assert!(app.ui.artifact_viewer.is_none());
                app.prepare_artifact_save(handle.clone());
                assert!(app.ui.export_dialog.is_none());
                app.delete_artifact(handle);
                assert_eq!(std::fs::read(&path).expect("locked memory remains"), stored);
                crate::protection::lock().expect("clear test session");
            },
        );
    }

    #[test]
    fn failed_legacy_reauthentication_forces_restart_and_cannot_be_cancelled_back_to_cache() {
        use crate::protection::ui::SecurityRestart;
        super::super::tests::with_test_app_config(
            "credential-failed-reauthentication",
            AppConfig::default(),
            |app| {
                let config = app.config.blocking_read().clone();
                KeyStore::initialize_vault(&config, "existing-password").expect("initialize");
                app.open_credential_manager(true);
                let manager = app.ui.credential_manager.as_mut().expect("manager");
                manager.go(Screen::Unlock);
                manager.input = "wrong-password".into();
                manager.secret_value = "PRIVATE_CACHED_CREDENTIAL_MARKER".into();
                press(app, KeyCode::Enter);
                assert_eq!(app.security_restart, Some(SecurityRestart::Lock));
                assert!(app
                    .security_notice
                    .as_ref()
                    .is_some_and(|notice| notice.contains("authentication failed")));
                assert!(app.ui.protection_blocked);
                assert!(!KeyStore::vault_status(&config).expect("status").unlocked);
                press(app, KeyCode::Esc);
                assert_eq!(app.security_restart, Some(SecurityRestart::Lock));
                assert!(app.ui.credential_manager.is_some());
                let mut terminal =
                    ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30))
                        .expect("terminal");
                terminal
                    .draw(|frame| app.ui.render(frame))
                    .expect("blocked render");
                let rendered = terminal
                    .backend()
                    .buffer()
                    .content
                    .iter()
                    .map(|cell| cell.symbol())
                    .collect::<String>();
                assert!(!rendered.contains("PRIVATE_CACHED_CREDENTIAL_MARKER"));
            },
        );
    }

    #[test]
    fn configured_plaintext_first_encrypted_creation_and_vault_setup_request_standalone_initialization(
    ) {
        use crate::protection::ui::SecurityRestart;
        use crate::protection::ProtectionPolicy;

        for provider_creation in [true, false] {
            super::super::tests::with_test_app_config(
                "credential-plaintext-initialization",
                AppConfig::default(),
                |app| {
                    crate::protection::configure(ProtectionPolicy::default(), "")
                        .expect("plaintext first-run policy");
                    let config = app.config.blocking_read().clone();
                    KeyStore::set_with_storage(
                        &config,
                        "Existing",
                        "plaintext-sibling",
                        CredentialStorage::Plaintext,
                    )
                    .expect("existing plaintext connection");
                    if provider_creation {
                        app.open_credential_manager(false);
                        press(app, KeyCode::Enter);
                        press(app, KeyCode::Enter);
                        assert_eq!(
                            app.ui
                                .credential_manager
                                .as_ref()
                                .expect("storage")
                                .selected,
                            1
                        );
                        press(app, KeyCode::Up);
                        press(app, KeyCode::Enter);
                    } else {
                        app.open_credential_manager(true);
                        press(app, KeyCode::Enter);
                    }
                    assert_eq!(
                        app.security_restart,
                        Some(SecurityRestart::InitializePassword)
                    );
                    assert!(app.ui.protection_blocked);
                    assert!(
                        !KeyStore::vault_status(&config)
                            .expect("not yet initialized")
                            .initialized
                    );
                    assert_eq!(
                        crate::protection::policy().expect("same policy"),
                        Some(ProtectionPolicy::default())
                    );
                    assert_eq!(
                        KeyStore::vault_status(&config)
                            .expect("default unchanged")
                            .storage,
                        CredentialStorage::Plaintext
                    );
                    assert_eq!(
                        KeyStore::get(&config, "Existing")
                            .expect("sibling preserved")
                            .as_deref(),
                        Some("plaintext-sibling")
                    );
                    assert!(!KeyStore::has_credential(&config, "OpenAI").expect("not created"));
                },
            );
        }
    }

    #[test]
    fn storage_screen_uses_saved_default_and_explicit_choice_stays_local() {
        super::super::tests::with_test_app_config(
            "credential-storage-default",
            AppConfig::default(),
            |app| {
                let config = app.config.blocking_read().clone();
                KeyStore::initialize_vault(&config, "default-choice-password").expect("initialize");
                KeyStore::set_with_storage(
                    &config,
                    "OpenAI",
                    "encrypted-original",
                    CredentialStorage::Encrypted,
                )
                .expect("encrypted sibling");
                KeyStore::set_storage_default(&config, CredentialStorage::Plaintext)
                    .expect("saved plaintext default");
                app.open_credential_manager(false);
                press(app, KeyCode::Enter);
                press(app, KeyCode::Enter);
                assert_eq!(
                    app.ui
                        .credential_manager
                        .as_ref()
                        .expect("storage")
                        .selected,
                    1
                );
                press(app, KeyCode::Up);
                press(app, KeyCode::Enter);
                assert!(matches!(
                    app.ui
                        .credential_manager
                        .as_ref()
                        .expect("explicit encrypted")
                        .screen,
                    Screen::ApiKey
                ));
                assert_eq!(
                    KeyStore::vault_status(&config)
                        .expect("saved default")
                        .storage,
                    CredentialStorage::Plaintext
                );
                app.open_credential_manager(false);
                press(app, KeyCode::Enter);
                press(app, KeyCode::Enter);
                assert_eq!(
                    app.ui
                        .credential_manager
                        .as_ref()
                        .expect("saved choice")
                        .selected,
                    1
                );
                press(app, KeyCode::Enter);
                paste(app, "plaintext-new");
                press(app, KeyCode::Enter);
                assert_eq!(
                    KeyStore::get(&config, "OpenAI::plaintext")
                        .expect("new plaintext")
                        .as_deref(),
                    Some("plaintext-new")
                );
                assert_eq!(
                    KeyStore::get(&config, "OpenAI")
                        .expect("encrypted sibling intact")
                        .as_deref(),
                    Some("encrypted-original")
                );
                assert_eq!(
                    KeyStore::provider_storage(&config, "OpenAI").expect("encrypted storage"),
                    Some(CredentialStorage::Encrypted)
                );
            },
        );
    }

    #[test]
    fn locked_oauth_metadata_survives_manual_unlock_cancel_without_changing_tokens() {
        use crate::config::key_store::{
            OAuthCredential, OAuthCredentialOwnership, OAuthCredentialSource,
        };

        super::super::tests::with_test_app_config(
            "credential-locked-oauth",
            AppConfig::default(),
            |app| {
                let mut config = app.config.blocking_read().clone();
                let provider = crate::config::app_config::provider_presets()
                    .into_iter()
                    .find(|provider| provider.name == "Codex")
                    .expect("Codex preset");
                config.providers.push(provider);
                KeyStore::initialize_vault(&config, "oauth-manual-password").expect("initialize");
                let credential = OAuthCredential {
                    provider: "Codex".into(),
                    access_token: "oauth-access-preserved".into(),
                    refresh_token: Some("oauth-refresh-preserved".into()),
                    expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
                    account_id: Some("original-account".into()),
                    ownership: OAuthCredentialOwnership::Tcui,
                    source: OAuthCredentialSource::NativeOAuth,
                };
                KeyStore::upsert_oauth_with_storage(
                    &config,
                    &credential,
                    CredentialStorage::Encrypted,
                )
                .expect("OAuth connection");
                *app.config.blocking_write() = config.clone();
                app.ui.db_providers = TuiApp::provider_entries_with_local(&config, None);
                KeyStore::lock_vault(&config).expect("restart locked");
                app.refresh_visible_selectors();
                app.open_credential_manager(true);
                press(app, KeyCode::Enter);
                press(app, KeyCode::Esc);
                assert!(app.ui.credential_manager.is_none());
                assert_eq!(app.ui.visible_providers.len(), 1);
                assert!(KeyStore::get_oauth(&config, "Codex").is_err());
                app.open_credential_manager(false);
                let manager = app.ui.credential_manager.as_ref().expect("connections");
                assert_eq!(manager.providers[0].name, "Codex");
                assert_eq!(
                    provider_label(&manager.providers[0]),
                    "OpenAI OAuth (encrypted)"
                );
                KeyStore::unlock_vault(&config, "oauth-manual-password").expect("manual unlock");
                assert_eq!(
                    KeyStore::get_oauth(&config, "Codex").expect("OAuth preserved"),
                    Some(credential)
                );
            },
        );
    }
}
