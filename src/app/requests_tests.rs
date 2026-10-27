use super::*;
use crate::app::action::{Action, AssistantFinalization, GeneratedFileFinalization};
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
fn ordinary_lock_save_failure_keeps_partial_response_and_keys_for_retry() {
    with_test_app("security-lock-save-retry", |app| {
        crate::protection::configure(
            crate::protection::ProtectionPolicy {
                app_password: true,
                chats: true,
                ..Default::default()
            },
            "lock-test-password",
        )
        .expect("protect chats");
        let conversation = app.ui.tabs[0].active_conversation;
        let id = register(app, conversation, conversation);
        app.append_request_content(id, "partial response", false)
            .unwrap();
        app.action_tx
            .send(Action::StreamResponse(id, " already queued".into()))
            .unwrap();
        app.lock_application().expect("request ordinary lock");
        let paths = crate::storage::paths::TcuiDataPaths::discover();
        let backup = paths.root.join("chats-save-failure-backup");
        std::fs::rename(&paths.chats_dir, &backup).expect("retain fixture chats");
        std::fs::write(&paths.chats_dir, b"temporary save obstruction")
            .expect("inject save failure");
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let error = runtime
            .block_on(app.quiesce_for_security_restart())
            .expect_err("save must fail");
        assert!(app.recover_security_restart(&error));
        assert!(crate::protection::is_unlocked().unwrap());
        assert!(crate::protection::require_app().is_ok());
        assert!(!app.ui.protection_blocked);
        assert!(app.security_restart.is_none());
        assert!(app.requests[&id].messages[1]
            .content
            .contains("partial response already queued"));

        std::fs::remove_file(&paths.chats_dir).expect("remove fixture obstruction");
        std::fs::rename(&backup, &paths.chats_dir).expect("restore fixture chats");
        app.lock_application().expect("retry ordinary lock");
        runtime
            .block_on(app.quiesce_for_security_restart())
            .expect("retry save");
        assert!(app.requests.is_empty());
        assert_eq!(
            app.storage.get_messages(conversation).unwrap()[1].content,
            "partial response already queued\n\n[Response cancelled]"
        );
    });
}

#[test]
fn forced_revocation_never_recovers_into_the_cached_application() {
    with_test_app("security-forced-revocation", |app| {
        app.lock_application_after_error("credential authentication failed".into());
        let error = color_eyre::eyre::eyre!("partial response could not be saved");
        assert!(!app.recover_security_restart(&error));
        assert!(app.ui.protection_blocked);
        assert_eq!(
            app.security_restart,
            Some(crate::protection::ui::SecurityRestart::Lock)
        );
    });
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
                None,
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
            .create_conversation_snapshot(0, "Destination", &[], None)
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
                None,
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

#[test]
fn recovered_requests_keep_captured_selection_through_registration_and_repeated_rerouting() {
    for missing_at_registration in [false, true] {
        with_test_app(
            if missing_at_registration {
                "request-selection-missing-registration"
            } else {
                "request-selection-deleted-destination"
            },
            |app| {
                let selection = crate::config::ChatSelection {
                    provider: "Ollama".into(),
                    model: "request-only-model".into(),
                    effort: Some("high".into()),
                };
                app.ui.tabs[0].tab.provider = selection.provider.clone();
                app.ui.tabs[0].tab.model = selection.model.clone();
                app.ui.tabs[0].tab.reasoning_effort = selection.effort.clone();
                let origin = app.ui.tabs[0].active_conversation;
                if missing_at_registration {
                    app.storage.delete_conversation(origin).unwrap();
                }
                let id = register(app, origin, origin);
                let registered = app.requests[&id].route.destination_conversation_id;
                if missing_at_registration {
                    assert_ne!(registered, origin);
                    assert_eq!(
                        app.storage
                            .read_chat_document(registered)
                            .unwrap()
                            .selection,
                        Some(selection.clone())
                    );
                }
                app.append_request_content(id, "partial answer", false)
                    .unwrap();
                for replacement_id in [123, 456] {
                    let previous = app.requests[&id].route.destination_conversation_id;
                    // Replace the UI owner as well as its selection: recovery must update
                    // routing identity without replacing the request's captured selection.
                    app.ui.tabs[0].tab.id = Some(replacement_id);
                    app.ui.tabs[0].tab.provider = "OpenAI".into();
                    app.ui.tabs[0].tab.model = "different-visible-model".into();
                    app.ui.tabs[0].tab.reasoning_effort = Some("low".into());
                    app.storage.delete_conversation(previous).unwrap();
                    app.ensure_request_destination(id).unwrap();
                    let request = &app.requests[&id];
                    let recovered = request.route.destination_conversation_id;
                    assert_ne!(recovered, previous);
                    assert_eq!(request.route.origin_conversation_id, origin);
                    assert_eq!(request.owner.id, Some(replacement_id));
                    assert_eq!(request.owner.provider, selection.provider);
                    assert_eq!(request.owner.model, selection.model);
                    assert_eq!(request.owner.reasoning_effort, selection.effort);
                    let reopened = crate::storage::Storage::new().unwrap();
                    let document = reopened.read_chat_document(recovered).unwrap();
                    assert_eq!(document.selection.as_ref(), Some(&selection));
                    assert_eq!(document.messages[1].content, "partial answer");
                    assert!(document.parent.is_none());
                    let exported = crate::export::render_chat_document(
                        &document,
                        crate::export::OutputFormat::Json,
                    )
                    .unwrap();
                    let exported: serde_json::Value = serde_json::from_str(&exported).unwrap();
                    assert_eq!(
                        exported["selection"],
                        serde_json::to_value(&selection).unwrap()
                    );
                }
                let recovered = app.requests[&id].route.destination_conversation_id;
                complete(app, id, "complete answer");
                app.load_conversation_into_tab(0, recovered).unwrap();
                assert_eq!(app.ui.tabs[0].tab.provider, selection.provider);
                assert_eq!(app.ui.tabs[0].tab.model, selection.model);
                assert_eq!(app.ui.tabs[0].tab.reasoning_effort, selection.effort);
                assert_eq!(app.ui.tabs[0].messages[1].content, "complete answer");
            },
        );
    }
}

#[cfg(unix)]
#[test]
fn cancellation_waits_for_concurrently_committed_host_receipt_before_removing_request() {
    use std::os::unix::fs::PermissionsExt;
    with_test_app("late-host-receipt", |app| {
        let conversation = app.ui.tabs[0].active_conversation;
        let id = register(app, conversation, conversation);
        let home = std::path::PathBuf::from(std::env::var_os("HOME").unwrap());
        let program = home.join("committed-action");
        let effect = home.join("actual-effect");
        std::fs::write(
            &program,
            "#!/bin/sh\nprintf 'completed' > \"$HOME/actual-effect\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut config = app.config.blocking_read().clone();
        config.commands.whitelist = vec![program.to_string_lossy().into_owned()];
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let receipts = app.requests[&id].host_receipts.clone();
            let sender = app.action_tx.clone();
            let completion = StreamCompletion {
                request_id: id,
                sender: sender.clone(),
            };
            let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            let worker = tokio::spawn(crate::skill_runtime::host_receipts::capture(
                receipts,
                id,
                sender,
                async move {
                    let _completion = completion;
                    let output = crate::commands::run(&config, program.to_str().unwrap(), &[])
                        .await
                        .unwrap();
                    assert!(output.success);
                    entered_tx.send(()).unwrap();
                    // Hold a currently executing poll, not a pending future: abort cannot
                    // preempt it, reproducing the multithreaded publication race.
                    release_rx
                        .recv_timeout(std::time::Duration::from_secs(5))
                        .unwrap();
                    crate::skill_runtime::host_receipts::record(
                        "Confirmed host command completed".into(),
                    );
                    std::future::pending::<()>().await;
                },
            ));
            app.requests.get_mut(&id).unwrap().abort = Some(worker.abort_handle());
            entered_rx.await.unwrap();
            assert_eq!(std::fs::read(&effect).unwrap(), b"completed");
            app.cancel_request(id).unwrap();
            app.cancel_request(id).unwrap();
            assert!(
                app.requests.contains_key(&id),
                "retain destination until producer stops"
            );
            release_tx.send(()).unwrap();
            assert!(worker.await.unwrap_err().is_cancelled());
            while let Ok(action) = app.action_rx.try_recv() {
                app.dispatch(action).await.unwrap();
            }
        });
        assert!(!app.requests.contains_key(&id));
        let persisted = app.storage.get_messages(conversation).unwrap();
        assert!(persisted
            .iter()
            .any(
                |message| message.tool_source.as_deref() == Some("Host action receipt")
                    && message.content == "Confirmed host command completed"
            ));
        assert!(persisted
            .last()
            .unwrap()
            .content
            .contains("[Response cancelled]"));
    });
}
