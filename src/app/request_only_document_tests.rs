use std::sync::Arc;

use super::message::Message;
use super::submission::{
    ContextReference, InputSubmission, PendingDocument, PendingDocumentError, PendingDocuments,
    PendingDocumentsError, PreparedUserTurn, SubmissionState, MAX_DOCUMENTS_BYTES,
    MAX_DOCUMENT_BYTES,
};

#[test]
fn inserted_document_expansion_snapshots_utf8_metadata_immutably() {
    // Given
    let mut source = b"first\nsecond\n".to_vec();

    // When
    let document = PendingDocument::snapshot("/private/absolute/source.md", "source.md", &source)
        .expect("snapshot valid UTF-8");
    source.fill(b'x');

    // Then
    assert_eq!(document.display_name(), "source.md");
    assert_eq!(document.line_count(), 2);
    assert_eq!(document.byte_count(), 13);
    assert_eq!(document.content(), "first\nsecond\n");
    assert_eq!(
        document.digest().as_bytes(),
        blake3::hash(b"first\nsecond\n").as_bytes()
    );
}

#[test]
fn inserted_document_expansion_rejects_binary_and_per_item_oversize() {
    // Given
    let binary = [0xff, 0xfe];
    let oversized = vec![b'x'; MAX_DOCUMENT_BYTES + 1];

    // When
    let binary_error = PendingDocument::snapshot("binary", "binary.dat", &binary)
        .expect_err("binary input must be rejected");
    let size_error = PendingDocument::snapshot("large", "large.md", &oversized)
        .expect_err("oversized input must be rejected");

    // Then
    assert_eq!(binary_error, PendingDocumentError::NonUtf8);
    assert_eq!(size_error, PendingDocumentError::ItemTooLarge);
}

#[test]
fn inserted_document_expansion_collection_deduplicates_orders_limits_and_removes() {
    // Given
    let first = PendingDocument::snapshot("source-a", "a.md", b"alpha").expect("snapshot first");
    let same_source =
        PendingDocument::snapshot("source-a", "changed.md", b"beta").expect("snapshot same source");
    let same_digest =
        PendingDocument::snapshot("source-b", "copy.md", b"alpha").expect("snapshot same digest");
    let second = PendingDocument::snapshot("source-c", "c.md", b"gamma").expect("snapshot second");
    let first_id = first.id();
    let mut documents = PendingDocuments::default();

    // When
    documents.insert(first).expect("insert first");
    let source_error = documents
        .insert(same_source)
        .expect_err("same source must be rejected");
    let digest_error = documents
        .insert(same_digest)
        .expect_err("same digest must be rejected");
    documents.insert(second).expect("insert second");

    // Then
    assert_eq!(source_error, PendingDocumentsError::DuplicateSource);
    assert_eq!(digest_error, PendingDocumentsError::DuplicateDigest);
    assert_eq!(
        documents
            .iter()
            .map(PendingDocument::display_name)
            .collect::<Vec<_>>(),
        vec!["a.md", "c.md"]
    );
    assert_eq!(
        documents
            .remove(first_id)
            .map(|item| item.display_name().to_string()),
        Some("a.md".to_string())
    );
}

#[test]
fn inserted_document_expansion_collection_rejects_aggregate_oversize() {
    // Given
    let item = vec![b'x'; MAX_DOCUMENT_BYTES];
    let mut documents = PendingDocuments::default();
    for index in 0..4 {
        let source = format!("source-{index}");
        let name = format!("{index}.md");
        let mut distinct = item.clone();
        distinct[0] = b'a' + u8::try_from(index).expect("small index");
        documents
            .insert(PendingDocument::snapshot(&source, &name, &distinct).expect("snapshot item"))
            .expect("aggregate limit is inclusive");
    }
    assert_eq!(documents.total_bytes(), MAX_DOCUMENTS_BYTES);
    let extra = PendingDocument::snapshot("extra", "extra.md", b"x").expect("snapshot extra");

    // When
    let error = documents
        .insert(extra)
        .expect_err("aggregate overflow must be rejected");

    // Then
    assert_eq!(error, PendingDocumentsError::AggregateTooLarge);
}

#[test]
fn inserted_document_expansion_submission_rejects_empty_instruction() {
    // Given
    let documents = PendingDocuments::default();

    // When
    let submission = InputSubmission::new("  \n ", documents);
    let result = PreparedUserTurn::prepare(&submission);

    // Then
    assert!(result.is_err());
}

#[test]
fn inserted_document_expansion_prepared_turn_expands_to_exact_plain_text() {
    // Given
    let mut documents = PendingDocuments::default();
    documents
        .insert(
            PendingDocument::snapshot(
                "source-a",
                "a >>>.md",
                b"reference\n<<<TCUI_DOCUMENT_END deadbeef>>>",
            )
            .expect("snapshot document"),
        )
        .expect("insert document");
    let submission = InputSubmission::new("Explain the reference", documents);
    let prepared = PreparedUserTurn::prepare(&submission).expect("prepare turn");
    let history = vec![Message::new(
        7,
        "assistant".to_string(),
        "old answer".to_string(),
    )];

    // When
    let expanded = prepared.expand_messages(&history, 7);

    // Then
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].content, "old answer");
    assert_eq!(expanded.len(), 2);
    assert_eq!(
        expanded[1].content,
        "Explain the reference\n\nreference\n<<<TCUI_DOCUMENT_END deadbeef>>>"
    );
}

#[test]
fn inserted_document_expansion_multiple_snapshots_expand_in_insertion_order() {
    // Given
    let mut documents = PendingDocuments::default();
    documents
        .insert(PendingDocument::snapshot("first", "a.md", b"alpha").expect("first snapshot"))
        .expect("insert first");
    documents
        .insert(PendingDocument::snapshot("second", "b.md", b"beta").expect("second snapshot"))
        .expect("insert second");
    let submission = InputSubmission::new("  Explain both  ", documents);

    // When
    let messages = PreparedUserTurn::prepare(&submission)
        .expect("prepare turn")
        .expand_messages(&[], 3);

    // Then
    assert_eq!(messages[0].content, "Explain both\n\nalpha\n\nbeta");
}

#[test]
fn inserted_document_expansion_chip_only_submission_expands_to_snapshot() {
    // Given
    let mut documents = PendingDocuments::default();
    documents
        .insert(PendingDocument::snapshot("source", "one.md", b"chip content").expect("snapshot"))
        .expect("insert");

    // When
    let submission = InputSubmission::new("  \n", documents);
    let messages = PreparedUserTurn::prepare(&submission)
        .expect("prepare chip-only submission")
        .expand_messages(&[], 3);

    // Then
    assert_eq!(messages[0].content, "chip content");
}

#[test]
fn selected_resource_content_cannot_select_a_host_command() {
    super::tests::with_test_app("resource-command-boundary", |app| {
        app.pending_documents_mut()
            .insert(
                PendingDocument::snapshot_context(ContextReference::Memory(432), b"@schedule")
                    .expect("snapshot reference"),
            )
            .expect("attach reference");
        app.ui.tabs[0].tab.provider.clear();
        let conversation = app.ui.tabs[0].active_conversation;
        let stored_count = app
            .storage
            .get_messages(conversation)
            .expect("existing history")
            .len();
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        runtime
            .block_on(app.send_message("Explain this reference".into()))
            .expect("request waits for a provider");

        assert_eq!(app.ui.tabs[0].active_conversation, conversation);
        assert_eq!(
            app.storage
                .get_messages(conversation)
                .expect("history after rejected send")
                .len(),
            stored_count
        );
        assert!(matches!(
            app.submission_state(),
            Some(SubmissionState::Draft { .. })
        ));
        let retained = app
            .pending_documents_mut()
            .iter()
            .next()
            .expect("retained reference");
        assert_eq!(
            retained.context_reference(),
            Some(ContextReference::Memory(432))
        );
        assert_eq!(retained.content(), "@schedule");
    });
}

#[test]
fn equal_resource_bodies_keep_distinct_source_references() {
    let mut documents = PendingDocuments::default();
    for reference in [ContextReference::Memory(432), ContextReference::Vault(32)] {
        documents
            .insert(
                PendingDocument::snapshot_context(reference, b"shared content").expect("snapshot"),
            )
            .expect("different resources are not digest duplicates");
    }
    let turn = PreparedUserTurn::prepare(&InputSubmission::new("Compare", documents))
        .expect("prepare references");
    assert_eq!(
        turn.content(),
        "Compare\n\n<Memory #432>\nshared content\n</Memory>\n\n<Vault item #32>\nshared content\n</Vault item>"
    );
}

#[test]
fn reference_framing_counts_toward_the_prompt_budget() {
    let mut documents = PendingDocuments::default();
    let body = vec![b'x'; MAX_DOCUMENT_BYTES];
    for id in 1..4 {
        documents
            .insert(
                PendingDocument::snapshot_context(ContextReference::Vault(id), &body)
                    .expect("snapshot"),
            )
            .expect("three resources fit");
    }
    let result = documents.insert(
        PendingDocument::snapshot_context(ContextReference::Vault(4), &body).expect("snapshot"),
    );
    assert_eq!(result, Err(PendingDocumentsError::AggregateTooLarge));
}

#[test]
fn binary_resource_context_is_lossless_and_bounded_after_encoding() {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let bytes = [0xff, 0x00, 0x1b, 0x80, b'\n'];
    let document = PendingDocument::snapshot_context(ContextReference::Vault(32), &bytes)
        .expect("binary reference");
    let encoded = document
        .content()
        .split_once('\n')
        .expect("encoding label")
        .1;
    assert_eq!(STANDARD.decode(encoded).expect("decode reference"), bytes);
    let oversized_after_encoding = vec![0xff; MAX_DOCUMENT_BYTES];
    assert_eq!(
        PendingDocument::snapshot_context(ContextReference::Vault(32), &oversized_after_encoding)
            .expect_err("encoded content exceeds budget"),
        PendingDocumentError::ItemTooLarge,
    );
}

#[test]
fn inserted_document_expansion_next_turn_reuses_expanded_history_once() {
    // Given
    let mut documents = PendingDocuments::default();
    documents
        .insert(PendingDocument::snapshot("source", "one.md", b"one-shot").expect("snapshot"))
        .expect("insert");
    let first = InputSubmission::new("first", documents);
    let first_messages = PreparedUserTurn::prepare(&first)
        .expect("prepare first")
        .expand_messages(&[], 3);
    let persisted = first_messages.clone();
    let next = InputSubmission::new("next", PendingDocuments::default());

    // When
    let next_messages = PreparedUserTurn::prepare(&next)
        .expect("prepare next")
        .expand_messages(&persisted, 3);

    // Then
    assert_eq!(first_messages[0].content, "first\n\none-shot");
    assert_eq!(next_messages.len(), 2);
    assert_eq!(next_messages[0].content, "first\n\none-shot");
    assert_eq!(next_messages[1].content, "next");
    assert_eq!(
        next_messages
            .iter()
            .filter(|message| message.content.contains("one-shot"))
            .count(),
        1
    );
}

#[test]
fn inserted_document_expansion_state_transitions_consume_snapshots_only_at_launch() {
    // Given
    let mut documents = PendingDocuments::default();
    documents
        .insert(PendingDocument::snapshot("source", "one.md", b"one-shot").expect("snapshot"))
        .expect("insert");
    let submission = InputSubmission::new("send", documents);
    let state = SubmissionState::draft(submission);

    // When
    let state = state.begin_preparing().expect("Draft to Preparing");
    assert!(matches!(state, SubmissionState::Preparing(_)));
    let state = state.finish_preparing().expect("Preparing to Prepared");
    assert!(matches!(state, SubmissionState::Prepared(_)));
    let state = state.commit().expect("Prepared to Committed");
    assert!(matches!(state, SubmissionState::Committed(_)));
    let state = state.launch().expect("Committed to Launched");

    // Then
    assert!(matches!(state, SubmissionState::Launched { .. }));
    assert!(state.documents().is_none());
}

#[test]
fn inserted_document_expansion_postlaunch_failure_never_restores_snapshots() {
    // Given
    let mut documents = PendingDocuments::default();
    documents
        .insert(PendingDocument::snapshot("source", "one.md", b"one-shot").expect("snapshot"))
        .expect("insert");
    let launched = SubmissionState::draft(InputSubmission::new("send", documents))
        .begin_preparing()
        .expect("prepare")
        .finish_preparing()
        .expect("prepared")
        .commit()
        .expect("committed")
        .launch()
        .expect("launched");

    // When
    let failed = launched.fail_after_launch().expect("record launch failure");

    // Then
    assert!(matches!(failed, SubmissionState::FailedAfterLaunch { .. }));
    assert!(failed.documents().is_none());
    assert!(failed.return_to_draft().is_err());
}

#[test]
fn inserted_document_expansion_prelaunch_failure_restores_draft_with_documents() {
    // Given
    let mut documents = PendingDocuments::default();
    documents
        .insert(PendingDocument::snapshot("source", "one.md", b"one-shot").expect("snapshot"))
        .expect("insert");
    let submission = InputSubmission::new("send", documents);
    let state = SubmissionState::draft(submission)
        .begin_preparing()
        .expect("start preparing");

    // When
    let state = state.return_to_draft().expect("restore draft");

    // Then
    assert!(matches!(state, SubmissionState::Draft(_)));
    assert_eq!(state.documents().map(PendingDocuments::len), Some(1));
}

#[test]
fn inserted_document_expansion_debug_and_errors_redact_content_and_source_paths() {
    // Given
    let secret = "DOCUMENT-CONTENT-SECRET";
    let path = "/home/private/absolute/secret.md";
    let document =
        PendingDocument::snapshot(path, "secret.md", secret.as_bytes()).expect("snapshot document");
    let debug = format!("{document:?}");
    let error = PendingDocument::snapshot(path, "secret.md", &[0xff])
        .expect_err("binary must fail")
        .to_string();

    // Then
    assert!(!debug.contains(secret));
    assert!(!debug.contains(path));
    assert!(!debug.contains("TCUI_DOCUMENT"));
    assert!(!error.contains(path));
    assert!(!error.contains(secret));
}

#[test]
fn inserted_document_expansion_arc_snapshot_is_shared_without_mutation() {
    // Given
    let document =
        PendingDocument::snapshot("source", "one.md", b"immutable").expect("snapshot document");

    // When
    let first: Arc<str> = document.content_arc();
    let second: Arc<str> = document.content_arc();

    // Then
    assert!(Arc::ptr_eq(&first, &second));
}

#[test]
fn inserted_document_expansion_prelaunch_validation_retains_snapshots_and_persists_nothing() {
    super::tests::with_test_app("request-doc-prelaunch", |app| {
        // Given
        app.pending_documents_mut()
            .insert(
                PendingDocument::snapshot("source", "one.md", b"RETAIN-ON-FAILURE")
                    .expect("snapshot"),
            )
            .expect("insert");
        app.ui.tabs[0].tab.provider = "Ollama".to_string();
        app.ui.tabs[0].tab.endpoint = Some("http://127.0.0.1:9".to_string());
        app.ui.tabs[0].tab.model.clear();
        let conversation_id = app.ui.tabs[0].active_conversation;
        let before = app
            .storage
            .get_messages(conversation_id)
            .expect("load initial history");
        let runtime = tokio::runtime::Runtime::new().expect("create runtime");

        // When
        let result = runtime.block_on(app.send_message("instruction".to_string()));

        // Then
        assert!(result.is_err());
        assert!(matches!(
            app.submission_state(),
            Some(SubmissionState::Draft(_))
        ));
        assert_eq!(
            app.submission_state()
                .and_then(SubmissionState::documents)
                .map(PendingDocuments::len),
            Some(1)
        );
        assert_eq!(
            app.storage
                .get_messages(conversation_id)
                .expect("reload history")
                .len(),
            before.len()
        );
    });
}

#[test]
fn inserted_document_expansion_launch_consumes_snapshots_and_reloads_expanded_text() {
    super::tests::with_test_app("request-doc-launch", |app| {
        // Given
        #[cfg(feature = "memory")]
        {
            app.config.blocking_write().memory.enabled = false;
        }
        app.pending_documents_mut()
            .insert(
                PendingDocument::snapshot("source", "one.md", b"NEVER-PERSIST-THIS")
                    .expect("snapshot"),
            )
            .expect("insert");
        app.ui.tabs[0].tab.provider = "Ollama".to_string();
        app.ui.tabs[0].tab.endpoint = Some("http://127.0.0.1:9".to_string());
        app.ui.tabs[0].tab.model = "request-only-test".to_string();
        let runtime = tokio::runtime::Runtime::new().expect("create runtime");

        // When
        runtime
            .block_on(app.send_message("persist only me".to_string()))
            .expect("launch prepared request");

        // Then
        assert!(matches!(
            app.submission_state(),
            Some(SubmissionState::Launched { .. })
        ));
        assert!(!app
            .pending_documents
            .contains_key(&app.ui.app_tabs.active_id()));
        let stored = app
            .storage
            .get_messages(app.ui.tabs[0].active_conversation)
            .expect("reload persisted history");
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].content, "persist only me\n\nNEVER-PERSIST-THIS");
        assert_eq!(
            app.ui.tabs[0].messages[0].content,
            "persist only me\n\nNEVER-PERSIST-THIS"
        );
    });
}

#[test]
fn inserted_document_expansion_whitespace_instruction_launches_nonempty_snapshot() {
    super::tests::with_test_app("request-doc-empty", |app| {
        // Given
        app.pending_documents_mut()
            .insert(PendingDocument::snapshot("source", "one.md", b"retain").expect("snapshot"))
            .expect("insert");
        let runtime = tokio::runtime::Runtime::new().expect("create runtime");

        // When
        #[cfg(feature = "memory")]
        {
            app.config.blocking_write().memory.enabled = false;
        }
        app.ui.tabs[0].tab.provider = "Ollama".to_string();
        app.ui.tabs[0].tab.endpoint = Some("http://127.0.0.1:9".to_string());
        app.ui.tabs[0].tab.model = "chip-only-test".to_string();

        let result = runtime.block_on(app.send_message("  \n".to_string()));

        // Then
        result.expect("launch chip-only request");
        assert!(matches!(
            app.submission_state(),
            Some(SubmissionState::Launched { .. })
        ));
        assert!(app.pending_documents_mut().is_empty());
        let stored = app
            .storage
            .get_messages(app.ui.tabs[0].active_conversation)
            .expect("stored history");
        assert_eq!(stored[0].content, "retain");
    });
}

#[test]
fn native_editor_preserves_unicode_across_multiline_edits_and_cancelled_close() {
    use crate::ui::modals::resource_editor::{EditorAction, ResourceEditor};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    super::tests::with_test_app("native-editor-unicode", |_| {
        let mut editor = ResourceEditor::new(
            crate::ui::artifact_sidebar::ArtifactHandle::Vault("32".parse().unwrap()),
            "unicode.txt".into(),
            "α\nβ\nγ".as_bytes().to_vec(),
        )
        .unwrap();
        for code in [
            KeyCode::Up,
            KeyCode::Home,
            KeyCode::Backspace,
            KeyCode::End,
            KeyCode::Char('!'),
            KeyCode::Down,
            KeyCode::Left,
            KeyCode::Delete,
        ] {
            editor.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
        }
        editor.insert("δ\nend");
        assert_eq!(editor.bytes().unwrap().as_ref(), "αβ!\nδ\nend".as_bytes());
        assert!(editor.is_dirty());
        assert!(matches!(
            editor.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            EditorAction::None
        ));
        editor.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        assert_eq!(editor.bytes().unwrap().as_ref(), "αβ!\nδ\nend".as_bytes());
        assert!(editor.is_dirty());
    });
}

#[test]
fn native_binary_editor_rejects_invalid_base64_without_changing_source_digest() {
    use crate::ui::modals::resource_editor::ResourceEditor;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    super::tests::with_test_app("native-editor-binary", |_| {
        let original = [0xff, 0, 0x80, 7];
        let mut editor = ResourceEditor::new(
            crate::ui::artifact_sidebar::ArtifactHandle::Vault("32".parse().unwrap()),
            "binary.bin".into(),
            original.to_vec(),
        )
        .unwrap();
        assert_eq!(editor.bytes().unwrap().as_ref(), original);
        editor.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
        editor.insert("not valid base64!");
        assert!(editor.bytes().is_err());
        assert_eq!(editor.source_digest, blake3::hash(&original));
        assert!(editor.is_dirty());
    });
}

#[cfg(feature = "memory")]
#[test]
fn memory_view_enter_does_not_execute_a_hidden_chat_command() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    super::tests::with_test_app("memory-hidden-chat-command", |app| {
        let store = crate::memory::MemoryStore::open_default().unwrap();
        store
            .write(
                std::path::Path::new("entry.md"),
                "# Entry\n\nVisible memory.",
                false,
            )
            .unwrap();
        app.refresh_memory_artifacts().unwrap();
        app.ui.tabs[0].input_content = "/cancel".into();
        app.ui
            .app_tabs
            .open_view(crate::ui::app_tabs::AppView::Memories);
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.ui.tabs[0].input_content, "/cancel");
        let viewer = app
            .ui
            .artifact_viewer
            .as_ref()
            .expect("native entry opened");
        assert!(matches!(
            viewer.handle(),
            crate::ui::artifact_sidebar::ArtifactHandle::Memory(_)
        ));
        app.ui.artifact_viewer = None;
        app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL));
        assert!(
            app.ui.editor_popup.is_none(),
            "a native view cannot open the hidden chat draft"
        );
        assert_eq!(app.ui.tabs[0].input_content, "/cancel");
    });
}
