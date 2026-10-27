use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::MutexGuard;

use super::*;
use crate::storage::crypto::{decrypt_serialized, encrypt_serialized, SharedKey};
use crate::storage::paths::TcuiDataPaths;
use crate::storage::Storage;

#[path = "password_tests.rs"]
mod password_tests;

struct Environment {
    root: PathBuf,
    previous: Vec<(&'static str, Option<OsString>)>,
    _guard: MutexGuard<'static, ()>,
}

impl Environment {
    fn new() -> Self {
        let guard = crate::test_support::env_lock()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = std::env::temp_dir().join(format!(
            "tcui-protection-test-{:032x}",
            rand::random::<u128>()
        ));
        let mut previous = Vec::new();
        for (name, directory) in [
            ("HOME", "home"),
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_DATA_HOME", "data"),
            ("XDG_CACHE_HOME", "cache"),
        ] {
            previous.push((name, std::env::var_os(name)));
            let path = root.join(directory);
            std::fs::create_dir_all(&path).expect("create isolated protection directory");
            std::env::set_var(name, path);
        }
        Self {
            root,
            previous,
            _guard: guard,
        }
    }

    fn data(&self) -> PathBuf {
        self.root.join("data/tcui")
    }
}

impl Drop for Environment {
    fn drop(&mut self) {
        let _ = lock();
        for (name, previous) in self.previous.drain(..) {
            match previous {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn all_protected() -> ProtectionPolicy {
    ProtectionPolicy {
        app_password: true,
        credentials: true,
        chats: true,
        memories: true,
        vault: true,
    }
}

fn legacy_database(environment: &Environment) -> rusqlite::Connection {
    let key = SharedKey::load_or_create_default(&TcuiDataPaths::discover())
        .expect("legacy key")
        .key;
    let content =
        crate::storage::crypto::encrypt_shared_text_with_key(&key, "legacy SQLite private body")
            .expect("legacy cipher");
    let connection =
        rusqlite::Connection::open(environment.data().join("tcui.db")).expect("database");
    connection
        .execute_batch(
            "CREATE TABLE conversations(id INTEGER PRIMARY KEY,tab_id INTEGER NOT NULL,title TEXT);
             CREATE TABLE messages(
                 id INTEGER PRIMARY KEY,conversation_id INTEGER NOT NULL,role TEXT NOT NULL,
                 content TEXT NOT NULL,thinking_content TEXT,tool_calls TEXT,tool_result TEXT,
                 tool_source TEXT,images TEXT,diff_data TEXT,token_count INTEGER);",
        )
        .expect("legacy schema");
    connection
        .execute(
            "INSERT INTO conversations(id,tab_id,title) VALUES(42,7,?1)",
            ["legacy SQLite private title"],
        )
        .expect("conversation");
    connection
        .execute(
            "INSERT INTO messages(id,conversation_id,role,content) VALUES(9,42,'user',?1)",
            [&content],
        )
        .expect("message");
    connection
}

#[test]
fn protection_cutover_retires_legacy_sqlite_chats_before_returning() {
    let environment = Environment::new();
    drop(legacy_database(&environment));
    configure(
        ProtectionPolicy {
            chats: true,
            ..ProtectionPolicy::default()
        },
        "test protection password",
    )
    .expect("protect legacy data");
    let connection =
        rusqlite::Connection::open(environment.data().join("tcui.db")).expect("database");
    let remaining: i64 = connection
        .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
        .expect("remaining rows");
    assert_eq!(
        remaining, 0,
        "cutover must retire legacy rows before publishing protection"
    );
    let archived = environment
        .data()
        .join("chats/.trash/000000000000002a.tcui-chat");
    let raw = std::fs::read(&archived).expect("archive");
    assert!(raw.starts_with(MAGIC));
    let document: serde_json::Value =
        serde_json::from_slice(&read(Domain::Chats, &archived).expect("decrypt"))
            .expect("document");
    assert_eq!(document["title"], "legacy SQLite private title");
    assert_eq!(
        document["messages"][0]["content"],
        "legacy SQLite private body"
    );
    drop(connection);
    let database = std::fs::read(environment.data().join("tcui.db")).expect("database bytes");
    assert!(!database
        .windows(b"legacy SQLite private title".len())
        .any(|bytes| bytes == b"legacy SQLite private title"));
}

#[test]
fn legacy_archive_collision_preserves_both_sources_and_refuses_protection() {
    let environment = Environment::new();
    drop(legacy_database(&environment));
    let key =
        SharedKey::load_from_path(&environment.data().join("chat.key")).expect("original key");
    let archived = environment
        .data()
        .join("chats/.trash/000000000000002a.tcui-chat");
    let document = serde_json::json!({"schema_version":1,"id":42,"tab_id":7,"title":"unrelated archived chat","created_at_ms":1,"updated_at_ms":1,"pinned":false,"messages":[]});
    let original = encrypt_serialized(&key, "chat", &document).expect("archive cipher");
    write_plain(&archived, original.as_bytes()).expect("existing archive");
    assert!(configure(
        ProtectionPolicy {
            chats: true,
            ..ProtectionPolicy::default()
        },
        "test protection password"
    )
    .is_err());
    assert_eq!(
        std::fs::read(&archived).expect("existing archive retained"),
        original.as_bytes()
    );
    assert!(policy().expect("policy").is_none());
    let connection =
        rusqlite::Connection::open(environment.data().join("tcui.db")).expect("database");
    let remaining: i64 = connection
        .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
        .expect("remaining rows");
    assert_eq!(remaining, 1);
}

#[test]
fn legacy_checkpoint_failure_retries_sanitization_after_rows_are_retired() {
    let environment = Environment::new();
    let connection = legacy_database(&environment);
    connection
        .execute_batch("PRAGMA journal_mode=WAL")
        .expect("WAL mode");
    let reader =
        rusqlite::Connection::open(environment.data().join("tcui.db")).expect("old reader");
    reader
        .execute_batch("BEGIN; SELECT * FROM conversations")
        .expect("retained read snapshot");
    assert!(configure(
        ProtectionPolicy {
            chats: true,
            ..ProtectionPolicy::default()
        },
        "test protection password"
    )
    .is_err());
    assert!(policy().expect("unpublished policy").is_none());
    let remaining: i64 = connection
        .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
        .expect("retired rows");
    assert_eq!(
        remaining, 0,
        "the failed checkpoint occurs after durable archival and deletion"
    );
    drop(reader);
    configure(
        ProtectionPolicy {
            chats: true,
            ..ProtectionPolicy::default()
        },
        "test protection password",
    )
    .expect("retry after reader closes");
    let archived = environment
        .data()
        .join("chats/.trash/000000000000002a.tcui-chat");
    assert!(std::fs::read(archived)
        .expect("protected archive")
        .starts_with(MAGIC));
    for path in [
        environment.data().join("tcui.db"),
        environment.data().join("tcui.db-wal"),
    ] {
        match std::fs::read(path) {
            Ok(bytes) => assert!(!bytes
                .windows(b"legacy SQLite private title".len())
                .any(|window| window == b"legacy SQLite private title")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("read retired SQLite bytes: {error}"),
        }
    }
}

#[test]
fn legacy_archive_retry_requires_durability_before_retiring_database() {
    let environment = Environment::new();
    let connection = legacy_database(&environment);
    let key =
        SharedKey::load_from_path(&environment.data().join("chat.key")).expect("original key");
    let mut message =
        crate::app::message::Message::new(42, "user".into(), "legacy SQLite private body".into());
    message.id = Some(9);
    let document = serde_json::json!({"schema_version":1,"id":42,"tab_id":7,"title":"legacy SQLite private title","created_at_ms":1,"updated_at_ms":1,"pinned":false,"messages":[message]});
    let original = encrypt_serialized(&key, "chat", &document).expect("archive cipher");
    let archived = environment
        .data()
        .join("chats/.trash/000000000000002a.tcui-chat");
    write_plain(&archived, original.as_bytes()).expect("previously published archive");
    crate::storage::chat_store::FAIL_ARCHIVE_SYNC.store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(configure(
        ProtectionPolicy {
            chats: true,
            ..ProtectionPolicy::default()
        },
        "test protection password"
    )
    .is_err());
    let remaining: i64 = connection
        .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
        .expect("source rows");
    assert_eq!(remaining, 1);
    assert!(policy().expect("policy").is_none());
    assert_eq!(
        std::fs::read(&archived).expect("archive unchanged"),
        original.as_bytes()
    );
    configure(
        ProtectionPolicy {
            chats: true,
            ..ProtectionPolicy::default()
        },
        "test protection password",
    )
    .expect("durable retry");
    let retained: serde_json::Value =
        serde_json::from_slice(&read(Domain::Chats, &archived).expect("protected archive"))
            .expect("document");
    assert_eq!(
        retained["messages"][0]["content"],
        "legacy SQLite private body"
    );
    let remaining: i64 = connection
        .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
        .expect("retired rows");
    assert_eq!(remaining, 0);
}

#[test]
fn incomplete_legacy_chat_schema_never_publishes_protection() {
    for removed in ["messages", "conversations"] {
        let environment = Environment::new();
        let connection = legacy_database(&environment);
        connection
            .execute_batch(&format!("DROP TABLE {removed}"))
            .expect("partially restored database");
        assert!(configure(
            ProtectionPolicy {
                chats: true,
                ..ProtectionPolicy::default()
            },
            "test protection password"
        )
        .is_err());
        assert!(policy().expect("unpublished policy").is_none());
        let remaining = if removed == "messages" {
            "conversations"
        } else {
            "messages"
        };
        let count: i64 = connection
            .query_row(&format!("SELECT COUNT(*) FROM {remaining}"), [], |row| {
                row.get(0)
            })
            .expect("remaining legacy rows");
        assert_eq!(count, 1);
    }
}

#[test]
fn plaintext_choice_stores_readable_chats_without_a_machine_key() {
    let environment = Environment::new();
    configure(ProtectionPolicy::default(), "").expect("choose plaintext");
    let storage = Storage::new().expect("open unprotected storage");
    let id = storage
        .create_conversation_snapshot(1, "readable private title", &[], None)
        .expect("save chat");
    let path = environment
        .data()
        .join(format!("chats/{id:016x}.tcui-chat"));
    let raw = std::fs::read(&path).expect("read stored bytes");
    let document: serde_json::Value = serde_json::from_slice(&raw).expect("plaintext chat JSON");
    assert_eq!(document["title"], "readable private title");
    assert!(!environment.data().join("chat.key").exists());
    drop(storage);
    lock().expect("lock empty session");
    let reopened = Storage::new().expect("reopen plaintext storage");
    assert_eq!(
        reopened
            .read_chat_document(id)
            .expect("read persisted chat")
            .title,
        "readable private title"
    );
}

#[test]
fn protected_chats_require_password_even_with_the_old_machine_key() {
    let environment = Environment::new();
    let legacy = SharedKey::load_or_create_default(&TcuiDataPaths::discover())
        .expect("legacy key")
        .key;
    configure(
        ProtectionPolicy {
            chats: true,
            ..ProtectionPolicy::default()
        },
        "test protection password",
    )
    .expect("protect chats");
    let storage = Storage::new().expect("open unlocked storage");
    let id = storage
        .create_conversation_snapshot(2, "password-only title", &[], None)
        .expect("save protected chat");
    let path = environment
        .data()
        .join(format!("chats/{id:016x}.tcui-chat"));
    let ciphertext = std::fs::read(&path).expect("stored ciphertext");
    assert!(ciphertext.starts_with(MAGIC));
    assert!(!ciphertext
        .windows(b"password-only title".len())
        .any(|bytes| bytes == b"password-only title"));
    assert!(decrypt_serialized::<serde_json::Value>(
        &legacy,
        "chat",
        &String::from_utf8_lossy(&ciphertext)
    )
    .is_err());
    lock().expect("lock");
    assert!(startup_requires_unlock().expect("startup policy"));
    assert!(Storage::new().is_err());
    assert!(storage.get_conversations(2).is_err());
    assert!(storage.read_chat_document(id).is_err());
    assert!(unlock("wrong password").is_err());
    assert!(storage.read_chat_document(id).is_err());
    unlock("test protection password").expect("unlock");
    assert_eq!(
        storage
            .read_chat_document(id)
            .expect("read restored chat")
            .title,
        "password-only title"
    );
}

#[test]
fn app_entry_lock_blocks_content_even_when_at_rest_chat_protection_is_off() {
    let _environment = Environment::new();
    configure(
        ProtectionPolicy {
            app_password: true,
            ..ProtectionPolicy::default()
        },
        "entry password",
    )
    .expect("configure app gate");
    let storage = Storage::new().expect("unlocked storage");
    let id = storage
        .create_conversation_snapshot(1, "not encrypted at rest", &[], None)
        .expect("save");
    assert!(!is_protected(Domain::Chats).expect("chat policy"));
    lock().expect("lock entry");
    assert!(require_app().is_err());
    assert!(storage.read_chat_document(id).is_err());
    assert!(require(Domain::Vault).is_err());
    unlock("entry password").expect("unlock entry");
    assert_eq!(
        storage
            .read_chat_document(id)
            .expect("read unlocked chat")
            .title,
        "not encrypted at rest"
    );
}

#[test]
fn memory_and_vault_locks_do_not_lock_unprotected_chats() {
    let environment = Environment::new();
    configure(
        ProtectionPolicy {
            memories: true,
            vault: true,
            ..ProtectionPolicy::default()
        },
        "partial password",
    )
    .expect("partial protection");
    let memory = environment.data().join("memories/fixture.tcui-memory");
    let file = internal_vault_root().join("private.md");
    write(
        Domain::Memories,
        &memory,
        br#"{"markdown":"private memory"}"#,
    )
    .expect("save memory");
    write(Domain::Vault, &file, b"private vault note").expect("save vault note");
    lock().expect("lock partial stores");
    assert!(!startup_requires_unlock().expect("partial startup policy"));
    assert!(Storage::new().is_ok());
    assert!(read(Domain::Memories, &memory).is_err());
    assert!(read(Domain::Vault, &file).is_err());
    unlock("partial password").expect("shared unlock");
    assert_eq!(
        read(Domain::Vault, &file).expect("read note"),
        b"private vault note"
    );
    assert!(read(Domain::Memories, &memory)
        .expect("read memory")
        .windows(14)
        .any(|bytes| bytes == b"private memory"));
}

#[test]
fn legacy_documents_and_saved_binary_round_trip_through_protection() {
    let environment = Environment::new();
    let paths = TcuiDataPaths::discover();
    let key = SharedKey::load_or_create_default(&paths)
        .expect("legacy key")
        .key;
    let chat = paths.chats_dir.join("0000000000000001.tcui-chat");
    let memory = paths.memories_dir.join("0000000000000002.tcui-memory");
    let chat_document =
        serde_json::json!({"title":"legacy title", "messages":[{"content":"legacy content"}]});
    let memory_document =
        serde_json::json!({"markdown":"legacy memory", "metadata":{"scope":"user"}});
    io::atomic_write(
        &chat,
        encrypt_serialized(&key, "chat", &chat_document)
            .expect("legacy chat")
            .as_bytes(),
    )
    .expect("write chat");
    io::atomic_write(
        &memory,
        encrypt_serialized(&key, "memory", &memory_document)
            .expect("legacy memory")
            .as_bytes(),
    )
    .expect("write memory");
    let binary = internal_vault_root().join("download.bin");
    let payload = b"\x00\xffprivate downloaded bytes\x00";
    io::atomic_write(&binary, payload).expect("binary fixture");
    configure(all_protected(), "migration password").expect("migrate legacy data");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(
            &read(Domain::Chats, &chat).expect("read chat")
        )
        .expect("chat JSON"),
        chat_document
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(
            &read(Domain::Memories, &memory).expect("read memory")
        )
        .expect("memory JSON"),
        memory_document
    );
    assert_eq!(read(Domain::Vault, &binary).expect("read binary"), payload);
    assert!(std::fs::read(&binary)
        .expect("raw binary")
        .starts_with(portable::MAGIC));
    configure(ProtectionPolicy::default(), "migration password")
        .expect("disable protection explicitly");
    assert_eq!(std::fs::read(&binary).expect("plaintext binary"), payload);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(&chat).expect("plaintext chat"))
            .expect("chat JSON"),
        chat_document
    );
    assert!(
        environment.data().join("chat.key").exists(),
        "retain legacy key for remaining explicitly imported formats"
    );
}

#[test]
fn malformed_legacy_data_aborts_before_any_source_replacement() {
    let environment = Environment::new();
    let key = SharedKey::load_or_create_default(&TcuiDataPaths::discover())
        .expect("legacy key")
        .key;
    let first = environment.data().join("chats/0000000000000001.tcui-chat");
    let invalid = environment.data().join("chats/0000000000000002.tcui-chat");
    let original = encrypt_serialized(&key, "chat", &serde_json::json!({"title":"retained"}))
        .expect("legacy payload");
    io::atomic_write(&first, original.as_bytes()).expect("first source");
    io::atomic_write(&invalid, b"enc:v1:not-a-valid-envelope").expect("bad source");
    assert!(configure(all_protected(), "migration password").is_err());
    assert_eq!(
        std::fs::read(&first).expect("retained first source"),
        original.as_bytes()
    );
    assert_eq!(
        std::fs::read(&invalid).expect("retained invalid source"),
        b"enc:v1:not-a-valid-envelope"
    );
    assert!(policy().expect("policy remains unset").is_none());
    assert!(migration::pending()
        .expect("no incomplete commit")
        .is_none());
}

#[test]
fn interrupted_migration_blocks_reads_and_recovers_with_the_original_password() {
    let environment = Environment::new();
    configure(all_protected(), "recovery password").expect("initial protection");
    let file = internal_vault_root().join("recovery.md");
    write(Domain::Vault, &file, b"recover without data loss").expect("save original");
    let old = load_manifest().expect("manifest").expect("configured");
    let key = KeyStore::protection_key(false).expect("master key");
    let target = Manifest::new(ProtectionPolicy::default(), Some(&key)).expect("target policy");
    migration::prepare(Some(&old), &target, Some(&key)).expect("prepare durable recovery plan");
    lock().expect("simulate restart");
    assert!(startup_requires_unlock().expect("recovery gate"));
    assert!(read(Domain::Vault, &file).is_err());
    assert!(unlock("incorrect password").is_err());
    assert!(environment.data().join("protection-pending.json").exists());
    unlock("recovery password").expect("complete interrupted conversion");
    assert_eq!(
        std::fs::read(&file).expect("recovered plaintext file"),
        b"recover without data loss"
    );
    assert_eq!(
        policy().expect("published policy"),
        Some(ProtectionPolicy::default())
    );
    assert!(!environment.data().join("protection-pending.json").exists());
}

#[test]
fn recovery_preserves_concurrent_changes_instead_of_overwriting_them() {
    let environment = Environment::new();
    configure(all_protected(), "conflict password").expect("initial protection");
    let file = internal_vault_root().join("conflict.md");
    write(Domain::Vault, &file, b"original").expect("original");
    let old = load_manifest().expect("manifest").expect("configured");
    let key = KeyStore::protection_key(false).expect("master key");
    let target = Manifest::new(ProtectionPolicy::default(), Some(&key)).expect("target");
    migration::prepare(Some(&old), &target, Some(&key)).expect("prepare");
    io::atomic_write(&file, b"externally changed data").expect("simulate conflicting writer");
    lock().expect("restart");
    assert!(unlock("conflict password").is_err());
    assert_eq!(
        std::fs::read(&file).expect("retain conflict"),
        b"externally changed data"
    );
    assert!(environment.data().join("protection-pending.json").exists());
}

#[test]
fn authenticated_payloads_reject_cross_domain_reads_and_corruption() {
    let _environment = Environment::new();
    configure(all_protected(), "authentication password").expect("configure");
    let mut stored = encode(Domain::Chats, b"authenticated bytes").expect("encrypt");
    assert!(decode(Domain::Memories, &stored).is_err());
    let last = stored.last_mut().expect("ciphertext exists");
    *last ^= 1;
    assert!(decode(Domain::Chats, &stored).is_err());
    assert!(decode(Domain::Chats, b"unauthenticated plaintext").is_err());
}

#[test]
fn changed_policy_metadata_cannot_authenticate_with_the_original_password() {
    let _environment = Environment::new();
    configure(all_protected(), "policy password").expect("configure");
    let mut manifest = load_manifest().expect("load").expect("configured");
    manifest.policy.vault = false;
    io::atomic_write(
        &manifest_path().expect("manifest path"),
        &serde_json::to_vec(&manifest).expect("serialize"),
    )
    .expect("tamper fixture");
    lock().expect("lock");
    assert!(unlock("policy password").is_err());
    assert!(!is_unlocked().expect("remains locked"));
}

#[test]
fn another_active_application_prevents_protection_reconfiguration() {
    let _environment = Environment::new();
    configure(ProtectionPolicy::default(), "").expect("plaintext setup");
    let session = session_guard().expect("active app guard");
    assert!(configure(all_protected(), "busy password").is_err());
    assert_eq!(
        policy().expect("unchanged policy"),
        Some(ProtectionPolicy::default())
    );
    drop(session);
    configure(all_protected(), "busy password").expect("configure after app exits");
}

#[cfg(unix)]
#[test]
fn symlinked_migration_sources_never_modify_outside_files() {
    use std::os::unix::fs::symlink;
    let environment = Environment::new();
    let sentinel = environment.root.join("outside-private.txt");
    std::fs::write(&sentinel, b"outside sentinel").expect("sentinel");
    std::fs::create_dir_all(internal_vault_root()).expect("internal vault");
    symlink(&sentinel, internal_vault_root().join("symlink.md")).expect("symlink fixture");
    assert!(configure(all_protected(), "symlink password").is_err());
    assert_eq!(
        std::fs::read(&sentinel).expect("sentinel intact"),
        b"outside sentinel"
    );
    assert!(policy().expect("unpublished policy").is_none());
}

#[test]
fn live_policy_tampering_cannot_downgrade_an_unlocked_file() {
    let _environment = Environment::new();
    configure(all_protected(), "live policy password").expect("configure");
    let file = internal_vault_root().join("private.md");
    write(Domain::Vault, &file, b"remain encrypted").expect("save");
    let original = std::fs::read(&file).expect("original ciphertext");
    let mut manifest = load_manifest().expect("manifest").expect("configured");
    manifest.policy.vault = false;
    io::atomic_write(
        &manifest_path().expect("manifest path"),
        &serde_json::to_vec(&manifest).expect("serialize"),
    )
    .expect("tamper");
    assert!(read(Domain::Vault, &file).is_err());
    assert!(write(Domain::Vault, &file, b"remain encrypted").is_err());
    assert_eq!(std::fs::read(&file).expect("ciphertext retained"), original);
}

#[test]
fn user_lock_named_files_are_encrypted_and_decrypted_with_other_vault_content() {
    let _environment = Environment::new();
    let first = internal_vault_root().join("Cargo.lock");
    let second = internal_vault_root().join("private.lock");
    io::atomic_write(&first, b"private dependency inventory").expect("first file");
    io::atomic_write(&second, b"private lock-named document").expect("second file");
    configure(all_protected(), "lock filename password").expect("protect");
    assert!(std::fs::read(&first)
        .expect("protected first")
        .starts_with(portable::MAGIC));
    assert!(std::fs::read(&second)
        .expect("protected second")
        .starts_with(portable::MAGIC));
    assert_eq!(
        read(Domain::Vault, &first).expect("first content"),
        b"private dependency inventory"
    );
    configure(ProtectionPolicy::default(), "lock filename password").expect("unprotect");
    assert_eq!(
        std::fs::read(&first).expect("plaintext first"),
        b"private dependency inventory"
    );
    assert_eq!(
        std::fs::read(&second).expect("plaintext second"),
        b"private lock-named document"
    );
}

#[test]
fn failed_decryption_migration_does_not_leave_plaintext_staging() {
    let environment = Environment::new();
    configure(all_protected(), "staging password").expect("protect");
    let first = internal_vault_root().join("a.md");
    let damaged = internal_vault_root().join("z.md");
    write(
        Domain::Vault,
        &first,
        b"must not survive in plaintext staging",
    )
    .expect("first");
    write(Domain::Vault, &damaged, b"second").expect("second");
    let original = std::fs::read(&first).expect("first ciphertext");
    let mut bad = std::fs::read(&damaged).expect("second ciphertext");
    *bad.last_mut().expect("tag") ^= 1;
    io::atomic_write(&damaged, &bad).expect("damage second source");
    assert!(configure(ProtectionPolicy::default(), "staging password").is_err());
    assert_eq!(std::fs::read(&first).expect("first retained"), original);
    assert_eq!(
        policy().expect("old policy retained"),
        Some(all_protected())
    );
    for entry in walkdir::WalkDir::new(environment.data()).follow_links(false) {
        let entry = entry.expect("staging entry");
        if entry.file_type().is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "new")
        {
            let bytes = std::fs::read(entry.path()).expect("staged bytes");
            assert!(bytes.starts_with(MAGIC));
            assert!(!bytes
                .windows(b"must not survive in plaintext staging".len())
                .any(|bytes| bytes == b"must not survive in plaintext staging"));
        }
    }
}

#[test]
fn recovery_rejects_a_valid_but_different_target_policy() {
    let environment = Environment::new();
    configure(all_protected(), "journal password").expect("protect");
    let file = internal_vault_root().join("journal.md");
    write(
        Domain::Vault,
        &file,
        b"never install plaintext under a protected policy",
    )
    .expect("save");
    let original = std::fs::read(&file).expect("original");
    let old = load_manifest().expect("manifest").expect("configured");
    let key = KeyStore::protection_key(false).expect("master");
    let target = Manifest::new(ProtectionPolicy::default(), Some(&key)).expect("target");
    migration::prepare(Some(&old), &target, Some(&key)).expect("stage conversion");
    let path = environment.data().join("protection-pending.json");
    let mut pending: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).expect("pending")).expect("pending JSON");
    pending["target"] = serde_json::to_value(&old).expect("other valid policy");
    io::atomic_write(
        &path,
        &serde_json::to_vec(&pending).expect("serialize pending"),
    )
    .expect("swap target");
    lock().expect("restart");
    assert!(unlock("journal password").is_err());
    assert_eq!(std::fs::read(&file).expect("source retained"), original);
    assert!(path.exists());
}

#[test]
fn enabling_protection_cleans_plaintext_orphans_including_atomic_temporaries() {
    let environment = Environment::new();
    let stage = environment.data().join(format!(
        ".protection-migration-{:032x}",
        rand::random::<u128>()
    ));
    io::atomic_write(&stage.join("owner"), b"tcui-protection-staging-v1")
        .expect("ownership marker");
    io::atomic_write(&stage.join("0.new"), b"orphaned plaintext from first boot")
        .expect("partial payload");
    io::atomic_write(
        &stage.join("1.old"),
        b"remaining original after interrupted cleanup",
    )
    .expect("remaining backup");
    io::atomic_write(
        &stage.join(".tcui-protection-00000000000000000000000000000000.tmp"),
        b"unfinished plaintext atomic write",
    )
    .expect("crash temporary");
    let note = internal_vault_root().join("note.md");
    io::atomic_write(&note, b"current source data").expect("current source");
    configure(all_protected(), "orphan cleanup password").expect("safe protection cutover");
    assert!(!stage.exists());
    assert!(std::fs::read(&note)
        .expect("encrypted current source")
        .starts_with(portable::MAGIC));
    assert_eq!(
        read(Domain::Vault, &note).expect("source retained"),
        b"current source data"
    );
}

#[test]
fn unverified_orphan_payload_blocks_cutover_without_deleting_it() {
    let environment = Environment::new();
    let stage = environment.data().join(format!(
        ".protection-migration-{:032x}",
        rand::random::<u128>()
    ));
    let payload = stage.join("0.new");
    io::atomic_write(&payload, b"payload whose ownership marker was lost")
        .expect("unverified orphan");
    assert!(configure(all_protected(), "uncertain stage password").is_err());
    assert_eq!(
        std::fs::read(&payload).expect("preserved uncertain data"),
        b"payload whose ownership marker was lost"
    );
    assert!(policy().expect("no false encryption claim").is_none());
}

#[test]
fn password_enrollment_after_plaintext_setup_preserves_data_and_credential_choices() {
    use crate::config::key_store::CredentialStorage;
    let environment = Environment::new();
    configure(ProtectionPolicy::default(), "").expect("plaintext first boot");
    let config = AppConfig::default();
    let storage = Storage::new().expect("plaintext storage");
    let id = storage
        .create_conversation_snapshot(0, "keep this plaintext chat", &[], None)
        .expect("chat");
    let path = environment
        .data()
        .join(format!("chats/{id:016x}.tcui-chat"));
    let original = std::fs::read(&path).expect("original chat bytes");
    KeyStore::set_with_storage(
        &config,
        "Fixture",
        "plaintext sibling",
        CredentialStorage::Plaintext,
    )
    .expect("plain connection");
    initialize_password("later enrollment password").expect("enroll without converting");
    assert_eq!(
        policy().expect("same choices"),
        Some(ProtectionPolicy::default())
    );
    assert_eq!(std::fs::read(&path).expect("unchanged chat"), original);
    assert_eq!(
        KeyStore::vault_status(&config)
            .expect("storage default")
            .storage,
        CredentialStorage::Plaintext
    );
    KeyStore::set_with_storage(
        &config,
        "Fixture::encrypted",
        "encrypted sibling",
        CredentialStorage::Encrypted,
    )
    .expect("first encrypted connection");
    lock().expect("lock credentials only");
    assert!(Storage::new().is_ok());
    assert_eq!(
        KeyStore::get(&config, "Fixture")
            .expect("plaintext still available")
            .as_deref(),
        Some("plaintext sibling")
    );
    assert!(KeyStore::get(&config, "Fixture::encrypted").is_err());
    unlock("later enrollment password").expect("shared unlock");
    assert_eq!(
        KeyStore::get(&config, "Fixture::encrypted")
            .expect("encrypted recovered")
            .as_deref(),
        Some("encrypted sibling")
    );
}

#[test]
fn password_enrollment_reuses_existing_password_without_replacing_legacy_credentials() {
    use crate::config::key_store::CredentialStorage;
    let _environment = Environment::new();
    configure(ProtectionPolicy::default(), "").expect("plaintext options");
    let config = AppConfig::default();
    KeyStore::initialize_vault(&config, "existing credential password").expect("legacy password");
    KeyStore::set_with_storage(
        &config,
        "Legacy",
        "retained encrypted credential",
        CredentialStorage::Encrypted,
    )
    .expect("legacy connection");
    let original =
        std::fs::read(manifest_path().expect("manifest path")).expect("unsigned manifest");
    assert!(initialize_password("incorrect password").is_err());
    assert_eq!(
        std::fs::read(manifest_path().expect("path")).expect("manifest retained"),
        original
    );
    initialize_password("existing credential password").expect("reuse password");
    assert_eq!(
        KeyStore::get(&config, "Legacy")
            .expect("retained credential")
            .as_deref(),
        Some("retained encrypted credential")
    );
    assert_eq!(
        policy().expect("unchanged choices"),
        Some(ProtectionPolicy::default())
    );
}

#[test]
fn literal_envelope_prefix_is_valid_plaintext_vault_content_across_policy_changes() {
    let _environment = Environment::new();
    configure(ProtectionPolicy::default(), "").expect("plaintext setup");
    let path = internal_vault_root().join("literal-envelope.bin");
    let payload = [
        MAGIC,
        b"\xffordinary binary content, not an encrypted envelope",
    ]
    .concat();
    write(Domain::Vault, &path, &payload).expect("save literal bytes");
    assert_eq!(read(Domain::Vault, &path).expect("read plaintext"), payload);
    let protected = ProtectionPolicy {
        vault: true,
        ..ProtectionPolicy::default()
    };
    configure(protected, "literal-prefix-password").expect("enable Vault protection");
    assert_eq!(
        read(Domain::Vault, &path).expect("decrypt content"),
        payload
    );
    configure(ProtectionPolicy::default(), "literal-prefix-password")
        .expect("disable Vault protection");
    lock().expect("discard cached key");
    assert_eq!(std::fs::read(&path).expect("plaintext file"), payload);
    assert_eq!(
        read(Domain::Vault, &path).expect("locked plaintext read"),
        payload
    );
    configure(protected, "literal-prefix-password").expect("protect again");
    assert_eq!(read(Domain::Vault, &path).expect("decrypt again"), payload);
}

#[test]
fn valid_envelope_can_be_opaque_vault_content_without_extra_decryption() {
    let _environment = Environment::new();
    let protected = ProtectionPolicy {
        vault: true,
        ..ProtectionPolicy::default()
    };
    configure(protected, "opaque-envelope-password").expect("initialize password");
    let payload = with_key(|key| seal(Domain::Vault.context(), b"inner opaque payload", key))
        .expect("create a valid envelope as user content");
    configure(ProtectionPolicy::default(), "opaque-envelope-password")
        .expect("choose plaintext storage");
    let path = internal_vault_root().join("opaque-envelope.bin");
    write(Domain::Vault, &path, &payload).expect("save opaque bytes");
    assert_eq!(read(Domain::Vault, &path).expect("opaque read"), payload);
    configure(protected, "opaque-envelope-password").expect("encrypt outer file");
    assert_eq!(read(Domain::Vault, &path).expect("one layer only"), payload);
    configure(ProtectionPolicy::default(), "opaque-envelope-password")
        .expect("remove outer encryption");
    lock().expect("discard cached key");
    assert_eq!(
        read(Domain::Vault, &path).expect("locked opaque read"),
        payload
    );
    configure(protected, "opaque-envelope-password").expect("protect opaque content again");
    assert_eq!(
        read(Domain::Vault, &path).expect("opaque content retained"),
        payload
    );
}

#[test]
fn explicit_decryption_can_read_encrypted_copies_after_managed_protection_is_disabled() {
    let environment = Environment::new();
    configure(
        ProtectionPolicy {
            chats: true,
            ..ProtectionPolicy::default()
        },
        "archived-chat-password",
    )
    .expect("protect chats");
    let storage = Storage::new().expect("open chats");
    let id = storage
        .create_conversation_snapshot(1, "Archived conversation", &[], None)
        .expect("save chat");
    let archive = environment.root.join("archived.tcui-chat");
    std::fs::copy(
        environment
            .data()
            .join(format!("chats/{id:016x}.tcui-chat")),
        &archive,
    )
    .expect("retain an encrypted copy outside the managed store");
    drop(storage);
    configure(ProtectionPolicy::default(), "archived-chat-password")
        .expect("disable chat protection");
    lock().expect("lock the retained master");
    assert!(read_for_explicit_decryption(Domain::Chats, &archive).is_err());
    unlock("archived-chat-password").expect("unlock retained master");
    let exported = read_for_explicit_decryption(Domain::Chats, &archive).expect("explicit export");
    let document: serde_json::Value = serde_json::from_slice(&exported).expect("chat JSON");
    assert_eq!(document["title"], "Archived conversation");
    let mut damaged = std::fs::read(&archive).expect("read archive");
    *damaged.last_mut().expect("authenticated ciphertext") ^= 1;
    std::fs::write(&archive, damaged).expect("damage the archive");
    assert!(read_for_explicit_decryption(Domain::Chats, &archive).is_err());
}

/// Switches between synthetic instances while retaining the shared env_lock.
fn select_instance(environment: &Environment, name: &str) {
    for (variable, directory) in [
        ("HOME", "home"),
        ("XDG_CONFIG_HOME", "config"),
        ("XDG_DATA_HOME", "data"),
        ("XDG_CACHE_HOME", "cache"),
    ] {
        let path = environment.root.join(name).join(directory);
        std::fs::create_dir_all(&path).expect("instance directory");
        std::env::set_var(variable, path);
    }
}

#[test]
fn portable_content_crosses_instances_without_provider_secrets_and_reencodes_target_policy() {
    let environment = Environment::new();
    configure(all_protected(), "source password").expect("source setup");
    let vault = encode(Domain::Vault, b"portable binary \0\xff").expect("Vault envelope");
    let memory =
        encode(Domain::Memories, br#"{"memory":"portable fact"}"#).expect("Memory envelope");
    let source_root = root().expect("source root");
    let source_database = source_root.join("tcui.db");
    lock().expect("source restart");

    select_instance(&environment, "different-password-target");
    configure(all_protected(), "unrelated target password").expect("target setup");
    let target_key = KeyStore::protection_key(false).expect("target master");
    let error = decode(Domain::Vault, &vault).expect_err("needs source password");
    assert!(error.downcast_ref::<PortableUnlockNeeded>().is_some());
    let target_database =
        std::fs::read(root().expect("target root").join("tcui.db")).expect("target database");
    assert!(unlock_foreign_file(Domain::Vault, &vault, "wrong password").is_err());
    assert_eq!(
        std::fs::read(root().expect("target root").join("tcui.db"))
            .expect("unchanged target database"),
        target_database
    );
    assert!(decode(Domain::Vault, &vault).is_err());
    std::fs::remove_file(&source_database).expect("source database is not needed");
    unlock_foreign_file(Domain::Vault, &vault, "source password").expect("source password only");
    let plaintext = decode(Domain::Vault, &vault).expect("portable content");
    assert_eq!(plaintext, b"portable binary \0\xff");
    assert_eq!(
        decode(Domain::Memories, &memory).expect("same authenticated source wrapping"),
        br#"{"memory":"portable fact"}"#
    );
    let target = root().expect("target root").join("vault/imported.bin");
    write(Domain::Vault, &target, &plaintext).expect("import under target policy");
    let target_envelope = std::fs::read(&target).expect("target encrypted bytes");
    assert_ne!(target_envelope, vault);
    assert_eq!(
        portable::open_with_key(Domain::Vault, &target_envelope, &target_key)
            .expect("target master"),
        plaintext
    );
    lock().expect("clear all session keys");
    unlock("unrelated target password").expect("target password remains unchanged");
    assert!(decode(Domain::Vault, &vault).is_err());
    assert_eq!(
        read(Domain::Vault, &target).expect("target restart"),
        plaintext
    );

    lock().expect("target lock");
    select_instance(&environment, "plaintext-target");
    configure(ProtectionPolicy::default(), "").expect("plaintext target policy");
    assert!(decode(Domain::Vault, &vault)
        .expect_err("encrypted under plaintext policy")
        .downcast_ref::<PortableUnlockNeeded>()
        .is_some());
    unlock_foreign_file(Domain::Vault, &vault, "source password")
        .expect("plaintext target source unlock");
    let target = root().expect("plaintext root").join("vault/imported.bin");
    write(
        Domain::Vault,
        &target,
        &decode(Domain::Vault, &vault).expect("decode import"),
    )
    .expect("plaintext import");
    assert_eq!(std::fs::read(&target).expect("target plaintext"), plaintext);
    select_instance(&environment, "isolated-root");
    configure(ProtectionPolicy::default(), "").expect("separate root");
    assert!(
        decode(Domain::Vault, &vault).is_err(),
        "foreign caches must not cross roots"
    );
    select_instance(&environment, "plaintext-target");
    assert_eq!(
        decode(Domain::Vault, &vault).expect("original root retains its session"),
        plaintext
    );
    lock().expect("clear plaintext target foreign keys");
    assert!(decode(Domain::Vault, &vault).is_err());
}

#[test]
fn portable_rejects_domains_unbounded_kdf_truncation_and_authenticated_header_damage() {
    let environment = Environment::new();
    configure(all_protected(), "source authentication password").expect("source");
    let original = encode(Domain::Vault, b"authenticated portable content").expect("envelope");
    assert_eq!(
        inspect_bytes(&original).expect("inspection"),
        StoredFormat::Portable {
            domain: Domain::Vault
        }
    );
    assert!(decode(Domain::Memories, &original).is_err());
    let legacy = with_key(|key| seal(Domain::Vault.context(), b"legacy", key)).expect("V1 file");
    lock().expect("source lock");
    select_instance(&environment, "damaged-target");
    configure(ProtectionPolicy::default(), "").expect("target");
    for length in [
        portable::MAGIC.len(),
        portable::MAGIC.len() + 1,
        original.len() - b"authenticated portable content".len() - 1,
    ] {
        assert!(inspect_bytes(&original[..length]).is_err());
    }
    let mut unknown = original.clone();
    unknown[portable::MAGIC.len() - 1] = 1;
    assert!(inspect_bytes(&unknown).is_err());
    let mut hostile_kdf = original.clone();
    hostile_kdf[portable::MAGIC.len() + 1..portable::MAGIC.len() + 5]
        .copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(inspect_bytes(&hostile_kdf).is_err());
    assert!(unlock_foreign_file(
        Domain::Vault,
        &hostile_kdf,
        "source authentication password"
    )
    .is_err());
    assert!(unlock_foreign_file(
        Domain::Vault,
        &original[..original.len() - 1],
        "source authentication password"
    )
    .is_err());
    for index in [
        portable::MAGIC.len(), // A valid but wrong domain remains authenticated.
        portable::MAGIC.len() + 1 + 12, // Salt.
        portable::MAGIC.len() + 1 + 12 + 16, // Wrapped-master nonce.
        portable::MAGIC.len() + 1 + 12 + 16 + 12, // Wrapped master.
        portable::MAGIC.len() + 1 + 88, // Content nonce.
        original.len() - 1,    // Content authentication tag.
    ] {
        let mut damaged = original.clone();
        let domain = if index == portable::MAGIC.len() {
            damaged[index] = 2;
            Domain::Memories
        } else {
            damaged[index] ^= 1;
            Domain::Vault
        };
        assert!(unlock_foreign_file(domain, &damaged, "source authentication password").is_err());
        assert!(
            decode(Domain::Vault, &original).is_err(),
            "failed authentication must not cache a key"
        );
    }
    assert!(unlock_foreign_file(
        Domain::Memories,
        &original,
        "source authentication password"
    )
    .is_err());
    let error = unlock_foreign_file(Domain::Vault, &legacy, "source authentication password")
        .expect_err("V1 metadata is absent");
    assert!(error.downcast_ref::<MissingLegacyWrapping>().is_some());
    assert_eq!(
        inspect_bytes(&legacy).expect("legacy inspection"),
        StoredFormat::LegacyInstanceBound
    );
}

fn install_v1_fixture() -> (PathBuf, PathBuf, Vec<u8>, Vec<u8>) {
    configure(all_protected(), "legacy upgrade password").expect("initialize credential vault");
    let key = KeyStore::protection_key(false).expect("existing random app master");
    let vault_path = root().expect("root").join("vault/nested/private.bin");
    let memory_path = root()
        .expect("root")
        .join("memories/nested/private.tcui-memory");
    let vault = seal(Domain::Vault.context(), b"V1 Vault body", &key).expect("V1 Vault ciphertext");
    let memory = seal(Domain::Memories.context(), br#"{"memory":"V1 fact"}"#, &key)
        .expect("V1 memory ciphertext");
    io::atomic_write(&vault_path, &vault).expect("V1 Vault file");
    io::atomic_write(&memory_path, &memory).expect("V1 Memory file");
    let mut manifest = Manifest::new(all_protected(), Some(&key)).expect("manifest");
    manifest.version = 1;
    manifest.authentication = Some(manifest.tag(&key).expect("V1 authenticated metadata"));
    io::atomic_write(
        &manifest_path().expect("manifest path"),
        &serde_json::to_vec(&manifest).expect("manifest JSON"),
    )
    .expect("V1 manifest");
    lock().expect("V1 restart");
    (vault_path, memory_path, vault, memory)
}

#[test]
fn v1_normal_unlock_upgrades_complete_native_trees_durably_and_survives_restart() {
    let environment = Environment::new();
    let (vault_path, memory_path, _, _) = install_v1_fixture();
    assert!(needs_portable_upgrade().expect("V1 upgrade needed"));
    assert!(startup_requires_unlock().expect("upgrade startup gate"));
    unlock("legacy upgrade password").expect("authenticated durable upgrade");
    assert!(!needs_portable_upgrade().expect("V2 metadata"));
    for (domain, path) in [
        (Domain::Vault, &vault_path),
        (Domain::Memories, &memory_path),
    ] {
        assert_eq!(
            inspect_bytes(&std::fs::read(path).expect("upgraded file")).expect("portable format"),
            StoredFormat::Portable { domain }
        );
    }
    assert!(!environment.data().join("protection-pending.json").exists());
    lock().expect("restart after upgrade");
    unlock("legacy upgrade password").expect("V2 restart");
    assert_eq!(
        read(Domain::Vault, &vault_path).expect("upgraded content"),
        b"V1 Vault body"
    );
    let copied = std::fs::read(&memory_path).expect("copied native memory");
    lock().expect("close source");
    select_instance(&environment, "copy-target");
    configure(ProtectionPolicy::default(), "").expect("target");
    unlock_foreign_file(Domain::Memories, &copied, "legacy upgrade password")
        .expect("password-only copy unlock");
    assert_eq!(
        decode(Domain::Memories, &copied).expect("copied body"),
        br#"{"memory":"V1 fact"}"#
    );
}

#[test]
fn v1_upgrade_refuses_active_sessions_wrong_password_and_corruption_without_replacing_sources() {
    let _environment = Environment::new();
    let (vault_path, memory_path, vault, memory) = install_v1_fixture();
    assert!(unlock("wrong legacy password").is_err());
    assert_eq!(std::fs::read(&vault_path).expect("unchanged V1"), vault);
    let active = io::session_lock().expect("other active instance");
    assert!(unlock("legacy upgrade password").is_err());
    assert!(!is_unlocked().expect("failed upgrade locks session"));
    assert_eq!(
        std::fs::read(&vault_path).expect("no active-session overwrite"),
        vault
    );
    drop(active);
    let mut corrupt = memory.clone();
    *corrupt.last_mut().expect("tag") ^= 1;
    io::atomic_write(&memory_path, &corrupt).expect("corrupt V1 source");
    assert!(unlock("legacy upgrade password").is_err());
    assert_eq!(
        std::fs::read(&vault_path).expect("preflight retains first source"),
        vault
    );
    assert_eq!(
        std::fs::read(&memory_path).expect("damaged source retained"),
        corrupt
    );
    io::atomic_write(&memory_path, &memory).expect("restore synthetic ciphertext");
    unlock("legacy upgrade password").expect("retry after restoring source");
    assert_eq!(
        read(Domain::Vault, &vault_path).expect("recover Vault"),
        b"V1 Vault body"
    );
}

#[test]
fn interrupted_v1_upgrade_resumes_authenticated_v2_targets_without_a_new_master() {
    let environment = Environment::new();
    let (vault_path, memory_path, _, _) = install_v1_fixture();
    // Install the same authenticated session that unlock establishes before prepare.
    KeyStore::unlock_vault(&AppConfig::default(), "legacy upgrade password")
        .expect("vault password");
    let key = KeyStore::protection_key(false).expect("original master");
    let old = load_manifest().expect("manifest").expect("V1");
    SESSIONS.lock().expect("sessions").insert(
        root().expect("root"),
        Session {
            key: Zeroizing::new(*key),
            wrapping: KeyStore::protection_wrapping().expect("original wrapping"),
            manifest: Some(old.clone()),
        },
    );
    let target = Manifest::new(old.policy, Some(&key)).expect("V2 target");
    migration::prepare_format_upgrade(&old, &target, &key)
        .expect("durable V1 upgrade recovery plan");
    // One installed file simulates an interrupted commit, not a read-side rewrite.
    let staged = std::fs::read_dir(environment.data())
        .expect("data entries")
        .map(|entry| entry.expect("entry").path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(".protection-migration-"))
        })
        .expect("stage");
    let staged_memory = std::fs::read_dir(&staged)
        .expect("staged entries")
        .map(|entry| entry.expect("staged entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "new"))
        .map(|path| io::read_regular(&path).expect("staged bytes"))
        .find(|bytes| portable::open_with_key(Domain::Memories, bytes, &key).is_ok())
        .expect("authenticated staged memory");
    io::atomic_write(&memory_path, &staged_memory).expect("first install");
    lock().expect("interrupted process");
    assert!(read(Domain::Vault, &vault_path).is_err());
    unlock("legacy upgrade password").expect("finish original authenticated journal");
    assert_eq!(
        *KeyStore::protection_key(false).expect("retained master"),
        *key
    );
    assert_eq!(
        read(Domain::Vault, &vault_path).expect("Vault recovered"),
        b"V1 Vault body"
    );
    assert_eq!(
        read(Domain::Memories, &memory_path).expect("Memory recovered"),
        br#"{"memory":"V1 fact"}"#
    );
    assert!(!environment.data().join("protection-pending.json").exists());
}

#[test]
fn copied_native_portable_data_requires_adoption_and_ordinary_writes_cannot_downgrade_it() {
    let environment = Environment::new();
    configure(all_protected(), "copied-store source password").expect("source protection");
    let vault = encode(Domain::Vault, b"copied private Vault body").expect("source Vault");
    let memory =
        encode(Domain::Memories, br#"{"memory":"copied private fact"}"#).expect("source memory");
    lock().expect("close source");
    select_instance(&environment, "native-adoption-target");
    configure(ProtectionPolicy::default(), "").expect("plaintext destination");
    initialize_password("different native destination password")
        .expect("existing destination password vault");
    let original_target_key = KeyStore::protection_key(false).expect("existing destination master");
    let vault_path = root().expect("target root").join("vault/nested/copied.bin");
    let memory_path = root()
        .expect("target root")
        .join("memories/nested/copied.tcui-memory");
    io::atomic_write(&vault_path, &vault).expect("copy native Vault file");
    io::atomic_write(&memory_path, &memory).expect("copy native Memory file");
    for (domain, path, ciphertext) in [
        (Domain::Vault, &vault_path, &vault),
        (Domain::Memories, &memory_path, &memory),
    ] {
        let prefix = read_regular_prefix(path, NATIVE_STORE_ADOPTION_PROBE_LEN)
            .expect("bounded regular probe");
        assert_eq!(prefix.len(), NATIVE_STORE_ADOPTION_PROBE_LEN);
        let error = require_native_store_adoption(domain, &prefix)
            .expect_err("native management needs adoption");
        assert!(read_native(domain, path)
            .expect_err("native body read requires adoption")
            .downcast_ref::<NativeStoreAdoptionNeeded>()
            .is_some());
        assert_eq!(
            error
                .downcast_ref::<NativeStoreAdoptionNeeded>()
                .expect("typed adoption")
                .domain,
            domain
        );
        let error = write(domain, path, b"must not overwrite encrypted content")
            .expect_err("no implicit downgrade");
        assert!(error.downcast_ref::<NativeStoreAdoptionNeeded>().is_some());
        assert_eq!(
            std::fs::read(path).expect("unchanged encrypted file"),
            *ciphertext
        );
    }
    unlock_foreign_file(Domain::Vault, &vault, "copied-store source password")
        .expect("authenticate source wrapping");
    assert_eq!(
        decode(Domain::Vault, &vault).expect("explicit import decode remains allowed"),
        b"copied private Vault body"
    );
    assert!(
        require_native_store_adoption(Domain::Vault, &vault).is_err(),
        "source password alone does not adopt native policy"
    );
    assert!(write(Domain::Vault, &vault_path, b"still must not downgrade").is_err());
    assert!(adopt_native_store(Domain::Vault, "wrong destination password").is_err());
    assert_eq!(
        std::fs::read(&vault_path).expect("wrong password preserves source"),
        vault
    );
    assert_eq!(
        std::fs::read(&memory_path).expect("wrong password preserves memory"),
        memory
    );
    adopt_native_store(Domain::Vault, "different native destination password")
        .expect("adopt Vault under destination protection");
    adopt_native_store(Domain::Memories, "different native destination password")
        .expect("adopt Memories under destination protection");
    let target_key = KeyStore::protection_key(false).expect("destination app master");
    assert_eq!(
        *target_key, *original_target_key,
        "adoption must retain the destination master"
    );
    for (domain, path, expected) in [
        (
            Domain::Vault,
            &vault_path,
            b"copied private Vault body".as_slice(),
        ),
        (
            Domain::Memories,
            &memory_path,
            br#"{"memory":"copied private fact"}"#.as_slice(),
        ),
    ] {
        let ciphertext = std::fs::read(path).expect("adopted ciphertext");
        require_native_store_adoption(domain, &ciphertext)
            .expect("protected native management allowed");
        assert_eq!(
            portable::open_with_key(domain, &ciphertext, &target_key)
                .expect("destination-owned encryption"),
            expected
        );
    }
    lock().expect("restart after adoption");
    unlock("different native destination password").expect("destination password only");
    assert_eq!(
        read(Domain::Vault, &vault_path).expect("adopted Vault body"),
        b"copied private Vault body"
    );
    configure(
        ProtectionPolicy::default(),
        "different native destination password",
    )
    .expect("explicit decryption migration remains allowed");
    assert_eq!(
        std::fs::read(&vault_path).expect("explicit plaintext Vault"),
        b"copied private Vault body"
    );
    assert_eq!(
        std::fs::read(&memory_path).expect("explicit plaintext Memory"),
        br#"{"memory":"copied private fact"}"#
    );
}

#[test]
fn adopting_foreign_content_in_an_already_protected_store_rewraps_it_permanently() {
    let environment = Environment::new();
    configure(all_protected(), "protected-copy source password").expect("source");
    let source = encode(Domain::Vault, b"permanent protected adoption").expect("source envelope");
    lock().expect("source lock");
    select_instance(&environment, "already-protected-target");
    let target_policy = ProtectionPolicy {
        app_password: true,
        credentials: true,
        vault: true,
        ..ProtectionPolicy::default()
    };
    configure(target_policy, "existing protected target password").expect("protected target");
    let target_key = KeyStore::protection_key(false).expect("existing target master");
    let path = root().expect("target root").join("vault/copied.bin");
    io::atomic_write(&path, &source).expect("copied portable file");
    assert!(read_native(Domain::Vault, &path)
        .expect_err("source key is not local")
        .downcast_ref::<PortableUnlockNeeded>()
        .is_some());
    unlock_foreign_file(Domain::Vault, &source, "protected-copy source password")
        .expect("source authentication");
    assert_eq!(
        read_native(Domain::Vault, &path).expect("source content"),
        b"permanent protected adoption"
    );
    adopt_native_store(Domain::Vault, "existing protected target password")
        .expect("force same-policy adoption");
    assert_eq!(policy().expect("policy"), Some(target_policy));
    assert_eq!(
        *KeyStore::protection_key(false).expect("retained target master"),
        *target_key
    );
    let adopted = std::fs::read(&path).expect("adopted envelope");
    assert_ne!(adopted, source);
    assert_eq!(
        portable::open_with_key(Domain::Vault, &adopted, &target_key)
            .expect("destination-owned wrapping"),
        b"permanent protected adoption"
    );
    lock().expect("clear source cache");
    unlock("existing protected target password").expect("destination password only");
    assert_eq!(
        read_native(Domain::Vault, &path).expect("permanent adoption after restart"),
        b"permanent protected adoption"
    );
}

#[test]
fn explicit_adoption_accepts_mixed_plaintext_only_in_the_selected_protected_domain() {
    let _environment = Environment::new();
    let password = "mixed protected native adoption password";
    configure(all_protected(), password).expect("protected destination");
    let data = root().expect("native root");
    let vault = crate::storage::chat_files::ChatSavedFileStore::new(
        data.join("vault"),
        crate::storage::paths::TcuiDataPaths::discover(),
    );
    let existing = vault
        .create_item("existing.md", b"existing encrypted Vault body", None)
        .expect("existing indexed item");
    let copied = vault
        .create_item("copied.md", b"copied plaintext Vault body", None)
        .expect("copied item identity");
    io::atomic_write(&copied.path, b"copied plaintext Vault body")
        .expect("simulate plaintext folder copy retaining its native identity");
    let memory = serde_json::json!({
        "schema_version": 1,
        "id": 73,
        "logical_path": "imported/fact.md",
        "title": "Source machine fact",
        "created_at_ms": 123,
        "updated_at_ms": 456,
        "markdown": "Keep this source fact unchanged.",
        "metadata": {
            "scope": "machine",
            "machine": {"id": "source-machine-id", "name": "Source workstation"},
            "tags": ["imported"],
            "source": {"conversation_id": 91, "conversation_title": "Source discussion"}
        }
    });
    let copied_memory = data.join("memories/copied.tcui-memory");
    let existing_memory = data.join("memories/existing.tcui-memory");
    let memory_bytes = serde_json::to_vec(&memory).expect("native memory document");
    let mut other_memory = memory.clone();
    other_memory["id"] = 74.into();
    other_memory["logical_path"] = "existing/fact.md".into();
    let other_bytes = serde_json::to_vec(&other_memory).expect("existing document");
    write(Domain::Memories, &existing_memory, &other_bytes).expect("existing encrypted memory");
    io::atomic_write(&copied_memory, &memory_bytes).expect("copied plaintext memory");

    for (domain, path) in [
        (Domain::Vault, &copied.path),
        (Domain::Memories, &copied_memory),
    ] {
        assert!(
            read_native(domain, path).is_err(),
            "ordinary protected read rejects plaintext"
        );
        let old = load_manifest().expect("manifest").expect("configured");
        let mut policy = old.policy;
        match domain {
            Domain::Vault => policy.vault = false,
            Domain::Memories => policy.memories = false,
            Domain::Chats => unreachable!(),
        }
        let key = KeyStore::protection_key(false).expect("master");
        let target = Manifest::new(policy, Some(&key)).expect("policy transition");
        assert!(
            migration::prepare(Some(&old), &target, Some(&key)).is_err(),
            "ordinary policy migration must not authorize copied plaintext"
        );
        assert!(migration::pending()
            .expect("no published migration")
            .is_none());
    }
    adopt_native_store(Domain::Vault, password).expect("explicit mixed Vault adoption");
    assert!(
        read_native(Domain::Memories, &copied_memory).is_err(),
        "Vault adoption does not authorize Memory plaintext"
    );
    adopt_native_store(Domain::Memories, password).expect("explicit mixed Memory adoption");
    lock().expect("close destination session");
    assert!(read_native(Domain::Vault, &copied.path).is_err());
    assert!(read_native(Domain::Memories, &copied_memory).is_err());
    unlock(password).expect("reopen using destination password");
    assert_eq!(
        vault
            .read_item(existing.id)
            .expect("existing Vault identity"),
        b"existing encrypted Vault body"
    );
    assert_eq!(
        vault.read_item(copied.id).expect("copied Vault identity"),
        b"copied plaintext Vault body"
    );
    let items = vault.list_items().expect("retained native item metadata");
    assert!(items.iter().any(|item| item == &existing));
    assert!(items.iter().any(|item| item == &copied));
    for (path, expected) in [
        (&copied_memory, &memory_bytes),
        (&existing_memory, &other_bytes),
    ] {
        let stored = std::fs::read(path).expect("protected native document");
        assert!(matches!(
            inspect_bytes(&stored).expect("format"),
            StoredFormat::Portable { .. }
        ));
        let decoded = read_native(Domain::Memories, path).expect("retained native memory");
        assert_eq!(
            &decoded, expected,
            "IDs, source metadata, paths and content survive adoption"
        );
    }
}

#[test]
fn adopting_memories_does_not_decrypt_or_rewrap_an_unselected_copied_vault() {
    let environment = Environment::new();
    configure(all_protected(), "mixed source password").expect("source");
    let vault = encode(Domain::Vault, b"unselected private Vault bytes").expect("source Vault");
    let memory = encode(Domain::Memories, br#"{"memory":"selected fact"}"#).expect("source memory");
    lock().expect("source lock");
    select_instance(&environment, "memory-only-adoption");
    configure(ProtectionPolicy::default(), "").expect("plaintext target");
    let vault_path = root().expect("root").join("vault/unselected.bin");
    let memory_path = root().expect("root").join("memories/selected.tcui-memory");
    io::atomic_write(&vault_path, &vault).expect("copy Vault");
    io::atomic_write(&memory_path, &memory).expect("copy memory");
    unlock_foreign_file(Domain::Memories, &memory, "mixed source password").expect("source key");
    adopt_native_store(Domain::Memories, "new native target password")
        .expect("enroll target and adopt only memories");
    assert_eq!(
        std::fs::read(&vault_path).expect("unselected ciphertext"),
        vault
    );
    assert_eq!(
        read_native(Domain::Memories, &memory_path).expect("selected body"),
        br#"{"memory":"selected fact"}"#
    );
    assert!(read_native(Domain::Vault, &vault_path)
        .expect_err("Vault still needs its own adoption")
        .downcast_ref::<NativeStoreAdoptionNeeded>()
        .is_some());
    let mut target = policy().expect("target policy").expect("configured");
    target.credentials = true;
    assert!(
        configure(target, "new native target password").is_err(),
        "normal configuration must not bypass unselected native adoption"
    );
    assert_eq!(
        std::fs::read(&vault_path).expect("no policy-change downgrade"),
        vault
    );
    lock().expect("forget source cache");
    unlock("new native target password").expect("restart target");
    assert_eq!(
        read_native(Domain::Memories, &memory_path).expect("permanent memory adoption"),
        br#"{"memory":"selected fact"}"#
    );
    assert!(decode(Domain::Vault, &vault)
        .expect_err("unselected source master was not adopted")
        .downcast_ref::<PortableUnlockNeeded>()
        .is_some());
}

#[test]
fn retained_backup_body_and_hash_descriptor_recover_together_after_interrupted_protection_commit() {
    let environment = Environment::new();
    configure(ProtectionPolicy::default(), "").expect("plaintext setup");
    let id = format!("{:032x}", rand::random::<u128>());
    let retained = root()
        .expect("root")
        .join(".native-transfer-backups")
        .join(&id);
    let body_path = retained.join("root/private.bin");
    let descriptor_path = retained.join("recovery.json");
    let body = b"private retained replacement history";
    let descriptor = serde_json::to_vec(&serde_json::json!({
        "version": 1,
        "domain": Domain::Vault,
        "id": id,
        "files": [{ "path": "root/private.bin", "hash": *blake3::hash(body).as_bytes() }],
        "root_existed": true,
        "auxiliary_existed": false,
        "protected": false,
        "complete": true
    }))
    .expect("retained descriptor");
    io::atomic_write(&body_path, body).expect("retained original body");
    io::atomic_write(&descriptor_path, &descriptor).expect("retained original descriptor");
    initialize_password("retained-history recovery password").expect("enroll existing app master");
    let old = load_manifest().expect("old metadata").expect("configured");
    let key = KeyStore::protection_key(false).expect("existing master");
    let target = Manifest::new(
        ProtectionPolicy {
            vault: true,
            ..ProtectionPolicy::default()
        },
        Some(&key),
    )
    .expect("target metadata");
    migration::prepare(Some(&old), &target, Some(&key)).expect("durable history migration plan");
    let stage = std::fs::read_dir(environment.data())
        .expect("data entries")
        .map(|entry| entry.expect("entry").path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(".protection-migration-"))
        })
        .expect("stage directory");
    let mut staged_body = None;
    let mut staged_descriptor = None;
    for entry in std::fs::read_dir(&stage).expect("stage entries") {
        let path = entry.expect("stage entry").path();
        if !path.extension().is_some_and(|extension| extension == "new") {
            continue;
        }
        let bytes = io::read_regular(&path).expect("staged bytes");
        let Ok(plaintext) = portable::open_with_key(Domain::Vault, &bytes, &key) else {
            continue;
        };
        let plaintext = Zeroizing::new(plaintext);
        if plaintext.as_slice() == body {
            staged_body = Some(bytes);
        } else if serde_json::from_slice::<serde_json::Value>(&plaintext)
            .ok()
            .is_some_and(|value| {
                value.get("id").and_then(|value| value.as_str()) == Some(id.as_str())
            })
        {
            staged_descriptor = Some((path, bytes));
        }
    }
    let staged_body = staged_body.expect("encrypted history target");
    let (staged_descriptor_path, staged_descriptor_bytes) =
        staged_descriptor.expect("encrypted rewritten descriptor");
    io::atomic_write(&body_path, &staged_body).expect("simulate first installed history entry");
    io::atomic_write(&staged_descriptor_path, b"damaged staged descriptor")
        .expect("simulate staging damage");
    lock().expect("interrupted process");
    assert!(unlock("wrong retained-history password").is_err());
    assert!(unlock("retained-history recovery password").is_err());
    assert_eq!(
        std::fs::read(&descriptor_path).expect("original descriptor retained"),
        descriptor
    );
    assert_eq!(
        std::fs::read(&body_path).expect("installed target retained"),
        staged_body
    );
    assert!(environment.data().join("protection-pending.json").exists());
    io::atomic_write(&staged_descriptor_path, &staged_descriptor_bytes)
        .expect("restore authenticated synthetic stage");
    unlock("retained-history recovery password").expect("finish original durable transaction");
    let stored_body = std::fs::read(&body_path).expect("encrypted retained body");
    let metadata: serde_json::Value = serde_json::from_slice(
        &read_native(Domain::Vault, &descriptor_path).expect("encrypted retained descriptor"),
    )
    .expect("rewritten recovery JSON");
    assert_eq!(metadata["protected"], true);
    let recorded: [u8; 32] = serde_json::from_value(metadata["files"][0]["hash"].clone())
        .expect("rewritten stored-byte hash");
    assert_eq!(recorded, *blake3::hash(&stored_body).as_bytes());
    assert_eq!(
        read_native(Domain::Vault, &body_path).expect("retained body"),
        body
    );
    assert!(!environment.data().join("protection-pending.json").exists());
    lock().expect("restart completed migration");
    unlock("retained-history recovery password").expect("restart with destination password");
    assert_eq!(
        read_native(Domain::Vault, &body_path).expect("durable retained history"),
        body
    );
}

#[test]
fn initial_configuration_prompts_for_source_unlock_and_explicit_adoption_before_creating_keys() {
    let environment = Environment::new();
    configure(all_protected(), "first-copy source password").expect("source setup");
    let vault = encode(Domain::Vault, b"copied before initial setup").expect("source Vault");
    let memory =
        encode(Domain::Memories, br#"{"memory":"first-copy fact"}"#).expect("source memory");
    lock().expect("close source");
    select_instance(&environment, "first-run-copied-native");
    let vault_path = root()
        .expect("target root")
        .join("vault/copied-before-setup.bin");
    let memory_path = root()
        .expect("target root")
        .join("memories/copied-before-setup.tcui-memory");
    io::atomic_write(&vault_path, &vault).expect("copy Vault without instance credentials");
    io::atomic_write(&memory_path, &memory).expect("copy Memories without instance credentials");
    let target = all_protected();
    let error = configure(target, "first-copy destination password")
        .expect_err("source unlock must precede first setup");
    assert!(error.downcast_ref::<PortableUnlockNeeded>().is_some());
    assert!(load_manifest().expect("no policy change").is_none());
    assert!(
        !root().expect("root").join("tcui.db").exists(),
        "source prompt must not generate an orphan destination key"
    );
    assert_eq!(std::fs::read(&vault_path).expect("unchanged source"), vault);
    assert!(unlock_foreign_file(Domain::Vault, &vault, "wrong source password").is_err());
    assert!(!root().expect("root").join("tcui.db").exists());
    unlock_foreign_file(Domain::Vault, &vault, "first-copy source password")
        .expect("source authentication");
    for domain in [Domain::Vault, Domain::Memories] {
        assert_eq!(
            preflight_native_adoption(domain)
                .expect_err("explicit adoption is still required")
                .downcast_ref::<NativeStoreAdoptionNeeded>()
                .expect("typed adoption")
                .domain,
            domain
        );
    }
    assert!(configure(target, "first-copy destination password")
        .expect_err("configuration is not an implicit adoption bypass")
        .downcast_ref::<NativeStoreAdoptionNeeded>()
        .is_some());
    assert!(!root().expect("root").join("tcui.db").exists());
    adopt_native_store(Domain::Vault, "first-copy destination password")
        .expect("bootstrap protected Vault");
    adopt_native_store(Domain::Memories, "first-copy destination password")
        .expect("bootstrap protected Memories");
    configure(target, "first-copy destination password")
        .expect("finish remaining app and chat options");
    assert_eq!(policy().expect("configured policy"), Some(target));
    lock().expect("clear source cache");
    unlock("first-copy destination password").expect("restart uses only destination password");
    assert_eq!(
        read_native(Domain::Vault, &vault_path).expect("native Vault"),
        b"copied before initial setup"
    );
    assert_eq!(
        read_native(Domain::Memories, &memory_path).expect("native memory"),
        br#"{"memory":"first-copy fact"}"#
    );
}

#[test]
fn v1_format_upgrade_preserves_foreign_portable_files_and_metadata_without_source_unlock() {
    let environment = Environment::new();
    configure(all_protected(), "foreign V2 source password").expect("source setup");
    let foreign_vault =
        encode(Domain::Vault, b"foreign portable Vault body").expect("foreign Vault");
    let foreign_memory =
        encode(Domain::Memories, br#"{"memory":"foreign portable fact"}"#).expect("foreign memory");
    let foreign_journal =
        encode(Domain::Vault, b"opaque foreign journal framing").expect("foreign native journal");
    let foreign_descriptor = encode(Domain::Vault, b"opaque foreign retained descriptor")
        .expect("foreign retained descriptor");
    lock().expect("close source");
    select_instance(&environment, "legacy-target-with-copies");
    let (local_vault, local_memory, _, _) = install_v1_fixture();
    let copied_vault = root().expect("root").join("vault/foreign-portable.bin");
    let copied_memory = root()
        .expect("root")
        .join("memories/foreign-portable.tcui-memory");
    let copied_journal = root()
        .expect("root")
        .join("vault-native-ops/11111111111111111111111111111111/journal.json");
    let copied_descriptor = root()
        .expect("root")
        .join(".native-transfer-backups/22222222222222222222222222222222/recovery.json");
    let copied_backup_body = copied_descriptor
        .parent()
        .expect("backup parent")
        .join("root/foreign.bin");
    for (path, bytes) in [
        (&copied_vault, &foreign_vault),
        (&copied_memory, &foreign_memory),
        (&copied_journal, &foreign_journal),
        (&copied_descriptor, &foreign_descriptor),
        (&copied_backup_body, &foreign_vault),
    ] {
        io::atomic_write(path, bytes).expect("copy portable source without its credentials");
    }
    unlock("legacy upgrade password")
        .expect("format-only unlock must not need foreign V2 passwords");
    assert!(!needs_portable_upgrade().expect("V2 local manifest"));
    assert_eq!(
        read_native(Domain::Vault, &local_vault).expect("upgraded local Vault"),
        b"V1 Vault body"
    );
    assert_eq!(
        read_native(Domain::Memories, &local_memory).expect("upgraded local memory"),
        br#"{"memory":"V1 fact"}"#
    );
    for (path, bytes) in [
        (&copied_vault, &foreign_vault),
        (&copied_memory, &foreign_memory),
        (&copied_journal, &foreign_journal),
        (&copied_descriptor, &foreign_descriptor),
        (&copied_backup_body, &foreign_vault),
    ] {
        assert_eq!(
            std::fs::read(path).expect("byte-exact portable preservation"),
            *bytes
        );
    }
    assert!(read_native(Domain::Vault, &copied_vault)
        .expect_err("foreign body still needs its source password")
        .downcast_ref::<PortableUnlockNeeded>()
        .is_some());
    lock().expect("restart upgraded target");
    unlock("legacy upgrade password")
        .expect("local restart remains independent of source authentication");
    assert_eq!(
        read_native(Domain::Vault, &local_vault).expect("durable local upgrade"),
        b"V1 Vault body"
    );
    unlock_foreign_file(Domain::Vault, &foreign_vault, "foreign V2 source password")
        .expect("explicit later source authentication");
    assert_eq!(
        decode(Domain::Vault, &foreign_vault).expect("later source decode"),
        b"foreign portable Vault body"
    );
    assert_eq!(
        std::fs::read(&copied_vault).expect("source file was not implicitly adopted"),
        foreign_vault
    );
}

#[test]
fn vault_only_format_upgrade_leaves_unselected_memory_records_and_cache_unchanged() {
    let _environment = Environment::new();
    let (vault_path, memory_path, _, _) = install_v1_fixture();
    KeyStore::unlock_vault(&AppConfig::default(), "legacy upgrade password")
        .expect("authenticate fixture master");
    let key = KeyStore::protection_key(false).expect("existing master");
    let mut manifest = load_manifest().expect("V1 metadata").expect("configured");
    manifest.policy.memories = false;
    manifest.authentication = Some(
        manifest
            .tag(&key)
            .expect("authenticate Vault-only V1 policy"),
    );
    let memory = br#"{"memory":"unselected plaintext memory"}"#;
    io::atomic_write(&memory_path, memory).expect("plaintext unselected memory");
    io::atomic_write(
        &manifest_path().expect("metadata path"),
        &serde_json::to_vec(&manifest).expect("V1 JSON"),
    )
    .expect("Vault-only V1 metadata");
    let cache_path = root().expect("root").join("memory.sqlite3");
    io::atomic_write(&cache_path, b"unselected derived cache sentinel").expect("unselected cache");
    lock().expect("restart Vault-only V1 instance");
    unlock("legacy upgrade password").expect("upgrade only selected Vault data");
    assert_eq!(
        inspect_bytes(&std::fs::read(&vault_path).expect("Vault ciphertext"))
            .expect("upgraded envelope"),
        StoredFormat::Portable {
            domain: Domain::Vault
        }
    );
    assert_eq!(
        std::fs::read(&memory_path).expect("unselected memory"),
        memory
    );
    assert_eq!(
        std::fs::read(&cache_path).expect("unselected cache remains"),
        b"unselected derived cache sentinel"
    );
}

#[test]
fn entry_and_credential_choices_preserve_unchanged_protected_domains_without_foreign_passwords() {
    let environment = Environment::new();
    configure(all_protected(), "unchanged-domain source password").expect("source");
    let foreign_vault =
        encode(Domain::Vault, b"foreign protected Vault data").expect("source Vault");
    let foreign_memory =
        encode(Domain::Memories, br#"{"memory":"foreign protected fact"}"#).expect("source memory");
    let foreign_journal =
        encode(Domain::Vault, b"opaque foreign protected journal").expect("source journal");
    lock().expect("close source");
    select_instance(&environment, "independent-protected-choices");
    let mut target = ProtectionPolicy {
        vault: true,
        memories: true,
        ..ProtectionPolicy::default()
    };
    configure(target, "unchanged-domain target password").expect("protected target");
    let master = KeyStore::protection_key(false).expect("existing master");
    let wrapping = KeyStore::protection_wrapping().expect("existing master wrapping");
    let local_path = root().expect("root").join("vault/local.bin");
    let local = encode(Domain::Vault, b"existing local protected data").expect("local ciphertext");
    let vault_path = root().expect("root").join("vault/foreign.bin");
    let memory_path = root().expect("root").join("memories/foreign.tcui-memory");
    let journal_path = root()
        .expect("root")
        .join("vault-native-ops/33333333333333333333333333333333/journal.json");
    let cache_path = root().expect("root").join("memory.sqlite3");
    for (path, bytes) in [
        (&local_path, &local),
        (&vault_path, &foreign_vault),
        (&memory_path, &foreign_memory),
        (&journal_path, &foreign_journal),
        (&cache_path, &foreign_memory),
    ] {
        io::atomic_write(path, bytes).expect("existing protected bytes");
    }
    target.app_password = true;
    configure(target, "unchanged-domain target password")
        .expect("entry-only change must not need foreign source authentication");
    target.credentials = true;
    configure(target, "unchanged-domain target password")
        .expect("credential-only change must not need foreign source authentication");
    assert_eq!(
        policy().expect("published independent choices"),
        Some(target)
    );
    assert_eq!(
        KeyStore::vault_status(&AppConfig::default())
            .expect("credential preference")
            .storage,
        crate::config::key_store::CredentialStorage::Encrypted
    );
    assert_eq!(
        *KeyStore::protection_key(false).expect("stable master"),
        *master
    );
    assert!(KeyStore::protection_wrapping().expect("stable master wrapping") == wrapping);
    for (path, bytes) in [
        (&local_path, &local),
        (&vault_path, &foreign_vault),
        (&memory_path, &foreign_memory),
        (&journal_path, &foreign_journal),
        (&cache_path, &foreign_memory),
    ] {
        assert_eq!(
            std::fs::read(path).expect("unchanged protected ciphertext"),
            *bytes
        );
    }
    assert_eq!(
        read_native(Domain::Vault, &local_path).expect("local protected data"),
        b"existing local protected data"
    );
    assert!(read_native(Domain::Vault, &vault_path)
        .expect_err("unchanged foreign content still needs source authentication")
        .downcast_ref::<PortableUnlockNeeded>()
        .is_some());
    assert!(read_native(Domain::Memories, &memory_path)
        .expect_err("unchanged foreign memory still needs source authentication")
        .downcast_ref::<PortableUnlockNeeded>()
        .is_some());
    lock().expect("entry gate");
    assert!(read_native(Domain::Vault, &local_path).is_err());
    unlock("unchanged-domain target password")
        .expect("local restart needs only destination authentication");
    assert_eq!(
        read_native(Domain::Vault, &local_path).expect("local body after restart"),
        b"existing local protected data"
    );
    assert!(read_native(Domain::Vault, &vault_path)
        .expect_err("restart does not unlock foreign sources")
        .downcast_ref::<PortableUnlockNeeded>()
        .is_some());
}
