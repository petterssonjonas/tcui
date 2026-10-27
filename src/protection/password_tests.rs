use super::*;
use crate::config::key_store::{CredentialStorage, PasswordChange, SecretKind};

fn prepared_rotation(password: &str) -> (PathBuf, PasswordChange) {
    let old = load_manifest().expect("manifest").expect("configured");
    let key = KeyStore::protection_key(false).expect("authenticated master");
    let target = Manifest::new(old.policy, Some(&key)).expect("same authenticated policy");
    let change = KeyStore::prepare_password_change(password).expect("prepare credential cutover");
    let copy = serde_json::from_slice(&serde_json::to_vec(&change).expect("ciphertext plan"))
        .expect("copy ciphertext plan");
    migration::prepare_password_change(Some(&old), &target, &key, change)
        .expect("durable rotation plan");
    let pending: serde_json::Value = serde_json::from_slice(
        &io::read_regular(&root().expect("root").join("protection-pending.json"))
            .expect("pending marker"),
    )
    .expect("pending metadata");
    (
        root()
            .expect("root")
            .join(pending["stage"].as_str().expect("stage name")),
        copy,
    )
}

fn staged_target(stage: &Path, relative: &str) -> Vec<u8> {
    let key = KeyStore::protection_key(false).expect("master");
    let journal = Zeroizing::new(
        open(
            b"tcui/protection/v1/migration-journal",
            &io::read_regular(&stage.join("journal")).expect("sealed journal"),
            &key,
        )
        .expect("authenticate journal"),
    );
    let journal: serde_json::Value = serde_json::from_slice(&journal).expect("journal");
    let number = journal["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .position(|entry| entry["relative"] == relative)
        .expect("staged source");
    io::read_regular(&stage.join(format!("{number}.new"))).expect("staged target")
}

#[test]
fn unlocked_policy_changes_reuse_the_shared_password_and_preserve_other_domains() {
    let environment = Environment::new();
    configure(
        ProtectionPolicy {
            credentials: true,
            ..ProtectionPolicy::default()
        },
        "shared password",
    )
    .expect("credential protection");
    let vault = internal_vault_root().join("existing.md");
    let memory = environment.data().join("memories/existing.tcui-memory");
    write(Domain::Vault, &vault, b"existing vault body").expect("plaintext vault");
    write(Domain::Memories, &memory, b"{\"body\":\"existing memory\"}").expect("plaintext memory");
    let memory_bytes = std::fs::read(&memory).expect("original memory");
    lock().expect("lock");
    assert!(configure_unlocked(all_protected()).is_err());
    unlock("shared password").expect("single shared unlock");
    let selected = ProtectionPolicy {
        credentials: true,
        vault: true,
        ..ProtectionPolicy::default()
    };
    configure_unlocked(selected).expect("add vault protection without a password");
    assert_eq!(policy().expect("policy"), Some(selected));
    assert_eq!(
        read(Domain::Vault, &vault).expect("vault data"),
        b"existing vault body"
    );
    assert_eq!(
        std::fs::read(&memory).expect("unchanged plaintext memory"),
        memory_bytes
    );
    lock().expect("lock after policy change");
    assert!(unlock("not the shared password").is_err());
    assert!(configure_unlocked(all_protected()).is_err());
    unlock("shared password").expect("existing password still opens both domains");
    assert_eq!(
        read(Domain::Vault, &vault).expect("vault after restart"),
        b"existing vault body"
    );
}

#[test]
fn unlocked_policy_changes_accept_a_legacy_credential_session_without_a_manifest() {
    let _environment = Environment::new();
    let config = AppConfig::default();
    assert!(configure_unlocked(ProtectionPolicy::default()).is_err());
    KeyStore::initialize_vault(&config, "legacy shared password").expect("legacy vault");
    KeyStore::set_with_storage(
        &config,
        "Legacy::encrypted",
        "retained key",
        CredentialStorage::Encrypted,
    )
    .expect("legacy provider");
    KeyStore::lock_vault(&config).expect("lock legacy vault");
    KeyStore::unlock_vault(&config, "legacy shared password")
        .expect("legacy credential-only unlock");
    assert!(!is_unlocked().expect("no protection session yet"));
    configure_unlocked(ProtectionPolicy {
        vault: true,
        ..ProtectionPolicy::default()
    })
    .expect("reuse authenticated legacy key");
    assert!(is_unlocked().expect("protection session established"));
    assert_eq!(
        KeyStore::get(&config, "Legacy::encrypted")
            .expect("retained provider")
            .as_deref(),
        Some("retained key")
    );
    lock().expect("restart");
    unlock("legacy shared password").expect("same password authenticates protection");
}

#[test]
fn password_rotation_preserves_mixed_credentials_labels_chats_and_every_managed_wrapper() {
    let environment = Environment::new();
    configure(all_protected(), "old shared password").expect("protected setup");
    let config = AppConfig::default();
    KeyStore::set_with_storage(
        &config,
        "Provider::encrypted",
        "encrypted provider",
        CredentialStorage::Encrypted,
    )
    .expect("encrypted connection");
    KeyStore::set_with_storage(
        &config,
        "Provider::plaintext",
        "explicit plaintext provider",
        CredentialStorage::Plaintext,
    )
    .expect("explicit plaintext connection");
    KeyStore::set_secret(
        &config,
        "stable-machine-id",
        "Retained machine label",
        SecretKind::Password,
        "machine secret",
    )
    .expect("machine secret");
    let labels = KeyStore::list_secrets(&config).expect("original labels and IDs");
    let storage = Storage::new().expect("chat store");
    let chat = storage
        .create_conversation_snapshot(0, "preserved chat title", &[])
        .expect("chat");
    drop(storage);
    let chat_path = environment
        .data()
        .join(format!("chats/{chat:016x}.tcui-chat"));
    let chat_bytes = std::fs::read(&chat_path).expect("chat ciphertext");
    let chat_document = read(Domain::Chats, &chat_path).expect("chat document");
    let fixtures = [
        (
            Domain::Vault,
            "vault/current.md",
            b"current vault".as_slice(),
        ),
        (
            Domain::Vault,
            "vault/.Trash/deleted.md",
            b"deleted vault".as_slice(),
        ),
        (
            Domain::Vault,
            "vault-backups/retained.bin",
            b"retained vault backup".as_slice(),
        ),
        (
            Domain::Memories,
            "memories/current.tcui-memory",
            b"current memory".as_slice(),
        ),
        (
            Domain::Memories,
            "memories/.trash/deleted.tcui-memory",
            b"deleted memory".as_slice(),
        ),
    ];
    for (domain, relative, body) in fixtures {
        write(domain, &environment.data().join(relative), body).expect("managed fixture");
    }
    let old_export =
        std::fs::read(environment.data().join("vault/current.md")).expect("old export");
    let export_path = environment.root.join("outside-managed-storage.tcui-vault");
    write_plain(&export_path, &old_export).expect("external export");
    change_password("new shared password").expect("rotate shared password");
    assert_eq!(policy().expect("unchanged policy"), Some(all_protected()));
    assert_eq!(
        KeyStore::list_secrets(&config).expect("same labels and IDs"),
        labels
    );
    assert_eq!(
        KeyStore::get_secret(&config, "stable-machine-id")
            .expect("same secret")
            .as_deref(),
        Some("machine secret")
    );
    assert_eq!(
        KeyStore::get(&config, "Provider::encrypted")
            .expect("encrypted provider retained")
            .as_deref(),
        Some("encrypted provider")
    );
    assert_eq!(
        KeyStore::get(&config, "Provider::plaintext")
            .expect("plaintext provider retained")
            .as_deref(),
        Some("explicit plaintext provider")
    );
    assert_eq!(
        KeyStore::provider_storage(&config, "Provider::plaintext").expect("explicit storage"),
        Some(CredentialStorage::Plaintext)
    );
    assert_eq!(
        std::fs::read(&chat_path).expect("identical chat bytes"),
        chat_bytes
    );
    assert_eq!(
        read(Domain::Chats, &chat_path).expect("intact chat"),
        chat_document
    );
    for (domain, relative, body) in fixtures {
        let path = environment.data().join(relative);
        let stored = std::fs::read(&path).expect("new portable envelope");
        assert!(
            unlock_foreign_file(domain, &stored, "old shared password").is_err(),
            "old password still opens {relative}"
        );
        unlock_foreign_file(domain, &stored, "new shared password")
            .expect("new password opens portable copy");
        assert_eq!(read(domain, &path).expect("retained managed body"), body);
    }
    assert_eq!(
        std::fs::read(export_path).expect("external export untouched"),
        old_export
    );
    lock().expect("restart");
    assert!(unlock("old shared password").is_err());
    assert_eq!(
        KeyStore::get(&config, "Provider::plaintext")
            .expect("plaintext provider stays available")
            .as_deref(),
        Some("explicit plaintext provider")
    );
    unlock("new shared password").expect("new password opens shared vault");
    assert_eq!(
        read(Domain::Chats, &chat_path).expect("chat after restart"),
        chat_document
    );
}

#[test]
fn password_rotation_preserves_plaintext_stores_with_or_without_a_protection_manifest() {
    for configured in [false, true] {
        let environment = Environment::new();
        if configured {
            configure(ProtectionPolicy::default(), "").expect("plaintext policy");
        }
        let config = AppConfig::default();
        KeyStore::initialize_vault(&config, "old plaintext-state password")
            .expect("shared password only");
        let original_storage = KeyStore::vault_status(&config)
            .expect("original default")
            .storage;
        KeyStore::set_with_storage(
            &config,
            "Plain",
            "plaintext provider",
            CredentialStorage::Plaintext,
        )
        .expect("plaintext connection");
        let path = environment.data().join("vault/plain.bin");
        let body = [MAGIC, b"literal prefix in arbitrary plaintext"].concat();
        write_plain(&path, &body).expect("plaintext store");
        KeyStore::lock_vault(&config).expect("legacy lock");
        unlock("old plaintext-state password").expect("unlocks a credential-only shared vault");
        change_password("new plaintext-state password")
            .expect("rotate initialized plaintext state");
        assert_eq!(
            policy().expect("same policy"),
            Some(ProtectionPolicy::default())
        );
        assert_eq!(std::fs::read(&path).expect("plaintext untouched"), body);
        assert_eq!(
            KeyStore::vault_status(&config)
                .expect("same default")
                .storage,
            original_storage
        );
        lock().expect("restart");
        assert!(!startup_requires_unlock().expect("no whole-app lock added"));
        assert_eq!(
            KeyStore::get(&config, "Plain")
                .expect("plaintext still available")
                .as_deref(),
            Some("plaintext provider")
        );
        assert!(unlock("old plaintext-state password").is_err());
        unlock("new plaintext-state password").expect("new shared password");
    }
}

#[test]
fn password_rotation_rejects_locked_invalid_unchanged_and_busy_requests_without_cutover() {
    let _environment = Environment::new();
    configure(all_protected(), "original password").expect("setup");
    let path = internal_vault_root().join("unchanged.md");
    write(Domain::Vault, &path, b"unchanged body").expect("source");
    let original = std::fs::read(&path).expect("original bytes");
    lock().expect("lock");
    assert!(change_password("replacement password").is_err());
    unlock("original password").expect("authenticate");
    assert!(change_password("").is_err());
    assert!(change_password(&"x".repeat(4097)).is_err());
    assert!(change_password("original password").is_err());
    let guard = session_guard().expect("another active application guard");
    assert!(change_password("replacement password").is_err());
    assert!(configure_unlocked(ProtectionPolicy::default()).is_err());
    drop(guard);
    assert_eq!(std::fs::read(&path).expect("source preserved"), original);
    assert!(migration::pending()
        .expect("no published cutover")
        .is_none());
    lock().expect("restart");
    unlock("original password").expect("old password unchanged");
    assert!(unlock("replacement password").is_err());
}

#[test]
fn rotation_recovery_before_sqlite_cutover_uses_old_password_then_requires_new_password() {
    let _environment = Environment::new();
    configure(all_protected(), "old recovery password").expect("setup");
    let path = internal_vault_root().join("recover.md");
    write(Domain::Vault, &path, b"recoverable body").expect("source");
    let (stage, _change) = prepared_rotation("new recovery password");
    write_plain(&path, &staged_target(&stage, "vault/recover.md"))
        .expect("simulate installed Vault target before SQLite key cutover");
    lock().expect("crash before SQLite installation");
    assert!(startup_requires_unlock().expect("recovery gate"));
    assert!(read(Domain::Vault, &path).is_err());
    assert!(unlock("new recovery password").is_err());
    assert!(migration::pending().expect("plan retained").is_some());
    let recovered = unlock("old recovery password")
        .expect_err("new password authentication required after recovery");
    assert!(recovered
        .downcast_ref::<PasswordRotationUnlockNeeded>()
        .is_some());
    assert!(!is_unlocked().expect("fully locked after key cutover"));
    assert!(migration::pending().expect("recovery completed").is_none());
    assert!(unlock("old recovery password").is_err());
    unlock("new recovery password").expect("new password finishes authentication");
    assert_eq!(
        read(Domain::Vault, &path).expect("retained body"),
        b"recoverable body"
    );
}

#[test]
fn rotation_recovery_after_sqlite_commit_accepts_only_new_password_and_is_idempotent() {
    let environment = Environment::new();
    configure(all_protected(), "old committed password").expect("setup");
    let path = internal_vault_root().join("committed.md");
    write(Domain::Vault, &path, b"committed body").expect("source");
    let indexes = encrypted_memory_index_fixtures(&environment);
    let (stage, change) = prepared_rotation("new committed password");
    let target = staged_target(&stage, "vault/committed.md");
    write_plain(&path, &target).expect("simulate completed filesystem installation");
    KeyStore::apply_password_change(&change).expect("simulate atomic SQLite commit");
    lock().expect("crash before manifest and pending-marker publication");
    assert!(unlock("old committed password").is_err());
    assert!(migration::pending().expect("plan still durable").is_some());
    unlock("new committed password").expect("recover already-committed credential key");
    assert!(migration::pending().expect("recovery complete").is_none());
    for path in indexes {
        assert!(
            !path.exists(),
            "recovery retained an old-password index at {}",
            path.display()
        );
    }
    assert_eq!(std::fs::read(&path).expect("target not rewritten"), target);
    assert_eq!(
        read(Domain::Vault, &path).expect("body intact"),
        b"committed body"
    );
    lock().expect("restart after recovery");
    unlock("new committed password").expect("stable final password");
}

#[test]
fn rotation_rejects_tampered_portable_data_and_credentials_before_replacing_sources() {
    let environment = Environment::new();
    configure(all_protected(), "source password").expect("setup");
    let config = AppConfig::default();
    KeyStore::set_with_storage(
        &config,
        "Encrypted",
        "provider data",
        CredentialStorage::Encrypted,
    )
    .expect("provider");
    let path = internal_vault_root().join("tampered.md");
    write(Domain::Vault, &path, b"original body").expect("source");
    let original = std::fs::read(&path).expect("original ciphertext");
    let mut damaged = original.clone();
    *damaged.last_mut().expect("authentication tag") ^= 1;
    write_plain(&path, &damaged).expect("tampered fixture");
    assert!(change_password("replacement password").is_err());
    assert_eq!(
        std::fs::read(&path).expect("damaged source not overwritten"),
        damaged
    );
    assert!(migration::pending().expect("no cutover").is_none());
    write_plain(&path, &original).expect("restore source fixture");
    let connection =
        rusqlite::Connection::open(environment.data().join("tcui.db")).expect("database");
    connection.execute("UPDATE tcui_credentials SET payload=zeroblob(length(payload)) WHERE kind='api_key' AND id='Encrypted'", []).expect("tamper encrypted provider");
    drop(connection);
    assert!(change_password("replacement password").is_err());
    assert_eq!(
        std::fs::read(&path).expect("intact vault not replaced"),
        original
    );
    assert!(migration::pending().expect("no cutover").is_none());
    lock().expect("restart");
    unlock("source password").expect("original verifier still authenticates");
    assert!(unlock("replacement password").is_err());
}

#[test]
fn pending_policy_migration_cannot_be_replaced_by_password_rotation() {
    let _environment = Environment::new();
    configure(all_protected(), "policy recovery password").expect("setup");
    let old = load_manifest().expect("manifest").expect("configured");
    let key = KeyStore::protection_key(false).expect("master");
    let target = Manifest::new(ProtectionPolicy::default(), Some(&key)).expect("policy target");
    migration::prepare(Some(&old), &target, Some(&key)).expect("interrupted policy migration");
    assert!(change_password("unrelated replacement password").is_err());
    lock().expect("restart pending policy migration");
    unlock("policy recovery password").expect("original policy recovery still works");
    assert_eq!(
        policy().expect("original recovery target"),
        Some(ProtectionPolicy::default())
    );
    assert!(unlock("unrelated replacement password").is_err());
}

#[test]
fn password_rotation_rejects_changed_password_metadata_in_an_authenticated_legacy_session() {
    let environment = Environment::new();
    let config = AppConfig::default();
    KeyStore::initialize_vault(&config, "legacy metadata password").expect("legacy session");
    let path = environment.data().join("vault/plain.bin");
    write_plain(&path, b"unchanged plaintext").expect("plaintext data");
    let connection =
        rusqlite::Connection::open(environment.data().join("tcui.db")).expect("database");
    connection
        .execute(
            "UPDATE tcui_credential_vault SET salt=zeroblob(16) WHERE singleton=1",
            [],
        )
        .expect("tamper salt after authenticating");
    drop(connection);
    assert!(change_password("new metadata password").is_err());
    assert_eq!(
        std::fs::read(path).expect("data unchanged"),
        b"unchanged plaintext"
    );
    assert!(migration::pending()
        .expect("no published cutover")
        .is_none());
}

fn encrypted_memory_index_fixtures(environment: &Environment) -> Vec<PathBuf> {
    let key = KeyStore::protection_key(false).expect("authenticated master");
    let wrapping = KeyStore::protection_wrapping().expect("old password wrapping");
    let stored = portable::seal(
        Domain::Memories,
        b"derived private search index",
        &key,
        &wrapping,
    )
    .expect("old portable index");
    let mut files = Vec::new();
    for database in [
        environment.data().join("memory.sqlite3"),
        environment.root.join("cache/tcui/memory.sqlite3"),
    ] {
        for suffix in ["", "-wal", "-shm", "-journal"] {
            let mut name = database.as_os_str().to_os_string();
            name.push(suffix);
            let path = PathBuf::from(name);
            write_plain(
                &path,
                if suffix.is_empty() {
                    &stored
                } else {
                    b"old search-index sidecar"
                },
            )
            .expect("derived index fixture");
            files.push(path);
        }
    }
    files
}

#[test]
fn password_rotation_retires_current_and_legacy_memory_indexes_for_either_memory_policy() {
    for memories in [false, true] {
        let environment = Environment::new();
        let selected = ProtectionPolicy {
            credentials: true,
            memories,
            ..ProtectionPolicy::default()
        };
        configure(selected, "old index password").expect("shared password");
        let source = environment.data().join("memories/source.tcui-memory");
        let document = br#"{"schema_version":1,"id":1,"logical_path":"source.md","title":"Source","created_at_ms":1,"updated_at_ms":1,"markdown":"Preserved source memory","metadata":null}"#;
        write(Domain::Memories, &source, document).expect("authoritative memory source");
        let indexes = encrypted_memory_index_fixtures(&environment);
        change_password("new index password").expect("rotate password and retire derived indexes");
        for path in indexes {
            assert!(
                !path.exists(),
                "old-password derived index survived at {}",
                path.display()
            );
        }
        assert_eq!(policy().expect("same selected policy"), Some(selected));
        assert_eq!(
            read(Domain::Memories, &source).expect("memory source retained"),
            document
        );
    }
}
