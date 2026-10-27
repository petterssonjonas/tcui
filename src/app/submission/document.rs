use std::fmt;
use std::sync::Arc;

use base64::{engine::general_purpose::STANDARD, Engine};
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

/// A user-selected resource whose label is kept with its immutable prompt snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextReference {
    Memory(u64),
    Vault(u64),
}

impl ContextReference {
    pub(super) const fn closing_tag(self) -> &'static str {
        match self {
            Self::Memory(_) => "</Memory>",
            Self::Vault(_) => "</Vault item>",
        }
    }
}

impl fmt::Display for ContextReference {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Memory(id) => write!(formatter, "<Memory #{id}>"),
            Self::Vault(id) => write!(formatter, "<Vault item #{id}>"),
        }
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
    reference: Option<ContextReference>,
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
            reference: None,
        })
    }

    pub fn snapshot_context(
        reference: ContextReference,
        bytes: &[u8],
    ) -> Result<Self, PendingDocumentError> {
        if bytes.len() > MAX_DOCUMENT_BYTES {
            return Err(PendingDocumentError::ItemTooLarge);
        }
        let label = reference.to_string();
        let text = std::str::from_utf8(bytes).ok().filter(|text| {
            !text
                .chars()
                .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
        });
        let mut document = if text.is_some() {
            Self::snapshot(&label, &label, bytes)?
        } else {
            const PREFIX: &str = "Binary file content (base64):\n";
            let encoded_len = bytes.len().div_ceil(3) * 4;
            if PREFIX.len() + encoded_len > MAX_DOCUMENT_BYTES {
                return Err(PendingDocumentError::ItemTooLarge);
            }
            let mut encoded = String::with_capacity(PREFIX.len() + encoded_len);
            encoded.push_str(PREFIX);
            STANDARD.encode_string(bytes, &mut encoded);
            Self::snapshot(&label, &label, encoded.as_bytes())?
        };
        document.reference = Some(reference);
        Ok(document)
    }

    pub const fn context_reference(&self) -> Option<ContextReference> {
        self.reference
    }

    pub(crate) fn display_name_arc(&self) -> Arc<str> {
        Arc::clone(&self.display_name)
    }

    fn prompt_bytes(&self) -> usize {
        self.byte_count
            + self.reference.map_or(0, |reference| {
                self.display_name.len() + reference.closing_tag().len() + 2
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
pub enum PendingDocumentInput {
    Document {
        source_identity: String,
        display_name: String,
        bytes: Vec<u8>,
    },
    Resource {
        reference: ContextReference,
        bytes: Vec<u8>,
    },
}

impl PendingDocumentInput {
    pub(crate) fn snapshot(self) -> Result<PendingDocument, PendingDocumentError> {
        match self {
            Self::Resource { reference, bytes } => {
                PendingDocument::snapshot_context(reference, &bytes)
            }
            Self::Document {
                source_identity,
                display_name,
                bytes,
            } => PendingDocument::snapshot(&source_identity, &display_name, &bytes),
        }
    }
}

impl fmt::Debug for PendingDocumentInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = formatter.debug_struct("PendingDocumentInput");
        match self {
            Self::Document {
                display_name,
                bytes,
                ..
            } => {
                debug
                    .field("display_name", display_name)
                    .field("byte_count", &bytes.len());
            }
            Self::Resource { reference, bytes } => {
                debug
                    .field("reference", reference)
                    .field("byte_count", &bytes.len());
            }
        }
        debug.finish_non_exhaustive()
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
        if self.items.iter().any(|existing| {
            existing.reference.is_none()
                && document.reference.is_none()
                && existing.digest == document.digest
        }) {
            return Err(PendingDocumentsError::DuplicateDigest);
        }
        let total_bytes = self
            .total_bytes
            .checked_add(document.prompt_bytes())
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
        self.total_bytes = self.total_bytes.saturating_sub(removed.prompt_bytes());
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
