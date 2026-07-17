use std::path::Path;

use crate::app::submission::{
    InputSubmission, PendingDocuments, PendingVaultMutation, PreparedUserTurn, SubmissionState,
    VaultMutationConfirmationState,
};
use crate::config::vault_profiles::{VaultCatalog, VaultProfile};
use crate::vault::move_plan::{MovePair, MovePlan, VaultFileRef};
use crate::vault::mutation_plan::VaultMutationKind;
use crate::vault::VaultRelativePath;

use super::preview_ui::VaultMutationFixture;

#[test]
fn vault_mutation_preview_cancel_restores_exact_draft_without_side_effects() {
    super::super::tests::with_test_app("vault-mutation-preview-cancel", |app| {
        let fixture = VaultMutationFixture::new("cancel");
        let document = crate::app::submission::PendingDocument::snapshot(
            "fixture",
            "context.md",
            b"request-only context",
        )
        .expect("snapshot document");
        let mut documents = PendingDocuments::default();
        documents.insert(document).expect("insert document");
        let submission = InputSubmission::new("@vault move source.md to Archive", documents);
        let prepared = PreparedUserTurn::prepare(&submission).expect("prepare turn");
        let pending = PendingVaultMutation::new(
            prepared,
            VaultMutationKind::Move(fixture.create_plan(false)),
            &fixture.catalog,
        )
        .expect("pending preview");
        let tab_id = app.ui.app_tabs.active_id();
        let conversation_id = app.ui.tabs[0].active_conversation;
        let history_before = app
            .storage
            .get_messages(conversation_id)
            .expect("initial history");
        let source_before =
            std::fs::read(fixture.source_root.join("source.md")).expect("read source sentinel");
        app.submission_states.insert(
            tab_id,
            SubmissionState::draft(submission)
                .begin_preparing()
                .and_then(SubmissionState::await_confirmation)
                .expect("await confirmation"),
        );
        app.ui.pending_vault_mutation = Some(pending);

        app.cancel_pending_vault_mutation()
            .expect("cancel pending mutation");

        assert!(app.ui.pending_vault_mutation.is_none());
        assert!(matches!(
            app.submission_state(),
            Some(SubmissionState::Draft(_))
        ));
        assert_eq!(
            app.ui.tabs[0].input_content,
            "@vault move source.md to Archive"
        );
        let history_after = app
            .storage
            .get_messages(conversation_id)
            .expect("history after cancel");
        assert_eq!(history_after.len(), history_before.len());
        assert_eq!(
            std::fs::read(fixture.source_root.join("source.md")).expect("source after cancel"),
            source_before
        );
        assert!(matches!(
            app.action_rx.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));
    });
}

#[test]
fn vault_mutation_preview_stale_confirm_keeps_submission_unconsumed() {
    let fixture = VaultMutationFixture::new("stale");
    let submission = InputSubmission::new(
        "@vault move source.md to Archive",
        PendingDocuments::default(),
    );
    let mut pending = PendingVaultMutation::new(
        PreparedUserTurn::prepare(&submission).expect("prepare turn"),
        VaultMutationKind::Move(fixture.create_plan(false)),
        &fixture.catalog,
    )
    .expect("pending preview");
    std::fs::write(
        fixture.source_root.join("source.md"),
        "changed after preview",
    )
    .expect("change source");

    let result = pending.confirm(&fixture.catalog, fixture.data_root.as_path());

    assert!(result.is_err());
    assert!(matches!(
        pending.state,
        VaultMutationConfirmationState::Reviewing { error: Some(_) }
    ));
    assert_eq!(
        pending.submission.instruction(),
        "@vault move source.md to Archive"
    );
}

#[test]
fn vault_mutation_preview_valid_confirm_is_noncancellable_t12c_handoff() {
    let fixture = VaultMutationFixture::new("confirm");
    let submission = InputSubmission::new(
        "@vault move source.md to Archive",
        PendingDocuments::default(),
    );
    let mut pending = PendingVaultMutation::new(
        PreparedUserTurn::prepare(&submission).expect("prepare turn"),
        VaultMutationKind::Move(fixture.create_plan(false)),
        &fixture.catalog,
    )
    .expect("pending preview");

    pending
        .confirm(&fixture.catalog, fixture.data_root.as_path())
        .expect("revalidate preview");

    assert_eq!(
        pending.state,
        VaultMutationConfirmationState::ReadyForExecution
    );
    assert!(!pending.can_cancel());
    assert!(fixture.source_root.join("source.md").exists());
    assert!(!fixture.destination_root.join("moved.md").exists());
}

#[allow(dead_code)]
fn _type_check_helpers(
    _catalog: VaultCatalog,
    _profile: VaultProfile,
    _pair: MovePair,
    _plan: MovePlan,
    _file: VaultFileRef,
    _path: VaultRelativePath,
) {
    let _ = Path::new("unused");
}
