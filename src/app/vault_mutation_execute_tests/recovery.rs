use crate::app::submission::VaultMutationConfirmationState;
use crate::vault::move_execute::{
    inject_failure as inject_move_failure, TestFailurePoint as MoveFailurePoint,
};
use crate::vault::move_rollback::{
    inject_failure as inject_recovery_failure, RecoveryFailurePoint,
};
use crate::vault::VaultMutationLock;

use super::support::{install_pending, MutationFixture};

#[test]
fn vault_mutation_execute_rollback_reports_failure_without_partial_success() {
    super::super::tests::with_test_app_config(
        "vault-mutation-rollback",
        MutationFixture::config(),
        |app| {
            let fixture = MutationFixture::new();
            install_pending(app, fixture.move_kind());
            inject_move_failure(MoveFailurePoint::DestinationDirectorySync);
            let runtime = tokio::runtime::Runtime::new().expect("runtime");
            assert!(runtime
                .block_on(app.confirm_pending_vault_mutation())
                .is_err());
            assert!(fixture.source_root.join("source.md").exists());
            assert!(!fixture.destination_root.join("moved.md").exists());
            assert!(matches!(
                app.ui
                    .pending_vault_mutation
                    .as_ref()
                    .map(|pending| &pending.state),
                Some(VaultMutationConfirmationState::ExecutionFailed { .. })
            ));
            assert!(app
                .storage
                .get_messages(app.ui.tabs[0].active_conversation)
                .expect("messages")
                .is_empty());
        },
    );
}

#[test]
fn vault_mutation_execute_recovery_required_retains_operation_and_blocks_mutations() {
    super::super::tests::with_test_app_config(
        "vault-mutation-recovery-required",
        MutationFixture::config(),
        |app| {
            let fixture = MutationFixture::new();
            install_pending(app, fixture.move_kind());
            inject_move_failure(MoveFailurePoint::SourceDirectorySync);
            inject_recovery_failure(RecoveryFailurePoint::RestoreSource);
            let runtime = tokio::runtime::Runtime::new().expect("runtime");
            assert!(runtime
                .block_on(app.confirm_pending_vault_mutation())
                .is_err());
            let pending = app
                .ui
                .pending_vault_mutation
                .as_ref()
                .expect("pending status");
            assert!(matches!(
                pending.state,
                VaultMutationConfirmationState::RecoveryRequired { .. }
            ));
            assert!(format!("{pending:?}").contains("Recovery required"));
            install_pending(app, fixture.write_kind());
            assert!(runtime
                .block_on(app.confirm_pending_vault_mutation())
                .is_err());
        },
    );
}

#[test]
fn vault_mutation_execute_lock_contention_is_retryable_without_disk_work() {
    super::super::tests::with_test_app_config(
        "vault-mutation-contention",
        MutationFixture::config(),
        |app| {
            let fixture = MutationFixture::new();
            install_pending(app, fixture.move_kind());
            let data_root = crate::storage::paths::TcuiDataPaths::discover().root;
            let guard = VaultMutationLock::at_data_root(&data_root)
                .try_acquire()
                .expect("hold mutation lock");
            let runtime = tokio::runtime::Runtime::new().expect("runtime");
            assert!(runtime
                .block_on(app.confirm_pending_vault_mutation())
                .is_err());
            assert!(fixture.source_root.join("source.md").exists());
            assert!(!fixture.destination_root.join("moved.md").exists());
            drop(guard);
            runtime
                .block_on(app.confirm_pending_vault_mutation())
                .expect("retry after lock release");
            assert!(fixture.destination_root.join("moved.md").exists());
        },
    );
}

#[test]
fn vault_mutation_execute_stale_workspace_refresh_is_ignored_and_debug_is_redacted() {
    super::super::tests::with_test_app_config(
        "vault-mutation-stale-refresh",
        MutationFixture::config(),
        |app| {
            let fixture = MutationFixture::new();
            let changes =
                super::super::vault_mutation_execute::affected_paths(&fixture.move_kind());
            let launcher = app.ui.app_tabs.add_launcher();
            app.ui
                .vault_workspaces
                .activate(&mut app.ui.app_tabs, launcher)
                .expect("activate workspace");
            let targets = app
                .ui
                .vault_workspaces
                .mutation_targets(&changes)
                .expect("stable targets");
            let tab_id = app.ui.app_tabs.active_id();
            let _ = app.ui.vault_workspaces.close(&mut app.ui.app_tabs, tab_id);
            let report = app.ui.vault_workspaces.refresh_mutation_targets(&targets);
            assert_eq!(report.refreshed, 0);
            assert_eq!(report.stale, 1);
            let debug = format!("{changes:?}");
            assert!(!debug.contains(&fixture.source_root.display().to_string()));
            assert!(!debug.contains("source bytes"));
        },
    );
}
