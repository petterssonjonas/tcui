use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use super::TuiApp;
use crate::config::AppConfig;
use crate::llm::LlmClient;
use crate::storage::Storage;

fn env_lock() -> &'static Mutex<()> {
    crate::test_support::env_lock()
}

struct TestEnvironment {
    root: PathBuf,
    previous: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

impl TestEnvironment {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "tcui-chat-saved-reset-app-{label}-{}",
            rand::random::<u64>()
        ));
        let previous = [
            "XDG_DATA_HOME",
            "XDG_CONFIG_HOME",
            "XDG_CACHE_HOME",
            "HOME",
            "TCUI_TEST_SAVED_FILE_MIGRATION",
        ]
        .into_iter()
        .map(|name| (name, std::env::var_os(name)))
        .collect();
        for directory in ["data", "config", "cache", "home"] {
            std::fs::create_dir_all(root.join(directory)).expect("create isolated directory");
        }
        std::env::set_var("XDG_DATA_HOME", root.join("data"));
        std::env::set_var("XDG_CONFIG_HOME", root.join("config"));
        std::env::set_var("XDG_CACHE_HOME", root.join("cache"));
        std::env::set_var("HOME", root.join("home"));
        std::env::set_var("TCUI_TEST_SAVED_FILE_MIGRATION", "1");
        Self { root, previous }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    fn write(&self, relative: &str, contents: &[u8]) {
        let path = self.path(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create file parent");
        }
        std::fs::write(path, contents).expect("write isolated file");
    }
}

impl Drop for TestEnvironment {
    fn drop(&mut self) {
        for (name, value) in self.previous.drain(..) {
            match value {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn start_app(config: AppConfig) -> TuiApp {
    TuiApp::new(
        Storage::new().expect("open isolated storage"),
        Arc::new(tokio::sync::RwLock::new(config)),
        Arc::new(LlmClient::new()),
    )
}

#[test]
fn saved_file_migration_preserves_existing_files_before_the_saved_store_opens() {
    // Given
    let _guard = env_lock().lock().expect("environment lock poisoned");
    let environment = TestEnvironment::new("startup");
    let saved_root = environment.path("saved");
    let vault_root = environment.path("vault-profile");
    std::fs::create_dir_all(&saved_root).expect("create Saved root");
    std::fs::create_dir_all(&vault_root).expect("create Vault root");
    environment.write("saved/legacy.md", b"legacy saved");
    environment.write("cache/tcui/chat-files/1/payload", b"legacy cache");
    environment.write("vault-profile/sentinel.md", b"vault sentinel");
    let config = AppConfig {
        saved_files_dir: Some(saved_root.display().to_string()),
        ..AppConfig::default()
    };

    // When
    drop(start_app(config.clone()));

    // Then
    assert_eq!(
        std::fs::read(environment.path("saved/legacy.md")).expect("source retained"),
        b"legacy saved"
    );
    assert_eq!(
        std::fs::read(environment.path("data/tcui/vault/legacy.md")).expect("internal copy"),
        b"legacy saved"
    );
    assert!(environment.path("cache/tcui/chat-files/1/payload").exists());
    assert_eq!(
        std::fs::read(environment.path("vault-profile/sentinel.md")).expect("read Vault sentinel"),
        b"vault sentinel"
    );
    assert!(environment
        .path("data/tcui/migrations/saved-internal-vault-v1.done")
        .is_file());

    // Given
    environment.write("saved/after-marker.md", b"new saved file");

    // When
    drop(start_app(config));

    // Then
    assert_eq!(
        std::fs::read(environment.path("saved/after-marker.md"))
            .expect("read post-marker Saved file"),
        b"new saved file"
    );
}

#[test]
fn saved_file_migration_refuses_home_root_without_mutating_source_or_cache() {
    // Given
    let _guard = env_lock().lock().expect("environment lock poisoned");
    let environment = TestEnvironment::new("unsafe-warning");
    environment.write("home/legacy.md", b"home sentinel");
    environment.write("cache/tcui/chat-files/1/payload", b"cache sentinel");
    drop(Storage::new().expect("prime isolated storage"));
    let config = AppConfig {
        saved_files_dir: Some(environment.path("home").display().to_string()),
        ..AppConfig::default()
    };

    // When
    drop(start_app(config));

    // Then
    assert_eq!(
        std::fs::read(environment.path("home/legacy.md")).expect("read home sentinel"),
        b"home sentinel"
    );
    assert_eq!(
        std::fs::read(environment.path("cache/tcui/chat-files/1/payload"))
            .expect("read cache sentinel"),
        b"cache sentinel"
    );
    assert!(!environment
        .path("data/tcui/migrations/saved-internal-vault-v1.done")
        .exists());
}

#[test]
fn saved_file_migration_conflict_preserves_both_files_and_retries_without_clobbering() {
    let _guard = env_lock().lock().expect("environment lock poisoned");
    let environment = TestEnvironment::new("conflict-retry");
    environment.write("saved/report.md", b"legacy report");
    environment.write("data/tcui/vault/report.md", b"native report");
    let config = AppConfig {
        saved_files_dir: Some(environment.path("saved").display().to_string()),
        ..AppConfig::default()
    };

    let result = crate::storage::saved_file_migration::migrate_saved_files(&config);

    assert!(matches!(
        result,
        Err(crate::storage::saved_file_migration::MigrationError::Conflict { .. })
    ));
    assert_eq!(
        std::fs::read(environment.path("saved/report.md")).expect("read retained source"),
        b"legacy report"
    );
    assert_eq!(
        std::fs::read(environment.path("data/tcui/vault/report.md"))
            .expect("read retained native file"),
        b"native report"
    );
    let paths = crate::storage::paths::TcuiDataPaths::discover();
    assert!(crate::storage::saved_file_migration::require_complete(&paths).is_err());

    // Simulate the user resolving the conflict without removing either report.
    std::fs::rename(
        environment.path("data/tcui/vault/report.md"),
        environment.path("data/tcui/vault/native-report.md"),
    )
    .expect("retain native report under another name");
    crate::storage::saved_file_migration::migrate_saved_files(&config).expect("resume migration");

    assert!(
        crate::storage::saved_file_migration::require_complete(&paths).expect("read completion")
    );
    assert_eq!(
        std::fs::read(environment.path("data/tcui/vault/report.md")).expect("read migrated report"),
        b"legacy report"
    );
    assert_eq!(
        std::fs::read(environment.path("data/tcui/vault/native-report.md"))
            .expect("read original native report"),
        b"native report"
    );
    assert_eq!(
        std::fs::read(environment.path("saved/report.md")).expect("read unchanged source"),
        b"legacy report"
    );
}
