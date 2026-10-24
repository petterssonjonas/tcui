use std::path::PathBuf;

use crate::ui::app_tabs::AppTabId;
use crate::ui::artifact_sidebar::infer_kind;
use crate::ui::modals::artifact_viewer::document_viewer::{
    DocumentKind, DocumentSource, DocumentViewerState,
};
use crate::ui::modals::terminal_editor::TerminalEditorState;
use crate::ui::vault::hit_areas::VaultHitAreas;
use crate::ui::vault::tree::{TreeSourceEntry, VaultTreeModel};
use crate::vault::watcher::VaultWatcher;
use crate::vault::{VaultEntryKind, VaultFs, VaultWalkLimits};

use super::reconcile::WorkspaceReconciliation;
use super::registry::VaultWorkspaceError;
use super::VaultMoveInput;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum VaultFocus {
    #[default]
    Tree,
    Viewer,
    Editor,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum VaultContentPane {
    #[default]
    Viewer,
    Editor,
}

#[cfg(test)]
struct WorkspaceDropProbe(std::sync::Arc<std::sync::atomic::AtomicUsize>);

#[cfg(test)]
impl Drop for WorkspaceDropProbe {
    fn drop(&mut self) {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

pub(crate) struct VaultWorkspaceState {
    pub(crate) root: PathBuf,
    pub(super) watcher: VaultWatcher,
    pub(super) vault_fs: VaultFs,
    pub(super) reconciliation: WorkspaceReconciliation,
    pub(crate) tree: VaultTreeModel,
    pub(crate) viewer: DocumentViewerState,
    pub(super) viewer_path: Option<PathBuf>,
    pub(crate) editor: Option<TerminalEditorState>,
    pub(super) editor_path: Option<PathBuf>,
    pub(super) editor_changed_on_disk: bool,
    pub(crate) focus: VaultFocus,
    pub(crate) content_pane: VaultContentPane,
    pub(crate) hit_areas: VaultHitAreas,
    pub(crate) metadata: Option<String>,
    pub(crate) move_input: Option<VaultMoveInput>,
    #[cfg(test)]
    drop_probe: Option<WorkspaceDropProbe>,
}

impl VaultWorkspaceState {
    pub(super) fn construct(tab_id: AppTabId) -> Result<Self, VaultWorkspaceError> {
        let root = crate::protection::internal_vault_root();
        let vault_fs = VaultFs::open(&root)?;
        let watcher = VaultWatcher::new(&root, tab_id)?;
        let lock_notice = crate::vault::require_root(&root).err().map(|error| {
            format!(
                "Vault locked or unavailable: {error}. Use /unlock to unlock protected contents."
            )
        });
        let listing = if lock_notice.is_none() {
            vault_fs.list(None, VaultWalkLimits::default())?
        } else {
            crate::vault::VaultListing {
                entries: Vec::new(),
                limits: Vec::new(),
            }
        };
        let tree_entries = listing
            .entries
            .into_iter()
            .map(|entry| {
                let artifact_kind =
                    (entry.kind == VaultEntryKind::File).then(|| infer_kind(&entry.path, None));
                TreeSourceEntry::new(entry.path, entry.kind, artifact_kind)
            })
            .collect();
        Ok(Self {
            root,
            watcher,
            vault_fs,
            reconciliation: WorkspaceReconciliation::default(),
            tree: VaultTreeModel::new(tree_entries),
            viewer: DocumentViewerState::new(DocumentSource::new(
                DocumentKind::Text,
                Some(
                    lock_notice
                        .clone()
                        .unwrap_or_else(|| "Select a file to preview".to_string()),
                ),
                None,
            )),
            viewer_path: None,
            editor: None,
            editor_path: None,
            editor_changed_on_disk: false,
            focus: VaultFocus::Tree,
            content_pane: VaultContentPane::Viewer,
            hit_areas: VaultHitAreas::default(),
            metadata: lock_notice,
            move_input: None,
            #[cfg(test)]
            drop_probe: None,
        })
    }

    pub(crate) fn vault_fs(&self) -> &VaultFs {
        &self.vault_fs
    }

    #[cfg(test)]
    pub(crate) fn metadata(&self) -> Option<&str> {
        self.metadata.as_deref()
    }

    #[cfg(test)]
    pub(crate) fn set_editor_for_test(&mut self, editor: TerminalEditorState) {
        self.editor = Some(editor);
    }

    #[cfg(test)]
    pub(crate) fn set_editor_path_for_test(&mut self, path: PathBuf) {
        self.editor_path = Some(path);
    }

    #[cfg(test)]
    pub(crate) const fn editor_changed_on_disk_for_test(&self) -> bool {
        self.editor_changed_on_disk
    }

    #[cfg(test)]
    pub(crate) fn viewer_path_for_test(&self) -> Option<&std::path::Path> {
        self.viewer_path.as_deref()
    }

    #[cfg(test)]
    pub(crate) fn viewer_content_for_test(&self) -> Option<&str> {
        self.viewer.content_for_test()
    }

    #[cfg(test)]
    pub(crate) fn inject_watcher_event_for_test(
        &mut self,
        tab_id: AppTabId,
        change: crate::vault::watcher::VaultWatchChange,
        path: &str,
    ) {
        self.watcher
            .enqueue_for_test(crate::vault::watcher::VaultWatchEvent::for_test(
                tab_id,
                change,
                PathBuf::from(path),
            ));
    }

    #[cfg(test)]
    pub(crate) fn set_watcher_drop_counter_for_test(
        &mut self,
        counter: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    ) {
        self.watcher.set_drop_probe_for_test(counter);
    }

    #[cfg(test)]
    pub(crate) fn force_watcher_overflow_for_test(&self) {
        self.watcher.force_overflow_for_test();
    }

    #[cfg(test)]
    pub(crate) fn watcher_overflowed_for_test(&self) -> bool {
        self.watcher.overflowed()
    }

    #[cfg(test)]
    pub(crate) fn set_drop_counter_for_test(
        &mut self,
        counter: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    ) {
        self.drop_probe = Some(WorkspaceDropProbe(counter));
    }
}
