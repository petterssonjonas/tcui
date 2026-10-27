use super::InsertArtifactError;
use crate::app::submission::{ContextReference, PendingDocumentInput};
use crate::app::TuiApp;
use crate::ui::artifact_sidebar::{ArtifactHandle, ArtifactKind};

impl TuiApp {
    pub(super) fn pending_document_input(
        &self,
        handle: &ArtifactHandle,
    ) -> color_eyre::Result<PendingDocumentInput> {
        self.require_artifact_access(handle)?;
        let artifact = self
            .find_artifact(handle)
            .ok_or(InsertArtifactError::Unavailable)?;
        let reference = match handle {
            ArtifactHandle::Memory(id) => Some(ContextReference::Memory(*id)),
            ArtifactHandle::Vault(id) => Some(ContextReference::Vault(id.get())),
            _ => None,
        };
        if reference.is_none()
            && !matches!(artifact.kind, ArtifactKind::Markdown | ArtifactKind::Text)
        {
            return Err(InsertArtifactError::Unsupported.into());
        }
        let bytes = self.artifact_bytes(handle)?;
        if bytes.is_empty() {
            return Err(InsertArtifactError::Empty.into());
        }
        if let Some(reference) = reference {
            return Ok(PendingDocumentInput::Resource { reference, bytes });
        }
        let source_identity = match handle {
            ArtifactHandle::Temporary(id) => format!("temporary:{id}"),
            ArtifactHandle::ChatFile {
                conversation_id,
                id,
            } => format!("chat-file:{conversation_id}:{id}"),
            ArtifactHandle::Media(source) => format!("media:{source}"),
            ArtifactHandle::Memory(_) | ArtifactHandle::Vault(_) => {
                unreachable!("native resource handled above")
            }
        };
        Ok(PendingDocumentInput::Document {
            source_identity,
            display_name: artifact.name,
            bytes,
        })
    }
}
