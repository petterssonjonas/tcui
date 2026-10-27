use std::path::{Path, PathBuf};

use crate::app::submission::{
    InputSubmission, PendingDocuments, PendingVaultMutation, PreparedUserTurn, SubmissionState,
};
use crate::config::AppConfig;
use crate::vault::move_plan::{MovePair, MovePlan, VaultFileRef};
use crate::vault::mutation_plan::{VaultMutationKind, VaultRestorePlan, VaultWritePlan};
use crate::vault::{StandaloneBackupId, VaultRelativePath, VaultWriteService};

pub(super) struct MutationFixture {
    pub(super) source_root: PathBuf,
    pub(super) destination_root: PathBuf,
    data_root: PathBuf,
}

impl MutationFixture {
    // The caller owns the isolated environment through with_test_app_config.
    pub(super) fn new() -> Self {
        let data_root = crate::storage::paths::TcuiDataPaths::discover().root;
        let source_root = data_root.join("vault");
        std::fs::create_dir_all(&source_root).expect("create native Vault");
        crate::protection::write(
            crate::protection::Domain::Vault,
            &source_root.join("source.md"),
            b"source bytes",
        )
        .expect("write source note");
        Self {
            destination_root: source_root.clone(),
            source_root,
            data_root,
        }
    }

    pub(super) fn config() -> AppConfig {
        AppConfig {
            providers: vec![crate::config::ProviderConfig {
                name: "Ollama".to_string(),
                endpoint: "http://127.0.0.1:9".to_string(),
                env_var: String::new(),
                backend_type: "openai".to_string(),
                auth_type: "api_key".to_string(),
            }],
            ..AppConfig::default()
        }
    }

    pub(super) fn path(raw: &str) -> VaultRelativePath {
        VaultRelativePath::parse(Path::new(raw)).expect("valid relative path")
    }

    pub(super) fn move_kind(&self) -> VaultMutationKind {
        let pair = MovePair::new(
            VaultFileRef::new(Self::path("source.md")),
            VaultFileRef::new(Self::path("moved.md")),
            false,
        );
        VaultMutationKind::Move(
            MovePlan::build(vec![pair], &self.data_root, |_| false).expect("move plan"),
        )
    }

    pub(super) fn write_kind(&self) -> VaultMutationKind {
        VaultMutationKind::Write(
            VaultWritePlan::build(
                &self.data_root,
                Self::path("source.md"),
                "replacement".to_string(),
                true,
            )
            .expect("write plan"),
        )
    }

    pub(super) fn restore_kind(&self, backup_id: StandaloneBackupId) -> VaultMutationKind {
        VaultMutationKind::Restore(
            VaultRestorePlan::build(&self.data_root, Self::path("source.md"), backup_id)
                .expect("restore plan"),
        )
    }

    pub(super) fn backup_source(&self) -> StandaloneBackupId {
        VaultWriteService::at_data_root(&self.data_root)
            .expect("write service")
            .write(&Self::path("source.md"), b"intermediate")
            .expect("create source backup")
            .backup_id
    }
}

pub(super) fn install_pending(app: &mut super::super::TuiApp, kind: VaultMutationKind) {
    app.ui.tabs[0].tab.provider = "Ollama".to_string();
    app.ui.tabs[0].tab.endpoint = Some("http://127.0.0.1:9".to_string());
    app.ui.tabs[0].tab.model = "vault-mutation-test".to_string();
    let submission = InputSubmission::new(
        "@vault move source.md to moved.md",
        PendingDocuments::default(),
    );
    let prepared = PreparedUserTurn::prepare(&submission).expect("prepare turn");
    let pending = PendingVaultMutation::new(prepared, kind).expect("pending mutation");
    let tab_id = app.ui.app_tabs.active_id();
    app.submission_states.insert(
        tab_id,
        SubmissionState::draft(submission)
            .begin_preparing()
            .and_then(SubmissionState::await_confirmation)
            .expect("await confirmation"),
    );
    app.ui.pending_vault_mutation = Some(pending);
}
