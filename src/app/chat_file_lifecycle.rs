use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::{Action, GeneratedFile, TuiApp};
use crate::config::AppConfig;
use crate::llm::LlmClient;
use crate::storage::Storage;
use crate::ui::artifact_sidebar::ArtifactHandle;

fn env_lock() -> &'static Mutex<()> {
    crate::test_support::env_lock()
}

fn unique_root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "tcui-chat-file-lifecycle-{label}-{}",
        rand::random::<u64>()
    ))
}

fn configure_environment(root: &Path) -> PathBuf {
    let data_home = root.join("data");
    let config_home = root.join("config");
    let cache_home = root.join("cache");
    std::fs::create_dir_all(&data_home).expect("create data home");
    std::fs::create_dir_all(&config_home).expect("create config home");
    std::fs::create_dir_all(&cache_home).expect("create cache home");
    std::env::set_var("XDG_DATA_HOME", data_home);
    std::env::set_var("XDG_CONFIG_HOME", config_home);
    std::env::set_var("XDG_CACHE_HOME", &cache_home);
    cache_home
}

fn app_with_saved_dir(saved_dir: &Path) -> TuiApp {
    let config = AppConfig {
        saved_files_dir: Some(saved_dir.display().to_string()),
        artifact_save_dir: None,
        ..AppConfig::default()
    };
    TuiApp::new(
        Storage::new().expect("create storage"),
        Arc::new(tokio::sync::RwLock::new(config)),
        Arc::new(LlmClient::new()),
        None,
    )
}

fn persist_report(app: &mut TuiApp, conversation_id: i64) -> ArtifactHandle {
    let generated = GeneratedFile::from_skill_response(
        7,
        conversation_id,
        "@save report.md",
        "# Report\n\nPersistent content.",
    )
    .expect("parse @save response");
    app.persist_generated_file(0, generated)
        .expect("persist generated file");
    app.ui.tabs[0].temporary_artifacts[0].handle.clone()
}

fn reset_environment(root: &Path) {
    std::env::remove_var("XDG_DATA_HOME");
    std::env::remove_var("XDG_CONFIG_HOME");
    std::env::remove_var("XDG_CACHE_HOME");
    std::fs::remove_dir_all(root).expect("cleanup lifecycle root");
}

#[test]
fn restart_persistence_when_save_response_is_reloaded() {
    // Given
    let _guard = env_lock().lock().expect("env lock poisoned");
    let root = unique_root("restart");
    configure_environment(&root);
    let saved_dir = root.join("saved");
    let mut app = app_with_saved_dir(&saved_dir);
    let conversation_id = app.ui.tabs[0].active_conversation;
    let handle = persist_report(&mut app, conversation_id);
    drop(app);

    // When
    let restarted = app_with_saved_dir(&saved_dir);

    // Then
    assert_eq!(restarted.ui.tabs[0].temporary_artifacts.len(), 1);
    assert_eq!(restarted.ui.tabs[0].temporary_artifacts[0].handle, handle);
    assert_eq!(
        restarted.ui.tabs[0].temporary_artifacts[0]
            .content
            .as_deref(),
        Some("# Report\n\nPersistent content.")
    );
    reset_environment(&root);
}

#[test]
fn saved_copy_survives_chat_file_deletion_and_uses_collision_suffix() {
    // Given
    let _guard = env_lock().lock().expect("env lock poisoned");
    let root = unique_root("independent-copy");
    configure_environment(&root);
    let saved_dir = root.join("saved");
    std::fs::create_dir_all(&saved_dir).expect("create saved directory");
    std::fs::write(saved_dir.join("report.md"), "existing").expect("seed collision");
    let mut app = app_with_saved_dir(&saved_dir);
    let conversation_id = app.ui.tabs[0].active_conversation;
    let handle = persist_report(&mut app, conversation_id);

    // When
    app.prepare_artifact_save(handle.clone());
    app.delete_artifact(handle);

    // Then
    assert_eq!(
        std::fs::read_to_string(saved_dir.join("report (2).md")).expect("read saved copy"),
        "# Report\n\nPersistent content."
    );
    assert!(app
        .chat_files
        .list(conversation_id)
        .expect("list chat files")
        .is_empty());
    reset_environment(&root);
}

#[test]
fn explicit_conversation_delete_purges_chat_files_after_storage_delete() {
    // Given
    let _guard = env_lock().lock().expect("env lock poisoned");
    let root = unique_root("explicit-delete");
    let cache_home = configure_environment(&root);
    let mut app = app_with_saved_dir(&root.join("saved"));
    let conversation_id = app.ui.tabs[0].active_conversation;
    persist_report(&mut app, conversation_id);

    // When
    app.delete_conversation_by_id(conversation_id)
        .expect("delete conversation");

    // Then
    assert!(!cache_home
        .join("tcui/chat-files")
        .join(conversation_id.to_string())
        .exists());
    assert!(app.storage.get_messages(conversation_id).is_err());
    reset_environment(&root);
}

#[test]
fn close_chat_purges_chat_files_after_storage_delete() {
    // Given
    let _guard = env_lock().lock().expect("env lock poisoned");
    let root = unique_root("close-delete");
    let cache_home = configure_environment(&root);
    let mut app = app_with_saved_dir(&root.join("saved"));
    let conversation_id = app.ui.tabs[0].active_conversation;
    persist_report(&mut app, conversation_id);

    // When
    tokio::runtime::Runtime::new()
        .expect("runtime")
        .block_on(app.dispatch(Action::CloseChat))
        .expect("close chat");

    // Then
    assert!(!cache_home
        .join("tcui/chat-files")
        .join(conversation_id.to_string())
        .exists());
    assert!(app.storage.get_messages(conversation_id).is_err());
    reset_environment(&root);
}

#[test]
fn purge_failure_keeps_conversation_deleted_until_link_cleanup_retries() {
    // Given
    let _guard = env_lock().lock().expect("env lock poisoned");
    let root = unique_root("purge-recovery");
    configure_environment(&root);
    let saved_dir = root.join("saved");
    let mut app = app_with_saved_dir(&saved_dir);
    let conversation_id = app.ui.tabs[0].active_conversation;
    persist_report(&mut app, conversation_id);
    app.chat_files.fail_next_purge();

    // When
    app.delete_conversation_by_id(conversation_id)
        .expect("conversation deletion stays committed");

    // Then
    assert!(app.storage.get_messages(conversation_id).is_err());
    assert_eq!(
        app.chat_files
            .list(conversation_id)
            .expect("list retained link")
            .len(),
        1
    );
    assert_eq!(
        app.chat_files.list_saved().expect("list saved files").len(),
        1
    );

    // When
    app.chat_files
        .purge_conversation(conversation_id)
        .expect("retry link cleanup");

    // Then
    assert!(app
        .chat_files
        .list(conversation_id)
        .expect("list cleaned links")
        .is_empty());
    assert_eq!(
        app.chat_files.list_saved().expect("list saved files").len(),
        1
    );
    reset_environment(&root);
}

#[test]
fn lock_contention_preserves_chat_file_until_delete_can_acquire_lock() {
    // Given
    let _guard = env_lock().lock().expect("env lock poisoned");
    let root = unique_root("lock-contention");
    configure_environment(&root);
    let mut app = app_with_saved_dir(&root.join("saved"));
    let conversation_id = app.ui.tabs[0].active_conversation;
    let handle = persist_report(&mut app, conversation_id);
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(
            root.join("data")
                .join("tcui")
                .join("chat-file-links")
                .join(".lock"),
        )
        .expect("open chat-file lock");
    lock.try_lock().expect("hold chat-file lock");

    // When
    app.delete_artifact(handle.clone());

    // Then
    assert_eq!(app.ui.tabs[0].temporary_artifacts.len(), 1);
    assert!(root
        .join("data")
        .join("tcui")
        .join("artifacts")
        .join("report.md")
        .exists());
    drop(lock);
    app.delete_artifact(handle);
    assert!(app
        .chat_files
        .list(conversation_id)
        .expect("list deleted file")
        .is_empty());
    reset_environment(&root);
}
