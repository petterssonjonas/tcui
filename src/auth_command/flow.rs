use std::time::Duration;

use crate::config::key_store::ApiKeyCredentialSource;
use crate::config::{AppConfig, KeyStore};
use crate::llm::auth::{
    codex::{codex_status, logout_external_cli, CodexNativeLogout, CodexStatus},
    oauth::{
        CallbackPath, CallbackTimeout, LoopbackCallbackConfig, OAuthCancellation, RedirectUri,
    },
    openrouter::OpenRouterAdapter,
};

use super::adapters::{codex_native_adapter, openrouter_adapter};
use super::errors::{map_codex_cli, map_native, map_oauth, map_openrouter};
use super::input::{read_callback_url, read_headless_input, PrintingBrowser};
use super::{
    AuthCommandError, AuthCommandRequest, AuthCommandResult, AuthLoginRequest, AuthLogoutRequest,
    AuthProvider, AuthStatusRequest,
};

const OPENROUTER_CALLBACK_PATH: &str = "/auth/openrouter";
const OPENROUTER_HEADLESS_REDIRECT: &str = "http://127.0.0.1:1455/auth/openrouter";
const OPENROUTER_CALLBACK_TIMEOUT: Duration = Duration::from_secs(5 * 60);

pub(super) async fn execute(
    request: AuthCommandRequest,
    cancellation: &OAuthCancellation,
) -> Result<AuthCommandResult, AuthCommandError> {
    match request {
        AuthCommandRequest::Login(request) => login(request, cancellation).await,
        AuthCommandRequest::Logout(request) => logout(request, cancellation).await,
        AuthCommandRequest::Status(request) => status(request),
    }
}

async fn login(
    request: AuthLoginRequest,
    cancellation: &OAuthCancellation,
) -> Result<AuthCommandResult, AuthCommandError> {
    let base = match request.provider {
        AuthProvider::Codex => "Codex",
        AuthProvider::OpenRouter => "OpenRouter OAuth",
    };
    let initial_config = config(request.provider)?;
    let storage = KeyStore::vault_status(&initial_config)
        .map_err(|_| AuthCommandError::CredentialStore {
            provider: request.provider,
        })?
        .storage;
    let name = KeyStore::connection_id(&initial_config, base, storage).map_err(|_| {
        AuthCommandError::CredentialStore {
            provider: request.provider,
        }
    })?;
    crate::llm::auth::invalidate_provider_refreshes(&name);
    let result = match request.provider {
        AuthProvider::Codex => {
            native_codex_login(
                request.headless,
                request.open_browser,
                cancellation,
                &name,
                storage,
            )
            .await
        }
        AuthProvider::OpenRouter => {
            openrouter_login(
                request.headless,
                request.open_browser,
                cancellation,
                &name,
                storage,
            )
            .await
        }
    }?;
    let mut config = config(request.provider)?;
    let mut preset = crate::config::app_config::provider_presets()
        .into_iter()
        .find(|preset| preset.name == base)
        .ok_or(AuthCommandError::CredentialStore {
            provider: request.provider,
        })?;
    preset.name = name.clone();
    if let Some(existing) = config.providers.iter_mut().find(|entry| entry.name == name) {
        *existing = preset;
    } else {
        config.providers.push(preset);
    }
    config.disabled_providers.retain(|entry| entry != &name);
    config
        .save()
        .map_err(|_| AuthCommandError::CredentialStore {
            provider: request.provider,
        })?;
    Ok(result)
}

async fn native_codex_login(
    headless: bool,
    open_browser: bool,
    cancellation: &OAuthCancellation,
    connection: &str,
    storage: crate::config::key_store::CredentialStorage,
) -> Result<AuthCommandResult, AuthCommandError> {
    let config = config(AuthProvider::Codex)?;
    let adapter = codex_native_adapter()?
        .for_connection(connection)
        .with_storage(storage);
    if headless {
        let authorization = adapter
            .begin_device(cancellation)
            .await
            .map_err(map_native)?;
        println!(
            "Codex native verification URL: {}",
            authorization.verification_url()
        );
        println!("Codex native device code: {}", authorization.user_code());
        adapter
            .complete_device(&config, authorization, cancellation)
            .await
            .map_err(map_native)?;
    } else {
        adapter
            .login_browser(
                &config,
                &PrintingBrowser::new("Codex native authorization URL", open_browser),
                cancellation,
                read_callback_url(cancellation),
            )
            .await
            .map_err(map_native)?;
    }
    Ok(AuthCommandResult::Login {
        message: "TCUI-native Codex authorization completed.".to_owned(),
    })
}

async fn openrouter_login(
    headless: bool,
    open_browser: bool,
    cancellation: &OAuthCancellation,
    connection: &str,
    storage: crate::config::key_store::CredentialStorage,
) -> Result<AuthCommandResult, AuthCommandError> {
    let config = config(AuthProvider::OpenRouter)?;
    let adapter = openrouter_adapter()?
        .for_connection(connection)
        .with_storage(storage);
    let grant = if headless {
        let authorization = adapter
            .begin_headless(
                RedirectUri::parse(OPENROUTER_HEADLESS_REDIRECT)
                    .map_err(|error| map_oauth(AuthProvider::OpenRouter, error))?,
            )
            .map_err(map_openrouter)?;
        println!(
            "OpenRouter authorization URL: {}",
            authorization.authorization_url()
        );
        println!("Paste the final response URL and press Enter.");
        let mut input = tokio::time::timeout(
            OPENROUTER_CALLBACK_TIMEOUT,
            read_headless_input(cancellation),
        )
        .await
        .map_err(|_| {
            map_oauth(
                AuthProvider::OpenRouter,
                crate::llm::auth::oauth::OAuthError::CallbackTimeout,
            )
        })?
        .map_err(|error| map_oauth(AuthProvider::OpenRouter, error))?;
        authorization
            .complete_headless(&mut input)
            .map_err(map_openrouter)?
    } else {
        let callback = LoopbackCallbackConfig::new(
            CallbackPath::parse(OPENROUTER_CALLBACK_PATH)
                .map_err(|error| map_oauth(AuthProvider::OpenRouter, error))?,
            CallbackTimeout::new(OPENROUTER_CALLBACK_TIMEOUT)
                .map_err(|error| map_oauth(AuthProvider::OpenRouter, error))?,
        );
        let authorization = adapter
            .begin_loopback(callback)
            .await
            .map_err(map_openrouter)?;
        authorization
            .open_browser(&PrintingBrowser::new(
                "OpenRouter authorization URL",
                open_browser,
            ))
            .map_err(map_openrouter)?;
        authorization
            .receive_code_with_redirect(cancellation, read_callback_url(cancellation))
            .await
            .map_err(map_openrouter)?
    };
    adapter
        .exchange_and_persist(&config, grant, cancellation)
        .await
        .map_err(map_openrouter)?;
    Ok(AuthCommandResult::Login {
        message: "OpenRouter authorization completed.".to_owned(),
    })
}

fn selected_connection(
    config: &AppConfig,
    provider: AuthProvider,
) -> Result<String, AuthCommandError> {
    let base = match provider {
        AuthProvider::Codex => "Codex",
        AuthProvider::OpenRouter => "OpenRouter OAuth",
    };
    let storage = KeyStore::vault_status(config)
        .map_err(|_| AuthCommandError::CredentialStore { provider })?
        .storage;
    KeyStore::connection_id(config, base, storage)
        .map_err(|_| AuthCommandError::CredentialStore { provider })
}

async fn logout(
    request: AuthLogoutRequest,
    cancellation: &OAuthCancellation,
) -> Result<AuthCommandResult, AuthCommandError> {
    match request.provider {
        AuthProvider::Codex if request.external => {
            logout_external_cli(cancellation)
                .await
                .map_err(map_codex_cli)?;
            Ok(AuthCommandResult::Logout {
                message: "Codex CLI credentials were logged out by Codex CLI.".to_owned(),
            })
        }
        AuthProvider::Codex => {
            let config = config(AuthProvider::Codex)?;
            let connection = selected_connection(&config, AuthProvider::Codex)?;
            crate::llm::auth::invalidate_provider_refreshes(&connection);
            let adapter = codex_native_adapter()?.for_connection(&connection);
            let message = match adapter
                .logout(&config, cancellation)
                .await
                .map_err(map_native)?
            {
                CodexNativeLogout::NoNativeCredential => {
                    "No TCUI-owned Codex credential was stored.".to_owned()
                }
                CodexNativeLogout::Revoked => "Removed TCUI-owned Codex credential.".to_owned(),
                CodexNativeLogout::RevocationFailed(_) => {
                    "Removed TCUI-owned Codex credential; remote revocation did not complete."
                        .to_owned()
                }
            };
            Ok(AuthCommandResult::Logout { message })
        }
        AuthProvider::OpenRouter => {
            let config = config(AuthProvider::OpenRouter)?;
            let connection = selected_connection(&config, AuthProvider::OpenRouter)?;
            crate::llm::auth::invalidate_provider_refreshes(&connection);
            let removed =
                OpenRouterAdapter::logout(&config, &connection).map_err(map_openrouter)?;
            let message = if removed {
                "Removed TCUI-owned OpenRouter credential.".to_owned()
            } else {
                "No TCUI-owned OpenRouter credential was stored.".to_owned()
            };
            Ok(AuthCommandResult::Logout { message })
        }
    }
}

fn status(request: AuthStatusRequest) -> Result<AuthCommandResult, AuthCommandError> {
    let provider = request.provider;
    let messages = match provider {
        Some(provider) => match status_for(provider)? {
            Some(message) => vec![message],
            None => return Err(AuthCommandError::Unauthenticated { provider }),
        },
        None => [
            status_for(AuthProvider::Codex)?,
            status_for(AuthProvider::OpenRouter)?,
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>(),
    };
    if messages.is_empty() {
        Err(AuthCommandError::NoAuthenticationConfigured)
    } else {
        Ok(AuthCommandResult::Status {
            message: messages.join("\n"),
        })
    }
}

fn status_for(provider: AuthProvider) -> Result<Option<String>, AuthCommandError> {
    let config = config(provider)?;
    let connection = selected_connection(&config, provider)?;
    match provider {
        AuthProvider::Codex => match codex_status(&config, &connection) {
            Ok(CodexStatus::Unauthenticated) => Ok(None),
            Ok(status) => Ok(Some(status.to_string())),
            Err(_) => Err(AuthCommandError::CredentialStore { provider }),
        },
        AuthProvider::OpenRouter => {
            let credential = KeyStore::get_api_key_credential(&config, &connection)
                .map_err(|_| AuthCommandError::CredentialStore { provider })?;
            Ok(credential.map(|credential| match credential.source() {
                ApiKeyCredentialSource::OpenRouterPkce => {
                    "OpenRouter: authenticated source=tcui-pkce expires_at=none".to_owned()
                }
            }))
        }
    }
}

fn config(provider: AuthProvider) -> Result<AppConfig, AuthCommandError> {
    AppConfig::load().map_err(|_| AuthCommandError::CredentialStore { provider })
}
