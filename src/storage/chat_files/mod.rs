mod atomic;
mod compat;
mod deletion;
mod lock;
mod manifest;
mod model;

use std::fs;
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

use crate::storage::paths::{ensure_directory, TcuiDataPaths};

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
        let contents = fs::read(source)?;
        self.create_for_chat(conversation_id, name, &contents)
    }

    pub(crate) fn list_saved(&self) -> Result<Vec<SavedFileRecord>, ChatFileError> {
        self.ensure_saved_root()?;
        let mut records = Vec::new();
        for entry in fs::read_dir(&self.saved_root)? {
            let entry = entry?;
            let metadata = fs::symlink_metadata(entry.path())?;
            let Some(filename) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            if metadata.file_type().is_file() && !filename.starts_with('.') {
                records.push(SavedFileRecord {
                    filename,
                    path: entry.path(),
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
        Ok(fs::read(self.saved_path(&link.filename))?)
    }

    fn ensure_saved_root(&self) -> Result<(), ChatFileError> {
        ensure_directory(&self.saved_root)?;
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
        manifest.links.retain(|link| {
            fs::symlink_metadata(self.saved_path(&link.filename))
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
        let sanitized = sanitize_name(name);
        for suffix in 1..=u32::MAX {
            let filename = collision_filename(&sanitized, suffix);
            let path = self.saved_path(&filename);
            match self
                .writer
                .write_new(&path, contents, AtomicWriteTarget::SavedFile)
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
        fs::remove_file(path).map_err(|rollback| ChatFileError::RollbackFailed {
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

    fn target_is_regular(&self, path: &Path) -> Result<(), ChatFileError> {
        let metadata = fs::symlink_metadata(path)?;
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
