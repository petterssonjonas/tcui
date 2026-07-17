use std::path::PathBuf;

use super::{ChatFileError, ChatFileId, ChatFileRecord, ChatSavedFileStore};
use crate::storage::paths::TcuiDataPaths;

impl ChatSavedFileStore {
    pub(crate) fn discover() -> Self {
        Self::new(
            PathBuf::from(crate::config::app_config::default_artifact_save_dir()),
            TcuiDataPaths::discover(),
        )
    }

    pub(crate) fn create(
        &self,
        conversation_id: i64,
        name: &str,
        contents: &[u8],
    ) -> Result<ChatFileRecord, ChatFileError> {
        let link = self.create_for_chat(conversation_id, name, contents)?;
        Ok(ChatFileRecord {
            id: link.id,
            name: link.filename,
            size_bytes: u64::try_from(contents.len())
                .map_err(|error| std::io::Error::other(error.to_string()))?,
        })
    }

    pub(crate) fn list(&self, conversation_id: i64) -> Result<Vec<ChatFileRecord>, ChatFileError> {
        self.list_for_conversation(conversation_id)?
            .into_iter()
            .map(|link| {
                let size_bytes = std::fs::metadata(self.saved_path(&link.filename))?.len();
                Ok(ChatFileRecord {
                    id: link.id,
                    name: link.filename,
                    size_bytes,
                })
            })
            .collect()
    }

    pub(crate) fn open(
        &self,
        conversation_id: i64,
        id: ChatFileId,
    ) -> Result<Vec<u8>, ChatFileError> {
        self.open_link(conversation_id, id)
    }

    pub(crate) fn delete(&self, conversation_id: i64, id: ChatFileId) -> Result<(), ChatFileError> {
        self.delete_by_link(conversation_id, id)
    }

    pub(crate) fn purge_conversation(&self, conversation_id: i64) -> Result<(), ChatFileError> {
        #[cfg(test)]
        if self
            .purge_failure
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            return Err(ChatFileError::InjectedPurgeFailure);
        }
        self.remove_conversation_links(conversation_id)
    }

    pub(crate) const fn cleanup_orphans(&self) -> Result<usize, ChatFileError> {
        Ok(0)
    }

    #[cfg(test)]
    pub(crate) fn fail_next_purge(&self) {
        self.purge_failure
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
}
