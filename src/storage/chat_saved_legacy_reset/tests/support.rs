use std::path::{Path, PathBuf};

use crate::config::AppConfig;
use crate::storage::cache_paths::TcuiCachePaths;
use crate::storage::paths::TcuiDataPaths;

use super::super::{ChatSavedLegacyReset, LegacyResetEnvironment};

pub(super) struct TestRoot {
    path: PathBuf,
}

impl TestRoot {
    pub(super) fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "tcui-chat-saved-legacy-reset-{label}-{}",
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&path).expect("create test root");
        Self { path }
    }

    pub(super) fn path(&self, relative: &str) -> PathBuf {
        self.path.join(relative)
    }

    pub(super) fn create_dir(&self, relative: &str) -> PathBuf {
        let path = self.path(relative);
        std::fs::create_dir_all(&path).expect("create test directory");
        path
    }

    pub(super) fn write(&self, relative: &str, contents: &[u8]) -> PathBuf {
        let path = self.path(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create test file parent");
        }
        std::fs::write(&path, contents).expect("write test file");
        path
    }

    pub(super) fn read(&self, relative: &str) -> Vec<u8> {
        std::fs::read(self.path(relative)).expect("read test file")
    }

    pub(super) fn environment(&self) -> LegacyResetEnvironment {
        LegacyResetEnvironment {
            data_paths: TcuiDataPaths::from_root(self.path("data/tcui")),
            cache_paths: TcuiCachePaths::from_root(self.path("cache/tcui")),
            home_dir: self.path("home"),
        }
    }

    pub(super) fn config(&self, saved_root: &Path) -> AppConfig {
        AppConfig {
            saved_files_dir: Some(saved_root.display().to_string()),
            artifact_save_dir: None,
            ..AppConfig::default()
        }
    }

    pub(super) fn reset(
        &self,
        config: &AppConfig,
        active_vault_root: Option<&Path>,
    ) -> ChatSavedLegacyReset {
        ChatSavedLegacyReset::prepare(config, active_vault_root, self.environment())
            .expect("prepare legacy reset")
    }

    pub(super) fn marker_path(&self) -> PathBuf {
        self.path("data/tcui/migrations/chat-saved-files-v2.done")
    }

    pub(super) fn lock_path(&self) -> PathBuf {
        self.path("data/tcui/migrations/chat-saved-files-v2.lock")
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
