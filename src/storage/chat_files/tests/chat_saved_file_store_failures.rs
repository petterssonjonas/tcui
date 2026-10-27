use std::fs::OpenOptions;

use super::TestRoot;
use crate::storage::chat_files::{AtomicWriteTarget, ChatFileError};

#[test]
fn chat_saved_file_store_returns_busy_without_mutation_when_global_lock_is_contended() {
    // Given
    let root = TestRoot::new("lock-contention");
    let store = root.store();
    store
        .create_for_chat(151, "prior.md", b"prior")
        .expect("create prior state");
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(root.lock_path())
        .expect("open global lock");
    lock.try_lock().expect("hold global lock");

    // When
    let error = store
        .create_for_chat(152, "blocked.md", b"blocked")
        .expect_err("contended lock must fail");

    // Then
    assert!(matches!(error, ChatFileError::ChatFilesBusy));
    assert!(!root.saved_path("blocked.md").exists());
}

#[test]
fn chat_saved_file_store_saved_write_failure_preserves_prior_pair() {
    // Given
    let root = TestRoot::new("saved-write-failure");
    let store = root.store();
    let prior = store
        .create_for_chat(161, "prior.md", b"prior")
        .expect("create prior pair");
    store.fail_next_atomic_write(AtomicWriteTarget::SavedFile);

    // When
    let error = store
        .create_for_chat(161, "failed.md", b"failed")
        .expect_err("inject saved-file write failure");

    // Then
    assert!(matches!(error, ChatFileError::InjectedAtomicWriteFailure));
    assert_eq!(
        store.list_for_conversation(161).expect("list prior pair"),
        vec![prior]
    );
    assert!(!root.saved_path("failed.md").exists());
}

#[test]
fn chat_saved_file_store_manifest_failure_rolls_back_new_saved_file() {
    // Given
    let root = TestRoot::new("create-manifest-failure");
    let store = root.store();
    let prior = store
        .create_for_chat(171, "prior.md", b"prior")
        .expect("create prior pair");
    store.fail_next_atomic_write(AtomicWriteTarget::Manifest);

    // When
    let error = store
        .create_for_chat(171, "failed.md", b"failed")
        .expect_err("inject manifest failure");

    // Then
    assert!(matches!(error, ChatFileError::InjectedAtomicWriteFailure));
    assert_eq!(
        store.list_for_conversation(171).expect("list prior pair"),
        vec![prior]
    );
    assert!(!root.saved_path("failed.md").exists());
}

#[test]
fn chat_saved_file_store_delete_manifest_failure_restores_file_and_links() {
    // Given
    let root = TestRoot::new("delete-manifest-failure");
    let store = root.store();
    let link = store
        .create_for_chat(181, "restored.md", b"restored")
        .expect("create pair");
    store.fail_next_atomic_write(AtomicWriteTarget::Manifest);

    // When
    let error = store
        .delete_by_link(181, link.id)
        .expect_err("inject delete manifest failure");

    // Then
    assert!(matches!(error, ChatFileError::InjectedAtomicWriteFailure));
    assert_eq!(
        store
            .list_for_conversation(181)
            .expect("list restored link"),
        vec![link]
    );
    assert_eq!(root.read(&root.saved_path("restored.md")), b"restored");
}

#[test]
fn chat_saved_file_store_create_reports_explicit_rollback_failure() {
    // Given
    let root = TestRoot::new("rollback-failure");
    let store = root.store();
    store.fail_next_atomic_write(AtomicWriteTarget::Manifest);
    store.fail_next_rollback();

    // When
    let error = store
        .create_for_chat(191, "stranded.md", b"stranded")
        .expect_err("inject rollback failure");

    // Then
    assert!(matches!(
        error,
        ChatFileError::RollbackFailed {
            operation: "create",
            ..
        }
    ));
}

#[test]
fn chat_saved_file_store_tombstone_remove_failure_restores_file_and_links() {
    // Given
    let root = TestRoot::new("tombstone-remove-failure");
    let store = root.store();
    let link = store
        .create_for_chat(201, "restored.md", b"restored")
        .expect("create pair");
    store.fail_next_tombstone_remove();

    // When
    let error = store
        .delete_by_link(201, link.id)
        .expect_err("inject tombstone removal failure");

    // Then
    assert!(matches!(
        error,
        ChatFileError::InjectedTombstoneRemoveFailure
    ));
    assert_eq!(
        store
            .list_for_conversation(201)
            .expect("list restored link"),
        vec![link]
    );
    assert_eq!(root.read(&root.saved_path("restored.md")), b"restored");
}
