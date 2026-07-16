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
mod path;
mod write;

#[cfg(test)]
#[path = "vault_move_execute_test.rs"]
mod vault_move_execute;

#[cfg(test)]
#[path = "vault_move_recovery_test.rs"]
mod vault_move_recovery;

pub use backup::{BackupError, BackupIdError, StandaloneBackupId};
pub(crate) use editor::EditorTargetError;
pub use editor::ValidatedEditorTarget;
pub use fs::{
    VaultEntry, VaultEntryKind, VaultFs, VaultFsError, VaultListing, VaultSearchMatch,
    VaultSearchResult,
};
pub use limits::{VaultLimitKind, VaultLimitReached, VaultSearchLimits, VaultWalkLimits};
pub use path::{VaultPathError, VaultRelativePath};
pub use write::{
    VaultDiff, VaultMutationGuard, VaultMutationLock, VaultProfileRegistration,
    VaultRestoreOutcome, VaultWriteError, VaultWriteOutcome, VaultWriteService,
};
