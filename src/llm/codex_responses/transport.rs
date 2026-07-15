use std::fmt;
use std::time::Duration;

use tokio::time::{timeout_at, Instant};

mod framing;

use framing::SseDecoder;

#[derive(Clone, Copy, Debug)]
pub(super) struct TransportLimits {
    pub(super) max_stream_bytes: usize,
    pub(super) max_event_bytes: usize,
    pub(super) max_error_body_bytes: usize,
    pub(super) deadline: Duration,
}

impl TransportLimits {
    pub(super) const PRODUCTION: Self = Self {
        max_stream_bytes: 2 * 1024 * 1024,
        max_event_bytes: 128 * 1024,
        max_error_body_bytes: 64 * 1024,
        deadline: Duration::from_secs(120),
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CodexHttpStatus {
    BadRequest,
    Forbidden,
    RateLimited,
    InternalServerError,
    Other(u16),
}

impl From<reqwest::StatusCode> for CodexHttpStatus {
    fn from(status: reqwest::StatusCode) -> Self {
        match status {
            reqwest::StatusCode::BAD_REQUEST => Self::BadRequest,
            reqwest::StatusCode::FORBIDDEN => Self::Forbidden,
            reqwest::StatusCode::TOO_MANY_REQUESTS => Self::RateLimited,
            reqwest::StatusCode::INTERNAL_SERVER_ERROR => Self::InternalServerError,
            status => Self::Other(status.as_u16()),
        }
    }
}

impl fmt::Display for CodexHttpStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadRequest => formatter.write_str("400 Bad Request"),
            Self::Forbidden => formatter.write_str("403 Forbidden"),
            Self::RateLimited => formatter.write_str("429 Too Many Requests"),
            Self::InternalServerError => formatter.write_str("500 Internal Server Error"),
            Self::Other(status) => write!(formatter, "HTTP {status}"),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub(super) enum CodexTransportError {
    #[error("Codex response stream exceeded its total byte limit")]
    SseTotalTooLarge,
    #[error("Codex response stream exceeded its per-event byte limit")]
    SseEventTooLarge,
    #[error("Codex response stream contains invalid UTF-8")]
    InvalidSseUtf8,
    #[error("Codex response stream ended with an unterminated SSE event")]
    UnterminatedSse,
    #[error("Codex response stream deadline elapsed")]
    StreamDeadline,
    #[error("Codex response error body exceeded its byte limit")]
    ErrorBodyTooLarge,
    #[error("Codex response error body could not be decoded")]
    InvalidErrorBodyUtf8,
    #[error("Codex response body could not be read")]
    ResponseBody,
    #[error("Codex response stream could not be read")]
    ResponseStream,
    #[error("Codex response event failed: {message}")]
    Event { message: String },
    #[error("Codex returned HTTP {status}: {message}")]
    Http {
        status: CodexHttpStatus,
        message: String,
    },
    #[error("Codex stream ended before response.completed")]
    IncompleteResponse,
}

pub(super) async fn read_sse<F>(
    mut response: reqwest::Response,
    limits: TransportLimits,
    mut on_data: F,
) -> Result<(), CodexTransportError>
where
    F: FnMut(&str) -> Result<bool, CodexTransportError>,
{
    let deadline = Instant::now() + limits.deadline;
    let mut total_bytes: usize = 0;
    let mut decoder = SseDecoder::new(limits.max_event_bytes);
    loop {
        let chunk = timeout_at(deadline, response.chunk())
            .await
            .map_err(|_| CodexTransportError::StreamDeadline)?
            .map_err(|_| CodexTransportError::ResponseStream)?;
        let Some(chunk) = chunk else {
            return decoder.finish(&mut on_data);
        };
        total_bytes = total_bytes
            .checked_add(chunk.len())
            .ok_or(CodexTransportError::SseTotalTooLarge)?;
        if total_bytes > limits.max_stream_bytes {
            return Err(CodexTransportError::SseTotalTooLarge);
        }
        if !decoder.push(&chunk, &mut on_data)? {
            return Ok(());
        }
    }
}

pub(super) async fn http_error(
    mut response: reqwest::Response,
    provider: &str,
    limits: TransportLimits,
) -> CodexTransportError {
    let status = response.status();
    let body = match read_error_body(&mut response, limits).await {
        Ok(body) => body,
        Err(error) => return error,
    };
    let body = match String::from_utf8(body) {
        Ok(body) => body,
        Err(_) => return CodexTransportError::InvalidErrorBodyUtf8,
    };
    let message = super::sse::response_error_message(&body)
        .unwrap_or_else(|| format!("Provider returned HTTP {status}"));
    let message = crate::llm::auth::redact_secrets(&message);
    crate::diagnostics::provider_error(provider, &message);
    CodexTransportError::Http {
        status: status.into(),
        message,
    }
}

async fn read_error_body(
    response: &mut reqwest::Response,
    limits: TransportLimits,
) -> Result<Vec<u8>, CodexTransportError> {
    let deadline = Instant::now() + limits.deadline;
    let mut body = Vec::new();
    loop {
        let chunk = timeout_at(deadline, response.chunk())
            .await
            .map_err(|_| CodexTransportError::StreamDeadline)?
            .map_err(|_| CodexTransportError::ResponseBody)?;
        let Some(chunk) = chunk else {
            return Ok(body);
        };
        let size = body
            .len()
            .checked_add(chunk.len())
            .ok_or(CodexTransportError::ErrorBodyTooLarge)?;
        if size > limits.max_error_body_bytes {
            return Err(CodexTransportError::ErrorBodyTooLarge);
        }
        body.extend_from_slice(&chunk);
    }
}
