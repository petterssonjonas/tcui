use std::path::Path;

use similar::TextDiff;

use crate::config::vault_profiles::{VaultCatalog, VaultProfile, VaultProfileId};

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
    pub(crate) profile_id: VaultProfileId,
    pub(crate) path: VaultRelativePath,
    #[expect(dead_code, reason = "T12C will wire mutation execution")]
    pub(crate) replacement: String,
    pub(super) original: FileIdentity,
    pub(crate) diff: String,
    pub(super) overwrite_allowed: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct VaultRestorePlan {
    pub(crate) profile_id: VaultProfileId,
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
        catalog: &VaultCatalog,
        profile_id: VaultProfileId,
        path: VaultRelativePath,
        replacement: String,
        overwrite_allowed: bool,
    ) -> Result<Self, VaultMutationPlanError> {
        let profile = profile(catalog, &profile_id)?;
        let current = read_text(profile, &path)?;
        Ok(Self {
            profile_id,
            path,
            replacement: replacement.clone(),
            original: identity(current.as_deref().map(str::as_bytes)),
            diff: unified_diff(current.as_deref().unwrap_or_default(), &replacement),
            overwrite_allowed,
        })
    }
}

impl VaultRestorePlan {
    pub(crate) fn build(
        catalog: &VaultCatalog,
        data_root: &Path,
        profile_id: VaultProfileId,
        path: VaultRelativePath,
        backup_id: StandaloneBackupId,
    ) -> Result<Self, VaultMutationPlanError> {
        let profile = profile(catalog, &profile_id)?;
        let current = read_text(profile, &path)?;
        let loaded = BackupStore::new(data_root.join("vault-backups")).load(&backup_id)?;
        if loaded.envelope.state != BackupState::Complete
            || loaded.envelope.profile_id != profile_id.to_string()
            || loaded.envelope.path != path
        {
            return Err(VaultMutationPlanError::BackupTargetMismatch);
        }
        let backup = identity(loaded.contents.as_deref());
        let replacement = match loaded.contents {
            Some(bytes) => {
                String::from_utf8(bytes).map_err(|_| VaultMutationPlanError::BackupIsNotUtf8)?
            }
            None => String::new(),
        };
        Ok(Self {
            profile_id,
            path,
            backup_id,
            original: identity(current.as_deref().map(str::as_bytes)),
            backup,
            diff: unified_diff(current.as_deref().unwrap_or_default(), &replacement),
        })
    }
}

impl VaultMutationKind {
    pub(crate) fn revalidate(
        &self,
        catalog: &VaultCatalog,
        data_root: &Path,
    ) -> Result<(), VaultMutationPlanError> {
        match self {
            Self::Write(plan) => {
                let actual = read_identity(profile(catalog, &plan.profile_id)?, &plan.path)?;
                (actual == plan.original)
                    .then_some(())
                    .ok_or(VaultMutationPlanError::Stale)
            }
            Self::Restore(plan) => {
                let actual = read_identity(profile(catalog, &plan.profile_id)?, &plan.path)?;
                let loaded =
                    BackupStore::new(data_root.join("vault-backups")).load(&plan.backup_id)?;
                let valid = actual == plan.original
                    && loaded.envelope.state == BackupState::Complete
                    && loaded.envelope.profile_id == plan.profile_id.to_string()
                    && loaded.envelope.path == plan.path
                    && identity(loaded.contents.as_deref()) == plan.backup;
                valid.then_some(()).ok_or(VaultMutationPlanError::Stale)
            }
            Self::Move(plan) => plan
                .revalidate(catalog)
                .map_err(|error| VaultMutationPlanError::Move(error.to_string())),
        }
    }
}

pub(super) fn profile<'a>(
    catalog: &'a VaultCatalog,
    id: &VaultProfileId,
) -> Result<&'a VaultProfile, VaultMutationPlanError> {
    catalog
        .get(id)
        .ok_or_else(|| VaultMutationPlanError::UnknownProfile(id.to_string()))
}

fn read_text(
    profile: &VaultProfile,
    path: &VaultRelativePath,
) -> Result<Option<String>, VaultMutationPlanError> {
    let fs = VaultFs::open_profile(&profile.root)?;
    match fs.read_text(path) {
        Ok(text) => Ok(Some(text)),
        Err(VaultFsError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            Ok(None)
        }
        Err(error) => Err(error.into()),
    }
}

fn read_identity(
    profile: &VaultProfile,
    path: &VaultRelativePath,
) -> Result<FileIdentity, VaultMutationPlanError> {
    Ok(identity(
        read_text(profile, path)?.as_deref().map(str::as_bytes),
    ))
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
    #[error("unknown Vault profile {0}")]
    UnknownProfile(String),
    #[error("restore backup does not match the requested Vault file")]
    BackupTargetMismatch,
    #[error("restore backup is not UTF-8 text")]
    BackupIsNotUtf8,
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
