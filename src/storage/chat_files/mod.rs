mod atomic;
mod compat;
mod deletion;
mod lock;
mod manifest;
mod model;

use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use atomic::AtomicWriter;
use lock::GlobalLinkLock;
use manifest::LinkManifest;

pub(crate) use atomic::AtomicWriteTarget;
use model::{collision_filename, sanitize_name, validate_conversation_id};
pub(crate) use model::{
    ChatFileError, ChatFileId, ChatFileLink, ChatFileLinkManifestV1, SavedFileRecord,
};

use crate::storage::paths::TcuiDataPaths;

#[derive(Debug, Clone)]
pub(crate) struct ChatSavedFileStore {
    saved_root: PathBuf,
    paths: TcuiDataPaths,
    writer: AtomicWriter,
    #[cfg(test)]
    rollback_failure: Arc<AtomicBool>,
    #[cfg(test)]
    tombstone_remove_failure: Arc<AtomicBool>,
    #[cfg(test)]
    conversation_cleanup_failure: Arc<AtomicBool>,
}

impl ChatSavedFileStore {
    pub(crate) fn new(saved_root: PathBuf, paths: TcuiDataPaths) -> Self {
        Self {
            saved_root,
            paths,
            writer: AtomicWriter::default(),
            #[cfg(test)]
            rollback_failure: Arc::new(AtomicBool::new(false)),
            #[cfg(test)]
            tombstone_remove_failure: Arc::new(AtomicBool::new(false)),
            #[cfg(test)]
            conversation_cleanup_failure: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Save an explicitly requested artifact without retaining a conversation link.
    pub(crate) fn create_saved(
        &self,
        name: &str,
        contents: &[u8],
    ) -> Result<SavedFileRecord, ChatFileError> {
        let _lock = GlobalLinkLock::acquire(&self.paths)?;
        self.ensure_saved_root()?;
        let (filename, path) = self.write_unique_saved_file(name, contents)?;
        Ok(SavedFileRecord {
            filename,
            path,
            size_bytes: contents.len() as u64,
        })
    }

    pub(crate) fn create_for_chat(
        &self,
        conversation_id: i64,
        name: &str,
        contents: &[u8],
    ) -> Result<ChatFileLink, ChatFileError> {
        validate_conversation_id(conversation_id)?;
        let _lock = GlobalLinkLock::acquire(&self.paths)?;
        self.ensure_saved_root()?;
        let mut manifest = self.load_manifest()?;
        self.prune_stale_links(&mut manifest)?;
        let (filename, path) = self.write_unique_saved_file(name, contents)?;
        let link = ChatFileLink {
            id: self.allocate_id(&manifest),
            conversation_id,
            filename,
        };
        manifest.links.push(link.clone());
        if let Err(error) = self.write_manifest(&manifest) {
            self.rollback_created_file(&path)?;
            return Err(error);
        }
        Ok(link)
    }

    pub(crate) fn import_for_chat(
        &self,
        conversation_id: i64,
        source: &Path,
    ) -> Result<ChatFileLink, ChatFileError> {
        let name = source
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(ChatFileError::InvalidFileName)?;
        let contents = crate::vault::read_artifact_path(source)?;
        self.create_for_chat(conversation_id, name, &contents)
    }

    pub(crate) fn list_saved(&self) -> Result<Vec<SavedFileRecord>, ChatFileError> {
        self.ensure_saved_root()?;
        let mut records = Vec::new();
        let directory = self.open_saved_root()?;
        for entry in directory.entries()? {
            let entry = entry?;
            let metadata = directory.symlink_metadata(entry.file_name())?;
            let Some(filename) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            if metadata.file_type().is_file() && !filename.starts_with('.') {
                records.push(SavedFileRecord {
                    filename,
                    path: self.saved_root.join(entry.file_name()),
                    size_bytes: metadata.len(),
                });
            }
        }
        records.sort_by(|left, right| left.filename.cmp(&right.filename));
        Ok(records)
    }

    pub(crate) fn list_for_conversation(
        &self,
        conversation_id: i64,
    ) -> Result<Vec<ChatFileLink>, ChatFileError> {
        validate_conversation_id(conversation_id)?;
        let _lock = GlobalLinkLock::acquire(&self.paths)?;
        self.ensure_saved_root()?;
        let mut manifest = self.load_manifest()?;
        self.prune_stale_links(&mut manifest)?;
        Ok(manifest
            .links
            .into_iter()
            .filter(|link| link.conversation_id == conversation_id)
            .collect())
    }

    pub(crate) fn open_link(
        &self,
        conversation_id: i64,
        id: ChatFileId,
    ) -> Result<Vec<u8>, ChatFileError> {
        validate_conversation_id(conversation_id)?;
        let _lock = GlobalLinkLock::acquire(&self.paths)?;
        self.ensure_saved_root()?;
        let mut manifest = self.load_manifest()?;
        self.prune_stale_links(&mut manifest)?;
        let link = manifest
            .links
            .iter()
            .find(|link| link.conversation_id == conversation_id && link.id == id)
            .ok_or(ChatFileError::NotFound { id })?;
        self.read_saved_path(&self.saved_path(&link.filename))
    }

    fn open_saved_root(&self) -> Result<cap_std::fs::Dir, ChatFileError> {
        crate::vault::require_root(&self.saved_root)?;
        crate::protection::open_directory(&self.saved_root, true)
            .map_err(|error| std::io::Error::other(error.to_string()).into())
    }

    fn ensure_saved_root(&self) -> Result<(), ChatFileError> {
        let complete = if self.saved_root == self.paths.root.join("vault") {
            crate::storage::saved_file_migration::require_complete(&self.paths)?
        } else {
            true
        };
        self.open_saved_root()?;
        if !complete {
            self.verify_migrated_targets()?;
        }
        Ok(())
    }

    pub(crate) fn verify_migrated_targets(&self) -> Result<(), ChatFileError> {
        let manifest = self.load_manifest()?;
        let vault = crate::vault::VaultFs::open_profile(&self.saved_root)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        for link in manifest.links {
            let path = crate::vault::VaultRelativePath::parse(Path::new(&link.filename))
                .map_err(|_| ChatFileError::InvalidFileName)?;
            vault.read_bytes(&path).map_err(|error| {
                std::io::Error::other(format!(
                    "Saved migration cannot complete while linked file {} is unavailable: {error}",
                    link.filename
                ))
            })?;
        }
        Ok(())
    }

    pub(crate) fn require_unlinked_move(&self, source: &Path) -> Result<(), ChatFileError> {
        if source.parent() != Some(self.saved_root.as_path()) {
            return Ok(());
        }
        self.ensure_saved_root()?;
        let filename = source
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(ChatFileError::InvalidFileName)?;
        if self
            .load_manifest()?
            .links
            .iter()
            .any(|link| link.filename == filename)
        {
            return Err(std::io::Error::other("This file has chat links and cannot be moved or renamed. Explicitly copy/export it instead; the linked original must remain in the internal Vault.").into());
        }
        Ok(())
    }

    fn load_manifest(&self) -> Result<ChatFileLinkManifestV1, ChatFileError> {
        LinkManifest::new(&self.paths, &self.writer).load()
    }

    fn write_manifest(&self, manifest: &ChatFileLinkManifestV1) -> Result<(), ChatFileError> {
        LinkManifest::new(&self.paths, &self.writer).write(manifest)
    }

    fn prune_stale_links(
        &self,
        manifest: &mut ChatFileLinkManifestV1,
    ) -> Result<(), ChatFileError> {
        let before = manifest.links.len();
        let directory = self.open_saved_root()?;
        manifest.links.retain(|link| {
            directory
                .symlink_metadata(&link.filename)
                .is_ok_and(|metadata| metadata.file_type().is_file())
        });
        if manifest.links.len() != before {
            self.write_manifest(manifest)?;
        }
        Ok(())
    }

    fn write_unique_saved_file(
        &self,
        name: &str,
        contents: &[u8],
    ) -> Result<(String, PathBuf), ChatFileError> {
        let contents = crate::vault::encode_root(&self.saved_root, contents)?;
        let sanitized = sanitize_name(name);
        for suffix in 1..=u32::MAX {
            let filename = collision_filename(&sanitized, suffix);
            let path = self.saved_path(&filename);
            match self
                .writer
                .write_new(&path, &contents, AtomicWriteTarget::SavedFile)
            {
                Ok(()) => return Ok((filename, path)),
                Err(ChatFileError::Io(error))
                    if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "Saved files directory has no available filename",
        )
        .into())
    }

    fn allocate_id(&self, manifest: &ChatFileLinkManifestV1) -> ChatFileId {
        loop {
            let id = ChatFileId::random();
            if manifest.links.iter().all(|link| link.id != id) {
                return id;
            }
        }
    }

    fn rollback_created_file(&self, path: &Path) -> Result<(), ChatFileError> {
        #[cfg(test)]
        if self.rollback_failure.swap(false, Ordering::SeqCst) {
            return Err(ChatFileError::RollbackFailed {
                operation: "create",
                rollback: std::io::Error::other("injected rollback failure"),
            });
        }
        let filename = path.file_name().ok_or(ChatFileError::InvalidFileName)?;
        self.open_saved_root()?
            .remove_file(filename)
            .map_err(|rollback| ChatFileError::RollbackFailed {
                operation: "create",
                rollback,
            })?;
        atomic::sync_directory(&self.saved_root)
    }

    fn saved_path(&self, filename: &str) -> PathBuf {
        self.saved_root.join(filename)
    }

    pub(crate) fn path_for_link(&self, link: &ChatFileLink) -> PathBuf {
        self.saved_path(&link.filename)
    }

    pub(crate) fn read_saved_path(&self, path: &Path) -> Result<Vec<u8>, ChatFileError> {
        self.ensure_saved_root()?;
        let filename = path.file_name().ok_or(ChatFileError::InvalidFileName)?;
        if path.parent() != Some(self.saved_root.as_path()) {
            return Err(ChatFileError::PathOutsideSavedRoot);
        }
        let relative = crate::vault::VaultRelativePath::parse(Path::new(filename))
            .map_err(|_| ChatFileError::InvalidFileName)?;
        crate::vault::VaultFs::open_profile(&self.saved_root)
            .and_then(|vault| vault.read_bytes(&relative))
            .map_err(|error| std::io::Error::other(error.to_string()).into())
    }

    fn target_is_regular(&self, path: &Path) -> Result<(), ChatFileError> {
        let metadata = self
            .open_saved_root()?
            .symlink_metadata(path.file_name().ok_or(ChatFileError::InvalidFileName)?)?;
        if metadata.file_type().is_file() {
            Ok(())
        } else {
            Err(ChatFileError::NotRegularFile {
                path: path.to_path_buf(),
            })
        }
    }

    #[cfg(test)]
    fn fail_next_atomic_write(&self, target: AtomicWriteTarget) {
        self.writer.fail_next(target);
    }

    #[cfg(test)]
    pub(crate) fn fail_next_link_write(&self) {
        self.writer.fail_next(AtomicWriteTarget::Manifest);
    }

    #[cfg(test)]
    fn fail_next_rollback(&self) {
        self.rollback_failure.store(true, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests;
