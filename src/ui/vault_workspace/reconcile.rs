use std::path::Path;

use crate::ui::app_tabs::AppTabId;
use crate::ui::artifact_sidebar::{infer_kind, ArtifactKind};
use crate::ui::modals::artifact_viewer::document_viewer::{
    DocumentKind, DocumentSource, DocumentViewerState,
};
use crate::ui::vault::tree::TreeSourceEntry;
use crate::vault::watcher::{VaultWatchBatch, VaultWatchChange};
use crate::vault::{VaultEntry, VaultEntryKind, VaultRelativePath, VaultWalkLimits};

use super::registry::VaultWorkspaceRegistry;
use super::state::VaultWorkspaceState;

pub(super) const FULL_RESCAN_ENTRIES_PER_TICK: usize = 1_000;

#[derive(Default)]
pub(super) struct WorkspaceReconciliation {
    pending: Option<PendingTreeRescan>,
}

struct PendingTreeRescan {
    remaining: std::vec::IntoIter<VaultEntry>,
    staged: Vec<TreeSourceEntry>,
    overflow_generation: Option<u64>,
}

struct CompletedTreeRescan {
    entries: Vec<TreeSourceEntry>,
    overflow_generation: Option<u64>,
}

impl VaultWorkspaceRegistry {
    pub(crate) fn reconcile_tick(&mut self) -> Vec<String> {
        let mut notices = Vec::new();
        for (tab_id, state) in &mut self.states {
            notices.extend(state.reconcile_tick(*tab_id));
        }
        notices
    }
}

impl VaultWorkspaceState {
    fn reconcile_tick(&mut self, tab_id: AppTabId) -> Vec<String> {
        let batch = self.watcher.drain();
        let mut notices = Vec::new();
        let accepted = batch
            .tab_id()
            .is_none_or(|event_tab_id| event_tab_id == tab_id);
        if accepted {
            let mut rescan = batch.requires_tree_rescan() || batch.requires_full_rescan();
            for (path, change) in batch.paths() {
                self.mark_editor_change(path, *change, &mut notices);
                if batch.is_modified(path) {
                    self.refresh_viewer(path, &mut notices);
                    rescan |= self.shape_changed(path, &mut notices);
                }
            }
            if rescan {
                self.begin_rescan(&batch, &mut notices);
            }
        }
        if let Some(completed) = self.advance_rescan() {
            self.commit_rescan(completed, &mut notices);
        }
        notices
    }

    fn shape_changed(&self, path: &Path, notices: &mut Vec<String>) -> bool {
        let relative = match VaultRelativePath::parse(path) {
            Ok(relative) => relative,
            Err(error) => {
                notices.push(format!("Vault watcher path was rejected: {error}"));
                return true;
            }
        };
        match self.vault_fs.entry_kind(&relative) {
            Ok(kind) => self.tree.entry_kind(path) != kind,
            Err(error) => {
                notices.push(format!("Vault metadata refresh failed: {error}"));
                true
            }
        }
    }

    fn refresh_viewer(&mut self, path: &Path, notices: &mut Vec<String>) {
        if self.viewer_path.as_deref() != Some(path) {
            return;
        }
        let Some(node) = self.tree.node_for_path(path).cloned() else {
            return;
        };
        let source = match (node.entry_kind, node.artifact_kind) {
            (VaultEntryKind::File, Some(ArtifactKind::Markdown | ArtifactKind::Text)) => {
                let Ok(relative) = VaultRelativePath::parse(path) else {
                    return;
                };
                let Ok(content) = self.vault_fs.read_text(&relative) else {
                    return;
                };
                DocumentSource::new(
                    if node.artifact_kind == Some(ArtifactKind::Markdown) {
                        DocumentKind::Markdown
                    } else {
                        DocumentKind::Text
                    },
                    Some(content),
                    None,
                )
            }
            (VaultEntryKind::File, Some(ArtifactKind::Image)) => {
                DocumentSource::new(DocumentKind::Image, None, Some(self.root.join(path)))
            }
            (VaultEntryKind::Directory, _)
            | (VaultEntryKind::File, _)
            | (VaultEntryKind::UnsupportedSymlink | VaultEntryKind::Unsupported, _) => {
                DocumentSource::new(DocumentKind::Text, Some(node.metadata()), None)
            }
        };
        let old_scroll = self.viewer.scroll;
        let line_count = source
            .content
            .as_deref()
            .map_or(0, |content| content.lines().count());
        self.viewer = DocumentViewerState::new(source);
        self.viewer.scroll = old_scroll;
        self.viewer.clamp_scroll(line_count, 1);
        notices.push(format!(
            "{} changed on disk; preview reloaded",
            path.display()
        ));
    }

    fn mark_editor_change(
        &mut self,
        path: &Path,
        change: VaultWatchChange,
        notices: &mut Vec<String>,
    ) {
        if self.editor.is_none() || self.editor_path.as_deref() != Some(path) {
            return;
        }
        self.editor_changed_on_disk = true;
        notices.push(format!(
            "{} changed on disk; editor was left running ({change:?})",
            path.display()
        ));
    }

    fn begin_rescan(&mut self, batch: &VaultWatchBatch, notices: &mut Vec<String>) {
        let listing = match self.vault_fs.list(None, VaultWalkLimits::default()) {
            Ok(listing) => listing,
            Err(error) => {
                notices.push(format!("Vault rescan failed: {error}"));
                return;
            }
        };
        self.reconciliation.pending = Some(PendingTreeRescan {
            remaining: listing.entries.into_iter(),
            staged: Vec::new(),
            overflow_generation: batch
                .requires_full_rescan()
                .then_some(batch.overflow_generation()),
        });
        if batch.requires_full_rescan() {
            notices
                .push("Vault watcher overflow or ambiguous event; full rescan started".to_string());
        }
    }

    fn advance_rescan(&mut self) -> Option<CompletedTreeRescan> {
        let pending = self.reconciliation.pending.as_mut()?;
        pending.advance();
        if !pending.is_complete() {
            return None;
        }
        let pending = self.reconciliation.pending.take()?;
        Some(CompletedTreeRescan {
            entries: pending.staged,
            overflow_generation: pending.overflow_generation,
        })
    }

    fn commit_rescan(&mut self, completed: CompletedTreeRescan, notices: &mut Vec<String>) {
        self.tree.replace_sources(completed.entries);
        if self
            .viewer_path
            .as_deref()
            .is_some_and(|path| self.tree.node_for_path(path).is_none())
        {
            let removed = self.viewer_path.take().unwrap_or_default();
            self.viewer = DocumentViewerState::new(DocumentSource::new(
                DocumentKind::Text,
                Some("Select a file to preview".to_string()),
                None,
            ));
            self.metadata = None;
            notices.push(format!(
                "{} was removed; preview cleared",
                removed.display()
            ));
        }
        if let Some(generation) = completed.overflow_generation {
            self.watcher.clear_overflow_after_success(generation);
        }
    }
}

impl PendingTreeRescan {
    fn advance(&mut self) {
        self.staged.extend(
            self.remaining
                .by_ref()
                .take(FULL_RESCAN_ENTRIES_PER_TICK)
                .map(tree_source),
        );
    }

    fn is_complete(&self) -> bool {
        self.remaining.as_slice().is_empty()
    }
}

fn tree_source(entry: VaultEntry) -> TreeSourceEntry {
    let artifact_kind = (entry.kind == VaultEntryKind::File).then(|| infer_kind(&entry.path, None));
    TreeSourceEntry::new(entry.path, entry.kind, artifact_kind)
}

#[cfg(test)]
#[path = "reconcile_tests.rs"]
mod tests;
