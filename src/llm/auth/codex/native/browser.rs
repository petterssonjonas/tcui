use crate::config::AppConfig;
use crate::llm::auth::oauth::{
    open_authorization_url, AuthorizationCodeExchange, AuthorizationRequest, BrowserLauncher,
    CallbackPath, CallbackTimeout, LoopbackCallback, LoopbackCallbackConfig, OAuthCancellation,
    PkceVerifier, State, TokenService,
};

use super::error::CodexNativeError;
use super::store::persist_tokens;
use super::{CodexNativeAdapter, NATIVE_CALLBACK_TIMEOUT};

const CALLBACK_PATH: &str = "/auth/callback";

pub(super) async fn login(
    adapter: &CodexNativeAdapter,
    config: &AppConfig,
    browser: &impl BrowserLauncher,
    cancellation: &OAuthCancellation,
) -> Result<super::super::credential::CodexCredential, CodexNativeError> {
    let state = State::generate()?;
    let callback = bind_callback(adapter, state.clone()).await?;
    let verifier = PkceVerifier::generate()?;
    let request = AuthorizationRequest::new(
        adapter.endpoints.client_id.clone(),
        callback.redirect_uri().clone(),
        state,
        verifier.s256_challenge(),
        Some(adapter.endpoints.scopes.clone()),
    )
    .with_extra_parameter("id_token_add_organizations", "true")?
    .with_extra_parameter("codex_cli_simplified_flow", "true")?
    .with_extra_parameter("originator", "codex_cli_rs")?;
    let authorization_url = request.build_url(&adapter.endpoints.authorization)?;
    open_authorization_url(browser, &authorization_url)?;
    let redirect_uri = callback.redirect_uri().clone();
    let code = callback.receive(cancellation).await?;
    let exchange = AuthorizationCodeExchange::new(
        adapter.endpoints.client_id.clone(),
        redirect_uri,
        code,
        verifier,
    );
    let service = TokenService::hardened(adapter.endpoints.token.clone())?;
    let tokens = service
        .exchange(&exchange, cancellation, chrono::Utc::now())
        .await?;
    persist_tokens(config, &tokens, None)
}

async fn bind_callback(
    adapter: &CodexNativeAdapter,
    state: State,
) -> Result<LoopbackCallback, CodexNativeError> {
    let path = CallbackPath::parse(CALLBACK_PATH)?;
    let timeout = CallbackTimeout::new(NATIVE_CALLBACK_TIMEOUT)?;
    match adapter.endpoints.callback_port {
        Some(port) => {
            let config = LoopbackCallbackConfig::fixed_localhost_port(path.clone(), timeout, port);
            match LoopbackCallback::bind(config, state.clone()).await {
                Ok(callback) => Ok(callback),
                Err(crate::llm::auth::oauth::OAuthError::CallbackIo) if port == 1455 => {
                    LoopbackCallback::bind(
                        LoopbackCallbackConfig::fixed_localhost_port(path, timeout, 1457),
                        state,
                    )
                    .await
                    .map_err(CodexNativeError::OAuth)
                }
                Err(error) => Err(CodexNativeError::OAuth(error)),
            }
        }
        None => LoopbackCallback::bind(LoopbackCallbackConfig::new(path, timeout), state)
            .await
            .map_err(CodexNativeError::OAuth),
    }
}
