use std::path::Path;

use ratatui::{backend::TestBackend, Terminal};

use crate::app::submission::VaultMutationConfirmationState;
use crate::ui::modals::vault_mutation_preview::VaultMutationPreviewModal;

use super::support::{install_pending, MutationFixture};

#[test]
fn vault_mutation_execute_confirm_runs_disk_and_chat_once() {
    super::super::tests::with_test_app_config(
        "vault-mutation-execute-once",
        MutationFixture::config(),
        |app| {
            let fixture = MutationFixture::new();
            install_pending(app, fixture.move_kind());
            let runtime = tokio::runtime::Runtime::new().expect("runtime");
            runtime
                .block_on(app.confirm_pending_vault_mutation())
                .expect("confirm mutation");
            assert!(!fixture.source_root.join("source.md").exists());
            assert_eq!(
                std::fs::read(fixture.destination_root.join("moved.md")).expect("moved note"),
                b"source bytes"
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
    super::super::tests::with_test_app_config(
        "vault-mutation-chat-retry",
        MutationFixture::config(),
        |app| {
            let fixture = MutationFixture::new();
            install_pending(app, fixture.move_kind());
            super::super::vault_mutation_execute::inject_chat_persistence_failure_once();
            let runtime = tokio::runtime::Runtime::new().expect("runtime");
            assert!(runtime
                .block_on(app.confirm_pending_vault_mutation())
                .is_err());
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
                    .expect("messages")
                    .len(),
                1
            );
            assert_eq!(
                std::fs::read(fixture.destination_root.join("moved.md")).expect("same moved note"),
                b"source bytes"
            );
        },
    );
}

#[test]
fn vault_mutation_execute_blocks_write_restore_and_move_on_live_editor_paths() {
    super::super::tests::with_test_app_config(
        "vault-mutation-editor-block",
        MutationFixture::config(),
        |app| {
            let fixture = MutationFixture::new();
            let launcher = app.ui.app_tabs.add_launcher();
            app.ui
                .vault_workspaces
                .activate(&mut app.ui.app_tabs, launcher)
                .expect("activate native workspace");
            let tab_id = app.ui.app_tabs.active_id();
            let (editor, _probe) =
                crate::ui::modals::terminal_editor::terminal_editor_test_support::live_editor();
            let workspace = app
                .ui
                .vault_workspaces
                .get_mut_for_test(tab_id)
                .expect("workspace");
            workspace.set_editor_for_test(editor);
            workspace.set_editor_path_for_test(Path::new("source.md").to_path_buf());
            let backup_id = fixture.backup_source();
            let cases = [
                fixture.write_kind(),
                fixture.restore_kind(backup_id),
                fixture.move_kind(),
            ];
            let runtime = tokio::runtime::Runtime::new().expect("runtime");
            for kind in cases {
                install_pending(app, kind);
                assert!(runtime
                    .block_on(app.confirm_pending_vault_mutation())
                    .is_err());
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
fn vault_mutation_execute_refreshes_native_workspace() {
    super::super::tests::with_test_app_config(
        "vault-mutation-workspace-refresh",
        MutationFixture::config(),
        |app| {
            let fixture = MutationFixture::new();
            let launcher = app.ui.app_tabs.add_launcher();
            app.ui
                .vault_workspaces
                .activate(&mut app.ui.app_tabs, launcher)
                .expect("activate workspace");
            install_pending(app, fixture.move_kind());
            let runtime = tokio::runtime::Runtime::new().expect("runtime");
            runtime
                .block_on(app.confirm_pending_vault_mutation())
                .expect("confirm move");
            let workspace = app
                .ui
                .vault_workspaces
                .get(launcher)
                .expect("native workspace");
            assert!(!workspace
                .tree
                .nodes()
                .iter()
                .any(|node| node.path == Path::new("source.md")));
            assert!(workspace
                .tree
                .nodes()
                .iter()
                .any(|node| node.path == Path::new("moved.md")));
        },
    );
}

#[test]
fn vault_mutation_execute_recovery_status_renders_restart_guidance() {
    super::super::tests::with_test_app_config(
        "vault-mutation-recovery-render",
        MutationFixture::config(),
        |app| {
            let fixture = MutationFixture::new();
            install_pending(app, fixture.move_kind());
            let pending = app
                .ui
                .pending_vault_mutation
                .as_mut()
                .expect("pending mutation");
            pending.state = VaultMutationConfirmationState::RecoveryRequired {
                operation_id: "move-0123456789abcdef0123456789abcdef".to_string(),
                recovery_status:
                    "Recovery required for move-0123456789abcdef0123456789abcdef; restart TCUI"
                        .to_string(),
            };
            let mut terminal = Terminal::new(TestBackend::new(100, 24)).expect("terminal");
            terminal
                .draw(|frame| {
                    VaultMutationPreviewModal::new(pending).render(frame, frame.area());
                })
                .expect("render status");
            let rendered = terminal.backend().to_string();
            assert!(rendered.contains("restart TCUI"));
            assert!(rendered.contains("[Cancel]"));
            assert!(!rendered.contains("[Retry Chat]"));
        },
    );
}
