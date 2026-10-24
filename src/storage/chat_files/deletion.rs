use std::path::Path;

use super::atomic::{sync_directory, AtomicWriteTarget};
use super::lock::GlobalLinkLock;
use super::model::{link_filename_is_valid, validate_conversation_id};
use super::{ChatFileError, ChatFileId, ChatFileManifest, ChatSavedFileStore};

impl ChatSavedFileStore {
    pub(crate) fn delete_by_link(
        &self,
        conversation_id: i64,
        id: ChatFileId,
    ) -> Result<(), ChatFileError> {
        validate_conversation_id(conversation_id)?;
        let _mutation = self.acquire_mutation_lock()?;
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
            .strip_prefix(&self.saved_root)
            .ok()
            .and_then(Path::to_str)
            .filter(|filename| link_filename_is_valid(filename))
            .ok_or(ChatFileError::PathOutsideSavedRoot)?;
        let _mutation = self.acquire_mutation_lock()?;
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
        #[cfg(test)]
        if self
            .conversation_cleanup_failure
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            return Err(ChatFileError::InjectedPurgeFailure);
        }
        let _mutation = self.acquire_mutation_lock()?;
        let _lock = GlobalLinkLock::acquire(&self.paths)?;
        self.remove_conversation_links_locked(conversation_id)
    }

    pub(crate) fn mark_orphaned_conversation(
        &self,
        conversation_id: i64,
    ) -> Result<(), ChatFileError> {
        validate_conversation_id(conversation_id)?;
        let _mutation = self.acquire_mutation_lock()?;
        let _lock = GlobalLinkLock::acquire(&self.paths)?;
        self.require_native_adoption()?;
        crate::protection::open_directory(&self.paths.chat_file_links_dir, true)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        let empty = crate::protection::encode(crate::protection::Domain::Vault, b"")
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        self.writer.write_replace(
            &self.orphan_marker_path(conversation_id),
            &empty,
            AtomicWriteTarget::OrphanMarker,
        )
    }

    pub(crate) fn cleanup_orphans(&self) -> Result<usize, ChatFileError> {
        crate::protection::require(crate::protection::Domain::Vault)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        let _mutation = self.acquire_mutation_lock()?;
        let _lock = GlobalLinkLock::acquire(&self.paths)?;
        let directory = crate::protection::open_directory(&self.paths.chat_file_links_dir, true)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        let mut markers = Vec::new();
        for entry in directory.entries()? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Some(raw_id) = name.strip_prefix(".orphan-conversation-") else {
                continue;
            };
            let Ok(conversation_id) = raw_id.parse::<i64>() else {
                continue;
            };
            if conversation_id > 0 {
                markers.push((conversation_id, entry.file_name()));
            }
        }
        markers.sort_by_key(|(conversation_id, _)| *conversation_id);
        for (conversation_id, marker) in &markers {
            self.remove_conversation_links_locked(*conversation_id)?;
            directory.remove_file(marker)?;
            sync_directory(&self.paths.chat_file_links_dir)?;
        }
        Ok(markers.len())
    }

    fn remove_conversation_links_locked(&self, conversation_id: i64) -> Result<(), ChatFileError> {
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

    fn orphan_marker_path(&self, conversation_id: i64) -> std::path::PathBuf {
        self.paths
            .chat_file_links_dir
            .join(format!(".orphan-conversation-{conversation_id}"))
    }

    fn delete_filename(
        &self,
        filename: &str,
        manifest: ChatFileManifest,
    ) -> Result<(), ChatFileError> {
        let target = self.saved_path(filename);
        self.target_is_regular(&target)?;
        let tombstone = self.allocate_tombstone(filename);
        let (directory, _) = self.item_parent(Path::new(filename))?;
        directory.rename(
            target.file_name().ok_or(ChatFileError::InvalidFileName)?,
            &directory,
            tombstone
                .file_name()
                .ok_or(ChatFileError::InvalidFileName)?,
        )?;
        super::atomic::sync_handle(&directory)?;

        let prior = manifest;
        let mut updated = prior.clone();
        updated.links.retain(|link| link.filename != filename);
        for item in &mut updated.items {
            if item.relative_path == Path::new(filename)
                && matches!(item.state, super::model::VaultItemState::Active)
            {
                item.state = super::model::VaultItemState::Missing;
            }
        }
        if let Err(error) = self.write_manifest(&updated) {
            self.restore_tombstone(&tombstone, &target, "delete")?;
            return Err(error);
        }
        let removal = self.remove_tombstone(&tombstone);
        if let Err(error) = removal {
            self.rollback_delete(&prior, &tombstone, &target)?;
            return Err(error);
        }
        super::atomic::sync_handle(&directory)?;
        Ok(())
    }

    fn allocate_tombstone(&self, filename: &str) -> std::path::PathBuf {
        let relative = Path::new(filename);
        let parent = self
            .saved_root
            .join(relative.parent().unwrap_or_else(|| Path::new("")));
        let name = relative.file_name().unwrap_or_default().to_string_lossy();
        loop {
            let path = parent.join(format!(".delete-{}-{}.tmp", name, rand::random::<u64>()));
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
        let relative = target
            .strip_prefix(&self.saved_root)
            .map_err(|_| ChatFileError::PathOutsideSavedRoot)?;
        let (directory, _) = self.item_parent(relative)?;
        directory
            .rename(
                tombstone
                    .file_name()
                    .ok_or(ChatFileError::InvalidFileName)?,
                &directory,
                target.file_name().ok_or(ChatFileError::InvalidFileName)?,
            )
            .map_err(|rollback| ChatFileError::RollbackFailed {
                operation,
                rollback,
            })?;
        super::atomic::sync_handle(&directory)?;
        Ok(())
    }

    fn rollback_delete(
        &self,
        prior: &ChatFileManifest,
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
        let relative = tombstone
            .strip_prefix(&self.saved_root)
            .map_err(|_| ChatFileError::PathOutsideSavedRoot)?;
        let (directory, name) = self.saved_parent(relative)?;
        directory.remove_file(name)?;
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn fail_next_tombstone_remove(&self) {
        self.tombstone_remove_failure
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    #[cfg(test)]
    pub(crate) fn fail_next_conversation_cleanup(&self) {
        self.conversation_cleanup_failure
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
}
