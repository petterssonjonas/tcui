// allow: SIZE_OK - T04B keeps the envelope codec, lifecycle, retention, and recovery tests together.

#[cfg(test)]
use std::fs;
use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use super::VaultRelativePath;

const BACKUP_VERSION: u32 = 2;
const ENVELOPE_FILE: &str = "envelope.json";
const PAYLOAD_FILE: &str = "original.bin";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StandaloneBackupId(String);

impl StandaloneBackupId {
    pub(crate) fn new() -> Self {
        Self(format!("write-{:032x}", rand::random::<u128>()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for StandaloneBackupId {
    type Err = BackupIdError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let Some(hex) = raw.strip_prefix("write-") else {
            return Err(BackupIdError);
        };
        if hex.len() != 32
            || !hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(BackupIdError);
        }
        Ok(Self(raw.to_owned()))
    }
}

impl Serialize for StandaloneBackupId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for StandaloneBackupId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error(
    "standalone Vault backup ID must be write- followed by 32 lowercase hexadecimal characters"
)]
pub struct BackupIdError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BackupState {
    Prepared,
    Complete,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum BackupOriginal {
    Missing,
    File { len: u64, blake3: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BackupEnvelopeV1 {
    pub(crate) version: u32,
    pub(crate) id: StandaloneBackupId,
    #[serde(with = "relative_path_serde")]
    pub(crate) path: VaultRelativePath,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) state: BackupState,
    pub(crate) original: BackupOriginal,
}

#[derive(Debug)]
pub(crate) struct LoadedBackup {
    pub(crate) envelope: BackupEnvelopeV1,
    pub(crate) contents: Option<Vec<u8>>,
}

#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    #[error("unsupported standalone Vault backup version {version}")]
    UnknownVersion { version: u64 },
    #[error("standalone Vault backup is corrupt: {reason}")]
    Corrupt { reason: &'static str },
    #[error("failed to serialize standalone Vault backup: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("failed to {operation} standalone Vault backup at {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

pub(crate) struct BackupStore {
    root: PathBuf,
}

impl BackupStore {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub(crate) fn create_prepared(
        &self,
        path: &VaultRelativePath,
        original: Option<&[u8]>,
    ) -> Result<StandaloneBackupId, BackupError> {
        self.ensure_root()?;
        let id = StandaloneBackupId::new();
        let record = self.root.join(id.as_str());
        let directory = open_directory(&self.root, false)?;
        rustix::fs::mkdirat(
            &directory,
            id.as_str(),
            rustix::fs::Mode::from_bits_truncate(0o700),
        )
        .map_err(|error| io_error("create", &record, error.into()))?;

        let original_state = match original {
            Some(bytes) => {
                write_new_synced(&record.join(PAYLOAD_FILE), bytes)?;
                BackupOriginal::File {
                    len: u64::try_from(bytes.len()).map_err(|_| BackupError::Corrupt {
                        reason: "payload length overflow",
                    })?,
                    blake3: blake3::hash(bytes).to_hex().to_string(),
                }
            }
            None => BackupOriginal::Missing,
        };
        let envelope = BackupEnvelopeV1 {
            version: BACKUP_VERSION,
            id: id.clone(),
            path: path.clone(),
            created_at: Utc::now(),
            state: BackupState::Prepared,
            original: original_state,
        };
        self.write_new_envelope(&record, &envelope)?;
        sync_directory(&record)?;
        sync_directory(&self.root)?;
        Ok(id)
    }

    pub(crate) fn mark_state(
        &self,
        id: &StandaloneBackupId,
        state: BackupState,
    ) -> Result<(), BackupError> {
        let record = self.root.join(id.as_str());
        let mut loaded = self.load(id)?;
        loaded.envelope.state = state;
        let bytes = serde_json::to_vec(&loaded.envelope)?;
        let temporary = record.join(format!(
            ".{ENVELOPE_FILE}.{:032x}.tmp",
            rand::random::<u128>()
        ));
        write_new_synced(&temporary, &bytes)?;
        let directory = open_directory(&record, false)?;
        directory
            .rename(
                temporary.file_name().ok_or(BackupError::Corrupt {
                    reason: "missing temporary filename",
                })?,
                &directory,
                ENVELOPE_FILE,
            )
            .map_err(|source| io_error("replace envelope in", &record, source))?;
        sync_directory(&record)
    }

    pub(crate) fn load(&self, id: &StandaloneBackupId) -> Result<LoadedBackup, BackupError> {
        let record = self.root.join(id.as_str());
        let envelope_path = record.join(ENVELOPE_FILE);
        let bytes =
            crate::protection::read_native(crate::protection::Domain::Vault, &envelope_path)
                .map_err(|error| {
                    io_error("read", &envelope_path, super::protection_io_error(error))
                })?;
        let version = serde_json::from_slice::<VersionProbe>(&bytes)?.version;
        if version != u64::from(BACKUP_VERSION) {
            return Err(BackupError::UnknownVersion { version });
        }
        let envelope: BackupEnvelopeV1 = serde_json::from_slice(&bytes)?;
        if envelope.id != *id {
            return Err(BackupError::Corrupt {
                reason: "backup ID mismatch",
            });
        }
        let contents = match &envelope.original {
            BackupOriginal::Missing => None,
            BackupOriginal::File { len, blake3 } => {
                let payload_path = record.join(PAYLOAD_FILE);
                let payload =
                    crate::protection::read_native(crate::protection::Domain::Vault, &payload_path)
                        .map_err(|error| {
                            io_error("read", &payload_path, super::protection_io_error(error))
                        })?;
                if u64::try_from(payload.len()).ok() != Some(*len) {
                    return Err(BackupError::Corrupt {
                        reason: "payload length mismatch",
                    });
                }
                if blake3::hash(&payload).to_hex().as_str() != blake3 {
                    return Err(BackupError::Corrupt {
                        reason: "payload digest mismatch",
                    });
                }
                Some(payload)
            }
        };
        Ok(LoadedBackup { envelope, contents })
    }

    pub(crate) fn prune_terminal(&self, retention_days: i64) -> Result<(), BackupError> {
        crate::protection::require(crate::protection::Domain::Vault).map_err(|error| {
            io_error(
                "unlock before pruning",
                &self.root,
                io::Error::other(error.to_string()),
            )
        })?;
        let cutoff = Utc::now() - Duration::days(retention_days);
        let directory = match crate::protection::open_directory(&self.root, false) {
            Ok(directory) => directory,
            Err(error)
                if error
                    .downcast_ref::<io::Error>()
                    .is_some_and(|error| error.kind() == io::ErrorKind::NotFound) =>
            {
                return Ok(())
            }
            Err(error) => {
                return Err(io_error(
                    "list",
                    &self.root,
                    io::Error::other(error.to_string()),
                ))
            }
        };
        for entry in directory
            .entries()
            .map_err(|source| io_error("list", &self.root, source))?
        {
            let entry = entry.map_err(|source| io_error("list", &self.root, source))?;
            if !entry
                .file_type()
                .map_err(|source| io_error("inspect", &self.root.join(entry.file_name()), source))?
                .is_dir()
            {
                continue;
            }
            let record = self.root.join(entry.file_name());
            let envelope_path = record.join(ENVELOPE_FILE);
            let Ok(bytes) =
                crate::protection::read_native(crate::protection::Domain::Vault, &envelope_path)
            else {
                continue;
            };
            let Ok(probe) = serde_json::from_slice::<VersionProbe>(&bytes) else {
                continue;
            };
            if probe.version != u64::from(BACKUP_VERSION) {
                continue;
            }
            let Ok(envelope) = serde_json::from_slice::<BackupEnvelopeV1>(&bytes) else {
                continue;
            };
            if envelope.state == BackupState::Complete && envelope.created_at < cutoff {
                directory
                    .remove_dir_all(entry.file_name())
                    .map_err(|source| io_error("remove", &record, source))?;
            }
        }
        sync_directory(&self.root)
    }

    fn ensure_root(&self) -> Result<(), BackupError> {
        open_directory(&self.root, true).map(|_| ())
    }

    fn write_new_envelope(
        &self,
        record: &Path,
        envelope: &BackupEnvelopeV1,
    ) -> Result<(), BackupError> {
        write_new_synced(&record.join(ENVELOPE_FILE), &serde_json::to_vec(envelope)?)
    }

    #[cfg(test)]
    fn write_envelope_for_test(
        &self,
        envelope: &BackupEnvelopeV1,
        payload: &[u8],
    ) -> Result<PathBuf, BackupError> {
        self.ensure_root()?;
        let record = self.root.join(envelope.id.as_str());
        fs::create_dir(&record).map_err(|source| io_error("create", &record, source))?;
        write_new_synced(&record.join(PAYLOAD_FILE), payload)?;
        self.write_new_envelope(&record, envelope)?;
        Ok(record)
    }
}

#[derive(Deserialize)]
struct VersionProbe {
    version: u64,
}

fn write_new_synced(path: &Path, bytes: &[u8]) -> Result<(), BackupError> {
    let bytes = crate::protection::encode(crate::protection::Domain::Vault, bytes)
        .map_err(|error| io_error("encrypt", path, io::Error::other(error.to_string())))?;
    let parent = path.parent().ok_or(BackupError::Corrupt {
        reason: "missing parent",
    })?;
    let directory = open_directory(parent, false)?;
    let name = path.file_name().ok_or(BackupError::Corrupt {
        reason: "missing filename",
    })?;
    let descriptor = rustix::fs::openat(
        &directory,
        name,
        rustix::fs::OFlags::WRONLY
            | rustix::fs::OFlags::CREATE
            | rustix::fs::OFlags::EXCL
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::from_bits_truncate(0o600),
    )
    .map_err(|error| io_error("create", path, error.into()))?;
    let mut file = File::from(descriptor);
    file.write_all(&bytes)
        .map_err(|source| io_error("write", path, source))?;
    file.sync_all()
        .map_err(|source| io_error("sync", path, source))
}

fn sync_directory(path: &Path) -> Result<(), BackupError> {
    rustix::fs::fsync(open_directory(path, false)?)
        .map_err(|error| io_error("sync", path, error.into()))?;
    Ok(())
}

fn open_directory(path: &Path, create: bool) -> Result<cap_std::fs::Dir, BackupError> {
    crate::protection::open_directory(path, create)
        .map_err(|error| io_error("open directory", path, io::Error::other(error.to_string())))
}

fn io_error(operation: &'static str, path: &Path, source: io::Error) -> BackupError {
    BackupError::Io {
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
        let components = path
            .as_path()
            .components()
            .map(|component| encode_component(component.as_os_str()))
            .collect::<Vec<_>>();
        components.serialize(serializer)
    }

    pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<VaultRelativePath, D::Error>
    where
        D: Deserializer<'de>,
    {
        let components = Vec::<String>::deserialize(deserializer)?;
        let mut path = PathBuf::new();
        for component in components {
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

#[cfg(test)]
mod tests {
    use std::fs;

    use chrono::{Duration, Utc};

    use super::{
        BackupEnvelopeV1, BackupError, BackupOriginal, BackupState, BackupStore, StandaloneBackupId,
    };
    use crate::vault::VaultRelativePath;

    fn test_root(label: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "tcui-vault-backup-{label}-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        fs::create_dir_all(&root).expect("create backup test root");
        root
    }

    fn envelope(state: BackupState, age_days: i64) -> BackupEnvelopeV1 {
        BackupEnvelopeV1 {
            version: super::BACKUP_VERSION,
            id: StandaloneBackupId::new(),
            path: VaultRelativePath::parse(std::path::Path::new("note.md"))
                .expect("valid vault path"),
            created_at: Utc::now() - Duration::days(age_days),
            state,
            original: BackupOriginal::File {
                len: 3,
                blake3: blake3::hash(b"old").to_hex().to_string(),
            },
        }
    }

    #[test]
    fn vault_confined_write_unknown_backup_version_is_rejected_and_retained() {
        let _env_guard = crate::test_support::env_lock()
            .lock()
            .expect("environment lock");
        // Given
        let root = test_root("unknown-version");
        let store = BackupStore::new(root.clone());
        let id = StandaloneBackupId::new();
        let record = root.join(id.as_str());
        fs::create_dir_all(&record).expect("create unknown backup record");
        fs::write(record.join("envelope.json"), br#"{"version":99}"#)
            .expect("write unknown envelope");

        // When
        let result = store.load(&id);
        store
            .prune_terminal(7)
            .expect("prune known terminal records");

        // Then
        assert!(matches!(
            result,
            Err(BackupError::UnknownVersion { version: 99 })
        ));
        assert!(record.exists());
        fs::remove_dir_all(root).expect("remove backup test root");
    }

    #[test]
    fn vault_confined_write_retention_removes_only_old_terminal_backups() {
        let _env_guard = crate::test_support::env_lock()
            .lock()
            .expect("environment lock");
        // Given
        let root = test_root("retention");
        let store = BackupStore::new(root.clone());
        let terminal = envelope(BackupState::Complete, 8);
        let failed = envelope(BackupState::Failed, 8);
        let terminal_path = store
            .write_envelope_for_test(&terminal, b"old")
            .expect("write terminal backup");
        let failed_path = store
            .write_envelope_for_test(&failed, b"old")
            .expect("write failed backup");

        // When
        store.prune_terminal(7).expect("prune terminal backups");

        // Then
        assert!(!terminal_path.exists());
        assert!(failed_path.exists());
        fs::remove_dir_all(root).expect("remove backup test root");
    }

    #[test]
    fn vault_confined_write_backup_id_rejects_path_components() {
        let result = "../../outside".parse::<StandaloneBackupId>();

        assert!(result.is_err());
    }
}
