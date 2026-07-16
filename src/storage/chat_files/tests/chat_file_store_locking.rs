use std::fs::OpenOptions;

use super::TestRoot;
use crate::storage::chat_files::ChatFileError;

#[test]
fn chat_file_store_returns_busy_when_conversation_lock_is_contended() {
    // Given
    let root = TestRoot::new("lock-contention");
    let paths = root.paths();
    let store = root.store();
    let record = store
        .create(71, "locked.md", b"locked")
        .expect("create payload");
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(paths.conversation_dir(71).join(".lock"))
        .expect("open conversation lock");
    lock.try_lock().expect("hold conversation lock");

    // When
    let error = store
        .delete(71, record.id)
        .expect_err("contended lock must fail");

    // Then
    assert!(matches!(error, ChatFileError::ChatFilesBusy));
}

#[test]
fn chat_file_store_allows_distinct_conversations_to_mutate_concurrently() {
    // Given
    let root = TestRoot::new("lock-isolation");
    let paths = root.paths();
    let store = root.store();
    store
        .create(72, "first.md", b"first")
        .expect("create first conversation");
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(paths.conversation_dir(72).join(".lock"))
        .expect("open first conversation lock");
    lock.try_lock().expect("hold first conversation lock");

    // When
    let second = store.create(73, "second.md", b"second");

    // Then
    assert!(
        second.is_ok(),
        "distinct conversation was blocked: {second:?}"
    );
}
