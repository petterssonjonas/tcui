use super::*;
use crate::storage::paths::TcuiDataPaths;
use crate::vault::move_execute::{inject_interruption, TestFailurePoint};
use crate::vault::move_journal::MoveJournalState;
use crate::vault::move_plan::MovePlanError;
use crate::vault::move_recovery::{MoveRecoveryConfig, MoveRecoveryService};

fn pair(source: &Path, destination: &Path, overwrite: bool) -> MovePair {
    MovePair::new(
        VaultFileRef::new(VaultRelativePath::parse(source).expect("source relative path")),
        VaultFileRef::new(
            VaultRelativePath::parse(destination).expect("destination relative path"),
        ),
        overwrite,
    )
}

#[test]
fn native_move_preserves_item_identity_binary_bytes_and_chat_links() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let _environment = Environment::new();
    let store = ChatSavedFileStore::discover();
    let bytes = b"\x89PNG\0native image bytes";
    let item = store
        .create_item("image.png", bytes, Some(7))
        .expect("native item");
    let link_id = item.chat_links[0].id;
    let data = TcuiDataPaths::discover();
    let plan = MovePlan::build(
        vec![pair(
            &item.relative_path,
            Path::new("nested/image.png"),
            false,
        )],
        &data.root,
        |_| false,
    )
    .expect("native move");
    MoveExecutor::at_data_root(&data.root)
        .execute(plan)
        .expect("move linked item");
    let moved = store
        .list_items()
        .expect("native index")
        .into_iter()
        .find(|entry| entry.id == item.id)
        .expect("same item ID");
    assert_eq!(moved.relative_path, Path::new("nested/image.png"));
    assert_eq!(moved.chat_links[0].id, link_id);
    assert_eq!(store.read_item(item.id).expect("same binary item"), bytes);
    assert_eq!(
        store.open_link(7, link_id).expect("same chat association"),
        bytes
    );
    assert!(!item.path.exists());
}

#[test]
fn native_copy_keeps_original_identity_and_chat_association() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let _environment = Environment::new();
    let store = ChatSavedFileStore::discover();
    let item = store
        .create_item("source.md", b"source content", Some(7))
        .expect("native item");
    let data = TcuiDataPaths::discover();
    let plan = MovePlan::build_copy(
        vec![pair(&item.relative_path, Path::new("copy.md"), false)],
        &data.root,
        |_| false,
    )
    .expect("native copy");
    MoveExecutor::at_data_root(&data.root)
        .execute(plan)
        .expect("copy");
    let items = store.list_items().expect("native index");
    let original = items
        .iter()
        .find(|entry| entry.id == item.id)
        .expect("original item");
    let copy = items
        .iter()
        .find(|entry| entry.relative_path == Path::new("copy.md"))
        .expect("new copied item");
    assert_ne!(copy.id, item.id);
    assert_eq!(original.chat_links, item.chat_links);
    assert!(copy.chat_links.is_empty());
    assert_eq!(
        store.read_item(copy.id).expect("copied content"),
        b"source content"
    );
}

#[test]
fn interrupted_native_overwrite_restores_both_ids_links_and_bodies() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let _environment = Environment::new();
    let store = ChatSavedFileStore::discover();
    let source = store
        .create_item("source.md", b"source bytes", Some(7))
        .expect("source item");
    let destination = store
        .create_item("destination.md", b"destination bytes", Some(8))
        .expect("destination item");
    let data = TcuiDataPaths::discover();
    let plan = MovePlan::build(
        vec![pair(
            &source.relative_path,
            &destination.relative_path,
            true,
        )],
        &data.root,
        |_| false,
    )
    .expect("overwrite plan");
    let operation_id = crate::vault::move_journal::MoveOperationId::from_plan(&plan);
    inject_interruption(TestFailurePoint::IdentityCommitted);
    assert!(MoveExecutor::at_data_root(&data.root)
        .execute(plan)
        .is_err());
    assert_eq!(
        JournalStore::at_data_root(&data.root)
            .load(&operation_id)
            .expect("pending journal")
            .state,
        MoveJournalState::RollingBack
    );
    // Do not reconcile the index between interruption and recovery.
    let report = MoveRecoveryService::at_data_root(&data.root, MoveRecoveryConfig::default())
        .recover_startup()
        .expect("restart recovery");
    assert!(!report.blocked());
    assert_eq!(
        store.read_item(source.id).expect("restored source"),
        b"source bytes"
    );
    assert_eq!(
        store
            .read_item(destination.id)
            .expect("restored overwritten destination"),
        b"destination bytes"
    );
    assert_eq!(
        store
            .open_link(7, source.chat_links[0].id)
            .expect("source chat association"),
        b"source bytes"
    );
    assert_eq!(
        store
            .open_link(8, destination.chat_links[0].id)
            .expect("destination chat association"),
        b"destination bytes"
    );
    assert_eq!(
        JournalStore::at_data_root(&data.root)
            .load(&operation_id)
            .expect("settled journal")
            .state,
        MoveJournalState::RolledBack
    );
}

#[test]
fn native_recovery_never_replays_or_removes_legacy_external_journals() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let environment = Environment::new();
    let external = environment.root.join("external");
    std::fs::create_dir_all(&external).expect("external fixture");
    let sentinel = external.join("user.md");
    std::fs::write(&sentinel, b"external user data").expect("external sentinel");
    let data = TcuiDataPaths::discover();
    let legacy = data
        .root
        .join("vault-ops/move-00000000000000000000000000000000");
    std::fs::create_dir_all(&legacy).expect("legacy journal directory");
    let bytes = serde_json::to_vec(
        &serde_json::json!({"version":1,"external_root":external,"state":"sources_removing"}),
    )
    .expect("legacy bytes");
    std::fs::write(legacy.join("journal.json"), &bytes).expect("legacy journal");
    let report =
        MoveRecoveryService::at_data_root(&data.root, MoveRecoveryConfig { retention_days: 0 })
            .recover_startup()
            .expect("native recovery");
    assert!(!report.blocked());
    assert_eq!(
        std::fs::read(&sentinel).expect("external preserved"),
        b"external user data"
    );
    assert_eq!(
        std::fs::read(legacy.join("journal.json")).expect("legacy journal retained"),
        bytes
    );
}

#[cfg(unix)]
#[test]
fn native_move_rejects_hardlink_aliases_before_mutation() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let _environment = Environment::new();
    let root = protection::internal_vault_root();
    std::fs::create_dir_all(&root).expect("native root");
    std::fs::write(root.join("source.md"), b"source").expect("source");
    std::fs::hard_link(root.join("source.md"), root.join("alias.md")).expect("hardlink alias");
    let data = TcuiDataPaths::discover();
    assert!(matches!(
        MovePlan::build(
            vec![pair(Path::new("source.md"), Path::new("alias.md"), true)],
            &data.root,
            |_| false
        ),
        Err(MovePlanError::OverlappingSourceDestination { .. })
    ));
    assert_eq!(
        std::fs::read(root.join("source.md")).expect("source preserved"),
        b"source"
    );
}

#[test]
fn native_portable_reads_and_writes_require_adoption_after_source_unlock() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let stored = {
        let _source = Environment::new();
        protection::configure(
            ProtectionPolicy {
                vault: true,
                ..ProtectionPolicy::default()
            },
            "source-password",
        )
        .expect("source policy");
        protection::encode(Domain::Vault, b"portable native content").expect("portable envelope")
    };
    let _destination = Environment::new();
    let root = protection::internal_vault_root();
    std::fs::create_dir_all(&root).expect("native destination");
    let path = root.join("copied.md");
    std::fs::write(&path, &stored).expect("copied native envelope");
    protection::unlock_foreign_file(Domain::Vault, &stored, "source-password")
        .expect("source authentication");
    assert_eq!(
        protection::decode(Domain::Vault, &stored).expect("explicit import decode"),
        b"portable native content"
    );
    let vault = VaultFs::open(&root).expect("native handle");
    let error = vault
        .read_bytes(&relative("copied.md"))
        .expect_err("native adoption required");
    match error {
        crate::vault::VaultFsError::Io { source, .. } => assert!(source
            .get_ref()
            .is_some_and(|error| error.is::<protection::NativeStoreAdoptionNeeded>())),
        error => panic!("unexpected native read error: {error}"),
    }
    assert!(vault.read_text_limited(&relative("copied.md"), 0).is_err());
    assert!(crate::vault::read_artifact_path(&path).is_err());
    let service = VaultWriteService::new().expect("native write handle");
    assert!(service
        .write(&relative("copied.md"), b"replacement")
        .is_err());
    assert!(
        crate::vault::create_artifact_path(&root.join("new-export.md"), b"plaintext export")
            .is_err()
    );
    assert!(crate::vault::write_artifact_path(&path, b"plaintext replacement").is_err());
    assert!(service
        .write(&relative("new.md"), b"new plaintext")
        .is_err());
    assert!(service.remove(&relative("copied.md")).is_err());
    assert_eq!(
        std::fs::read(&path).expect("copied ciphertext retained"),
        stored
    );
    assert!(!root.join("new.md").exists());
    assert!(!TcuiDataPaths::discover().chat_file_links_manifest.exists());
}

#[test]
fn native_adoption_preserves_private_stage_identity_until_protected_recovery() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let _source = Environment::new();
    protection::configure(
        ProtectionPolicy {
            vault: true,
            ..ProtectionPolicy::default()
        },
        "source-password",
    )
    .expect("source policy");
    let source_store = ChatSavedFileStore::discover();
    let item = source_store
        .create_item("source.md", b"protected source body", Some(7))
        .expect("source item");
    let source_paths = TcuiDataPaths::discover();
    let source_stored = std::fs::read(&item.path).expect("source ciphertext");
    let plan = MovePlan::build(
        vec![pair(
            &item.relative_path,
            Path::new("nested/moved.md"),
            false,
        )],
        &source_paths.root,
        |_| false,
    )
    .expect("native move");
    let operation_id = crate::vault::move_journal::MoveOperationId::from_plan(&plan);
    inject_interruption(TestFailurePoint::StageSync);
    assert!(MoveExecutor::at_data_root(&source_paths.root)
        .execute(plan)
        .is_err());
    let journal = JournalStore::at_data_root(&source_paths.root)
        .load(&operation_id)
        .expect("source journal");
    let stage_relative = journal.artifacts[0]
        .staged_path
        .as_ref()
        .expect("private stage")
        .as_path()
        .to_path_buf();
    let source_stage = source_paths.root.join("vault").join(&stage_relative);
    let identity = crate::vault::move_journal::StageIdentity::capture(
        &protection::open_directory(source_stage.parent().expect("stage parent"), false)
            .expect("stage directory")
            .symlink_metadata(source_stage.file_name().expect("stage name"))
            .expect("stage metadata"),
    );
    protection::lock().expect("clear source session");

    let _destination = Environment::new();
    let destination_paths = TcuiDataPaths::discover();
    std::fs::create_dir_all(&destination_paths.root).expect("destination data root");
    // Relocation preserves private stage identity; an arbitrary copied inode is not trusted.
    for directory in [
        "vault",
        "vault-backups",
        "vault-native-ops",
        "chat-file-links",
    ] {
        std::fs::rename(
            source_paths.root.join(directory),
            destination_paths.root.join(directory),
        )
        .expect("relocate portable native storage without credentials");
    }
    protection::unlock_foreign_file(Domain::Vault, &source_stored, "source-password")
        .expect("authenticate source wrapping");
    assert!(!protection::is_protected(Domain::Vault).expect("plaintext destination policy"));
    let destination_stage = destination_paths.root.join("vault").join(&stage_relative);
    assert_eq!(
        crate::vault::protection_migration_stage_paths().expect("authenticated stage exclusions"),
        vec![destination_stage.clone()]
    );
    let held_stage = destination_stage.with_extension("held");
    std::fs::rename(&destination_stage, &held_stage).expect("hold original stage");
    std::fs::copy(&held_stage, &destination_stage).expect("replace stage with copied inode");
    assert!(crate::vault::protection_migration_stage_paths().is_err());
    std::fs::remove_file(&destination_stage).expect("remove synthetic replacement");
    std::fs::rename(&held_stage, &destination_stage).expect("restore original stage inode");
    let restored_identity = crate::vault::move_journal::StageIdentity::capture(
        &protection::open_directory(destination_stage.parent().expect("stage parent"), false)
            .expect("stage directory")
            .symlink_metadata(destination_stage.file_name().expect("stage name"))
            .expect("stage metadata"),
    );
    assert_eq!(restored_identity, identity);
    protection::adopt_native_store(Domain::Vault, "destination-password")
        .expect("durable native adoption and recovery");
    assert!(protection::is_protected(Domain::Vault).expect("protected destination policy"));
    assert!(!destination_stage.exists());
    let destination_store = ChatSavedFileStore::discover();
    assert_eq!(
        destination_store
            .read_item(item.id)
            .expect("native item after recovery"),
        b"protected source body"
    );
    assert_ne!(
        std::fs::read(
            destination_paths
                .root
                .join("vault")
                .join(&item.relative_path)
        )
        .expect("destination ciphertext"),
        b"protected source body"
    );
    assert_eq!(
        JournalStore::at_data_root(&destination_paths.root)
            .load(&operation_id)
            .expect("recovered native journal")
            .state,
        MoveJournalState::RolledBack
    );
}

#[test]
fn format_upgrade_stage_registry_skips_foreign_portable_journals_without_unlock() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let stored = {
        let _source = Environment::new();
        protection::configure(
            ProtectionPolicy {
                vault: true,
                ..ProtectionPolicy::default()
            },
            "foreign-source-password",
        )
        .expect("source policy");
        protection::encode(Domain::Vault, b"opaque foreign journal bytes")
            .expect("foreign portable envelope")
    };
    let _destination = Environment::new();
    let paths = TcuiDataPaths::discover();
    let operation = "move-00000000000000000000000000000000";
    let operation_root = paths.root.join("vault-native-ops").join(operation);
    std::fs::create_dir_all(&operation_root).expect("copied operation directory");
    let journal_path = operation_root.join("journal.json");
    std::fs::write(&journal_path, &stored).expect("copied foreign journal");
    let stage_path = paths
        .root
        .join("vault")
        .join(format!(".tcui-{operation}-000.stage"));
    std::fs::create_dir_all(stage_path.parent().expect("native root")).expect("native directory");
    std::fs::write(&stage_path, &stored).expect("copied foreign stage");
    assert!(protection::read(Domain::Vault, &journal_path).is_err());
    assert!(crate::vault::protection_format_upgrade_stage_paths()
        .expect("format-only stage registry")
        .is_empty());
    assert!(crate::vault::protection_migration_stage_paths().is_err());
    assert_eq!(
        std::fs::read(&journal_path).expect("foreign journal preserved"),
        stored
    );
    assert_eq!(
        std::fs::read(&stage_path).expect("foreign stage preserved"),
        stored
    );
    assert!(!paths.chat_file_links_manifest.exists());
}
