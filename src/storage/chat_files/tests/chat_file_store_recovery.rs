use super::TestRoot;
use crate::storage::chat_files::{AtomicWriteTarget, ChatFileError, ChatFileManifestV1};

#[test]
fn chat_file_store_quarantines_corrupt_manifest_and_rebuilds_from_payloads() {
    // Given
    let root = TestRoot::new("corrupt-manifest");
    let paths = root.paths();
    let store = root.store();
    let record = store
        .create(61, "Recovered Report.md", b"recover me")
        .expect("create payload");
    let conversation = paths.conversation_dir(61);
    std::fs::write(conversation.join("manifest.json"), b"{broken").expect("corrupt manifest");

    // When
    let rebuilt = store.list(61).expect("rebuild corrupt manifest");

    // Then
    assert_eq!(rebuilt, vec![record]);
    let manifest: ChatFileManifestV1 = serde_json::from_slice(
        &std::fs::read(conversation.join("manifest.json")).expect("read rebuilt manifest"),
    )
    .expect("parse rebuilt manifest");
    assert_eq!(manifest.version, 1);
    assert!(std::fs::read_dir(conversation)
        .expect("read conversation directory")
        .filter_map(Result::ok)
        .any(|entry| entry
            .file_name()
            .to_string_lossy()
            .starts_with("manifest.corrupt-")));
}

#[test]
fn chat_file_store_preserves_future_manifest_without_rewrite() {
    // Given
    let root = TestRoot::new("future-manifest");
    let paths = root.paths();
    let store = root.store();
    store
        .create(62, "future.md", b"future")
        .expect("create payload");
    let manifest_path = paths.conversation_dir(62).join("manifest.json");
    let future = br#"{"version":2,"conversation_id":62,"files":[]}"#;
    std::fs::write(&manifest_path, future).expect("write future manifest");

    // When
    let error = store
        .list(62)
        .expect_err("future manifest must fail closed");

    // Then
    assert!(matches!(
        error,
        ChatFileError::UnsupportedManifestVersion { version: 2 }
    ));
    assert_eq!(
        std::fs::read(manifest_path).expect("read future manifest"),
        future
    );
}

#[test]
fn chat_file_store_explicit_rebuild_recovers_safe_payload_records() {
    // Given
    let root = TestRoot::new("explicit-rebuild");
    let paths = root.paths();
    let store = root.store();
    let record = store
        .create(66, "rebuild.md", b"rebuild")
        .expect("create payload");
    std::fs::remove_file(paths.conversation_dir(66).join("manifest.json"))
        .expect("remove manifest");

    // When
    let rebuilt = store.rebuild_manifest(66).expect("rebuild manifest");

    // Then
    assert_eq!(rebuilt, vec![record]);
}

#[test]
fn chat_file_store_manifest_write_failure_preserves_prior_payload_and_metadata() {
    // Given
    let root = TestRoot::new("manifest-failure");
    let store = root.store();
    let prior = store
        .create(63, "prior.md", b"prior")
        .expect("create prior payload");
    store.fail_next_atomic_write(AtomicWriteTarget::Manifest);

    // When
    let error = store
        .create(63, "failed.md", b"failed")
        .expect_err("inject manifest write failure");

    // Then
    assert!(matches!(error, ChatFileError::InjectedAtomicWriteFailure));
    assert_eq!(
        store.list(63).expect("list prior state"),
        vec![prior.clone()]
    );
    assert_eq!(
        store.open(63, prior.id).expect("open prior payload"),
        b"prior"
    );
}

#[test]
fn chat_file_store_payload_write_failure_preserves_prior_payload_and_metadata() {
    // Given
    let root = TestRoot::new("payload-failure");
    let store = root.store();
    let prior = store
        .create(64, "prior.md", b"prior")
        .expect("create prior payload");
    store.fail_next_atomic_write(AtomicWriteTarget::Payload);

    // When
    let error = store
        .create(64, "failed.md", b"failed")
        .expect_err("inject payload write failure");

    // Then
    assert!(matches!(error, ChatFileError::InjectedAtomicWriteFailure));
    assert_eq!(store.list(64).expect("list prior state"), vec![prior]);
}

#[test]
fn chat_file_store_delete_manifest_failure_restores_prior_payload_and_metadata() {
    // Given
    let root = TestRoot::new("delete-manifest-failure");
    let store = root.store();
    let prior = store
        .create(65, "prior.md", b"prior")
        .expect("create prior payload");
    store.fail_next_atomic_write(AtomicWriteTarget::Manifest);

    // When
    let error = store
        .delete(65, prior.id)
        .expect_err("inject delete manifest failure");

    // Then
    assert!(matches!(error, ChatFileError::InjectedAtomicWriteFailure));
    assert_eq!(
        store.list(65).expect("list restored state"),
        vec![prior.clone()]
    );
    assert_eq!(
        store.open(65, prior.id).expect("open restored payload"),
        b"prior"
    );
}
