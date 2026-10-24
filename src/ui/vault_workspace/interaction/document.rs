use ratatui::layout::Rect;

use crate::ui::artifact_sidebar::ArtifactKind;
use crate::ui::modals::artifact_viewer::document_viewer::{
    DocumentKind, DocumentSource, DocumentViewerState,
};
use crate::ui::modals::terminal_editor::{CloseRequest, TerminalEditorState};
use crate::ui::vault::hit_areas::VaultMouseAction;
use crate::vault::{VaultEntryKind, VaultRelativePath};

use super::super::VaultInputAction;
use crate::ui::vault_workspace::state::{VaultContentPane, VaultFocus, VaultWorkspaceState};

impl VaultWorkspaceState {
    pub(super) fn activate_selected(&mut self) -> VaultInputAction {
        let Some(node) = self.tree.selected_node().cloned() else {
            return VaultInputAction::None;
        };
        if node.is_directory() {
            self.tree.toggle_selected();
            return VaultInputAction::None;
        }
        self.metadata = Some(format!("{} · {}", node.path.display(), node.metadata()));
        let source = match (node.entry_kind, node.artifact_kind) {
            (VaultEntryKind::File, Some(ArtifactKind::Markdown | ArtifactKind::Text)) => {
                let Ok(path) = VaultRelativePath::parse(&node.path) else {
                    return VaultInputAction::Notice("Invalid vault path".to_string());
                };
                match self.vault_fs.read_text(&path) {
                    Ok(content) => DocumentSource::new(
                        if node.artifact_kind == Some(ArtifactKind::Markdown) {
                            DocumentKind::Markdown
                        } else {
                            DocumentKind::Text
                        },
                        Some(content),
                        None,
                    ),
                    Err(error) => return VaultInputAction::Notice(error.to_string()),
                }
            }
            (VaultEntryKind::File, Some(ArtifactKind::Image)) => {
                DocumentSource::new(DocumentKind::Image, None, Some(self.root.join(&node.path)))
            }
            (VaultEntryKind::Directory, _)
            | (VaultEntryKind::File, _)
            | (VaultEntryKind::UnsupportedSymlink | VaultEntryKind::Unsupported, _) => {
                DocumentSource::new(DocumentKind::Text, self.metadata.clone(), None)
            }
        };
        self.viewer = DocumentViewerState::new(source);
        self.viewer_path = Some(node.path);
        self.focus = VaultFocus::Viewer;
        self.content_pane = VaultContentPane::Viewer;
        VaultInputAction::None
    }

    pub(super) fn open_editor(&mut self) -> VaultInputAction {
        let Some(node) = self.tree.selected_node() else {
            return VaultInputAction::Notice("Select a file to edit".to_string());
        };
        if node.entry_kind == VaultEntryKind::File
            && (!node.is_editable()
                || crate::protection::is_protected(crate::protection::Domain::Vault)
                    .unwrap_or(true))
        {
            return VaultInputAction::Edit(node.path.clone());
        }
        if !node.is_editable() {
            return VaultInputAction::Notice(format!("{} cannot be edited", node.metadata()));
        }
        let path = node.path.clone();
        let Ok(relative) = VaultRelativePath::parse(&path) else {
            return VaultInputAction::Notice("Invalid vault path".to_string());
        };
        let target = match self.vault_fs.validate_editor_target(&self.root, &relative) {
            Ok(target) => target,
            Err(error) => return VaultInputAction::Notice(error.to_string()),
        };
        let area = self
            .hit_areas
            .area(VaultMouseAction::FocusEditor)
            .unwrap_or(Rect::new(0, 0, 80, 24));
        match TerminalEditorState::new(target, &self.root, area) {
            Ok(editor) => {
                self.editor = Some(editor);
                self.editor_path = Some(path);
                self.editor_changed_on_disk = false;
                self.focus = VaultFocus::Editor;
                self.content_pane = VaultContentPane::Editor;
                VaultInputAction::None
            }
            Err(error) => VaultInputAction::Notice(error.to_string()),
        }
    }

    pub(super) fn request_editor_close(&mut self) {
        let Some(editor) = self.editor.as_mut() else {
            return;
        };
        if matches!(editor.request_close(), CloseRequest::Closed) {
            self.editor = None;
            self.editor_path = None;
            self.editor_changed_on_disk = false;
            self.focus = VaultFocus::Viewer;
            self.content_pane = VaultContentPane::Viewer;
        }
    }

    pub(super) fn confirm_editor_close(&mut self) {
        if let Some(editor) = self.editor.as_mut() {
            editor.confirm_close();
        }
        self.editor = None;
        self.editor_path = None;
        self.editor_changed_on_disk = false;
        self.focus = VaultFocus::Viewer;
        self.content_pane = VaultContentPane::Viewer;
    }
}
