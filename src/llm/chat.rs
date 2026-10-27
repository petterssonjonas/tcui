use crate::app::message::Message;
use color_eyre::eyre::eyre;
use futures::StreamExt;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
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
struct AnthropicStreamEvent {
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
    Title(String),
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
            ChatStreamEvent::Title(_) => {}
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

    crate::diagnostics::provider_request(&request.provider, &request.endpoint, &request.model);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;

    match request.backend_type.trim() {
        "anthropic" => stream_anthropic(&client, &request, &mut on_event).await,
        "codex" => {
            crate::llm::codex_responses::stream_codex_responses(&client, &request, &mut on_event)
                .await
        }
        _ => stream_openai_compatible(&client, &request, &mut on_event).await,
    }
}

async fn stream_openai_compatible<F>(
    client: &reqwest::Client,
    request: &ChatRequest,
    on_event: &mut F,
) -> color_eyre::Result<ChatStreamOutput>
where
    F: FnMut(ChatStreamEvent) + Send,
{
    if crate::llm::auth::canonical_provider_name(&request.provider) == "Codex" {
        return Err(eyre!(
            "Codex OAuth credentials must use the dedicated Codex Responses backend."
        ));
    }
    let url = format!(
        "{}/chat/completions",
        request.endpoint.trim_end_matches('/')
    );
    let mut messages = Vec::with_capacity(request.messages.len() + 1);
    if !request.system_prompt.trim().is_empty() {
        messages.push(WireMessage {
            role: "system",
            content: request.system_prompt.as_str(),
        });
    }
    for message in &request.messages {
        if !message.content.trim().is_empty() {
            messages.push(WireMessage {
                role: message.role.as_str(),
                content: message.content.as_str(),
            });
        }
    }

    let mut builder = client.post(url).json(&WireRequest {
        model: request.model.as_str(),
        messages,
        reasoning_effort: request.reasoning_effort.as_deref(),
        stream: true,
    });

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
    if request.provider == "OpenRouter" {
        builder = builder
            .header("HTTP-Referer", "https://github.com/jp/TermChatUI")
            .header("X-Title", "TermChatUI");
    }

    let response = builder.send().await.inspect_err(|err| {
        crate::diagnostics::provider_error(&request.provider, &err.to_string())
    })?;
    let status = response.status();
    crate::diagnostics::provider_response(&request.provider, status);
    if !status.is_success() {
        return provider_http_error(response, status, &request.provider).await;
    }

    let mut output = ChatStreamOutput::default();
    let mut title_filter = TitleTagFilter::default();
    read_sse(response, |data| {
        let chunk = openai_stream_events(data, &mut title_filter)?;
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
    flush_title_filter(&mut output, on_event, title_filter);
    ensure_not_empty(&mut output, on_event, &request.provider);
    Ok(output)
}

async fn stream_anthropic<F>(
    client: &reqwest::Client,
    request: &ChatRequest,
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
    if !crate::llm::auth::trusted_provider_endpoint(&request.provider, &request.endpoint) {
        return Err(eyre!(
            "Refusing to send {provider} credentials to an untrusted endpoint.",
            provider = request.provider
        ));
    }

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
    let url = format!("{}/messages", request.endpoint.trim_end_matches('/'));
    let response = client
        .post(url)
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&AnthropicRequest {
            model: request.model.as_str(),
            max_tokens: 4096,
            system: (!request.system_prompt.trim().is_empty())
                .then_some(request.system_prompt.as_str()),
            messages,
            stream: true,
        })
        .send()
        .await
        .inspect_err(|err| {
            crate::diagnostics::provider_error(&request.provider, &err.to_string())
        })?;
    let status = response.status();
    crate::diagnostics::provider_response(&request.provider, status);
    if !status.is_success() {
        return provider_http_error(response, status, &request.provider).await;
    }

    let mut output = ChatStreamOutput::default();
    let mut title_filter = TitleTagFilter::default();
    read_sse(response, |data| {
        for event in anthropic_stream_events(data, &mut title_filter)? {
            output.push(&event);
            on_event(event);
        }
        Ok(true)
    })
    .await?;
    flush_title_filter(&mut output, on_event, title_filter);
    ensure_not_empty(&mut output, on_event, &request.provider);
    Ok(output)
}

async fn provider_http_error<T>(
    response: reqwest::Response,
    status: reqwest::StatusCode,
    provider: &str,
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
    crate::diagnostics::provider_error(provider, &message);
    Err(eyre!("{}", crate::llm::auth::redact_secrets(&message)))
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

fn openai_stream_events(
    data: &str,
    title_filter: &mut TitleTagFilter,
) -> color_eyre::Result<OpenAiChunkResult> {
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
            if let Some(content) = delta.content {
                push_title_filtered_content(&mut events, title_filter.push(&content));
            }
        }
        if let Some(text) = choice.text {
            push_title_filtered_content(&mut events, title_filter.push(&text));
        }
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

fn anthropic_stream_events(
    data: &str,
    title_filter: &mut TitleTagFilter,
) -> color_eyre::Result<Vec<ChatStreamEvent>> {
    let parsed: AnthropicStreamEvent = serde_json::from_str(data)?;
    if let Some(error) = parsed.error {
        return Err(eyre!(
            "{}",
            crate::llm::auth::redact_secrets(&error.message)
        ));
    }

    let mut events = Vec::new();
    if let Some(delta) = parsed.delta {
        push_if_present(&mut events, ChatStreamEvent::Thinking, delta.thinking);
        if let Some(text) = delta.text {
            push_title_filtered_content(&mut events, title_filter.push(&text));
        }
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

pub(super) fn push_title_filtered_content(
    events: &mut Vec<ChatStreamEvent>,
    chunk: TitleFilteredChunk,
) {
    if let Some(title) = chunk.title {
        events.push(ChatStreamEvent::Title(title));
    }
    if !chunk.visible.is_empty() {
        events.push(ChatStreamEvent::Answer(chunk.visible));
    }
}

pub(super) fn flush_title_filter<F>(
    output: &mut ChatStreamOutput,
    on_event: &mut F,
    title_filter: TitleTagFilter,
) where
    F: FnMut(ChatStreamEvent) + Send,
{
    let tail = title_filter.finish();
    if let Some(title) = tail.title {
        let event = ChatStreamEvent::Title(title);
        output.push(&event);
        on_event(event);
    }
    if !tail.visible.is_empty() {
        let event = ChatStreamEvent::Answer(tail.visible);
        output.push(&event);
        on_event(event);
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

#[derive(Debug, Default)]
pub(super) struct TitleTagFilter {
    buffer: String,
    title: String,
    capturing: bool,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct TitleFilteredChunk {
    visible: String,
    title: Option<String>,
}

impl TitleTagFilter {
    const START: &str = "<tcui:chat-title>";
    const END: &str = "</tcui:chat-title>";

    pub(super) fn push(&mut self, input: &str) -> TitleFilteredChunk {
        let mut chunk = TitleFilteredChunk::default();

        self.buffer.push_str(input);
        loop {
            if self.capturing {
                if let Some(end) = self.buffer.find(Self::END) {
                    self.title.push_str(&self.buffer[..end]);
                    self.buffer.drain(..end + Self::END.len());
                    self.capturing = false;
                    let title = self.title.trim();
                    if !title.is_empty() {
                        chunk.title = Some(title.to_string());
                    }
                    self.title.clear();
                    continue;
                }
                let keep = matching_suffix_prefix_len(&self.buffer, Self::END);
                let capture_len = self.buffer.len().saturating_sub(keep);
                self.title.push_str(&self.buffer[..capture_len]);
                self.buffer.drain(..capture_len);
                break;
            }

            if let Some(start) = self.buffer.find(Self::START) {
                chunk.visible.push_str(&self.buffer[..start]);
                self.buffer.drain(..start + Self::START.len());
                self.capturing = true;
                continue;
            }

            let keep = matching_suffix_prefix_len(&self.buffer, Self::START);
            if self.buffer.len() > keep {
                let emit_len = self.buffer.len() - keep;
                chunk.visible.push_str(&self.buffer[..emit_len]);
                self.buffer.drain(..emit_len);
            }
            break;
        }
        chunk
    }

    fn finish(mut self) -> TitleFilteredChunk {
        if self.capturing {
            TitleFilteredChunk::default()
        } else {
            TitleFilteredChunk {
                visible: std::mem::take(&mut self.buffer),
                title: None,
            }
        }
    }
}

fn matching_suffix_prefix_len(buffer: &str, pattern: &str) -> usize {
    let max = pattern.len().saturating_sub(1);
    for (idx, _) in buffer.char_indices().rev() {
        let suffix = &buffer[idx..];
        if suffix.len() <= max && pattern.starts_with(suffix) {
            return suffix.len();
        }
    }
    0
}

#[cfg(test)]
#[path = "chat_tests.rs"]
mod chat_tests;
