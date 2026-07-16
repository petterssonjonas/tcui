mod atomic;
mod lock;
mod manifest;
mod model;

use std::fs;
use std::path::Path;

use atomic::AtomicWriter;
use lock::ConversationLock;
use manifest::{ConversationManifest, ManifestInspection};
use model::StoreLimits;

pub(crate) use model::{ChatFileError, ChatFileId, ChatFileManifestV1, ChatFileRecord};

use crate::storage::cache_paths::TcuiCachePaths;

#[cfg(test)]
pub(crate) use atomic::AtomicWriteTarget;

#[derive(Debug, Clone)]
pub(crate) struct ChatFileStore {
    paths: TcuiCachePaths,
    limits: StoreLimits,
    writer: AtomicWriter,
}

impl ChatFileStore {
    pub(crate) fn discover() -> Self {
        Self::new(TcuiCachePaths::discover())
    }

    pub(crate) fn new(paths: TcuiCachePaths) -> Self {
        Self {
            paths,
            limits: StoreLimits::default(),
            writer: AtomicWriter::default(),
        }
    }

    #[cfg(test)]
    fn with_limits(paths: TcuiCachePaths, limits: StoreLimits) -> Self {
        Self {
            paths,
            limits,
            writer: AtomicWriter::default(),
        }
    }

    pub(crate) fn create(
        &self,
        conversation_id: i64,
        name: &str,
        contents: &[u8],
    ) -> Result<ChatFileRecord, ChatFileError> {
        model::validate_conversation_id(conversation_id)?;
        let size_bytes = model::payload_size(contents, self.limits.per_file)?;
        let directory = self.paths.conversation_dir(conversation_id);
        let _lock = ConversationLock::acquire(&self.paths, conversation_id)?;
        let mut manifest = self
            .manifest(&directory, conversation_id)
            .load_or_rebuild()?;
        model::enforce_conversation_quota(&manifest.files, size_bytes, self.limits)?;

        let name = model::sanitize_name(name);
        let id = self.allocate_id(&directory, &manifest)?;
        let record = ChatFileRecord {
            id,
            name,
            size_bytes,
        };
        let payload_path = directory.join(record.payload_name());
        self.writer
            .write(&payload_path, contents, atomic::AtomicWriteTarget::Payload)?;

        manifest.files.push(record.clone());
        if let Err(error) = self.manifest(&directory, conversation_id).write(&manifest) {
            if let Err(rollback) = fs::remove_file(&payload_path) {
                return Err(ChatFileError::RollbackFailed {
                    operation: "create",
                    rollback,
                });
            }
            atomic::sync_directory(&directory)?;
            return Err(error);
        }
        Ok(record)
    }

    pub(crate) fn list(&self, conversation_id: i64) -> Result<Vec<ChatFileRecord>, ChatFileError> {
        model::validate_conversation_id(conversation_id)?;
        let directory = self.paths.conversation_dir(conversation_id);
        if !directory.exists() {
            return Ok(Vec::new());
        }

        match self.manifest(&directory, conversation_id).inspect()? {
            ManifestInspection::Current(manifest) => Ok(manifest.files),
            ManifestInspection::Future(version) => {
                Err(ChatFileError::UnsupportedManifestVersion { version })
            }
            ManifestInspection::Missing | ManifestInspection::Corrupt => {
                let _lock = ConversationLock::acquire(&self.paths, conversation_id)?;
                Ok(self
                    .manifest(&directory, conversation_id)
                    .load_or_rebuild()?
                    .files)
            }
        }
    }

    pub(crate) fn open(
        &self,
        conversation_id: i64,
        id: ChatFileId,
    ) -> Result<Vec<u8>, ChatFileError> {
        let record = self
            .list(conversation_id)?
            .into_iter()
            .find(|record| record.id == id)
            .ok_or(ChatFileError::NotFound { id })?;
        let path = self
            .paths
            .conversation_dir(conversation_id)
            .join(record.payload_name());
        let contents = fs::read(path)?;
        let actual_size = u64::try_from(contents.len()).map_err(|_| ChatFileError::SizeOverflow)?;
        if actual_size != record.size_bytes {
            return Err(ChatFileError::PayloadChanged { id });
        }
        Ok(contents)
    }

    pub(crate) fn delete(&self, conversation_id: i64, id: ChatFileId) -> Result<(), ChatFileError> {
        model::validate_conversation_id(conversation_id)?;
        let directory = self.paths.conversation_dir(conversation_id);
        let _lock = ConversationLock::acquire(&self.paths, conversation_id)?;
        let mut manifest = self
            .manifest(&directory, conversation_id)
            .load_or_rebuild()?;
        let index = manifest
            .files
            .iter()
            .position(|record| record.id == id)
            .ok_or(ChatFileError::NotFound { id })?;
        let record = manifest.files.remove(index);
        let payload_path = directory.join(record.payload_name());
        let tombstone = directory.join(format!(".delete-{}-{}.tmp", id, rand::random::<u64>()));
        fs::rename(&payload_path, &tombstone)?;
        atomic::sync_directory(&directory)?;

        if let Err(error) = self.manifest(&directory, conversation_id).write(&manifest) {
            if let Err(rollback) = fs::rename(&tombstone, &payload_path) {
                return Err(ChatFileError::RollbackFailed {
                    operation: "delete",
                    rollback,
                });
            }
            atomic::sync_directory(&directory)?;
            return Err(error);
        }
        fs::remove_file(tombstone)?;
        atomic::sync_directory(&directory)?;
        Ok(())
    }

    pub(crate) fn purge_conversation(&self, conversation_id: i64) -> Result<(), ChatFileError> {
        model::validate_conversation_id(conversation_id)?;
        let directory = self.paths.conversation_dir(conversation_id);
        let conversation_lock = ConversationLock::acquire(&self.paths, conversation_id)?;
        self.manifest(&directory, conversation_id)
            .load_or_rebuild()?;
        let purged = self.paths.chat_files_dir.join(format!(
            ".purge-{conversation_id}-{}",
            rand::random::<u64>()
        ));
        fs::rename(&directory, &purged)?;
        atomic::sync_directory(&self.paths.chat_files_dir)?;
        drop(conversation_lock);
        fs::remove_dir_all(purged)?;
        atomic::sync_directory(&self.paths.chat_files_dir)?;
        Ok(())
    }

    pub(crate) fn rebuild_manifest(
        &self,
        conversation_id: i64,
    ) -> Result<Vec<ChatFileRecord>, ChatFileError> {
        model::validate_conversation_id(conversation_id)?;
        let directory = self.paths.conversation_dir(conversation_id);
        let _lock = ConversationLock::acquire(&self.paths, conversation_id)?;
        Ok(self
            .manifest(&directory, conversation_id)
            .force_rebuild()?
            .files)
    }

    fn manifest<'a>(
        &'a self,
        directory: &'a Path,
        conversation_id: i64,
    ) -> ConversationManifest<'a> {
        ConversationManifest::new(directory, conversation_id, self)
    }

    fn allocate_id(
        &self,
        directory: &Path,
        manifest: &ChatFileManifestV1,
    ) -> Result<ChatFileId, ChatFileError> {
        loop {
            let id = ChatFileId::random();
            let in_manifest = manifest.files.iter().any(|record| record.id == id);
            let prefix = format!("{id}--");
            let mut on_disk = false;
            for entry in fs::read_dir(directory)? {
                if entry?.file_name().to_string_lossy().starts_with(&prefix) {
                    on_disk = true;
                    break;
                }
            }
            if !in_manifest && !on_disk {
                return Ok(id);
            }
        }
    }

    #[cfg(test)]
    fn fail_next_atomic_write(&self, target: AtomicWriteTarget) {
        self.writer.fail_next(target);
    }
}

#[cfg(test)]
mod tests;
