use std::time::Duration;

use chrono::{DateTime, Utc};
use oauth2::{
    basic::{BasicErrorResponse, BasicRevocationErrorResponse, BasicTokenIntrospectionResponse},
    AsyncHttpClient, Client as OAuthClient, RequestTokenError, StandardRevocableToken, TokenUrl,
};
use reqwest::{redirect::Policy, Client, Url};
use tokio::time::Instant;

use super::http_client::{validate_form, BoundedOAuthHttpClient, OAuthHttpError};
use super::token::AuthorizationCodeExchange;
use super::token_values::{OpenIdTokenResponse, RefreshTokenExchange, TokenSet};
use super::{OAuthCancellation, OAuthError, TokenErrorKind};

const DEFAULT_TOKEN_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub(crate) struct TokenEndpoint(TokenUrl);

impl TokenEndpoint {
    pub(crate) fn parse(value: &str) -> Result<Self, OAuthError> {
        let url = Url::parse(value).map_err(|_| OAuthError::InvalidUrl)?;
        let loopback_http =
            url.scheme() == "http" && matches!(url.host_str(), Some("127.0.0.1") | Some("::1"));
        if (!loopback_http && url.scheme() != "https") || url.fragment().is_some() {
            return Err(OAuthError::InvalidUrl);
        }
        TokenUrl::new(value.to_owned())
            .map(Self)
            .map_err(|_| OAuthError::InvalidUrl)
    }

    fn as_oauth(&self) -> TokenUrl {
        self.0.clone()
    }

    #[cfg(test)]
    pub(crate) fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

pub(crate) struct TokenService {
    client: Client,
    endpoint: TokenEndpoint,
    timeout: TokenRequestTimeout,
}

impl TokenService {
    pub(crate) fn new(client: &Client, endpoint: TokenEndpoint) -> Self {
        Self {
            client: client.clone(),
            endpoint,
            timeout: TokenRequestTimeout::default(),
        }
    }

    pub(crate) fn hardened(endpoint: TokenEndpoint) -> Result<Self, OAuthError> {
        let client = Client::builder()
            .redirect(Policy::none())
            .build()
            .map_err(|_| OAuthError::TokenTransport)?;
        Ok(Self::new(&client, endpoint))
    }

    pub(crate) fn with_timeout(
        client: &Client,
        endpoint: TokenEndpoint,
        timeout: TokenRequestTimeout,
    ) -> Self {
        Self {
            client: client.clone(),
            endpoint,
            timeout,
        }
    }

    pub(crate) async fn exchange(
        &self,
        request: &AuthorizationCodeExchange,
        cancellation: &OAuthCancellation,
        now: DateTime<Utc>,
    ) -> Result<TokenSet, OAuthError> {
        let transport = self.transport(cancellation)?;
        let client = open_id_client(request.client_id.as_oauth())
            .set_token_uri(self.endpoint.as_oauth())
            .set_redirect_uri(request.redirect_uri.as_oauth());
        let response = client
            .exchange_code(request.code.as_oauth())
            .set_pkce_verifier(request.verifier.as_oauth())
            .request_async(&transport)
            .await
            .map_err(map_token_error)?;
        TokenSet::from_oauth(response, now, None)
    }

    pub(crate) async fn refresh(
        &self,
        request: &RefreshTokenExchange,
        cancellation: &OAuthCancellation,
        now: DateTime<Utc>,
    ) -> Result<TokenSet, OAuthError> {
        let transport = self.transport(cancellation)?;
        let client =
            open_id_client(request.client_id.as_oauth()).set_token_uri(self.endpoint.as_oauth());
        let response = client
            .exchange_refresh_token(&request.refresh_token.as_oauth())
            .request_async(&transport)
            .await
            .map_err(map_token_error)?;
        TokenSet::from_oauth(response, now, Some(&request.refresh_token))
    }

    pub(super) async fn exchange_device_code(
        &self,
        client_id: &str,
        device_code: &str,
        cancellation: &OAuthCancellation,
    ) -> Result<OpenIdTokenResponse, OAuthError> {
        let body = oauth2::url::form_urlencoded::Serializer::new(String::new())
            .append_pair("grant_type", "urn:ietf:params:oauth:grant-type:device_code")
            .append_pair("client_id", client_id)
            .append_pair("device_code", device_code)
            .finish()
            .into_bytes();
        validate_form(&body).map_err(OAuthHttpError::into_oauth_error)?;
        let request = oauth2::http::Request::builder()
            .method(oauth2::http::Method::POST)
            .uri(self.endpoint.0.url().as_str())
            .header(oauth2::http::header::ACCEPT, "application/json")
            .header(
                oauth2::http::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .body(body)
            .map_err(|_| OAuthError::TokenTransport)?;
        let transport = self.transport(cancellation)?;
        let response = transport
            .call(request)
            .await
            .map_err(OAuthHttpError::into_oauth_error)?;
        if response.status() == oauth2::http::StatusCode::OK {
            return serde_json::from_slice(response.body())
                .map_err(|_| OAuthError::MalformedTokenResponse);
        }
        let error = serde_json::from_slice::<oauth2::DeviceCodeErrorResponse>(response.body())
            .map_err(|_| OAuthError::UnexpectedTokenStatus)?;
        Err(OAuthError::TokenServer(token_error_kind(
            error.error().as_ref(),
        )))
    }

    fn transport(
        &self,
        cancellation: &OAuthCancellation,
    ) -> Result<BoundedOAuthHttpClient, OAuthError> {
        let deadline = Instant::now()
            .checked_add(self.timeout.0)
            .ok_or(OAuthError::TokenDeadline)?;
        Ok(BoundedOAuthHttpClient::new(
            &self.client,
            cancellation,
            deadline,
        ))
    }
}

fn open_id_client(
    client_id: oauth2::ClientId,
) -> OAuthClient<
    BasicErrorResponse,
    OpenIdTokenResponse,
    BasicTokenIntrospectionResponse,
    StandardRevocableToken,
    BasicRevocationErrorResponse,
> {
    OAuthClient::new(client_id)
}

#[derive(Clone, Copy)]
pub(crate) struct TokenRequestTimeout(Duration);

impl TokenRequestTimeout {
    pub(crate) fn new(value: Duration) -> Result<Self, OAuthError> {
        if value.is_zero() {
            return Err(OAuthError::InvalidValue);
        }
        Ok(Self(value))
    }
}

impl Default for TokenRequestTimeout {
    fn default() -> Self {
        Self(DEFAULT_TOKEN_REQUEST_TIMEOUT)
    }
}

fn map_token_error(error: RequestTokenError<OAuthHttpError, BasicErrorResponse>) -> OAuthError {
    match error {
        RequestTokenError::ServerResponse(response) => {
            OAuthError::TokenServer(token_error_kind(response.error().as_ref()))
        }
        RequestTokenError::Request(error) => error.into_oauth_error(),
        RequestTokenError::Parse(_, _) => OAuthError::MalformedTokenResponse,
        RequestTokenError::Other(_) => OAuthError::UnexpectedTokenStatus,
    }
}

fn token_error_kind(value: &str) -> TokenErrorKind {
    match value {
        "authorization_pending" => TokenErrorKind::AuthorizationPending,
        "slow_down" => TokenErrorKind::SlowDown,
        "access_denied" => TokenErrorKind::AccessDenied,
        "expired" | "expired_token" => TokenErrorKind::Expired,
        "invalid_grant" => TokenErrorKind::InvalidGrant,
        _ => TokenErrorKind::Other,
    }
}
