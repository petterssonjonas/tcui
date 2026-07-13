use std::time::Duration;

use reqwest::Url;

use crate::config::key_store::{
    ApiKeyCredential, ApiKeyCredentialOwnership, ApiKeyCredentialSource, Credential,
};
use crate::config::{AppConfig, KeyStore};

use super::oauth::{
    AuthorizationCode, LoopbackCallback, LoopbackCallbackConfig, OAuthCancellation, OAuthError,
    OAuthJsonPost, OAuthJsonService, PkceVerifier, RedirectUri,
};
use protocol::{CreateCodeRequest, CreateCodeResponse, ExchangeCodeRequest, ExchangeCodeResponse};

mod flow;
mod protocol;

pub(crate) use flow::{
    OpenRouterAuthorization, OpenRouterCodeGrant, OpenRouterLoopbackAuthorization,
};

const AUTHORIZATION_ENDPOINT: &str = "https://openrouter.ai/auth";
const CODE_CREATION_ENDPOINT: &str = "https://openrouter.ai/api/v1/auth/keys/code";
const KEY_EXCHANGE_ENDPOINT: &str = "https://openrouter.ai/api/v1/auth/keys";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) struct OpenRouterAdapter {
    endpoints: OpenRouterEndpoints,
    http: OAuthJsonService,
}

impl OpenRouterAdapter {
    pub(crate) fn production() -> Result<Self, OpenRouterError> {
        let endpoints = OpenRouterEndpoints::production()?;
        let http = OAuthJsonService::hardened(REQUEST_TIMEOUT)?;
        Ok(Self { endpoints, http })
    }

    pub(crate) fn begin_headless(
        &self,
        redirect_uri: RedirectUri,
    ) -> Result<OpenRouterAuthorization, OpenRouterError> {
        let verifier = PkceVerifier::generate()?;
        let mut authorization_url = self.endpoints.authorization.clone();
        authorization_url.query_pairs_mut().extend_pairs([
            ("callback_url", redirect_uri.as_url().as_str()),
            ("code_challenge", verifier.s256_challenge().as_str()),
            ("code_challenge_method", "S256"),
        ]);
        Ok(OpenRouterAuthorization::new(
            redirect_uri,
            verifier,
            authorization_url,
        ))
    }

    pub(crate) async fn begin_loopback(
        &self,
        callback_config: LoopbackCallbackConfig,
    ) -> Result<OpenRouterLoopbackAuthorization, OpenRouterError> {
        let callback = LoopbackCallback::bind_without_state(callback_config).await?;
        let authorization = self.begin_headless(callback.redirect_uri().clone())?;
        Ok(OpenRouterLoopbackAuthorization::new(
            authorization,
            callback,
        ))
    }

    pub(crate) async fn create_authorization_code(
        &self,
        management_key: &str,
        authorization: OpenRouterAuthorization,
        cancellation: &OAuthCancellation,
    ) -> Result<OpenRouterCodeGrant, OpenRouterError> {
        let payload = serde_json::to_vec(&CreateCodeRequest {
            callback_url: authorization.redirect_uri(),
            code_challenge: authorization.verifier().s256_challenge().as_str(),
            code_challenge_method: "S256",
        })
        .map_err(|_| OpenRouterError::MalformedResponse)?;
        let request = OAuthJsonPost::new(self.endpoints.code_creation.clone(), payload)
            .with_bearer(management_key)?;
        let response = self.http.post(request, cancellation).await?;
        let response = serde_json::from_slice::<CreateCodeResponse>(&response)
            .map_err(|_| OpenRouterError::MalformedResponse)?;
        let code = AuthorizationCode::parse(response.data.id)?;
        Ok(authorization.into_grant(code))
    }

    pub(crate) async fn exchange_and_persist(
        &self,
        config: &AppConfig,
        grant: OpenRouterCodeGrant,
        cancellation: &OAuthCancellation,
    ) -> Result<(), OpenRouterError> {
        let (code, verifier) = grant.into_parts();
        let payload = serde_json::to_vec(&ExchangeCodeRequest {
            code: code.as_str(),
            code_verifier: verifier.as_str(),
            code_challenge_method: "S256",
        })
        .map_err(|_| OpenRouterError::MalformedResponse)?;
        let response = self
            .http
            .post(
                OAuthJsonPost::new(self.endpoints.key_exchange.clone(), payload),
                cancellation,
            )
            .await?;
        let response = serde_json::from_slice::<ExchangeCodeResponse>(&response)
            .map_err(|_| OpenRouterError::MalformedResponse)?;
        persist_exchanged_key(config, response.key, cancellation)
    }

    pub(crate) fn logout(config: &AppConfig) -> Result<bool, OpenRouterError> {
        KeyStore::remove_api_key(config, "OpenRouter", ApiKeyCredentialSource::OpenRouterPkce)
            .map_err(|_| OpenRouterError::CredentialStore)
    }
}

pub(super) fn persist_exchanged_key(
    config: &AppConfig,
    key: String,
    cancellation: &OAuthCancellation,
) -> Result<(), OpenRouterError> {
    let credential = ApiKeyCredential::new(
        "OpenRouter",
        key,
        ApiKeyCredentialOwnership::Tcui,
        ApiKeyCredentialSource::OpenRouterPkce,
    )
    .map_err(|_| OpenRouterError::MalformedResponse)?;
    if cancellation.is_cancelled() {
        return Err(OpenRouterError::OAuth(OAuthError::Cancelled));
    }
    KeyStore::upsert_credential(config, &Credential::ApiKey(credential))
        .map_err(|_| OpenRouterError::CredentialStore)
}

#[derive(Clone)]
struct OpenRouterEndpoints {
    authorization: Url,
    code_creation: Url,
    key_exchange: Url,
}

impl OpenRouterEndpoints {
    fn production() -> Result<Self, OpenRouterError> {
        Ok(Self {
            authorization: parse_production_url(AUTHORIZATION_ENDPOINT)?,
            code_creation: parse_production_url(CODE_CREATION_ENDPOINT)?,
            key_exchange: parse_production_url(KEY_EXCHANGE_ENDPOINT)?,
        })
    }
}

#[cfg(debug_assertions)]
pub(crate) struct OpenRouterTestEndpoints {
    authorization: Url,
    code_creation: Url,
    key_exchange: Url,
}

#[cfg(debug_assertions)]
impl OpenRouterTestEndpoints {
    pub(crate) fn new(authorization: Url, code_creation: Url, key_exchange: Url) -> Self {
        Self {
            authorization,
            code_creation,
            key_exchange,
        }
    }
}

#[cfg(debug_assertions)]
impl OpenRouterAdapter {
    pub(crate) fn for_test(
        endpoints: OpenRouterTestEndpoints,
        timeout: Duration,
    ) -> Result<Self, OpenRouterError> {
        Ok(Self {
            endpoints: OpenRouterEndpoints {
                authorization: endpoints.authorization,
                code_creation: endpoints.code_creation,
                key_exchange: endpoints.key_exchange,
            },
            http: OAuthJsonService::hardened(timeout)?,
        })
    }
}

fn parse_production_url(value: &str) -> Result<Url, OpenRouterError> {
    let url = Url::parse(value).map_err(|_| OpenRouterError::Configuration)?;
    if url.scheme() != "https" || url.host_str() != Some("openrouter.ai") {
        return Err(OpenRouterError::Configuration);
    }
    Ok(url)
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum OpenRouterError {
    #[error("OpenRouter adapter configuration is invalid")]
    Configuration,
    #[error("OpenRouter OAuth operation failed")]
    OAuth(#[from] OAuthError),
    #[error("OpenRouter returned an invalid response")]
    MalformedResponse,
    #[error("OpenRouter credential could not be stored")]
    CredentialStore,
}
