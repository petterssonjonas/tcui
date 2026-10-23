use crate::app::message::Message;
use crate::app::tests::with_test_app;
use crate::storage::db::Storage;

#[test]
fn fork_persists_independent_history_and_keeps_current_selection() {
    with_test_app("fork-independent-history", |app| {
        let source_id = app.ui.tabs[0].active_conversation;
        let mut original = Message::new(source_id, "user".into(), "source question".into());
        original.id = Some(41);
        original.thinking_content = Some("copied thinking".into());
        app.storage
            .replace_messages(source_id, &[original])
            .expect("persist source");
        app.storage
            .update_conversation_title(source_id, "Source title")
            .expect("name source");
        let attachment = crate::storage::paths::TcuiDataPaths::discover()
            .root
            .join("source.txt");
        std::fs::write(&attachment, b"source artifact").expect("write source artifact");
        app.chat_files
            .import_for_chat(source_id, &attachment)
            .expect("attach source artifact");
        app.refresh_tab_conversations(0).expect("refresh source");
        app.load_conversation_into_tab(0, source_id)
            .expect("load source");
        app.ui.tabs[0].tab.provider = "Ollama".into();
        app.ui.tabs[0].tab.model = "selected-model".into();
        app.ui.tabs[0].tab.reasoning_effort = Some("high".into());
        app.ui.tabs[0].manual_model_override = true;
        app.ui.tabs[0].streaming = true;
        let source_before = serde_json::to_value(
            app.storage
                .read_chat_document(source_id)
                .expect("read source"),
        )
        .expect("serialize source");

        app.fork_active_conversation().expect("fork conversation");

        let child_id = app.ui.tabs[0].active_conversation;
        assert_ne!(child_id, source_id);
        assert!(!app.ui.tabs[0].streaming);
        assert_eq!(app.ui.tabs[0].tab.provider, "Ollama");
        assert_eq!(app.ui.tabs[0].tab.model, "selected-model");
        assert_eq!(app.ui.tabs[0].tab.reasoning_effort.as_deref(), Some("high"));
        assert!(app.ui.tabs[0].manual_model_override);
        assert_eq!(
            app.ui.tabs[0].sidebar_state.selection,
            crate::ui::sidebar::SidebarSelection::Conversation(child_id)
        );
        assert!(app
            .chat_files
            .list_for_conversation(child_id)
            .expect("read child artifacts")
            .is_empty());
        assert_eq!(
            app.chat_files
                .list_for_conversation(source_id)
                .expect("read source artifacts")
                .len(),
            1
        );
        assert!(app.ui.tabs[0].temporary_artifacts.is_empty());
        let child = app
            .storage
            .read_chat_document(child_id)
            .expect("read child");
        assert_eq!(child.title, "Fork of Source title");
        assert_eq!(child.messages[0].id, Some(1));
        assert_eq!(child.messages[0].conversation_id, child_id);
        assert_eq!(
            child.messages[0].thinking_content.as_deref(),
            Some("copied thinking")
        );
        let lineage = child.parent.expect("fork lineage");
        assert_eq!(lineage.conversation_id, source_id);
        assert_eq!(lineage.last_message_id, Some(41));
        assert_eq!(lineage.copied_message_count, 1);
        app.storage
            .save_message(&Message::new(
                child_id,
                "user".into(),
                "child continuation".into(),
            ))
            .expect("continue child");

        let reloaded = Storage::new().expect("reopen storage");
        assert_eq!(
            reloaded.get_messages(child_id).expect("reload child").len(),
            2
        );
        assert_eq!(
            serde_json::to_value(
                reloaded
                    .read_chat_document(source_id)
                    .expect("reload source")
            )
            .expect("serialize reloaded source"),
            source_before
        );
        app.load_conversation_into_tab(0, source_id)
            .expect("switch to source");
        app.load_conversation_into_tab(0, child_id)
            .expect("switch to child");
        assert_eq!(app.ui.tabs[0].messages.len(), 2);
        assert!(!app.ui.tabs[0].streaming);
    });
}

#[test]
fn fork_rejects_drafts_and_pending_documents_without_discarding_them() {
    with_test_app("fork-pending-input", |app| {
        let source_id = app.ui.tabs[0].active_conversation;
        let count = app.storage.get_conversations(0).expect("list chats").len();
        app.ui.tabs[0].input_content = "unfinished draft".into();
        let error = app
            .fork_active_conversation()
            .expect_err("draft must be retained");
        assert!(error.to_string().contains("draft"));
        assert_eq!(app.ui.tabs[0].input_content, "unfinished draft");
        app.ui.tabs[0].input_content.clear();
        app.pending_documents_mut()
            .insert(
                crate::app::submission::PendingDocument::snapshot(
                    "source",
                    "draft.md",
                    b"private draft",
                )
                .expect("snapshot document"),
            )
            .expect("queue document");
        let error = app
            .fork_active_conversation()
            .expect_err("documents must be retained");
        assert!(error.to_string().contains("documents"));
        assert_eq!(app.pending_documents_mut().len(), 1);
        assert_eq!(app.ui.tabs[0].active_conversation, source_id);
        assert_eq!(
            app.storage
                .get_conversations(0)
                .expect("list chats after rejection")
                .len(),
            count
        );
    });
}

#[test]
fn complete_snapshot_is_encrypted_and_write_failure_does_not_leave_empty_child() {
    with_test_app("fork-atomic-snapshot", |app| {
        let source_id = app.ui.tabs[0].active_conversation;
        let messages = [Message::new(
            source_id,
            "assistant".into(),
            "partial answer snapshot".into(),
        )];
        let child_id = app
            .storage
            .create_conversation_snapshot(0, "Recovered", &messages)
            .expect("persist complete snapshot");
        let stored = app.storage.get_messages(child_id).expect("read snapshot");
        assert_eq!(stored[0].content, "partial answer snapshot");
        assert_eq!(stored[0].conversation_id, child_id);
        assert_eq!(stored[0].id, Some(1));
        let paths = crate::storage::paths::TcuiDataPaths::discover();
        let raw = std::fs::read_to_string(
            paths
                .chats_dir
                .join(format!("{:016x}.tcui-chat", child_id as u64)),
        )
        .expect("read ciphertext");
        assert!(raw.starts_with("enc:v1:"));
        assert!(!raw.contains("partial answer snapshot"));
        let count = app.storage.get_conversations(0).expect("list chats").len();
        let backup = paths.root.join("chats-backup");
        std::fs::rename(&paths.chats_dir, &backup).expect("hide chats directory");
        std::fs::write(&paths.chats_dir, b"block directory creation")
            .expect("block snapshot write");

        let result = app
            .storage
            .create_conversation_snapshot(0, "Failed", &messages);

        std::fs::remove_file(&paths.chats_dir).expect("unblock directory");
        std::fs::rename(&backup, &paths.chats_dir).expect("restore chats directory");
        assert!(result.is_err());
        assert_eq!(
            app.storage
                .get_conversations(0)
                .expect("list chats after failure")
                .len(),
            count
        );
        assert_eq!(app.ui.tabs[0].active_conversation, source_id);
    });
}

#[test]
fn legacy_documents_default_lineage_and_corruption_is_not_missing_history() {
    with_test_app("fork-lineage-compatibility", |app| {
        let source_id = app.ui.tabs[0].active_conversation;
        let document = app
            .storage
            .read_chat_document(source_id)
            .expect("read source");
        let mut legacy = serde_json::to_value(document).expect("serialize document");
        legacy
            .as_object_mut()
            .expect("chat object")
            .remove("parent");
        let decoded: crate::storage::chat_store::ChatDocument =
            serde_json::from_value(legacy).expect("read old document without lineage");
        assert!(decoded.parent.is_none());
        assert!(app
            .storage
            .conversation_exists(source_id)
            .expect("source exists"));
        assert!(!app
            .storage
            .conversation_exists(0)
            .expect("missing document"));
        let paths = crate::storage::paths::TcuiDataPaths::discover();
        std::fs::write(
            paths
                .chats_dir
                .join(format!("{:016x}.tcui-chat", source_id as u64)),
            b"corrupt ciphertext",
        )
        .expect("corrupt chat");
        assert!(app.storage.conversation_exists(source_id).is_err());
    });
}

#[test]
fn fork_copies_partial_answer_without_transferring_original_request() {
    with_test_app("fork-active-stream", |app| {
        let source_id = app.ui.tabs[0].active_conversation;
        let submission =
            crate::app::submission::InputSubmission::new("question", Default::default());
        let prepared = crate::app::submission::PreparedUserTurn::prepare(&submission)
            .expect("prepare request");
        let request_id = prepared.submission_id();
        let route = crate::app::requests::RequestRoute {
            request_id,
            origin_conversation_id: source_id,
            destination_conversation_id: source_id,
        };
        app.register_request(
            route,
            0,
            vec![Message::new(source_id, "user".into(), "question".into())],
        )
        .expect("register request");
        app.append_request_content(request_id, "partial answer", false)
            .expect("stream partial answer");
        app.append_request_content(request_id, "partial thinking", true)
            .expect("stream partial thinking");
        app.cancel_escape = Some((request_id, std::time::Instant::now()));
        let stored_before = serde_json::to_value(
            app.storage
                .read_chat_document(source_id)
                .expect("read source"),
        )
        .expect("serialize source");
        let snapshot_before = serde_json::to_value(
            app.conversation_snapshot(source_id)
                .expect("snapshot source"),
        )
        .expect("serialize snapshot");

        app.fork_active_conversation().expect("fork active stream");

        let child_id = app.ui.tabs[0].active_conversation;
        assert_ne!(source_id, child_id);
        assert!(app.active_request_id().is_none());
        assert!(app.cancel_escape.is_none());
        assert!(app.request_accepts_events(request_id));
        assert_eq!(app.requests.len(), 1);
        assert_eq!(app.requests[&request_id].route, route);
        assert_eq!(app.ui.tabs[0].messages[1].content, "partial answer");
        assert_eq!(
            app.ui.tabs[0].messages[1].thinking_content.as_deref(),
            Some("partial thinking")
        );
        assert!(!app.ui.tabs[0].streaming);
        assert_eq!(
            serde_json::to_value(
                app.storage
                    .read_chat_document(source_id)
                    .expect("read unchanged source")
            )
            .expect("serialize source"),
            stored_before
        );
        assert_eq!(
            serde_json::to_value(
                app.conversation_snapshot(source_id)
                    .expect("snapshot unchanged source")
            )
            .expect("serialize snapshot"),
            snapshot_before
        );
        app.append_request_content(request_id, " completed", false)
            .expect("continue original request");
        assert_eq!(app.ui.tabs[0].messages[1].content, "partial answer");
        let reloaded = Storage::new().expect("reopen fork storage");
        assert_eq!(
            reloaded.get_messages(child_id).expect("reload fork")[1].content,
            "partial answer"
        );
        app.load_conversation_into_tab(0, source_id)
            .expect("return to running source");
        assert_eq!(app.active_request_id(), Some(request_id));
        assert!(app.ui.tabs[0].streaming);
        assert_eq!(
            app.ui.tabs[0].messages[1].content,
            "partial answer completed"
        );
    });
}

#[test]
fn deleting_running_destination_recovers_snapshot_and_keeps_request_origin() {
    with_test_app("delete-running-destination", |app| {
        let source_id = app.ui.tabs[0].active_conversation;
        let submission =
            crate::app::submission::InputSubmission::new("question", Default::default());
        let prepared = crate::app::submission::PreparedUserTurn::prepare(&submission)
            .expect("prepare request");
        let request_id = prepared.submission_id();
        app.register_request(
            crate::app::requests::RequestRoute {
                request_id,
                origin_conversation_id: source_id,
                destination_conversation_id: source_id,
            },
            0,
            vec![Message::new(source_id, "user".into(), "question".into())],
        )
        .expect("register request");
        app.append_request_content(request_id, "partial answer", false)
            .expect("stream partial answer");

        app.delete_conversation_by_id(source_id)
            .expect("delete and recover running destination");

        let recovered_id = app.ui.tabs[0].active_conversation;
        assert_ne!(recovered_id, source_id);
        assert!(!app
            .storage
            .conversation_exists(source_id)
            .expect("source was deleted"));
        assert_eq!(
            app.requests[&request_id].route.origin_conversation_id,
            source_id
        );
        assert_eq!(
            app.requests[&request_id].route.destination_conversation_id,
            recovered_id
        );
        assert_eq!(
            app.storage
                .get_messages(recovered_id)
                .expect("read recovered snapshot")[1]
                .content,
            "partial answer"
        );
        assert_eq!(app.active_request_id(), Some(request_id));
        assert!(app.ui.tabs[0].streaming);
        app.append_request_content(request_id, " continued", false)
            .expect("continue recovered stream");
        assert_eq!(
            app.ui.tabs[0].messages[1].content,
            "partial answer continued"
        );
    });
}
