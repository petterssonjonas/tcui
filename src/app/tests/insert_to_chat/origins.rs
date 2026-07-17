use super::*;

use crate::ui::artifact_sidebar::{ArtifactEntry, ArtifactHandle};

#[test]
fn insert_to_chat_viewer_button_click_queues_active_artifact() {
    with_test_app("insert-to-chat-viewer-click", |app| {
        // Given
        let artifact = ArtifactEntry::temp_markdown(
            42,
            "clicked.md".to_string(),
            "clicked content".to_string(),
        );
        let handle = artifact.handle.clone();
        app.ui.tabs[0].temporary_artifacts.push(artifact);
        app.open_artifact(handle.clone());
        isolate_chat_surface(app);
        let _ = render_text(app, 80, 24);
        let insert = app
            .ui
            .artifact_viewer
            .as_ref()
            .and_then(|viewer| viewer.hit_areas.insert_to_chat)
            .expect("insert hit area");

        // When
        let action = app
            .handle_mouse_click(insert.x, insert.y)
            .expect("insert action");
        let runtime = tokio::runtime::Runtime::new().expect("create runtime");
        runtime
            .block_on(app.dispatch(action))
            .expect("insert artifact");

        // Then
        assert!(app.ui.artifact_viewer.is_none());
        assert_eq!(app.pending_documents_mut().len(), 1);
        assert_eq!(
            app.ui.tabs[0].pending_document_chips[0].label(),
            "[clicked.md - 1 lines]"
        );
    });
}

#[test]
fn insert_to_chat_chat_file_uses_fresh_safe_store_snapshot() {
    with_test_app("insert-to-chat-chat-file-store", |app| {
        // Given
        let conversation_id = app.ui.tabs[0].active_conversation;
        let record = app
            .chat_files
            .create(conversation_id, "fresh.md", b"FRESH-FROM-SAFE-STORE")
            .expect("create chat file");
        let artifact = ArtifactEntry::chat_file(
            conversation_id,
            record.id,
            record.name,
            "STALE-PREVIEW".to_string(),
        );
        let handle = artifact.handle.clone();
        app.ui.tabs[0].temporary_artifacts.push(artifact.clone());
        app.ui.artifact_viewer =
            Some(crate::ui::modals::artifact_viewer::ArtifactViewerState::new(artifact));

        // When
        let runtime = tokio::runtime::Runtime::new().expect("create runtime");
        runtime
            .block_on(app.dispatch(Action::InsertArtifactToChat(handle)))
            .expect("insert chat file");

        // Then
        let document = app
            .pending_documents_mut()
            .iter()
            .next()
            .expect("pending document");
        assert_eq!(document.content(), "FRESH-FROM-SAFE-STORE");
        assert_ne!(document.content(), "STALE-PREVIEW");
        assert!(matches!(
            app.ui.tabs[0].temporary_artifacts[0].handle,
            ArtifactHandle::ChatFile { .. }
        ));
    });
}
