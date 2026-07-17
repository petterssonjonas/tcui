use crate::app::message::Message;
use crate::app::submission::{
    InputSubmission, PendingDocument, PendingDocuments, PreparedUserTurn,
};
use crate::llm::chat::{serialize_request_body, ChatRequest};

fn request_with_documents(backend_type: &str) -> ChatRequest {
    let mut documents = PendingDocuments::default();
    documents
        .insert(
            PendingDocument::snapshot("source", "wire.md", b"WIRE-DOCUMENT-CONTENT")
                .expect("snapshot"),
        )
        .expect("insert");
    let submission = InputSubmission::new("wire instruction", documents);
    let history = vec![Message::new(
        9,
        "assistant".to_string(),
        "previous".to_string(),
    )];
    ChatRequest {
        provider: if backend_type == "anthropic" {
            "Anthropic".to_string()
        } else if backend_type == "codex" {
            "Codex".to_string()
        } else {
            "OpenAI".to_string()
        },
        endpoint: "https://example.invalid".to_string(),
        model: "model".to_string(),
        reasoning_effort: None,
        supported_reasoning_efforts: Vec::new(),
        backend_type: backend_type.to_string(),
        api_key: None,
        system_prompt: "system".to_string(),
        messages: PreparedUserTurn::prepare(&submission)
            .expect("prepare turn")
            .expand_messages(&history, 9),
    }
}

fn serialized_body(backend_type: &str) -> serde_json::Value {
    let bytes = serialize_request_body(&request_with_documents(backend_type))
        .expect("serialize provider request");
    serde_json::from_slice(&bytes).expect("provider request is JSON")
}

fn count_exact_string(value: &serde_json::Value, expected: &str) -> usize {
    match value {
        serde_json::Value::String(text) => usize::from(text == expected),
        serde_json::Value::Array(items) => items
            .iter()
            .map(|item| count_exact_string(item, expected))
            .sum(),
        serde_json::Value::Object(fields) => fields
            .values()
            .map(|item| count_exact_string(item, expected))
            .sum(),
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => 0,
    }
}

fn serialized_text(backend_type: &str) -> String {
    serialized_body(backend_type).to_string()
}

#[test]
fn inserted_document_expansion_openai_compatible_wire_body_includes_snapshot() {
    assert!(serialized_text("openai").contains("WIRE-DOCUMENT-CONTENT"));
}

#[test]
fn inserted_document_expansion_anthropic_wire_body_includes_snapshot() {
    assert!(serialized_text("anthropic").contains("WIRE-DOCUMENT-CONTENT"));
}

#[test]
fn inserted_document_expansion_codex_wire_body_includes_snapshot() {
    assert!(serialized_text("codex").contains("WIRE-DOCUMENT-CONTENT"));
}

#[test]
fn inserted_document_expansion_request_debug_redacts_snapshot_and_delimiters() {
    let debug = format!("{:?}", request_with_documents("openai"));

    assert!(!debug.contains("WIRE-DOCUMENT-CONTENT"));
    assert!(!debug.contains("TCUI_DOCUMENT"));
}

fn assert_insert_to_chat_wire_filter(backend_type: &str) {
    let body = serialized_body(backend_type);
    let text = body.to_string();

    assert_eq!(
        count_exact_string(&body, "wire instruction\n\nWIRE-DOCUMENT-CONTENT"),
        1
    );
    assert!(!text.contains("wire.md"));
    assert!(!text.contains("TCUI_DOCUMENT"));
    assert!(!text.contains("request-documents"));
    assert!(!text.contains("pending_document_chips"));
    println!("INSERT_TO_CHAT_WIRE_{backend_type}={text}");
}

#[test]
fn insert_to_chat_generic_wire_contains_instruction_and_snapshot_without_chip_metadata() {
    assert_insert_to_chat_wire_filter("openai");
}

#[test]
fn insert_to_chat_anthropic_wire_contains_instruction_and_snapshot_without_chip_metadata() {
    assert_insert_to_chat_wire_filter("anthropic");
}

#[test]
fn insert_to_chat_codex_wire_contains_instruction_and_snapshot_without_chip_metadata() {
    assert_insert_to_chat_wire_filter("codex");
}

#[test]
fn insert_to_chat_debug_output_redacts_snapshot_content_and_chip_metadata() {
    let debug = format!("{:?}", request_with_documents("openai"));

    assert!(!debug.contains("WIRE-DOCUMENT-CONTENT"));
    assert!(!debug.contains("[wire.md - 1 lines]"));
    assert!(!debug.contains("TCUI_DOCUMENT"));
    println!("INSERT_TO_CHAT_REDACTED_DEBUG={debug}");
}
