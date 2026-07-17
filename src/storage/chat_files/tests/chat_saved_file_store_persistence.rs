use super::TestRoot;

#[test]
fn chat_saved_file_store_create_appears_globally_and_only_in_linked_conversation() {
    // Given
    let root = TestRoot::new("create-scope");
    let store = root.store();

    // When
    let link = store
        .create_for_chat(41, "report.md", b"report bytes")
        .expect("create saved file and link");

    // Then
    assert_eq!(
        store.list_for_conversation(41).expect("list chat"),
        vec![link]
    );
    assert!(store
        .list_for_conversation(42)
        .expect("list other chat")
        .is_empty());
    assert_eq!(store.list_saved().expect("list saved").len(), 1);
}

#[test]
fn chat_saved_file_store_restart_preserves_link_and_bytes() {
    // Given
    let root = TestRoot::new("restart");
    let link = root
        .store()
        .create_for_chat(51, "restart.md", b"restart bytes")
        .expect("create saved file and link");

    // When
    let restarted = root.store();

    // Then
    assert_eq!(
        restarted.list_for_conversation(51).expect("reload links"),
        vec![link.clone()]
    );
    assert_eq!(
        restarted
            .open_link(51, link.id)
            .expect("open restarted link"),
        b"restart bytes"
    );
}

#[test]
fn chat_saved_file_store_create_uses_collision_suffix() {
    // Given
    let root = TestRoot::new("collision");
    let store = root.store();
    let first = store
        .create_for_chat(61, "report.md", b"first")
        .expect("create first file");

    // When
    let second = store
        .create_for_chat(61, "report.md", b"second")
        .expect("create colliding file");

    // Then
    assert_eq!(first.filename, "report.md");
    assert_eq!(second.filename, "report (2).md");
    assert_eq!(root.read(&root.saved_path("report.md")), b"first");
    assert_eq!(root.read(&root.saved_path("report (2).md")), b"second");
}

#[test]
fn chat_saved_file_store_import_copies_source_bytes_and_links_copy() {
    // Given
    let root = TestRoot::new("import");
    let source = root.path.join("Imported Report.md");
    std::fs::write(&source, b"imported bytes").expect("write source");
    let store = root.store();

    // When
    let link = store
        .import_for_chat(71, &source)
        .expect("import source file");

    // Then
    assert_eq!(root.read(&source), b"imported bytes");
    assert_eq!(
        store.open_link(71, link.id).expect("open imported link"),
        b"imported bytes"
    );
    assert_eq!(link.filename, "Imported-Report.md");
}

#[test]
fn chat_saved_file_store_lists_unlinked_saved_files_globally() {
    // Given
    let root = TestRoot::new("global-list");
    std::fs::create_dir_all(root.saved_root()).expect("create saved root");
    std::fs::write(root.saved_path("manual.txt"), b"manual").expect("write manual saved file");

    // When
    let saved = root.store().list_saved().expect("list saved files");

    // Then
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].filename, "manual.txt");
    assert_eq!(saved[0].path, root.saved_path("manual.txt"));
}

#[cfg(unix)]
#[test]
fn chat_saved_file_store_uses_private_metadata_and_file_permissions() {
    use std::os::unix::fs::PermissionsExt;

    // Given
    let root = TestRoot::new("permissions");
    let store = root.store();

    // When
    let link = store
        .create_for_chat(81, "private.md", b"private")
        .expect("create private file");

    // Then
    for path in [root.saved_root(), root.data_paths().chat_file_links_dir] {
        assert_eq!(
            std::fs::metadata(path)
                .expect("directory metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
    for path in [
        root.saved_path(&link.filename),
        root.manifest_path(),
        root.lock_path(),
    ] {
        assert_eq!(
            std::fs::metadata(path)
                .expect("file metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}
