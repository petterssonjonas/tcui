use std::path::{Path, PathBuf};

use super::move_execute::MoveExecutor;
use super::move_journal::JournalStore;
use super::move_plan::{MovePair, MovePlan, VaultFileRef};
use super::{
    VaultFs, VaultProfileRegistration, VaultRelativePath, VaultWalkLimits, VaultWriteService,
};
use crate::config::vault_profiles::{VaultCatalog, VaultProfile, VaultProfileId};
use crate::protection::{self, Domain, ProtectionPolicy};
use crate::storage::chat_files::ChatSavedFileStore;

struct Environment {
    root: PathBuf,
    previous: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

impl Environment {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "tcui-internal-vault-protection-{}",
            rand::random::<u64>()
        ));
        let previous = ["XDG_DATA_HOME", "XDG_CONFIG_HOME", "XDG_CACHE_HOME", "HOME"]
            .into_iter()
            .map(|name| (name, std::env::var_os(name)))
            .collect();
        for (variable, directory) in [
            ("XDG_DATA_HOME", "data"),
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_CACHE_HOME", "cache"),
            ("HOME", "home"),
        ] {
            std::fs::create_dir_all(root.join(directory)).expect("isolated directory");
            std::env::set_var(variable, root.join(directory));
        }
        Self { root, previous }
    }
}

impl Drop for Environment {
    fn drop(&mut self) {
        protection::lock().expect("clear isolated session");
        for (name, previous) in self.previous.drain(..) {
            match previous {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn relative(path: &str) -> VaultRelativePath {
    VaultRelativePath::parse(Path::new(path)).expect("relative path")
}

#[test]
fn internal_vault_migration_saved_media_backups_and_cross_profile_moves_obey_protection() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let environment = Environment::new();
    let legacy = environment.root.join("legacy-saved");
    let external = environment.root.join("external-vault");
    std::fs::create_dir_all(&legacy).expect("legacy saved root");
    std::fs::create_dir_all(&external).expect("external vault");
    std::fs::write(legacy.join("existing.md"), b"existing saved content").expect("old file");
    std::fs::write(external.join("ordinary.md"), b"external plaintext").expect("external fixture");
    std::fs::create_dir_all(legacy.join("nested")).expect("old nested directory");
    std::fs::write(legacy.join("nested/download.bin"), b"nested saved bytes")
        .expect("old nested file");
    #[cfg(unix)]
    std::os::unix::fs::symlink(external.join("ordinary.md"), legacy.join("escape.md"))
        .expect("source symlink");
    let external_profile = VaultProfile::new("External", &external).expect("external profile");
    let mut config = crate::config::AppConfig {
        saved_files_dir: Some(legacy.display().to_string()),
        ..crate::config::AppConfig::default()
    };
    config
        .vaults
        .insert(external_profile.id.clone(), external_profile.clone());
    config.save().expect("isolated config");
    protection::configure(
        ProtectionPolicy {
            vault: true,
            ..ProtectionPolicy::default()
        },
        "vault-test-password",
    )
    .expect("configure");
    let root = protection::internal_vault_root();
    assert_eq!(
        protection::read(Domain::Vault, &root.join("existing.md")).expect("migrated data"),
        b"existing saved content"
    );
    assert_ne!(
        std::fs::read(root.join("existing.md")).expect("encrypted old file"),
        b"existing saved content"
    );
    assert_eq!(
        std::fs::read(legacy.join("existing.md")).expect("external saved source retained"),
        b"existing saved content"
    );
    assert_eq!(
        std::fs::read(external.join("ordinary.md")).expect("external untouched"),
        b"external plaintext"
    );
    assert_eq!(
        protection::read(Domain::Vault, &root.join("nested/download.bin"))
            .expect("nested migration"),
        b"nested saved bytes"
    );
    assert_ne!(
        std::fs::read(root.join("nested/download.bin")).expect("nested ciphertext"),
        b"nested saved bytes"
    );
    assert!(
        !root.join("escape.md").exists(),
        "migration never follows source symlinks"
    );

    let saved = ChatSavedFileStore::discover();
    let payload = b"\x89PNG\0binary-secret-payload";
    let link = saved
        .create_for_chat(7, "image.png", payload)
        .expect("save binary");
    assert_eq!(saved.open_link(7, link.id).expect("read binary"), payload);
    assert_ne!(
        std::fs::read(saved.path_for_link(&link)).expect("ciphertext"),
        payload
    );
    let data = crate::storage::paths::TcuiDataPaths::discover();
    let manifest = std::fs::read(&data.chat_file_links_manifest).expect("link manifest");
    assert!(serde_json::from_slice::<serde_json::Value>(&manifest).is_err());

    let id = VaultProfileId::internal();
    let service = VaultWriteService::new([
        VaultProfileRegistration::new(id.clone(), root.clone()),
        VaultProfileRegistration::new(external_profile.id.clone(), external.clone()),
    ])
    .expect("service");
    let first = service
        .write(&id, &relative("existing.md"), b"replacement")
        .expect("protected write");
    assert!(service
        .validate_editor_target(&id, &relative("existing.md"))
        .is_err());
    service
        .restore(&id, &relative("existing.md"), &first.backup_id)
        .expect("restore decoded backup");
    assert_eq!(
        VaultFs::open_profile(&root)
            .expect("vault")
            .read_text(&relative("existing.md"))
            .expect("restored"),
        "existing saved content"
    );
    let backup = data
        .root
        .join("vault-backups")
        .join(first.backup_id.as_str());
    assert_ne!(
        std::fs::read(backup.join("original.bin")).expect("backup ciphertext"),
        b"existing saved content"
    );
    assert!(serde_json::from_slice::<serde_json::Value>(
        &std::fs::read(backup.join("envelope.json")).expect("metadata ciphertext")
    )
    .is_err());

    let catalog = VaultCatalog {
        profiles: config.vaults,
        default_id: Some(id.clone()),
    };
    let outgoing = MovePlan::build(
        vec![MovePair::new(
            VaultFileRef::new(id.clone(), relative("existing.md")),
            VaultFileRef::new(external_profile.id.clone(), relative("exported.md")),
            false,
        )],
        &catalog,
        |_| false,
    )
    .expect("outgoing move plan");
    let outcome = MoveExecutor::new(&catalog)
        .execute(outgoing)
        .expect("decode external move");
    assert_eq!(
        std::fs::read(external.join("exported.md")).expect("explicit plaintext destination"),
        b"existing saved content"
    );
    assert!(!root.join("existing.md").exists());
    let journal_path = data
        .root
        .join("vault-ops")
        .join(outcome.operation_id.as_str())
        .join("journal.json");
    assert!(serde_json::from_slice::<serde_json::Value>(
        &std::fs::read(journal_path).expect("journal ciphertext")
    )
    .is_err());
    let incoming = MovePlan::build(
        vec![MovePair::new(
            VaultFileRef::new(external_profile.id.clone(), relative("ordinary.md")),
            VaultFileRef::new(id.clone(), relative("imported.md")),
            false,
        )],
        &catalog,
        |_| false,
    )
    .expect("incoming move plan");
    MoveExecutor::new(&catalog)
        .execute(incoming)
        .expect("encode internal move");
    assert_ne!(
        std::fs::read(root.join("imported.md")).expect("encrypted imported file"),
        b"external plaintext"
    );
    assert_eq!(
        protection::read(Domain::Vault, &root.join("imported.md")).expect("decoded import"),
        b"external plaintext"
    );

    let open_handle = VaultFs::open_profile(&root).expect("open handle");
    protection::lock().expect("lock");
    assert!(open_handle.read_text(&relative("imported.md")).is_err());
    assert!(open_handle.list(None, VaultWalkLimits::default()).is_err());
    assert!(saved.open_link(7, link.id).is_err());
    assert!(saved.list_saved().is_err());
    assert!(JournalStore::at_data_root(&data.root)
        .load(&outcome.operation_id)
        .is_err());
    assert!(service
        .restore(&id, &relative("existing.md"), &first.backup_id)
        .is_err());
    assert!(protection::unlock("wrong-password").is_err());
    assert!(saved.open_link(7, link.id).is_err());
    assert_eq!(
        VaultFs::open_profile(&external)
            .expect("external handle")
            .read_text(&relative("exported.md"))
            .expect("external remains available"),
        "existing saved content"
    );
    protection::unlock("vault-test-password").expect("unlock");
    assert_eq!(
        saved.open_link(7, link.id).expect("binary round trip"),
        payload
    );
}

#[test]
fn saved_file_relocation_preserves_conflicts_and_restarts_without_deleting_sources() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let environment = Environment::new();
    let mut config = crate::config::AppConfig::load().expect("isolated config");
    let source = environment.root.join("old-saved");
    std::fs::create_dir_all(&source).expect("old saved directory");
    std::fs::write(source.join("note.md"), b"source data").expect("old saved file");
    let destination = protection::internal_vault_root().join("note.md");
    std::fs::write(&destination, b"different destination").expect("conflict");
    config.saved_files_dir = Some(source.display().to_string());
    let result = crate::storage::saved_file_migration::migrate_saved_files(&config);
    assert!(matches!(
        result,
        Err(crate::storage::saved_file_migration::MigrationError::Conflict { .. })
    ));
    assert_eq!(
        std::fs::read(source.join("note.md")).expect("source retained"),
        b"source data"
    );
    assert_eq!(
        std::fs::read(&destination).expect("destination retained"),
        b"different destination"
    );
    let marker = crate::storage::paths::TcuiDataPaths::discover()
        .migrations_dir
        .join("saved-internal-vault-v1.done");
    assert!(
        !marker.exists(),
        "conflicts must not publish migration completion"
    );
    std::fs::remove_file(&destination).expect("resolve isolated fixture conflict");
    crate::storage::saved_file_migration::migrate_saved_files(&config).expect("retry");
    crate::storage::saved_file_migration::migrate_saved_files(&config).expect("idempotent restart");
    assert_eq!(
        std::fs::read(&destination).expect("copied file"),
        b"source data"
    );
    assert_eq!(
        std::fs::read(source.join("note.md")).expect("original still retained"),
        b"source data"
    );
}

#[path = "cutover_tests.rs"]
mod cutover;
