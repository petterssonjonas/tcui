use super::super::{LegacyResetError, LegacyResetOutcome};
use super::support::TestRoot;

#[test]
fn chat_saved_legacy_reset_marker_makes_second_startup_a_noop() {
    // Given
    let root = TestRoot::new("second-startup");
    let saved_root = root.create_dir("saved");
    root.write("saved/legacy.md", b"legacy");
    root.write("cache/tcui/chat-files/1/payload", b"legacy cache");
    let config = root.config(&saved_root);
    let reset = root.reset(&config, None);
    assert_eq!(
        reset.run().expect("first startup"),
        LegacyResetOutcome::Completed
    );
    let marker_before = std::fs::read(root.marker_path()).expect("read first marker");
    root.write("saved/after-marker.md", b"new saved data");
    root.write("cache/tcui/chat-files/2/after-marker", b"new cache data");

    // When
    let outcome = reset.run().expect("second startup");

    // Then
    assert_eq!(outcome, LegacyResetOutcome::AlreadyCompleted);
    assert_eq!(root.read("saved/after-marker.md"), b"new saved data");
    assert_eq!(
        root.read("cache/tcui/chat-files/2/after-marker"),
        b"new cache data"
    );
    assert_eq!(
        std::fs::read(root.marker_path()).expect("read second marker"),
        marker_before
    );
}

#[test]
fn chat_saved_legacy_reset_retries_after_interrupted_marker_write() {
    // Given
    let root = TestRoot::new("marker-retry");
    let saved_root = root.create_dir("saved");
    root.write("saved/first.md", b"first legacy saved");
    root.write("cache/tcui/chat-files/1/first", b"first legacy cache");
    let config = root.config(&saved_root);
    let reset = root.reset(&config, None);
    reset.fail_next_marker_write();

    // When
    let first_error = reset.run().expect_err("inject marker write interruption");

    // Then
    assert!(matches!(
        first_error,
        LegacyResetError::InjectedMarkerWriteFailure
    ));
    assert!(!root.marker_path().exists());
    assert!(!root.path("saved/first.md").exists());
    assert!(!root.path("cache/tcui/chat-files").exists());

    // Given
    root.write("saved/retry.md", b"retry legacy saved");
    root.write("cache/tcui/chat-files/2/retry", b"retry legacy cache");

    // When
    let retry_outcome = reset.run().expect("retry interrupted reset");

    // Then
    assert_eq!(retry_outcome, LegacyResetOutcome::Completed);
    assert!(!root.path("saved/retry.md").exists());
    assert!(!root.path("cache/tcui/chat-files").exists());
    assert!(root.marker_path().is_file());
}
