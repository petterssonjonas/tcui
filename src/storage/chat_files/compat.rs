use std::path::PathBuf;

use super::ChatSavedFileStore;
use crate::storage::paths::TcuiDataPaths;

impl ChatSavedFileStore {
    pub(crate) fn discover() -> Self {
        Self::new(
            PathBuf::from(crate::config::app_config::default_artifact_save_dir()),
            TcuiDataPaths::discover(),
        )
    }
}
