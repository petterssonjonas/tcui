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
    let submission = InputSubmission::new("wire instruction", documents).expect("submission");
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
        messages: PreparedUserTurn::prepare(&submission).expand_messages(&history, 9),
    }
}

fn serialized_text(backend_type: &str) -> String {
    let bytes = serialize_request_body(&request_with_documents(backend_type))
        .expect("serialize provider request");
    String::from_utf8(bytes).expect("JSON is UTF-8")
}

#[test]
fn request_only_document_openai_compatible_wire_body_includes_snapshot() {
    assert!(serialized_text("openai").contains("WIRE-DOCUMENT-CONTENT"));
}

#[test]
fn request_only_document_anthropic_wire_body_includes_snapshot() {
    assert!(serialized_text("anthropic").contains("WIRE-DOCUMENT-CONTENT"));
}

#[test]
fn request_only_document_codex_wire_body_includes_snapshot() {
    assert!(serialized_text("codex").contains("WIRE-DOCUMENT-CONTENT"));
}

#[test]
fn request_only_document_request_debug_redacts_snapshot_and_delimiters() {
    let debug = format!("{:?}", request_with_documents("openai"));

    assert!(!debug.contains("WIRE-DOCUMENT-CONTENT"));
    assert!(!debug.contains("TCUI_DOCUMENT"));
}

fn assert_insert_to_chat_wire_filter(backend_type: &str) {
    let body = serialized_text(backend_type);

    assert_eq!(body.matches("wire instruction").count(), 1);
    assert_eq!(body.matches("WIRE-DOCUMENT-CONTENT").count(), 1);
    assert!(!body.contains("[wire.md - 1 lines]"));
    assert!(!body.contains("pending_document_chips"));
    println!("INSERT_TO_CHAT_WIRE_{backend_type}={body}");
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
