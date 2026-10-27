use super::*;
use crate::app::action::{AssistantFinalization, GeneratedFileFinalization};
use crate::app::message::Message;
use crate::app::requests::RequestRoute;
use crate::app::submission::{InputSubmission, PendingDocument, PreparedUserTurn, SubmissionId};
use crate::app::tests::with_test_app;

fn seed(app: &mut TuiApp) -> i64 {
    let id = app.ui.tabs[0].active_conversation;
    app.storage
        .update_conversation_title(id, "Research")
        .unwrap();
    app.storage
        .replace_messages(
            id,
            &[Message::new(id, "user".into(), "inherited question".into())],
        )
        .unwrap();
    app.refresh_tab_conversations(0).unwrap();
    app.load_conversation_into_tab(0, id).unwrap();
    id
}

fn register(app: &mut TuiApp, question: &str) -> SubmissionId {
    let destination = app.ui.tabs[app.ui.active_tab].active_conversation;
    let id = PreparedUserTurn::prepare(&InputSubmission::new(question, Default::default()))
        .unwrap()
        .submission_id();
    let mut messages = app.conversation_snapshot(destination).unwrap();
    messages.push(Message::new(destination, "user".into(), question.into()));
    app.register_request(
        RequestRoute {
            request_id: id,
            origin_conversation_id: destination,
            destination_conversation_id: destination,
        },
        app.ui.active_tab,
        messages,
    )
    .unwrap();
    id
}

#[test]
fn temporary_fork_preserves_live_parent_and_isolates_drafts_and_documents() {
    with_test_app("ephemeral-parent-input", |app| {
        let source = seed(app);
        let parent_request = register(app, "parent request");
        app.append_request_content(parent_request, "live parent answer", false)
            .unwrap();
        app.replace_input_content("unsent main draft".into());
        app.pending_documents_mut()
            .insert(PendingDocument::snapshot("source", "main.md", b"main document").unwrap())
            .unwrap();
        let main_input = app.input_context_id();
        let previous = app.ui.panel_state.right;
        app.open_ephemeral().unwrap();
        let temporary = app.ui.tabs[1].active_conversation;
        assert!(temporary < 0);
        assert_eq!(
            app.ui.ephemeral.as_ref().unwrap().title,
            "Ephemeral fork of Research conversation"
        );
        assert_eq!(
            app.ui.tabs[1].messages.last().unwrap().content,
            "live parent answer"
        );
        assert!(!app.ui.tabs[1].streaming);
        assert_ne!(app.input_context_id(), main_input);
        assert!(!app.has_pending_documents());
        app.replace_input_content("unsent temporary draft".into());
        app.pending_documents_mut()
            .insert(PendingDocument::snapshot("source", "temp.md", b"temporary document").unwrap())
            .unwrap();
        let temporary_input = app.input_context_id();
        app.focus_chat_pane(0);
        app.open_ephemeral().unwrap();
        assert_eq!(app.ui.tabs.len(), 2);
        assert_eq!(app.ui.tabs[1].input_content, "unsent temporary draft");
        app.close_ephemeral().unwrap();
        assert_eq!(app.ui.active_tab, 0);
        assert_eq!(app.ui.tabs[0].active_conversation, source);
        assert_eq!(app.ui.tabs[0].input_content, "unsent main draft");
        assert!(app.has_pending_documents());
        assert!(!app.pending_documents.contains_key(&temporary_input));
        assert!(app.requests.contains_key(&parent_request));
        assert_eq!(app.ui.panel_state.right, previous);
        assert!(!app.storage.conversation_exists(temporary).unwrap());
        assert_eq!(app.storage.get_conversations(0).unwrap().len(), 1);
    });
}

#[test]
fn closing_aborts_temporary_worker_and_rejects_late_output_without_recovery() {
    with_test_app("ephemeral-close-worker", |app| {
        seed(app);
        app.open_ephemeral().unwrap();
        let temporary = app.ui.tabs[1].active_conversation;
        let id = register(app, "temporary request");
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let handle = runtime.spawn(std::future::pending::<()>());
        app.requests.get_mut(&id).unwrap().abort = Some(handle.abort_handle());
        app.append_request_content(id, "partial branch answer", false)
            .unwrap();
        app.close_ephemeral().unwrap();
        assert!(runtime.block_on(handle).unwrap_err().is_cancelled());
        app.append_request_content(id, "late answer", false)
            .unwrap();
        app.finalize_assistant_message(AssistantFinalization {
            request_id: id,
            message: Message::new(temporary, "assistant".into(), "late saved content".into()),
            generated_file: GeneratedFileFinalization::Ready(crate::app::GeneratedFile {
                id: 1,
                conversation_id: temporary,
                name: "late.md".into(),
                content: "must not persist".into(),
            }),
        })
        .unwrap();
        app.finish_request(id).unwrap();
        assert!(app.chat_files.list_saved().unwrap().is_empty());
        assert!(!app.storage.conversation_exists(temporary).unwrap());
        assert_eq!(app.storage.get_conversations(0).unwrap().len(), 1);
        assert_eq!(app.ui.tabs[0].messages[0].content, "inherited question");
        let route = RequestRoute {
            request_id: id,
            origin_conversation_id: temporary,
            destination_conversation_id: temporary,
        };
        assert!(app.register_request(route, 0, Vec::new()).is_err());
        assert_eq!(app.storage.get_conversations(0).unwrap().len(), 1);
    });
}

#[test]
fn requested_context_uses_live_branch_excludes_inherited_and_hidden_content() {
    with_test_app("ephemeral-context-read", |app| {
        seed(app);
        assert!(app
            .ephemeral_context_for_request("read the temp chat")
            .is_err());
        app.open_ephemeral().unwrap();
        let id = register(app, "branch-only question @save quoted-marker");
        app.append_request_content(id, "live branch-only answer", false)
            .unwrap();
        app.append_request_content(id, "hidden reasoning", true)
            .unwrap();
        app.focus_chat_pane(0);
        assert!(app
            .ephemeral_context_for_request("Discuss temporary chats")
            .unwrap()
            .is_none());
        let context = app
            .ephemeral_context_for_request("Please read the ephemeral chat")
            .unwrap()
            .unwrap();
        assert!(context.contains("branch-only question @save quoted-marker"));
        assert!(context.contains("live branch-only answer"));
        assert!(!context.contains("inherited question"));
        assert!(!context.contains("hidden reasoning"));
        let other = app.storage.create_conversation(0).unwrap();
        app.load_conversation_into_tab(0, other).unwrap();
        let full = app
            .ephemeral_context_for_request("Copy the temporary conversation into this chat")
            .unwrap()
            .unwrap();
        assert!(full.contains("inherited question"));
        app.close_ephemeral().unwrap();
        assert!(app
            .ephemeral_context_for_request("read the temp chat")
            .is_err());
    });
}

#[test]
fn explicit_saved_artifact_survives_close_without_persisting_temporary_chat_links() {
    with_test_app("ephemeral-explicit-save", |app| {
        let source = seed(app);
        app.open_ephemeral().unwrap();
        let temporary = app.ui.tabs[1].active_conversation;
        let id = register(app, "@save branch result");
        app.finalize_assistant_message(AssistantFinalization {
            request_id: id,
            message: Message::new(
                temporary,
                "assistant".into(),
                "# Deliberately saved\n\nBranch result.".into(),
            ),
            generated_file: GeneratedFileFinalization::Ready(crate::app::GeneratedFile {
                id: 1,
                conversation_id: temporary,
                name: "branch.md".into(),
                content: "# Deliberately saved\n\nBranch result.".into(),
            }),
        })
        .unwrap();
        app.finish_request(id).unwrap();
        app.close_ephemeral().unwrap();
        let saved = app.chat_files.list_saved().unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(
            std::fs::read_to_string(&saved[0].path).unwrap(),
            "# Deliberately saved\n\nBranch result."
        );
        assert!(app
            .chat_files
            .list_for_conversation(source)
            .unwrap()
            .is_empty());
        assert!(!app.storage.conversation_exists(temporary).unwrap());
        assert_eq!(
            app.storage.get_messages(source).unwrap()[0].content,
            "inherited question"
        );
    });
}

#[test]
fn ordinary_temporary_media_output_does_not_create_durable_artifacts() {
    with_test_app("ephemeral-media-private", |app| {
        seed(app);
        let path = crate::storage::paths::TcuiDataPaths::discover()
            .root
            .join("private.png");
        std::fs::write(&path, b"private media").unwrap();
        app.open_ephemeral().unwrap();
        let id = register(app, "show a local image");
        app.finalize_assistant_message(AssistantFinalization {
            request_id: id,
            message: Message::new(
                0,
                "assistant".into(),
                format!("![image]({})", path.display()),
            ),
            generated_file: GeneratedFileFinalization::NotRequested,
        })
        .unwrap();
        app.finish_request(id).unwrap();
        app.close_ephemeral().unwrap();
        assert!(app.chat_files.list_saved().unwrap().is_empty());
        assert_eq!(std::fs::read(path).unwrap(), b"private media");
    });
}

#[test]
fn persistent_fork_from_temporary_opens_main_and_survives_temporary_close() {
    with_test_app("ephemeral-persistent-fork", |app| {
        seed(app);
        app.open_ephemeral().unwrap();
        let temporary = app.ui.tabs[1].active_conversation;
        app.storage
            .save_message(&Message::new(
                temporary,
                "assistant".into(),
                "branch result".into(),
            ))
            .unwrap();
        app.ui.tabs[1].tab.model = "temporary-model".into();
        app.ui.tabs[1].manual_model_override = true;
        app.fork_active_conversation().unwrap();
        let child = app.ui.tabs[0].active_conversation;
        assert!(child > 0);
        assert_eq!(app.ui.active_tab, 0);
        assert_eq!(app.ui.tabs[0].tab.model, "temporary-model");
        assert_eq!(
            app.storage.read_chat_document(child).unwrap().title,
            "Fork of Ephemeral fork of Research conversation"
        );
        assert_eq!(
            app.storage
                .get_messages(child)
                .unwrap()
                .last()
                .unwrap()
                .content,
            "branch result"
        );
        app.close_ephemeral().unwrap();
        assert_eq!(app.ui.tabs[0].active_conversation, child);
        assert_eq!(
            crate::storage::db::Storage::new()
                .unwrap()
                .get_messages(child)
                .unwrap()
                .last()
                .unwrap()
                .content,
            "branch result"
        );
    });
}

#[test]
fn context_copy_requires_an_explicit_non_negated_request() {
    for instruction in [
        "read the temp chat",
        "Please copy the ephemeral conversation",
        "Summarize the temporary fork",
    ] {
        assert!(requests_ephemeral_context(instruction), "{instruction}");
    }
    for instruction in [
        "There is a temp chat",
        "Do not read the temp chat",
        "Don't copy the ephemeral conversation",
        "Never use the temporary fork",
        "Read this document, which is unrelated to any branch. We also discussed temp chats.",
    ] {
        assert!(!requests_ephemeral_context(instruction), "{instruction}");
    }
}

#[cfg(feature = "memory")]
#[test]
fn memory_command_arguments_do_not_request_temporary_context() {
    for instruction in [
        "@memory read temporary-fork-fact.md",
        "@remember I read the temporary chat before closing it.",
        "@memorize write ephemeral-chat.md\nI use a temporary chat.",
    ] {
        assert!(!requests_ephemeral_context(instruction), "{instruction}");
    }
}
