use crate::config::vault_profiles::{VaultCatalog, VaultProfileId};
use crate::ui::modals::artifact_viewer::document_viewer::{
    DocumentKind, DocumentSource, DocumentViewerState,
};
use crate::ui::modals::terminal_editor::TerminalEditorState;
use crate::vault::VaultFs;

use super::registry::MarkdownVaultWorkspaceError;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct MarkdownVaultTreeModelPlaceholder;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct MarkdownVaultWatcherPlaceholder;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MarkdownVaultProfileControls {
    pub(crate) selected_profile_id: VaultProfileId,
    pub(crate) expanded: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum MarkdownVaultFocus {
    #[default]
    Tree,
    Viewer,
    Editor,
    ProfileControls,
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
    profile_id: VaultProfileId,
    vault_fs: VaultFs,
    pub(crate) tree: MarkdownVaultTreeModelPlaceholder,
    pub(crate) viewer: DocumentViewerState,
    pub(crate) editor: Option<TerminalEditorState>,
    pub(crate) focus: MarkdownVaultFocus,
    pub(crate) profile_controls: MarkdownVaultProfileControls,
    pub(crate) status: MarkdownVaultWorkspaceStatus,
    pub(crate) watcher: MarkdownVaultWatcherPlaceholder,
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
        Ok(Self {
            profile_id: profile_id.clone(),
            vault_fs,
            tree: MarkdownVaultTreeModelPlaceholder,
            viewer: DocumentViewerState::new(DocumentSource::new(
                DocumentKind::Markdown,
                None,
                None,
            )),
            editor: None,
            focus: MarkdownVaultFocus::Tree,
            profile_controls: MarkdownVaultProfileControls {
                selected_profile_id: profile_id.clone(),
                expanded: false,
            },
            status: MarkdownVaultWorkspaceStatus::Ready,
            watcher: MarkdownVaultWatcherPlaceholder,
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
