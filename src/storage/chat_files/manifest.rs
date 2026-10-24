use std::collections::HashSet;

use serde::Deserialize;

use super::atomic::{AtomicWriteTarget, AtomicWriter};
use super::model::{
    link_filename_is_valid, ChatFileError, ChatFileLink, ChatFileManifest, VaultItemState,
    MANIFEST_VERSION,
};
use crate::storage::paths::TcuiDataPaths;

pub(super) struct LinkManifest<'a> {
    paths: &'a TcuiDataPaths,
    writer: &'a AtomicWriter,
}

impl<'a> LinkManifest<'a> {
    pub(super) const fn new(paths: &'a TcuiDataPaths, writer: &'a AtomicWriter) -> Self {
        Self { paths, writer }
    }

    pub(super) fn load(&self) -> Result<ChatFileManifest, ChatFileError> {
        crate::protection::require(crate::protection::Domain::Vault)
            .map_err(ChatFileError::from_protection)?;
        let bytes = match crate::protection::read_regular(&self.paths.chat_file_links_manifest) {
            Ok(bytes) => bytes,
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
            {
                return Ok(ChatFileManifest::empty());
            }
            Err(error) => return Err(ChatFileError::from_protection(error)),
        };
        crate::protection::require_native_store_adoption(crate::protection::Domain::Vault, &bytes)
            .map_err(ChatFileError::from_protection)?;
        let bytes = crate::protection::decode(crate::protection::Domain::Vault, &bytes)
            .map_err(ChatFileError::from_protection)?;
        let header = serde_json::from_slice::<VersionHeader>(&bytes)
            .map_err(|_| ChatFileError::CorruptManifest)?;
        if header.version > u64::from(MANIFEST_VERSION) {
            return Err(ChatFileError::UnsupportedManifestVersion {
                version: header.version,
            });
        }
        let manifest = match header.version {
            1 => {
                let legacy = serde_json::from_slice::<LegacyManifest>(&bytes)
                    .map_err(|_| ChatFileError::CorruptManifest)?;
                let mut manifest = ChatFileManifest::empty();
                manifest.version = 1;
                manifest.links = legacy.links;
                manifest
            }
            version if version == u64::from(MANIFEST_VERSION) => {
                serde_json::from_slice::<ChatFileManifest>(&bytes)
                    .map_err(|_| ChatFileError::CorruptManifest)?
            }
            _ => return Err(ChatFileError::CorruptManifest),
        };
        if manifest_is_valid(&manifest) {
            Ok(manifest)
        } else {
            Err(ChatFileError::CorruptManifest)
        }
    }

    pub(super) fn write(&self, manifest: &ChatFileManifest) -> Result<(), ChatFileError> {
        if !manifest_is_valid(manifest) {
            return Err(ChatFileError::CorruptManifest);
        }
        super::adoption::require_path_adoption(&self.paths.chat_file_links_manifest)?;
        let bytes = serde_json::to_vec_pretty(manifest)?;
        let bytes = crate::protection::encode(crate::protection::Domain::Vault, &bytes)
            .map_err(ChatFileError::from_protection)?;
        self.writer.write_replace(
            &self.paths.chat_file_links_manifest,
            &bytes,
            AtomicWriteTarget::Manifest,
        )
    }
}

#[derive(Deserialize)]
struct VersionHeader {
    version: u64,
}

#[derive(Deserialize)]
struct LegacyManifest {
    links: Vec<ChatFileLink>,
}

fn manifest_is_valid(manifest: &ChatFileManifest) -> bool {
    matches!(manifest.version, 1 | MANIFEST_VERSION)
        && manifest_parts_are_valid(manifest.next_item_id, &manifest.links, &manifest.items)
}

pub(super) fn manifest_parts_are_valid(
    next_item_id: u64,
    links: &[ChatFileLink],
    items: &[super::model::VaultItemEntry],
) -> bool {
    if next_item_id == 0 {
        return false;
    }
    let mut link_ids = HashSet::new();
    if !links
        .iter()
        .all(|link| valid_link(link) && link_ids.insert(link.id))
    {
        return false;
    }
    let mut item_ids = HashSet::new();
    let mut active_paths = HashSet::new();
    let mut trash_paths = HashSet::new();
    items.iter().all(|item| {
        item.id.get() > 0
            && item.id.get() < next_item_id
            && item_ids.insert(item.id)
            && super::items::item_path_is_valid(&item.relative_path)
            && (!matches!(
                item.state,
                VaultItemState::Active | VaultItemState::Trashing { .. }
            ) || active_paths.insert(&item.relative_path))
            && match &item.state {
                VaultItemState::Trashing { trash_path }
                | VaultItemState::Trashed { trash_path } => {
                    super::items::trash_path_is_valid(trash_path) && trash_paths.insert(trash_path)
                }
                VaultItemState::Active | VaultItemState::Missing => item.archived_links.is_empty(),
            }
            && item.archived_links.iter().all(|link| {
                valid_link(link) && std::path::Path::new(&link.filename) == item.relative_path
            })
    })
}

fn valid_link(link: &ChatFileLink) -> bool {
    link.id.is_nonzero() && link.conversation_id > 0 && link_filename_is_valid(&link.filename)
}
