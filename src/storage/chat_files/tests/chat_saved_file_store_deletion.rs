use super::TestRoot;

#[test]
fn chat_saved_file_store_delete_by_link_removes_file_and_every_association() {
    // Given
    let root = TestRoot::new("delete-link");
    std::fs::create_dir_all(root.saved_root()).expect("create saved root");
    std::fs::write(root.saved_path("shared.md"), b"shared").expect("write shared file");
    root.write_manifest(
        br#"{"version":1,"links":[{"id":11,"conversation_id":91,"filename":"shared.md"},{"id":12,"conversation_id":92,"filename":"shared.md"}]}"#,
    );
    let store = root.store();

    // When
    store
        .delete_by_link(91, serde_json::from_str("11").expect("link ID"))
        .expect("delete by link");

    // Then
    assert!(!root.saved_path("shared.md").exists());
    assert!(store
        .list_for_conversation(91)
        .expect("list first chat")
        .is_empty());
    assert!(store
        .list_for_conversation(92)
        .expect("list second chat")
        .is_empty());
}

#[test]
fn chat_saved_file_store_delete_by_saved_path_removes_file_and_every_association() {
    // Given
    let root = TestRoot::new("delete-path");
    let store = root.store();
    let link = store
        .create_for_chat(101, "delete.md", b"delete")
        .expect("create linked file");

    // When
    store
        .delete_by_saved_path(&root.saved_path(&link.filename))
        .expect("delete by saved path");

    // Then
    assert!(!root.saved_path(&link.filename).exists());
    assert!(store
        .list_for_conversation(101)
        .expect("list links")
        .is_empty());
}

#[test]
fn chat_saved_file_store_remove_conversation_links_preserves_saved_bytes() {
    // Given
    let root = TestRoot::new("remove-conversation-links");
    let store = root.store();
    let link = store
        .create_for_chat(111, "preserved.md", b"preserved")
        .expect("create linked file");

    // When
    store
        .remove_conversation_links(111)
        .expect("remove conversation links");

    // Then
    assert!(store
        .list_for_conversation(111)
        .expect("list removed links")
        .is_empty());
    assert_eq!(root.read(&root.saved_path(&link.filename)), b"preserved");
    assert_eq!(store.list_saved().expect("list saved").len(), 1);
}

#[test]
fn chat_saved_file_store_delete_rejects_path_outside_saved_root() {
    // Given
    let root = TestRoot::new("outside-delete");
    let outside = root.path.join("outside.txt");
    std::fs::write(&outside, b"sentinel").expect("write sentinel");
    let store = root.store();

    // When
    let error = store
        .delete_by_saved_path(&outside)
        .expect_err("outside path must be rejected");

    // Then
    assert!(matches!(
        error,
        super::super::ChatFileError::PathOutsideSavedRoot
    ));
    assert_eq!(root.read(&outside), b"sentinel");
}

#[cfg(unix)]
#[test]
fn chat_saved_file_store_delete_rejects_symlink_without_touching_target() {
    use std::os::unix::fs::symlink;

    // Given
    let root = TestRoot::new("symlink-delete");
    let outside = root.path.join("outside.txt");
    std::fs::write(&outside, b"sentinel").expect("write sentinel");
    std::fs::create_dir_all(root.saved_root()).expect("create saved root");
    let link_path = root.saved_path("outside-link.txt");
    symlink(&outside, &link_path).expect("create symlink");

    // When
    let error = root
        .store()
        .delete_by_saved_path(&link_path)
        .expect_err("symlink must be rejected");

    // Then
    assert!(matches!(
        error,
        super::super::ChatFileError::NotRegularFile { .. }
    ));
    assert_eq!(root.read(&outside), b"sentinel");
    assert!(link_path.exists());
}
