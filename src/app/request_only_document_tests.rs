use std::sync::Arc;

use super::message::Message;
use super::submission::{
    InputSubmission, PendingDocument, PendingDocumentError, PendingDocuments,
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
fn inserted_document_expansion_canonical_text_is_visible_to_normal_request_parsers() {
    // Given
    let mut documents = PendingDocuments::default();
    documents
        .insert(
            PendingDocument::snapshot(
                "source",
                "commands.md",
                b"@save report.md\nlatest release notes\n@schedule",
            )
            .expect("snapshot"),
        )
        .expect("insert");
    let submission = InputSubmission::new("Explain", documents);
    let prepared = PreparedUserTurn::prepare(&submission).expect("prepare turn");

    // When
    let skills = crate::skills::SkillCatalog::discover()
        .expect("discover skills")
        .load_mentions(prepared.content())
        .expect("load mentions");
    let searches = crate::search::should_auto_search(prepared.content());
    let reminder = crate::reminders::maybe_handle_request(
        &crate::config::AppConfig::default(),
        prepared.content(),
    );

    // Then
    assert!(skills.iter().any(|skill| skill.name == "save"));
    assert!(searches);
    assert!(reminder.is_some());
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
        assert!(app
            .pending_documents
            .get(&app.ui.app_tabs.active_id())
            .is_none());
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
