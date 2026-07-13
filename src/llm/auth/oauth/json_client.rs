use std::fmt;
use std::time::Duration;

use reqwest::{Client, Url, redirect::Policy};
use tokio::time::Instant;

use super::http_client::{BoundedOAuthHttpClient, OAuthHttpError};
use super::{OAuthCancellation, OAuthError};

pub(crate) struct OAuthJsonPost {
    endpoint: Url,
    body: Vec<u8>,
    bearer_token: Option<String>,
}

impl OAuthJsonPost {
    pub(crate) fn new(endpoint: Url, body: Vec<u8>) -> Self {
        Self {
            endpoint,
            body,
            bearer_token: None,
        }
    }

    pub(crate) fn with_bearer(mut self, bearer_token: &str) -> Result<Self, OAuthError> {
        if bearer_token.trim().is_empty() {
            return Err(OAuthError::InvalidValue);
        }
        self.bearer_token = Some(bearer_token.to_owned());
        Ok(self)
    }
}

impl fmt::Debug for OAuthJsonPost {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OAuthJsonPost")
            .field("endpoint", &self.endpoint)
            .field("body", &"<redacted>")
            .field(
                "bearer_token",
                &self.bearer_token.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

#[derive(Clone)]
pub(crate) struct OAuthJsonService {
    client: Client,
    timeout: Duration,
}

impl OAuthJsonService {
    pub(crate) fn hardened(timeout: Duration) -> Result<Self, OAuthError> {
        if timeout.is_zero() {
            return Err(OAuthError::InvalidValue);
        }
        let client = Client::builder()
            .redirect(Policy::none())
            .build()
            .map_err(|_| OAuthError::TokenTransport)?;
        Ok(Self { client, timeout })
    }

    pub(crate) async fn post(
        &self,
        post: OAuthJsonPost,
        cancellation: &OAuthCancellation,
    ) -> Result<Vec<u8>, OAuthError> {
        let deadline = Instant::now()
            .checked_add(self.timeout)
            .ok_or(OAuthError::TokenDeadline)?;
        let mut request = oauth2::http::Request::builder()
            .method(oauth2::http::Method::POST)
            .uri(post.endpoint.as_str())
            .header(oauth2::http::header::ACCEPT, "application/json")
            .header(oauth2::http::header::CONTENT_TYPE, "application/json");
        if let Some(bearer_token) = post.bearer_token {
            request = request.header(
                oauth2::http::header::AUTHORIZATION,
                format!("Bearer {bearer_token}"),
            );
        }
        let request = request
            .body(post.body)
            .map_err(|_| OAuthError::TokenTransport)?;
        let client = BoundedOAuthHttpClient::new(&self.client, cancellation, deadline);
        client
            .call_json(request)
            .await
            .map(|response| response.into_body())
            .map_err(OAuthHttpError::into_oauth_error)
    }
}
