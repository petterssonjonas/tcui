use std::fs::File;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::markdown::parse_memory;
use super::metadata::MemoryMetadata;
use super::paths::MemoryPaths;
use super::store::{now_ms, MemoryDocument, MemoryError, MemoryStore};
use crate::protection::{self, Domain};

/// A native memory identity and provenance, without the document body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct MemoryItemRecord {
    pub(crate) id: u64,
    pub(crate) logical_path: PathBuf,
    pub(crate) title: String,
    pub(crate) size_bytes: u64,
    pub(crate) created_at_ms: i64,
    pub(crate) updated_at_ms: i64,
    pub(crate) metadata: Option<MemoryMetadata>,
}

impl From<MemoryDocument> for MemoryItemRecord {
    fn from(document: MemoryDocument) -> Self {
        Self {
            id: document.id,
            logical_path: document.logical_path,
            title: document.title,
            size_bytes: document.markdown.len() as u64,
            created_at_ms: document.created_at_ms,
            updated_at_ms: document.updated_at_ms,
            metadata: document.metadata,
        }
    }
}

/// Checks only framing. External import decoding deliberately does not use this guard.
pub(super) fn require_native_file_policy(path: &Path) -> color_eyre::eyre::Result<()> {
    match protection::read_regular_prefix(path, protection::NATIVE_STORE_ADOPTION_PROBE_LEN) {
        Ok(bytes) => protection::require_native_store_adoption(Domain::Memories, &bytes),
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
        {
            Ok(())
        }
        Err(error) => Err(error),
    }
}

/// Defers unavailable cache I/O to indexing, without ignoring encrypted framing.
/// Index access still uses the strict file guard before opening or replacing it.
pub(super) fn require_native_cache_policy(path: &Path) -> color_eyre::eyre::Result<()> {
    match protection::read_regular_prefix(path, protection::NATIVE_STORE_ADOPTION_PROBE_LEN) {
        Ok(bytes) => protection::require_native_store_adoption(Domain::Memories, &bytes),
        Err(error) if error.downcast_ref::<std::io::Error>().is_some() => Ok(()),
        Err(error) => Err(error),
    }
}

impl MemoryStore {
    #[cfg(test)]
    pub(crate) fn root_path(&self) -> &Path {
        self.paths.root()
    }

    /// Checks persisted framing once, including Trash and any readable cache.
    /// Cache I/O cannot block source operations; indexing reports those errors.
    /// No result is cached: copied files must be detected in an already open store.
    pub(super) fn require_native_policy(&self) -> Result<(), MemoryError> {
        protection::require(Domain::Memories)?;
        let _directories = self.paths.directories()?;
        require_native_cache_policy(&self.database)?;
        for entry in walkdir::WalkDir::new(self.paths.root()).follow_links(false) {
            let entry = entry.map_err(|error| MemoryError::Walk(error.to_string()))?;
            if entry
                .path()
                .extension()
                .and_then(|extension| extension.to_str())
                != Some("tcui-memory")
            {
                continue;
            }
            require_native_file_policy(entry.path())?;
        }
        Ok(())
    }

    pub(crate) fn list_entries(&self) -> Result<Vec<MemoryItemRecord>, MemoryError> {
        let mut records = self
            .active_documents()?
            .into_iter()
            .map(|(_, document)| document.into())
            .collect::<Vec<MemoryItemRecord>>();
        records.sort_by_key(|record| record.id);
        Ok(records)
    }

    fn document_for_entry(&self, id: u64) -> Result<(PathBuf, MemoryDocument), MemoryError> {
        self.require_native_policy()?;
        let path = self.paths.active_document_path(id);
        if id == 0 || !path.try_exists()? {
            return Err(MemoryError::Invalid(format!("memory #{id} is unavailable")));
        }
        let document = self.read_document_at(&path)?;
        if document.id != id {
            return Err(MemoryError::Invalid(format!(
                "memory #{id} has a mismatched stored identity"
            )));
        }
        Ok((path, document))
    }

    pub(crate) fn read_entry(&self, id: u64) -> Result<String, MemoryError> {
        Ok(self.document_for_entry(id)?.1.markdown)
    }

    /// Returns decrypted native JSON for an explicit export. The caller selects
    /// the destination protection policy; Markdown-only exports lose provenance.
    pub(crate) fn export_document(&self, id: u64) -> Result<Vec<u8>, MemoryError> {
        Ok(serde_json::to_vec(&self.document_for_entry(id)?.1)?)
    }

    /// Validates a decrypted source without opening or changing the local store.
    /// Stored user content is not passed through automatic capture secret filters.
    pub(crate) fn validate_import_document(bytes: &[u8]) -> Result<MemoryDocument, MemoryError> {
        let document: MemoryDocument = serde_json::from_slice(bytes)?;
        if document.schema_version != 1 || document.id == 0 {
            return Err(MemoryError::Invalid(
                "unsupported memory schema or zero identity".to_string(),
            ));
        }
        MemoryPaths::validate_logical_path(&document.logical_path)?;
        if let Some(metadata) = &document.metadata {
            metadata.validate()?;
        }
        Ok(document)
    }

    /// Rejects stale editor content without changing the source or its index.
    /// The digest covers the exact Markdown bytes returned by `read_entry`.
    pub(crate) fn replace_entry_if_unchanged(
        &self,
        id: u64,
        markdown: &str,
        expected: blake3::Hash,
    ) -> Result<MemoryItemRecord, MemoryError> {
        let _guard = self.lock_mutations()?;
        super::write::validate_markdown(markdown)?;
        let (path, mut document) = self.document_for_entry(id)?;
        if blake3::hash(document.markdown.as_bytes()) != expected {
            return Err(MemoryError::EntryChanged { id });
        }
        document.title = parse_memory(markdown, &document.title).title;
        document.markdown = markdown.to_string();
        document.updated_at_ms = now_ms();
        self.write_document_at(&path, &document)?;
        self.index_saved_document(&document)?;
        Ok(document.into())
    }

    pub(crate) fn trash_entry(&self, id: u64) -> Result<(), MemoryError> {
        let _guard = self.lock_mutations()?;
        let (source, document) = self.document_for_entry(id)?;
        self.trash_document(&source, &document)?;
        Ok(())
    }

    /// Adds rather than merges. Both identity and logical-name conflicts retain
    /// the existing record and assign the incoming record a free target identity.
    pub(crate) fn import_document(&self, bytes: &[u8]) -> Result<MemoryItemRecord, MemoryError> {
        let mut document = Self::validate_import_document(bytes)?;
        let _guard = self.lock_mutations()?;
        let documents = self.active_documents()?;
        let path_conflicts = documents
            .iter()
            .any(|(_, existing)| existing.logical_path == document.logical_path);
        if path_conflicts
            || documents
                .iter()
                .any(|(_, existing)| existing.id == document.id)
            || self.paths.active_document_path(document.id).try_exists()?
            || self.paths.trash_document_path(document.id).try_exists()?
        {
            document.id = self.allocate_document_id()?;
        }
        if path_conflicts {
            let original = document.logical_path.clone();
            let stem = original
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("memory");
            let mut suffix = 2u64;
            loop {
                let candidate = original.with_file_name(format!("{stem} ({suffix}).md"));
                if !documents
                    .iter()
                    .any(|(_, existing)| existing.logical_path == candidate)
                {
                    document.logical_path = candidate;
                    break;
                }
                suffix = suffix.checked_add(1).ok_or_else(|| {
                    MemoryError::Invalid("memory name space is exhausted".to_string())
                })?;
            }
        }
        self.paths.logical_path(&document.logical_path)?;
        let path = self.paths.active_document_path(document.id);
        if protection::configured()? {
            // A concurrent non-cooperating writer must not be replaced by Add.
            protection::write_new(Domain::Memories, &path, &serde_json::to_vec(&document)?)?;
        } else {
            let key = self.key.as_ref().ok_or_else(|| {
                MemoryError::Invalid("legacy memory key is unavailable".to_string())
            })?;
            let encrypted = crate::storage::crypto::encrypt_serialized(
                key,
                super::store::MEMORY_DOCUMENT_KIND,
                &document,
            )?;
            protection::write_new_plain(&path, encrypted.as_bytes())?;
        }
        self.index_saved_document(&document)?;
        Ok(document.into())
    }

    pub(super) fn index_saved_document(
        &self,
        document: &MemoryDocument,
    ) -> Result<(), MemoryError> {
        self.sync()
            .map_err(|error| MemoryError::SavedButNotIndexed {
                title: document.title.clone(),
                path: document.logical_path.to_string_lossy().into_owned(),
                reason: error.to_string(),
            })
    }

    /// Serializes identity/name allocation and source mutations across processes.
    pub(super) fn lock_mutations(&self) -> Result<File, MemoryError> {
        self.require_native_policy()?;
        let descriptor = rustix::fs::open(
            self.paths.root().with_file_name(".memory-management.lock"),
            rustix::fs::OFlags::RDWR
                | rustix::fs::OFlags::CREATE
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::from_bits_truncate(0o600),
        )
        .map_err(std::io::Error::from)?;
        let file = File::from(descriptor);
        if !file.metadata()?.is_file() {
            return Err(MemoryError::Invalid(
                "memory lock is not a regular file".to_string(),
            ));
        }
        file.try_lock()
            .map_err(|error| MemoryError::Invalid(format!("memory store is busy: {error}")))?;
        Ok(file)
    }
}

#[cfg(test)]
mod tests;
