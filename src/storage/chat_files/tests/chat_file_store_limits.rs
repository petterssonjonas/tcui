use super::{StoreLimits, TestRoot};
use crate::storage::chat_files::ChatFileError;

#[test]
fn chat_file_store_uses_the_planned_production_quotas() {
    // Given
    let expected_per_file = 64 * 1024 * 1024;
    let expected_per_conversation = 256 * 1024 * 1024;

    // When
    let limits = StoreLimits::default();

    // Then
    assert_eq!(limits.per_file, expected_per_file);
    assert_eq!(limits.per_conversation, expected_per_conversation);
}

#[test]
fn chat_file_store_rejects_payload_above_per_file_quota() {
    // Given
    let root = TestRoot::new("file-quota");
    let store = root.store_with_limits(8, 16);

    // When
    let error = store
        .create(41, "oversized.md", b"123456789")
        .expect_err("payload must exceed per-file quota");

    // Then
    assert!(matches!(
        error,
        ChatFileError::FileQuotaExceeded { size: 9, limit: 8 }
    ));
}

#[test]
fn chat_file_store_rejects_payload_above_conversation_quota_without_eviction() {
    // Given
    let root = TestRoot::new("conversation-quota");
    let store = root.store_with_limits(8, 12);
    let existing = store
        .create(42, "existing.md", b"12345678")
        .expect("create existing payload");

    // When
    let error = store
        .create(42, "overflow.md", b"12345")
        .expect_err("aggregate quota must reject new payload");

    // Then
    assert!(matches!(
        error,
        ChatFileError::ConversationQuotaExceeded {
            size: 13,
            limit: 12
        }
    ));
    assert_eq!(store.list(42).expect("list prior payload"), vec![existing]);
}
