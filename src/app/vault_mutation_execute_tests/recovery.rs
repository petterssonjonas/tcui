use crate::app::submission::VaultMutationConfirmationState;
use crate::vault::move_execute::{
    inject_failure as inject_move_failure, TestFailurePoint as MoveFailurePoint,
};
use crate::vault::move_rollback::inject_failure as inject_recovery_failure;
use crate::vault::move_rollback::RecoveryFailurePoint;
use crate::vault::VaultMutationLock;

use super::support::{install_pending, MutationFixture};

#[test]
fn vault_mutation_execute_rollback_reports_failure_without_partial_success() {
    let fixture = MutationFixture::new("rollback");
    super::super::tests::with_test_app_config(
        "vault-mutation-execute-rollback",
        fixture.config(),
        |app| {
            install_pending(app, &fixture, fixture.move_kind());
            inject_move_failure(MoveFailurePoint::DestinationDirectorySync);
            let runtime = tokio::runtime::Runtime::new().expect("runtime");

            let result = runtime.block_on(app.confirm_pending_vault_mutation());

            assert!(result.is_err());
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
                .expect("stored messages")
                .is_empty());
        },
    );
}

#[test]
fn vault_mutation_execute_recovery_required_retains_operation_and_blocks_mutations() {
    let fixture = MutationFixture::new("recovery-required");
    super::super::tests::with_test_app_config(
        "vault-mutation-execute-recovery-required",
        fixture.config(),
        |app| {
            install_pending(app, &fixture, fixture.move_kind());
            inject_move_failure(MoveFailurePoint::SourceDirectorySync);
            inject_recovery_failure(RecoveryFailurePoint::RestoreSource);
            let runtime = tokio::runtime::Runtime::new().expect("runtime");

            let result = runtime.block_on(app.confirm_pending_vault_mutation());

            assert!(result.is_err());
            let pending = app
                .ui
                .pending_vault_mutation
                .as_ref()
                .expect("pending status");
            assert!(matches!(
                pending.state,
                VaultMutationConfirmationState::RecoveryRequired { .. }
            ));
            assert!(format!("{pending:?}").contains("@vault restore"));
            install_pending(app, &fixture, fixture.write_kind());
            assert!(runtime
                .block_on(app.confirm_pending_vault_mutation())
                .is_err());
        },
    );
}

#[test]
fn vault_mutation_execute_lock_contention_is_retryable_without_disk_work() {
    let fixture = MutationFixture::new("contention");
    super::super::tests::with_test_app_config(
        "vault-mutation-execute-contention",
        fixture.config(),
        |app| {
            install_pending(app, &fixture, fixture.move_kind());
            let data_root = crate::storage::paths::TcuiDataPaths::discover().root;
            let guard = VaultMutationLock::at_data_root(&data_root)
                .try_acquire()
                .expect("hold global mutation lock");
            let runtime = tokio::runtime::Runtime::new().expect("runtime");

            let first = runtime.block_on(app.confirm_pending_vault_mutation());

            assert!(first.is_err());
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
    let fixture = MutationFixture::new("stale-refresh");
    let changes = super::super::vault_mutation_execute::affected_paths(&fixture.move_kind());
    let mut tabs = crate::ui::app_tabs::AppTabs::default();
    let launcher = tabs.add_launcher();
    let mut workspaces =
        crate::ui::markdown_vault_workspace::MarkdownVaultWorkspaceRegistry::default();
    workspaces
        .activate(
            &mut tabs,
            &fixture.catalog,
            launcher,
            &fixture.destination_id,
        )
        .expect("activate destination workspace");
    let targets = workspaces
        .mutation_targets(&changes)
        .expect("capture stable targets");
    let tab_id = tabs.active_id();
    let _ = workspaces.close(&mut tabs, tab_id);

    let report = workspaces.refresh_mutation_targets(&targets);

    assert_eq!(report.refreshed, 0);
    assert_eq!(report.stale, 1);
    let debug = format!("{changes:?}");
    assert!(!debug.contains(&fixture.source_root.display().to_string()));
    assert!(!debug.contains(&fixture.destination_root.display().to_string()));
    assert!(!debug.contains("source bytes"));
}
