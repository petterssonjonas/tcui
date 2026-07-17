use std::fmt;
use std::sync::Arc;

use thiserror::Error;

pub const MAX_DOCUMENT_BYTES: usize = 256 * 1024;
pub const MAX_DOCUMENTS_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PendingDocumentId(blake3::Hash);

impl fmt::Debug for PendingDocumentId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("PendingDocumentId")
            .field(&digest_prefix(self.0))
            .finish()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DocumentDigest(blake3::Hash);

impl DocumentDigest {
    #[cfg(test)]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        self.0.as_bytes()
    }
}

impl fmt::Debug for DocumentDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("DocumentDigest")
            .field(&digest_prefix(self.0))
            .finish()
    }
}

#[derive(Clone)]
pub struct PendingDocument {
    id: PendingDocumentId,
    source_identity: blake3::Hash,
    display_name: Arc<str>,
    line_count: usize,
    byte_count: usize,
    digest: DocumentDigest,
    content: Arc<str>,
}

impl PendingDocument {
    pub fn snapshot(
        source_identity: &str,
        display_name: &str,
        bytes: &[u8],
    ) -> Result<Self, PendingDocumentError> {
        if bytes.len() > MAX_DOCUMENT_BYTES {
            return Err(PendingDocumentError::ItemTooLarge);
        }
        let content = std::str::from_utf8(bytes).map_err(|_| PendingDocumentError::NonUtf8)?;
        let source_identity = blake3::hash(source_identity.as_bytes());
        let digest = DocumentDigest(blake3::hash(bytes));
        let mut id_hasher = blake3::Hasher::new();
        id_hasher.update(source_identity.as_bytes());
        id_hasher.update(digest.0.as_bytes());

        Ok(Self {
            id: PendingDocumentId(id_hasher.finalize()),
            source_identity,
            display_name: Arc::from(display_name),
            line_count: content.lines().count(),
            byte_count: bytes.len(),
            digest,
            content: Arc::from(content),
        })
    }

    pub const fn id(&self) -> PendingDocumentId {
        self.id
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    pub const fn line_count(&self) -> usize {
        self.line_count
    }

    #[cfg(test)]
    pub const fn byte_count(&self) -> usize {
        self.byte_count
    }

    #[cfg(test)]
    pub const fn digest(&self) -> DocumentDigest {
        self.digest
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    #[cfg(test)]
    pub fn content_arc(&self) -> Arc<str> {
        Arc::clone(&self.content)
    }
}

impl fmt::Debug for PendingDocument {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PendingDocument")
            .field("id", &self.id)
            .field("display_name", &self.display_name)
            .field("line_count", &self.line_count)
            .field("byte_count", &self.byte_count)
            .field("digest", &self.digest)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
pub enum PendingDocumentError {
    #[error("document is not valid UTF-8")]
    NonUtf8,
    #[error("document exceeds the 256 KiB item limit")]
    ItemTooLarge,
}

#[derive(Clone)]
pub struct PendingDocumentInput {
    pub source_identity: String,
    pub display_name: String,
    pub bytes: Vec<u8>,
}

impl PendingDocumentInput {
    pub(crate) fn snapshot(self) -> Result<PendingDocument, PendingDocumentError> {
        PendingDocument::snapshot(&self.source_identity, &self.display_name, &self.bytes)
    }
}

impl fmt::Debug for PendingDocumentInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PendingDocumentInput")
            .field("display_name", &self.display_name)
            .field("byte_count", &self.bytes.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Default)]
pub struct PendingDocuments {
    items: Vec<PendingDocument>,
    total_bytes: usize,
}

impl PendingDocuments {
    pub fn insert(
        &mut self,
        document: PendingDocument,
    ) -> Result<PendingDocumentId, PendingDocumentsError> {
        if self
            .items
            .iter()
            .any(|existing| existing.source_identity == document.source_identity)
        {
            return Err(PendingDocumentsError::DuplicateSource);
        }
        if self
            .items
            .iter()
            .any(|existing| existing.digest == document.digest)
        {
            return Err(PendingDocumentsError::DuplicateDigest);
        }
        let total_bytes = self
            .total_bytes
            .checked_add(document.byte_count)
            .filter(|total| *total <= MAX_DOCUMENTS_BYTES)
            .ok_or(PendingDocumentsError::AggregateTooLarge)?;
        let id = document.id();
        self.items.push(document);
        self.total_bytes = total_bytes;
        Ok(id)
    }

    pub fn remove(&mut self, id: PendingDocumentId) -> Option<PendingDocument> {
        let index = self.items.iter().position(|document| document.id == id)?;
        let removed = self.items.remove(index);
        self.total_bytes = self.total_bytes.saturating_sub(removed.byte_count);
        Some(removed)
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = &PendingDocument> {
        self.items.iter()
    }

    #[cfg(test)]
    pub const fn len(&self) -> usize {
        self.items.len()
    }

    pub const fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub const fn total_bytes(&self) -> usize {
        self.total_bytes
    }
}

impl fmt::Debug for PendingDocuments {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PendingDocuments")
            .field("items", &self.items)
            .field("total_bytes", &self.total_bytes)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
pub enum PendingDocumentsError {
    #[error("a document from this source is already pending")]
    DuplicateSource,
    #[error("a document with this content is already pending")]
    DuplicateDigest,
    #[error("pending documents exceed the 1 MiB aggregate limit")]
    AggregateTooLarge,
}

fn digest_prefix(digest: blake3::Hash) -> String {
    digest.to_hex()[..12].to_string()
}
