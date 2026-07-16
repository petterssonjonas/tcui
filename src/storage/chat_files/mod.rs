mod atomic;
mod lock;
mod manifest;
mod model;

use std::fs;
use std::path::Path;
#[cfg(test)]
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

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
    #[cfg(test)]
    purge_failure: Arc<AtomicBool>,
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
            #[cfg(test)]
            purge_failure: Arc::new(AtomicBool::new(false)),
        }
    }

    #[cfg(test)]
    fn with_limits(paths: TcuiCachePaths, limits: StoreLimits) -> Self {
        Self {
            paths,
            limits,
            writer: AtomicWriter::default(),
            purge_failure: Arc::new(AtomicBool::new(false)),
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
        let result = self.purge_conversation_inner(conversation_id);
        match result {
            Ok(()) => {
                self.remove_orphan_marker(conversation_id)?;
                Ok(())
            }
            Err(error) => {
                self.write_orphan_marker(conversation_id)?;
                Err(error)
            }
        }
    }

    pub(crate) fn cleanup_orphans(&self) -> Result<usize, ChatFileError> {
        if !self.paths.chat_files_dir.exists() {
            return Ok(0);
        }
        let mut conversation_ids = fs::read_dir(&self.paths.chat_files_dir)?
            .filter_map(Result::ok)
            .filter_map(|entry| orphan_conversation_id(&entry.file_name().to_string_lossy()))
            .collect::<Vec<_>>();
        conversation_ids.sort_unstable();
        conversation_ids.dedup();

        let mut cleaned = 0;
        for conversation_id in conversation_ids {
            self.purge_conversation(conversation_id)?;
            cleaned += 1;
        }
        Ok(cleaned)
    }

    fn purge_conversation_inner(&self, conversation_id: i64) -> Result<(), ChatFileError> {
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

        #[cfg(test)]
        if self.purge_failure.swap(false, Ordering::SeqCst) {
            return Err(ChatFileError::InjectedPurgeFailure);
        }

        let prefix = format!(".purge-{conversation_id}-");
        for entry in fs::read_dir(&self.paths.chat_files_dir)? {
            let entry = entry?;
            if entry.file_name().to_string_lossy().starts_with(&prefix) {
                fs::remove_dir_all(entry.path())?;
            }
        }
        atomic::sync_directory(&self.paths.chat_files_dir)?;
        Ok(())
    }

    fn write_orphan_marker(&self, conversation_id: i64) -> Result<(), ChatFileError> {
        self.paths.ensure_layout()?;
        let marker = self.orphan_marker(conversation_id);
        fs::write(&marker, [])?;
        crate::storage::paths::set_unix_mode(&marker, 0o600)?;
        atomic::sync_directory(&self.paths.chat_files_dir)
    }

    fn remove_orphan_marker(&self, conversation_id: i64) -> Result<(), ChatFileError> {
        let marker = self.orphan_marker(conversation_id);
        match fs::remove_file(marker) {
            Ok(()) => atomic::sync_directory(&self.paths.chat_files_dir),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    fn orphan_marker(&self, conversation_id: i64) -> std::path::PathBuf {
        self.paths
            .chat_files_dir
            .join(format!(".orphan-{conversation_id}"))
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

    #[cfg(test)]
    pub(crate) fn fail_next_purge(&self) {
        self.purge_failure.store(true, Ordering::SeqCst);
    }
}

fn orphan_conversation_id(name: &str) -> Option<i64> {
    name.strip_prefix(".orphan-")
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value > 0)
}

#[cfg(test)]
mod tests;
