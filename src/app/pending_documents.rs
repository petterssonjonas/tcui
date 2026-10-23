use thiserror::Error;

mod artifact;

use super::submission::{
    PendingDocumentError, PendingDocumentId, PendingDocumentInput, PendingDocumentsError,
};
use super::{Action, TuiApp};
use crate::ui::artifact_sidebar::ArtifactHandle;

#[derive(Debug, Clone, Copy)]
pub(crate) enum PendingDocumentDiscardAction {
    Quit,
    LoadConversation(i64),
    NewConversation(usize),
    CloseConversation,
    SwitchTab(usize),
    RemoveTab(usize),
}

enum QueuePendingDocumentOutcome {
    Added,
    Duplicate,
}

#[derive(Debug, Error)]
enum InsertArtifactError {
    #[error("document is unavailable")]
    Unavailable,
    #[error("only readable Markdown and text documents can be inserted")]
    Unsupported,
    #[error("empty documents cannot be inserted")]
    Empty,
    #[error(transparent)]
    Snapshot(#[from] PendingDocumentError),
    #[error(transparent)]
    Collection(#[from] PendingDocumentsError),
}

impl TuiApp {
    pub(crate) fn has_pending_documents(&self) -> bool {
        self.pending_documents
            .get(&self.input_context_id())
            .is_some_and(|documents| !documents.is_empty())
    }

    pub(crate) fn queue_pending_document(
        &mut self,
        input: PendingDocumentInput,
    ) -> color_eyre::Result<()> {
        match self.queue_pending_document_inner(input)? {
            QueuePendingDocumentOutcome::Added => Ok(()),
            QueuePendingDocumentOutcome::Duplicate => {
                self.focus_composer_with_notice("Document is already inserted.");
                Ok(())
            }
        }
    }

    pub(crate) fn insert_artifact_to_chat(&mut self, handle: ArtifactHandle) {
        let input = match self.pending_document_input(&handle) {
            Ok(input) => input,
            Err(error) => {
                self.ui
                    .show_toast(format!("Could not insert document: {error}"));
                return;
            }
        };
        match self.queue_pending_document_inner(input) {
            Ok(QueuePendingDocumentOutcome::Added) => self.focus_composer(),
            Ok(QueuePendingDocumentOutcome::Duplicate) => {
                self.focus_composer_with_notice("Document is already inserted.");
            }
            Err(error) => {
                self.ui
                    .show_toast(format!("Could not insert document: {error}"));
            }
        }
    }

    pub(crate) fn remove_pending_document(&mut self, id: PendingDocumentId) {
        let app_tab_id = self.input_context_id();
        let remove_collection =
            self.pending_documents
                .get_mut(&app_tab_id)
                .is_some_and(|documents| {
                    documents.remove(id);
                    documents.is_empty()
                });
        if remove_collection {
            self.pending_documents.remove(&app_tab_id);
        }
        if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
            tab.pending_document_chips.retain(|chip| chip.id() != id);
            tab.pending_document_chip_hit_areas
                .retain(|(chip_id, _)| *chip_id != id);
        }
    }

    pub(crate) fn discard_pending_documents(&mut self) {
        self.pending_documents.remove(&self.input_context_id());
        if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
            tab.pending_document_chips.clear();
            tab.pending_document_chip_hit_areas.clear();
        }
    }

    pub(crate) fn request_pending_document_discard(
        &mut self,
        action: PendingDocumentDiscardAction,
    ) -> bool {
        if !self.has_pending_documents() {
            return false;
        }
        self.pending_document_discard_action = Some(action);
        self.ui.active_modal = Some(crate::ui::Modal::DiscardPendingDocuments);
        true
    }

    pub(crate) fn cancel_pending_document_discard(&mut self) {
        self.pending_document_discard_action = None;
    }

    pub(crate) fn confirm_pending_document_discard(&mut self) -> color_eyre::Result<()> {
        let Some(action) = self.pending_document_discard_action.take() else {
            return Ok(());
        };
        self.discard_pending_documents();
        self.ui.active_modal = None;
        match action {
            PendingDocumentDiscardAction::Quit => {
                if self.ui.terminals.has_running() {
                    self.ui.active_modal = Some(crate::ui::Modal::QuitConfirm);
                } else {
                    self.action_tx.send(Action::Quit).ok();
                }
                Ok(())
            }
            PendingDocumentDiscardAction::LoadConversation(conversation_id) => {
                self.load_conversation(conversation_id)
            }
            PendingDocumentDiscardAction::NewConversation(tab_id) => self.new_conversation(tab_id),
            PendingDocumentDiscardAction::CloseConversation => self.close_active_conversation(),
            PendingDocumentDiscardAction::SwitchTab(tab_id) => {
                self.ui.active_tab = tab_id;
                self.ui.settings_v2 = None;
                self.refresh_visible_selectors();
                Ok(())
            }
            PendingDocumentDiscardAction::RemoveTab(tab_id) => {
                self.ui.remove_tab(tab_id);
                self.refresh_visible_selectors();
                Ok(())
            }
        }
    }

    pub(crate) fn close_active_conversation(&mut self) -> color_eyre::Result<()> {
        if self.ui.focused_ephemeral() {
            return self.close_ephemeral();
        }
        let active_conversation = self
            .ui
            .tabs
            .get(self.ui.active_tab)
            .map(|tab| tab.active_conversation)
            .unwrap_or(0);
        if active_conversation != 0 {
            self.delete_conversation_by_id(active_conversation)
        } else {
            self.ensure_tab_has_active_conversation(self.ui.active_tab)
        }
    }

    fn queue_pending_document_inner(
        &mut self,
        input: PendingDocumentInput,
    ) -> Result<QueuePendingDocumentOutcome, InsertArtifactError> {
        let document = input.snapshot()?;
        let chip = crate::ui::pending_documents::PendingDocumentChip::from_document(&document);
        let app_tab_id = self.input_context_id();
        match self
            .pending_documents
            .entry(app_tab_id)
            .or_default()
            .insert(document)
        {
            Ok(_) => {
                if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
                    tab.pending_document_chips.push(chip);
                }
                Ok(QueuePendingDocumentOutcome::Added)
            }
            Err(
                PendingDocumentsError::DuplicateSource | PendingDocumentsError::DuplicateDigest,
            ) => Ok(QueuePendingDocumentOutcome::Duplicate),
            Err(error) => Err(error.into()),
        }
    }

    fn focus_composer(&mut self) {
        self.ui.artifact_viewer = None;
        self.ui.focus = crate::tui::focus::Focus::Chat;
    }

    fn focus_composer_with_notice(&mut self, notice: &str) {
        self.focus_composer();
        self.ui.show_toast(notice.to_string());
    }
}
