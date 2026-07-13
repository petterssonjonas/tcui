use serde_json::json;

use super::*;
use crate::app::message::Message;
use crate::llm::chat::{ChatRequest, stream_chat};

#[test]
fn request_body_maps_system_messages_and_reasoning_to_responses_input() {
    let mut assistant = Message::new(1, "assistant".to_string(), "previous answer".to_string());
    assistant.thinking_content = Some("previous reasoning".to_string());
    let request = ChatRequest {
        provider: "Codex".to_string(),
        endpoint: "https://chatgpt.com/backend-api/codex".to_string(),
        model: "gpt-5.6".to_string(),
        reasoning_effort: Some("high".to_string()),
        supported_reasoning_efforts: vec!["high".to_string()],
        backend_type: "codex".to_string(),
        api_key: None,
        system_prompt: "system policy".to_string(),
        messages: vec![
            Message::new(1, "system".to_string(), "history policy".to_string()),
            Message::new(1, "user".to_string(), "hello".to_string()),
            assistant,
        ],
    };

    assert_eq!(
        build_request_body(&request),
        json!({
            "model": "gpt-5.6",
            "instructions": "system policy\n\nhistory policy",
            "input": [
                {
                    "type": "message",
                    "role": "user",
                    "content": [{"type": "input_text", "text": "hello"}]
                },
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": "previous answer"}]
                },
                {
                    "type": "reasoning",
                    "summary": [{"type": "summary_text", "text": "previous reasoning"}]
                }
            ],
            "reasoning": {"effort": "high"},
            "stream": true,
            "store": false,
            "client_metadata": {
                "session_id": "tcui-conversation-0000000000000001",
                "thread_id": "tcui-conversation-0000000000000001"
            }
        })
    );
}

#[test]
fn headers_match_the_codex_subscription_contract() -> color_eyre::Result<()> {
    let request = ChatRequest {
        provider: "Codex".to_string(),
        endpoint: "https://chatgpt.com/backend-api/codex".to_string(),
        model: "gpt-5.6".to_string(),
        reasoning_effort: None,
        supported_reasoning_efforts: Vec::new(),
        backend_type: "codex".to_string(),
        api_key: None,
        system_prompt: String::new(),
        messages: vec![Message::new(1, "user".to_string(), "hello".to_string())],
    };
    let headers = codex_headers("access-token", "account-123", &request)?;

    assert_eq!(headers["authorization"], "Bearer access-token");
    assert_eq!(headers["chatgpt-account-id"], "account-123");
    assert_eq!(headers["originator"], "codex_cli_rs");
    assert_eq!(headers["user-agent"], request::codex_user_agent()?);
    assert_eq!(headers["accept"], "text/event-stream");
    assert!(!headers.contains_key("version"));
    assert_eq!(headers["session-id"], "tcui-conversation-0000000000000001");
    assert_eq!(headers["thread-id"], "tcui-conversation-0000000000000001");
    assert_eq!(
        headers["x-client-request-id"],
        "tcui-conversation-0000000000000001"
    );
    assert!(!headers.contains_key("openai-beta"));
    Ok(())
}

#[test]
fn request_omits_contextual_metadata_without_a_persisted_conversation() -> color_eyre::Result<()> {
    let request = ChatRequest {
        provider: "Codex".to_string(),
        endpoint: "https://chatgpt.com/backend-api/codex".to_string(),
        model: "gpt-5.6".to_string(),
        reasoning_effort: None,
        supported_reasoning_efforts: Vec::new(),
        backend_type: "codex".to_string(),
        api_key: None,
        system_prompt: String::new(),
        messages: vec![Message::new(0, "user".to_string(), "hello".to_string())],
    };

    let headers = codex_headers("access-token", "account-123", &request)?;
    let body = build_request_body(&request);

    assert!(!headers.contains_key("session-id"));
    assert!(!headers.contains_key("thread-id"));
    assert!(!headers.contains_key("x-client-request-id"));
    assert!(body.get("client_metadata").is_none());
    Ok(())
}

#[test]
fn codex_trust_accepts_only_the_subscription_backend() {
    assert!(crate::llm::auth::trusted_provider_endpoint(
        "Codex",
        "https://chatgpt.com/backend-api/codex"
    ));
    assert!(!crate::llm::auth::trusted_provider_endpoint(
        "Codex",
        "https://api.openai.com/v1"
    ));
}

#[test]
fn request_rejects_reasoning_effort_not_advertised_by_the_selected_model()
-> Result<(), Box<dyn std::error::Error>> {
    let request = ChatRequest {
        provider: "Codex".to_string(),
        endpoint: "https://chatgpt.com/backend-api/codex".to_string(),
        model: "gpt-5.6".to_string(),
        reasoning_effort: Some("none".to_string()),
        supported_reasoning_efforts: vec![
            "low".to_string(),
            "medium".to_string(),
            "high".to_string(),
        ],
        backend_type: "codex".to_string(),
        api_key: None,
        system_prompt: String::new(),
        messages: vec![Message::new(1, "user".to_string(), "hello".to_string())],
    };

    let Err(error) = request::validate_reasoning_effort(&request) else {
        return Err("unsupported effort was accepted".into());
    };

    assert_eq!(
        error.to_string(),
        "Reasoning effort 'none' is not supported by Codex model 'gpt-5.6'. Supported efforts: low, medium, high."
    );
    Ok(())
}

#[test]
fn request_serializes_the_same_trimmed_effort_that_validation_accepts()
-> Result<(), Box<dyn std::error::Error>> {
    let request = ChatRequest {
        provider: "Codex".to_string(),
        endpoint: "https://chatgpt.com/backend-api/codex".to_string(),
        model: "gpt-5.6".to_string(),
        reasoning_effort: Some(" high ".to_string()),
        supported_reasoning_efforts: vec!["high".to_string()],
        backend_type: "codex".to_string(),
        api_key: None,
        system_prompt: String::new(),
        messages: vec![Message::new(1, "user".to_string(), "hello".to_string())],
    };

    request::validate_reasoning_effort(&request)?;
    let body = build_request_body(&request);

    assert_eq!(body["reasoning"], json!({"effort": "high"}));
    Ok(())
}

#[tokio::test]
async fn codex_oauth_token_is_rejected_before_the_public_responses_endpoint() {
    let request = ChatRequest {
        provider: "Codex".to_string(),
        endpoint: "https://api.openai.com/v1".to_string(),
        model: "gpt-5.6".to_string(),
        reasoning_effort: None,
        supported_reasoning_efforts: Vec::new(),
        backend_type: "codex".to_string(),
        api_key: Some("oauth-access-token".to_string()),
        system_prompt: String::new(),
        messages: Vec::new(),
    };

    let error = stream_chat(request, |_| {})
        .await
        .expect_err("the public endpoint must be rejected before OAuth use");

    assert!(
        error.to_string().contains(
            "Codex OAuth credentials can only be sent to the ChatGPT subscription endpoint"
        )
    );
}
