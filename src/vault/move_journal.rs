// allow: SIZE_OK - the versioned envelope, transition graph, and durable codec form one boundary.
#![allow(dead_code)]

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

use crate::config::vault_profiles::VaultProfileId;
use crate::storage::paths::{ensure_directory, set_unix_mode};

use super::backup::StandaloneBackupId;
use super::move_plan::{MoveCollision, MoveFileKind, MovePlan};
use super::VaultRelativePath;

const JOURNAL_VERSION: u32 = 1;
const JOURNAL_FILE: &str = "journal.json";

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub(crate) struct MoveOperationId(String);

impl MoveOperationId {
    pub(crate) fn from_plan(plan: &MovePlan) -> Self {
        Self(plan.id().as_str().to_owned())
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    fn parse(raw: String) -> Result<Self, &'static str> {
        let Some(hex) = raw.strip_prefix("move-") else {
            return Err("move operation ID must start with move-");
        };
        if hex.len() != 32
            || !hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err("move operation ID must end with 32 lowercase hexadecimal characters");
        }
        Ok(Self(raw))
    }
}

impl<'de> Deserialize<'de> for MoveOperationId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Self::parse(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl std::fmt::Display for MoveOperationId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MoveJournalState {
    Planned,
    Staging,
    DestinationsCommitted,
    SourcesRemoving,
    Complete,
    RollingBack,
    RolledBack,
    RecoveryRequired,
}

impl MoveJournalState {
    const fn permits(self, next: Self) -> bool {
        match (self, next) {
            (Self::Planned, Self::Staging | Self::RollingBack | Self::RecoveryRequired)
            | (
                Self::Staging,
                Self::DestinationsCommitted | Self::RollingBack | Self::RecoveryRequired,
            )
            | (
                Self::DestinationsCommitted,
                Self::SourcesRemoving | Self::RollingBack | Self::RecoveryRequired,
            )
            | (
                Self::SourcesRemoving,
                Self::Complete | Self::RollingBack | Self::RecoveryRequired,
            )
            | (Self::RollingBack, Self::RolledBack | Self::RecoveryRequired)
            | (Self::RecoveryRequired, Self::RollingBack) => true,
            (Self::Complete | Self::RolledBack, _)
            | (Self::Planned, _)
            | (Self::Staging, _)
            | (Self::DestinationsCommitted, _)
            | (Self::SourcesRemoving, _)
            | (Self::RollingBack, _)
            | (Self::RecoveryRequired, _) => false,
        }
    }

    const fn blocks_mutation(self) -> bool {
        match self {
            Self::Complete | Self::RolledBack => false,
            Self::Planned
            | Self::Staging
            | Self::DestinationsCommitted
            | Self::SourcesRemoving
            | Self::RollingBack
            | Self::RecoveryRequired => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MoveJournalFileRef {
    pub(crate) profile_id: VaultProfileId,
    #[serde(with = "relative_path_serde")]
    pub(crate) path: VaultRelativePath,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MoveJournalPair {
    pub(crate) source: MoveJournalFileRef,
    pub(crate) destination: MoveJournalFileRef,
    pub(crate) overwrite: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MoveJournalPrecondition {
    pub(crate) size: u64,
    pub(crate) modified_unix_nanos: i128,
    pub(crate) blake3: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MoveJournalCollision {
    Create,
    ExplicitOverwrite,
    UnapprovedCollision,
    InvalidDirectory,
    InvalidSymlink,
    InvalidUnsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MoveJournalPlan {
    pub(crate) plan_id: String,
    pub(crate) pairs: Vec<MoveJournalPair>,
    pub(crate) preconditions: Vec<MoveJournalPrecondition>,
    pub(crate) collisions: Vec<MoveJournalCollision>,
    pub(crate) total_size: u64,
}

impl MoveJournalPlan {
    fn capture(plan: &MovePlan) -> Self {
        let pairs = plan
            .pairs()
            .iter()
            .map(|pair| MoveJournalPair {
                source: MoveJournalFileRef {
                    profile_id: pair.source().profile_id().clone(),
                    path: pair.source().path().clone(),
                },
                destination: MoveJournalFileRef {
                    profile_id: pair.destination().profile_id().clone(),
                    path: pair.destination().path().clone(),
                },
                overwrite: pair.overwrite(),
            })
            .collect();
        let preconditions = plan
            .preconditions()
            .iter()
            .map(|precondition| MoveJournalPrecondition {
                size: precondition.size,
                modified_unix_nanos: system_time_nanos(precondition.modified),
                blake3: precondition.blake3.to_hex().to_string(),
            })
            .collect();
        let collisions = plan
            .collisions()
            .iter()
            .copied()
            .map(MoveJournalCollision::from)
            .collect();
        Self {
            plan_id: plan.id().as_str().to_owned(),
            pairs,
            preconditions,
            collisions,
            total_size: plan.total_size(),
        }
    }
}

impl From<MoveCollision> for MoveJournalCollision {
    fn from(collision: MoveCollision) -> Self {
        match collision {
            MoveCollision::Create => Self::Create,
            MoveCollision::ExplicitOverwrite => Self::ExplicitOverwrite,
            MoveCollision::UnapprovedCollision => Self::UnapprovedCollision,
            MoveCollision::InvalidType(MoveFileKind::Directory) => Self::InvalidDirectory,
            MoveCollision::InvalidType(MoveFileKind::Symlink) => Self::InvalidSymlink,
            MoveCollision::InvalidType(MoveFileKind::Unsupported) => Self::InvalidUnsupported,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MoveJournalArtifact {
    pub(crate) source_backup_id: Option<StandaloneBackupId>,
    pub(crate) destination_backup_id: Option<StandaloneBackupId>,
    #[serde(with = "optional_relative_path_serde")]
    pub(crate) staged_path: Option<VaultRelativePath>,
    pub(crate) destination_committed: bool,
    pub(crate) source_removed: bool,
}

impl MoveJournalArtifact {
    fn pending() -> Self {
        Self {
            source_backup_id: None,
            destination_backup_id: None,
            staged_path: None,
            destination_committed: false,
            source_removed: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MoveJournalV1 {
    pub(crate) version: u32,
    pub(crate) operation_id: MoveOperationId,
    pub(crate) plan: MoveJournalPlan,
    pub(crate) state: MoveJournalState,
    pub(crate) artifacts: Vec<MoveJournalArtifact>,
}

impl MoveJournalV1 {
    pub(crate) fn planned(plan: &MovePlan) -> Self {
        Self {
            version: JOURNAL_VERSION,
            operation_id: MoveOperationId::from_plan(plan),
            plan: MoveJournalPlan::capture(plan),
            state: MoveJournalState::Planned,
            artifacts: vec![MoveJournalArtifact::pending(); plan.file_count()],
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum MoveJournalError {
    #[error("unsupported Vault move journal version {version}")]
    UnknownVersion { version: u64 },
    #[error("Vault move journal requires recovery: {operation_id} is {state:?}")]
    RecoveryPending {
        operation_id: MoveOperationId,
        state: MoveJournalState,
    },
    #[error("invalid Vault move journal transition from {from:?} to {to:?}")]
    InvalidTransition {
        from: MoveJournalState,
        to: MoveJournalState,
    },
    #[error("Vault move journal is corrupt: {reason}")]
    Corrupt { reason: &'static str },
    #[error("failed to serialize Vault move journal: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("failed to {operation} Vault move journal at {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

pub(crate) struct JournalStore {
    root: PathBuf,
}

impl JournalStore {
    pub(crate) fn at_data_root(data_root: &Path) -> Self {
        Self {
            root: data_root.join("vault-ops"),
        }
    }

    pub(crate) fn ensure_mutation_allowed(&self) -> Result<(), MoveJournalError> {
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(source) => return Err(io_error("list", &self.root, source)),
        };
        for entry in entries {
            let entry = entry.map_err(|source| io_error("list", &self.root, source))?;
            let metadata = entry
                .file_type()
                .map_err(|source| io_error("inspect", &entry.path(), source))?;
            if !metadata.is_dir() || metadata.is_symlink() {
                return Err(MoveJournalError::Corrupt {
                    reason: "operation entry is not a directory",
                });
            }
            let journal = self.load_path(&entry.path())?;
            if journal.state.blocks_mutation() {
                return Err(MoveJournalError::RecoveryPending {
                    operation_id: journal.operation_id,
                    state: journal.state,
                });
            }
        }
        Ok(())
    }

    pub(crate) fn create(&self, journal: &MoveJournalV1) -> Result<(), MoveJournalError> {
        self.ensure_root()?;
        let operation_root = self.root.join(journal.operation_id.as_str());
        fs::create_dir(&operation_root)
            .map_err(|source| io_error("create operation", &operation_root, source))?;
        set_unix_mode(&operation_root, 0o700)
            .map_err(|source| io_error("set permissions on", &operation_root, source))?;
        write_new_synced(
            &operation_root.join(JOURNAL_FILE),
            &serde_json::to_vec(journal)?,
        )?;
        sync_directory(&operation_root)?;
        sync_directory(&self.root)
    }

    pub(crate) fn load(
        &self,
        operation_id: &MoveOperationId,
    ) -> Result<MoveJournalV1, MoveJournalError> {
        let journal = self.load_path(&self.root.join(operation_id.as_str()))?;
        if journal.operation_id != *operation_id {
            return Err(MoveJournalError::Corrupt {
                reason: "operation ID mismatch",
            });
        }
        Ok(journal)
    }

    pub(crate) fn persist(&self, journal: &MoveJournalV1) -> Result<(), MoveJournalError> {
        if journal.version != JOURNAL_VERSION {
            return Err(MoveJournalError::UnknownVersion {
                version: u64::from(journal.version),
            });
        }
        let operation_root = self.root.join(journal.operation_id.as_str());
        let temporary = operation_root.join(format!(
            ".{JOURNAL_FILE}.{:032x}.tmp",
            rand::random::<u128>()
        ));
        write_new_synced(&temporary, &serde_json::to_vec(journal)?)?;
        fs::rename(&temporary, operation_root.join(JOURNAL_FILE))
            .map_err(|source| io_error("replace", &operation_root, source))?;
        sync_directory(&operation_root)
    }

    pub(crate) fn transition(
        &self,
        journal: &mut MoveJournalV1,
        next: MoveJournalState,
    ) -> Result<(), MoveJournalError> {
        if !journal.state.permits(next) {
            return Err(MoveJournalError::InvalidTransition {
                from: journal.state,
                to: next,
            });
        }
        let previous = journal.state;
        journal.state = next;
        if let Err(error) = self.persist(journal) {
            journal.state = previous;
            return Err(error);
        }
        Ok(())
    }

    fn ensure_root(&self) -> Result<(), MoveJournalError> {
        ensure_directory(&self.root).map_err(|source| io_error("create", &self.root, source))?;
        set_unix_mode(&self.root, 0o700)
            .map_err(|source| io_error("set permissions on", &self.root, source))
    }

    fn load_path(&self, operation_root: &Path) -> Result<MoveJournalV1, MoveJournalError> {
        let path = operation_root.join(JOURNAL_FILE);
        let bytes = fs::read(&path).map_err(|source| io_error("read", &path, source))?;
        let version = serde_json::from_slice::<VersionProbe>(&bytes)?.version;
        if version != u64::from(JOURNAL_VERSION) {
            return Err(MoveJournalError::UnknownVersion { version });
        }
        let journal: MoveJournalV1 = serde_json::from_slice(&bytes)?;
        let directory_matches = operation_root
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == journal.operation_id.as_str());
        if !directory_matches
            || journal.version != JOURNAL_VERSION
            || journal.plan.plan_id != journal.operation_id.as_str()
            || journal.artifacts.len() != journal.plan.pairs.len()
            || journal.plan.preconditions.len() != journal.plan.pairs.len()
            || journal.plan.collisions.len() != journal.plan.pairs.len()
        {
            return Err(MoveJournalError::Corrupt {
                reason: "journal shape mismatch",
            });
        }
        Ok(journal)
    }
}

#[derive(Deserialize)]
struct VersionProbe {
    version: u64,
}

fn system_time_nanos(time: std::time::SystemTime) -> i128 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => {
            i128::from(duration.as_secs()) * 1_000_000_000 + i128::from(duration.subsec_nanos())
        }
        Err(error) => {
            let duration = error.duration();
            -(i128::from(duration.as_secs()) * 1_000_000_000 + i128::from(duration.subsec_nanos()))
        }
    }
}

fn write_new_synced(path: &Path, bytes: &[u8]) -> Result<(), MoveJournalError> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|source| io_error("create", path, source))?;
    set_unix_mode(path, 0o600).map_err(|source| io_error("set permissions on", path, source))?;
    file.write_all(bytes)
        .map_err(|source| io_error("write", path, source))?;
    file.sync_all()
        .map_err(|source| io_error("sync", path, source))
}

fn sync_directory(path: &Path) -> Result<(), MoveJournalError> {
    #[cfg(unix)]
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| io_error("sync", path, source))?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn io_error(operation: &'static str, path: &Path, source: io::Error) -> MoveJournalError {
    MoveJournalError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    }
}

mod relative_path_serde {
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    use base64::engine::general_purpose::STANDARD_NO_PAD;
    use base64::Engine as _;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use crate::vault::VaultRelativePath;

    pub(super) fn serialize<S>(path: &VaultRelativePath, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        path.as_path()
            .components()
            .map(|component| encode_component(component.as_os_str()))
            .collect::<Vec<_>>()
            .serialize(serializer)
    }

    pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<VaultRelativePath, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut path = PathBuf::new();
        for component in Vec::<String>::deserialize(deserializer)? {
            path.push(decode_component(&component).map_err(serde::de::Error::custom)?);
        }
        VaultRelativePath::parse(Path::new(&path)).map_err(serde::de::Error::custom)
    }

    #[cfg(unix)]
    fn encode_component(component: &std::ffi::OsStr) -> String {
        use std::os::unix::ffi::OsStrExt as _;
        STANDARD_NO_PAD.encode(component.as_bytes())
    }

    #[cfg(unix)]
    fn decode_component(encoded: &str) -> Result<OsString, String> {
        use std::os::unix::ffi::OsStringExt as _;
        STANDARD_NO_PAD
            .decode(encoded)
            .map(OsString::from_vec)
            .map_err(|error| error.to_string())
    }

    #[cfg(windows)]
    fn encode_component(component: &std::ffi::OsStr) -> String {
        use std::os::windows::ffi::OsStrExt as _;
        let bytes = component
            .encode_wide()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        STANDARD_NO_PAD.encode(bytes)
    }

    #[cfg(windows)]
    fn decode_component(encoded: &str) -> Result<OsString, String> {
        use std::os::windows::ffi::OsStringExt as _;
        let bytes = STANDARD_NO_PAD
            .decode(encoded)
            .map_err(|error| error.to_string())?;
        if bytes.len() % 2 != 0 {
            return Err("Windows path component has an odd byte count".to_owned());
        }
        let wide = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        Ok(OsString::from_wide(&wide))
    }
}

mod optional_relative_path_serde {
    use serde::{Deserialize, Deserializer, Serializer};

    use crate::vault::VaultRelativePath;

    use super::relative_path_serde;

    pub(super) fn serialize<S>(
        path: &Option<VaultRelativePath>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match path {
            Some(path) => serializer.serialize_some(&PathRef(path)),
            None => serializer.serialize_none(),
        }
    }

    pub(super) fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<Option<VaultRelativePath>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Option::<PathOwned>::deserialize(deserializer).map(|path| path.map(|owned| owned.0))
    }

    struct PathRef<'a>(&'a VaultRelativePath);

    impl serde::Serialize for PathRef<'_> {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            relative_path_serde::serialize(self.0, serializer)
        }
    }

    struct PathOwned(VaultRelativePath);

    impl<'de> Deserialize<'de> for PathOwned {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            relative_path_serde::deserialize(deserializer).map(Self)
        }
    }
}
