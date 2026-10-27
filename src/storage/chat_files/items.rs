use std::collections::HashSet;
use std::ffi::OsString;
use std::io;
use std::path::{Component, Path, PathBuf};

use cap_fs_ext::DirExt;
use cap_std::fs::Dir;

use super::atomic::{sync_directory, sync_handle, AtomicWriteTarget};
use super::lock::GlobalLinkLock;
use super::model::{validate_conversation_id, VaultItemEntry, VaultItemState, MANIFEST_VERSION};
use super::{
    ChatFileError, ChatFileLink, ChatFileManifest, ChatSavedFileStore, VaultItemId, VaultItemRecord,
};

impl ChatSavedFileStore {
    /// List all native files, including nested files. Existing bodies are indexed in place.
    pub(crate) fn list_items(&self) -> Result<Vec<VaultItemRecord>, ChatFileError> {
        let _mutation = self.acquire_mutation_lock()?;
        let _lock = GlobalLinkLock::acquire(&self.paths)?;
        self.ensure_saved_root()?;
        let mut manifest = self.load_manifest()?;
        self.prune_stale_links(&mut manifest)?;
        let mut records = manifest
            .items
            .iter()
            .filter(|item| matches!(item.state, VaultItemState::Active))
            .map(|item| self.item_record(item, &manifest))
            .collect::<Result<Vec<_>, _>>()?;
        records.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        Ok(records)
    }

    pub(crate) fn read_item(&self, id: VaultItemId) -> Result<Vec<u8>, ChatFileError> {
        let _mutation = self.acquire_mutation_lock()?;
        let _lock = GlobalLinkLock::acquire(&self.paths)?;
        self.ensure_saved_root()?;
        let mut manifest = self.load_manifest()?;
        self.prune_stale_links(&mut manifest)?;
        let item = active_item(&manifest, id)?;
        self.read_native_path(&self.saved_root.join(&item.relative_path))
    }

    /// Create one authoritative body, with an optional existing-style chat link.
    pub(crate) fn create_item(
        &self,
        name: &str,
        contents: &[u8],
        conversation_id: Option<i64>,
    ) -> Result<VaultItemRecord, ChatFileError> {
        let filename = super::model::sanitize_name(name);
        self.create_item_at(Path::new(&filename), contents, conversation_id)
    }

    /// Preserve an imported folder path without overwriting another native file.
    pub(crate) fn create_item_at(
        &self,
        relative_path: &Path,
        contents: &[u8],
        conversation_id: Option<i64>,
    ) -> Result<VaultItemRecord, ChatFileError> {
        self.create_native_item(relative_path, contents, conversation_id, None)
    }

    /// Preserve a source transfer ID only when it cannot rebind an existing or retired ID.
    pub(crate) fn import_item_at(
        &self,
        relative_path: &Path,
        contents: &[u8],
        preferred_id: Option<VaultItemId>,
    ) -> Result<VaultItemRecord, ChatFileError> {
        self.create_native_item(relative_path, contents, None, preferred_id)
    }

    fn create_native_item(
        &self,
        relative_path: &Path,
        contents: &[u8],
        conversation_id: Option<i64>,
        preferred_id: Option<VaultItemId>,
    ) -> Result<VaultItemRecord, ChatFileError> {
        if !item_path_is_valid(relative_path) {
            return Err(ChatFileError::InvalidFileName);
        }
        if let Some(conversation_id) = conversation_id {
            validate_conversation_id(conversation_id)?;
        }
        let _mutation = self.acquire_mutation_lock()?;
        let _lock = GlobalLinkLock::acquire(&self.paths)?;
        self.ensure_saved_root()?;
        let mut manifest = self.load_manifest()?;
        self.prune_stale_links(&mut manifest)?;
        let id = match preferred_id {
            Some(id) if id.get() >= manifest.next_item_id && id.get() < u64::MAX => {
                manifest.next_item_id = id.get() + 1;
                id
            }
            _ => allocate_item_id(&mut manifest)?,
        };
        let (filename, path) = self.write_unique_saved_file(relative_path, contents)?;
        if let Some(conversation_id) = conversation_id {
            manifest.links.push(ChatFileLink {
                id: self.allocate_id(&manifest),
                conversation_id,
                filename: filename.clone(),
            });
        }
        let item = VaultItemEntry {
            id,
            relative_path: PathBuf::from(filename),
            state: VaultItemState::Active,
            archived_links: Vec::new(),
        };
        // Obtain metadata before publication so a failed result cannot strand a new identity.
        let record = match self.item_record(&item, &manifest) {
            Ok(record) => record,
            Err(error) => {
                self.rollback_created_file(&path)?;
                return Err(error);
            }
        };
        manifest.items.push(item);
        if let Err(error) = self.write_manifest(&manifest) {
            self.rollback_created_file(&path)?;
            return Err(error);
        }
        Ok(record)
    }

    /// Atomically replace the same body without changing its identity or chat links.
    pub(crate) fn replace_item(
        &self,
        id: VaultItemId,
        contents: &[u8],
    ) -> Result<VaultItemRecord, ChatFileError> {
        self.replace_item_checked(id, contents, None)
    }

    /// Replace an editor snapshot only if the current plaintext still matches its source.
    pub(crate) fn replace_item_if_unchanged(
        &self,
        id: VaultItemId,
        contents: &[u8],
        expected_plaintext_blake3: blake3::Hash,
    ) -> Result<VaultItemRecord, ChatFileError> {
        self.replace_item_checked(id, contents, Some(expected_plaintext_blake3))
    }

    fn replace_item_checked(
        &self,
        id: VaultItemId,
        contents: &[u8],
        expected_plaintext_blake3: Option<blake3::Hash>,
    ) -> Result<VaultItemRecord, ChatFileError> {
        let _mutation = self.acquire_mutation_lock()?;
        let _lock = GlobalLinkLock::acquire(&self.paths)?;
        self.ensure_saved_root()?;
        let mut manifest = self.load_manifest()?;
        self.prune_stale_links(&mut manifest)?;
        let item = active_item(&manifest, id)?;
        let (parent, name) = self.item_parent(&item.relative_path)?;
        require_regular(&parent, &name, &item.relative_path)?;
        if let Some(expected) = expected_plaintext_blake3 {
            let current = self.read_native_path(&self.saved_root.join(&item.relative_path))?;
            if blake3::hash(&current) != expected {
                return Err(ChatFileError::ItemConflict { id });
            }
        }
        let contents = crate::vault::encode_root(&self.saved_root, contents)?;
        self.writer
            .write_replace_at(&parent, &name, &contents, AtomicWriteTarget::SavedFile)?;
        self.item_record(item, &manifest)
    }

    /// Move the encoded body to private Trash. The protected manifest records recovery intent.
    /// Chat associations are archived with the item and no longer appear in active chat lists.
    pub(crate) fn trash_item(&self, id: VaultItemId) -> Result<(), ChatFileError> {
        let _mutation = self.acquire_mutation_lock()?;
        let _lock = GlobalLinkLock::acquire(&self.paths)?;
        self.ensure_saved_root()?;
        let mut manifest = self.load_manifest()?;
        self.prune_stale_links(&mut manifest)?;
        let item = active_item(&manifest, id)?;
        let (parent, name) = self.item_parent(&item.relative_path)?;
        require_regular(&parent, &name, &item.relative_path)?;
        let trash_path = PathBuf::from(format!(".Trash/{id}-{:032x}.bin", rand::random::<u128>()));
        crate::protection::open_directory(&self.saved_root.join(".Trash"), true)
            .map_err(|error| io::Error::other(error.to_string()))?;
        sync_directory(&self.saved_root)?;
        let item = manifest
            .items
            .iter_mut()
            .find(|item| item.id == id)
            .ok_or(ChatFileError::ItemNotFound { id })?;
        item.state = VaultItemState::Trashing { trash_path };
        self.write_manifest(&manifest)?;
        self.recover_trash(&mut manifest)?;
        self.write_manifest(&manifest)
    }

    fn item_record(
        &self,
        item: &VaultItemEntry,
        manifest: &ChatFileManifest,
    ) -> Result<VaultItemRecord, ChatFileError> {
        let (parent, name) = self.item_parent(&item.relative_path)?;
        let metadata = require_regular(&parent, &name, &item.relative_path)?;
        let filename = name
            .to_str()
            .ok_or(ChatFileError::InvalidFileName)?
            .to_string();
        Ok(VaultItemRecord {
            id: item.id,
            kind: item
                .relative_path
                .extension()
                .and_then(|value| value.to_str())
                .map(str::to_ascii_lowercase)
                .unwrap_or_else(|| "file".to_string()),
            filename,
            path: self.saved_root.join(&item.relative_path),
            relative_path: item.relative_path.clone(),
            size_bytes: metadata.len(),
            chat_links: manifest
                .links
                .iter()
                .filter(|link| Path::new(&link.filename) == item.relative_path)
                .cloned()
                .collect(),
        })
    }

    /// Reconcile under the existing global manifest lock. Missing IDs remain retired forever.
    pub(super) fn reconcile_items(
        &self,
        manifest: &mut ChatFileManifest,
    ) -> Result<(), ChatFileError> {
        let mut changed = manifest.version != MANIFEST_VERSION;
        manifest.version = MANIFEST_VERSION;
        self.require_auxiliary_adoption()?;
        let mut files = self.scan_item_paths()?;
        if self.recover_trash(manifest)? {
            changed = true;
            files = self.scan_item_paths()?;
        }
        let present: HashSet<_> = files.iter().collect();
        for item in &mut manifest.items {
            if matches!(item.state, VaultItemState::Active)
                && !present.contains(&item.relative_path)
            {
                item.state = VaultItemState::Missing;
                changed = true;
            }
        }
        let active: HashSet<_> = manifest
            .items
            .iter()
            .filter(|item| matches!(item.state, VaultItemState::Active))
            .map(|item| item.relative_path.clone())
            .collect();
        for relative_path in files {
            if !active.contains(&relative_path) {
                let id = allocate_item_id(manifest)?;
                manifest.items.push(VaultItemEntry {
                    id,
                    relative_path,
                    state: VaultItemState::Active,
                    archived_links: Vec::new(),
                });
                changed = true;
            }
        }
        let before = manifest.links.len();
        let active: HashSet<_> = manifest
            .items
            .iter()
            .filter(|item| matches!(item.state, VaultItemState::Active))
            .map(|item| &item.relative_path)
            .collect();
        manifest
            .links
            .retain(|link| active.contains(&PathBuf::from(&link.filename)));
        changed |= manifest.links.len() != before;
        if changed {
            self.write_manifest(manifest)?;
        }
        Ok(())
    }

    fn scan_item_paths(&self) -> Result<Vec<PathBuf>, ChatFileError> {
        let mut paths = Vec::new();
        self.scan_native_files(Some(&mut paths))?;
        paths.sort();
        Ok(paths)
    }

    pub(super) fn item_parent(&self, path: &Path) -> Result<(Dir, OsString), ChatFileError> {
        if !item_path_is_valid(path) && !trash_path_is_valid(path) {
            return Err(ChatFileError::InvalidFileName);
        }
        self.saved_parent(path)
    }

    pub(super) fn saved_parent(&self, path: &Path) -> Result<(Dir, OsString), ChatFileError> {
        if !crate::vault::VaultRelativePath::parse(path)
            .is_ok_and(|parsed| parsed.as_path() == path)
        {
            return Err(ChatFileError::InvalidFileName);
        }
        let mut directory = self.open_saved_root()?;
        if let Some(parent) = path.parent() {
            for component in parent.components() {
                let Component::Normal(name) = component else {
                    return Err(ChatFileError::InvalidFileName);
                };
                directory = directory.open_dir_nofollow(name)?;
            }
        }
        let name = path
            .file_name()
            .ok_or(ChatFileError::InvalidFileName)?
            .to_os_string();
        Ok((directory, name))
    }

    fn recover_trash(&self, manifest: &mut ChatFileManifest) -> Result<bool, ChatFileError> {
        let mut changed = false;
        for item in &mut manifest.items {
            let VaultItemState::Trashing { trash_path } = &item.state else {
                continue;
            };
            let (source, source_name) = self.item_parent(&item.relative_path)?;
            let (trash, trash_name) = self.item_parent(trash_path)?;
            let source_exists = regular_exists(&source, &source_name, &item.relative_path)?;
            let trash_exists = regular_exists(&trash, &trash_name, trash_path)?;
            match (source_exists, trash_exists) {
                (true, false) => {
                    move_to_trash(&source, &source_name, &trash, &trash_name)?;
                }
                (false, true) => {}
                (true, true) => {
                    return Err(io::Error::other(
                        "Vault Trash recovery conflict; both source and Trash exist",
                    )
                    .into());
                }
                (false, false) => {
                    return Err(io::Error::other(
                        "Vault Trash recovery cannot find the source or Trash body",
                    )
                    .into());
                }
            }
            sync_handle(&source)?;
            sync_handle(&trash)?;
            let trash_path = trash_path.clone();
            let mut archived = Vec::new();
            manifest.links.retain(|link| {
                if Path::new(&link.filename) == item.relative_path {
                    archived.push(link.clone());
                    false
                } else {
                    true
                }
            });
            item.archived_links = archived;
            item.state = VaultItemState::Trashed { trash_path };
            changed = true;
        }
        Ok(changed)
    }
}

fn active_item(
    manifest: &ChatFileManifest,
    id: VaultItemId,
) -> Result<&VaultItemEntry, ChatFileError> {
    manifest
        .items
        .iter()
        .find(|item| item.id == id && matches!(item.state, VaultItemState::Active))
        .ok_or(ChatFileError::ItemNotFound { id })
}

fn allocate_item_id(manifest: &mut ChatFileManifest) -> Result<VaultItemId, ChatFileError> {
    let id = manifest.next_item_id;
    manifest.next_item_id = id.checked_add(1).ok_or(ChatFileError::ItemIdExhausted)?;
    Ok(VaultItemId(id))
}

pub(super) fn item_path_is_valid(path: &Path) -> bool {
    path.to_str().is_some()
        && crate::vault::VaultRelativePath::parse(path).is_ok_and(|parsed| parsed.as_path() == path)
        && path.components().all(|component| {
            matches!(component, Component::Normal(name) if !name.as_encoded_bytes().starts_with(b"."))
        })
}

pub(super) fn trash_path_is_valid(path: &Path) -> bool {
    let mut components = path.components();
    matches!(components.next(), Some(Component::Normal(name)) if name == ".Trash")
        && matches!(components.next(), Some(Component::Normal(name)) if name.to_str().is_some_and(|name| {
            name.ends_with(".bin") && name.trim_end_matches(".bin").chars().all(|character| character.is_ascii_hexdigit() || character == '-')
        }))
        && components.next().is_none()
}

fn require_regular(
    directory: &Dir,
    name: &std::ffi::OsStr,
    path: &Path,
) -> Result<cap_std::fs::Metadata, ChatFileError> {
    let metadata = directory.symlink_metadata(name)?;
    if !metadata.is_file() {
        return Err(ChatFileError::NotRegularFile {
            path: path.to_path_buf(),
        });
    }
    Ok(metadata)
}

fn regular_exists(
    directory: &Dir,
    name: &std::ffi::OsStr,
    path: &Path,
) -> Result<bool, ChatFileError> {
    match require_regular(directory, name, path) {
        Ok(_) => Ok(true),
        Err(ChatFileError::Io(error)) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn move_to_trash(
    source: &Dir,
    name: &std::ffi::OsStr,
    trash: &Dir,
    trash_name: &std::ffi::OsStr,
) -> Result<(), ChatFileError> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    rustix::fs::renameat_with(
        source,
        name,
        trash,
        trash_name,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(io::Error::from)?;
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        // The private, random destination is checked above; rename never follows a target symlink.
        source.rename(name, trash, trash_name)?;
    }
    Ok(())
}
