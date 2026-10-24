use super::*;

#[test]
fn direct_save_preserves_colliding_file_and_survives_restart() {
    // Given
    let environment = Environment::new("direct-save");
    let saved_dir = environment.saved_dir();
    let mut app = start_app();
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
    let restarted = start_app();
    assert_eq!(restarted.ui.tabs[0].temporary_artifacts[0].handle, handle);
    let saved_path = restarted.ui.tabs[0].temporary_artifacts[0]
        .path
        .as_ref()
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(saved_path).unwrap(),
        "# Report\n\nPersistent content."
    );
}

#[test]
fn failed_save_preserves_completed_response_without_file_link_or_success_claim() {
    // Given
    let _environment = Environment::new("save-failure");
    let mut app = start_app();
    let conversation_id = app.ui.tabs[0].active_conversation;
    let (request_id, message_idx) = add_assistant_placeholder(&mut app, conversation_id);
    let generated =
        GeneratedFile::from_skill_response(9, conversation_id, "@save report.md", "# Report")
            .expect("parse @save response");
    app.chat_files.fail_next_link_write();

    // When
    assert!(tokio::runtime::Runtime::new()
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
        .is_err());

    // Then
    let response = &app.ui.tabs[0].messages[message_idx].content;
    assert_eq!(response, "# Report");
    assert_eq!(
        app.requests[&request_id].messages[message_idx].content,
        "# Report"
    );
    assert!(app.requests[&request_id].finalized);
    assert_eq!(
        app.storage
            .get_messages(conversation_id)
            .expect("persisted response")[message_idx]
            .content,
        "# Report"
    );
    assert_eq!(
        app.ui.connection_status,
        crate::ui::status_bar::ConnectionStatus::Failed
    );
    assert!(app
        .chat_files
        .list_for_conversation(conversation_id)
        .expect("list links")
        .is_empty());
    assert!(app.chat_files.list_saved().expect("list saved").is_empty());
}

#[test]
fn queued_generated_file_save_failure_during_lock_preserves_the_completed_response() {
    let environment = Environment::new("queued-save-failure");
    let saved_dir = environment.saved_dir();
    let mut app = start_app();
    let conversation_id = app.ui.tabs[0].active_conversation;
    let (request_id, message_idx) = add_assistant_placeholder(&mut app, conversation_id);
    let body = "# Completed report\n\nThis response must remain recoverable.";
    let generated =
        GeneratedFile::from_skill_response(10, conversation_id, "@save report.md", body)
            .expect("parse completed report");
    std::fs::remove_dir(&saved_dir).expect("remove empty saved-file destination");
    std::fs::write(&saved_dir, b"broken saved-file destination")
        .expect("break saved-file destination");
    app.action_tx
        .send(Action::FinalizeAssistantMessage(AssistantFinalization {
            request_id,
            message: Message::new(conversation_id, "assistant".into(), body.into()),
            generated_file: GeneratedFileFinalization::Ready(generated),
        }))
        .expect("queue final response");
    app.action_tx
        .send(Action::StopStream(request_id))
        .expect("queue stream completion");
    app.lock_application().expect("request ordinary lock");
    let runtime = tokio::runtime::Runtime::new().expect("runtime");

    let error = runtime
        .block_on(app.quiesce_for_security_restart())
        .expect_err("artifact failure must stop ordinary lock");

    assert!(app.recover_security_restart(&error));
    assert!(app.security_restart.is_none());
    assert!(!app.ui.protection_blocked);
    assert!(app.requests.is_empty());
    assert_eq!(app.ui.tabs[0].messages[message_idx].content, body);
    assert_eq!(
        app.storage
            .get_messages(conversation_id)
            .expect("working chat storage")[message_idx]
            .content,
        body
    );
    assert_eq!(
        app.ui.connection_status,
        crate::ui::status_bar::ConnectionStatus::Failed
    );
    assert!(app.ui.tabs[0].temporary_artifacts.is_empty());
    assert_eq!(
        std::fs::read(&saved_dir).expect("obstruction unchanged"),
        b"broken saved-file destination"
    );

    std::fs::remove_file(&saved_dir).expect("remove fixture obstruction");
    std::fs::create_dir(&saved_dir).expect("restore saved-file destination");
    assert!(app
        .chat_files
        .list_for_conversation(conversation_id)
        .expect("no phantom links")
        .is_empty());
    assert!(app
        .chat_files
        .list_saved()
        .expect("no generated file")
        .is_empty());
    app.lock_application().expect("retry ordinary lock");
    runtime
        .block_on(app.quiesce_for_security_restart())
        .expect("safe lock retry");
    drop(app);
    let restarted = start_app();
    assert_eq!(
        restarted
            .storage
            .get_messages(conversation_id)
            .expect("response survives restart")[message_idx]
            .content,
        body
    );
    drop(restarted);
}

#[test]
fn completed_model_media_is_imported_once_and_reload_only_hydrates_link() {
    // Given
    let environment = Environment::new("media");
    let saved_dir = environment.saved_dir();
    let source = environment.root.join("generated.png");
    std::fs::write(&source, b"generated-image").expect("write generated media");
    let mut app = start_app();
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
    let restarted = start_app();

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
}

#[test]
fn generated_file_action_uses_the_canonical_saved_store() {
    // Given
    let _environment = Environment::new("skill-action");
    let mut app = start_app();
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
}

#[test]
fn invalid_save_response_persists_failure_without_creating_artifact() {
    // Given
    let _environment = Environment::new("invalid-save");
    let mut app = start_app();
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
    assert!(app.requests[&request_id].finalized);
    assert_eq!(
        app.ui.connection_status,
        crate::ui::status_bar::ConnectionStatus::Failed
    );
    let response = &app.ui.tabs[0].messages[message_idx].content;
    assert!(!response.is_empty());
    assert_eq!(
        &app.storage
            .get_messages(conversation_id)
            .expect("persisted failure")[message_idx]
            .content,
        response
    );
    assert!(app.chat_files.list_saved().expect("list saved").is_empty());
    assert!(app.ui.tabs[0].temporary_artifacts.is_empty());
}

#[test]
fn chat_links_are_isolated_while_saved_files_remain_global() {
    // Given
    let _environment = Environment::new("chat-isolation");
    let mut app = start_app();
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
}
