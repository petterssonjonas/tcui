use crate::app::message::Message;
use bytes::Bytes;
use color_eyre::eyre::eyre;
use futures::StreamExt;
use reqwest::header::CONTENT_TYPE;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone)]
pub(crate) struct ChatRequest {
    pub(crate) provider: String,
    pub(crate) endpoint: String,
    pub(crate) model: String,
    pub(crate) reasoning_effort: Option<String>,
    pub(crate) supported_reasoning_efforts: Vec<String>,
    pub(crate) backend_type: String,
    pub(crate) api_key: Option<String>,
    pub(crate) system_prompt: String,
    pub(crate) messages: Vec<Message>,
}

impl fmt::Debug for ChatRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ChatRequest")
            .field("provider", &self.provider)
            .field("endpoint", &self.endpoint)
            .field("model", &self.model)
            .field("reasoning_effort", &self.reasoning_effort)
            .field("backend_type", &self.backend_type)
            .field("has_api_key", &self.api_key.is_some())
            .field("system_prompt_bytes", &self.system_prompt.len())
            .field("message_count", &self.messages.len())
            .finish()
    }
}

pub(crate) struct PreparedChatRequest {
    request: ChatRequest,
    body: Bytes,
    codex_session: Option<crate::llm::codex_responses::PreparedCodexSession>,
}

impl fmt::Debug for PreparedChatRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedChatRequest")
            .field("provider", &self.request.provider)
            .field("endpoint", &self.request.endpoint)
            .field("model", &self.request.model)
            .field("backend_type", &self.request.backend_type)
            .field("message_count", &self.request.messages.len())
            .field("body_bytes", &self.body.len())
            .finish_non_exhaustive()
    }
}

pub(crate) fn prepare_chat_request(
    request: ChatRequest,
) -> color_eyre::Result<PreparedChatRequest> {
    if request.model.trim().is_empty() {
        return Err(eyre!("Choose a model before sending."));
    }
    if request.endpoint.trim().is_empty() {
        return Err(eyre!("Provider endpoint is empty."));
    }

    let codex_session = match request.backend_type.trim() {
        "anthropic" | "anthropic_sdk" => {
            let has_api_key = request
                .api_key
                .as_deref()
                .is_some_and(|key| !key.trim().is_empty());
            if !has_api_key {
                return Err(eyre!("Missing Anthropic API key."));
            }
            if request.backend_type.trim() == "anthropic_sdk"
                && request.endpoint.trim_end_matches('/') != "https://api.anthropic.com/v1"
            {
                return Err(eyre!(
                    "Anthropic Agent SDK requires the official Anthropic endpoint."
                ));
            }
            validate_credential_endpoint(&request)?;
            None
        }
        "codex" => {
            if !crate::llm::auth::trusted_provider_endpoint("Codex", &request.endpoint) {
                return Err(eyre!(
                    "Codex OAuth credentials can only be sent to the ChatGPT subscription endpoint"
                ));
            }
            crate::llm::codex_responses::validate_request(&request)?;
            Some(crate::llm::codex_responses::prepare_session(
                &request.provider,
            )?)
        }
        _ => {
            if crate::llm::auth::canonical_provider_name(&request.provider) == "Codex" {
                return Err(eyre!(
                    "Codex OAuth credentials must use the dedicated Codex Responses backend."
                ));
            }
            if !crate::llm::local::is_local_provider(&request.provider)
                && !request
                    .api_key
                    .as_deref()
                    .is_some_and(|key| !key.trim().is_empty())
            {
                return Err(eyre!(
                    "No saved provider credential. Use /connect, or /vault to unlock credentials."
                ));
            }
            if request
                .api_key
                .as_deref()
                .is_some_and(|key| !key.trim().is_empty())
            {
                validate_credential_endpoint(&request)?;
            }
            None
        }
    };
    let body = Bytes::from(serialize_request_body(&request)?);
    Ok(PreparedChatRequest {
        request,
        body,
        codex_session,
    })
}

pub(crate) fn serialize_request_body(request: &ChatRequest) -> color_eyre::Result<Vec<u8>> {
    match request.backend_type.trim() {
        "anthropic" | "anthropic_sdk" => Ok(serde_json::to_vec(&anthropic_request(request))?),
        "codex" => crate::llm::codex_responses::serialize_request_body(request),
        _ => Ok(serde_json::to_vec(&openai_request(request))?),
    }
}

#[derive(Debug, Serialize)]
struct WireRequest<'a> {
    model: &'a str,
    messages: Vec<WireMessage<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<&'a str>,
    stream: bool,
}

#[derive(Debug, Serialize)]
struct WireMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Debug, Deserialize)]
struct WireResponse {
    error: Option<WireError>,
}

#[derive(Debug, Deserialize)]
struct OpenAiStreamChunk {
    choices: Option<Vec<OpenAiStreamChoice>>,
    error: Option<WireError>,
    usage: Option<OpenAiUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAiStreamChoice {
    delta: Option<OpenAiDelta>,
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiDelta {
    content: Option<String>,
    reasoning_content: Option<String>,
    thinking: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiUsage {
    total_tokens: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct WireError {
    message: String,
}

#[derive(Debug, Serialize)]
struct AnthropicRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    system: Option<&'a str>,
    messages: Vec<WireMessage<'a>>,
    stream: bool,
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    error: Option<WireError>,
}

#[derive(Debug, Deserialize)]
pub(super) struct AnthropicStreamEvent {
    delta: Option<AnthropicDelta>,
    error: Option<WireError>,
}

#[derive(Debug, Deserialize)]
struct AnthropicDelta {
    text: Option<String>,
    thinking: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ChatStreamEvent {
    Answer(String),
    Thinking(String),
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct ChatStreamOutput {
    pub(crate) answer: String,
    pub(crate) thinking: String,
    pub(crate) total_tokens: Option<i64>,
}

impl ChatStreamOutput {
    pub(super) fn push(&mut self, event: &ChatStreamEvent) {
        match event {
            ChatStreamEvent::Answer(content) => self.answer.push_str(content),
            ChatStreamEvent::Thinking(content) => self.thinking.push_str(content),
        }
    }
}

pub(crate) async fn stream_chat<F>(
    request: ChatRequest,
    mut on_event: F,
) -> color_eyre::Result<ChatStreamOutput>
where
    F: FnMut(ChatStreamEvent) + Send,
{
    if request.model.trim().is_empty() {
        let output = ChatStreamOutput {
            answer: "Choose a model before sending.".to_string(),
            thinking: String::new(),
            total_tokens: None,
        };
        on_event(ChatStreamEvent::Answer(output.answer.clone()));
        return Ok(output);
    }

    let prepared = prepare_chat_request(request)?;
    stream_prepared_chat(prepared, on_event).await
}

pub(crate) async fn stream_prepared_chat<F>(
    prepared: PreparedChatRequest,
    mut on_event: F,
) -> color_eyre::Result<ChatStreamOutput>
where
    F: FnMut(ChatStreamEvent) + Send,
{
    let PreparedChatRequest {
        request,
        body,
        codex_session,
    } = prepared;
    crate::diagnostics::provider_request(&request.provider, &request.endpoint, &request.model);
    if request.backend_type.trim() == "anthropic_sdk" {
        return crate::llm::anthropic_sdk::stream(&request, body, &mut on_event).await;
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;

    let result = match request.backend_type.trim() {
        "anthropic" => stream_anthropic(&client, &request, body, &mut on_event).await,
        "codex" => {
            let Some(session) = codex_session else {
                return Err(eyre!("Codex request was not prepared with credentials."));
            };
            crate::llm::codex_responses::stream_prepared_codex_responses(
                &client,
                &request,
                body,
                session,
                &mut on_event,
            )
            .await
        }
        _ => stream_openai_compatible(&client, &request, body, &mut on_event).await,
    };
    result.map_err(|error| {
        let message = error.to_string();
        if request
            .api_key
            .as_deref()
            .is_some_and(|key| !key.is_empty() && message.contains(key))
        {
            eyre!(
                "{}",
                crate::llm::auth::redact_known_secret(&message, request.api_key.as_deref())
            )
        } else {
            error
        }
    })
}

async fn stream_openai_compatible<F>(
    client: &reqwest::Client,
    request: &ChatRequest,
    body: Bytes,
    on_event: &mut F,
) -> color_eyre::Result<ChatStreamOutput>
where
    F: FnMut(ChatStreamEvent) + Send,
{
    let url = format!(
        "{}/chat/completions",
        request.endpoint.trim_end_matches('/')
    );
    let mut builder = client
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .body(body);

    if let Some(api_key) = request
        .api_key
        .as_deref()
        .filter(|key| !key.trim().is_empty())
    {
        if !crate::llm::auth::trusted_provider_endpoint(&request.provider, &request.endpoint) {
            return Err(eyre!(
                "Refusing to send {provider} credentials to an untrusted endpoint.",
                provider = request.provider
            ));
        }
        builder = builder.bearer_auth(api_key);
    }
    if matches!(request.provider.as_str(), "OpenRouter" | "OpenRouter OAuth") {
        builder = builder
            .header("HTTP-Referer", "https://github.com/jp/TermChatUI")
            .header("X-Title", "TermChatUI");
    }
    if request.provider == "OpenCode Go" {
        builder = builder.header("User-Agent", concat!("tcui/", env!("CARGO_PKG_VERSION")));
        if let Some(message) = request.messages.first() {
            builder = builder.header(
                "x-opencode-session",
                format!("tcui-conversation-{}", message.conversation_id),
            );
        }
    }

    let response = builder.send().await.inspect_err(|err| {
        crate::diagnostics::provider_error(&request.provider, &err.to_string())
    })?;
    let status = response.status();
    crate::diagnostics::provider_response(&request.provider, status);
    if !status.is_success() {
        return provider_http_error(
            response,
            status,
            &request.provider,
            request.api_key.as_deref(),
        )
        .await;
    }

    let mut output = ChatStreamOutput::default();
    read_sse(response, |data| {
        let chunk = openai_stream_events(data)?;
        if let Some(total_tokens) = chunk.total_tokens {
            output.total_tokens = Some(total_tokens);
        }
        for event in chunk.events {
            output.push(&event);
            on_event(event);
        }
        Ok(true)
    })
    .await?;
    ensure_not_empty(&mut output, on_event, &request.provider);
    Ok(output)
}

async fn stream_anthropic<F>(
    client: &reqwest::Client,
    request: &ChatRequest,
    body: Bytes,
    on_event: &mut F,
) -> color_eyre::Result<ChatStreamOutput>
where
    F: FnMut(ChatStreamEvent) + Send,
{
    let Some(api_key) = request
        .api_key
        .as_deref()
        .filter(|key| !key.trim().is_empty())
    else {
        return Err(eyre!("Missing Anthropic API key."));
    };
    let url = format!("{}/messages", request.endpoint.trim_end_matches('/'));
    let response = client
        .post(url)
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header(CONTENT_TYPE, "application/json")
        .body(body)
        .send()
        .await
        .inspect_err(|err| {
            crate::diagnostics::provider_error(&request.provider, &err.to_string())
        })?;
    let status = response.status();
    crate::diagnostics::provider_response(&request.provider, status);
    if !status.is_success() {
        return provider_http_error(
            response,
            status,
            &request.provider,
            request.api_key.as_deref(),
        )
        .await;
    }

    let mut output = ChatStreamOutput::default();
    read_sse(response, |data| {
        for event in anthropic_stream_events(data)? {
            output.push(&event);
            on_event(event);
        }
        Ok(true)
    })
    .await?;
    ensure_not_empty(&mut output, on_event, &request.provider);
    Ok(output)
}

fn validate_credential_endpoint(request: &ChatRequest) -> color_eyre::Result<()> {
    if crate::llm::auth::trusted_provider_endpoint(&request.provider, &request.endpoint) {
        return Ok(());
    }
    Err(eyre!(
        "Refusing to send {provider} credentials to an untrusted endpoint.",
        provider = request.provider
    ))
}

fn openai_request(request: &ChatRequest) -> WireRequest<'_> {
    let mut messages = Vec::with_capacity(request.messages.len() + 1);
    if !request.system_prompt.trim().is_empty() {
        messages.push(WireMessage {
            role: "system",
            content: request.system_prompt.as_str(),
        });
    }
    messages.extend(
        request
            .messages
            .iter()
            .filter(|message| !message.content.trim().is_empty())
            .map(|message| WireMessage {
                role: message.role.as_str(),
                content: message.content.as_str(),
            }),
    );
    WireRequest {
        model: request.model.as_str(),
        messages,
        reasoning_effort: request.reasoning_effort.as_deref(),
        stream: true,
    }
}

fn anthropic_request(request: &ChatRequest) -> AnthropicRequest<'_> {
    let messages = request
        .messages
        .iter()
        .filter(|message| !message.content.trim().is_empty() && message.role != "system")
        .map(|message| WireMessage {
            role: if message.role == "assistant" {
                "assistant"
            } else {
                "user"
            },
            content: message.content.as_str(),
        })
        .collect();
    AnthropicRequest {
        model: request.model.as_str(),
        max_tokens: 4096,
        system: (!request.system_prompt.trim().is_empty())
            .then_some(request.system_prompt.as_str()),
        messages,
        stream: true,
    }
}

async fn provider_http_error<T>(
    response: reqwest::Response,
    status: reqwest::StatusCode,
    provider: &str,
    api_key: Option<&str>,
) -> color_eyre::Result<T> {
    let body = response.text().await?;
    let message = serde_json::from_str::<WireResponse>(&body)
        .ok()
        .and_then(|body| body.error.map(|error| error.message))
        .or_else(|| {
            serde_json::from_str::<AnthropicResponse>(&body)
                .ok()
                .and_then(|body| body.error.map(|error| error.message))
        })
        .unwrap_or_else(|| format!("Provider returned HTTP {status}"));
    let message = crate::llm::auth::redact_known_secret(&message, api_key);
    crate::diagnostics::provider_error(provider, &message);
    Err(eyre!("{}", message))
}

pub(super) async fn read_sse<F>(
    response: reqwest::Response,
    mut on_data: F,
) -> color_eyre::Result<()>
where
    F: FnMut(&str) -> color_eyre::Result<bool>,
{
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        bytes.extend_from_slice(&chunk);
        if !consume_sse_bytes(&mut bytes, &mut on_data)? {
            return Ok(());
        }
    }

    let buffer = std::str::from_utf8(&bytes)?;
    if !buffer.trim().is_empty() {
        handle_sse_event(buffer, &mut on_data)?;
    }
    Ok(())
}

fn consume_sse_bytes<F>(bytes: &mut Vec<u8>, on_data: &mut F) -> color_eyre::Result<bool>
where
    F: FnMut(&str) -> color_eyre::Result<bool>,
{
    let Ok(buffer) = std::str::from_utf8(bytes) else {
        return Ok(true);
    };

    let mut consumed = 0;
    while let Some((raw_event, event_end)) = next_sse_event(&buffer[consumed..]) {
        let raw_event = raw_event.to_string();
        consumed += event_end;
        if !handle_sse_event(&raw_event, on_data)? {
            return Ok(false);
        }
    }
    if consumed > 0 {
        bytes.drain(..consumed);
    }
    Ok(true)
}

fn next_sse_event(buffer: &str) -> Option<(&str, usize)> {
    let lf = buffer.find("\n\n").map(|idx| (idx, idx + 2));
    let crlf = buffer.find("\r\n\r\n").map(|idx| (idx, idx + 4));
    match (lf, crlf) {
        (Some((lf_idx, lf_end)), Some((crlf_idx, crlf_end))) => {
            if lf_idx < crlf_idx {
                Some((&buffer[..lf_idx], lf_end))
            } else {
                Some((&buffer[..crlf_idx], crlf_end))
            }
        }
        (Some((idx, end)), None) | (None, Some((idx, end))) => Some((&buffer[..idx], end)),
        (None, None) => None,
    }
}

fn handle_sse_event<F>(raw_event: &str, on_data: &mut F) -> color_eyre::Result<bool>
where
    F: FnMut(&str) -> color_eyre::Result<bool>,
{
    let data = raw_event
        .lines()
        .filter_map(|line| line.trim_end().strip_prefix("data:"))
        .map(str::trim_start)
        .collect::<Vec<_>>()
        .join("\n");
    if data.trim().is_empty() {
        return Ok(true);
    }
    if data.trim() == "[DONE]" {
        return Ok(false);
    }
    on_data(&data)
}

fn openai_stream_events(data: &str) -> color_eyre::Result<OpenAiChunkResult> {
    let parsed: OpenAiStreamChunk = serde_json::from_str(data)?;
    if let Some(error) = parsed.error {
        return Err(eyre!(
            "{}",
            crate::llm::auth::redact_secrets(&error.message)
        ));
    }

    let mut events = Vec::new();
    for choice in parsed.choices.unwrap_or_default() {
        if let Some(delta) = choice.delta {
            push_if_present(
                &mut events,
                ChatStreamEvent::Thinking,
                delta.reasoning_content,
            );
            push_if_present(&mut events, ChatStreamEvent::Thinking, delta.thinking);
            push_if_present(&mut events, ChatStreamEvent::Answer, delta.content);
        }
        push_if_present(&mut events, ChatStreamEvent::Answer, choice.text);
    }
    Ok(OpenAiChunkResult {
        events,
        total_tokens: parsed.usage.and_then(|usage| usage.total_tokens),
    })
}

struct OpenAiChunkResult {
    events: Vec<ChatStreamEvent>,
    total_tokens: Option<i64>,
}

fn anthropic_stream_events(data: &str) -> color_eyre::Result<Vec<ChatStreamEvent>> {
    anthropic_events(serde_json::from_str(data)?)
}

pub(super) fn anthropic_events(
    parsed: AnthropicStreamEvent,
) -> color_eyre::Result<Vec<ChatStreamEvent>> {
    if let Some(error) = parsed.error {
        return Err(eyre!(
            "{}",
            crate::llm::auth::redact_secrets(&error.message)
        ));
    }

    let mut events = Vec::new();
    if let Some(delta) = parsed.delta {
        push_if_present(&mut events, ChatStreamEvent::Thinking, delta.thinking);
        push_if_present(&mut events, ChatStreamEvent::Answer, delta.text);
    }
    Ok(events)
}

fn push_if_present(
    events: &mut Vec<ChatStreamEvent>,
    event: fn(String) -> ChatStreamEvent,
    content: Option<String>,
) {
    if let Some(content) = content.filter(|content| !content.is_empty()) {
        events.push(event(content));
    }
}

pub(super) fn ensure_not_empty<F>(output: &mut ChatStreamOutput, on_event: &mut F, provider: &str)
where
    F: FnMut(ChatStreamEvent) + Send,
{
    if output.answer.trim().is_empty() && output.thinking.trim().is_empty() {
        crate::diagnostics::provider_error(provider, "provider returned an empty response");
        let event = ChatStreamEvent::Answer("Provider returned an empty response.".to_string());
        output.push(&event);
        on_event(event);
    }
}

#[cfg(test)]
#[path = "chat_tests.rs"]
mod chat_tests;
