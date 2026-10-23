use super::*;
use crate::app::action::{AssistantFinalization, GeneratedFileFinalization};
use crate::app::submission::{InputSubmission, PreparedUserTurn};
use crate::app::tests::with_test_app;

fn register(app: &mut TuiApp, origin: i64, destination: i64) -> SubmissionId {
    let id = PreparedUserTurn::prepare(&InputSubmission::new("question", Default::default()))
        .expect("prepare request")
        .submission_id();
    let messages = vec![Message::new(destination, "user".into(), "question".into())];
    app.register_request(
        RequestRoute {
            request_id: id,
            origin_conversation_id: origin,
            destination_conversation_id: destination,
        },
        app.ui.active_tab,
        messages,
    )
    .expect("register request");
    id
}

fn complete(app: &mut TuiApp, id: SubmissionId, content: &str) {
    app.finalize_assistant_message(AssistantFinalization {
        request_id: id,
        message: Message::new(0, "assistant".into(), content.into()),
        generated_file: GeneratedFileFinalization::NotRequested,
    })
    .expect("finalize request");
    app.finish_request(id).expect("finish request");
}

#[test]
fn background_output_and_files_never_replace_the_displayed_conversation() {
    with_test_app("background-request-routing", |app| {
        let source = app.ui.tabs[0].active_conversation;
        let id = register(app, source, source);
        app.append_request_content(id, "first", false)
            .expect("first chunk");
        let other = app
            .storage
            .create_conversation_snapshot(
                0,
                "Other chat",
                &[
                    Message::new(0, "user".into(), "unrelated question".into()),
                    Message::new(0, "assistant".into(), "unrelated answer".into()),
                ],
            )
            .expect("create other conversation");
        app.load_conversation_into_tab(0, other)
            .expect("switch away");
        let other_before =
            serde_json::to_value(app.storage.read_chat_document(other).unwrap()).unwrap();
        app.append_request_content(id, " second", false)
            .expect("late answer");
        app.append_request_content(id, "private reasoning", true)
            .expect("late thinking");
        assert_eq!(app.ui.tabs[0].messages[1].content, "unrelated answer");
        assert!(app.ui.tabs[0].messages[1].thinking_content.is_none());
        assert!(!app.ui.tabs[0].streaming);
        app.load_conversation_into_tab(0, source)
            .expect("return to request");
        assert_eq!(app.ui.tabs[0].messages[1].content, "first second");
        assert_eq!(
            app.ui.tabs[0].messages[1].thinking_content.as_deref(),
            Some("private reasoning")
        );
        assert!(app.ui.tabs[0].streaming);
        app.load_conversation_into_tab(0, other)
            .expect("switch away again");
        app.finalize_assistant_message(AssistantFinalization {
            request_id: id,
            message: Message::new(other, "assistant".into(), "# Report".into()),
            generated_file: GeneratedFileFinalization::Ready(crate::app::GeneratedFile {
                id: 7,
                conversation_id: other,
                name: "report.md".into(),
                content: "# Report".into(),
            }),
        })
        .expect("finalize background artifact");
        app.finish_request(id).expect("persist source");
        assert_eq!(
            serde_json::to_value(app.storage.read_chat_document(other).unwrap()).unwrap(),
            other_before
        );
        assert_eq!(app.ui.tabs[0].messages[1].content, "unrelated answer");
        assert!(app.ui.tabs[0].temporary_artifacts.is_empty());
        assert!(app
            .chat_files
            .list_for_conversation(other)
            .unwrap()
            .is_empty());
        let artifacts = app.chat_files.list_for_conversation(source).unwrap();
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].filename, "report.md");
        let stored = app.storage.get_messages(source).unwrap();
        assert_eq!(stored[1].content, "Saved as report.md");
        assert_eq!(
            stored[1].request.unwrap().destination_conversation_id,
            source
        );
    });
}

#[test]
fn explicit_destination_is_independent_and_each_chat_allows_one_request() {
    with_test_app("explicit-request-destination", |app| {
        let origin = app.ui.tabs[0].active_conversation;
        let destination = app
            .storage
            .create_conversation_snapshot(0, "Destination", &[])
            .unwrap();
        let routed = register(app, origin, destination);
        let local = register(app, origin, origin);
        let duplicate =
            PreparedUserTurn::prepare(&InputSubmission::new("duplicate", Default::default()))
                .unwrap()
                .submission_id();
        assert!(app
            .register_request(
                RequestRoute {
                    request_id: duplicate,
                    origin_conversation_id: origin,
                    destination_conversation_id: destination,
                },
                0,
                Vec::new()
            )
            .is_err());
        app.append_request_content(routed, "remote answer", false)
            .unwrap();
        app.append_request_content(local, "local answer", false)
            .unwrap();
        assert_eq!(app.active_request_id(), Some(local));
        assert_eq!(app.ui.tabs[0].messages[1].content, "local answer");
        complete(app, routed, "remote answer");
        complete(app, local, "local answer");
        let remote = app.storage.get_messages(destination).unwrap();
        assert_eq!(remote[1].content, "remote answer");
        assert_eq!(
            remote[1].request,
            Some(RequestRoute {
                request_id: routed,
                origin_conversation_id: origin,
                destination_conversation_id: destination,
            })
        );
        assert_eq!(
            app.storage.get_messages(origin).unwrap()[1].content,
            "local answer"
        );
        assert!(app.requests.is_empty());
    });
}

#[test]
fn cancelled_request_cannot_mutate_a_replacement_request_or_create_files() {
    with_test_app("cancelled-late-events", |app| {
        let conversation = app.ui.tabs[0].active_conversation;
        let old = register(app, conversation, conversation);
        app.requests.get_mut(&old).unwrap().hide_answer = true;
        app.append_request_content(old, "partial file body", false)
            .unwrap();
        assert_eq!(app.ui.tabs[0].messages[1].content, "");
        app.cancel_active_request().unwrap();
        assert_eq!(
            app.storage.get_messages(conversation).unwrap()[1].content,
            "partial file body\n\n[Response cancelled]"
        );
        let fresh = register(app, conversation, conversation);
        app.append_request_content(fresh, "new answer", false)
            .unwrap();
        app.append_request_content(old, "late wrong answer", false)
            .unwrap();
        app.append_request_content(old, "late wrong thinking", true)
            .unwrap();
        app.finalize_assistant_message(AssistantFinalization {
            request_id: old,
            message: Message::new(
                conversation,
                "assistant".into(),
                "wrong final answer".into(),
            ),
            generated_file: GeneratedFileFinalization::Ready(crate::app::GeneratedFile {
                id: 8,
                conversation_id: conversation,
                name: "stale.md".into(),
                content: "must not exist".into(),
            }),
        })
        .unwrap();
        app.finish_request(old).unwrap();
        assert_eq!(app.active_request_id(), Some(fresh));
        assert!(app.ui.tabs[0].streaming);
        assert_eq!(app.ui.tabs[0].messages[1].content, "new answer");
        assert!(app.ui.tabs[0].messages[1].thinking_content.is_none());
        assert!(app
            .chat_files
            .list_for_conversation(conversation)
            .unwrap()
            .is_empty());
        assert_ne!(
            app.storage.read_chat_document(conversation).unwrap().title,
            "Wrong title"
        );
        complete(app, fresh, "new answer");
        assert_eq!(
            app.storage.get_messages(conversation).unwrap()[1].content,
            "new answer"
        );
    });
}

#[test]
fn removed_tab_recovers_without_using_the_replacement_tab_index() {
    with_test_app("removed-request-tab", |app| {
        let origin = app.ui.tabs[0].active_conversation;
        let id = register(app, origin, origin);
        app.append_request_content(id, "partial", false).unwrap();
        let mut other_tab = app.ui.tabs[0].tab.clone();
        other_tab.id = Some(123);
        app.ui.add_tab(other_tab);
        let other = app
            .storage
            .create_conversation_snapshot(
                1,
                "Other",
                &[Message::new(0, "user".into(), "keep this".into())],
            )
            .unwrap();
        app.load_conversation_into_tab(1, other).unwrap();
        app.ui.remove_tab(0);
        app.recover_missing_request_destinations().unwrap();
        let recovered = app.requests[&id].route.destination_conversation_id;
        assert_ne!(recovered, origin);
        assert_ne!(recovered, other);
        assert_eq!(app.requests[&id].route.origin_conversation_id, origin);
        assert_eq!(app.ui.tabs[0].active_conversation, other);
        assert_eq!(app.ui.tabs[0].messages[0].content, "keep this");
        assert!(app.ui.tabs[0]
            .conversations
            .iter()
            .any(|chat| chat.id == recovered));
        app.append_request_content(id, " complete", false).unwrap();
        complete(app, id, "partial complete");
        assert_eq!(
            app.storage.get_messages(recovered).unwrap()[1].content,
            "partial complete"
        );
        assert_eq!(
            app.storage.get_messages(other).unwrap()[0].content,
            "keep this"
        );
    });
}

#[test]
fn worker_abort_stops_spinner_and_persists_partial_response() {
    with_test_app("worker-abort-cleanup", |app| {
        let conversation = app.ui.tabs[0].active_conversation;
        let id = register(app, conversation, conversation);
        app.append_request_content(id, "partial before failure", false)
            .unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let sender = app.action_tx.clone();
        runtime.block_on(async {
            let (started_tx, started_rx) = tokio::sync::oneshot::channel();
            let worker = tokio::spawn(async move {
                let _completion = StreamCompletion {
                    request_id: id,
                    sender,
                };
                let _ = started_tx.send(());
                std::future::pending::<()>().await;
            });
            started_rx.await.unwrap();
            worker.abort();
            assert!(worker.await.unwrap_err().is_cancelled());
            let action = app.action_rx.recv().await.unwrap();
            app.dispatch(action).await.unwrap();
        });
        assert!(!app.ui.tabs[0].streaming);
        assert!(app.active_request_id().is_none());
        assert_eq!(
            app.storage.get_messages(conversation).unwrap()[1].content,
            "partial before failure\n\n[Response interrupted]"
        );
    });
}

#[test]
fn corrupt_destination_is_not_replaced_and_cancellation_still_stops_the_view() {
    with_test_app("corrupt-request-destination", |app| {
        let conversation = app.ui.tabs[0].active_conversation;
        let id = register(app, conversation, conversation);
        app.append_request_content(id, "partial", false).unwrap();
        let path = crate::storage::paths::TcuiDataPaths::discover()
            .chats_dir
            .join(format!("{:016x}.tcui-chat", conversation as u64));
        std::fs::write(&path, b"corrupt original").unwrap();
        assert!(app.cancel_active_request().is_err());
        assert!(!app.ui.tabs[0].streaming);
        assert!(!app.request_accepts_events(id));
        assert_eq!(std::fs::read(&path).unwrap(), b"corrupt original");
        assert_eq!(
            app.requests[&id].messages[1].content,
            "partial\n\n[Response cancelled]"
        );
        assert_eq!(
            app.requests[&id].route.destination_conversation_id,
            conversation
        );
    });
}

#[test]
fn missing_destination_is_recovered_without_selecting_it_or_using_the_active_app_tab() {
    with_test_app("request-start-missing-destination", |app| {
        let origin = app.ui.tabs[0].active_conversation;
        let missing = app.storage.create_conversation(0).unwrap();
        app.storage.delete_conversation(missing).unwrap();
        let source_before =
            serde_json::to_value(app.storage.read_chat_document(origin).unwrap()).unwrap();
        app.ui
            .app_tabs
            .open_view(crate::ui::app_tabs::AppView::Settings);
        let settings_id = app.ui.app_tabs.active_id();
        let id = register(app, origin, missing);
        let route = app.requests[&id].route;
        assert_ne!(route.destination_conversation_id, missing);
        assert_ne!(route.destination_conversation_id, origin);
        assert_eq!(route.origin_conversation_id, origin);
        assert_ne!(app.requests[&id].app_tab_id, settings_id);
        assert_eq!(app.ui.app_tabs.active_id(), settings_id);
        assert_eq!(app.ui.tabs[0].active_conversation, origin);
        app.append_request_content(id, "scheduled answer", false)
            .unwrap();
        complete(app, id, "scheduled answer");
        let stored = app
            .storage
            .get_messages(route.destination_conversation_id)
            .unwrap();
        assert_eq!(stored[0].content, "question");
        assert_eq!(stored[1].content, "scheduled answer");
        assert_eq!(stored[1].request, Some(route));
        assert_eq!(
            serde_json::to_value(app.storage.read_chat_document(origin).unwrap()).unwrap(),
            source_before
        );
    });
}
