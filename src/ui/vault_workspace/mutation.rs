use crate::app::vault_mutation_execute::VaultMutationAffectedPath;
use crate::ui::app_tabs::AppTabId;
use crate::ui::artifact_sidebar::infer_kind;
use crate::ui::modals::artifact_viewer::document_viewer::{
    DocumentKind, DocumentSource, DocumentViewerState,
};
use crate::ui::vault::tree::TreeSourceEntry;
use crate::vault::{VaultEntryKind, VaultWalkLimits};

use super::registry::VaultWorkspaceRegistry;
use super::state::VaultWorkspaceState;

#[derive(Debug, Clone)]
pub(crate) struct VaultMutationTarget {
    tab_id: AppTabId,
    changes: Vec<VaultMutationAffectedPath>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VaultMutationRefreshReport {
    pub(crate) refreshed: usize,
    pub(crate) stale: usize,
}

#[derive(Debug, thiserror::Error)]
#[error(
    "Vault file is open in an editor: {path}",
    path = .path.display()
)]
pub(crate) struct VaultEditorConflict {
    path: crate::vault::VaultRelativePath,
}

impl VaultWorkspaceRegistry {
    pub(crate) fn mutation_targets(
        &self,
        changes: &[VaultMutationAffectedPath],
    ) -> Result<Vec<VaultMutationTarget>, VaultEditorConflict> {
        let mut targets = Vec::new();
        for (tab_id, state) in &self.states {
            let relevant = changes.to_vec();
            if relevant.is_empty() {
                continue;
            }
            if state.editor.is_some() {
                if let Some(conflict) = relevant
                    .iter()
                    .find(|change| state.editor_path.as_deref() == Some(change.path.as_path()))
                {
                    return Err(VaultEditorConflict {
                        path: conflict.path.clone(),
                    });
                }
            }
            targets.push(VaultMutationTarget {
                tab_id: *tab_id,
                changes: relevant,
            });
        }
        Ok(targets)
    }

    pub(crate) fn refresh_mutation_targets(
        &mut self,
        targets: &[VaultMutationTarget],
    ) -> VaultMutationRefreshReport {
        let mut report = VaultMutationRefreshReport {
            refreshed: 0,
            stale: 0,
        };
        for target in targets {
            let Some(state) = self.states.get_mut(&target.tab_id) else {
                report.stale += 1;
                continue;
            };
            state.refresh_after_mutation(&target.changes);
            report.refreshed += 1;
        }
        report
    }
}

impl VaultWorkspaceState {
    fn refresh_after_mutation(&mut self, changes: &[VaultMutationAffectedPath]) {
        let Ok(listing) = self.vault_fs.list(None, VaultWalkLimits::default()) else {
            return;
        };
        let entries = listing
            .entries
            .into_iter()
            .map(|entry| {
                let artifact_kind =
                    (entry.kind == VaultEntryKind::File).then(|| infer_kind(&entry.path, None));
                TreeSourceEntry::new(entry.path, entry.kind, artifact_kind)
            })
            .collect();
        self.tree.replace_sources(entries);
        let Some(path) = self.viewer_path.clone() else {
            return;
        };
        if !changes.iter().any(|change| change.path.as_path() == path) {
            return;
        }
        let Some(node) = self.tree.node_for_path(&path).cloned() else {
            self.viewer_path = None;
            self.viewer = empty_viewer();
            self.metadata = None;
            return;
        };
        if node.entry_kind != VaultEntryKind::File {
            return;
        }
        let Ok(relative) = crate::vault::VaultRelativePath::parse(&path) else {
            return;
        };
        let Ok(content) = self.vault_fs.read_text(&relative) else {
            return;
        };
        let old_scroll = self.viewer.scroll;
        let line_count = content.lines().count();
        let kind =
            if node.artifact_kind == Some(crate::ui::artifact_sidebar::ArtifactKind::Markdown) {
                DocumentKind::Markdown
            } else {
                DocumentKind::Text
            };
        self.viewer = DocumentViewerState::new(DocumentSource::new(kind, Some(content), None));
        self.viewer.scroll = old_scroll;
        self.viewer.clamp_scroll(line_count, 1);
    }
}

fn empty_viewer() -> DocumentViewerState {
    DocumentViewerState::new(DocumentSource::new(
        DocumentKind::Text,
        Some("Select a file to preview".to_string()),
        None,
    ))
}
