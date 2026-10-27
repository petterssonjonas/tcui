use super::*;

use crate::ui::artifact_sidebar::{ArtifactEntry, ArtifactHandle, ArtifactKind, ArtifactOrigin};

#[test]
fn insert_to_chat_queue_adds_metadata_chip_without_touching_composer_state() {
    with_test_app("insert-to-chat-chip", |app| {
        // Given
        app.ui.tabs[0].input_content = "Explain this".to_string();
        app.ui.tabs[0].input_cursor = 4;
        app.ui.tabs[0].input_history_index = Some(0);
        app.ui.tabs[0].input_history_draft = Some("draft".to_string());

        // When
        queue_document(app, "temporary:7", "notes.md", b"first\nsecond\n");

        // Then
        let tab = &app.ui.tabs[0];
        assert_eq!(tab.input_content, "Explain this");
        assert_eq!(tab.input_cursor, 4);
        assert_eq!(tab.input_history_index, Some(0));
        assert_eq!(tab.input_history_draft.as_deref(), Some("draft"));
        assert_eq!(tab.pending_document_chips.len(), 1);
        assert_eq!(
            tab.pending_document_chips[0].label(),
            "[notes.md - 2 lines]"
        );
        assert_eq!(app.pending_documents_mut().len(), 1);
    });
}

#[test]
fn insert_to_chat_chips_wrap_above_input_and_publish_close_hit_areas() {
    with_test_app("insert-to-chat-geometry", |app| {
        // Given
        queue_document(app, "one", "first-long-name.md", b"one");
        queue_document(app, "two", "second-long-name.md", b"two\nlines");
        isolate_chat_surface(app);

        // When
        let screenshot = render_text(app, 64, 18);

        // Then
        let tab = &app.ui.tabs[0];
        println!("INSERT_TO_CHAT_CHIP_GEOMETRY\n{screenshot}");
        assert!(screenshot.contains("[first-long-name.md - 1 lines]"));
        assert!(screenshot.contains("[second-long-name.md - 2 lines]"));
        assert_eq!(tab.pending_document_chip_hit_areas.len(), 2);
        let input = tab.input_area.expect("input area");
        assert!(tab
            .pending_document_chip_hit_areas
            .iter()
            .all(|(_, area)| area.y < input.y));
    });
}

#[test]
fn insert_to_chat_empty_input_backspace_removes_last_chip_only() {
    with_test_app("insert-to-chat-backspace", |app| {
        // Given
        queue_document(app, "one", "one.md", b"one");
        queue_document(app, "two", "two.md", b"two");

        // When
        let action = app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Backspace,
            crossterm::event::KeyModifiers::NONE,
        ));
        let runtime = tokio::runtime::Runtime::new().expect("create runtime");
        runtime
            .block_on(app.dispatch(action.expect("remove action")))
            .expect("remove pending document");

        // Then
        assert_eq!(app.ui.tabs[0].pending_document_chips.len(), 1);
        assert_eq!(
            app.ui.tabs[0].pending_document_chips[0].label(),
            "[one.md - 1 lines]"
        );
        assert_eq!(app.pending_documents_mut().len(), 1);
        assert!(app.ui.tabs[0].input_content.is_empty());
    });
}

#[test]
fn insert_to_chat_empty_input_enter_submits_pending_document() {
    with_test_app("insert-to-chat-chip-only-enter", |app| {
        // Given
        queue_document(app, "one", "one.md", b"chip only");

        // When
        let action = app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ));

        // Then
        assert!(matches!(action, Some(Action::SendMessage(content)) if content.is_empty()));
        assert_eq!(app.ui.tabs[0].pending_document_chips.len(), 1);
    });
}

#[test]
fn insert_to_chat_chip_close_click_removes_selected_snapshot() {
    with_test_app("insert-to-chat-chip-click", |app| {
        // Given
        queue_document(app, "one", "one.md", b"one");
        queue_document(app, "two", "two.md", b"two");
        isolate_chat_surface(app);
        let _ = render_text(app, 80, 24);
        let close = app.ui.tabs[0].pending_document_chip_hit_areas[0].1;

        // When
        let action = app
            .handle_mouse_click(close.x, close.y)
            .expect("remove action");
        let runtime = tokio::runtime::Runtime::new().expect("create runtime");
        runtime
            .block_on(app.dispatch(action))
            .expect("remove selected chip");

        // Then
        assert_eq!(app.ui.tabs[0].pending_document_chips.len(), 1);
        assert_eq!(
            app.ui.tabs[0].pending_document_chips[0].label(),
            "[two.md - 1 lines]"
        );
        assert_eq!(app.pending_documents_mut().len(), 1);
    });
}

#[test]
fn insert_to_chat_slash_command_and_history_never_include_chip_text() {
    with_test_app("insert-to-chat-input-isolation", |app| {
        // Given
        queue_document(app, "one", "private.md", b"PRIVATE-SNAPSHOT");
        let conversation_id = app.ui.tabs[0].active_conversation;
        app.ui.tabs[0].messages.push(Message::new(
            conversation_id,
            "user".to_string(),
            "prior instruction".to_string(),
        ));
        app.replace_input_content("/help".to_string());

        // When
        let action = app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ));
        let browsed = app.browse_input_history(false);

        // Then
        assert!(matches!(action, Some(Action::ShowHelp)));
        assert!(browsed);
        assert_eq!(app.ui.tabs[0].input_content, "prior instruction");
        assert!(!app.ui.tabs[0].input_content.contains("private.md"));
        assert!(!app.ui.tabs[0].input_content.contains("PRIVATE-SNAPSHOT"));
        assert_eq!(app.ui.tabs[0].pending_document_chips.len(), 1);
    });
}

#[test]
fn insert_to_chat_duplicate_focuses_composer_and_keeps_one_chip() {
    with_test_app("insert-to-chat-duplicate", |app| {
        // Given
        queue_document(app, "temporary:9", "same.md", b"same");
        let artifact = ArtifactEntry::temp_markdown(9, "same.md".to_string(), "same".to_string());
        app.ui.tabs[0].temporary_artifacts.push(artifact.clone());
        app.ui.artifact_viewer =
            Some(crate::ui::modals::artifact_viewer::ArtifactViewerState::new(artifact));

        // When
        let runtime = tokio::runtime::Runtime::new().expect("create runtime");
        runtime
            .block_on(app.dispatch(Action::InsertArtifactToChat(ArtifactHandle::Temporary(9))))
            .expect("duplicate is handled as a notice");
        let screenshot = render_text(app, 80, 24);

        // Then
        assert_eq!(app.pending_documents_mut().len(), 1);
        assert_eq!(app.ui.tabs[0].pending_document_chips.len(), 1);
        assert!(app.ui.artifact_viewer.is_none());
        assert_eq!(app.ui.focus, crate::tui::focus::Focus::Chat);
        assert!(screenshot.contains("already inserted"));
    });
}

#[test]
fn insert_to_chat_read_failure_leaves_viewer_and_composer_unchanged() {
    with_test_app("insert-to-chat-read-failure", |app| {
        // Given
        let missing = std::env::temp_dir().join("tcui-missing-insert-to-chat.md");
        let handle =
            ArtifactHandle::Vault("18446744073709551615".parse().expect("missing item ID"));
        let artifact = ArtifactEntry {
            handle: handle.clone(),
            name: "missing.md".to_string(),
            kind: ArtifactKind::Markdown,
            origin: ArtifactOrigin::Vault,
            content: Some("preview".to_string()),
            path: Some(missing),
            relative_path: Some(std::path::PathBuf::from("missing.md")),
        };
        app.ui.vault_artifacts.push(artifact.clone());
        app.ui.artifact_viewer =
            Some(crate::ui::modals::artifact_viewer::ArtifactViewerState::new(artifact));
        app.replace_input_content("keep me".to_string());

        // When
        let runtime = tokio::runtime::Runtime::new().expect("create runtime");
        runtime
            .block_on(app.dispatch(Action::InsertArtifactToChat(handle)))
            .expect("read failure becomes a notice");

        // Then
        assert!(app.ui.artifact_viewer.is_some());
        assert_eq!(app.ui.tabs[0].input_content, "keep me");
        assert!(app.ui.tabs[0].pending_document_chips.is_empty());
        assert!(app.pending_documents_mut().is_empty());
    });
}
