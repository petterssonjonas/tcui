mod adoption;
mod atomic;
mod compat;
mod deletion;
mod items;
mod lock;
mod manifest;
mod model;
mod moves;

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
use model::{collision_filename, validate_conversation_id};
pub(crate) use model::{
    ChatFileError, ChatFileId, ChatFileLink, ChatFileManifest, SavedFileRecord, VaultItemId,
    VaultItemRecord,
};
pub(crate) use moves::NativeMoveSnapshot;

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
        let item = self.create_item(name, contents, None)?;
        Ok(SavedFileRecord {
            filename: item.filename,
            path: item.path,
            size_bytes: item.size_bytes,
        })
    }

    pub(crate) fn create_for_chat(
        &self,
        conversation_id: i64,
        name: &str,
        contents: &[u8],
    ) -> Result<ChatFileLink, ChatFileError> {
        self.create_item(name, contents, Some(conversation_id))?
            .chat_links
            .into_iter()
            .next()
            .ok_or(ChatFileError::CorruptManifest)
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
        Ok(self
            .list_items()?
            .into_iter()
            .map(|item| SavedFileRecord {
                filename: item.filename,
                path: item.path,
                size_bytes: item.size_bytes,
            })
            .collect())
    }

    pub(crate) fn list_for_conversation(
        &self,
        conversation_id: i64,
    ) -> Result<Vec<ChatFileLink>, ChatFileError> {
        validate_conversation_id(conversation_id)?;
        let _mutation = self.acquire_mutation_lock()?;
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
        let _mutation = self.acquire_mutation_lock()?;
        let _lock = GlobalLinkLock::acquire(&self.paths)?;
        self.ensure_saved_root()?;
        let mut manifest = self.load_manifest()?;
        self.prune_stale_links(&mut manifest)?;
        let link = manifest
            .links
            .iter()
            .find(|link| link.conversation_id == conversation_id && link.id == id)
            .ok_or(ChatFileError::NotFound { id })?;
        self.read_native_path(&self.saved_path(&link.filename))
    }

    fn acquire_mutation_lock(&self) -> Result<crate::vault::VaultMutationGuard, ChatFileError> {
        let guard = crate::vault::VaultMutationLock::at_data_root(&self.paths.root)
            .try_acquire()
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        self.require_native_adoption()?;
        crate::vault::move_journal::JournalStore::at_data_root(&self.paths.root)
            .ensure_mutation_allowed()
            .map_err(|error| ChatFileError::from_protection(error.into()))?;
        Ok(guard)
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
        let vault = crate::vault::VaultFs::open(&self.saved_root)
            .map_err(|error| ChatFileError::from_protection(error.into()))?;
        for link in manifest.links {
            let path = crate::vault::VaultRelativePath::parse(Path::new(&link.filename))
                .map_err(|_| ChatFileError::InvalidFileName)?;
            vault
                .read_bytes(&path)
                .map_err(|error| ChatFileError::from_protection(error.into()))?;
        }
        Ok(())
    }

    fn load_manifest(&self) -> Result<ChatFileManifest, ChatFileError> {
        LinkManifest::new(&self.paths, &self.writer).load()
    }

    fn write_manifest(&self, manifest: &ChatFileManifest) -> Result<(), ChatFileError> {
        LinkManifest::new(&self.paths, &self.writer).write(manifest)
    }

    fn prune_stale_links(&self, manifest: &mut ChatFileManifest) -> Result<(), ChatFileError> {
        self.reconcile_items(manifest)
    }

    fn write_unique_saved_file(
        &self,
        relative_path: &Path,
        contents: &[u8],
    ) -> Result<(String, PathBuf), ChatFileError> {
        let contents = crate::vault::encode_root(&self.saved_root, contents)?;
        let parent = relative_path.parent().unwrap_or_else(|| Path::new(""));
        crate::protection::open_directory(&self.saved_root.join(parent), true)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        let name = relative_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(ChatFileError::InvalidFileName)?;
        for suffix in 1..=u32::MAX {
            let relative = parent.join(collision_filename(name, suffix));
            let filename = relative
                .to_str()
                .ok_or(ChatFileError::InvalidFileName)?
                .to_string();
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

    fn allocate_id(&self, manifest: &ChatFileManifest) -> ChatFileId {
        loop {
            let id = ChatFileId::random();
            if manifest.links.iter().all(|link| link.id != id)
                && manifest
                    .items
                    .iter()
                    .all(|item| item.archived_links.iter().all(|link| link.id != id))
            {
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
        let relative = path
            .strip_prefix(&self.saved_root)
            .map_err(|_| ChatFileError::PathOutsideSavedRoot)?;
        let (directory, name) = self.item_parent(relative)?;
        directory
            .remove_file(name)
            .map_err(|rollback| ChatFileError::RollbackFailed {
                operation: "create",
                rollback,
            })?;
        atomic::sync_handle(&directory)?;
        Ok(())
    }

    fn saved_path(&self, filename: &str) -> PathBuf {
        self.saved_root.join(filename)
    }

    pub(crate) fn path_for_link(&self, link: &ChatFileLink) -> PathBuf {
        self.saved_path(&link.filename)
    }

    pub(crate) fn read_saved_path(&self, path: &Path) -> Result<Vec<u8>, ChatFileError> {
        let _mutation = self.acquire_mutation_lock()?;
        let _lock = GlobalLinkLock::acquire(&self.paths)?;
        self.ensure_saved_root()?;
        self.require_native_adoption()?;
        self.read_native_path(path)
    }

    fn read_native_path(&self, path: &Path) -> Result<Vec<u8>, ChatFileError> {
        let relative_path = path
            .strip_prefix(&self.saved_root)
            .map_err(|_| ChatFileError::PathOutsideSavedRoot)?;
        if !items::item_path_is_valid(relative_path) {
            return Err(ChatFileError::PathOutsideSavedRoot);
        }
        let relative = crate::vault::VaultRelativePath::parse(relative_path)
            .map_err(|_| ChatFileError::InvalidFileName)?;
        crate::vault::VaultFs::open(&self.saved_root)
            .and_then(|vault| vault.read_bytes(&relative))
            .map_err(|error| ChatFileError::from_protection(error.into()))
    }

    fn target_is_regular(&self, path: &Path) -> Result<(), ChatFileError> {
        let relative = path
            .strip_prefix(&self.saved_root)
            .map_err(|_| ChatFileError::PathOutsideSavedRoot)?;
        let (parent, name) = self.item_parent(relative)?;
        let metadata = parent.symlink_metadata(name)?;
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
