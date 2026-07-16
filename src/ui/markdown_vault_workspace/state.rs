use std::path::PathBuf;

use crate::config::vault_profiles::{VaultCatalog, VaultProfileId};
use crate::ui::artifact_sidebar::ArtifactEntry;
use crate::ui::markdown_vault::footer::MarkdownVaultProfileOption;
use crate::ui::markdown_vault::hit_areas::MarkdownVaultHitAreas;
use crate::ui::markdown_vault::tree::{MarkdownVaultTreeModel, TreeSourceEntry};
use crate::ui::modals::artifact_viewer::document_viewer::{
    DocumentKind, DocumentSource, DocumentViewerState,
};
use crate::ui::modals::terminal_editor::TerminalEditorState;
use crate::vault::{VaultEntryKind, VaultFs, VaultWalkLimits};

use super::registry::MarkdownVaultWorkspaceError;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct MarkdownVaultWatcherPlaceholder;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MarkdownVaultProfileControls {
    pub(crate) selected_profile_id: VaultProfileId,
    pub(crate) expanded: bool,
    pub(crate) selected: usize,
    pub(crate) add_selected: bool,
    pub(crate) current_label: String,
    pub(crate) profiles: Vec<MarkdownVaultProfileOption>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum MarkdownVaultFocus {
    #[default]
    Tree,
    Viewer,
    Editor,
    ProfileControls,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum MarkdownVaultContentPane {
    #[default]
    Viewer,
    Editor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MarkdownVaultWorkspaceStatus {
    Ready,
    RebindConfirmationRequired { profile_id: VaultProfileId },
}

#[cfg(test)]
struct WorkspaceDropProbe(std::sync::Arc<std::sync::atomic::AtomicUsize>);

#[cfg(test)]
impl Drop for WorkspaceDropProbe {
    fn drop(&mut self) {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

pub(crate) struct MarkdownVaultWorkspaceState {
    pub(super) profile_id: VaultProfileId,
    pub(crate) root: PathBuf,
    pub(super) vault_fs: VaultFs,
    pub(crate) tree: MarkdownVaultTreeModel,
    pub(crate) viewer: DocumentViewerState,
    pub(crate) editor: Option<TerminalEditorState>,
    pub(crate) focus: MarkdownVaultFocus,
    pub(crate) content_pane: MarkdownVaultContentPane,
    pub(crate) profile_controls: MarkdownVaultProfileControls,
    pub(crate) hit_areas: MarkdownVaultHitAreas,
    pub(crate) status: MarkdownVaultWorkspaceStatus,
    pub(crate) watcher: MarkdownVaultWatcherPlaceholder,
    pub(crate) metadata: Option<String>,
    pub(super) pending_rebind_profile_id: Option<VaultProfileId>,
    #[cfg(test)]
    drop_probe: Option<WorkspaceDropProbe>,
}

impl MarkdownVaultWorkspaceState {
    pub(super) fn construct(
        catalog: &VaultCatalog,
        profile_id: &VaultProfileId,
    ) -> Result<Self, MarkdownVaultWorkspaceError> {
        catalog.validate()?;
        let profile =
            catalog
                .get(profile_id)
                .ok_or_else(|| MarkdownVaultWorkspaceError::UnknownProfile {
                    profile_id: profile_id.clone(),
                })?;
        if !profile.available || !profile.root.is_dir() {
            return Err(MarkdownVaultWorkspaceError::ProfileUnavailable {
                profile_id: profile_id.clone(),
            });
        }
        let vault_fs = VaultFs::open_profile(&profile.root)?;
        let listing = vault_fs.list(None, VaultWalkLimits::default())?;
        let tree_entries = listing
            .entries
            .into_iter()
            .map(|entry| {
                let artifact_kind = (entry.kind == VaultEntryKind::File).then(|| {
                    ArtifactEntry::vault_file(&profile.root, &profile.root.join(&entry.path)).kind
                });
                TreeSourceEntry::new(entry.path, entry.kind, artifact_kind)
            })
            .collect();
        let mut profiles = catalog
            .profiles
            .values()
            .map(|candidate| MarkdownVaultProfileOption {
                id: candidate.id.clone(),
                label: candidate.display_name.clone(),
                available: candidate.available,
            })
            .collect::<Vec<_>>();
        profiles.sort_by_key(|profile| profile.label.to_lowercase());
        let selected = profiles
            .iter()
            .position(|candidate| candidate.id == *profile_id)
            .unwrap_or_default();
        Ok(Self {
            profile_id: profile_id.clone(),
            root: profile.root.clone(),
            vault_fs,
            tree: MarkdownVaultTreeModel::new(tree_entries),
            viewer: DocumentViewerState::new(DocumentSource::new(
                DocumentKind::Text,
                Some("Select a file to preview".to_string()),
                None,
            )),
            editor: None,
            focus: MarkdownVaultFocus::Tree,
            content_pane: MarkdownVaultContentPane::Viewer,
            profile_controls: MarkdownVaultProfileControls {
                selected_profile_id: profile_id.clone(),
                expanded: false,
                selected,
                add_selected: false,
                current_label: profile.display_name.clone(),
                profiles,
            },
            hit_areas: MarkdownVaultHitAreas::default(),
            status: MarkdownVaultWorkspaceStatus::Ready,
            watcher: MarkdownVaultWatcherPlaceholder,
            metadata: None,
            pending_rebind_profile_id: None,
            #[cfg(test)]
            drop_probe: None,
        })
    }

    pub(crate) fn profile_id(&self) -> &VaultProfileId {
        &self.profile_id
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
    pub(crate) fn set_drop_counter_for_test(
        &mut self,
        counter: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    ) {
        self.drop_probe = Some(WorkspaceDropProbe(counter));
    }
}
