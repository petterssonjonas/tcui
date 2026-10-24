use super::TestRoot;
use crate::storage::chat_files::model::{VaultItemState, MANIFEST_VERSION};
use crate::storage::chat_files::{AtomicWriteTarget, ChatFileError, ChatFileManifest};

#[test]
fn native_item_upgrade_indexes_existing_bodies_and_preserves_every_chat_link() {
    let root = TestRoot::new("native-upgrade");
    std::fs::create_dir_all(root.saved_root().join("media")).expect("create nested directory");
    std::fs::write(root.saved_path("shared.md"), b"original").expect("write linked body");
    std::fs::write(root.saved_path("media/photo.PNG"), b"\x89PNG\0bytes").expect("write media");
    root.write_manifest(br#"{"version":1,"links":[{"id":11,"conversation_id":91,"filename":"shared.md"},{"id":12,"conversation_id":92,"filename":"shared.md"}]}"#);
    let store = root.store();

    let items = store.list_items().expect("upgrade native index");
    assert_eq!(items.len(), 2);
    let shared = items
        .iter()
        .find(|item| item.filename == "shared.md")
        .expect("linked item");
    let media = items
        .iter()
        .find(|item| item.filename == "photo.PNG")
        .expect("media item");
    assert_eq!(media.kind, "png");
    assert_eq!(media.relative_path, std::path::Path::new("media/photo.PNG"));
    assert_eq!(
        store.read_item(media.id).expect("read native media"),
        b"\x89PNG\0bytes"
    );
    assert_eq!(shared.chat_links.len(), 2);
    assert_eq!(shared.chat_links[0].id.to_string(), "11");
    assert_eq!(shared.chat_links[1].id.to_string(), "12");
    let replaced = store
        .replace_item(shared.id, b"replacement")
        .expect("replace indexed body");
    assert_eq!(replaced.id, shared.id);
    assert_eq!(replaced.chat_links, shared.chat_links);
    for link in &shared.chat_links {
        assert_eq!(
            store
                .open_link(link.conversation_id, link.id)
                .expect("same linked body"),
            b"replacement"
        );
    }
    let restarted = root.store().list_items().expect("restart native index");
    assert!(restarted
        .iter()
        .any(|item| item.id == shared.id && item.chat_links == shared.chat_links));
    assert!(restarted.iter().any(|item| item.id == media.id));
    assert_eq!(
        std::fs::read_dir(root.saved_root())
            .expect("list native root")
            .count(),
        2
    );
    assert_eq!(
        std::fs::read_dir(root.saved_root().join("media"))
            .expect("list media")
            .count(),
        1
    );
    let manifest: ChatFileManifest =
        serde_json::from_slice(&root.read(&root.manifest_path())).expect("upgraded manifest");
    assert_eq!(manifest.version, MANIFEST_VERSION);
}

#[test]
fn native_creation_and_chat_cleanup_keep_one_stable_identity() {
    let root = TestRoot::new("native-create");
    let store = root.store();
    let first = store
        .create_item("first.txt", b"first", Some(41))
        .expect("create native item");
    let unlinked = store
        .create_saved("second.bin", b"\0second")
        .expect("save existing API");
    let link = store
        .create_for_chat(42, "third.md", b"third")
        .expect("save chat API");
    let items = store.list_items().expect("list shared index");
    assert_eq!(items.len(), 3);
    assert_eq!(first.id.get(), 1);
    assert_eq!(
        items
            .iter()
            .find(|item| item.path == unlinked.path)
            .expect("unlinked identity")
            .id
            .get(),
        2
    );
    assert_eq!(
        items
            .iter()
            .find(|item| item.chat_links.contains(&link))
            .expect("chat identity")
            .id
            .get(),
        3
    );
    store
        .remove_conversation_links(41)
        .expect("unlink conversation");
    let kept = store
        .list_items()
        .expect("list after unlink")
        .into_iter()
        .find(|item| item.id == first.id)
        .expect("preserved identity");
    assert!(kept.chat_links.is_empty());
    assert_eq!(
        store.read_item(first.id).expect("preserved bytes"),
        b"first"
    );
}

#[test]
fn native_editor_replace_rejects_an_intervening_writer_without_changing_body_or_links() {
    let root = TestRoot::new("native-editor-conflict");
    let store = root.store();
    let item = store
        .create_item("draft.md", b"source", Some(44))
        .expect("create linked item");
    let expected = blake3::hash(&store.read_item(item.id).expect("read editor snapshot"));
    let writer = root.store();
    writer
        .replace_item(item.id, b"winner")
        .expect("independent writer updates source");

    assert!(matches!(
        store.replace_item_if_unchanged(item.id, b"editor draft", expected),
        Err(ChatFileError::ItemConflict { id }) if id == item.id
    ));
    assert_eq!(
        store
            .read_item(item.id)
            .expect("retained concurrent content"),
        b"winner"
    );
    assert_eq!(
        store
            .list_for_conversation(44)
            .expect("retained chat links"),
        item.chat_links
    );

    let saved = store
        .replace_item_if_unchanged(item.id, b"editor draft", blake3::hash(b"winner"))
        .expect("save after resolving the source conflict");
    assert_eq!(saved.id, item.id);
    assert_eq!(saved.chat_links, item.chat_links);
    assert_eq!(
        store
            .open_link(44, item.chat_links[0].id)
            .expect("linked replacement"),
        b"editor draft"
    );
}

#[test]
fn native_trash_preserves_body_and_associations_without_reusing_identity() {
    let root = TestRoot::new("native-trash");
    let store = root.store();
    let item = store
        .create_item("saved.png", b"\x89PNG\0private", Some(51))
        .expect("create item");
    let link = item.chat_links[0].clone();
    store.trash_item(item.id).expect("move to Trash");

    assert!(!item.path.exists());
    assert!(store
        .list_items()
        .expect("list active native files")
        .is_empty());
    assert!(store
        .list_for_conversation(51)
        .expect("list chat files")
        .is_empty());
    assert!(matches!(
        store.read_item(item.id),
        Err(ChatFileError::ItemNotFound { .. })
    ));
    assert!(matches!(
        store.open_link(51, link.id),
        Err(ChatFileError::NotFound { .. })
    ));
    let manifest = store.load_manifest().expect("load Trash metadata");
    let archived = manifest
        .items
        .iter()
        .find(|entry| entry.id == item.id)
        .expect("archived item");
    let VaultItemState::Trashed { trash_path } = &archived.state else {
        panic!("expected completed Trash state");
    };
    assert_eq!(
        root.read(&root.saved_root().join(trash_path)),
        b"\x89PNG\0private"
    );
    assert_eq!(archived.archived_links, vec![link]);
    let replacement = store
        .create_item("saved.png", b"new body", None)
        .expect("recreate name");
    assert!(replacement.id > item.id);
    assert!(replacement.chat_links.is_empty());
}

#[test]
fn native_replace_and_trash_publication_failures_leave_existing_body_and_links() {
    let root = TestRoot::new("native-failures");
    let store = root.store();
    let item = store
        .create_item("prior.md", b"prior", Some(61))
        .expect("create prior item");
    store.fail_next_atomic_write(AtomicWriteTarget::SavedFile);
    assert!(matches!(
        store.replace_item(item.id, b"changed"),
        Err(ChatFileError::InjectedAtomicWriteFailure)
    ));
    assert_eq!(
        store
            .read_item(item.id)
            .expect("retained replacement bytes"),
        b"prior"
    );
    store.fail_next_atomic_write(AtomicWriteTarget::Manifest);
    assert!(matches!(
        store.trash_item(item.id),
        Err(ChatFileError::InjectedAtomicWriteFailure)
    ));
    assert_eq!(
        store.read_item(item.id).expect("retained Trash bytes"),
        b"prior"
    );
    assert_eq!(
        store.list_for_conversation(61).expect("retained links"),
        item.chat_links
    );
    assert_eq!(
        store.list_items().expect("retained identity")[0].id,
        item.id
    );
}

#[test]
fn native_trash_recovery_finishes_intent_before_and_after_body_move() {
    for moved in [false, true] {
        let root = TestRoot::new(if moved {
            "native-recover-moved"
        } else {
            "native-recover-intent"
        });
        let store = root.store();
        let item = store
            .create_item("recover.md", b"recover bytes", Some(71))
            .expect("create item");
        let trash_path =
            std::path::PathBuf::from(format!(".Trash/{}-{:032x}.bin", item.id, 123_u128));
        std::fs::create_dir(root.saved_root().join(".Trash")).expect("create Trash");
        let mut manifest = store.load_manifest().expect("load index");
        manifest
            .items
            .iter_mut()
            .find(|entry| entry.id == item.id)
            .expect("item entry")
            .state = VaultItemState::Trashing {
            trash_path: trash_path.clone(),
        };
        store
            .write_manifest(&manifest)
            .expect("persist recovery intent");
        if moved {
            std::fs::rename(&item.path, root.saved_root().join(&trash_path))
                .expect("simulate body publication");
        }
        let restarted = root.store();
        assert!(restarted.list_items().expect("recover Trash").is_empty());
        assert!(!item.path.exists());
        assert_eq!(
            root.read(&root.saved_root().join(&trash_path)),
            b"recover bytes"
        );
        assert!(restarted
            .list_for_conversation(71)
            .expect("recover active links")
            .is_empty());
        let recovered = restarted.load_manifest().expect("recovered metadata");
        let entry = recovered
            .items
            .iter()
            .find(|entry| entry.id == item.id)
            .expect("retired ID");
        assert!(matches!(entry.state, VaultItemState::Trashed { .. }));
        assert_eq!(entry.archived_links, item.chat_links);
    }
}

#[test]
fn native_missing_and_permanently_deleted_ids_do_not_rebind_to_recreated_paths() {
    let root = TestRoot::new("native-retired");
    let store = root.store();
    let item = store
        .create_item("retired.md", b"old", None)
        .expect("create item");
    store
        .delete_by_saved_path(&item.path)
        .expect("existing permanent deletion");
    std::fs::write(&item.path, b"new").expect("recreate original path");
    let replacement = store
        .list_items()
        .expect("index replacement")
        .pop()
        .expect("replacement identity");
    assert!(replacement.id > item.id);
    assert!(matches!(
        store.read_item(item.id),
        Err(ChatFileError::ItemNotFound { .. })
    ));
    std::fs::remove_file(&replacement.path).expect("remove outside application");
    assert!(store.list_items().expect("retire missing item").is_empty());
    std::fs::write(&replacement.path, b"third").expect("recreate missing path");
    assert!(store.list_items().expect("index third body")[0].id > replacement.id);
}

#[cfg(unix)]
#[test]
fn native_items_do_not_follow_replaced_parent_symlinks() {
    use std::os::unix::fs::symlink;
    let root = TestRoot::new("native-symlink-parent");
    let nested = root.saved_root().join("nested");
    let external = root.path.join("external");
    std::fs::create_dir_all(&nested).expect("create nested root");
    std::fs::create_dir(&external).expect("create external root");
    std::fs::write(nested.join("same.md"), b"native").expect("create native file");
    std::fs::write(external.join("same.md"), b"external sentinel").expect("create sentinel");
    let store = root.store();
    let id = store.list_items().expect("index nested file")[0].id;
    std::fs::rename(&nested, root.saved_root().join("retained")).expect("move original directory");
    symlink(&external, &nested).expect("replace parent with symlink");
    assert!(store.replace_item(id, b"must not escape").is_err());
    assert!(store.trash_item(id).is_err());
    assert!(store.read_item(id).is_err());
    assert_eq!(root.read(&external.join("same.md")), b"external sentinel");
}

#[test]
fn native_manifest_rejects_escaping_item_paths_without_rewrite() {
    let root = TestRoot::new("native-invalid-index");
    let invalid = br#"{"version":2,"links":[],"next_item_id":2,"items":[{"id":1,"relative_path":"../outside.md","state":{"status":"active"},"archived_links":[]}]}"#;
    root.write_manifest(invalid);
    assert!(matches!(
        root.store().list_items(),
        Err(ChatFileError::CorruptManifest)
    ));
    assert_eq!(root.read(&root.manifest_path()), invalid);
}

#[test]
fn native_move_snapshot_preserves_source_identity_and_restores_overwritten_destination_on_rollback()
{
    let root = TestRoot::new("native-move-overwrite");
    let store = root.store();
    let source = store
        .create_item("source.md", b"source", Some(81))
        .expect("create source");
    let destination = store
        .create_item("destination.md", b"destination", Some(82))
        .expect("create destination");
    let guard = store
        .acquire_mutation_lock()
        .expect("hold native mutation lock");
    let snapshot = store
        .prepare_native_moves(&[(
            source.relative_path.clone(),
            destination.relative_path.clone(),
        )])
        .expect("prepare identity transaction");
    let journal_bytes = serde_json::to_vec(&snapshot).expect("serialize journal snapshot");
    let snapshot = serde_json::from_slice(&journal_bytes).expect("reload journal snapshot");
    std::fs::rename(&source.path, &destination.path).expect("simulate disk overwrite");
    store
        .commit_native_moves(&snapshot)
        .expect("commit identity transaction");
    store
        .commit_native_moves(&snapshot)
        .expect("repeat completed identity transaction");
    drop(guard);
    assert_eq!(
        store.read_item(source.id).expect("source ID follows body"),
        b"source"
    );
    assert!(matches!(
        store.read_item(destination.id),
        Err(ChatFileError::ItemNotFound { .. })
    ));
    assert_eq!(
        store
            .open_link(81, source.chat_links[0].id)
            .expect("source link follows body"),
        b"source"
    );
    assert!(store
        .list_for_conversation(82)
        .expect("overwritten destination links")
        .is_empty());

    let guard = store.acquire_mutation_lock().expect("hold rollback lock");
    std::fs::rename(&destination.path, &source.path).expect("simulate source rollback");
    std::fs::write(&destination.path, b"destination").expect("simulate destination backup restore");
    store
        .rollback_native_moves(&snapshot)
        .expect("restore identity snapshot");
    store
        .rollback_native_moves(&snapshot)
        .expect("repeat restored identity snapshot");
    drop(guard);
    assert_eq!(
        store.read_item(source.id).expect("restored source ID"),
        b"source"
    );
    assert_eq!(
        store
            .read_item(destination.id)
            .expect("restored destination ID"),
        b"destination"
    );
    assert_eq!(
        store
            .list_for_conversation(81)
            .expect("restored source links"),
        source.chat_links
    );
    assert_eq!(
        store
            .list_for_conversation(82)
            .expect("restored destination links"),
        destination.chat_links
    );
}

#[test]
fn native_nested_move_preserves_chat_link_id_and_supports_existing_deletion() {
    let root = TestRoot::new("native-nested-move");
    let store = root.store();
    let item = store
        .create_item("source.md", b"nested content", Some(91))
        .expect("create linked item");
    std::fs::create_dir(root.saved_root().join("archive")).expect("create target directory");
    let target = std::path::PathBuf::from("archive/renamed.md");
    let guard = store.acquire_mutation_lock().expect("hold move lock");
    let snapshot = store
        .prepare_native_moves(&[(item.relative_path.clone(), target.clone())])
        .expect("prepare nested move");
    std::fs::rename(&item.path, root.saved_root().join(&target))
        .expect("simulate nested disk move");
    store
        .commit_native_moves(&snapshot)
        .expect("commit nested metadata");
    drop(guard);
    let moved = store
        .list_items()
        .expect("reload nested identity")
        .pop()
        .expect("moved item");
    assert_eq!(moved.id, item.id);
    assert_eq!(moved.relative_path, target);
    assert_eq!(moved.chat_links[0].id, item.chat_links[0].id);
    assert_eq!(
        store
            .open_link(91, item.chat_links[0].id)
            .expect("read moved chat link"),
        b"nested content"
    );
    store
        .delete_by_link(91, item.chat_links[0].id)
        .expect("delete moved link");
    assert!(!moved.path.exists());
    assert!(store
        .list_items()
        .expect("list after nested deletion")
        .is_empty());
}

#[test]
fn native_trash_retries_failed_final_metadata_publication_without_losing_body_or_links() {
    let root = TestRoot::new("native-trash-final-publication");
    let store = root.store();
    let item = store
        .create_item("retry.md", b"retained in Trash", Some(101))
        .expect("create linked item");
    std::fs::create_dir(root.saved_root().join(".Trash")).expect("create Trash");
    let trash_path = std::path::PathBuf::from(format!(".Trash/{}-{:032x}.bin", item.id, 456_u128));
    let mut intent = store.load_manifest().expect("load native manifest");
    intent
        .items
        .iter_mut()
        .find(|entry| entry.id == item.id)
        .expect("item")
        .state = VaultItemState::Trashing {
        trash_path: trash_path.clone(),
    };
    store
        .write_manifest(&intent)
        .expect("persist authorized intent");
    store.fail_next_atomic_write(AtomicWriteTarget::Manifest);
    assert!(matches!(
        store.list_items(),
        Err(ChatFileError::InjectedAtomicWriteFailure)
    ));
    assert!(!item.path.exists());
    assert_eq!(
        root.read(&root.saved_root().join(&trash_path)),
        b"retained in Trash"
    );
    let restarted = root.store();
    assert!(restarted
        .list_items()
        .expect("retry final publication")
        .is_empty());
    let committed = restarted.load_manifest().expect("committed Trash manifest");
    let archived = committed
        .items
        .iter()
        .find(|entry| entry.id == item.id)
        .expect("retained identity");
    assert!(matches!(archived.state, VaultItemState::Trashed { .. }));
    assert_eq!(archived.archived_links, item.chat_links);
}

#[test]
fn native_folder_creation_preserves_relative_paths_and_rolls_back_failed_publication() {
    let root = TestRoot::new("native-folder-create");
    let store = root.store();
    let relative = std::path::Path::new("Imported Folder/note with spaces.md");
    let first = store
        .create_item_at(relative, b"first", Some(111))
        .expect("create nested item");
    let second = store
        .create_item_at(relative, b"second", None)
        .expect("create nested collision");
    assert_eq!(first.relative_path, relative);
    assert_eq!(first.filename, "note with spaces.md");
    assert_eq!(
        second.relative_path,
        std::path::Path::new("Imported Folder/note with spaces (2).md")
    );
    assert_eq!(
        store
            .open_link(111, first.chat_links[0].id)
            .expect("nested chat body"),
        b"first"
    );
    store.fail_next_atomic_write(AtomicWriteTarget::Manifest);
    assert!(matches!(
        store.create_item_at(relative, b"failed third", None),
        Err(ChatFileError::InjectedAtomicWriteFailure)
    ));
    assert!(!root
        .saved_path("Imported Folder/note with spaces (3).md")
        .exists());
    let items = root.store().list_items().expect("restart nested index");
    assert_eq!(items.len(), 2);
    assert!(items
        .iter()
        .any(|item| item.id == first.id && item.relative_path == relative));
    assert!(store
        .create_item_at(std::path::Path::new("../escape.md"), b"escape", None)
        .is_err());
    assert!(!root.path.join("escape.md").exists());
}

#[test]
fn native_import_preserves_only_safe_source_ids_and_never_rebinds_retired_ids() {
    let root = TestRoot::new("native-import-ids");
    let store = root.store();
    let preferred = "4096".parse().expect("source item ID");
    let imported = store
        .import_item_at(
            std::path::Path::new("imported.md"),
            b"import",
            Some(preferred),
        )
        .expect("preserve available source ID");
    assert_eq!(imported.id, preferred);
    assert!(imported.chat_links.is_empty());
    let next = store
        .create_item("local.md", b"local", None)
        .expect("new local item");
    assert_eq!(next.id.get(), 4097);
    store.trash_item(imported.id).expect("retire imported ID");
    let repeated = store
        .import_item_at(
            std::path::Path::new("imported.md"),
            b"new import",
            Some(preferred),
        )
        .expect("allocate after retired source ID");
    assert_eq!(repeated.id.get(), 4098);
    assert!(matches!(
        store.read_item(preferred),
        Err(ChatFileError::ItemNotFound { .. })
    ));
}

fn portable_framing() -> Vec<u8> {
    // Adoption probes validate framing, not authentication. Crypto tests cover authenticated envelopes.
    let mut bytes = b"TCUI-PORTABLE\x02".to_vec();
    bytes.push(1);
    for parameter in [65536_u32, 3, 1] {
        bytes.extend_from_slice(&parameter.to_le_bytes());
    }
    bytes.resize(crate::protection::NATIVE_STORE_ADOPTION_PROBE_LEN, 0);
    bytes
}

#[test]
fn native_store_requires_adoption_before_indexes_or_mutations_for_copied_portable_framing() {
    for location in ["body", "trash", "backup", "journal", "orphan", "manifest"] {
        let root = TestRoot::new(location);
        let store = root.store();
        let prior = store
            .create_item("prior.md", b"prior", Some(121))
            .expect("create prior item");
        let path = match location {
            "body" => root.saved_path("foreign.bin"),
            "trash" => root.saved_path(".Trash/foreign.bin"),
            "backup" => root
                .data_paths()
                .root
                .join("vault-backups/copied/original.bin"),
            "journal" => root
                .data_paths()
                .root
                .join("vault-native-ops/copied-metadata.bin"),
            "orphan" => root
                .data_paths()
                .chat_file_links_dir
                .join(".orphan-conversation-122"),
            "manifest" => root.manifest_path(),
            _ => unreachable!(),
        };
        std::fs::create_dir_all(path.parent().expect("parent"))
            .expect("create copied data directory");
        let portable = portable_framing();
        std::fs::write(&path, &portable).expect("copy portable framing");
        let manifest_before = root.read(&root.manifest_path());
        assert!(
            matches!(
                store.list_items(),
                Err(ChatFileError::NativeStoreAdoptionNeeded(_))
            ),
            "{location}"
        );
        assert!(
            matches!(
                store.read_item(prior.id),
                Err(ChatFileError::NativeStoreAdoptionNeeded(_))
            ),
            "{location}"
        );
        assert!(
            matches!(
                store.read_saved_path(&prior.path),
                Err(ChatFileError::NativeStoreAdoptionNeeded(_))
            ),
            "{location}"
        );
        assert!(
            matches!(
                store.create_item("blocked.md", b"blocked", None),
                Err(ChatFileError::NativeStoreAdoptionNeeded(_))
            ),
            "{location}"
        );
        assert!(
            matches!(
                store.replace_item(prior.id, b"changed"),
                Err(ChatFileError::NativeStoreAdoptionNeeded(_))
            ),
            "{location}"
        );
        assert!(
            matches!(
                store.trash_item(prior.id),
                Err(ChatFileError::NativeStoreAdoptionNeeded(_))
            ),
            "{location}"
        );
        assert_eq!(root.read(&path), portable);
        assert_eq!(root.read(&root.manifest_path()), manifest_before);
        assert_eq!(root.read(&prior.path), b"prior");
        assert!(!root.saved_path("blocked.md").exists());
    }
}

#[test]
fn native_adoption_probe_preserves_literal_v1_content_and_does_not_index_owned_stages() {
    let root = TestRoot::new("native-opaque-legacy");
    let store = root.store();
    std::fs::create_dir_all(root.saved_root()).expect("create native root");
    let literal = b"TCUI-PROTECTED\x01literal user content";
    std::fs::write(root.saved_path("literal.bin"), literal).expect("write literal prefix");
    let stage = root.saved_path(".tcui-move-example-000.stage");
    let temporary = root.saved_path(".chat-file-example.tmp");
    std::fs::write(&stage, b"TCUI-PORTABLE\x02").expect("incomplete owned stage");
    std::fs::write(&temporary, b"TCUI-PORTABLE\x02").expect("incomplete atomic temporary");
    let items = store.list_items().expect("opaque V1 remains manageable");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].filename, "literal.bin");
    assert_eq!(
        store.read_item(items[0].id).expect("read literal content"),
        literal
    );
    assert_eq!(root.read(&stage), b"TCUI-PORTABLE\x02");
    assert_eq!(root.read(&temporary), b"TCUI-PORTABLE\x02");
}
