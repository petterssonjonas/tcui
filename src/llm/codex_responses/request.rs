use color_eyre::eyre::{Result, eyre};
use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderValue, USER_AGENT};
use serde_json::{Map, Value, json};

use crate::llm::chat::ChatRequest;

const CODEX_ORIGINATOR: &str = "codex_cli_rs";

pub(super) fn codex_headers(
    access_token: &str,
    account_id: &str,
    request: &ChatRequest,
) -> Result<HeaderMap> {
    let authorization = HeaderValue::from_str(&format!("Bearer {access_token}"))?;
    let account_id = HeaderValue::from_str(account_id)?;
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, authorization);
    headers.insert("ChatGPT-Account-ID", account_id);
    headers.insert("originator", HeaderValue::from_static(CODEX_ORIGINATOR));
    headers.insert(USER_AGENT, codex_user_agent()?);
    headers.insert(ACCEPT, HeaderValue::from_static("text/event-stream"));
    if let Some(context) = conversation_context(request) {
        headers.insert("session-id", HeaderValue::from_str(&context.session_id)?);
        headers.insert("thread-id", HeaderValue::from_str(&context.thread_id)?);
        headers.insert(
            "x-client-request-id",
            HeaderValue::from_str(&context.thread_id)?,
        );
    }
    Ok(headers)
}

pub(super) fn build_request_body(request: &ChatRequest) -> Value {
    let mut input = Vec::new();
    let mut instructions = request.system_prompt.trim().to_string();
    for message in &request.messages {
        if message.content.trim().is_empty() {
            continue;
        }
        if message.role == "system" {
            append_instruction(&mut instructions, &message.content);
            continue;
        }
        let content_type = if message.role == "assistant" {
            "output_text"
        } else {
            "input_text"
        };
        let role = if message.role == "assistant" {
            "assistant"
        } else {
            "user"
        };
        input.push(json!({
            "type": "message",
            "role": role,
            "content": [{"type": content_type, "text": message.content}],
        }));
        if let Some(reasoning) = message
            .thinking_content
            .as_deref()
            .filter(|reasoning| !reasoning.trim().is_empty())
        {
            input.push(json!({
                "type": "reasoning",
                "summary": [{"type": "summary_text", "text": reasoning}],
            }));
        }
    }

    let mut body = Map::new();
    body.insert("model".to_string(), Value::String(request.model.clone()));
    if !instructions.is_empty() {
        body.insert("instructions".to_string(), Value::String(instructions));
    }
    body.insert("input".to_string(), Value::Array(input));
    if let Some(effort) = request
        .reasoning_effort
        .as_deref()
        .map(str::trim)
        .filter(|effort| !effort.is_empty())
    {
        body.insert("reasoning".to_string(), json!({"effort": effort}));
    }
    body.insert("stream".to_string(), Value::Bool(true));
    body.insert("store".to_string(), Value::Bool(false));
    if let Some(context) = conversation_context(request) {
        body.insert(
            "client_metadata".to_string(),
            json!({
                "session_id": context.session_id,
                "thread_id": context.thread_id,
            }),
        );
    }
    Value::Object(body)
}

pub(super) fn validate_reasoning_effort(request: &ChatRequest) -> Result<()> {
    let Some(configured_effort) = request.reasoning_effort.as_deref() else {
        return Ok(());
    };
    let effort = configured_effort.trim();
    if !effort.is_empty()
        && request
            .supported_reasoning_efforts
            .iter()
            .any(|supported| supported == effort)
    {
        return Ok(());
    }

    let supported = if request.supported_reasoning_efforts.is_empty() {
        "none advertised".to_string()
    } else {
        request.supported_reasoning_efforts.join(", ")
    };
    Err(eyre!(
        "Reasoning effort '{effort}' is not supported by Codex model '{}'. Supported efforts: {supported}.",
        request.model
    ))
}

pub(super) fn codex_user_agent() -> Result<HeaderValue> {
    HeaderValue::from_str(&format!(
        "{CODEX_ORIGINATOR}/{} ({}; {}) tcui",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    ))
    .map_err(Into::into)
}

struct CodexConversationContext {
    session_id: String,
    thread_id: String,
}

fn conversation_context(request: &ChatRequest) -> Option<CodexConversationContext> {
    let conversation_id = request.messages.first()?.conversation_id;
    if conversation_id <= 0
        || request
            .messages
            .iter()
            .any(|message| message.conversation_id != conversation_id)
    {
        return None;
    }
    let identity = format!("tcui-conversation-{conversation_id:016x}");
    Some(CodexConversationContext {
        session_id: identity.clone(),
        thread_id: identity,
    })
}

fn append_instruction(instructions: &mut String, addition: &str) {
    if instructions.is_empty() {
        instructions.push_str(addition);
    } else {
        instructions.push_str("\n\n");
        instructions.push_str(addition);
    }
}
