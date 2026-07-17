use std::path::Path;

use ratatui::{backend::TestBackend, Terminal};

use crate::app::submission::{
    InputSubmission, PendingDocuments, PendingVaultMutation, PreparedUserTurn,
    VaultMutationConfirmationState,
};
use crate::ui::app_tabs::AppView;
use crate::ui::modals::vault_mutation_preview::VaultMutationPreviewModal;

use super::support::{install_pending, MutationFixture};

#[test]
fn vault_mutation_execute_confirm_runs_disk_and_chat_once() {
    let fixture = MutationFixture::new("exact-once");
    super::super::tests::with_test_app_config(
        "vault-mutation-execute-exact-once",
        fixture.config(),
        |app| {
            install_pending(app, &fixture, fixture.move_kind());
            let runtime = tokio::runtime::Runtime::new().expect("runtime");

            runtime
                .block_on(app.confirm_pending_vault_mutation())
                .expect("confirm mutation");

            assert!(!fixture.source_root.join("source.md").exists());
            assert_eq!(
                std::fs::read_to_string(fixture.destination_root.join("moved.md"))
                    .expect("moved note"),
                "source bytes"
            );
            let messages = app
                .storage
                .get_messages(app.ui.tabs[0].active_conversation)
                .expect("stored messages");
            assert_eq!(messages.len(), 1);
            assert_eq!(messages[0].role, "user");
            assert!(app.ui.pending_vault_mutation.is_none());
        },
    );
}

#[test]
fn vault_mutation_execute_completed_chat_retry_never_replays_disk() {
    let fixture = MutationFixture::new("chat-retry");
    super::super::tests::with_test_app_config(
        "vault-mutation-execute-chat-retry",
        fixture.config(),
        |app| {
            install_pending(app, &fixture, fixture.move_kind());
            super::super::vault_mutation_execute::inject_chat_persistence_failure_once();
            let runtime = tokio::runtime::Runtime::new().expect("runtime");

            let first = runtime.block_on(app.confirm_pending_vault_mutation());

            assert!(first.is_err());
            assert!(!fixture.source_root.join("source.md").exists());
            assert!(fixture.destination_root.join("moved.md").exists());
            assert!(matches!(
                app.ui
                    .pending_vault_mutation
                    .as_ref()
                    .map(|pending| &pending.state),
                Some(VaultMutationConfirmationState::CompletedAwaitingChat { .. })
            ));
            runtime
                .block_on(app.confirm_pending_vault_mutation())
                .expect("retry chat only");
            assert_eq!(
                app.storage
                    .get_messages(app.ui.tabs[0].active_conversation)
                    .expect("stored messages")
                    .len(),
                1
            );
            assert_eq!(
                std::fs::read_to_string(fixture.destination_root.join("moved.md"))
                    .expect("same moved note"),
                "source bytes"
            );
        },
    );
}

#[test]
fn vault_mutation_execute_blocks_write_restore_and_move_on_live_editor_paths() {
    let fixture = MutationFixture::new("editor-block");
    super::super::tests::with_test_app_config(
        "vault-mutation-execute-editor-block",
        fixture.config(),
        |app| {
            let launcher = app.ui.app_tabs.add_launcher();
            app.ui
                .markdown_vault_workspaces
                .activate(
                    &mut app.ui.app_tabs,
                    &fixture.catalog,
                    launcher,
                    &fixture.source_id,
                )
                .expect("activate source workspace");
            let tab_id = app.ui.app_tabs.active_id();
            assert!(matches!(
                app.ui.app_tabs.tab(tab_id).map(|tab| tab.view()),
                Some(AppView::MarkdownVault { .. })
            ));
            let (editor, _probe) =
                crate::ui::modals::terminal_editor::terminal_editor_test_support::live_editor();
            let workspace = app
                .ui
                .markdown_vault_workspaces
                .get_mut_for_test(tab_id)
                .expect("source workspace");
            workspace.set_editor_for_test(editor);
            workspace.set_editor_path_for_test(Path::new("source.md").to_path_buf());
            let data_root = crate::storage::paths::TcuiDataPaths::discover().root;
            let backup_id = fixture.backup_source(&data_root);
            let cases = [
                fixture.write_kind(),
                fixture.restore_kind(&data_root, backup_id),
                fixture.move_kind(),
            ];
            let runtime = tokio::runtime::Runtime::new().expect("runtime");

            for kind in cases {
                install_pending(app, &fixture, kind);
                let result = runtime.block_on(app.confirm_pending_vault_mutation());
                assert!(result.is_err());
                assert!(matches!(
                    app.ui
                        .pending_vault_mutation
                        .as_ref()
                        .map(|pending| &pending.state),
                    Some(VaultMutationConfirmationState::ExecutionFailed { .. })
                ));
                app.cancel_pending_vault_mutation()
                    .expect("cancel blocked mutation");
            }
            assert!(fixture.source_root.join("source.md").exists());
            assert!(!fixture.destination_root.join("moved.md").exists());
        },
    );
}

#[test]
fn vault_mutation_execute_refreshes_every_affected_open_workspace() {
    let fixture = MutationFixture::new("workspace-refresh");
    super::super::tests::with_test_app_config(
        "vault-mutation-execute-workspace-refresh",
        fixture.config(),
        |app| {
            let source_launcher = app.ui.app_tabs.add_launcher();
            app.ui
                .markdown_vault_workspaces
                .activate(
                    &mut app.ui.app_tabs,
                    &fixture.catalog,
                    source_launcher,
                    &fixture.source_id,
                )
                .expect("activate source workspace");
            let destination_launcher = app.ui.app_tabs.add_launcher();
            app.ui
                .markdown_vault_workspaces
                .activate(
                    &mut app.ui.app_tabs,
                    &fixture.catalog,
                    destination_launcher,
                    &fixture.destination_id,
                )
                .expect("activate destination workspace");
            install_pending(app, &fixture, fixture.move_kind());
            let runtime = tokio::runtime::Runtime::new().expect("runtime");

            runtime
                .block_on(app.confirm_pending_vault_mutation())
                .expect("confirm move");

            let source = app
                .ui
                .markdown_vault_workspaces
                .get(source_launcher)
                .expect("source workspace");
            let destination = app
                .ui
                .markdown_vault_workspaces
                .get(destination_launcher)
                .expect("destination workspace");
            assert!(!source
                .tree
                .nodes()
                .iter()
                .any(|node| node.path == Path::new("source.md")));
            assert!(destination
                .tree
                .nodes()
                .iter()
                .any(|node| node.path == Path::new("moved.md")));
        },
    );
}

#[test]
fn vault_mutation_execute_recovery_status_renders_explicit_command() {
    let fixture = MutationFixture::new("recovery-render");
    let submission = InputSubmission::new(
        "@vault move Work:source.md to Archive:moved.md",
        PendingDocuments::default(),
    )
    .expect("submission");
    let mut pending = PendingVaultMutation::new(
        PreparedUserTurn::prepare(&submission),
        fixture.move_kind(),
        &fixture.catalog,
    )
    .expect("pending mutation");
    pending.state = VaultMutationConfirmationState::RecoveryRequired {
        operation_id: "move-0123456789abcdef0123456789abcdef".to_string(),
        recovery_status:
            "Recovery required; run @vault restore move-0123456789abcdef0123456789abcdef"
                .to_string(),
    };
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).expect("terminal");

    terminal
        .draw(|frame| {
            VaultMutationPreviewModal::new(&pending).render(frame, frame.area());
        })
        .expect("render recovery status");

    let rendered = terminal.backend().to_string();
    println!("{rendered}");
    assert!(rendered
        .contains("Recovery required; run @vault restore move-0123456789abcdef0123456789abcdef"));
    assert!(rendered.contains("[Cancel]"));
    assert!(!rendered.contains("[Retry Chat]"));
}
