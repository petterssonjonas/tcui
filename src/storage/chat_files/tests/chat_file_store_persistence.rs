use super::TestRoot;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[test]
fn chat_file_store_reloads_records_and_payloads_after_restart() {
    // Given
    let root = TestRoot::new("restart");
    let first_store = root.store();
    let first = first_store
        .create(51, "First Report.md", b"first payload")
        .expect("create first payload");
    let second = first_store
        .create(51, "second.txt", b"second payload")
        .expect("create second payload");
    drop(first_store);

    // When
    let restarted_store = root.store();
    let records = restarted_store.list(51).expect("reload manifest");

    // Then
    assert_eq!(records, vec![first.clone(), second]);
    assert_eq!(
        restarted_store.open(51, first.id).expect("open payload"),
        b"first payload"
    );
}

#[test]
fn chat_file_store_delete_removes_only_the_selected_payload() {
    // Given
    let root = TestRoot::new("delete");
    let store = root.store();
    let deleted = store
        .create(52, "deleted.md", b"deleted")
        .expect("create deleted payload");
    let retained = store
        .create(52, "retained.md", b"retained")
        .expect("create retained payload");

    // When
    store.delete(52, deleted.id).expect("delete payload");

    // Then
    assert_eq!(store.list(52).expect("list payloads"), vec![retained]);
}

#[test]
fn chat_file_store_purge_removes_the_conversation_cache() {
    // Given
    let root = TestRoot::new("purge");
    let paths = root.paths();
    let store = root.store();
    store
        .create(53, "purged.md", b"purged")
        .expect("create payload");

    // When
    store.purge_conversation(53).expect("purge conversation");

    // Then
    assert!(!paths.conversation_dir(53).exists());
}

#[cfg(unix)]
#[test]
fn chat_file_store_uses_private_directory_and_file_permissions() {
    // Given
    let root = TestRoot::new("permissions");
    let paths = root.paths();
    let store = root.store();

    // When
    let record = store
        .create(54, "private.md", b"private")
        .expect("create private payload");

    // Then
    let conversation = paths.conversation_dir(54);
    let payload = conversation.join(record.payload_name());
    for directory in [&paths.root, &paths.chat_files_dir, &conversation] {
        let mode = std::fs::metadata(directory)
            .expect("read directory metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700, "private directory mode for {directory:?}");
    }
    for file in [
        payload,
        conversation.join("manifest.json"),
        conversation.join(".lock"),
    ] {
        let mode = std::fs::metadata(&file)
            .expect("read file metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "private file mode for {file:?}");
    }
}
