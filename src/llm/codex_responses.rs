use std::future::Future;

use bytes::Bytes;
use color_eyre::eyre::eyre;
use secrecy::ExposeSecret;

use crate::config::AppConfig;
use crate::llm::auth::codex::{resolve_credential, CodexCredentialSource, CodexNativeAdapter};
use crate::llm::auth::oauth::oauth_cancellation;
use crate::llm::chat::{
    ensure_not_empty, flush_title_filter, ChatRequest, ChatStreamEvent, ChatStreamOutput,
    TitleTagFilter,
};

mod request;
mod sse;
mod transport;

use request::{build_request_body, codex_headers, validate_reasoning_effort};
use sse::codex_stream_event;
use transport::{http_error, read_sse, CodexTransportError, TransportLimits};

pub(crate) fn serialize_request_body(request: &ChatRequest) -> color_eyre::Result<Vec<u8>> {
    Ok(serde_json::to_vec(&build_request_body(request))?)
}

pub(crate) fn validate_request(request: &ChatRequest) -> color_eyre::Result<()> {
    validate_reasoning_effort(request)
}

pub(crate) fn prepare_session() -> color_eyre::Result<PreparedCodexSession> {
    let config = AppConfig::load()?;
    let session = load_session(&config)?;
    Ok(PreparedCodexSession { session, config })
}

#[derive(Clone)]
pub(crate) struct CodexSession {
    access_token: String,
    account_id: String,
    source: CodexCredentialSource,
}

pub(crate) struct PreparedCodexSession {
    session: CodexSession,
    config: AppConfig,
}

#[cfg(test)]
pub(crate) async fn stream_codex_responses<F>(
    client: &reqwest::Client,
    request: &ChatRequest,
    on_event: &mut F,
) -> color_eyre::Result<ChatStreamOutput>
where
    F: FnMut(ChatStreamEvent) + Send,
{
    if !crate::llm::auth::trusted_provider_endpoint("Codex", &request.endpoint) {
        return Err(eyre!(
            "Codex OAuth credentials can only be sent to the ChatGPT subscription endpoint"
        ));
    }
    let config = AppConfig::load()?;
    let session = load_session(&config)?;
    let source = session.source;
    let body = Bytes::from(serialize_request_body(request)?);
    stream_prepared_with_one_refresh(
        client,
        request,
        body,
        session,
        || async { refresh_session(&config, source).await },
        on_event,
    )
    .await
}

pub(crate) async fn stream_prepared_codex_responses<F>(
    client: &reqwest::Client,
    request: &ChatRequest,
    body: Bytes,
    prepared: PreparedCodexSession,
    on_event: &mut F,
) -> color_eyre::Result<ChatStreamOutput>
where
    F: FnMut(ChatStreamEvent) + Send,
{
    let source = prepared.session.source;
    let config = prepared.config;
    stream_prepared_with_one_refresh(
        client,
        request,
        body,
        prepared.session,
        || async { refresh_session(&config, source).await },
        on_event,
    )
    .await
}

#[cfg(test)]
async fn stream_with_one_refresh<F, Fut, H>(
    client: &reqwest::Client,
    request: &ChatRequest,
    session: CodexSession,
    refresh: F,
    on_event: &mut H,
) -> color_eyre::Result<ChatStreamOutput>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = color_eyre::Result<CodexSession>>,
    H: FnMut(ChatStreamEvent) + Send,
{
    let body = Bytes::from(serialize_request_body(request)?);
    stream_prepared_with_one_refresh(client, request, body, session, refresh, on_event).await
}

async fn stream_prepared_with_one_refresh<F, Fut, H>(
    client: &reqwest::Client,
    request: &ChatRequest,
    body: Bytes,
    session: CodexSession,
    refresh: F,
    on_event: &mut H,
) -> color_eyre::Result<ChatStreamOutput>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = color_eyre::Result<CodexSession>>,
    H: FnMut(ChatStreamEvent) + Send,
{
    let response = send_serialized_request(client, request, body.clone(), &session).await?;
    let response = if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        let refreshed = refresh().await?;
        send_serialized_request(client, request, body, &refreshed).await?
    } else {
        response
    };

    crate::diagnostics::provider_response(&request.provider, response.status());
    if !response.status().is_success() {
        return codex_http_error(response, &request.provider).await;
    }

    let mut output = ChatStreamOutput::default();
    let mut title_filter = TitleTagFilter::default();
    let mut completed = false;
    read_sse(response, TransportLimits::PRODUCTION, |data| {
        let chunk = codex_stream_event(data, &mut title_filter).map_err(|error| {
            CodexTransportError::Event {
                message: crate::llm::auth::redact_secrets(&error.to_string()),
            }
        })?;
        if let Some(total_tokens) = chunk.total_tokens {
            output.total_tokens = Some(total_tokens);
        }
        for event in chunk.events {
            output.push(&event);
            on_event(event);
        }
        completed = chunk.completed;
        Ok(!completed)
    })
    .await?;
    if !completed {
        return Err(CodexTransportError::IncompleteResponse.into());
    }
    flush_title_filter(&mut output, on_event, title_filter);
    ensure_not_empty(&mut output, on_event, &request.provider);
    Ok(output)
}

#[cfg(test)]
async fn send_request(
    client: &reqwest::Client,
    request: &ChatRequest,
    session: &CodexSession,
) -> color_eyre::Result<reqwest::Response> {
    let body = Bytes::from(serialize_request_body(request)?);
    send_serialized_request(client, request, body, session).await
}

async fn send_serialized_request(
    client: &reqwest::Client,
    request: &ChatRequest,
    body: Bytes,
    session: &CodexSession,
) -> color_eyre::Result<reqwest::Response> {
    validate_reasoning_effort(request)?;
    let url = format!("{}/responses", request.endpoint.trim_end_matches('/'));
    crate::diagnostics::provider_request(&request.provider, &url, &request.model);
    client
        .post(url)
        .headers(codex_headers(
            &session.access_token,
            &session.account_id,
            request,
        )?)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body)
        .send()
        .await
        .inspect_err(|error| {
            crate::diagnostics::provider_error(&request.provider, &error.to_string())
        })
        .map_err(Into::into)
}

fn load_session(config: &AppConfig) -> color_eyre::Result<CodexSession> {
    let credential = resolve_credential(config)?
        .ok_or_else(|| eyre!("No Codex OAuth credential is available. Run `codex login`."))?;
    let account_id = credential
        .account_id()
        .filter(|account_id| !account_id.trim().is_empty())
        .ok_or_else(|| eyre!("Codex credentials do not include a ChatGPT account ID."))?;
    Ok(CodexSession {
        access_token: credential.access_token().expose_secret().to_owned(),
        account_id: account_id.to_string(),
        source: credential.source(),
    })
}

async fn refresh_session(
    config: &AppConfig,
    source: CodexCredentialSource,
) -> color_eyre::Result<CodexSession> {
    if source == CodexCredentialSource::TcuiNative {
        let (cancellation, _) = oauth_cancellation();
        CodexNativeAdapter::production()?
            .refresh(config, &cancellation)
            .await?;
    }
    load_session(config)
}

async fn codex_http_error<T>(response: reqwest::Response, provider: &str) -> color_eyre::Result<T> {
    Err(http_error(response, provider, TransportLimits::PRODUCTION)
        .await
        .into())
}

#[cfg(test)]
#[path = "codex_responses_request_tests.rs"]
mod request_tests;
#[cfg(test)]
#[path = "codex_responses_sse_tests.rs"]
mod sse_tests;
#[cfg(test)]
#[path = "codex_responses_transport_tests.rs"]
mod transport_tests;
