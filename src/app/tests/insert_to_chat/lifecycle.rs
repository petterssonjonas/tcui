use super::*;

use crate::app::submission::SubmissionState;

#[test]
fn insert_to_chat_prelaunch_failure_retains_chip_and_snapshot() {
    with_test_app("insert-to-chat-prelaunch", |app| {
        // Given
        queue_document(app, "one", "one.md", b"RETAIN-ME");
        app.ui.tabs[0].tab.model.clear();
        let runtime = tokio::runtime::Runtime::new().expect("create runtime");

        // When
        let result = runtime.block_on(app.send_message("instruction".to_string()));

        // Then
        assert!(result.is_err());
        assert!(matches!(
            app.submission_state(),
            Some(SubmissionState::Draft(_))
        ));
        assert_eq!(app.pending_documents_mut().len(), 1);
        assert_eq!(app.ui.tabs[0].pending_document_chips.len(), 1);
        println!("INSERT_TO_CHAT_STATE Draft -> Preparing -> Draft; chips=retained");
    });
}

#[test]
fn insert_to_chat_launch_consumes_chip_and_reload_stays_instruction_only() {
    with_test_app("insert-to-chat-launch", |app| {
        // Given
        #[cfg(feature = "memory")]
        {
            app.config.blocking_write().memory.enabled = false;
        }
        queue_document(app, "one", "one.md", b"ONE-REQUEST-ONLY");
        app.ui.tabs[0].tab.provider = "Ollama".to_string();
        app.ui.tabs[0].tab.endpoint = Some("http://127.0.0.1:9".to_string());
        app.ui.tabs[0].tab.model = "insert-to-chat-test".to_string();
        let runtime = tokio::runtime::Runtime::new().expect("create runtime");

        // When
        runtime
            .block_on(app.send_message("typed instruction".to_string()))
            .expect("launch request");
        let conversation_id = app.ui.tabs[0].active_conversation;
        app.load_conversation_into_tab(0, conversation_id)
            .expect("reload conversation");

        // Then
        assert!(matches!(
            app.submission_state(),
            Some(SubmissionState::Launched { .. })
        ));
        assert!(app
            .pending_documents
            .get(&app.ui.app_tabs.active_id())
            .is_none());
        assert!(app.ui.tabs[0].pending_document_chips.is_empty());
        assert_eq!(app.ui.tabs[0].messages.len(), 1);
        assert_eq!(app.ui.tabs[0].messages[0].content, "typed instruction");
        assert!(!app.ui.tabs[0].messages[0]
            .content
            .contains("ONE-REQUEST-ONLY"));
        let title = app.ui.tabs[0]
            .generated_title
            .as_deref()
            .expect("generated title");
        println!("INSERT_TO_CHAT_TITLE={title}");
        assert_eq!(title, TuiApp::generate_title("typed instruction"));
        assert!(!title.contains("ONE-REQUEST-ONLY"));
        println!("INSERT_TO_CHAT_STATE Draft -> Preparing -> Prepared -> Committed -> Launched; chips=consumed");
    });
}

#[test]
fn insert_to_chat_new_conversation_waits_for_discard_confirmation() {
    with_test_app("insert-to-chat-discard-new", |app| {
        // Given
        queue_document(app, "one", "one.md", b"discardable");
        let original = app.ui.tabs[0].active_conversation;
        let runtime = tokio::runtime::Runtime::new().expect("create runtime");

        // When
        runtime
            .block_on(app.dispatch(Action::NewChat))
            .expect("request new conversation");

        // Then
        assert_eq!(app.ui.tabs[0].active_conversation, original);
        assert!(matches!(
            app.ui.active_modal,
            Some(crate::ui::Modal::DiscardPendingDocuments)
        ));
        assert_eq!(app.pending_documents_mut().len(), 1);
    });
}

#[test]
fn insert_to_chat_discard_cancel_preserves_and_confirm_load_drops_snapshots() {
    with_test_app("insert-to-chat-discard-load", |app| {
        // Given
        queue_document(app, "one", "one.md", b"discardable");
        let original = app.ui.tabs[0].active_conversation;
        let target = app
            .storage
            .create_conversation(0)
            .expect("target conversation");
        app.refresh_tab_conversations(0)
            .expect("refresh conversations");
        let runtime = tokio::runtime::Runtime::new().expect("create runtime");

        // When: request then cancel
        runtime
            .block_on(app.dispatch(Action::LoadConversation(target)))
            .expect("request load");
        assert!(matches!(
            app.ui.active_modal,
            Some(crate::ui::Modal::DiscardPendingDocuments)
        ));
        runtime
            .block_on(app.dispatch(Action::CancelModal))
            .expect("cancel discard");

        // Then: cancel preserves everything
        assert_eq!(app.ui.tabs[0].active_conversation, original);
        assert_eq!(app.pending_documents_mut().len(), 1);
        assert_eq!(app.ui.tabs[0].pending_document_chips.len(), 1);

        // When: request then confirm
        runtime
            .block_on(app.dispatch(Action::LoadConversation(target)))
            .expect("request load again");
        runtime
            .block_on(app.dispatch(Action::ConfirmPendingDocumentDiscard))
            .expect("confirm discard");

        // Then: only snapshots are dropped before the requested load
        assert_eq!(app.ui.tabs[0].active_conversation, target);
        assert!(app.pending_documents_mut().is_empty());
        assert!(app.ui.tabs[0].pending_document_chips.is_empty());
    });
}

#[test]
fn insert_to_chat_switch_tab_waits_for_discard_confirmation() {
    with_test_app("insert-to-chat-discard-switch", |app| {
        // Given
        app.ui.add_tab(Tab::new(
            "Second".to_string(),
            "OpenAI".to_string(),
            "gpt-4o".to_string(),
        ));
        queue_document(app, "one", "one.md", b"discardable");
        let runtime = tokio::runtime::Runtime::new().expect("create runtime");

        // When
        runtime
            .block_on(app.dispatch(Action::SwitchTab(0)))
            .expect("request tab switch");

        // Then
        assert_eq!(app.ui.active_tab, 1);
        assert!(matches!(
            app.ui.active_modal,
            Some(crate::ui::Modal::DiscardPendingDocuments)
        ));
        assert_eq!(app.pending_documents_mut().len(), 1);
    });
}

#[test]
fn insert_to_chat_remove_tab_waits_for_discard_confirmation() {
    with_test_app("insert-to-chat-discard-close", |app| {
        // Given
        app.ui.add_tab(Tab::new(
            "Second".to_string(),
            "OpenAI".to_string(),
            "gpt-4o".to_string(),
        ));
        queue_document(app, "one", "one.md", b"discardable");
        let runtime = tokio::runtime::Runtime::new().expect("create runtime");

        // When
        runtime
            .block_on(app.dispatch(Action::RemoveTab(1)))
            .expect("request tab close");

        // Then
        assert_eq!(app.ui.tabs.len(), 2);
        assert!(matches!(
            app.ui.active_modal,
            Some(crate::ui::Modal::DiscardPendingDocuments)
        ));
        assert_eq!(app.pending_documents_mut().len(), 1);
    });
}

#[test]
fn insert_to_chat_quit_cancel_preserves_and_confirm_drops_snapshots() {
    let config = AppConfig {
        quit_confirmation: false,
        ..AppConfig::default()
    };
    with_test_app_config("insert-to-chat-discard-quit", config, |app| {
        // Given
        queue_document(app, "one", "one.md", b"discardable");
        let runtime = tokio::runtime::Runtime::new().expect("create runtime");

        // When: cancel
        runtime
            .block_on(app.dispatch(app.quit_action()))
            .expect("show quit confirmation");
        runtime
            .block_on(app.dispatch(Action::CancelModal))
            .expect("cancel quit");

        // Then: snapshots remain
        assert_eq!(app.pending_documents_mut().len(), 1);
        assert_eq!(app.ui.tabs[0].pending_document_chips.len(), 1);

        // When: confirm
        runtime
            .block_on(app.dispatch(app.quit_action()))
            .expect("show quit confirmation again");
        runtime
            .block_on(app.dispatch(Action::ConfirmPendingDocumentDiscard))
            .expect("confirm quit discard");

        // Then: snapshots are gone and quit is queued
        assert!(app.pending_documents_mut().is_empty());
        assert!(app.ui.tabs[0].pending_document_chips.is_empty());
        assert!(matches!(app.action_rx.try_recv(), Ok(Action::Quit)));
    });
}
