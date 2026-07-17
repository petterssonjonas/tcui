use std::path::PathBuf;

use crate::storage::cache_paths::TcuiCachePaths;

use super::{ChatFileStore, StoreLimits};

mod chat_file_store_limits;
mod chat_file_store_locking;
mod chat_file_store_persistence;
mod chat_file_store_recovery;

struct TestRoot {
    path: PathBuf,
}

impl TestRoot {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "tcui-chat-file-store-{label}-{}",
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&path).expect("create test root");
        Self { path }
    }

    fn paths(&self) -> TcuiCachePaths {
        TcuiCachePaths::from_root(self.path.join("tcui"))
    }

    fn store(&self) -> ChatFileStore {
        ChatFileStore::new(self.paths())
    }

    fn store_with_limits(&self, per_file: u64, per_conversation: u64) -> ChatFileStore {
        ChatFileStore::with_limits(
            self.paths(),
            StoreLimits {
                per_file,
                per_conversation,
            },
        )
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
