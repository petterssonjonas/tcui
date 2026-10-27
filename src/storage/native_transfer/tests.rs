use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::MutexGuard;

use super::*;
use crate::protection::ProtectionPolicy;

struct Environment {
    root: PathBuf,
    previous: Vec<(&'static str, Option<OsString>)>,
    _guard: MutexGuard<'static, ()>,
}

impl Environment {
    fn new() -> Self {
        let guard = crate::test_support::env_lock()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let root = std::env::temp_dir().join(format!(
            "tcui-native-transfer-{:032x}",
            rand::random::<u128>()
        ));
        let previous = ["HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME"]
            .into_iter()
            .map(|name| (name, std::env::var_os(name)))
            .collect();
        let environment = Self {
            root,
            previous,
            _guard: guard,
        };
        environment.select("initial");
        environment
    }

    fn select(&self, instance: &str) {
        protection::lock().expect("clear previous test session");
        for (variable, name) in [
            ("HOME", "home"),
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_DATA_HOME", "data"),
            ("XDG_CACHE_HOME", "cache"),
        ] {
            let path = self.root.join(instance).join(name);
            std::fs::create_dir_all(&path).expect("isolated instance directories");
            std::env::set_var(variable, path);
        }
    }

    fn plain(&self) {
        protection::configure(ProtectionPolicy::default(), "").expect("plaintext test policy");
    }

    fn vault_protected(&self, password: &str) {
        protection::configure(
            ProtectionPolicy {
                vault: true,
                ..ProtectionPolicy::default()
            },
            password,
        )
        .expect("protected test policy");
    }

    fn source(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.root.join("sources").join(name);
        protection::write_new_plain(&path, bytes).expect("source fixture");
        path
    }
}

impl Drop for Environment {
    fn drop(&mut self) {
        let _ = protection::lock();
        for (name, previous) in self.previous.drain(..) {
            match previous {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn add_preserves_nested_binary_bodies_and_both_collision_identities() {
    let environment = Environment::new();
    environment.plain();
    let store = vault_store().unwrap();
    let original = store
        .create_item_at(Path::new("nested/image.bin"), b"old\0body", None)
        .unwrap();
    let source = environment.source("folder/nested/image.bin", b"new\0body");
    let folder = source.parent().unwrap().parent().unwrap();
    let preview = NativeTransferService::prepare(Domain::Vault, folder, TransferMode::Add).unwrap();
    assert!(preview.files[0].conflicts);
    let report = preview.commit().unwrap();
    let items = store.list_items().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(store.read_item(original.id).unwrap(), b"old\0body");
    let imported = items.iter().find(|item| item.id != original.id).unwrap();
    assert_eq!(store.read_item(imported.id).unwrap(), b"new\0body");
    assert_eq!(imported.relative_path.parent(), Some(Path::new("nested")));
    assert_eq!(report.imported, 1);
    assert_eq!(std::fs::read(source).unwrap(), b"new\0body");
}

#[test]
fn replace_keeps_exact_old_native_body_and_manifest_in_private_backup() {
    use std::os::unix::fs::PermissionsExt;
    let environment = Environment::new();
    environment.plain();
    let store = vault_store().unwrap();
    let original = store.create_item("old.bin", b"old body", Some(42)).unwrap();
    let source = environment.source("new.bin", b"replacement body");
    let report = NativeTransferService::prepare(Domain::Vault, &source, TransferMode::Replace)
        .unwrap()
        .commit()
        .unwrap();
    let backup = report.backup.unwrap();
    assert_eq!(
        std::fs::read(backup.join("root/old.bin")).unwrap(),
        b"old body"
    );
    assert!(backup.join("auxiliary").is_file());
    assert_eq!(
        std::fs::metadata(&backup).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert!(store.read_item(original.id).is_err());
    assert_eq!(store.list_items().unwrap().len(), 1);
    assert!(NativeTransferService::recover_pending(Domain::Vault)
        .unwrap()
        .is_none());
    assert_eq!(std::fs::read(source).unwrap(), b"replacement body");
}

#[test]
fn interrupted_replace_restores_exact_ids_links_and_bodies_and_retains_partial_data() {
    let environment = Environment::new();
    environment.plain();
    let store = vault_store().unwrap();
    let original = store.create_item("old.md", b"old body", Some(73)).unwrap();
    let original_manifest =
        std::fs::read(TcuiDataPaths::discover().chat_file_links_manifest).unwrap();
    let backup = {
        let _exclusive = filesystem::exclusive().unwrap();
        let backup = filesystem::snapshot(Domain::Vault).unwrap();
        store.trash_item(original.id).unwrap();
        store
            .create_item("partial.bin", b"partial body", None)
            .unwrap();
        backup
    };
    let source = environment.source("new.bin", b"new body");
    let error = NativeTransferService::prepare(Domain::Vault, &source, TransferMode::Add)
        .unwrap()
        .commit()
        .unwrap_err();
    assert!(matches!(error, TransferError::RecoveryNeeded { .. }));
    protection::lock().unwrap();
    assert_eq!(
        NativeTransferService::recover_pending(Domain::Vault).unwrap(),
        Some(backup.clone())
    );
    assert_eq!(
        std::fs::read(TcuiDataPaths::discover().chat_file_links_manifest).unwrap(),
        original_manifest
    );
    let restored = store.list_items().unwrap();
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].id, original.id);
    assert_eq!(restored[0].chat_links, original.chat_links);
    assert_eq!(store.read_item(original.id).unwrap(), b"old body");
    let retained: Vec<_> = std::fs::read_dir(&backup)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("interrupted-")
        })
        .collect();
    assert_eq!(
        std::fs::read(retained[0].join("partial.bin")).unwrap(),
        b"partial body"
    );
    assert!(NativeTransferService::recover_pending(Domain::Vault)
        .unwrap()
        .is_none());
}

#[test]
fn changed_source_and_metadata_are_rejected_before_destination_mutation() {
    let environment = Environment::new();
    environment.plain();
    let store = vault_store().unwrap();
    let existing = store.create_item("keep.md", b"keep", None).unwrap();
    let source = environment.source("input.md", b"previewed");
    let preview =
        NativeTransferService::prepare(Domain::Vault, &source, TransferMode::Replace).unwrap();
    std::fs::write(&source, b"modified").unwrap();
    assert!(matches!(
        preview.commit(),
        Err(TransferError::SourceChanged(_))
    ));
    assert_eq!(store.read_item(existing.id).unwrap(), b"keep");
    assert_eq!(store.list_items().unwrap().len(), 1);
}

#[test]
fn portable_folder_crosses_different_instance_passwords_without_plaintext_staging() {
    let environment = Environment::new();
    environment.vault_protected("source password");
    let source_item = vault_store()
        .unwrap()
        .create_item_at(Path::new("nested/private.bin"), b"private\0bytes", None)
        .unwrap();
    let exported = environment.root.join("portable-export");
    NativeTransferService::export(Domain::Vault, &exported, ExportProtection::Preserve).unwrap();
    let source_stored = std::fs::read(exported.join("nested/private.bin")).unwrap();
    assert!(matches!(
        protection::inspect_bytes(&source_stored).unwrap(),
        StoredFormat::Portable {
            domain: Domain::Vault
        }
    ));
    environment.select("target");
    environment.vault_protected("different target password");
    let preview =
        NativeTransferService::prepare(Domain::Vault, &exported, TransferMode::Add).unwrap();
    assert!(preview
        .requirements
        .iter()
        .any(|r| matches!(r, TransferRequirement::SourcePassword { .. })));
    assert!(preview.unlock_source("wrong password").is_err());
    assert_eq!(
        std::fs::read(exported.join("nested/private.bin")).unwrap(),
        source_stored
    );
    preview.unlock_source("source password").unwrap();
    NativeTransferService::prepare(Domain::Vault, &exported, TransferMode::Add)
        .unwrap()
        .commit()
        .unwrap();
    let store = vault_store().unwrap();
    let imported = store.list_items().unwrap();
    assert_eq!(imported[0].id, source_item.id);
    let target_stored = std::fs::read(&imported[0].path).unwrap();
    assert_ne!(target_stored, source_stored);
    assert!(!target_stored
        .windows(b"private\0bytes".len())
        .any(|window| window == b"private\0bytes"));
    protection::lock().unwrap();
    protection::unlock("different target password").unwrap();
    assert_eq!(store.read_item(imported[0].id).unwrap(), b"private\0bytes");
    assert_eq!(
        std::fs::read_dir(exported).unwrap().count(),
        2,
        "only transfer metadata and nested native bodies are exported"
    );
}

#[test]
fn encrypted_source_requires_target_protection_and_same_root_adoption_survives_lock() {
    let environment = Environment::new();
    environment.vault_protected("source password");
    let envelope = protection::encode(Domain::Vault, b"copied encrypted body").unwrap();
    environment.select("copied-target");
    environment.plain();
    let native = native_root(Domain::Vault).unwrap();
    protection::write_new_plain(&native.join("copied.bin"), &envelope).unwrap();
    let preview =
        NativeTransferService::prepare(Domain::Vault, &native, TransferMode::Add).unwrap();
    assert!(preview.same_native_root);
    assert!(preview.requirements.iter().any(|r| matches!(
        r,
        TransferRequirement::EnableProtection {
            domain: Domain::Vault
        }
    )));
    preview.unlock_source("source password").unwrap();
    let preview =
        NativeTransferService::prepare(Domain::Vault, &native, TransferMode::Add).unwrap();
    NativeTransferService::adopt_native(preview, "new target password").unwrap();
    protection::lock().unwrap();
    protection::unlock("new target password").unwrap();
    let store = vault_store().unwrap();
    let items = store.list_items().unwrap();
    assert_eq!(
        store.read_item(items[0].id).unwrap(),
        b"copied encrypted body"
    );
    assert!(protection::is_protected(Domain::Vault).unwrap());
}

#[test]
fn corrupt_portable_content_and_foreign_legacy_data_never_become_plaintext() {
    let environment = Environment::new();
    environment.vault_protected("source password");
    let mut envelope = protection::encode(Domain::Vault, b"authenticated body").unwrap();
    *envelope.last_mut().unwrap() ^= 1;
    environment.select("target");
    environment.plain();
    let source = environment.source("corrupt.bin", &envelope);
    let preview =
        NativeTransferService::prepare(Domain::Vault, &source, TransferMode::Add).unwrap();
    assert!(preview.unlock_source("source password").is_err());
    assert!(preview.commit().is_err());
    let legacy = environment.source(
        "legacy.bin",
        b"TCUI-PROTECTED\x01opaque legacy ciphertext bytes",
    );
    let preview =
        NativeTransferService::prepare(Domain::Vault, &legacy, TransferMode::Add).unwrap();
    assert!(preview
        .requirements
        .iter()
        .any(|r| matches!(r, TransferRequirement::LegacySourceWrapping { .. })));
    assert!(preview.commit().is_err());
    assert_eq!(std::fs::read(source).unwrap(), envelope);
}

#[cfg(unix)]
#[test]
fn symlink_special_file_and_parent_paths_are_rejected_without_touching_sentinels() {
    use std::os::unix::fs::symlink;
    let environment = Environment::new();
    environment.plain();
    let sentinel = environment.source("sentinel.txt", b"do not touch");
    let folder = environment.root.join("hostile");
    std::fs::create_dir(&folder).unwrap();
    symlink(&sentinel, folder.join("link.txt")).unwrap();
    assert!(NativeTransferService::prepare(Domain::Vault, &folder, TransferMode::Add).is_err());
    assert!(NativeTransferService::prepare(
        Domain::Vault,
        &folder.join("../sources/sentinel.txt"),
        TransferMode::Add
    )
    .is_err());
    std::fs::remove_file(folder.join("link.txt")).unwrap();
    let directory = protection::open_directory(&folder, false).unwrap();
    rustix::fs::mknodat(
        &directory,
        "pipe",
        rustix::fs::FileType::Fifo,
        rustix::fs::Mode::from_bits_truncate(0o600),
        0,
    )
    .unwrap();
    assert!(NativeTransferService::prepare(Domain::Vault, &folder, TransferMode::Add).is_err());
    assert_eq!(std::fs::read(sentinel).unwrap(), b"do not touch");
}

#[test]
fn plaintext_export_requires_explicit_action_and_never_overwrites_an_existing_file() {
    let environment = Environment::new();
    environment.vault_protected("target password");
    vault_store()
        .unwrap()
        .create_item("private.bin", b"plain export body", None)
        .unwrap();
    let output = environment.root.join("confirmed-plaintext");
    NativeTransferService::export(Domain::Vault, &output, ExportProtection::PlaintextConfirmed)
        .unwrap();
    assert_eq!(
        std::fs::read(output.join("private.bin")).unwrap(),
        b"plain export body"
    );
    assert!(NativeTransferService::export(
        Domain::Vault,
        &output,
        ExportProtection::PlaintextConfirmed,
    )
    .is_err());
    assert_eq!(
        std::fs::read(output.join("private.bin")).unwrap(),
        b"plain export body"
    );
    assert!(protection::is_protected(Domain::Vault).unwrap());
}

#[cfg(feature = "memory")]
#[test]
fn native_memories_import_keeps_foreign_machine_metadata_and_conflicting_records() {
    let environment = Environment::new();
    environment.plain();
    let document = serde_json::json!({
        "schema_version":1,"id":7,"logical_path":"nested/source.md","title":"Source fact",
        "created_at_ms":10,"updated_at_ms":20,"markdown":"# Source fact\n\nStored source content.\n",
        "metadata":{"scope":"machine","machine":{"id":"source-machine","name":"Source laptop"},
            "tags":["source"],"source":{"conversation_id":12,"conversation_title":"Source conversation"}}
    });
    let bytes = serde_json::to_vec(&document).unwrap();
    let source = environment.source("source.tcui-memory", &bytes);
    NativeTransferService::prepare(Domain::Memories, &source, TransferMode::Add)
        .unwrap()
        .commit()
        .unwrap();
    NativeTransferService::prepare(Domain::Memories, &source, TransferMode::Add)
        .unwrap()
        .commit()
        .unwrap();
    let store = MemoryStore::open_default().unwrap();
    let entries = store.list_entries().unwrap();
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().any(|entry| entry.id == 7));
    assert_ne!(entries[0].id, entries[1].id);
    assert_ne!(entries[0].logical_path, entries[1].logical_path);
    for entry in entries {
        let exported: serde_json::Value =
            serde_json::from_slice(&store.export_document(entry.id).unwrap()).unwrap();
        assert_eq!(exported["metadata"], document["metadata"]);
        assert_eq!(exported["markdown"], document["markdown"]);
    }
    assert_eq!(std::fs::read(source).unwrap(), bytes);
}

#[test]
fn transfer_metadata_cannot_traverse_or_rebind_a_retired_vault_identity() {
    let environment = Environment::new();
    environment.plain();
    let store = vault_store().unwrap();
    let old = store.create_item("old.bin", b"old", None).unwrap();
    store.trash_item(old.id).unwrap();
    let source = environment.source("native/new.bin", b"new");
    let folder = source.parent().unwrap();
    let metadata = folder.join(METADATA);
    protection::write_new_plain(
        &metadata,
        &serde_json::to_vec(&serde_json::json!({
            "version":1,"domain":"Vault","items":[{"path":"../outside","id":old.id.get()}]
        }))
        .unwrap(),
    )
    .unwrap();
    assert!(NativeTransferService::prepare(Domain::Vault, folder, TransferMode::Add).is_err());
    protection::write_plain(
        &metadata,
        &serde_json::to_vec(&serde_json::json!({
            "version":1,"domain":"Vault","items":[{"path":"new.bin","id":old.id.get()}]
        }))
        .unwrap(),
    )
    .unwrap();
    NativeTransferService::prepare(Domain::Vault, folder, TransferMode::Add)
        .unwrap()
        .commit()
        .unwrap();
    let imported = store.list_items().unwrap();
    assert!(imported[0].id > old.id);
    assert!(store.read_item(old.id).is_err());
}

#[cfg(feature = "memory")]
#[test]
fn encrypted_memory_transfer_preserves_native_metadata_across_instance_passwords() {
    let environment = Environment::new();
    protection::configure(
        ProtectionPolicy {
            memories: true,
            ..ProtectionPolicy::default()
        },
        "source memory password",
    )
    .unwrap();
    let document = serde_json::json!({
        "schema_version":1,"id":123,"logical_path":"nested/source.md","title":"Source",
        "created_at_ms":10,"updated_at_ms":20,"markdown":"# Source\n\nPortable native memory.\n",
        "metadata":{"scope":"machine","machine":{"id":"foreign-machine","name":"Foreign laptop"},"tags":["portable"],"source":null}
    });
    MemoryStore::open_default()
        .unwrap()
        .import_document(&serde_json::to_vec(&document).unwrap())
        .unwrap();
    let exported = environment.root.join("memory-export");
    NativeTransferService::export(Domain::Memories, &exported, ExportProtection::Preserve).unwrap();
    let original = std::fs::read(exported.join("000000000000007b.tcui-memory")).unwrap();
    environment.select("memory-target");
    protection::configure(
        ProtectionPolicy {
            memories: true,
            ..ProtectionPolicy::default()
        },
        "target memory password",
    )
    .unwrap();
    let preview =
        NativeTransferService::prepare(Domain::Memories, &exported, TransferMode::Add).unwrap();
    preview.unlock_source("source memory password").unwrap();
    NativeTransferService::prepare(Domain::Memories, &exported, TransferMode::Add)
        .unwrap()
        .commit()
        .unwrap();
    protection::lock().unwrap();
    protection::unlock("target memory password").unwrap();
    let store = MemoryStore::open_default().unwrap();
    let imported: serde_json::Value =
        serde_json::from_slice(&store.export_document(123).unwrap()).unwrap();
    assert_eq!(imported, document);
    assert_eq!(
        std::fs::read(exported.join("000000000000007b.tcui-memory")).unwrap(),
        original
    );
    assert!(matches!(
        protection::inspect_bytes(
            &std::fs::read(store.root_path().join("000000000000007b.tcui-memory")).unwrap()
        )
        .unwrap(),
        StoredFormat::Portable {
            domain: Domain::Memories
        }
    ));
}

#[test]
fn completed_replace_can_restore_original_identity_without_discarding_new_data() {
    let environment = Environment::new();
    environment.plain();
    let store = vault_store().unwrap();
    let original = store.create_item("old.bin", b"original", Some(81)).unwrap();
    let source = environment.source("new.bin", b"new data");
    let report = NativeTransferService::prepare(Domain::Vault, &source, TransferMode::Replace)
        .unwrap()
        .commit()
        .unwrap();
    let backup = report.backup.unwrap();
    let added = store
        .create_item("later.md", b"later linked content", Some(129))
        .unwrap();
    let displaced_manifest =
        std::fs::read(TcuiDataPaths::discover().chat_file_links_manifest).unwrap();
    NativeTransferService::restore_backup(Domain::Vault, &backup).unwrap();
    let restored = store.list_items().unwrap();
    assert_eq!(restored[0].id, original.id);
    assert_eq!(restored[0].chat_links, original.chat_links);
    assert_eq!(store.read_item(original.id).unwrap(), b"original");
    let retained = std::fs::read_dir(&backup)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("interrupted-")
        })
        .unwrap();
    assert_eq!(
        std::fs::read(retained.join("new.bin")).unwrap(),
        b"new data"
    );
    let reference = std::fs::read_dir(&backup)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("displaced-")
        })
        .unwrap();
    let displaced_id: String = serde_json::from_slice(&std::fs::read(reference).unwrap()).unwrap();
    let displaced = backup.parent().unwrap().join(displaced_id);
    assert_eq!(
        std::fs::read(displaced.join("auxiliary")).unwrap(),
        displaced_manifest
    );
    assert_eq!(
        std::fs::read(displaced.join("root/later.md")).unwrap(),
        b"later linked content"
    );
    NativeTransferService::restore_backup(Domain::Vault, &displaced).unwrap();
    let returned = store.list_items().unwrap();
    let added_again = returned.iter().find(|item| item.id == added.id).unwrap();
    assert_eq!(added_again.chat_links, added.chat_links);
    assert_eq!(store.read_item(added.id).unwrap(), b"later linked content");
}

#[test]
fn changed_recovery_snapshot_never_replaces_current_native_data() {
    let environment = Environment::new();
    environment.plain();
    let store = vault_store().unwrap();
    let original = store.create_item("old.bin", b"original", None).unwrap();
    let backup = {
        let _exclusive = filesystem::exclusive().unwrap();
        filesystem::snapshot(Domain::Vault).unwrap()
    };
    std::fs::write(backup.join("root/old.bin"), b"corrupt backup").unwrap();
    assert!(NativeTransferService::recover_pending(Domain::Vault).is_err());
    assert_eq!(store.read_item(original.id).unwrap(), b"original");
    assert!(filesystem::pending(Domain::Vault).unwrap().is_some());
}

#[test]
fn security_transition_encrypts_retained_backup_and_rewrites_recovery_hashes_atomically() {
    let environment = Environment::new();
    environment.plain();
    let store = vault_store().unwrap();
    let original = store
        .create_item("old.bin", b"historical private body", Some(31))
        .unwrap();
    let source = environment.source("replacement.bin", b"replacement");
    let backup = NativeTransferService::prepare(Domain::Vault, &source, TransferMode::Replace)
        .unwrap()
        .commit()
        .unwrap()
        .backup
        .unwrap();
    environment.vault_protected("target password");
    let body = std::fs::read(backup.join("root/old.bin")).unwrap();
    assert!(matches!(
        protection::inspect_bytes(&body).unwrap(),
        StoredFormat::Portable {
            domain: Domain::Vault
        }
    ));
    assert!(!body
        .windows(b"historical private body".len())
        .any(|window| window == b"historical private body"));
    let descriptor = protection::read(Domain::Vault, &backup.join("recovery.json")).unwrap();
    let descriptor: serde_json::Value = serde_json::from_slice(&descriptor).unwrap();
    assert_eq!(descriptor["protected"], true);
    let expected: Vec<u8> = hash(&body).to_vec();
    assert_eq!(
        descriptor["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["path"] == "root/old.bin")
            .unwrap()["hash"],
        serde_json::to_value(expected).unwrap()
    );
    protection::lock().unwrap();
    protection::unlock("target password").unwrap();
    NativeTransferService::restore_backup(Domain::Vault, &backup).unwrap();
    assert_eq!(
        store.read_item(original.id).unwrap(),
        b"historical private body"
    );
    assert_eq!(
        store.list_items().unwrap()[0].chat_links,
        original.chat_links
    );
}

#[test]
fn password_rotation_rewraps_retained_native_history_and_preserves_restore_metadata() {
    let environment = Environment::new();
    environment.vault_protected("old history password");
    let store = vault_store().unwrap();
    let original = store
        .create_item("old.bin", b"historical body", Some(31))
        .unwrap();
    let source = environment.source("replacement.bin", b"replacement");
    let backup = NativeTransferService::prepare(Domain::Vault, &source, TransferMode::Replace)
        .unwrap()
        .commit()
        .unwrap()
        .backup
        .unwrap();
    protection::change_password("new history password").unwrap();
    for relative in ["root/old.bin", "auxiliary", "recovery.json"] {
        let stored = std::fs::read(backup.join(relative)).unwrap();
        assert!(
            protection::unlock_foreign_file(Domain::Vault, &stored, "old history password")
                .is_err()
        );
        protection::unlock_foreign_file(Domain::Vault, &stored, "new history password").unwrap();
    }
    let descriptor: serde_json::Value = serde_json::from_slice(
        &protection::read(Domain::Vault, &backup.join("recovery.json")).unwrap(),
    )
    .unwrap();
    let body = std::fs::read(backup.join("root/old.bin")).unwrap();
    assert_eq!(
        descriptor["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["path"] == "root/old.bin")
            .unwrap()["hash"],
        serde_json::to_value(hash(&body)).unwrap()
    );
    protection::lock().unwrap();
    assert!(protection::unlock("old history password").is_err());
    protection::unlock("new history password").unwrap();
    NativeTransferService::restore_backup(Domain::Vault, &backup).unwrap();
    assert_eq!(store.read_item(original.id).unwrap(), b"historical body");
    assert_eq!(
        store.list_items().unwrap()[0].chat_links,
        original.chat_links
    );
}

#[test]
fn password_rotation_keeps_plaintext_retained_history_byte_identical_and_restorable() {
    let environment = Environment::new();
    environment.plain();
    let store = vault_store().unwrap();
    let original = store
        .create_item("old.bin", b"plaintext history", Some(47))
        .unwrap();
    let source = environment.source("replacement.bin", b"replacement");
    let backup = NativeTransferService::prepare(Domain::Vault, &source, TransferMode::Replace)
        .unwrap()
        .commit()
        .unwrap()
        .backup
        .unwrap();
    let originals: Vec<_> = ["root/old.bin", "auxiliary", "recovery.json"]
        .into_iter()
        .map(|relative| (relative, std::fs::read(backup.join(relative)).unwrap()))
        .collect();
    protection::initialize_password("old plaintext history password").unwrap();
    protection::change_password("new plaintext history password").unwrap();
    for (relative, original) in originals {
        assert_eq!(std::fs::read(backup.join(relative)).unwrap(), original);
    }
    NativeTransferService::restore_backup(Domain::Vault, &backup).unwrap();
    assert_eq!(store.read_item(original.id).unwrap(), b"plaintext history");
    assert_eq!(
        store.list_items().unwrap()[0].chat_links,
        original.chat_links
    );
}

#[test]
fn pending_native_replacement_refuses_policy_change_before_any_source_or_key_change() {
    let environment = Environment::new();
    environment.plain();
    protection::initialize_password("new password").unwrap();
    let store = vault_store().unwrap();
    let item = store
        .create_item("unchanged.bin", b"unchanged", None)
        .unwrap();
    {
        let _exclusive = filesystem::exclusive().unwrap();
        filesystem::snapshot(Domain::Vault).unwrap();
    }
    assert!(protection::change_password("replacement password").is_err());
    assert!(protection::configure(
        ProtectionPolicy {
            vault: true,
            ..ProtectionPolicy::default()
        },
        "new password"
    )
    .is_err());
    assert!(!protection::is_protected(Domain::Vault).unwrap());
    assert_eq!(std::fs::read(&item.path).unwrap(), b"unchanged");
    NativeTransferService::recover_pending(Domain::Vault).unwrap();
    environment.vault_protected("new password");
    assert_eq!(store.read_item(item.id).unwrap(), b"unchanged");
}

#[test]
fn trash_only_adoption_lists_every_source_wrapper_and_keeps_trash_out_of_add() {
    let environment = Environment::new();
    environment.vault_protected("first source password");
    let first = protection::encode(Domain::Vault, b"first archived body").unwrap();
    environment.select("second-source");
    environment.vault_protected("second source password");
    let second = protection::encode(Domain::Vault, b"second archived body").unwrap();
    let external = environment.source("external/.Trash/first.bin", &first);
    let external_root = external.parent().unwrap().parent().unwrap();
    assert!(
        NativeTransferService::prepare(Domain::Vault, external_root, TransferMode::Add).is_err()
    );
    environment.select("trash-only-target");
    environment.plain();
    protection::initialize_password("target password").unwrap();
    let native = native_root(Domain::Vault).unwrap();
    protection::write_new_plain(&native.join(".Trash/first.bin"), &first).unwrap();
    protection::write_new_plain(&native.join(".Trash/second.bin"), &second).unwrap();
    let preview =
        NativeTransferService::prepare(Domain::Vault, &native, TransferMode::Add).unwrap();
    assert!(preview.files.is_empty());
    assert_eq!(preview.adoption_files.len(), 2);
    assert_eq!(
        preview
            .requirements
            .iter()
            .filter(|requirement| matches!(requirement, TransferRequirement::SourcePassword { .. }))
            .count(),
        2
    );
    preview.unlock_source("first source password").unwrap();
    let preview =
        NativeTransferService::prepare(Domain::Vault, &native, TransferMode::Add).unwrap();
    assert_eq!(
        preview
            .requirements
            .iter()
            .filter(|requirement| matches!(requirement, TransferRequirement::SourcePassword { .. }))
            .count(),
        1
    );
    preview.unlock_source("second source password").unwrap();
    let preview =
        NativeTransferService::prepare(Domain::Vault, &native, TransferMode::Add).unwrap();
    NativeTransferService::adopt_native_unlocked(preview).unwrap();
    protection::lock().unwrap();
    protection::unlock("target password").unwrap();
    assert_eq!(
        protection::read(Domain::Vault, &native.join(".Trash/first.bin")).unwrap(),
        b"first archived body"
    );
    assert_eq!(
        protection::read(Domain::Vault, &native.join(".Trash/second.bin")).unwrap(),
        b"second archived body"
    );
    assert!(vault_store().unwrap().list_items().unwrap().is_empty());
}

#[cfg(feature = "memory")]
#[test]
fn copied_memories_with_only_encrypted_trash_can_be_authenticated_and_adopted() {
    let environment = Environment::new();
    protection::configure(
        ProtectionPolicy {
            memories: true,
            ..ProtectionPolicy::default()
        },
        "source password",
    )
    .unwrap();
    let document = serde_json::to_vec(&serde_json::json!({
        "schema_version":1,"id":88,"logical_path":"archived.md","title":"Archived",
        "created_at_ms":1,"updated_at_ms":2,"markdown":"# Archived\n\nAn archived fact.\n","metadata":null
    })).unwrap();
    let source = protection::encode(Domain::Memories, &document).unwrap();
    environment.select("memory-trash-target");
    environment.plain();
    let native = native_root(Domain::Memories).unwrap();
    let archived = native.join(".trash/0000000000000058.tcui-memory");
    protection::write_new_plain(&archived, &source).unwrap();
    let preview =
        NativeTransferService::prepare(Domain::Memories, &native, TransferMode::Add).unwrap();
    assert!(preview.files.is_empty());
    assert_eq!(preview.adoption_files.len(), 1);
    preview.unlock_source("source password").unwrap();
    let preview =
        NativeTransferService::prepare(Domain::Memories, &native, TransferMode::Add).unwrap();
    NativeTransferService::adopt_native(preview, "target password").unwrap();
    protection::lock().unwrap();
    protection::unlock("target password").unwrap();
    assert_eq!(
        protection::read(Domain::Memories, &archived).unwrap(),
        document
    );
    assert!(MemoryStore::open_default()
        .unwrap()
        .list_entries()
        .unwrap()
        .is_empty());
}

#[test]
fn backup_policy_roundtrip_preserves_opaque_v1_prefix_bytes_without_double_decoding() {
    let environment = Environment::new();
    environment.plain();
    let opaque =
        b"TCUI-PROTECTED\x01this is literal saved binary content, not an authenticated envelope";
    let store = vault_store().unwrap();
    let original = store.create_item("opaque.bin", opaque, None).unwrap();
    let source = environment.source("replacement.bin", b"replacement");
    let backup = NativeTransferService::prepare(Domain::Vault, &source, TransferMode::Replace)
        .unwrap()
        .commit()
        .unwrap()
        .backup
        .unwrap();
    environment.vault_protected("target password");
    let stored = std::fs::read(backup.join("root/opaque.bin")).unwrap();
    assert_eq!(
        protection::decode_for_explicit_decryption(Domain::Vault, &stored).unwrap(),
        opaque
    );
    protection::configure(ProtectionPolicy::default(), "target password").unwrap();
    assert_eq!(
        std::fs::read(backup.join("root/opaque.bin")).unwrap(),
        opaque
    );
    NativeTransferService::restore_backup(Domain::Vault, &backup).unwrap();
    assert_eq!(store.read_item(original.id).unwrap(), opaque);
}

#[test]
fn unrelated_security_change_leaves_copied_encrypted_history_unchanged() {
    let environment = Environment::new();
    environment.vault_protected("source password");
    vault_store()
        .unwrap()
        .create_item("historical.bin", b"source history", None)
        .unwrap();
    let source = environment.source("replacement.bin", b"replacement");
    let backup = NativeTransferService::prepare(Domain::Vault, &source, TransferMode::Replace)
        .unwrap()
        .commit()
        .unwrap()
        .backup
        .unwrap();
    let id = backup.file_name().unwrap().to_os_string();
    let retained: Vec<_> = filesystem::scan(&backup, false)
        .unwrap()
        .into_iter()
        .map(|file| (file.relative, filesystem::read_file(&file.path).unwrap()))
        .collect();
    environment.select("target-unrelated-policy");
    environment.plain();
    let copied = protection::root()
        .unwrap()
        .join(".native-transfer-backups")
        .join(id);
    for (path, bytes) in &retained {
        protection::write_new_plain(&copied.join(path), bytes).unwrap();
    }
    protection::configure(
        ProtectionPolicy {
            app_password: true,
            ..ProtectionPolicy::default()
        },
        "target password",
    )
    .unwrap();
    assert!(!protection::is_protected(Domain::Vault).unwrap());
    for (path, original) in retained {
        assert_eq!(std::fs::read(copied.join(path)).unwrap(), original);
    }
}

#[test]
fn format_only_upgrade_leaves_foreign_portable_backup_without_requesting_its_password() {
    let environment = Environment::new();
    environment.vault_protected("foreign history password");
    vault_store()
        .unwrap()
        .create_item("foreign.bin", b"foreign retained body", None)
        .unwrap();
    let source = environment.source("replacement.bin", b"replacement");
    let backup = NativeTransferService::prepare(Domain::Vault, &source, TransferMode::Replace)
        .unwrap()
        .commit()
        .unwrap()
        .backup
        .unwrap();
    let id = backup.file_name().unwrap().to_os_string();
    let retained: Vec<_> = filesystem::scan(&backup, false)
        .unwrap()
        .into_iter()
        .map(|file| (file.relative, filesystem::read_file(&file.path).unwrap()))
        .collect();
    environment.select("format-upgrade-target");
    environment.vault_protected("different target password");
    let copied = protection::root()
        .unwrap()
        .join(".native-transfer-backups")
        .join(id);
    for (path, bytes) in &retained {
        protection::write_new_plain(&copied.join(path), bytes).unwrap();
    }
    stage_protection_migration(
        None,
        protection::policy().unwrap().unwrap(),
        true,
        |_, _| panic!("already-portable history must not be reencoded"),
        |_, _, _, _| panic!("already-portable history must not be staged"),
    )
    .unwrap();
    for (path, original) in retained {
        assert_eq!(std::fs::read(copied.join(path)).unwrap(), original);
    }
}

#[test]
fn native_source_budget_allows_accumulated_bytes_and_files_above_external_import_caps() {
    let environment = Environment::new();
    environment.plain();
    let source = environment.root.join("accumulated-native");
    // Small limits exercise the same streaming scanner without allocating or
    // reading hundreds of MiB only to cross the production external limits.
    for index in 0..6 {
        protection::write_new_plain(&source.join(format!("{index}.bin")), &[index; 53]).unwrap();
    }
    assert!(filesystem::scan_with_limits(&source, true, 64, Some(256)).is_err());
    let native = filesystem::scan_with_limits(&source, false, 1024, None).unwrap();
    assert_eq!(native.len(), 6);
    assert_eq!(native.iter().map(|file| file.size).sum::<u64>(), 318);
    for file in native {
        assert_eq!(file.hash, hash(&std::fs::read(file.path).unwrap()));
    }
    protection::write_new_plain(&source.join("larger-native.bin"), &[7; 65]).unwrap();
    assert!(filesystem::scan_with_limits(&source, true, 64, None).is_err());
    assert_eq!(
        filesystem::scan_with_limits(&source, false, 1024, None)
            .unwrap()
            .len(),
        7
    );
}

#[test]
fn unchanged_protected_domain_does_not_decrypt_foreign_history_for_entry_password_change() {
    let environment = Environment::new();
    environment.vault_protected("source history password");
    vault_store()
        .unwrap()
        .create_item("historical.bin", b"foreign protected history", None)
        .unwrap();
    let source = environment.source("replacement.bin", b"replacement");
    let backup = NativeTransferService::prepare(Domain::Vault, &source, TransferMode::Replace)
        .unwrap()
        .commit()
        .unwrap()
        .backup
        .unwrap();
    let id = backup.file_name().unwrap().to_os_string();
    let retained: Vec<_> = filesystem::scan(&backup, false)
        .unwrap()
        .into_iter()
        .map(|file| (file.relative, filesystem::read_file(&file.path).unwrap()))
        .collect();
    environment.select("already-protected-target");
    environment.vault_protected("different target password");
    let copied = protection::root()
        .unwrap()
        .join(".native-transfer-backups")
        .join(id);
    for (path, bytes) in &retained {
        protection::write_new_plain(&copied.join(path), bytes).unwrap();
    }
    protection::configure(
        ProtectionPolicy {
            vault: true,
            app_password: true,
            ..ProtectionPolicy::default()
        },
        "different target password",
    )
    .unwrap();
    assert!(protection::is_protected(Domain::Vault).unwrap());
    for (path, original) in retained {
        assert_eq!(std::fs::read(copied.join(path)).unwrap(), original);
    }
}

#[test]
fn native_snapshot_and_recovery_use_stored_file_bounds_without_external_total_cap() {
    let environment = Environment::new();
    const EXTERNAL_FILE_LIMIT: u64 = 4096;
    for protected in [false, true] {
        for offset in [-1i64, 0, 1] {
            environment.select(&format!("snapshot-boundary-{protected}-{offset}"));
            if protected {
                environment.vault_protected("snapshot password");
            } else {
                environment.plain();
            }
            let envelope_bytes = protection::encode(Domain::Vault, b"").unwrap().len() as u64;
            let native_limit = EXTERNAL_FILE_LIMIT + envelope_bytes;
            let payload_len = (EXTERNAL_FILE_LIMIT as i64 + offset) as usize;
            let store = vault_store().unwrap();
            let mut originals = Vec::new();
            for index in 0..5 {
                let item = store
                    .create_item(
                        &format!("original-{index}.bin"),
                        &vec![index as u8; payload_len],
                        Some(100 + index),
                    )
                    .unwrap();
                let stored = std::fs::read(&item.path).unwrap();
                assert_eq!(stored.len() as i64, native_limit as i64 + offset);
                originals.push((item, stored));
            }
            let native = native_root(Domain::Vault).unwrap();
            let manifest_path = TcuiDataPaths::discover().chat_file_links_manifest;
            let manifest = std::fs::read(&manifest_path).unwrap();
            let _exclusive = filesystem::exclusive().unwrap();
            let snapshot = filesystem::snapshot_with_limit(Domain::Vault, true, native_limit);
            if offset > 0 {
                assert!(snapshot.is_err());
                assert!(filesystem::pending(Domain::Vault).unwrap().is_none());
                assert_eq!(std::fs::read(&manifest_path).unwrap(), manifest);
                for (item, stored) in originals {
                    assert_eq!(std::fs::read(item.path).unwrap(), stored);
                }
                continue;
            }
            let backup = snapshot.unwrap();
            // The same small fixture exceeds the external cumulative allowance.
            assert!(filesystem::scan_with_limits(
                &native,
                false,
                native_limit,
                Some(4 * native_limit),
            )
            .is_err());
            if protected && offset == 0 {
                // A maximum-sized plaintext payload gains portable envelope bytes.
                assert!(
                    filesystem::scan_with_limits(&native, false, EXTERNAL_FILE_LIMIT, None,)
                        .is_err()
                );
            }
            for (item, stored) in &originals {
                assert_eq!(
                    std::fs::read(backup.join("root").join(&item.relative_path)).unwrap(),
                    *stored,
                );
                store.trash_item(item.id).unwrap();
            }
            store
                .create_item("replacement.bin", b"new data", None)
                .unwrap();
            let displaced_manifest = std::fs::read(&manifest_path).unwrap();
            // Reject an oversized retained file before touching current native paths.
            let stored_size = originals[0].1.len() as u64;
            assert!(filesystem::recover_with_limit(Domain::Vault, stored_size - 1).is_err());
            assert_eq!(std::fs::read(&manifest_path).unwrap(), displaced_manifest);
            assert_eq!(
                filesystem::recover_with_limit(Domain::Vault, native_limit).unwrap(),
                Some(backup),
            );
            assert_eq!(std::fs::read(&manifest_path).unwrap(), manifest);
            let restored = store.list_items().unwrap();
            assert_eq!(restored.len(), originals.len());
            for (index, (item, stored)) in originals.into_iter().enumerate() {
                let record = restored.iter().find(|record| record.id == item.id).unwrap();
                assert_eq!(record.chat_links, item.chat_links);
                assert_eq!(std::fs::read(&record.path).unwrap(), stored);
                assert_eq!(
                    store.read_item(item.id).unwrap(),
                    vec![index as u8; payload_len],
                );
            }
            assert!(filesystem::pending(Domain::Vault).unwrap().is_none());
        }
    }
}

#[test]
fn streamed_native_copy_rejects_changed_hash_limits_links_and_existing_destinations() {
    use std::os::unix::fs::symlink;
    let environment = Environment::new();
    environment.plain();
    let source = environment.source("stream/source.bin", b"stored native bytes");
    let destination = source.with_file_name("copy.bin");
    let sentinel = environment.source("stream/sentinel.bin", b"unchanged sentinel");
    let expected = hash(b"stored native bytes");
    let limit = b"stored native bytes".len() as u64;
    assert!(filesystem::copy_verified(&source, &destination, expected, limit - 1, false).is_err());
    assert!(
        filesystem::copy_verified(&source, &destination, hash(b"changed"), limit, false).is_err()
    );
    assert!(!destination.exists());
    filesystem::copy_verified(&source, &destination, expected, limit, false).unwrap();
    assert_eq!(std::fs::read(&destination).unwrap(), b"stored native bytes");
    assert!(filesystem::copy_verified(
        &sentinel,
        &destination,
        hash(b"unchanged sentinel"),
        limit,
        false,
    )
    .is_err());
    let source_link = source.with_file_name("source-link.bin");
    symlink(&source, &source_link).unwrap();
    assert!(filesystem::copy_verified(&source_link, &sentinel, expected, limit, true).is_err());
    let target_link = source.with_file_name("target-link.bin");
    symlink(&sentinel, &target_link).unwrap();
    assert!(filesystem::copy_verified(&source, &target_link, expected, limit, true).is_err());
    assert_eq!(std::fs::read(&sentinel).unwrap(), b"unchanged sentinel");
    assert_eq!(std::fs::read(&destination).unwrap(), b"stored native bytes");
}
