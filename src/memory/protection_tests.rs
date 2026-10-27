use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::index::MemoryIndex;
use super::metadata::{MemoryMachine, MemoryMetadata, MemoryScope};
use super::store::{MemoryDocument, MemoryStore};
use crate::protection::{self, Domain, ProtectionPolicy};
use crate::storage::crypto::{read_encrypted_document, SharedKey};
use crate::storage::paths::TcuiDataPaths;

pub(super) struct Fixture {
    root: PathBuf,
    previous: Vec<(&'static str, Option<OsString>)>,
}

impl Fixture {
    pub(super) fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("tcui-memory-protection-{}", rand::random::<u64>()));
        std::fs::create_dir_all(&root).expect("fixture root");
        let previous = ["XDG_DATA_HOME", "XDG_CONFIG_HOME", "XDG_CACHE_HOME"]
            .into_iter()
            .map(|name| {
                let previous = std::env::var_os(name);
                std::env::set_var(name, root.join(name));
                (name, previous)
            })
            .collect();
        Self { root, previous }
    }

    pub(super) fn configure(&self, protected: bool) {
        protection::configure(
            ProtectionPolicy {
                memories: protected,
                ..ProtectionPolicy::default()
            },
            "memory-test-password",
        )
        .expect("configure memory protection");
    }
}

impl Drop for Fixture {
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

fn metadata() -> MemoryMetadata {
    MemoryMetadata {
        scope: MemoryScope::Machine,
        machine: Some(MemoryMachine {
            id: "desktop".to_string(),
            name: "Desktop".to_string(),
        }),
        ..MemoryMetadata::default()
    }
}

#[test]
fn default_memories_work_without_a_vault_and_do_not_follow_external_profiles() {
    let _guard = crate::test_support::env_lock().lock().expect("env lock");
    let fixture = Fixture::new();
    let store = MemoryStore::open_default().expect("independent store");
    store
        .write(Path::new("editor.md"), "# Editor\n\nUse Helix.\n", false)
        .expect("save without vault");
    let key = SharedKey::load_or_create_default(&TcuiDataPaths::discover())
        .expect("legacy key")
        .key;
    for profile in ["first-profile", "second-profile"] {
        let root = fixture.root.join(profile);
        std::fs::create_dir(&root).expect("external profile");
        let reopened =
            MemoryStore::open_with_key(&root, &MemoryStore::default_cache_path(), key.clone())
                .expect("explicit profile context");
        assert_eq!(reopened.paths.root(), store.paths.root());
        assert_eq!(
            reopened
                .read(Path::new("editor.md"))
                .expect("same managed memory"),
            "# Editor\n\nUse Helix.\n"
        );
        assert!(
            !root.join("memories").exists(),
            "opening a store must not create external directories"
        );
    }
    let mut config = crate::config::AppConfig::default();
    config.memory.enabled = true;
    config.memory.min_similarity = -1.0;
    let recall =
        super::host::recall(&config, "Helix editor").expect("contextual recall without vault");
    assert_eq!(recall.titles, vec!["Editor"]);
}

#[test]
fn configured_unprotected_documents_are_plain_json_without_creating_chat_key() {
    let _guard = crate::test_support::env_lock().lock().expect("env lock");
    let fixture = Fixture::new();
    fixture.configure(false);
    let store = MemoryStore::open_default().expect("unprotected store");
    store
        .write(Path::new("editor.md"), "# Editor\n\nUse Helix.\n", false)
        .expect("plaintext save");
    let documents = store.active_documents().expect("documents");
    let raw = std::fs::read(&documents[0].0).expect("stored document");
    let document: MemoryDocument = serde_json::from_slice(&raw).expect("plaintext JSON");
    assert_eq!(document.markdown, "# Editor\n\nUse Helix.\n");
    assert!(!TcuiDataPaths::discover().chat_key.exists());
    drop(store);
    let reopened = MemoryStore::open_default().expect("restart unprotected");
    assert_eq!(
        reopened
            .search("Helix editor", 1)
            .expect("search after restart")[0]
            .title,
        "Editor"
    );
}

#[test]
fn protected_documents_and_index_deny_locked_access_and_survive_restart() {
    let _guard = crate::test_support::env_lock().lock().expect("env lock");
    let fixture = Fixture::new();
    let legacy_key = SharedKey::load_or_create_default(&TcuiDataPaths::discover())
        .expect("legacy key")
        .key;
    fixture.configure(true);
    let store = MemoryStore::open_default().expect("protected store");
    store
        .remember_context("# Editor\n\nUse Helix for Rust.\n", metadata())
        .expect("protected save");
    let documents = store.active_documents().expect("documents");
    let physical = documents[0].0.clone();
    let logical = documents[0].1.logical_path.clone();
    let raw = std::fs::read(&physical).expect("encrypted document");
    assert!(!raw
        .windows(b"Use Helix".len())
        .any(|window| window == b"Use Helix"));
    assert!(read_encrypted_document::<MemoryDocument>(&physical, &legacy_key, "memory").is_err());
    let index_bytes = std::fs::read(&store.database).expect("encrypted index snapshot");
    assert!(!index_bytes.starts_with(b"SQLite format 3"));
    for suffix in ["-wal", "-shm", "-journal"] {
        assert!(!PathBuf::from(format!("{}{suffix}", store.database.display())).exists());
    }

    protection::lock().expect("lock");
    assert!(MemoryStore::open_default().is_err());
    assert!(MemoryIndex::open(&store.database).is_err());
    assert!(store.active_documents().is_err());
    assert!(store.list_files().is_err());
    assert!(store.read(&logical).is_err());
    assert!(store.read_document_at(&physical).is_err());
    assert!(
        store.search("editor", 0).is_err(),
        "zero results must not bypass lock"
    );
    assert!(store.reindex().is_err());
    assert!(store.forget(&logical).is_err());
    assert!(store
        .write(Path::new("blocked.md"), "Blocked", false)
        .is_err());
    assert!(store
        .add_from_plaintext(&fixture.root.join("absent.md"))
        .is_err());
    assert_eq!(std::fs::read(&physical).expect("unchanged document"), raw);
    assert_eq!(
        std::fs::read(&store.database).expect("unchanged index"),
        index_bytes
    );
    assert!(protection::unlock("wrong-password").is_err());
    assert!(store.status().is_err());
    protection::unlock("memory-test-password").expect("unlock all domains");
    drop(store);
    let reopened = MemoryStore::open_default().expect("restart protected");
    let hits = reopened
        .search_scoped(
            "Rust editor",
            1,
            Some(MemoryScope::Machine),
            Some("desktop"),
        )
        .expect("protected semantic search after restart");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].metadata.as_ref(), Some(&metadata()));
    assert_eq!(hits[0].path, logical);
    assert_eq!(
        protection::read(Domain::Memories, &physical).expect("master decoding"),
        serde_json::to_vec(&documents[0].1).expect("document JSON")
    );
}

#[test]
fn configuring_protection_migrates_legacy_active_and_trash_records_without_losing_search() {
    let _guard = crate::test_support::env_lock().lock().expect("env lock");
    let fixture = Fixture::new();
    let legacy = MemoryStore::open_default().expect("legacy store");
    legacy
        .write(
            Path::new("active.md"),
            "# Active\n\nUse Helix for Rust.\n",
            false,
        )
        .expect("legacy active");
    legacy
        .write(
            Path::new("removed.md"),
            "# Removed\n\nKeep historical configuration.\n",
            false,
        )
        .expect("legacy removed");
    let trash = legacy
        .forget(Path::new("removed.md"))
        .expect("legacy trash");
    let active = legacy.active_documents().expect("legacy documents")[0]
        .0
        .clone();
    drop(legacy);
    fixture.configure(true);
    let migrated = MemoryStore::open_default().expect("migrated store");
    assert_eq!(
        migrated
            .read(Path::new("active.md"))
            .expect("migrated active"),
        "# Active\n\nUse Helix for Rust.\n"
    );
    assert_eq!(
        migrated
            .read_document_at(&trash)
            .expect("migrated trash")
            .title,
        "Removed"
    );
    assert_eq!(
        migrated.search("Rust editor", 1).expect("migrated search")[0].title,
        "Active"
    );
    protection::configure(ProtectionPolicy::default(), "memory-test-password")
        .expect("explicit unprotected cutover");
    let plaintext = MemoryStore::open_default().expect("plaintext store");
    let document: MemoryDocument =
        serde_json::from_slice(&std::fs::read(&active).expect("plaintext active bytes"))
            .expect("migrated plaintext JSON");
    assert_eq!(document.title, "Active");
    let trashed: MemoryDocument =
        serde_json::from_slice(&std::fs::read(&trash).expect("plaintext trash bytes"))
            .expect("plaintext trash JSON");
    assert_eq!(trashed.title, "Removed");
    assert_eq!(
        plaintext
            .search("Rust editor", 1)
            .expect("search after unprotect")[0]
            .title,
        "Active"
    );
}

#[test]
fn protected_index_snapshot_restores_metadata_and_vectors_without_plaintext_sidecars() {
    let _guard = crate::test_support::env_lock().lock().expect("env lock");
    let fixture = Fixture::new();
    fixture.configure(true);
    let database = MemoryStore::default_cache_path();
    let index = MemoryIndex::open(&database).expect("protected index");
    index.connection().expect("unlocked index").execute(
        "INSERT INTO memory_files(id,rel_path,modified_ns,size_bytes,content_fingerprint,scope,machine_key)
         VALUES (1,'private-source.tcui-memory',1,1,1,'machine','private-machine-metadata')",
        [],
    ).expect("index metadata");
    index
        .connection()
        .expect("unlocked index")
        .execute(
            "INSERT INTO memory_chunks(id,file_id,ordinal,start_byte,end_byte) VALUES (1,1,0,0,1)",
            [],
        )
        .expect("index chunk");
    let vector = super::embedding::as_blob(&vec![0.01_f32; super::embedding::DIMENSIONS]);
    index
        .connection()
        .expect("unlocked index")
        .execute(
            "INSERT INTO vec_memory_chunks(chunk_id,embedding) VALUES (1,?1)",
            [&vector],
        )
        .expect("index vector");
    index.persist().expect("encrypted snapshot");
    drop(index);
    let mut reopened = MemoryIndex::open(&database).expect("restore snapshot");
    let machine: String = reopened
        .connection()
        .expect("unlocked index")
        .query_row(
            "SELECT machine_key FROM memory_files WHERE id=1",
            [],
            |row| row.get(0),
        )
        .expect("restored metadata");
    assert_eq!(machine, "private-machine-metadata");
    let id: i64 = reopened
        .connection()
        .expect("unlocked index")
        .query_row(
            "SELECT chunk_id FROM vec_memory_chunks WHERE embedding MATCH ?1 AND k=1",
            [&vector],
            |row| row.get(0),
        )
        .expect("restored semantic search");
    assert_eq!(id, 1);
    let raw = std::fs::read(&database).expect("stored snapshot");
    assert!(!raw
        .windows(b"private-machine-metadata".len())
        .any(|window| window == b"private-machine-metadata"));
    for suffix in ["-wal", "-shm", "-journal"] {
        assert!(!PathBuf::from(format!("{}{suffix}", database.display())).exists());
    }
    protection::lock().expect("lock");
    assert!(reopened.meta("model_id").is_err());
    assert!(reopened.persist().is_err());
    assert!(reopened.connection().is_err());
    assert!(reopened.connection_mut().is_err());
    assert!(reopened.set_meta_for_test("model_id", "blocked").is_err());
}

#[test]
fn protected_store_rejects_plaintext_documents_and_indexes_without_replacing_them() {
    let _guard = crate::test_support::env_lock().lock().expect("env lock");
    let fixture = Fixture::new();
    fixture.configure(true);
    let store = MemoryStore::open_default().expect("protected store");
    store
        .write(
            Path::new("note.md"),
            "# Note\n\nProtected content.\n",
            false,
        )
        .expect("protected save");
    let document = store.active_documents().expect("documents").remove(0);
    let plaintext = serde_json::to_vec(&document.1).expect("plaintext document");
    std::fs::write(&document.0, &plaintext).expect("tampered plaintext");
    assert!(store.read_document_at(&document.0).is_err());
    assert_eq!(
        std::fs::read(&document.0).expect("unchanged plaintext"),
        plaintext
    );
    std::fs::write(&store.database, b"SQLite format 3\0plaintext index").expect("tampered index");
    assert!(MemoryIndex::open(&store.database).is_err());
    assert_eq!(
        std::fs::read(&store.database).expect("unchanged invalid snapshot"),
        b"SQLite format 3\0plaintext index"
    );
}

#[test]
fn corrupt_protected_index_does_not_block_source_access_or_explicit_repair() {
    let _guard = crate::test_support::env_lock().lock().expect("env lock");
    let fixture = Fixture::new();
    fixture.configure(true);
    let store = MemoryStore::open_default().expect("protected store");
    store
        .write(
            Path::new("editor.md"),
            "# Editor\n\nUse Helix for Rust.\n",
            false,
        )
        .expect("note");
    let database = store.database.clone();
    let damaged = b"damaged encrypted snapshot";
    std::fs::write(&database, damaged).expect("corrupt derived index");
    drop(store);
    let reopened = MemoryStore::open_default().expect("source store opens independently of cache");
    assert_eq!(
        reopened
            .read(Path::new("editor.md"))
            .expect("intact source"),
        "# Editor\n\nUse Helix for Rust.\n"
    );
    assert_eq!(
        reopened.list_files().expect("intact catalog"),
        vec![(PathBuf::from("editor.md"), "Editor".to_string())]
    );
    assert!(reopened.search("Rust editor", 1).is_err());
    assert_eq!(
        std::fs::read(&database).expect("preserved damaged index"),
        damaged
    );
    assert_eq!(reopened.reindex().expect("explicit repair").files, 1);
    assert_eq!(
        reopened
            .search("Rust editor", 1)
            .expect("search after repair")[0]
            .title,
        "Editor"
    );
    assert_ne!(
        std::fs::read(&database).expect("repaired encrypted snapshot"),
        damaged
    );
}

#[test]
fn failed_explicit_rebuild_preserves_the_existing_derived_snapshot() {
    let _guard = crate::test_support::env_lock().lock().expect("env lock");
    let fixture = Fixture::new();
    fixture.configure(true);
    let store = MemoryStore::open_default().expect("protected store");
    store
        .write(Path::new("editor.md"), "# Editor\n\nUse Helix.\n", false)
        .expect("note");
    let physical = store.active_documents().expect("documents")[0].0.clone();
    let snapshot = std::fs::read(&store.database).expect("existing encrypted snapshot");
    protection::write(Domain::Memories, &physical, b"invalid document JSON")
        .expect("damaged document");
    assert!(store.reindex().is_err());
    assert_eq!(
        std::fs::read(&store.database).expect("preserved snapshot"),
        snapshot
    );
}

#[test]
fn unselected_memory_remains_usable_when_only_another_domain_is_locked() {
    let _guard = crate::test_support::env_lock().lock().expect("env lock");
    let _fixture = Fixture::new();
    protection::configure(
        ProtectionPolicy {
            vault: true,
            ..ProtectionPolicy::default()
        },
        "memory-test-password",
    )
    .expect("independent vault protection");
    let store = MemoryStore::open_default().expect("unprotected memory");
    store
        .write(Path::new("editor.md"), "# Editor\n\nUse Helix.\n", false)
        .expect("note");
    protection::lock().expect("lock selected vault");
    assert!(protection::require(Domain::Vault).is_err());
    assert_eq!(
        store
            .read(Path::new("editor.md"))
            .expect("unselected memory read"),
        "# Editor\n\nUse Helix.\n"
    );
}
