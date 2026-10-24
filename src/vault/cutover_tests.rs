use super::*;
use crate::storage::paths::TcuiDataPaths;
use crate::vault::move_execute::{inject_interruption, TestFailurePoint};
use crate::vault::move_journal::MoveJournalState;
use crate::vault::move_plan::MovePlanError;
use crate::vault::move_recovery::{MoveRecoveryConfig, MoveRecoveryService};
use crate::vault::mutation_plan::{VaultMutationKind, VaultRestorePlan};

fn config() -> crate::config::AppConfig {
    crate::config::AppConfig::load().expect("isolated configuration")
}

fn catalog(config: &crate::config::AppConfig) -> VaultCatalog {
    VaultCatalog {
        profiles: config.vaults.clone(),
        default_id: config.default_vault_id.clone(),
    }
}

fn profile(name: &str, root: &Path) -> VaultProfile {
    std::fs::create_dir_all(root).expect("profile root");
    VaultProfile::new(name, root).expect("registered profile")
}

#[test]
fn unavailable_legacy_root_never_prunes_links_or_allows_saved_mutations_until_remounted() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let environment = Environment::new();
    let mut config = config();
    let paths = TcuiDataPaths::discover();
    let legacy = environment.root.join("mounted-saved");
    let old = ChatSavedFileStore::new(legacy.clone(), paths.clone());
    let link = old
        .create_for_chat(19, "existing.md", b"linked legacy bytes")
        .expect("legacy link");
    let before = std::fs::read(&paths.chat_file_links_manifest).expect("original links");
    let disconnected = environment.root.join("disconnected-mount");
    std::fs::rename(&legacy, &disconnected).expect("unmount fixture");
    config.saved_files_dir = Some(legacy.display().to_string());
    assert!(crate::storage::saved_file_migration::migrate_saved_files(&config).is_err());
    assert!(!paths
        .migrations_dir
        .join("saved-internal-vault-v1.done")
        .exists());
    let current = ChatSavedFileStore::discover();
    assert!(current.list_for_conversation(19).is_err());
    assert!(current.create_saved("new.md", b"must not create").is_err());
    assert!(current.remove_conversation_links(19).is_err());
    assert_eq!(
        std::fs::read(&paths.chat_file_links_manifest).expect("links not pruned"),
        before
    );
    std::fs::rename(&disconnected, &legacy).expect("remount fixture");
    crate::storage::saved_file_migration::migrate_saved_files(&config).expect("resume relocation");
    assert_eq!(
        current
            .open_link(19, link.id)
            .expect("association survives"),
        b"linked legacy bytes"
    );
    assert_eq!(
        current.list_for_conversation(19).expect("same link"),
        vec![link]
    );
    assert_eq!(
        std::fs::read(legacy.join("existing.md")).expect("source retained"),
        b"linked legacy bytes"
    );
}

#[test]
fn nested_profile_aliases_and_hard_links_are_rejected_during_planning_and_revalidation() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let environment = Environment::new();
    let _config = config();
    let parent = profile("Parent", &environment.root.join("notes"));
    let child = profile("Child", &parent.root.join("sub"));
    let other = profile("Other", &parent.root.join("other"));
    std::fs::write(child.root.join("report.md"), b"original").expect("source");
    std::fs::write(parent.root.join("first.md"), b"first").expect("first source");
    std::fs::write(parent.root.join("second.md"), b"second").expect("second source");
    std::fs::write(other.root.join("report.md"), b"destination").expect("existing destination");
    let mut catalog = VaultCatalog {
        profiles: [parent.clone(), child.clone(), other.clone()]
            .into_iter()
            .map(|profile| (profile.id.clone(), profile))
            .collect(),
        default_id: Some(parent.id.clone()),
    };
    let pair = |source: &VaultProfile,
                source_path: &str,
                destination: &VaultProfile,
                destination_path: &str| {
        MovePair::new(
            VaultFileRef::new(source.id.clone(), relative(source_path)),
            VaultFileRef::new(destination.id.clone(), relative(destination_path)),
            true,
        )
    };
    assert!(matches!(
        MovePlan::build(
            vec![pair(&parent, "sub/report.md", &child, "report.md")],
            &catalog,
            |_| false
        ),
        Err(MovePlanError::OverlappingSourceDestination { .. })
    ));
    assert!(matches!(
        MovePlan::build(
            vec![
                pair(&parent, "first.md", &parent, "sub/missing/new.md"),
                pair(&parent, "second.md", &child, "missing/new.md"),
            ],
            &catalog,
            |_| false
        ),
        Err(MovePlanError::DuplicateDestination { .. })
    ));
    std::fs::hard_link(child.root.join("report.md"), other.root.join("alias.md"))
        .expect("hard-link alias");
    assert!(matches!(
        MovePlan::build(
            vec![pair(&child, "report.md", &other, "alias.md")],
            &catalog,
            |_| false
        ),
        Err(MovePlanError::OverlappingSourceDestination { .. })
    ));
    let plan = MovePlan::build(
        vec![pair(&parent, "sub/report.md", &other, "report.md")],
        &catalog,
        |_| false,
    )
    .expect("initial distinct targets");
    catalog.profiles.get_mut(&other.id).expect("profile").root = child.root.clone();
    assert!(matches!(
        plan.revalidate(&catalog),
        Err(MovePlanError::OverlappingSourceDestination { .. })
    ));
    assert_eq!(
        std::fs::read(child.root.join("report.md")).expect("source unchanged"),
        b"original"
    );
}

#[test]
fn linked_saved_moves_are_rejected_but_authorized_agent_copies_preserve_sources_and_links() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let environment = Environment::new();
    let mut config = config();
    let external = profile("External", &environment.root.join("external"));
    config.vaults.insert(external.id.clone(), external.clone());
    config.save().expect("configuration");
    protection::configure(
        ProtectionPolicy {
            vault: true,
            ..ProtectionPolicy::default()
        },
        "copy-test-password",
    )
    .expect("protect vault");
    let saved = ChatSavedFileStore::discover();
    let payload = b"\x89PNG\0linked binary";
    let link = saved
        .create_for_chat(23, "linked.png", payload)
        .expect("linked saved file");
    let source_id = VaultProfileId::internal();
    let pair = MovePair::new(
        VaultFileRef::new(source_id.clone(), relative(&link.filename)),
        VaultFileRef::new(external.id.clone(), relative("copy.png")),
        false,
    );
    let catalog = catalog(&config);
    assert!(MovePlan::build(vec![pair.clone()], &catalog, |_| false).is_err());
    let copy =
        MovePlan::build_copy(vec![pair], &catalog, |_| false).expect("non-destructive copy plan");
    assert!(copy.copy_only());
    assert!(VaultMutationKind::Move(copy.clone())
        .preview(&catalog)
        .expect("copy preview")
        .rows
        .iter()
        .any(|row| row.contains("source documents")));
    MoveExecutor::new(&catalog)
        .execute(copy)
        .expect("copy across protection domains");
    assert_eq!(
        std::fs::read(external.root.join("copy.png")).expect("explicit external plaintext copy"),
        payload
    );
    assert_eq!(
        saved
            .open_link(23, link.id)
            .expect("linked original remains"),
        payload
    );
    assert_eq!(
        saved.list_for_conversation(23).expect("link IDs retained"),
        vec![link.clone()]
    );

    use crate::skill_runtime::vault_mutation::{VaultMutationAuthorization, VaultMutationTool};
    let authorization = VaultMutationAuthorization::capture("@vault copy linked.png to External")
        .expect("capture")
        .expect("copy authorization");
    assert!(authorization.permits(VaultMutationTool::Copy));
    assert!(!authorization.permits(VaultMutationTool::Move));
    assert!(VaultMutationAuthorization::capture("@vault write copy.md")
        .expect("filename is not a copy verb")
        .expect("write authorization")
        .permits(VaultMutationTool::Write));
    let plan: crate::skill_runtime::ToolPlan = serde_json::from_value(serde_json::json!({
        "tool": "copy_notes", "arguments": { "copies": [{
            "source_profile": catalog.profiles[&source_id].display_name, "source_path": link.filename,
            "destination_profile": external.display_name, "destination_path": "agent-copy.png"
        }] }
    }))
    .expect("planner copy selection");
    let kind = crate::skill_runtime::vault_mutation::build_kind(
        &config,
        source_id.clone(),
        authorization,
        &plan,
    )
    .expect("host-authorized copy");
    let VaultMutationKind::Move(plan) = kind else {
        panic!("expected shared transfer plan")
    };
    assert!(plan.copy_only());
    MoveExecutor::new(&catalog)
        .execute(plan)
        .expect("agent binary copy");
    let unauthorized: crate::skill_runtime::ToolPlan =
        serde_json::from_value(serde_json::json!({ "tool": "move_notes", "arguments": {} }))
            .expect("unauthorized planner selection");
    assert!(crate::skill_runtime::vault_mutation::build_kind(
        &config,
        source_id,
        authorization,
        &unauthorized
    )
    .is_err());
    assert_eq!(
        saved
            .open_link(23, link.id)
            .expect("agent copy retains source"),
        payload
    );
    std::fs::write(external.root.join("rollback.png"), b"prior destination")
        .expect("overwrite target");
    let copy = MovePlan::build_copy(
        vec![MovePair::new(
            VaultFileRef::new(VaultProfileId::internal(), relative(&link.filename)),
            VaultFileRef::new(external.id.clone(), relative("rollback.png")),
            true,
        )],
        &catalog,
        |_| false,
    )
    .expect("copy overwrite plan");
    inject_interruption(TestFailurePoint::DestinationDirectorySync);
    assert!(MoveExecutor::new(&catalog).execute(copy).is_err());
    let data = TcuiDataPaths::discover();
    let report =
        MoveRecoveryService::at_data_root(&catalog, &data.root, MoveRecoveryConfig::default())
            .recover_startup()
            .expect("copy recovery");
    assert!(!report.blocked());
    assert_eq!(
        std::fs::read(external.root.join("rollback.png")).expect("old destination restored"),
        b"prior destination"
    );
    assert_eq!(
        saved
            .open_link(23, link.id)
            .expect("copy rollback preserves linked source"),
        payload
    );
}

#[test]
fn protected_empty_and_truncated_private_stages_are_recoverable_without_decryption() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let _environment = Environment::new();
    let config = config();
    protection::configure(
        ProtectionPolicy {
            vault: true,
            ..ProtectionPolicy::default()
        },
        "stage-test-password",
    )
    .expect("protect vault");
    let catalog = catalog(&config);
    let root = protection::internal_vault_root();
    let data = TcuiDataPaths::discover();
    for (index, point) in [TestFailurePoint::StageWrite, TestFailurePoint::StageSync]
        .into_iter()
        .enumerate()
    {
        let source = format!("source-{index}.md");
        let destination = format!("destination-{index}.md");
        protection::write(Domain::Vault, &root.join(&source), b"source must survive")
            .expect("source");
        let id = VaultProfileId::internal();
        let plan = MovePlan::build(
            vec![MovePair::new(
                VaultFileRef::new(id.clone(), relative(&source)),
                VaultFileRef::new(id, relative(&destination)),
                false,
            )],
            &catalog,
            |_| false,
        )
        .expect("plan");
        inject_interruption(point);
        assert!(MoveExecutor::new(&catalog).execute(plan).is_err());
        let store = JournalStore::at_data_root(&data.root);
        let mut journal = store
            .operation_roots()
            .expect("operations")
            .into_iter()
            .map(|path| store.load_operation_root(&path).expect("journal"))
            .find(|journal| journal.state == MoveJournalState::RollingBack)
            .expect("interrupted operation");
        let stage = journal.artifacts[0]
            .staged_path
            .clone()
            .expect("stage intent");
        assert!(journal.artifacts[0].staged_identity.is_some());
        if index == 0 {
            journal.artifacts[0].staged_identity = None;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                std::fs::set_permissions(
                    root.join(stage.as_path()),
                    std::fs::Permissions::from_mode(0o644),
                )
                .expect("legacy stage mode");
            }
            let mut legacy = serde_json::to_value(&journal).expect("journal shape");
            legacy["plan"]
                .as_object_mut()
                .expect("plan object")
                .remove("copy_only");
            for artifact in legacy["artifacts"].as_array_mut().expect("artifacts") {
                artifact
                    .as_object_mut()
                    .expect("artifact object")
                    .remove("staged_identity");
            }
            let path = data
                .root
                .join("vault-ops")
                .join(journal.operation_id.as_str())
                .join("journal.json");
            protection::write(
                Domain::Vault,
                &path,
                &serde_json::to_vec(&legacy).expect("old journal"),
            )
            .expect("simulate old journal and crash before identity publication");
            assert!(
                !store
                    .load(&journal.operation_id)
                    .expect("old journal defaults")
                    .plan
                    .copy_only
            );
        } else {
            std::fs::write(root.join(stage.as_path()), b"truncated ciphertext")
                .expect("truncate in place, retaining identity");
        }
        assert!(protection::read(Domain::Vault, &root.join(stage.as_path())).is_err());
        crate::vault::prepare_protection_migration(&config)
            .expect("recover under old policy before migration");
        assert!(!root.join(stage.as_path()).exists());
        assert!(!root.join(&destination).exists());
        assert_eq!(
            protection::read(Domain::Vault, &root.join(&source)).expect("source restored"),
            b"source must survive"
        );
        assert_eq!(
            store
                .load(&journal.operation_id)
                .expect("terminal journal")
                .state,
            MoveJournalState::RolledBack
        );
        let journal_path = data
            .root
            .join("vault-ops")
            .join(journal.operation_id.as_str())
            .join("journal.json");
        std::fs::File::options()
            .write(true)
            .open(&journal_path)
            .expect("journal metadata")
            .set_times(std::fs::FileTimes::new().set_modified(
                std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1),
            ))
            .expect("old terminal history");
        crate::vault::prepare_protection_migration(&config).expect("terminal-only preflight");
        assert!(
            journal_path.exists(),
            "protection changes retain terminal transfer history"
        );
    }
}

#[test]
fn adapter_deletion_backup_has_real_profile_identity_and_binary_restore_plan_is_byte_based() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let _environment = Environment::new();
    let config = config();
    protection::configure(
        ProtectionPolicy {
            vault: true,
            ..ProtectionPolicy::default()
        },
        "restore-test-password",
    )
    .expect("protect vault");
    let root = protection::internal_vault_root();
    let id = VaultProfileId::internal();
    let payload = b"\x89PNG\0\xffbinary original";
    protection::write(Domain::Vault, &root.join("image.png"), payload).expect("binary fixture");
    crate::obsidian::Vault::new(root.clone())
        .remove_file(&id, Path::new("image.png"))
        .expect("sidebar adapter deletion");
    let data = TcuiDataPaths::discover();
    let record = std::fs::read_dir(data.root.join("vault-backups"))
        .expect("backup records")
        .next()
        .expect("record")
        .expect("entry");
    let backup_id: crate::vault::StandaloneBackupId = record
        .file_name()
        .to_str()
        .expect("backup ID")
        .parse()
        .expect("valid backup ID");
    let catalog = catalog(&config);
    let plan = VaultRestorePlan::build(
        &catalog,
        &data.root,
        id.clone(),
        relative("image.png"),
        backup_id.clone(),
    )
    .expect("binary backup matches real registered profile");
    let kind = VaultMutationKind::Restore(plan);
    assert!(kind
        .preview(&catalog)
        .expect("binary preview")
        .rows
        .iter()
        .any(|row| row.contains("Binary restore")));
    kind.revalidate(&catalog, &data.root)
        .expect("byte identity revalidation");
    let service = VaultWriteService::new([VaultProfileRegistration::new(id.clone(), root.clone())])
        .expect("restore service");
    service
        .restore(&id, &relative("image.png"), &backup_id)
        .expect("restore bytes");
    assert_eq!(
        protection::read(Domain::Vault, &root.join("image.png")).expect("exact binary round trip"),
        payload
    );
    assert!(
        kind.revalidate(&catalog, &data.root).is_err(),
        "missing-vs-present identity must invalidate the old plan"
    );
    let plan = VaultRestorePlan::build(
        &catalog,
        &data.root,
        id.clone(),
        relative("image.png"),
        backup_id,
    )
    .expect("binary current and binary backup");
    protection::write(
        Domain::Vault,
        &root.join("image.png"),
        b"\x89PNG\0\xffchanged",
    )
    .expect("concurrent byte change");
    assert!(VaultMutationKind::Restore(plan)
        .revalidate(&catalog, &data.root)
        .is_err());
}

#[test]
fn nested_external_profile_handles_cannot_write_plaintext_into_protected_internal_targets() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let environment = Environment::new();
    let _config = config();
    protection::configure(
        ProtectionPolicy {
            vault: true,
            ..ProtectionPolicy::default()
        },
        "nested-test-password",
    )
    .expect("protect vault");
    let data = TcuiDataPaths::discover();
    let root = protection::internal_vault_root();
    let above = profile("Above", &data.root);
    let nested = profile("Nested", &root.join("sub"));
    let external = profile("External", &environment.root.join("ordinary"));
    let service = VaultWriteService::new([
        VaultProfileRegistration::new(above.id.clone(), above.root.clone()),
        VaultProfileRegistration::new(nested.id.clone(), nested.root.clone()),
        VaultProfileRegistration::new(external.id.clone(), external.root.clone()),
    ])
    .expect("overlapping profile handles");
    service
        .write(&above.id, &relative("vault/above.md"), b"above secret")
        .expect("ancestor-root write");
    service
        .write(&nested.id, &relative("nested.md"), b"nested secret")
        .expect("nested-root write");
    service
        .write(
            &external.id,
            &relative("ordinary.md"),
            b"ordinary plaintext",
        )
        .expect("external write");
    assert_ne!(
        std::fs::read(root.join("above.md")).expect("protected bytes"),
        b"above secret"
    );
    assert_ne!(
        std::fs::read(root.join("sub/nested.md")).expect("protected nested bytes"),
        b"nested secret"
    );
    assert_eq!(
        VaultFs::open_profile(&above.root)
            .expect("ancestor handle")
            .read_bytes(&relative("vault/above.md"))
            .expect("ancestor decoding"),
        b"above secret"
    );
    assert_eq!(
        std::fs::read(external.root.join("ordinary.md")).expect("ordinary external bytes"),
        b"ordinary plaintext"
    );
    protection::lock().expect("lock");
    assert!(service
        .write(&above.id, &relative("vault/leak.md"), b"must not write")
        .is_err());
    assert!(!root.join("leak.md").exists());
    assert!(VaultFs::open_profile(&nested.root)
        .expect("nested handle")
        .read_text(&relative("nested.md"))
        .is_err());
    assert_eq!(
        VaultFs::open_profile(&external.root)
            .expect("external handle")
            .read_text(&relative("ordinary.md"))
            .expect("external is independent"),
        "ordinary plaintext"
    );
}

#[test]
fn concurrent_automatic_exports_never_replace_each_other_or_follow_dangling_candidates() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let environment = Environment::new();
    let _config = config();
    protection::configure(
        ProtectionPolicy {
            vault: true,
            ..ProtectionPolicy::default()
        },
        "export-test-password",
    )
    .expect("protect vault");
    let internal = protection::internal_vault_root();
    let external = environment.root.join("exports");
    std::fs::create_dir_all(&external).expect("explicit destination");
    #[cfg(unix)]
    std::os::unix::fs::symlink(environment.root.join("absent"), external.join("same-7.txt"))
        .expect("dangling candidate");
    for destination in [&internal, &external] {
        let results = std::thread::scope(|scope| {
            let jobs: Vec<_> = (0..8)
                .map(|number| {
                    scope.spawn(move || {
                        let bytes = format!("export-{number}").into_bytes();
                        let path = crate::export::create_named_export(
                            destination,
                            "same",
                            7,
                            "txt",
                            &bytes,
                        )
                        .expect("non-clobber export");
                        (path, bytes)
                    })
                })
                .collect();
            jobs.into_iter()
                .map(|job| job.join().expect("export thread"))
                .collect::<Vec<_>>()
        });
        let unique: std::collections::HashSet<_> = results.iter().map(|(path, _)| path).collect();
        assert_eq!(unique.len(), 8);
        for (path, expected) in results {
            assert_eq!(
                crate::vault::read_artifact_path(&path).expect("winner bytes"),
                expected
            );
            if destination == &internal {
                assert_ne!(std::fs::read(path).expect("ciphertext"), expected);
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn recovery_never_deletes_a_stage_replaced_with_an_unrelated_regular_file() {
    use std::os::unix::fs::PermissionsExt as _;
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let _environment = Environment::new();
    let config = config();
    protection::configure(
        ProtectionPolicy {
            vault: true,
            ..ProtectionPolicy::default()
        },
        "identity-test-password",
    )
    .expect("protect vault");
    let catalog = catalog(&config);
    let root = protection::internal_vault_root();
    let data = TcuiDataPaths::discover();
    protection::write(Domain::Vault, &root.join("source.md"), b"source").expect("source");
    let id = VaultProfileId::internal();
    let plan = MovePlan::build(
        vec![MovePair::new(
            VaultFileRef::new(id.clone(), relative("source.md")),
            VaultFileRef::new(id, relative("destination.md")),
            false,
        )],
        &catalog,
        |_| false,
    )
    .expect("plan");
    inject_interruption(TestFailurePoint::StageSync);
    assert!(MoveExecutor::new(&catalog).execute(plan).is_err());
    let store = JournalStore::at_data_root(&data.root);
    let operation = store.operation_roots().expect("operations").remove(0);
    let journal = store.load_operation_root(&operation).expect("journal");
    let stage = root.join(
        journal.artifacts[0]
            .staged_path
            .as_ref()
            .expect("stage")
            .as_path(),
    );
    let _original_inode = std::fs::File::open(&stage).expect("retain original inode");
    std::fs::remove_file(&stage).expect("substitute fixture");
    std::fs::write(&stage, b"unrelated data").expect("unrelated regular file");
    std::fs::set_permissions(&stage, std::fs::Permissions::from_mode(0o600))
        .expect("same private mode");
    let report =
        MoveRecoveryService::at_data_root(&catalog, &data.root, MoveRecoveryConfig::default())
            .recover_startup()
            .expect("conflict report");
    assert!(report.blocked());
    assert!(
        crate::vault::prepare_protection_migration(&config).is_err(),
        "identity uncertainty must block protection migration"
    );
    assert_eq!(
        std::fs::read(stage).expect("unrelated file retained"),
        b"unrelated data"
    );
    assert_eq!(
        protection::read(Domain::Vault, &root.join("source.md")).expect("source retained"),
        b"source"
    );
}
