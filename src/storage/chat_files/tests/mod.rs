use std::path::{Path, PathBuf};

use crate::storage::paths::TcuiDataPaths;

use super::ChatSavedFileStore;

mod chat_saved_file_store_deletion;
mod chat_saved_file_store_failures;
mod chat_saved_file_store_persistence;
mod chat_saved_file_store_reconciliation;
mod native_items;

struct TestRoot {
    path: PathBuf,
    _guard: std::sync::MutexGuard<'static, ()>,
}

impl TestRoot {
    fn new(label: &str) -> Self {
        let guard = crate::test_support::env_lock().lock().expect("env lock");
        let path = std::env::temp_dir().join(format!(
            "tcui-chat-saved-file-store-{label}-{}",
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&path).expect("create test root");
        Self {
            path,
            _guard: guard,
        }
    }

    fn saved_root(&self) -> PathBuf {
        self.path.join("saved")
    }

    fn data_paths(&self) -> TcuiDataPaths {
        TcuiDataPaths::from_root(self.path.join("data").join("tcui"))
    }

    fn store(&self) -> ChatSavedFileStore {
        ChatSavedFileStore::new(self.saved_root(), self.data_paths())
    }

    fn manifest_path(&self) -> PathBuf {
        self.data_paths().chat_file_links_manifest
    }

    fn lock_path(&self) -> PathBuf {
        self.data_paths().chat_file_links_lock
    }

    fn saved_path(&self, filename: &str) -> PathBuf {
        self.saved_root().join(filename)
    }

    fn write_manifest(&self, bytes: &[u8]) {
        let paths = self.data_paths();
        std::fs::create_dir_all(&paths.chat_file_links_dir).expect("create link directory");
        std::fs::write(paths.chat_file_links_manifest, bytes).expect("write link manifest");
    }

    fn read(&self, path: &Path) -> Vec<u8> {
        std::fs::read(path).expect("read test file")
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
