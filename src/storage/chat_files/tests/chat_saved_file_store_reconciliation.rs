use super::TestRoot;
use crate::storage::chat_files::{ChatFileError, ChatFileManifest};

#[test]
fn chat_saved_file_store_list_prunes_links_with_missing_targets() {
    // Given
    let root = TestRoot::new("stale-list");
    root.write_manifest(
        br#"{"version":1,"links":[{"id":21,"conversation_id":121,"filename":"missing.md"}]}"#,
    );
    let store = root.store();

    // When
    let links = store.list_for_conversation(121).expect("list stale links");

    // Then
    assert!(links.is_empty());
    let manifest: ChatFileManifest =
        serde_json::from_slice(&root.read(&root.manifest_path())).expect("parse pruned manifest");
    assert!(manifest.links.is_empty());
}

#[test]
fn chat_saved_file_store_open_prunes_missing_target_and_returns_not_found() {
    // Given
    let root = TestRoot::new("stale-open");
    root.write_manifest(
        br#"{"version":1,"links":[{"id":22,"conversation_id":122,"filename":"missing.md"}]}"#,
    );
    let store = root.store();
    let id = serde_json::from_str("22").expect("link ID");

    // When
    let error = store
        .open_link(122, id)
        .expect_err("missing target must be stale");

    // Then
    assert!(matches!(error, ChatFileError::NotFound { .. }));
    let manifest: ChatFileManifest =
        serde_json::from_slice(&root.read(&root.manifest_path())).expect("parse pruned manifest");
    assert!(manifest.links.is_empty());
}

#[test]
fn chat_saved_file_store_preserves_future_manifest_without_rewrite() {
    // Given
    let root = TestRoot::new("future-manifest");
    let future = br#"{"version":3,"links":[],"future_field":"preserve me"}"#;
    root.write_manifest(future);
    let store = root.store();

    // When
    let error = store
        .list_for_conversation(131)
        .expect_err("future manifest must fail closed");

    // Then
    assert!(matches!(
        error,
        ChatFileError::UnsupportedManifestVersion { version: 3 }
    ));
    assert_eq!(root.read(&root.manifest_path()), future);
}

#[test]
fn chat_saved_file_store_rejects_escaping_manifest_without_touching_sentinel() {
    // Given
    let root = TestRoot::new("escaping-manifest");
    let sentinel = root.path.join("outside.txt");
    std::fs::write(&sentinel, b"sentinel").expect("write sentinel");
    let escaping =
        br#"{"version":1,"links":[{"id":31,"conversation_id":141,"filename":"../outside.txt"}]}"#;
    root.write_manifest(escaping);

    // When
    let error = root
        .store()
        .list_for_conversation(141)
        .expect_err("escaping manifest must fail closed");

    // Then
    assert!(matches!(error, ChatFileError::CorruptManifest));
    assert_eq!(root.read(&sentinel), b"sentinel");
    assert_eq!(root.read(&root.manifest_path()), escaping);
}

#[test]
fn chat_saved_file_store_rejects_invalid_conversation_id_without_mutation() {
    // Given
    let root = TestRoot::new("invalid-conversation");
    let store = root.store();

    // When
    let error = store
        .create_for_chat(0, "invalid.md", b"invalid")
        .expect_err("zero conversation ID must fail");

    // Then
    assert!(matches!(error, ChatFileError::InvalidConversationId));
    assert!(!root.saved_root().exists());
    assert!(!root.manifest_path().exists());
}

#[test]
fn chat_saved_file_store_rejects_zero_link_id_manifest_without_rewrite() {
    // Given
    let root = TestRoot::new("zero-link-id");
    std::fs::create_dir_all(root.saved_root()).expect("create saved root");
    std::fs::write(root.saved_path("linked.md"), b"linked").expect("write linked file");
    let invalid =
        br#"{"version":1,"links":[{"id":0,"conversation_id":151,"filename":"linked.md"}]}"#;
    root.write_manifest(invalid);

    // When
    let error = root
        .store()
        .list_for_conversation(151)
        .expect_err("zero link ID must fail closed");

    // Then
    assert!(matches!(error, ChatFileError::CorruptManifest));
    assert_eq!(root.read(&root.manifest_path()), invalid);
}
