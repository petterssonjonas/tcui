use std::fmt;
use std::io;

use serde::{Deserialize, Serialize};

pub(super) const MANIFEST_VERSION: u32 = 1;
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_CONVERSATION_BYTES: u64 = 256 * 1024 * 1024;
const MAX_NAME_BYTES: usize = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct StoreLimits {
    pub(super) per_file: u64,
    pub(super) per_conversation: u64,
}

impl Default for StoreLimits {
    fn default() -> Self {
        Self {
            per_file: MAX_FILE_BYTES,
            per_conversation: MAX_CONVERSATION_BYTES,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct ChatFileId(u64);

impl ChatFileId {
    pub(super) fn random() -> Self {
        loop {
            let value = rand::random::<u64>();
            if value != 0 {
                return Self(value);
            }
        }
    }

    pub(super) fn parse(value: &str) -> Option<Self> {
        value
            .parse::<u64>()
            .ok()
            .filter(|value| *value != 0)
            .map(Self)
    }
}

impl fmt::Display for ChatFileId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ChatFileRecord {
    pub(crate) id: ChatFileId,
    pub(crate) name: String,
    pub(crate) size_bytes: u64,
}

impl ChatFileRecord {
    pub(crate) fn payload_name(&self) -> String {
        format!("{}--{}", self.id, self.name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ChatFileManifestV1 {
    pub(crate) version: u32,
    pub(crate) conversation_id: i64,
    pub(crate) files: Vec<ChatFileRecord>,
}

impl ChatFileManifestV1 {
    pub(super) fn empty(conversation_id: i64) -> Self {
        Self {
            version: MANIFEST_VERSION,
            conversation_id,
            files: Vec::new(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ChatFileError {
    #[error("conversation ID must be positive")]
    InvalidConversationId,
    #[error("chat file size {size} exceeds the per-file limit {limit}")]
    FileQuotaExceeded { size: u64, limit: u64 },
    #[error("conversation chat-file size {size} exceeds the limit {limit}")]
    ConversationQuotaExceeded { size: u64, limit: u64 },
    #[error("chat-file size arithmetic overflowed")]
    SizeOverflow,
    #[error("chat files are busy")]
    ChatFilesBusy,
    #[error("chat-file manifest version {version} is not supported")]
    UnsupportedManifestVersion { version: u64 },
    #[error("chat file {id} was not found")]
    NotFound { id: ChatFileId },
    #[error("chat file {id} changed outside its manifest")]
    PayloadChanged { id: ChatFileId },
    #[error("chat-file payload set contains duplicate ID {id}")]
    DuplicatePayloadId { id: ChatFileId },
    #[error("failed to roll back {operation}: {rollback}")]
    RollbackFailed {
        operation: &'static str,
        #[source]
        rollback: io::Error,
    },
    #[cfg(test)]
    #[error("injected atomic write failure")]
    InjectedAtomicWriteFailure,
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Serialization(#[from] serde_json::Error),
}

pub(super) fn validate_conversation_id(conversation_id: i64) -> Result<(), ChatFileError> {
    if conversation_id <= 0 {
        return Err(ChatFileError::InvalidConversationId);
    }
    Ok(())
}

pub(super) fn payload_size(contents: &[u8], limit: u64) -> Result<u64, ChatFileError> {
    let size = u64::try_from(contents.len()).map_err(|_| ChatFileError::SizeOverflow)?;
    if size > limit {
        return Err(ChatFileError::FileQuotaExceeded { size, limit });
    }
    Ok(size)
}

pub(super) fn enforce_conversation_quota(
    files: &[ChatFileRecord],
    added_size: u64,
    limits: StoreLimits,
) -> Result<(), ChatFileError> {
    let current_size = files.iter().try_fold(0_u64, |total, record| {
        total
            .checked_add(record.size_bytes)
            .ok_or(ChatFileError::SizeOverflow)
    })?;
    let size = current_size
        .checked_add(added_size)
        .ok_or(ChatFileError::SizeOverflow)?;
    if size > limits.per_conversation {
        return Err(ChatFileError::ConversationQuotaExceeded {
            size,
            limit: limits.per_conversation,
        });
    }
    Ok(())
}

pub(super) fn sanitize_name(input: &str) -> String {
    let file_name = std::path::Path::new(input)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("chat-file");
    let mut output = String::with_capacity(file_name.len().min(MAX_NAME_BYTES));
    let mut separator = false;

    for character in file_name.chars() {
        if output.len() >= MAX_NAME_BYTES {
            break;
        }
        if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
            output.push(character);
            separator = false;
        } else if !separator && !output.is_empty() {
            output.push('-');
            separator = true;
        }
    }

    let sanitized = output.trim_matches(['.', '-', '_']);
    if sanitized.is_empty() {
        "chat-file".to_string()
    } else {
        sanitized.to_string()
    }
}

pub(super) fn record_is_structurally_valid(record: &ChatFileRecord, limits: StoreLimits) -> bool {
    record.id.0 != 0
        && record.name == sanitize_name(&record.name)
        && record.size_bytes <= limits.per_file
}
