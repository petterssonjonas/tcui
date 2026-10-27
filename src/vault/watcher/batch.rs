use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::ui::app_tabs::AppTabId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VaultWatchChange {
    Modify,
    Create,
    Remove,
    Rename,
}

#[derive(Debug)]
pub(crate) struct VaultWatchEvent {
    pub(super) tab_id: AppTabId,
    pub(super) change: VaultWatchChange,
    pub(super) paths: Vec<PathBuf>,
    pub(super) ambiguous: bool,
}

impl VaultWatchEvent {
    #[cfg(test)]
    pub(crate) fn for_test(tab_id: AppTabId, change: VaultWatchChange, path: PathBuf) -> Self {
        Self {
            tab_id,
            change,
            paths: vec![path],
            ambiguous: false,
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct VaultWatchBatch {
    pub(super) tab_id: Option<AppTabId>,
    pub(super) changes: BTreeMap<PathBuf, VaultWatchChange>,
    pub(super) modified: BTreeSet<PathBuf>,
    pub(super) event_count: usize,
    pub(super) full_rescan: bool,
    pub(super) overflow_generation: u64,
}

impl VaultWatchBatch {
    pub(crate) const fn tab_id(&self) -> Option<AppTabId> {
        self.tab_id
    }

    pub(crate) fn paths(&self) -> &BTreeMap<PathBuf, VaultWatchChange> {
        &self.changes
    }

    #[cfg(test)]
    pub(crate) fn change_for(&self, path: &Path) -> Option<VaultWatchChange> {
        self.changes.get(path).copied()
    }

    pub(crate) fn is_modified(&self, path: &Path) -> bool {
        self.modified.contains(path)
    }

    #[cfg(test)]
    pub(crate) const fn event_count(&self) -> usize {
        self.event_count
    }

    pub(crate) fn requires_tree_rescan(&self) -> bool {
        self.changes.values().any(|change| {
            matches!(
                change,
                VaultWatchChange::Create | VaultWatchChange::Remove | VaultWatchChange::Rename
            )
        })
    }

    pub(crate) const fn requires_full_rescan(&self) -> bool {
        self.full_rescan
    }

    pub(crate) const fn overflow_generation(&self) -> u64 {
        self.overflow_generation
    }
}

pub(super) const fn stronger_change(
    current: VaultWatchChange,
    incoming: VaultWatchChange,
) -> VaultWatchChange {
    match (current, incoming) {
        (VaultWatchChange::Rename, _) | (_, VaultWatchChange::Rename) => VaultWatchChange::Rename,
        (VaultWatchChange::Remove, _) | (_, VaultWatchChange::Remove) => VaultWatchChange::Remove,
        (VaultWatchChange::Create, _) | (_, VaultWatchChange::Create) => VaultWatchChange::Create,
        (VaultWatchChange::Modify, VaultWatchChange::Modify) => VaultWatchChange::Modify,
    }
}
