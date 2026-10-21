use std::path::{Path, PathBuf};

use crate::app::submission::{
    InputSubmission, PendingDocuments, PendingVaultMutation, PreparedUserTurn, SubmissionState,
};
use crate::config::vault_profiles::{VaultCatalog, VaultProfile, VaultProfileId};
use crate::config::AppConfig;
use crate::vault::move_plan::{MovePair, MovePlan, VaultFileRef};
use crate::vault::mutation_plan::{VaultMutationKind, VaultRestorePlan, VaultWritePlan};
use crate::vault::{StandaloneBackupId, VaultRelativePath, VaultWriteService};

pub(super) struct MutationFixture {
    root: PathBuf,
    pub(super) source_root: PathBuf,
    pub(super) destination_root: PathBuf,
    pub(super) source_id: VaultProfileId,
    pub(super) destination_id: VaultProfileId,
    pub(super) catalog: VaultCatalog,
}

impl MutationFixture {
    pub(super) fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "tcui-vault-mutation-execute-{label}-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let source_root = root.join("source");
        let destination_root = root.join("destination");
        std::fs::create_dir_all(&source_root).expect("create source vault");
        std::fs::create_dir_all(&destination_root).expect("create destination vault");
        std::fs::write(source_root.join("source.md"), "source bytes").expect("write source note");
        let source = VaultProfile::new("Work", &source_root).expect("source profile");
        let destination =
            VaultProfile::new("Archive", &destination_root).expect("destination profile");
        let source_id = source.id.clone();
        let destination_id = destination.id.clone();
        let mut catalog = VaultCatalog::default();
        catalog.add(source).expect("add source profile");
        catalog.add(destination).expect("add destination profile");
        Self {
            root,
            source_root,
            destination_root,
            source_id,
            destination_id,
            catalog,
        }
    }

    pub(super) fn config(&self) -> AppConfig {
        AppConfig {
            providers: vec![crate::config::ProviderConfig {
                name: "Ollama".to_string(),
                endpoint: "http://127.0.0.1:9".to_string(),
                env_var: String::new(),
                backend_type: "openai".to_string(),
                auth_type: "api_key".to_string(),
            }],
            vaults: self.catalog.profiles.clone(),
            default_vault_id: Some(self.source_id.clone()),
            ..AppConfig::default()
        }
    }

    pub(super) fn path(raw: &str) -> VaultRelativePath {
        VaultRelativePath::parse(Path::new(raw)).expect("valid relative path")
    }

    pub(super) fn move_kind(&self) -> VaultMutationKind {
        let pair = MovePair::new(
            VaultFileRef::new(self.source_id.clone(), Self::path("source.md")),
            VaultFileRef::new(self.destination_id.clone(), Self::path("moved.md")),
            false,
        );
        VaultMutationKind::Move(
            MovePlan::build(vec![pair], &self.catalog, |_| false).expect("move plan"),
        )
    }

    pub(super) fn write_kind(&self) -> VaultMutationKind {
        VaultMutationKind::Write(
            VaultWritePlan::build(
                &self.catalog,
                self.source_id.clone(),
                Self::path("source.md"),
                "replacement".to_string(),
                true,
            )
            .expect("write plan"),
        )
    }

    pub(super) fn restore_kind(
        &self,
        data_root: &Path,
        backup_id: StandaloneBackupId,
    ) -> VaultMutationKind {
        VaultMutationKind::Restore(
            VaultRestorePlan::build(
                &self.catalog,
                data_root,
                self.source_id.clone(),
                Self::path("source.md"),
                backup_id,
            )
            .expect("restore plan"),
        )
    }

    pub(super) fn backup_source(&self, data_root: &Path) -> StandaloneBackupId {
        let registrations = self.catalog.profiles.values().map(|profile| {
            crate::vault::VaultProfileRegistration::new(profile.id.clone(), profile.root.clone())
        });
        let service =
            VaultWriteService::at_data_root(registrations, data_root).expect("open write service");
        service
            .write(&self.source_id, &Self::path("source.md"), b"intermediate")
            .expect("create source backup")
            .backup_id
    }
}

impl Drop for MutationFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

pub(super) fn install_pending(
    app: &mut super::super::TuiApp,
    fixture: &MutationFixture,
    kind: VaultMutationKind,
) {
    app.ui.tabs[0].tab.provider = "Ollama".to_string();
    app.ui.tabs[0].tab.endpoint = Some("http://127.0.0.1:9".to_string());
    app.ui.tabs[0].tab.model = "vault-mutation-test".to_string();
    let submission = InputSubmission::new(
        "@vault move Work:source.md to Archive:moved.md",
        PendingDocuments::default(),
    );
    let prepared = PreparedUserTurn::prepare(&submission).expect("prepare turn");
    let pending =
        PendingVaultMutation::new(prepared, kind, &fixture.catalog).expect("pending mutation");
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
