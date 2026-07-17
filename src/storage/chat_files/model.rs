use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub(super) const MANIFEST_VERSION: u32 = 1;
const MAX_NAME_BYTES: usize = 120;

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

    pub(super) const fn is_nonzero(self) -> bool {
        self.0 != 0
    }
}

impl fmt::Display for ChatFileId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ChatFileLink {
    pub(crate) id: ChatFileId,
    pub(crate) conversation_id: i64,
    pub(crate) filename: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ChatFileLinkManifestV1 {
    pub(crate) version: u32,
    pub(crate) links: Vec<ChatFileLink>,
}

impl ChatFileLinkManifestV1 {
    pub(super) const fn empty() -> Self {
        Self {
            version: MANIFEST_VERSION,
            links: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SavedFileRecord {
    pub(crate) filename: String,
    pub(crate) path: PathBuf,
    pub(crate) size_bytes: u64,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ChatFileError {
    #[error("conversation ID must be positive")]
    InvalidConversationId,
    #[error("saved filename is invalid")]
    InvalidFileName,
    #[error("chat files are busy")]
    ChatFilesBusy,
    #[error("chat-file link manifest version {version} is not supported")]
    UnsupportedManifestVersion { version: u64 },
    #[error("chat-file link manifest is corrupt")]
    CorruptManifest,
    #[error("chat file link {id} was not found")]
    NotFound { id: ChatFileId },
    #[error("path is outside the Saved files root")]
    PathOutsideSavedRoot,
    #[error("saved path is not a regular file: {path}")]
    NotRegularFile { path: PathBuf },
    #[error("failed to roll back {operation}: {rollback}")]
    RollbackFailed {
        operation: &'static str,
        #[source]
        rollback: io::Error,
    },
    #[cfg(test)]
    #[error("injected atomic write failure")]
    InjectedAtomicWriteFailure,
    #[cfg(test)]
    #[error("injected conversation purge failure")]
    InjectedPurgeFailure,
    #[cfg(test)]
    #[error("injected tombstone removal failure")]
    InjectedTombstoneRemoveFailure,
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

pub(super) fn sanitize_name(input: &str) -> String {
    let file_name = Path::new(input)
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

pub(super) fn collision_filename(name: &str, suffix: u32) -> String {
    if suffix == 1 {
        return name.to_string();
    }
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("chat-file");
    match path.extension().and_then(|value| value.to_str()) {
        Some(extension) => format!("{stem} ({suffix}).{extension}"),
        None => format!("{stem} ({suffix})"),
    }
}

pub(super) fn link_filename_is_valid(filename: &str) -> bool {
    !filename.is_empty()
        && filename.len() <= MAX_NAME_BYTES + 24
        && Path::new(filename).components().count() == 1
        && Path::new(filename)
            .file_name()
            .and_then(|name| name.to_str())
            == Some(filename)
        && !filename.starts_with('.')
        && filename.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || matches!(character, '.' | '-' | '_' | ' ' | '(' | ')')
        })
}
