use super::*;

#[test]
fn direct_save_preserves_colliding_file_and_survives_restart() {
    // Given
    let _guard = env_lock().lock().expect("env lock poisoned");
    let root = unique_root("direct-save");
    configure_environment(&root);
    let saved_dir = root.join("saved");
    let mut app = app_with_saved_dir(&saved_dir);
    std::fs::write(saved_dir.join("report.md"), "existing").expect("seed collision");

    // When
    let handle = save_report(&mut app, "# Report\n\nPersistent content.");

    // Then
    assert_eq!(
        std::fs::read_to_string(saved_dir.join("report.md")).unwrap(),
        "existing"
    );
    assert_eq!(app.ui.saved_artifacts.len(), 2);
    assert_eq!(app.ui.tabs[0].temporary_artifacts.len(), 1);
    drop(app);
    let restarted = app_with_saved_dir(&saved_dir);
    assert_eq!(restarted.ui.tabs[0].temporary_artifacts[0].handle, handle);
    let saved_path = restarted.ui.tabs[0].temporary_artifacts[0]
        .path
        .as_ref()
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(saved_path).unwrap(),
        "# Report\n\nPersistent content."
    );
    reset_environment(&root);
}

#[test]
fn failed_save_persists_failure_without_file_link_or_success_claim() {
    // Given
    let _guard = env_lock().lock().expect("env lock poisoned");
    let root = unique_root("save-failure");
    configure_environment(&root);
    let saved_dir = root.join("saved");
    let mut app = app_with_saved_dir(&saved_dir);
    let conversation_id = app.ui.tabs[0].active_conversation;
    let (request_id, message_idx) = add_assistant_placeholder(&mut app, conversation_id);
    let generated =
        GeneratedFile::from_skill_response(9, conversation_id, "@save report.md", "# Report")
            .expect("parse @save response");
    app.chat_files.fail_next_link_write();

    // When
    tokio::runtime::Runtime::new()
        .expect("runtime")
        .block_on(
            app.dispatch(Action::FinalizeAssistantMessage(AssistantFinalization {
                request_id,
                message: Message::new(
                    conversation_id,
                    "assistant".to_string(),
                    "# Report".to_string(),
                ),
                generated_file: GeneratedFileFinalization::Ready(generated),
            })),
        )
        .expect("persist failure response");

    // Then
    let response = &app.ui.tabs[0].messages[message_idx].content;
    assert_eq!(response, "Could not save generated file.");
    assert!(!response.contains("Saved as"));
    assert!(app
        .chat_files
        .list_for_conversation(conversation_id)
        .expect("list links")
        .is_empty());
    assert!(app.chat_files.list_saved().expect("list saved").is_empty());
    reset_environment(&root);
}

#[test]
fn completed_model_media_is_imported_once_and_reload_only_hydrates_link() {
    // Given
    let _guard = env_lock().lock().expect("env lock poisoned");
    let root = unique_root("media");
    configure_environment(&root);
    let saved_dir = root.join("saved");
    let source = root.join("generated.png");
    std::fs::write(&source, b"generated-image").expect("write generated media");
    let mut app = app_with_saved_dir(&saved_dir);
    let conversation_id = app.ui.tabs[0].active_conversation;
    let (request_id, _) = add_assistant_placeholder(&mut app, conversation_id);
    let content = format!("![generated]({})", source.display());

    // When
    tokio::runtime::Runtime::new()
        .expect("runtime")
        .block_on(
            app.dispatch(Action::FinalizeAssistantMessage(AssistantFinalization {
                request_id,
                message: Message::new(conversation_id, "assistant".to_string(), content),
                generated_file: GeneratedFileFinalization::NotRequested,
            })),
        )
        .expect("finalize media response");
    drop(app);
    let restarted = app_with_saved_dir(&saved_dir);

    // Then
    assert_eq!(
        restarted.chat_files.list_saved().expect("list saved").len(),
        1
    );
    assert_eq!(restarted.ui.tabs[0].temporary_artifacts.len(), 1);
    assert_eq!(
        std::fs::read(saved_dir.join("generated.png")).expect("read imported media"),
        b"generated-image"
    );
    reset_environment(&root);
}

#[test]
fn generated_file_action_uses_the_canonical_saved_store() {
    // Given
    let _guard = env_lock().lock().expect("env lock poisoned");
    let root = unique_root("skill-action");
    configure_environment(&root);
    let saved_dir = root.join("saved");
    let mut app = app_with_saved_dir(&saved_dir);
    let conversation_id = app.ui.tabs[0].active_conversation;
    let generated = GeneratedFile::from_skill_response(
        11,
        conversation_id,
        "@research @save findings.md",
        "# Findings",
    )
    .expect("parse compound skill output");

    // When
    tokio::runtime::Runtime::new()
        .expect("runtime")
        .block_on(app.dispatch(Action::AddGeneratedFile(0, generated)))
        .expect("persist generated file action");

    // Then
    assert_eq!(app.chat_files.list_saved().expect("list saved").len(), 1);
    assert_eq!(app.ui.tabs[0].temporary_artifacts.len(), 1);
    reset_environment(&root);
}

#[test]
fn invalid_save_response_persists_failure_without_creating_artifact() {
    // Given
    let _guard = env_lock().lock().expect("env lock poisoned");
    let root = unique_root("invalid-save");
    configure_environment(&root);
    let saved_dir = root.join("saved");
    let mut app = app_with_saved_dir(&saved_dir);
    let conversation_id = app.ui.tabs[0].active_conversation;
    let (request_id, message_idx) = add_assistant_placeholder(&mut app, conversation_id);

    // When
    tokio::runtime::Runtime::new()
        .expect("runtime")
        .block_on(
            app.dispatch(Action::FinalizeAssistantMessage(AssistantFinalization {
                request_id,
                message: Message::new(conversation_id, "assistant".to_string(), String::new()),
                generated_file: GeneratedFileFinalization::Invalid,
            })),
        )
        .expect("persist invalid-save response");

    // Then
    assert_eq!(
        app.ui.tabs[0].messages[message_idx].content,
        "Could not save generated file."
    );
    assert!(app.chat_files.list_saved().expect("list saved").is_empty());
    assert!(app.ui.tabs[0].temporary_artifacts.is_empty());
    reset_environment(&root);
}

#[test]
fn chat_links_are_isolated_while_saved_files_remain_global() {
    // Given
    let _guard = env_lock().lock().expect("env lock poisoned");
    let root = unique_root("chat-isolation");
    configure_environment(&root);
    let saved_dir = root.join("saved");
    let mut app = app_with_saved_dir(&saved_dir);
    let first_conversation = app.ui.tabs[0].active_conversation;
    save_report(&mut app, "# Report");
    let second_conversation = app.storage.create_conversation(0).expect("create chat");

    // When
    app.load_conversation_into_tab(0, second_conversation)
        .expect("load second chat");

    // Then
    assert!(app.ui.tabs[0].temporary_artifacts.is_empty());
    assert_eq!(app.ui.saved_artifacts.len(), 1);
    app.load_conversation_into_tab(0, first_conversation)
        .expect("reload first chat");
    assert_eq!(app.ui.tabs[0].temporary_artifacts.len(), 1);
    reset_environment(&root);
}
