use super::*;

#[test]
fn chat_file_sidebar_view_reads_current_physical_file() {
    with_sidebar_app("fresh-view", |app, _| {
        // Given
        let (handle, path) = create_link(app, "fresh.md", b"STALE-PREVIEW");
        std::fs::write(path, "CURRENT-PHYSICAL-CONTENT").expect("update canonical file");

        // When
        app.open_artifact(handle);
        let rendered = render_app(app, 100, 30);

        // Then
        assert!(rendered.contains("CURRENT-PHYSICAL-CONTENT"));
        assert!(!rendered.contains("STALE-PREVIEW"));
    });
}

#[test]
fn chat_file_sidebar_saved_delete_closes_chat_link_viewer() {
    with_sidebar_app("saved-delete-chat-viewer", |app, _| {
        // Given
        let (chat_handle, path) = create_link(app, "cross.md", b"contents");
        let saved_handle = app.ui.saved_artifacts[0].handle.clone();
        app.open_artifact(chat_handle);
        assert!(app.ui.artifact_viewer.is_some());

        // When
        app.delete_artifact(saved_handle);

        // Then
        assert!(app.ui.artifact_viewer.is_none());
        assert!(!path.exists());
    });
}

#[test]
fn chat_file_sidebar_chat_delete_closes_saved_viewer() {
    with_sidebar_app("chat-delete-saved-viewer", |app, _| {
        // Given
        let (chat_handle, path) = create_link(app, "cross.md", b"contents");
        let saved_handle = app.ui.saved_artifacts[0].handle.clone();
        app.open_artifact(saved_handle);
        assert!(app.ui.artifact_viewer.is_some());

        // When
        app.delete_artifact(chat_handle);

        // Then
        assert!(app.ui.artifact_viewer.is_none());
        assert!(!path.exists());
    });
}

#[test]
fn chat_file_sidebar_missing_file_refresh_removes_broken_actions() {
    with_sidebar_app("missing-file", |app, _| {
        // Given
        let (_, path) = create_link(app, "missing.md", b"contents");
        let chat_files = app.ui.tabs[0].temporary_artifacts.clone();
        let mut state = ArtifactSidebarState::default();
        let terminal = render_sidebar(&chat_files, &[], ALL_SECTIONS, &mut state, 48, 20);
        let old_view = label_position(terminal.backend().buffer(), "[View]");
        app.ui.artifact_sidebar_state = state;
        std::fs::remove_file(path).expect("remove canonical file externally");

        // When
        let conversation_id = app.ui.tabs[0].active_conversation;
        app.refresh_canonical_file_catalogs(0, conversation_id)
            .expect("reconcile missing file");

        // Then
        assert!(app.ui.tabs[0].temporary_artifacts.is_empty());
        assert!(app.ui.saved_artifacts.is_empty());
        assert!(app.ui.artifact_sidebar_state.action_at(old_view).is_none());
    });
}
