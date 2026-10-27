mod backup;
mod editor;
mod fs;
mod limits;
pub(crate) mod move_execute;
pub(crate) mod move_journal;
pub(crate) mod move_plan;
pub(crate) mod move_recovery;
mod move_recovery_fs;
pub(crate) mod move_rollback;
mod move_rollback_support;
pub(crate) mod mutation_plan;
mod mutation_preview;
mod path;
pub(crate) mod read_tools;
pub(crate) mod watcher;
mod write;

#[cfg(test)]
#[path = "vault_move_execute_test.rs"]
mod vault_move_execute;

#[cfg(test)]
#[path = "vault_move_recovery_test.rs"]
mod vault_move_recovery;

#[cfg(test)]
#[path = "vault_read_only_test.rs"]
mod vault_read_only;

#[cfg(test)]
mod protection_tests;

pub use backup::{BackupError, BackupIdError, StandaloneBackupId};
pub(crate) use editor::EditorTargetError;
pub use editor::ValidatedEditorTarget;
pub use fs::{
    VaultEntry, VaultEntryKind, VaultFs, VaultFsError, VaultListing, VaultSearchMatch,
    VaultSearchResult, VaultTextRead,
};
pub use limits::{VaultLimitKind, VaultLimitReached, VaultSearchLimits, VaultWalkLimits};
pub use path::{VaultPathError, VaultRelativePath};
pub use write::{
    VaultDiff, VaultMutationGuard, VaultMutationLock, VaultRestoreOutcome, VaultWriteError,
    VaultWriteOutcome, VaultWriteService,
};

/// Only the application-owned Vault root and its descendants are managed.
pub(crate) fn is_internal_root(root: &std::path::Path) -> bool {
    root.starts_with(crate::protection::internal_vault_root())
        && !root
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
}

pub(crate) fn require_root(root: &std::path::Path) -> std::io::Result<()> {
    crate::protection::require_app().map_err(|error| std::io::Error::other(error.to_string()))?;
    #[cfg(test)]
    if !is_internal_root(root) {
        return Ok(());
    }
    if !is_internal_root(root)
        || root
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "Vault access is limited to the internal data root",
        ));
    }
    crate::protection::require(crate::protection::Domain::Vault)
        .map_err(|error| std::io::Error::other(error.to_string()))
}

pub(crate) fn require_native_bytes(bytes: &[u8]) -> std::io::Result<()> {
    crate::protection::require_native_store_adoption(crate::protection::Domain::Vault, bytes)
        .map_err(protection_io_error)
}

fn protection_io_error(error: color_eyre::Report) -> std::io::Error {
    if let Some(error) = error.downcast_ref::<crate::protection::NativeStoreAdoptionNeeded>() {
        return std::io::Error::other(*error);
    }
    if let Some(error) = error.downcast_ref::<crate::protection::PortableUnlockNeeded>() {
        return std::io::Error::other(*error);
    }
    std::io::Error::other(error.to_string())
}

pub(crate) fn decode_root(root: &std::path::Path, bytes: Vec<u8>) -> std::io::Result<Vec<u8>> {
    if is_internal_root(root) {
        require_native_bytes(&bytes)?;
        crate::protection::decode(crate::protection::Domain::Vault, &bytes)
            .map_err(protection_io_error)
    } else {
        Ok(bytes)
    }
}

pub(crate) fn encode_root<'a>(
    root: &std::path::Path,
    bytes: &'a [u8],
) -> std::io::Result<std::borrow::Cow<'a, [u8]>> {
    if is_internal_root(root) {
        crate::protection::encode(crate::protection::Domain::Vault, bytes)
            .map(std::borrow::Cow::Owned)
            .map_err(|error| std::io::Error::other(error.to_string()))
    } else {
        Ok(std::borrow::Cow::Borrowed(bytes))
    }
}

/// Read UI/media references without exposing ciphertext or staging plaintext.
pub(crate) fn read_artifact_path(path: &std::path::Path) -> std::io::Result<Vec<u8>> {
    crate::protection::require_app().map_err(|error| std::io::Error::other(error.to_string()))?;
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    #[cfg(feature = "memory")]
    {
        let paths = crate::storage::paths::TcuiDataPaths::discover();
        if absolute.starts_with(&paths.memories_dir)
            || absolute.starts_with(&paths.memories_trash_dir)
        {
            return crate::memory::MemoryStore::open_default()
                .and_then(|store| store.read_document_at(&absolute))
                .map(|document| document.markdown.into_bytes())
                .map_err(|error| std::io::Error::other(error.to_string()));
        }
    }
    let result = if is_internal_root(&absolute) {
        crate::protection::read_native(crate::protection::Domain::Vault, &absolute)
    } else {
        crate::protection::read_regular(&absolute)
    };
    result.map_err(protection_io_error)
}

/// An explicit destination outside the internal Vault is a plaintext export.
pub(crate) fn write_artifact_path(path: &std::path::Path, bytes: &[u8]) -> color_eyre::Result<()> {
    let absolute = artifact_destination(path)?;
    if is_internal_root(&absolute) {
        let _guard = artifact_mutation_guard()?;
        crate::protection::write(crate::protection::Domain::Vault, &absolute, bytes)
    } else {
        crate::protection::write_plain(&absolute, bytes)
    }
}

pub(crate) fn create_artifact_path(path: &std::path::Path, bytes: &[u8]) -> color_eyre::Result<()> {
    let absolute = artifact_destination(path)?;
    if is_internal_root(&absolute) {
        let _guard = artifact_mutation_guard()?;
        crate::protection::write_new(crate::protection::Domain::Vault, &absolute, bytes)
    } else {
        crate::protection::write_new_plain(&absolute, bytes)
    }
}

fn artifact_mutation_guard() -> color_eyre::Result<VaultMutationGuard> {
    let paths = crate::storage::paths::TcuiDataPaths::discover();
    let guard = VaultMutationLock::at_data_root(&paths.root).try_acquire()?;
    crate::storage::chat_files::ChatSavedFileStore::discover().require_native_adoption()?;
    move_journal::JournalStore::at_data_root(&paths.root).ensure_mutation_allowed()?;
    Ok(guard)
}

fn artifact_destination(path: &std::path::Path) -> color_eyre::Result<std::path::PathBuf> {
    crate::protection::require_app()?;
    use std::path::Component;
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        color_eyre::eyre::bail!("Export path cannot contain parent traversal");
    }
    Ok(if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    })
}

/// Settle native transfers before changing codecs. Legacy external journals are never replayed.
pub(crate) fn prepare_protection_migration() -> color_eyre::Result<()> {
    use color_eyre::eyre::WrapErr;
    let paths = crate::storage::paths::TcuiDataPaths::discover();
    let recovery = move_recovery::MoveRecoveryService::at_data_root(
        &paths.root,
        move_recovery::MoveRecoveryConfig::default(),
    );
    let report = recovery
        .recover_pending()
        .wrap_err("Resolve pending Vault transfers")?;
    if report.blocked() {
        color_eyre::eyre::bail!(
            "Resolve the remaining Vault recovery conflict before changing protection"
        );
    }
    move_journal::JournalStore::at_data_root(&paths.root)
        .ensure_mutation_allowed()
        .wrap_err("Resolve pending Vault transfers before changing protection")
}

/// Preserve only identity-verified private stages while an authenticated native store is rewrapped.
pub(crate) fn protection_migration_stage_paths() -> color_eyre::Result<Vec<std::path::PathBuf>> {
    protection_stage_paths(false)
}

/// A local format upgrade must not decrypt or authenticate already-portable foreign journals.
pub(crate) fn protection_format_upgrade_stage_paths() -> color_eyre::Result<Vec<std::path::PathBuf>>
{
    protection_stage_paths(true)
}

fn protection_stage_paths(
    skip_portable_journals: bool,
) -> color_eyre::Result<Vec<std::path::PathBuf>> {
    let paths = crate::storage::paths::TcuiDataPaths::discover();
    let mut stages = move_journal::JournalStore::at_data_root(&paths.root)
        .protection_migration_stages(&paths.root, skip_portable_journals)?
        .into_iter()
        .collect::<Vec<_>>();
    stages.sort();
    Ok(stages)
}
