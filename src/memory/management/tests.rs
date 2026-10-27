use std::path::{Path, PathBuf};

use super::*;
use crate::memory::metadata::{MemoryMachine, MemoryScope, MemorySource};
use crate::memory::protection_tests::Fixture;
use crate::protection::{NativeStoreAdoptionNeeded, ProtectionPolicy};

fn document(id: u64, logical_path: &str) -> MemoryDocument {
    MemoryDocument {
        schema_version: 1,
        id,
        logical_path: logical_path.into(),
        title: "Source editor".to_string(),
        created_at_ms: 123,
        updated_at_ms: 456,
        markdown: "# Source editor\n\nUse Helix on the source machine.\n".to_string(),
        metadata: Some(MemoryMetadata {
            scope: MemoryScope::Machine,
            machine: Some(MemoryMachine {
                id: "source-machine".to_string(),
                name: "Source desktop".to_string(),
            }),
            tags: vec!["editor".to_string(), "source-only".to_string()],
            source: Some(MemorySource {
                conversation_id: Some(82),
                conversation_title: "Source conversation".to_string(),
            }),
        }),
    }
}

fn bytes(document: &MemoryDocument) -> Vec<u8> {
    serde_json::to_vec(document).expect("document JSON")
}

fn assert_adoption(error: MemoryError) {
    let MemoryError::Protection(error) = error else {
        panic!("expected native adoption protection error, got {error}");
    };
    assert_eq!(
        error.downcast_ref::<NativeStoreAdoptionNeeded>(),
        Some(&NativeStoreAdoptionNeeded {
            domain: Domain::Memories,
        })
    );
}

#[test]
fn numeric_edit_preserves_provenance_identity_and_exact_user_text() {
    let _guard = crate::test_support::env_lock().lock().expect("env lock");
    let fixture = Fixture::new();
    fixture.configure(true);
    let store = MemoryStore::open_default().expect("store");
    let original = document(42, "preferences/editor.md");
    let record = store
        .import_document(&bytes(&original))
        .expect("native import");
    assert_eq!(record.id, original.id);
    assert_eq!(record.size_bytes, original.markdown.len() as u64);
    let edited = "# New editor\r\n\r\nKeep exact trailing spaces.  ";
    let changed = store
        .replace_entry_if_unchanged(42, edited, blake3::hash(original.markdown.as_bytes()))
        .expect("numeric edit");
    let exported: MemoryDocument =
        serde_json::from_slice(&store.export_document(42).expect("export"))
            .expect("native document");
    assert_eq!(changed.id, original.id);
    assert_eq!(changed.logical_path, original.logical_path);
    assert_eq!(exported.metadata, original.metadata);
    assert_eq!(exported.created_at_ms, original.created_at_ms);
    assert!(exported.updated_at_ms > original.updated_at_ms);
    assert_eq!(store.read_entry(42).expect("numeric read"), edited);
    assert_eq!(exported.markdown, edited);
    store.trash_entry(42).expect("trash by identity");
    assert!(store.read_entry(42).is_err());
    assert!(store.list_entries().expect("empty list").is_empty());
    assert!(store.paths.trash_document_path(42).exists());
}

#[test]
fn concurrent_conditional_edits_keep_one_winner_and_reject_stale_drafts_without_metadata_loss() {
    let _guard = crate::test_support::env_lock().lock().expect("env lock");
    let fixture = Fixture::new();
    fixture.configure(true);
    let store = MemoryStore::open_default().expect("store");
    let original = document(42, "nested/editor.md");
    store.import_document(&bytes(&original)).expect("source");
    let expected = blake3::hash(store.read_entry(42).expect("editor snapshot").as_bytes());
    let other = MemoryStore::open_default().expect("independent writer");
    let first_draft = "# First draft\r\n\r\nExact first text.  ";
    let second_draft = "# Second draft\n\nExact second text.  ";
    let gate = std::sync::Barrier::new(2);
    let (first, second) = std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            gate.wait();
            store.replace_entry_if_unchanged(42, first_draft, expected)
        });
        let second = scope.spawn(|| {
            gate.wait();
            other.replace_entry_if_unchanged(42, second_draft, expected)
        });
        (
            first.join().expect("first editor"),
            second.join().expect("second editor"),
        )
    });
    assert_ne!(
        first.is_ok(),
        second.is_ok(),
        "exactly one editor may commit the source snapshot"
    );
    let (winner, stale_draft, rejected) = match (first, second) {
        (Ok(record), Err(error)) => (record, second_draft, error),
        (Err(error), Ok(record)) => (record, first_draft, error),
        results => panic!("expected one winner, got {results:?}"),
    };
    assert!(
        matches!(rejected, MemoryError::EntryChanged { id: 42 })
            || matches!(&rejected, MemoryError::Invalid(reason) if reason.contains("memory store is busy"))
    );
    assert_eq!(winner.id, original.id);
    assert_eq!(winner.logical_path, original.logical_path);
    assert_eq!(winner.metadata, original.metadata);
    assert_eq!(winner.created_at_ms, original.created_at_ms);
    let winning_body = store.read_entry(42).expect("winning source");
    assert!(winning_body == first_draft || winning_body == second_draft);
    assert_ne!(winning_body, stale_draft);
    let source_before = std::fs::read(store.paths.active_document_path(42)).expect("source bytes");
    let index_before = std::fs::read(&store.database).expect("index bytes");
    assert!(matches!(
        store.replace_entry_if_unchanged(42, stale_draft, expected),
        Err(MemoryError::EntryChanged { id: 42 })
    ));
    assert_eq!(
        std::fs::read(store.paths.active_document_path(42)).expect("unchanged source"),
        source_before
    );
    assert_eq!(
        std::fs::read(&store.database).expect("unchanged index"),
        index_before
    );
    let fresh = blake3::hash(winning_body.as_bytes());
    store
        .replace_entry_if_unchanged(42, stale_draft, fresh)
        .expect("explicit edit of the current version");
    assert_eq!(store.read_entry(42).expect("new source"), stale_draft);
    let exported: MemoryDocument =
        serde_json::from_slice(&store.export_document(42).expect("export"))
            .expect("native document");
    assert_eq!(exported.metadata, original.metadata);
    assert_eq!(exported.created_at_ms, original.created_at_ms);
}

#[test]
fn native_add_conflicts_keep_both_bodies_and_allocate_free_ids_and_paths() {
    let _guard = crate::test_support::env_lock().lock().expect("env lock");
    let fixture = Fixture::new();
    fixture.configure(false);
    let store = MemoryStore::open_default().expect("store");
    let original = document(42, "nested/editor.md");
    store
        .import_document(&bytes(&original))
        .expect("first source");
    let mut incoming = original.clone();
    incoming.markdown =
        "# Incoming\n\nAlready stored valid user content: password=example.\n".to_string();
    let conflicting = store
        .import_document(&bytes(&incoming))
        .expect("conflicting addition");
    assert_ne!(conflicting.id, original.id);
    assert_eq!(
        conflicting.logical_path,
        PathBuf::from("nested/editor (2).md")
    );
    assert_eq!(conflicting.metadata, original.metadata);
    assert_eq!(
        store.read_entry(42).expect("unchanged old body"),
        original.markdown
    );
    assert_eq!(
        store.read_entry(conflicting.id).expect("incoming body"),
        incoming.markdown
    );
    let third = store
        .import_document(&bytes(&incoming))
        .expect("third addition");
    assert_ne!(third.id, conflicting.id);
    assert_eq!(third.logical_path, PathBuf::from("nested/editor (3).md"));
    let mut name_only = original.clone();
    name_only.id = 77;
    let name_conflict = store
        .import_document(&bytes(&name_only))
        .expect("name-only conflict");
    assert_ne!(name_conflict.id, 77);
    let mut id_only = original;
    id_only.logical_path = "unique.md".into();
    let id_conflict = store
        .import_document(&bytes(&id_only))
        .expect("ID-only conflict");
    assert_ne!(id_conflict.id, 42);
    assert_eq!(id_conflict.logical_path, PathBuf::from("unique.md"));
    let records = store.list_entries().expect("all retained");
    assert_eq!(records.len(), 5);
    assert!(records.windows(2).all(|pair| pair[0].id < pair[1].id));
}

#[test]
fn native_export_import_survives_distinct_instances_and_plain_protected_policies() {
    let _guard = crate::test_support::env_lock().lock().expect("env lock");
    let original = document(42, "source/editor.md");
    let exported = {
        let fixture = Fixture::new();
        fixture.configure(true);
        let store = MemoryStore::open_default().expect("source store");
        store
            .import_document(&bytes(&original))
            .expect("source import");
        store
            .export_document(original.id)
            .expect("explicit native export")
    };
    for protected in [false, true] {
        let _fixture = Fixture::new();
        protection::configure(
            ProtectionPolicy {
                memories: protected,
                ..ProtectionPolicy::default()
            },
            "different-target-password",
        )
        .expect("target protection");
        let store = MemoryStore::open_default().expect("target store");
        store
            .import_document(&exported)
            .expect("destination import");
        assert_eq!(store.export_document(42).expect("target export"), exported);
        let physical = store.paths.active_document_path(42);
        let stored = std::fs::read(&physical).expect("destination bytes");
        if protected {
            assert!(matches!(
                protection::inspect_bytes(&stored).expect("format"),
                protection::StoredFormat::Portable {
                    domain: Domain::Memories
                }
            ));
            assert!(!stored
                .windows(b"Source conversation".len())
                .any(|part| part == b"Source conversation"));
        } else {
            assert_eq!(
                serde_json::from_slice::<MemoryDocument>(&stored).expect("plain native document"),
                original
            );
        }
    }
}

fn encrypted_source() -> (Vec<u8>, Vec<u8>) {
    let fixture = Fixture::new();
    fixture.configure(true);
    let store = MemoryStore::open_default().expect("protected source");
    store
        .import_document(&bytes(&document(42, "source.md")))
        .expect("source memory");
    (
        std::fs::read(store.paths.active_document_path(42)).expect("protected record"),
        std::fs::read(&store.database).expect("protected index"),
    )
}

#[test]
fn cached_plain_store_refuses_live_copied_encrypted_record_without_index_leak() {
    let _guard = crate::test_support::env_lock().lock().expect("env lock");
    let (encrypted, _) = encrypted_source();
    let fixture = Fixture::new();
    fixture.configure(false);
    let store = MemoryStore::open_default().expect("plain target");
    store
        .import_document(&bytes(&document(43, "local.md")))
        .expect("local record");
    let before = std::fs::read(&store.database).expect("old plaintext index");
    let copied = store.paths.active_document_path(42);
    std::fs::write(&copied, &encrypted).expect("simulate copied source");
    assert_adoption(
        store
            .read_entry(43)
            .expect_err("block even existing plaintext read"),
    );
    assert_adoption(store.list_entries().expect_err("block enumeration"));
    assert_adoption(
        store
            .search("source", 0)
            .expect_err("block zero-limit search"),
    );
    assert_adoption(store.reindex().expect_err("block rebuild"));
    assert_adoption(
        store
            .replace_entry_if_unchanged(43, "Changed", blake3::hash(b"original"))
            .expect_err("block edit"),
    );
    assert_adoption(store.trash_entry(43).expect_err("block trash"));
    assert_adoption(
        store
            .write(Path::new("new.md"), "New", false)
            .expect_err("block write"),
    );
    assert_adoption(
        store
            .import_document(&bytes(&document(44, "incoming.md")))
            .expect_err("block import"),
    );
    assert_adoption(store.export_document(43).expect_err("block export"));
    assert_adoption(MemoryStore::open_default().expect_err("block reopen"));
    assert_eq!(
        std::fs::read(&store.database).expect("unchanged index"),
        before
    );
    assert_eq!(std::fs::read(&copied).expect("unchanged source"), encrypted);
    assert!(!store.paths.active_document_path(44).exists());
}

#[test]
fn copied_encrypted_index_blocks_fresh_rebuild_and_cached_store_mutations() {
    let _guard = crate::test_support::env_lock().lock().expect("env lock");
    let (_, encrypted_index) = encrypted_source();
    let fixture = Fixture::new();
    fixture.configure(false);
    let store = MemoryStore::open_default().expect("plain target");
    std::fs::write(&store.database, &encrypted_index).expect("copy encrypted cache only");
    assert_adoption(
        store
            .reindex()
            .expect_err("no replacement with plaintext index"),
    );
    assert_adoption(store.sync().expect_err("no plaintext derived cache"));
    assert_adoption(
        store
            .write(Path::new("local.md"), "Local", false)
            .expect_err("no write"),
    );
    assert!(super::super::index::MemoryIndex::open(&store.database).is_err());
    assert!(super::super::index::MemoryIndex::fresh(&store.database).is_err());
    assert_eq!(
        std::fs::read(&store.database).expect("unchanged encrypted index"),
        encrypted_index
    );
    for suffix in ["-wal", "-shm", "-journal"] {
        assert!(!PathBuf::from(format!("{}{suffix}", store.database.display())).exists());
    }
}

#[test]
fn copied_native_store_opens_only_after_authenticated_protected_adoption() {
    let _guard = crate::test_support::env_lock().lock().expect("env lock");
    let (encrypted, encrypted_index) = encrypted_source();
    let fixture = Fixture::new();
    fixture.configure(false);
    let store = MemoryStore::open_default().expect("plain target");
    let physical = store.paths.active_document_path(42);
    std::fs::write(&physical, &encrypted).expect("copy source record");
    std::fs::write(&store.database, encrypted_index).expect("copy source index");
    assert_adoption(store.read_entry(42).expect_err("require explicit adoption"));
    drop(store);
    assert!(
        protection::unlock_foreign_file(Domain::Memories, &encrypted, "wrong-password").is_err()
    );
    assert_adoption(MemoryStore::open_default().expect_err("wrong password does not adopt"));
    protection::unlock_foreign_file(Domain::Memories, &encrypted, "memory-test-password")
        .expect("authenticate copied source");
    assert_adoption(
        MemoryStore::open_default().expect_err("unlock alone does not adopt plaintext policy"),
    );
    let error = protection::configure(
        ProtectionPolicy {
            memories: true,
            ..ProtectionPolicy::default()
        },
        "new-target-password",
    )
    .expect_err("ordinary configuration cannot adopt copied native data");
    assert_eq!(
        error.downcast_ref::<NativeStoreAdoptionNeeded>(),
        Some(&NativeStoreAdoptionNeeded {
            domain: Domain::Memories,
        })
    );
    protection::adopt_native_store(Domain::Memories, "new-target-password")
        .expect("explicit protected adoption");
    let reopened = MemoryStore::open_default().expect("adopted store");
    let exported = reopened
        .export_document(42)
        .expect("adopted native document");
    assert_eq!(
        serde_json::from_slice::<MemoryDocument>(&exported).expect("native JSON"),
        document(42, "source.md")
    );
    reopened.reindex().expect("protected rebuild");
    for path in [&physical, &reopened.database] {
        let stored = std::fs::read(path).expect("protected destination");
        assert!(matches!(
            protection::inspect_bytes(&stored).expect("format"),
            protection::StoredFormat::Portable {
                domain: Domain::Memories
            }
        ));
        assert!(!stored.starts_with(b"SQLite format 3"));
    }
}

#[test]
fn validation_rejects_invalid_structure_but_keeps_previously_saved_user_content() {
    let mut source = document(42, "source.md");
    source.markdown = "Stored example: password=example\r\n".to_string();
    assert_eq!(
        MemoryStore::validate_import_document(&bytes(&source)).expect("valid source"),
        source
    );
    source.logical_path = "../escape.md".into();
    assert!(MemoryStore::validate_import_document(&bytes(&source)).is_err());
    source.logical_path = "source.md".into();
    source.metadata.as_mut().expect("metadata").machine = None;
    assert!(MemoryStore::validate_import_document(&bytes(&source)).is_err());
    source = document(0, "source.md");
    assert!(MemoryStore::validate_import_document(&bytes(&source)).is_err());
    source.id = 42;
    source.schema_version = 99;
    assert!(MemoryStore::validate_import_document(&bytes(&source)).is_err());
}

#[test]
fn index_failure_reports_saved_source_and_later_repair_keeps_native_metadata() {
    let _guard = crate::test_support::env_lock().lock().expect("env lock");
    let fixture = Fixture::new();
    fixture.configure(false);
    let store = MemoryStore::open_default().expect("store");
    let source = document(42, "source.md");
    store
        .import_document(&bytes(&source))
        .expect("initial source");
    let cached = std::fs::read(&store.database).expect("original cache");
    std::fs::write(&store.database, b"invalid plaintext SQLite snapshot")
        .expect("damage disposable cache");
    assert!(matches!(
        store.replace_entry_if_unchanged(
            42,
            "# Repaired\n\nNew content.\n",
            blake3::hash(source.markdown.as_bytes())
        ),
        Err(MemoryError::SavedButNotIndexed { .. })
    ));
    assert_eq!(
        store.read_entry(42).expect("source-first edited read"),
        "# Repaired\n\nNew content.\n"
    );
    let exported: MemoryDocument =
        serde_json::from_slice(&store.export_document(42).expect("source export"))
            .expect("native source");
    assert_eq!(exported.metadata, source.metadata);
    std::fs::write(&store.database, cached).expect("restore cache");
    store.reindex().expect("repair derived state");
    assert_eq!(store.status().expect("repaired status").files, 1);
}

#[cfg(unix)]
#[test]
fn copied_trash_symlink_is_rejected_before_chmod_or_source_mutation_even_after_foreign_unlock() {
    use std::os::unix::fs::{symlink, PermissionsExt};

    let _guard = crate::test_support::env_lock().lock().expect("env lock");
    let (encrypted, _) = encrypted_source();
    let fixture = Fixture::new();
    fixture.configure(false);
    let store = MemoryStore::open_default().expect("target store");
    store
        .import_document(&bytes(&document(43, "local.md")))
        .expect("local source");
    let source = store.paths.active_document_path(43);
    let source_before = std::fs::read(&source).expect("source snapshot");
    let index_before = std::fs::read(&store.database).expect("index snapshot");
    std::fs::write(store.paths.active_document_path(42), &encrypted)
        .expect("copied encrypted source");
    protection::unlock_foreign_file(Domain::Memories, &encrypted, "memory-test-password")
        .expect("authenticated same-root foreign cache");
    let outside = store.root_path().with_file_name("outside-trash");
    std::fs::create_dir(&outside).expect("outside target");
    std::fs::set_permissions(&outside, std::fs::Permissions::from_mode(0o755))
        .expect("outside mode");
    let sentinel = outside.join(source.file_name().expect("source name"));
    std::fs::write(&sentinel, b"outside sentinel must not be replaced").expect("outside sentinel");
    std::fs::remove_dir(store.root_path().join(".trash")).expect("empty native Trash");
    symlink(&outside, store.root_path().join(".trash")).expect("copied Trash symlink");
    std::fs::set_permissions(store.root_path(), std::fs::Permissions::from_mode(0o751))
        .expect("root sentinel mode");
    assert!(MemoryStore::open_default().is_err());
    assert!(store.forget(Path::new("local.md")).is_err());
    assert!(store.trash_entry(43).is_err());
    assert!(store.read_entry(43).is_err());
    assert_eq!(
        std::fs::read(&source).expect("unchanged source"),
        source_before
    );
    assert_eq!(
        std::fs::read(&store.database).expect("unchanged index"),
        index_before
    );
    assert_eq!(
        std::fs::read(&sentinel).expect("unchanged target"),
        b"outside sentinel must not be replaced"
    );
    assert_eq!(
        std::fs::read(store.paths.active_document_path(42)).expect("unchanged copied source"),
        encrypted
    );
    assert_eq!(
        std::fs::metadata(&outside)
            .expect("outside metadata")
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
    assert_eq!(
        std::fs::metadata(store.root_path())
            .expect("root metadata")
            .permissions()
            .mode()
            & 0o777,
        0o751
    );
}

#[cfg(unix)]
#[test]
fn cached_and_new_stores_reject_root_symlink_without_changing_external_or_retained_sources() {
    use std::os::unix::fs::{symlink, PermissionsExt};

    let _guard = crate::test_support::env_lock().lock().expect("env lock");
    let fixture = Fixture::new();
    fixture.configure(false);
    let store = MemoryStore::open_default().expect("store");
    store
        .import_document(&bytes(&document(42, "source.md")))
        .expect("source");
    let source_name = store
        .paths
        .active_document_path(42)
        .file_name()
        .expect("source name")
        .to_os_string();
    let source_before =
        std::fs::read(store.paths.active_document_path(42)).expect("source snapshot");
    let index_before = std::fs::read(&store.database).expect("index snapshot");
    let outside = store.root_path().with_file_name("outside-memories");
    std::fs::create_dir(&outside).expect("outside target");
    std::fs::set_permissions(&outside, std::fs::Permissions::from_mode(0o755))
        .expect("outside mode");
    let sentinel = outside.join(&source_name);
    std::fs::write(&sentinel, b"external root sentinel").expect("outside sentinel");
    let retained = store.root_path().with_file_name("retained-memories");
    std::fs::rename(store.root_path(), &retained).expect("retain real root");
    symlink(&outside, store.root_path()).expect("replace root with copied symlink");
    assert!(MemoryStore::open_default().is_err());
    assert!(store.forget(Path::new("source.md")).is_err());
    assert!(store.trash_entry(42).is_err());
    assert!(store
        .replace_entry_if_unchanged(42, "Changed", blake3::hash(b"original"))
        .is_err());
    assert_eq!(
        std::fs::read(retained.join(&source_name)).expect("retained source"),
        source_before
    );
    assert_eq!(
        std::fs::read(&sentinel).expect("external sentinel"),
        b"external root sentinel"
    );
    assert_eq!(
        std::fs::read(&store.database).expect("unchanged index"),
        index_before
    );
    assert_eq!(
        std::fs::metadata(&outside)
            .expect("outside metadata")
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
    assert!(!outside.join(".trash").exists());
}

#[test]
fn numeric_and_ordinary_trash_preserve_all_existing_collision_targets() {
    let _guard = crate::test_support::env_lock().lock().expect("env lock");
    let fixture = Fixture::new();
    fixture.configure(true);
    let store = MemoryStore::open_default().expect("store");
    for (id, logical) in [(42, "numeric.md"), (43, "ordinary.md")] {
        store
            .import_document(&bytes(&document(id, logical)))
            .expect("active source");
    }
    let trash = store.root_path().join(".trash");
    for id in [42, 43] {
        for suffix in ["", "-1"] {
            std::fs::write(
                trash.join(format!("{id:016x}{suffix}.tcui-memory")),
                format!("retained Trash entry {id}{suffix}"),
            )
            .expect("existing collision");
        }
    }
    store.trash_entry(42).expect("numeric Trash");
    let ordinary = store
        .forget(Path::new("ordinary.md"))
        .expect("ordinary Trash");
    assert_eq!(ordinary, trash.join("000000000000002b-2.tcui-memory"));
    for id in [42, 43] {
        for suffix in ["", "-1"] {
            assert_eq!(
                std::fs::read_to_string(trash.join(format!("{id:016x}{suffix}.tcui-memory")))
                    .expect("retained collision"),
                format!("retained Trash entry {id}{suffix}")
            );
        }
        let moved = trash.join(format!("{id:016x}-2.tcui-memory"));
        let native: MemoryDocument = serde_json::from_slice(
            &protection::read(Domain::Memories, &moved).expect("protected Trash body"),
        )
        .expect("native Trash document");
        assert_eq!(native.id, id);
        assert!(!store.paths.active_document_path(id).exists());
    }
    assert!(store.list_entries().expect("active list").is_empty());
}

#[cfg(unix)]
#[test]
fn unavailable_symlink_cache_reports_partial_mutations_without_touching_external_files() {
    use std::os::unix::fs::{symlink, PermissionsExt};

    let _guard = crate::test_support::env_lock().lock().expect("env lock");
    let fixture = Fixture::new();
    fixture.configure(false);
    let store = MemoryStore::open_default().expect("store");
    store
        .import_document(&bytes(&document(42, "source.md")))
        .expect("initial source");
    let outside = store.root_path().with_file_name("outside-cache.sqlite3");
    let sentinel = b"external cache sentinel";
    std::fs::write(&outside, sentinel).expect("outside cache");
    std::fs::set_permissions(&outside, std::fs::Permissions::from_mode(0o640))
        .expect("outside mode");
    std::fs::remove_file(&store.database).expect("remove disposable cache");
    symlink(&outside, &store.database).expect("unavailable no-follow cache");

    let error = store
        .remember_context(
            "# New note\n\nKeep this source.\n",
            MemoryMetadata::default(),
        )
        .expect_err("report cache failure after saving");
    let MemoryError::SavedButNotIndexed { path, .. } = error else {
        panic!("expected durable save, got {error}");
    };
    assert_eq!(
        store.read(Path::new(&path)).expect("read durable source"),
        "# New note\n\nKeep this source.\n"
    );
    assert!(matches!(
        store.forget(Path::new("source.md")),
        Err(MemoryError::RemovedButNotIndexed { .. })
    ));
    assert!(!store.paths.active_document_path(42).exists());
    assert!(store.paths.trash_document_path(42).is_file());
    assert_eq!(
        MemoryStore::open_default()
            .expect("cache does not prevent source access")
            .list_entries()
            .expect("remaining source")
            .len(),
        1
    );
    assert!(store.sync().is_err());
    assert!(store.reindex().is_err());
    assert!(std::fs::symlink_metadata(&store.database)
        .expect("retained cache symlink")
        .file_type()
        .is_symlink());
    assert_eq!(std::fs::read(&outside).expect("outside contents"), sentinel);
    assert_eq!(
        std::fs::metadata(&outside)
            .expect("outside metadata")
            .permissions()
            .mode()
            & 0o777,
        0o640
    );
    for path in [&outside, &store.database] {
        for suffix in ["-wal", "-shm", "-journal"] {
            assert!(!PathBuf::from(format!("{}{suffix}", path.display())).exists());
        }
    }
}

#[test]
fn unknown_or_truncated_encrypted_cache_blocks_mutations_without_changing_sources() {
    let _guard = crate::test_support::env_lock().lock().expect("env lock");
    let fixture = Fixture::new();
    fixture.configure(false);
    let store = MemoryStore::open_default().expect("store");
    store
        .import_document(&bytes(&document(42, "source.md")))
        .expect("initial source");
    let source = store.paths.active_document_path(42);
    let before = std::fs::read(&source).expect("source snapshot");

    for framing in [
        b"TCUI-PORTABLE\x7f".as_slice(),
        b"TCUI-PORTABLE\x02".as_slice(),
        b"TCUI-PROTECTED\x7f".as_slice(),
    ] {
        std::fs::write(&store.database, framing).expect("copied unknown encrypted cache");
        assert!(matches!(
            store.write(Path::new("new.md"), "New source", false),
            Err(MemoryError::Protection(_))
        ));
        assert!(matches!(
            store.forget(Path::new("source.md")),
            Err(MemoryError::Protection(_))
        ));
        assert!(store.reindex().is_err());
        assert!(store.read_entry(42).is_err());
        assert_eq!(std::fs::read(&source).expect("retained source"), before);
        assert_eq!(
            std::fs::read(&store.database).expect("retained encrypted framing"),
            framing
        );
        assert_eq!(
            std::fs::read_dir(store.root_path())
                .expect("native root")
                .map(|entry| entry.expect("native entry"))
                .filter(|entry| {
                    entry
                        .path()
                        .extension()
                        .is_some_and(|extension| extension == "tcui-memory")
                })
                .count(),
            1
        );
        assert_eq!(
            std::fs::read_dir(store.root_path().join(".trash"))
                .expect("Trash")
                .count(),
            0
        );
    }
}
