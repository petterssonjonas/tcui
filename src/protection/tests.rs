use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::MutexGuard;

use super::*;
use crate::storage::crypto::{decrypt_serialized, encrypt_serialized, SharedKey};
use crate::storage::paths::TcuiDataPaths;
use crate::storage::Storage;

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
    let content =
        Storage::encrypt_shared_text("legacy SQLite private body").expect("legacy cipher");
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
        .create_conversation_snapshot(1, "readable private title", &[])
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
        .create_conversation_snapshot(2, "password-only title", &[])
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
        .create_conversation_snapshot(1, "not encrypted at rest", &[])
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
        .starts_with(MAGIC));
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
        .starts_with(MAGIC));
    assert!(std::fs::read(&second)
        .expect("protected second")
        .starts_with(MAGIC));
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
        .starts_with(MAGIC));
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
        .create_conversation_snapshot(0, "keep this plaintext chat", &[])
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
        .create_conversation_snapshot(1, "Archived conversation", &[])
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
