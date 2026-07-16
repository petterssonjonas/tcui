mod backup;
mod editor;
mod fs;
mod limits;
pub(crate) mod move_plan;
mod path;
mod write;

pub use backup::{BackupError, BackupIdError, StandaloneBackupId};
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
