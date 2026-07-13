use color_eyre::eyre::eyre;
use serde::Deserialize;

use crate::llm::chat::{ChatStreamEvent, TitleTagFilter, push_title_filtered_content};

#[derive(Deserialize)]
struct CodexStreamEvent {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    delta: Option<String>,
    #[serde(default)]
    error: Option<CodexError>,
    #[serde(default)]
    response: Option<CodexResponse>,
}

#[derive(Deserialize)]
struct CodexResponse {
    #[serde(default)]
    error: Option<CodexError>,
    #[serde(default)]
    usage: Option<CodexUsage>,
}

#[derive(Deserialize)]
struct CodexError {
    message: Option<String>,
}

#[derive(Deserialize)]
struct CodexUsage {
    total_tokens: Option<i64>,
}

#[derive(Debug)]
pub(super) struct CodexChunk {
    pub(super) events: Vec<ChatStreamEvent>,
    pub(super) total_tokens: Option<i64>,
    pub(super) completed: bool,
}

pub(super) fn codex_stream_event(
    data: &str,
    title_filter: &mut TitleTagFilter,
) -> color_eyre::Result<CodexChunk> {
    let event: CodexStreamEvent = serde_json::from_str(data)?;
    if let Some(error) = event_error(&event) {
        return Err(eyre!("{}", crate::llm::auth::redact_secrets(&error)));
    }

    let total_tokens = event
        .response
        .as_ref()
        .and_then(|response| response.usage.as_ref())
        .and_then(|usage| usage.total_tokens);
    let mut events = Vec::new();
    match event.kind.as_str() {
        "response.output_text.delta" => {
            if let Some(delta) = event.delta.filter(|delta| !delta.is_empty()) {
                push_title_filtered_content(&mut events, title_filter.push(&delta));
            }
        }
        "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
            if let Some(delta) = event.delta.filter(|delta| !delta.is_empty()) {
                events.push(ChatStreamEvent::Thinking(delta));
            }
        }
        "response.failed" | "error" => return Err(eyre!("Codex response failed.")),
        _ => {}
    }
    Ok(CodexChunk {
        events,
        total_tokens,
        completed: event.kind == "response.completed",
    })
}

pub(super) fn response_error_message(body: &str) -> Option<String> {
    serde_json::from_str::<CodexResponse>(body)
        .ok()
        .and_then(|response| response.error)
        .and_then(|error| error.message)
}

fn event_error(event: &CodexStreamEvent) -> Option<String> {
    event
        .error
        .as_ref()
        .and_then(|error| error.message.clone())
        .or_else(|| {
            event
                .response
                .as_ref()
                .and_then(|response| response.error.as_ref())
                .and_then(|error| error.message.clone())
        })
}
