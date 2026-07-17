use std::path::{Path, PathBuf};

use super::InsertArtifactError;
use crate::app::submission::PendingDocumentInput;
use crate::app::TuiApp;
use crate::ui::artifact_sidebar::{ArtifactHandle, ArtifactKind};

impl TuiApp {
    pub(super) fn pending_document_input(
        &self,
        handle: &ArtifactHandle,
    ) -> Result<PendingDocumentInput, InsertArtifactError> {
        let artifact = self
            .find_artifact(handle)
            .ok_or(InsertArtifactError::Unavailable)?;
        if !matches!(artifact.kind, ArtifactKind::Markdown | ArtifactKind::Text) {
            return Err(InsertArtifactError::Unsupported);
        }
        let bytes = match handle {
            ArtifactHandle::Temporary(_) => artifact
                .content
                .map(String::into_bytes)
                .ok_or(InsertArtifactError::Unavailable)?,
            ArtifactHandle::ChatFile {
                conversation_id,
                id,
            } => self
                .chat_files
                .open_link(*conversation_id, *id)
                .map_err(|_| InsertArtifactError::Unavailable)?,
            ArtifactHandle::Saved(path) => self.read_saved_artifact(path)?,
            ArtifactHandle::Memory(path) => self.read_memory_artifact(path)?,
            ArtifactHandle::Vault(path) => self.read_vault_artifact(path)?,
            ArtifactHandle::Media(_) => return Err(InsertArtifactError::Unsupported),
        };
        if bytes.is_empty() {
            return Err(InsertArtifactError::Empty);
        }
        Ok(PendingDocumentInput {
            source_identity: source_identity(handle),
            display_name: artifact.name,
            bytes,
        })
    }

    fn read_saved_artifact(&self, path: &Path) -> Result<Vec<u8>, InsertArtifactError> {
        let root = canonical(&self.export_base_dir())?;
        let path = canonical(path)?;
        if !path.starts_with(root) {
            return Err(InsertArtifactError::Unavailable);
        }
        std::fs::read(path).map_err(|_| InsertArtifactError::Unavailable)
    }

    #[cfg(feature = "memory")]
    fn read_memory_artifact(&self, path: &Path) -> Result<Vec<u8>, InsertArtifactError> {
        let store = crate::memory::MemoryStore::open(
            &self.memory_vault_root(),
            &crate::memory::MemoryStore::default_cache_path(),
        )
        .map_err(|_| InsertArtifactError::Unavailable)?;
        store
            .read(path)
            .map(String::into_bytes)
            .map_err(|_| InsertArtifactError::Unavailable)
    }

    #[cfg(not(feature = "memory"))]
    fn read_memory_artifact(&self, _path: &Path) -> Result<Vec<u8>, InsertArtifactError> {
        Err(InsertArtifactError::Unavailable)
    }

    fn read_vault_artifact(&self, path: &Path) -> Result<Vec<u8>, InsertArtifactError> {
        let root = self.configured_document_vault_root()?;
        let canonical_root = canonical(&root)?;
        let target = canonical(&root.join(path))?;
        if !target.starts_with(&canonical_root) {
            return Err(InsertArtifactError::Unavailable);
        }
        crate::obsidian::Vault::new(root)
            .read_file(path)
            .map(String::into_bytes)
            .map_err(|_| InsertArtifactError::Unavailable)
    }

    #[cfg(feature = "memory")]
    fn memory_vault_root(&self) -> PathBuf {
        self.configured_document_vault_root().unwrap_or_else(|_| {
            crate::storage::paths::TcuiDataPaths::discover()
                .root
                .join("vault")
        })
    }

    fn configured_document_vault_root(&self) -> Result<PathBuf, InsertArtifactError> {
        if let Some(vault) = &self.vault {
            return Ok(vault.root.clone());
        }
        let path = self
            .config
            .try_read()
            .ok()
            .and_then(|config| config.vault_path.clone())
            .ok_or(InsertArtifactError::Unavailable)?;
        Ok(crate::app::generated_file::expand_user_path(
            Path::new(path.trim().trim_matches(['"', '\''])),
            dirs::home_dir().as_deref(),
        ))
    }
}

fn canonical(path: &Path) -> Result<PathBuf, InsertArtifactError> {
    std::fs::canonicalize(path).map_err(|_| InsertArtifactError::Unavailable)
}

fn source_identity(handle: &ArtifactHandle) -> String {
    match handle {
        ArtifactHandle::Temporary(id) => format!("temporary:{id}"),
        ArtifactHandle::ChatFile {
            conversation_id,
            id,
        } => format!("chat-file:{conversation_id}:{id}"),
        ArtifactHandle::Media(source) => format!("media:{source}"),
        ArtifactHandle::Saved(path) => format!("saved:{}", path.display()),
        ArtifactHandle::Memory(path) => format!("memory:{}", path.display()),
        ArtifactHandle::Vault(path) => format!("vault:{}", path.display()),
    }
}
