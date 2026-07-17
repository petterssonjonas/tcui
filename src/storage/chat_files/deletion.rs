use std::fs;
use std::path::Path;

use super::atomic::sync_directory;
use super::lock::GlobalLinkLock;
use super::model::{link_filename_is_valid, validate_conversation_id};
use super::{ChatFileError, ChatFileId, ChatFileLinkManifestV1, ChatSavedFileStore};

impl ChatSavedFileStore {
    pub(crate) fn delete_by_link(
        &self,
        conversation_id: i64,
        id: ChatFileId,
    ) -> Result<(), ChatFileError> {
        validate_conversation_id(conversation_id)?;
        let _lock = GlobalLinkLock::acquire(&self.paths)?;
        self.ensure_saved_root()?;
        let mut manifest = self.load_manifest()?;
        self.prune_stale_links(&mut manifest)?;
        let filename = manifest
            .links
            .iter()
            .find(|link| link.conversation_id == conversation_id && link.id == id)
            .map(|link| link.filename.clone())
            .ok_or(ChatFileError::NotFound { id })?;
        self.delete_filename(&filename, manifest)
    }

    pub(crate) fn delete_by_saved_path(&self, path: &Path) -> Result<(), ChatFileError> {
        let filename = path
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|filename| link_filename_is_valid(filename))
            .ok_or(ChatFileError::PathOutsideSavedRoot)?;
        if path.parent() != Some(self.saved_root.as_path()) {
            return Err(ChatFileError::PathOutsideSavedRoot);
        }
        let _lock = GlobalLinkLock::acquire(&self.paths)?;
        self.ensure_saved_root()?;
        let mut manifest = self.load_manifest()?;
        self.prune_stale_links(&mut manifest)?;
        self.delete_filename(filename, manifest)
    }

    pub(crate) fn remove_conversation_links(
        &self,
        conversation_id: i64,
    ) -> Result<(), ChatFileError> {
        validate_conversation_id(conversation_id)?;
        let _lock = GlobalLinkLock::acquire(&self.paths)?;
        self.ensure_saved_root()?;
        let mut manifest = self.load_manifest()?;
        self.prune_stale_links(&mut manifest)?;
        let before = manifest.links.len();
        manifest
            .links
            .retain(|link| link.conversation_id != conversation_id);
        if manifest.links.len() != before {
            self.write_manifest(&manifest)?;
        }
        Ok(())
    }

    fn delete_filename(
        &self,
        filename: &str,
        manifest: ChatFileLinkManifestV1,
    ) -> Result<(), ChatFileError> {
        let target = self.saved_path(filename);
        self.target_is_regular(&target)?;
        let tombstone = self.allocate_tombstone(filename);
        fs::rename(&target, &tombstone)?;
        sync_directory(&self.saved_root)?;

        let prior = manifest;
        let mut updated = prior.clone();
        updated.links.retain(|link| link.filename != filename);
        if let Err(error) = self.write_manifest(&updated) {
            self.restore_tombstone(&tombstone, &target, "delete")?;
            return Err(error);
        }
        let removal = self.remove_tombstone(&tombstone);
        if let Err(error) = removal {
            self.rollback_delete(&prior, &tombstone, &target)?;
            return Err(error);
        }
        sync_directory(&self.saved_root)
    }

    fn allocate_tombstone(&self, filename: &str) -> std::path::PathBuf {
        loop {
            let path = self.saved_root.join(format!(
                ".delete-{}-{}.tmp",
                filename,
                rand::random::<u64>()
            ));
            if !path.exists() {
                return path;
            }
        }
    }

    fn restore_tombstone(
        &self,
        tombstone: &Path,
        target: &Path,
        operation: &'static str,
    ) -> Result<(), ChatFileError> {
        fs::rename(tombstone, target).map_err(|rollback| ChatFileError::RollbackFailed {
            operation,
            rollback,
        })?;
        sync_directory(&self.saved_root)
    }

    fn rollback_delete(
        &self,
        prior: &ChatFileLinkManifestV1,
        tombstone: &Path,
        target: &Path,
    ) -> Result<(), ChatFileError> {
        self.write_manifest(prior)
            .map_err(|rollback| ChatFileError::RollbackFailed {
                operation: "delete",
                rollback: std::io::Error::other(rollback.to_string()),
            })?;
        self.restore_tombstone(tombstone, target, "delete")
    }

    fn remove_tombstone(&self, tombstone: &Path) -> Result<(), ChatFileError> {
        #[cfg(test)]
        if self
            .tombstone_remove_failure
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            return Err(ChatFileError::InjectedTombstoneRemoveFailure);
        }
        fs::remove_file(tombstone)?;
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn fail_next_tombstone_remove(&self) {
        self.tombstone_remove_failure
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
}
