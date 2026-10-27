use super::ChatSavedFileStore;
use crate::storage::paths::TcuiDataPaths;

impl ChatSavedFileStore {
    pub(crate) fn discover() -> Self {
        Self::new(
            crate::protection::internal_vault_root(),
            TcuiDataPaths::discover(),
        )
    }
}
