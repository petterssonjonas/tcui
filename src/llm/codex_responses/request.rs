use color_eyre::eyre::{eyre, Result};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, USER_AGENT};
use serde_json::{json, Map, Value};

use crate::llm::chat::ChatRequest;

const CODEX_ORIGINATOR: &str = "codex_cli_rs";

// Pin the backend compatibility version, not TCUI's unrelated release version.
// https://github.com/openai/codex/releases/tag/rust-v0.163.0
pub(crate) const CODEX_CLIENT_VERSION: &str = "0.163.0";
const CODEX_API_BASE: &str = "https://chatgpt.com/backend-api/codex";

pub(crate) fn codex_api_base() -> Result<String> {
    #[cfg(debug_assertions)]
    if let Ok(value) = std::env::var("TCUI_AUTH_TEST_CODEX_API_BASE") {
        let url = reqwest::Url::parse(&value)
            .map_err(|_| eyre!("Invalid local Codex fixture endpoint."))?;
        if url.scheme() != "http"
            || !matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))
            || url.port().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(eyre!("Invalid local Codex fixture endpoint."));
        }
        return Ok(value.trim_end_matches('/').to_string());
    }
    Ok(CODEX_API_BASE.to_string())
}

pub(crate) fn codex_client_headers(access_token: &str, account_id: &str) -> Result<HeaderMap> {
    let mut authorization = HeaderValue::from_str(&format!("Bearer {access_token}"))
        .map_err(|_| eyre!("Invalid Codex authorization metadata."))?;
    authorization.set_sensitive(true);
    let account_id =
        HeaderValue::from_str(account_id).map_err(|_| eyre!("Invalid Codex account metadata."))?;
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, authorization);
    headers.insert("ChatGPT-Account-ID", account_id);
    headers.insert("originator", HeaderValue::from_static(CODEX_ORIGINATOR));
    headers.insert(USER_AGENT, codex_user_agent()?);
    Ok(headers)
}

pub(super) fn codex_headers(
    access_token: &str,
    account_id: &str,
    request: &ChatRequest,
) -> Result<HeaderMap> {
    let mut headers = codex_client_headers(access_token, account_id)?;
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
        if message.role == "system" && !message.content.trim().is_empty() {
            append_instruction(&mut instructions, &message.content);
        }
    }
    if !instructions.is_empty() {
        input.push(json!({
            "type": "message",
            "role": "developer",
            "content": [{"type": "input_text", "text": instructions}],
        }));
    }
    for message in &request.messages {
        if message.content.trim().is_empty() || message.role == "system" {
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
    body.insert("tool_choice".to_string(), Value::String("auto".to_string()));
    body.insert("parallel_tool_calls".to_string(), Value::Bool(false));
    body.insert("include".to_string(), Value::Array(Vec::new()));
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
        CODEX_CLIENT_VERSION,
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
