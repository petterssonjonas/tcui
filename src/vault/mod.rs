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
pub(crate) mod target;
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
#[path = "vault_target_resolution_test.rs"]
mod vault_target_resolution;

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
    VaultDiff, VaultMutationGuard, VaultMutationLock, VaultProfileRegistration,
    VaultRestoreOutcome, VaultWriteError, VaultWriteOutcome, VaultWriteService,
};

/// Protection is attached to the application-owned root, never to external profiles.
pub(crate) fn is_internal_root(root: &std::path::Path) -> bool {
    let internal = crate::protection::internal_vault_root();
    root.starts_with(&internal)
        || std::fs::canonicalize(&internal).is_ok_and(|canonical| root.starts_with(canonical))
}

pub(crate) fn require_root(root: &std::path::Path) -> std::io::Result<()> {
    crate::protection::require_app().map_err(|error| std::io::Error::other(error.to_string()))?;
    if is_internal_root(root) {
        crate::protection::require(crate::protection::Domain::Vault)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
    }
    Ok(())
}

pub(crate) fn decode_root(root: &std::path::Path, bytes: Vec<u8>) -> std::io::Result<Vec<u8>> {
    if is_internal_root(root) {
        crate::protection::decode(crate::protection::Domain::Vault, &bytes)
            .map_err(|error| std::io::Error::other(error.to_string()))
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
        crate::protection::read(crate::protection::Domain::Vault, &absolute)
    } else {
        crate::protection::read_regular(&absolute)
    };
    result.map_err(|error| std::io::Error::other(error.to_string()))
}

/// An explicit destination outside the internal Vault is a plaintext export.
pub(crate) fn write_artifact_path(path: &std::path::Path, bytes: &[u8]) -> color_eyre::Result<()> {
    let absolute = artifact_destination(path)?;
    if is_internal_root(&absolute) {
        crate::protection::write(crate::protection::Domain::Vault, &absolute, bytes)
    } else {
        crate::protection::write_plain(&absolute, bytes)
    }
}

pub(crate) fn create_artifact_path(path: &std::path::Path, bytes: &[u8]) -> color_eyre::Result<()> {
    let absolute = artifact_destination(path)?;
    if is_internal_root(&absolute) {
        crate::protection::write_new(crate::protection::Domain::Vault, &absolute, bytes)
    } else {
        crate::protection::write_new_plain(&absolute, bytes)
    }
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

/// Settle already-authorized transfers under the old policy before changing codecs.
pub(crate) fn prepare_protection_migration(
    config: &crate::config::AppConfig,
) -> color_eyre::Result<()> {
    use color_eyre::eyre::WrapErr;
    let catalog = crate::config::vault_profiles::VaultCatalog {
        profiles: config.vaults.clone(),
        default_id: config.default_vault_id.clone(),
    };
    catalog
        .validate()
        .wrap_err("Resolve the Markdown Vault profile catalog before changing protection")?;
    let paths = crate::storage::paths::TcuiDataPaths::discover();
    let recovery = move_recovery::MoveRecoveryService::at_data_root(
        &catalog,
        &paths.root,
        move_recovery::MoveRecoveryConfig::default(),
    );
    let report = recovery
        .recover_pending()
        .wrap_err("Resolve pending transfers in Markdown Vault before changing protection")?;
    if report.blocked() {
        color_eyre::eyre::bail!("Resolve the remaining recovery conflict in Markdown Vault before changing protection; no protection settings were changed");
    }
    move_journal::JournalStore::at_data_root(&paths.root)
        .ensure_mutation_allowed()
        .wrap_err("Resolve pending transfers in Markdown Vault before changing protection")
}
