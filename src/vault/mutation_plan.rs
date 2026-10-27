use std::path::Path;

use similar::TextDiff;

use super::backup::{BackupState, BackupStore, StandaloneBackupId};
use super::move_plan::MovePlan;
use super::{VaultFs, VaultFsError, VaultRelativePath};

#[derive(Debug, Clone)]
pub(crate) enum VaultMutationKind {
    Write(VaultWritePlan),
    Restore(VaultRestorePlan),
    Move(MovePlan),
}

#[derive(Debug, Clone)]
pub(crate) struct VaultWritePlan {
    pub(crate) path: VaultRelativePath,
    pub(crate) replacement: String,
    pub(super) original: FileIdentity,
    pub(crate) diff: String,
    pub(super) overwrite_allowed: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct VaultRestorePlan {
    pub(crate) path: VaultRelativePath,
    pub(crate) backup_id: StandaloneBackupId,
    pub(super) original: FileIdentity,
    backup: FileIdentity,
    pub(crate) diff: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum FileIdentity {
    Missing,
    Present(blake3::Hash),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VaultMutationPreview {
    pub(crate) rows: Vec<String>,
    pub(crate) confirm_enabled: bool,
}

impl VaultWritePlan {
    pub(crate) fn build(
        data_root: &Path,
        path: VaultRelativePath,
        replacement: String,
        overwrite_allowed: bool,
    ) -> Result<Self, VaultMutationPlanError> {
        let current = read_bytes(data_root, &path)?;
        let original = identity(current.as_deref());
        let current = current
            .map(String::from_utf8)
            .transpose()
            .map_err(|source| VaultFsError::InvalidUtf8 {
                path: path.as_path().to_path_buf(),
                source,
            })?;
        let diff = unified_diff(current.as_deref().unwrap_or_default(), &replacement);
        Ok(Self {
            path,
            replacement,
            original,
            diff,
            overwrite_allowed,
        })
    }
}

impl VaultRestorePlan {
    pub(crate) fn build(
        data_root: &Path,
        path: VaultRelativePath,
        backup_id: StandaloneBackupId,
    ) -> Result<Self, VaultMutationPlanError> {
        let current = read_bytes(data_root, &path)?;
        let loaded = BackupStore::new(data_root.join("vault-backups")).load(&backup_id)?;
        if loaded.envelope.state != BackupState::Complete || loaded.envelope.path != path {
            return Err(VaultMutationPlanError::BackupTargetMismatch);
        }
        let backup = identity(loaded.contents.as_deref());
        let replacement = loaded.contents.as_deref().unwrap_or_default();
        let original = current.as_deref().unwrap_or_default();
        let diff = match (
            std::str::from_utf8(original),
            std::str::from_utf8(replacement),
        ) {
            (Ok(original), Ok(replacement)) => unified_diff(original, replacement),
            _ => format!(
                "Binary restore\ncurrent: {} bytes, blake3:{}\nbackup: {} bytes, blake3:{}",
                original.len(),
                blake3::hash(original),
                replacement.len(),
                blake3::hash(replacement)
            ),
        };
        Ok(Self {
            path,
            backup_id,
            original: identity(current.as_deref()),
            backup,
            diff,
        })
    }
}

impl VaultMutationKind {
    pub(crate) fn revalidate(&self, data_root: &Path) -> Result<(), VaultMutationPlanError> {
        let valid = match self {
            Self::Write(plan) => {
                identity(read_bytes(data_root, &plan.path)?.as_deref()) == plan.original
            }
            Self::Restore(plan) => {
                let loaded =
                    BackupStore::new(data_root.join("vault-backups")).load(&plan.backup_id)?;
                identity(read_bytes(data_root, &plan.path)?.as_deref()) == plan.original
                    && loaded.envelope.state == BackupState::Complete
                    && loaded.envelope.path == plan.path
                    && identity(loaded.contents.as_deref()) == plan.backup
            }
            Self::Move(plan) => {
                return plan
                    .revalidate(data_root)
                    .map_err(|error| VaultMutationPlanError::Move(error.to_string()))
            }
        };
        valid.then_some(()).ok_or(VaultMutationPlanError::Stale)
    }
}

fn read_bytes(
    data_root: &Path,
    path: &VaultRelativePath,
) -> Result<Option<Vec<u8>>, VaultMutationPlanError> {
    match VaultFs::open(&data_root.join("vault"))?.read_bytes(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(VaultFsError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            Ok(None)
        }
        Err(error) => Err(error.into()),
    }
}

fn identity(bytes: Option<&[u8]>) -> FileIdentity {
    bytes.map_or(FileIdentity::Missing, |bytes| {
        FileIdentity::Present(blake3::hash(bytes))
    })
}

fn unified_diff(current: &str, replacement: &str) -> String {
    TextDiff::from_lines(current, replacement)
        .unified_diff()
        .context_radius(3)
        .header("current", "proposed")
        .to_string()
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum VaultMutationPlanError {
    #[error("restore backup does not match the requested Vault file")]
    BackupTargetMismatch,
    #[error("Vault mutation plan is stale")]
    Stale,
    #[error("Vault mutation has an invalid or unapproved collision")]
    UnapprovedCollision,
    #[error("Vault move plan is stale or invalid: {0}")]
    Move(String),
    #[error(transparent)]
    FileSystem(#[from] VaultFsError),
    #[error(transparent)]
    Backup(#[from] super::BackupError),
}
