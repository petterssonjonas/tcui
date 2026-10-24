use super::*;

#[test]
fn deleting_chat_link_removes_canonical_file_and_both_listings() {
    // Given
    let environment = Environment::new("delete-chat-link");
    let saved_dir = environment.saved_dir();
    let mut app = start_app();
    let handle = save_report(&mut app, "# Report");

    // When
    app.delete_artifact(handle);

    // Then
    assert!(app.ui.saved_artifacts.is_empty());
    assert!(app.ui.tabs[0].temporary_artifacts.is_empty());
    assert!(!saved_dir.join("report.md").exists());
}

#[test]
fn deleting_saved_row_removes_canonical_file_and_chat_listing() {
    // Given
    let environment = Environment::new("delete-saved-row");
    let saved_dir = environment.saved_dir();
    let mut app = start_app();
    save_report(&mut app, "# Report");
    let saved_handle = ArtifactHandle::Saved(saved_dir.join("report.md"));

    // When
    app.delete_artifact(saved_handle);

    // Then
    assert!(app.ui.saved_artifacts.is_empty());
    assert!(app.ui.tabs[0].temporary_artifacts.is_empty());
    assert!(!saved_dir.join("report.md").exists());
}

#[test]
fn deleting_conversation_removes_links_but_preserves_saved_bytes() {
    // Given
    let environment = Environment::new("delete-conversation");
    let saved_dir = environment.saved_dir();
    let mut app = start_app();
    let conversation_id = app.ui.tabs[0].active_conversation;
    save_report(&mut app, "# Report\n\nKeep me.");

    // When
    app.delete_conversation_by_id(conversation_id)
        .expect("delete conversation");

    // Then
    assert!(app
        .chat_files
        .list_for_conversation(conversation_id)
        .expect("list deleted conversation links")
        .is_empty());
    assert_eq!(
        std::fs::read_to_string(saved_dir.join("report.md")).expect("read preserved file"),
        "# Report\n\nKeep me."
    );
}

#[test]
fn orphan_conversation_marker_retries_metadata_cleanup_on_restart() {
    // Given
    let environment = Environment::new("orphan-recovery");
    let saved_dir = environment.saved_dir();
    let mut app = start_app();
    let conversation_id = app.ui.tabs[0].active_conversation;
    save_report(&mut app, "# Report");
    app.chat_files.fail_next_conversation_cleanup();

    // When
    app.delete_conversation_by_id(conversation_id)
        .expect("conversation deletion stays committed");

    // Then
    let marker = environment
        .root
        .join("data/tcui/chat-file-links")
        .join(format!(".orphan-conversation-{conversation_id}"));
    assert!(marker.exists());
    assert_eq!(
        app.chat_files
            .list_for_conversation(conversation_id)
            .expect("list retained link")
            .len(),
        1
    );
    drop(app);
    let restarted = start_app();
    assert!(!marker.exists());
    assert!(restarted
        .chat_files
        .list_for_conversation(conversation_id)
        .expect("list recovered links")
        .is_empty());
    assert_eq!(
        std::fs::read_to_string(saved_dir.join("report.md")).expect("read preserved file"),
        "# Report"
    );
}
