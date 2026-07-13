use std::future::Future;
use std::pin::Pin;

use oauth2::{AsyncHttpClient, HttpRequest, HttpResponse};
use reqwest::{Client, Response};
use serde::Deserialize;
use tokio::time::Instant;

use super::deadline::OperationDeadline;
use super::{OAuthCancellation, OAuthError};

pub(super) const MAX_TOKEN_REQUEST_BYTES: usize = 16_384;
const MAX_TOKEN_REQUEST_FIELD_BYTES: usize = 4_096;
const MAX_TOKEN_RESPONSE_BYTES: usize = 16_384;

pub(super) struct BoundedOAuthHttpClient {
    client: Client,
    cancellation: OAuthCancellation,
    deadline: Instant,
}

impl BoundedOAuthHttpClient {
    pub(super) fn new(
        client: &Client,
        cancellation: &OAuthCancellation,
        deadline: Instant,
    ) -> Self {
        Self {
            client: client.clone(),
            cancellation: cancellation.clone(),
            deadline,
        }
    }

    fn deadline(&self) -> OperationDeadline {
        OperationDeadline::new(self.deadline, OAuthError::TokenDeadline)
    }

    async fn read_response(&self, response: Response) -> Result<HttpResponse, OAuthHttpError> {
        let status = response.status();
        let version = response.version();
        let headers = response.headers().clone();
        if response.content_length().is_some_and(|length| {
            usize::try_from(length).map_or(true, |length| length > MAX_TOKEN_RESPONSE_BYTES)
        }) {
            return Err(OAuthHttpError::ResponseTooLarge);
        }
        let mut response = response;
        let mut body = Vec::new();
        loop {
            let chunk = self
                .deadline()
                .race(&self.cancellation, response.chunk())
                .await
                .map_err(OAuthHttpError::from)?
                .map_err(|_| OAuthHttpError::Transport)?;
            let Some(chunk) = chunk else {
                break;
            };
            let length = body
                .len()
                .checked_add(chunk.len())
                .ok_or(OAuthHttpError::ResponseTooLarge)?;
            if length > MAX_TOKEN_RESPONSE_BYTES {
                return Err(OAuthHttpError::ResponseTooLarge);
            }
            body.extend_from_slice(&chunk);
        }
        if status != oauth2::http::StatusCode::OK && !has_oauth_error_code(&body) {
            return Err(OAuthHttpError::UnexpectedStatus);
        }
        let mut builder = oauth2::http::Response::builder()
            .status(status)
            .version(version);
        for (name, value) in &headers {
            builder = builder.header(name, value);
        }
        builder.body(body).map_err(|_| OAuthHttpError::Transport)
    }

    pub(super) async fn call_json(
        &self,
        request: HttpRequest,
    ) -> Result<HttpResponse, OAuthHttpError> {
        validate_json(request.body())?;
        let request = request.try_into().map_err(|_| OAuthHttpError::Transport)?;
        let response = self
            .deadline()
            .race(&self.cancellation, self.client.execute(request))
            .await
            .map_err(OAuthHttpError::from)?
            .map_err(|_| OAuthHttpError::Transport)?;
        self.read_response(response).await
    }
}

#[derive(Deserialize)]
struct OAuthErrorEnvelope {
    #[serde(rename = "error")]
    _error: String,
}

fn has_oauth_error_code(body: &[u8]) -> bool {
    serde_json::from_slice::<OAuthErrorEnvelope>(body).is_ok()
}

impl<'request> AsyncHttpClient<'request> for BoundedOAuthHttpClient {
    type Error = OAuthHttpError;
    type Future =
        Pin<Box<dyn Future<Output = Result<HttpResponse, Self::Error>> + Send + 'request>>;

    fn call(&'request self, request: HttpRequest) -> Self::Future {
        Box::pin(async move {
            validate_form(request.body())?;
            let request = request.try_into().map_err(|_| OAuthHttpError::Transport)?;
            let response = self
                .deadline()
                .race(&self.cancellation, self.client.execute(request))
                .await
                .map_err(OAuthHttpError::from)?
                .map_err(|_| OAuthHttpError::Transport)?;
            self.read_response(response).await
        })
    }
}

pub(super) fn validate_form(body: &[u8]) -> Result<(), OAuthHttpError> {
    if body.len() > MAX_TOKEN_REQUEST_BYTES
        || oauth2::url::form_urlencoded::parse(body)
            .any(|(_, value)| value.len() > MAX_TOKEN_REQUEST_FIELD_BYTES)
    {
        return Err(OAuthHttpError::InvalidRequest);
    }
    Ok(())
}

pub(super) fn validate_json(body: &[u8]) -> Result<(), OAuthHttpError> {
    if body.len() > MAX_TOKEN_REQUEST_BYTES {
        return Err(OAuthHttpError::InvalidRequest);
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub(super) enum OAuthHttpError {
    #[error("OAuth request was cancelled")]
    Cancelled,
    #[error("OAuth request deadline elapsed")]
    Deadline,
    #[error("OAuth request is too large")]
    InvalidRequest,
    #[error("OAuth response is too large")]
    ResponseTooLarge,
    #[error("OAuth response has an invalid non-success status")]
    UnexpectedStatus,
    #[error("OAuth transport failed")]
    Transport,
}

impl From<OAuthError> for OAuthHttpError {
    fn from(value: OAuthError) -> Self {
        match value {
            OAuthError::Cancelled => Self::Cancelled,
            OAuthError::TokenDeadline => Self::Deadline,
            OAuthError::TokenResponseTooLarge => Self::ResponseTooLarge,
            _ => Self::Transport,
        }
    }
}

impl OAuthHttpError {
    pub(super) fn into_oauth_error(self) -> OAuthError {
        match self {
            Self::Cancelled => OAuthError::Cancelled,
            Self::Deadline => OAuthError::TokenDeadline,
            Self::InvalidRequest => OAuthError::InvalidValue,
            Self::ResponseTooLarge => OAuthError::TokenResponseTooLarge,
            Self::UnexpectedStatus => OAuthError::UnexpectedTokenStatus,
            Self::Transport => OAuthError::TokenTransport,
        }
    }
}
