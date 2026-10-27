use std::time::Duration;

use crate::config::key_store::ApiKeyCredentialSource;
use crate::config::{AppConfig, KeyStore};
use crate::llm::auth::{
    codex::{codex_status, login_with_cli, logout_external_cli, CodexNativeLogout, CodexStatus},
    oauth::{
        CallbackPath, CallbackTimeout, LoopbackCallbackConfig, OAuthCancellation, RedirectUri,
    },
    openrouter::OpenRouterAdapter,
};

use super::adapters::{codex_native_adapter, openrouter_adapter};
use super::errors::{map_codex_cli, map_native, map_oauth, map_openrouter};
use super::input::{read_headless_input, PrintingBrowser};
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
    match request.provider {
        AuthProvider::Codex if request.native => {
            native_codex_login(request.headless, cancellation).await
        }
        AuthProvider::Codex => {
            login_with_cli(request.headless, cancellation)
                .await
                .map_err(map_codex_cli)?;
            Ok(AuthCommandResult::Login {
                message: "Codex CLI credentials are ready for TCUI use.".to_owned(),
            })
        }
        AuthProvider::OpenRouter => openrouter_login(request.headless, cancellation).await,
    }
}

async fn native_codex_login(
    headless: bool,
    cancellation: &OAuthCancellation,
) -> Result<AuthCommandResult, AuthCommandError> {
    let config = config(AuthProvider::Codex)?;
    let adapter = codex_native_adapter()?;
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
                &PrintingBrowser::new("Codex native authorization URL"),
                cancellation,
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
    cancellation: &OAuthCancellation,
) -> Result<AuthCommandResult, AuthCommandError> {
    let config = config(AuthProvider::OpenRouter)?;
    let adapter = openrouter_adapter()?;
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
        let mut input = read_headless_input(cancellation)
            .await
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
            .open_browser(&PrintingBrowser::new("OpenRouter authorization URL"))
            .map_err(map_openrouter)?;
        authorization
            .receive_code(cancellation)
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
            let adapter = codex_native_adapter()?;
            let message = match adapter
                .logout(&config(AuthProvider::Codex)?, cancellation)
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
            let removed = OpenRouterAdapter::logout(&config(AuthProvider::OpenRouter)?)
                .map_err(map_openrouter)?;
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
    match provider {
        AuthProvider::Codex => match codex_status(&config(provider)?) {
            Ok(CodexStatus::Unauthenticated) => Ok(None),
            Ok(status) => Ok(Some(status.to_string())),
            Err(_) => Err(AuthCommandError::CredentialStore { provider }),
        },
        AuthProvider::OpenRouter => {
            let credential = KeyStore::get_api_key_credential(&config(provider)?, "OpenRouter")
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
