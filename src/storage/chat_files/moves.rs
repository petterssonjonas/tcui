use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::lock::GlobalLinkLock;
use super::model::{VaultItemEntry, VaultItemState};
use super::{ChatFileError, ChatFileLink, ChatSavedFileStore, VaultItemId};

/// The protected move journal owns this metadata snapshot. It contains no file bodies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct NativeMoveSnapshot {
    before_items: Vec<VaultItemEntry>,
    after_items: Vec<VaultItemEntry>,
    before_links: Vec<ChatFileLink>,
    after_links: Vec<ChatFileLink>,
}

impl ChatSavedFileStore {
    /// Caller must hold VaultMutationGuard before this method acquires GlobalLinkLock.
    /// Run before any move changes a body, then store the snapshot in the durable move journal.
    pub(crate) fn prepare_native_moves(
        &self,
        pairs: &[(PathBuf, PathBuf)],
    ) -> Result<NativeMoveSnapshot, ChatFileError> {
        let _lock = GlobalLinkLock::acquire(&self.paths)?;
        self.ensure_saved_root()?;
        let mut manifest = self.load_manifest()?;
        self.prune_stale_links(&mut manifest)?;
        let mut sources = HashMap::new();
        let mut destinations = HashSet::new();
        for (source, destination) in pairs {
            if !super::items::item_path_is_valid(source)
                || !super::items::item_path_is_valid(destination)
                || source == destination
                || sources
                    .insert(source.as_path(), destination.as_path())
                    .is_some()
                || !destinations.insert(destination.as_path())
            {
                return Err(ChatFileError::InvalidFileName);
            }
        }
        let affected = |path: &Path| sources.contains_key(path) || destinations.contains(path);
        let before_items: Vec<_> = manifest
            .items
            .iter()
            .filter(|item| {
                matches!(item.state, VaultItemState::Active) && affected(&item.relative_path)
            })
            .cloned()
            .collect();
        let before_links: Vec<_> = manifest
            .links
            .iter()
            .filter(|link| affected(Path::new(&link.filename)))
            .cloned()
            .collect();
        let mut after_items = before_items.clone();
        for item in &mut after_items {
            if let Some(destination) = sources.get(item.relative_path.as_path()) {
                item.relative_path = destination.to_path_buf();
            } else {
                // The existing destination is backed up by the move journal, not a second live item.
                item.state = VaultItemState::Missing;
            }
        }
        let after_links = before_links
            .iter()
            .filter_map(|link| {
                sources.get(Path::new(&link.filename)).map(|destination| {
                    let mut link = link.clone();
                    link.filename = destination.to_string_lossy().into_owned();
                    link
                })
            })
            .collect();
        Ok(NativeMoveSnapshot {
            before_items,
            after_items,
            before_links,
            after_links,
        })
    }

    /// Caller holds VaultMutationGuard. Run after the disk move and before journal Complete.
    pub(crate) fn commit_native_moves(
        &self,
        snapshot: &NativeMoveSnapshot,
    ) -> Result<(), ChatFileError> {
        self.apply_native_move_snapshot(snapshot, true)
    }

    /// Caller holds VaultMutationGuard. Run after disk rollback and before journal RolledBack.
    pub(crate) fn rollback_native_moves(
        &self,
        snapshot: &NativeMoveSnapshot,
    ) -> Result<(), ChatFileError> {
        self.apply_native_move_snapshot(snapshot, false)
    }

    fn apply_native_move_snapshot(
        &self,
        snapshot: &NativeMoveSnapshot,
        forward: bool,
    ) -> Result<(), ChatFileError> {
        let _lock = GlobalLinkLock::acquire(&self.paths)?;
        self.open_saved_root()?;
        let mut manifest = self.load_manifest()?;
        validate_snapshot(snapshot)?;
        let (before_items, after_items, before_links, after_links) = if forward {
            (
                &snapshot.before_items,
                &snapshot.after_items,
                &snapshot.before_links,
                &snapshot.after_links,
            )
        } else {
            (
                &snapshot.after_items,
                &snapshot.before_items,
                &snapshot.after_links,
                &snapshot.before_links,
            )
        };
        let ids: HashSet<_> = before_items.iter().map(|item| item.id).collect();
        let link_ids: HashSet<_> = snapshot.before_links.iter().map(|link| link.id).collect();
        let current_items: Vec<_> = manifest
            .items
            .iter()
            .filter(|item| ids.contains(&item.id))
            .collect();
        let current_links: Vec<_> = manifest
            .links
            .iter()
            .filter(|link| link_ids.contains(&link.id))
            .collect();
        let matches = |items: &[VaultItemEntry], links: &[ChatFileLink]| {
            current_items.iter().copied().eq(items.iter())
                && current_links.iter().copied().eq(links.iter())
        };
        if matches(after_items, after_links) {
            return Ok(());
        }
        if !matches(before_items, before_links) {
            return Err(std::io::Error::other(
                "Vault move identity metadata conflicts with its journal snapshot",
            )
            .into());
        }
        for item in &mut manifest.items {
            if let Some(replacement) = after_items
                .iter()
                .find(|replacement| replacement.id == item.id)
            {
                item.clone_from(replacement);
            }
        }
        manifest.links.retain(|link| !link_ids.contains(&link.id));
        manifest.links.extend_from_slice(after_links);
        self.write_manifest(&manifest)
    }
}

fn validate_snapshot(snapshot: &NativeMoveSnapshot) -> Result<(), ChatFileError> {
    let before_ids: HashSet<VaultItemId> =
        snapshot.before_items.iter().map(|item| item.id).collect();
    let after_ids: HashSet<VaultItemId> = snapshot.after_items.iter().map(|item| item.id).collect();
    if before_ids != after_ids {
        return Err(ChatFileError::CorruptManifest);
    }
    for (items, links) in [
        (&snapshot.before_items, &snapshot.before_links),
        (&snapshot.after_items, &snapshot.after_links),
    ] {
        let next_item_id = items
            .iter()
            .map(|item| item.id.get())
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(ChatFileError::CorruptManifest)?;
        if !super::manifest::manifest_parts_are_valid(next_item_id, links, items) {
            return Err(ChatFileError::CorruptManifest);
        }
    }
    Ok(())
}
