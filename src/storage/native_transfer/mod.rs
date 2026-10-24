//! Explicit native Vault and Memories import, export, and recovery.
//!
//! UI sequence: prepare, unlock each requested source, prepare again, enable
//! protection when required, prepare again, show the final preview, then commit.
//! Drop all live stores and protection session guards before commit, recovery,
//! or adoption. A pending replacement must be recovered before reopening stores.
//! Source passwords and decoded bodies are never persisted by this service.

mod filesystem;
#[cfg(test)]
mod tests;

pub(crate) use filesystem::{assert_no_pending_recovery, stage_protection_migration};

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

#[cfg(feature = "memory")]
use crate::memory::MemoryStore;
use crate::protection::{self, Domain, StoredFormat};
use crate::storage::chat_files::{ChatSavedFileStore, VaultItemId};
use crate::storage::paths::TcuiDataPaths;

const METADATA: &str = ".tcui-transfer.json";
const MAX_FILES: usize = 10_000;
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 256 * 1024 * 1024;
/// Native adoption and retained snapshots bound stored bytes, including envelopes.
/// They process files independently with no cumulative byte cap.
const MAX_NATIVE_FILE_BYTES: u64 = 1024 * 1024 * 1024;

type Result<T, E = TransferError> = std::result::Result<T, E>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransferMode {
    Add,
    Replace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExportProtection {
    /// Preserve the native domain policy, including portable password wrapping.
    Preserve,
    /// The caller has obtained explicit confirmation of a plaintext export.
    PlaintextConfirmed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TransferRequirement {
    SourcePassword { path: PathBuf },
    EnableProtection { domain: Domain },
    LegacySourceWrapping { path: PathBuf },
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum TransferError {
    #[error("transfer I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("transfer data is invalid: {0}")]
    Invalid(String),
    #[error("transfer source changed after preview: {0}")]
    SourceChanged(PathBuf),
    #[error("transfer prerequisites are not satisfied: {0:?}")]
    Requirements(Vec<TransferRequirement>),
    #[error("an interrupted replacement needs recovery from {backup}")]
    RecoveryNeeded { backup: PathBuf },
    #[error("transfer stopped after {imported} imports; previous native data is retained at {backup:?}: {reason}")]
    CommitIncomplete {
        imported: usize,
        backup: Option<PathBuf>,
        reason: String,
    },
    #[error("transfer protection failed: {0}")]
    Protection(#[from] color_eyre::Report),
    #[error("transfer serialization failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Vault transfer failed: {0}")]
    Vault(#[from] crate::storage::chat_files::ChatFileError),
    #[cfg(feature = "memory")]
    #[error("Memories transfer failed: {0}")]
    Memory(#[from] crate::memory::MemoryError),
}

/// A content hash covers the original stored bytes, not only decoded content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TransferFilePreview {
    pub(crate) relative_path: PathBuf,
    pub(crate) size_bytes: u64,
    pub(crate) source_hash: [u8; 32],
    pub(crate) encrypted: bool,
    pub(crate) conflicts: bool,
}

#[derive(Debug)]
pub(crate) struct PreparedTransfer {
    pub(crate) domain: Domain,
    pub(crate) mode: TransferMode,
    pub(crate) source: PathBuf,
    pub(crate) files: Vec<TransferFilePreview>,
    /// Trash and native recovery sources authenticated only for in-place adoption.
    pub(crate) adoption_files: Vec<TransferFilePreview>,
    pub(crate) total_bytes: u64,
    pub(crate) requirements: Vec<TransferRequirement>,
    pub(crate) same_native_root: bool,
    source_is_directory: bool,
    // Includes transfer metadata so changing an ID map invalidates confirmation.
    observed: Vec<filesystem::ObservedFile>,
    identity_map: Vec<Identity>,
}

#[derive(Debug, Default)]
pub(crate) struct TransferReport {
    pub(crate) imported: usize,
    pub(crate) exported: usize,
    pub(crate) backup: Option<PathBuf>,
    pub(crate) paths: Vec<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TransferMetadata {
    version: u32,
    domain: Domain,
    items: Vec<Identity>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Identity {
    path: PathBuf,
    id: u64,
}

/// No persistent external roots or credentials are retained.
pub(crate) struct NativeTransferService;

impl NativeTransferService {
    /// Inspect and authenticate the source without changing any source or target.
    /// Locked portable files are listed as actionable prerequisites, not accepted
    /// as authenticated input. Commit repeats every hash and validation check.
    /// External sources are limited to 64 MiB per stored file and 256 MiB total.
    /// Native adoption has no cumulative byte cap and allows 1 GiB per stored
    /// file. Both scans permit at most 10,000 files and 64 relative components.
    /// Native authentication processes one file at a time without disk staging.
    pub(crate) fn prepare(
        domain: Domain,
        source: &Path,
        mode: TransferMode,
    ) -> Result<PreparedTransfer> {
        require_domain(domain)?;
        protection::require(domain)?;
        let source = absolute_path(source)?;
        let source_is_directory = filesystem::is_directory(&source)?;
        let target = native_root(domain)?;
        let same_native_root = source_is_directory && source == target;
        let observed = filesystem::scan_source(domain, &source, same_native_root)?;
        let mut files = Vec::new();
        let mut adoption_files = Vec::new();
        let mut requirements = Vec::new();
        let mut identity_map = Vec::new();
        let mut total_bytes = 0;
        let mut has_encrypted = false;
        for file in &observed {
            let stored = filesystem::read_source_file(&file.path, same_native_root)?;
            if hash(&stored) != file.hash {
                return Err(TransferError::SourceChanged(file.path.clone()));
            }
            let format = protection::inspect_bytes(&stored)?;
            if let StoredFormat::Portable { domain: actual } = format {
                if actual != domain {
                    return invalid("source encrypted domain does not match the selected manager");
                }
            }
            let encrypted = format != StoredFormat::Plaintext;
            has_encrypted |= encrypted;
            let borrowed_plaintext = format == StoredFormat::Plaintext
                && !(domain == Domain::Memories && stored.starts_with(b"enc:v1:"));
            let decrypted = if borrowed_plaintext {
                None
            } else {
                decode_source(domain, &stored, &file.path, &mut requirements)?.map(Zeroizing::new)
            };
            let decoded = if borrowed_plaintext {
                Some(stored.as_slice())
            } else {
                decrypted.as_ref().map(|bytes| bytes.as_slice())
            };
            if same_native_root
                && file.relative != Path::new(METADATA)
                && file.relative.components().any(|component| {
                    matches!(component,
                    Component::Normal(name) if name.as_encoded_bytes().starts_with(b"."))
                })
            {
                adoption_files.push(TransferFilePreview {
                    relative_path: file.relative.clone(),
                    size_bytes: file.size,
                    source_hash: file.hash,
                    encrypted,
                    conflicts: false,
                });
                total_bytes += file.size;
                continue;
            }
            if file.relative == Path::new(METADATA) {
                if let Some(decoded) = decoded {
                    let metadata: TransferMetadata = serde_json::from_slice(decoded)?;
                    if metadata.version != 1 || metadata.domain != domain {
                        return invalid("unsupported transfer metadata or domain");
                    }
                    let mut paths = HashSet::new();
                    let mut ids = HashSet::new();
                    for item in &metadata.items {
                        validate_item_path(&item.path)?;
                        if item.id == 0 || !paths.insert(item.path.clone()) || !ids.insert(item.id)
                        {
                            return invalid("duplicate or invalid transfer identity");
                        }
                    }
                    identity_map = metadata.items;
                }
                continue;
            }
            validate_item_path(&file.relative)?;
            if let Some(decoded) = decoded {
                validate_source_document(domain, &file.relative, decoded)?;
            }
            total_bytes += file.size;
            files.push(TransferFilePreview {
                relative_path: file.relative.clone(),
                size_bytes: file.size,
                source_hash: file.hash,
                encrypted,
                conflicts: target.join(&file.relative).try_exists()?,
            });
        }
        if files.is_empty() && !same_native_root {
            return invalid("source has no importable native files");
        }
        if !same_native_root
            && identity_map
                .iter()
                .any(|item| !files.iter().any(|file| file.relative_path == item.path))
        {
            return invalid("transfer identity metadata refers to a missing source file");
        }
        if has_encrypted && !protection::is_protected(domain)? {
            requirements.push(TransferRequirement::EnableProtection { domain });
        }
        // Memory collisions are logical paths and IDs, not physical filenames.
        // Native-store access is intentionally deferred until adoption is allowed.
        #[cfg(feature = "memory")]
        if domain == Domain::Memories
            && !same_native_root
            && !requirements
                .iter()
                .any(|r| matches!(r, TransferRequirement::EnableProtection { .. }))
        {
            let store = MemoryStore::open_default()?;
            let entries = store.list_entries()?;
            for file in &mut files {
                let source_file = observed
                    .iter()
                    .find(|entry| entry.relative == file.relative_path)
                    .ok_or_else(|| TransferError::Invalid("source preview mismatch".into()))?;
                let stored = filesystem::read_file(&source_file.path)?;
                if let Some(decoded) =
                    decode_source(domain, &stored, &source_file.path, &mut requirements)?
                {
                    if file
                        .relative_path
                        .extension()
                        .is_some_and(|ext| ext == "tcui-memory")
                    {
                        let document: serde_json::Value = serde_json::from_slice(&decoded)?;
                        let id = document.get("id").and_then(serde_json::Value::as_u64);
                        let logical = document
                            .get("logical_path")
                            .and_then(serde_json::Value::as_str);
                        file.conflicts = entries.iter().any(|entry| {
                            Some(entry.id) == id
                                || logical.is_some_and(|path| entry.logical_path == Path::new(path))
                        });
                    } else {
                        file.conflicts = entries.iter().any(|entry| {
                            entry.logical_path == file.relative_path.with_extension("md")
                        });
                    }
                }
            }
        }
        Ok(PreparedTransfer {
            domain,
            mode,
            source,
            files,
            adoption_files,
            total_bytes,
            requirements,
            same_native_root,
            source_is_directory,
            observed,
            identity_map,
        })
    }

    /// Authenticate each portable wrapper with this password. If a directory has
    /// several source passwords, call again for the remaining requested files.
    /// Wrong-password attempts do not modify native data or the source files.
    pub(crate) fn unlock_source(prepared: &PreparedTransfer, password: &str) -> Result<()> {
        prepared.check_source()?;
        let mut unlocked = false;
        let mut last_error = None;
        for file in &prepared.observed {
            let stored = filesystem::read_source_file(&file.path, prepared.same_native_root)?;
            if matches!(
                protection::inspect_bytes(&stored)?,
                StoredFormat::Portable { .. }
            ) {
                if protection::decode(prepared.domain, &stored).is_ok() {
                    continue;
                }
                match protection::unlock_foreign_file(prepared.domain, &stored, password) {
                    Ok(()) => unlocked = true,
                    Err(error) => last_error = Some(error),
                }
            }
        }
        if let Some(error) = last_error {
            if !unlocked {
                return Err(error.into());
            }
        }
        Ok(())
    }

    /// Enable this domain while retaining every other selected security option.
    /// The supplied password is the target instance password, not the source's.
    pub(crate) fn enable_protection(domain: Domain, target_password: &str) -> Result<()> {
        require_domain(domain)?;
        let mut policy = protection::policy()?.unwrap_or_default();
        match domain {
            Domain::Vault => policy.vault = true,
            Domain::Memories => policy.memories = true,
            Domain::Chats => return invalid("Chats are not a transfer domain"),
        }
        protection::configure(policy, target_password)?;
        Ok(())
    }

    /// Adopt copied native data in place. First prepare/unlock this exact root.
    /// Protection's authenticated migration provides durable, target-key rewrap.
    pub(crate) fn adopt_native(
        prepared: PreparedTransfer,
        target_password: &str,
    ) -> Result<TransferReport> {
        if !prepared.same_native_root {
            return invalid("adoption requires the selected native root");
        }
        prepared.check_source()?;
        let blocked: Vec<_> = prepared
            .requirements
            .iter()
            .filter(|r| !matches!(r, TransferRequirement::EnableProtection { .. }))
            .cloned()
            .collect();
        if !blocked.is_empty() {
            return Err(TransferError::Requirements(blocked));
        }
        // Authenticate again before changing the target policy.
        for file in &prepared.observed {
            let stored = filesystem::read_source_file(&file.path, true)?;
            if protection::inspect_bytes(&stored)? == StoredFormat::Plaintext {
                continue;
            }
            let mut requirements = Vec::new();
            let decoded = decode_source(prepared.domain, &stored, &file.path, &mut requirements)?
                .map(Zeroizing::new);
            if decoded.is_none() {
                return Err(TransferError::Requirements(requirements));
            }
        }
        protection::adopt_native_store(prepared.domain, target_password)?;
        Ok(TransferReport {
            imported: prepared.files.len(),
            paths: prepared
                .files
                .into_iter()
                .map(|f| f.relative_path)
                .collect(),
            ..TransferReport::default()
        })
    }

    /// Export an entire domain. Destination must not exist.
    /// Nothing from the credential store is exported.
    pub(crate) fn export(
        domain: Domain,
        destination: &Path,
        export_protection: ExportProtection,
    ) -> Result<TransferReport> {
        require_domain(domain)?;
        let _exclusive = filesystem::exclusive()?;
        require_no_pending(domain)?;
        protection::require(domain)?;
        let destination = absolute_path(destination)?;
        let root = native_root(domain)?;
        if destination.starts_with(&root) || root.starts_with(&destination) {
            return invalid("export destination overlaps native storage");
        }
        let mut documents = Vec::new();
        let mut export_bytes = 0u64;
        match domain {
            Domain::Vault => {
                let store = vault_store()?;
                for item in store.list_items()? {
                    if item.size_bytes > MAX_FILE_BYTES {
                        return invalid("native Vault item exceeds the export file limit");
                    }
                    let bytes = Zeroizing::new(store.read_item(item.id)?);
                    let encoded = encode_export(domain, &bytes, export_protection)?;
                    account_export(documents.len(), &mut export_bytes, &encoded)?;
                    documents.push((item.relative_path, item.id.get(), encoded));
                }
            }
            #[cfg(feature = "memory")]
            Domain::Memories => {
                let store = MemoryStore::open_default()?;
                for item in store.list_entries()? {
                    if item.size_bytes > MAX_FILE_BYTES {
                        return invalid("native Memory item exceeds the export file limit");
                    }
                    let bytes = Zeroizing::new(store.export_document(item.id)?);
                    let encoded = encode_export(domain, &bytes, export_protection)?;
                    account_export(documents.len(), &mut export_bytes, &encoded)?;
                    documents.push((
                        PathBuf::from(format!("{:016x}.tcui-memory", item.id)),
                        item.id,
                        encoded,
                    ));
                }
            }
            #[cfg(not(feature = "memory"))]
            Domain::Memories => {
                return invalid("Memories support is unavailable without the memory feature")
            }
            Domain::Chats => return invalid("Chats are not a transfer domain"),
        }
        if documents
            .iter()
            .any(|(_, _, bytes)| bytes.len() as u64 > MAX_FILE_BYTES)
            || documents
                .iter()
                .map(|(_, _, bytes)| bytes.len() as u64)
                .sum::<u64>()
                > MAX_TOTAL_BYTES
        {
            return invalid("export exceeds the file or total byte limit");
        }
        if documents.len() > MAX_FILES {
            return invalid("export exceeds transfer file limit");
        }
        filesystem::create_private_directory(&destination)?;
        let mut report = TransferReport::default();
        let mut identities = Vec::new();
        for (path, id, bytes) in documents {
            if let Err(error) = filesystem::write_new(&destination.join(&path), &bytes) {
                return Err(TransferError::CommitIncomplete {
                    imported: report.exported,
                    backup: Some(destination),
                    reason: error.to_string(),
                });
            }
            identities.push(Identity {
                path: path.clone(),
                id,
            });
            report.paths.push(path);
            report.exported += 1;
        }
        let metadata = TransferMetadata {
            version: 1,
            domain,
            items: identities,
        };
        let bytes = Zeroizing::new(serde_json::to_vec(&metadata)?);
        let stored = encode_export(domain, &bytes, export_protection)?;
        filesystem::write_new(&destination.join(METADATA), &stored).map_err(|error| {
            TransferError::CommitIncomplete {
                imported: report.exported,
                backup: Some(destination),
                reason: error.to_string(),
            }
        })?;
        Ok(report)
    }

    /// Restore exact encoded source files and native identities after interrupted
    /// Replace. Current partial data is retained beside the original backup.
    /// Call before opening any stores. Repeating recovery is safe after a crash.
    pub(crate) fn recover_pending(domain: Domain) -> Result<Option<PathBuf>> {
        require_domain(domain)?;
        let _exclusive = filesystem::exclusive()?;
        filesystem::recover(domain)
    }

    /// Restore a completed Replace backup after explicit user confirmation.
    /// Retained snapshots and their restoration use the native 1 GiB stored-file
    /// limit, not external import limits, with no cumulative byte cap. Snapshot
    /// and recovery copies use bounded streams, including encoded envelopes.
    /// The data displaced by restoration is retained in that same backup.
    pub(crate) fn restore_backup(domain: Domain, backup: &Path) -> Result<PathBuf> {
        require_domain(domain)?;
        let _exclusive = filesystem::exclusive()?;
        require_no_pending(domain)?;
        filesystem::restore_saved(domain, &absolute_path(backup)?)
    }
}

impl PreparedTransfer {
    pub(crate) fn unlock_source(&self, password: &str) -> Result<()> {
        NativeTransferService::unlock_source(self, password)
    }

    /// Explicit confirmation is represented by consuming this preview. Call only
    /// after the UI displays Add/Replace and the listed conflicts. Replace keeps
    /// exact prior bodies and identities in a durable private recovery snapshot.
    pub(crate) fn commit(self) -> Result<TransferReport> {
        if self.same_native_root {
            return invalid("use native adoption for a same-root transfer");
        }
        let _exclusive = filesystem::exclusive()?;
        require_no_pending(self.domain)?;
        self.check_source()?;
        let refreshed = NativeTransferService::prepare(self.domain, &self.source, self.mode)?;
        if !refreshed.requirements.is_empty() {
            return Err(TransferError::Requirements(refreshed.requirements));
        }
        if self.files != refreshed.files {
            return Err(TransferError::SourceChanged(self.source));
        }
        let root = native_root(self.domain)?;
        if self.source.starts_with(&root) || root.starts_with(&self.source) {
            return invalid("import source overlaps native storage");
        }
        // Decode and validate all inputs before the first destination mutation.
        let mut documents = Vec::with_capacity(self.files.len());
        for file in &self.observed {
            if file.relative == Path::new(METADATA) {
                continue;
            }
            let stored = filesystem::read_file(&file.path)?;
            if hash(&stored) != file.hash {
                return Err(TransferError::SourceChanged(file.path.clone()));
            }
            let mut requirements = Vec::new();
            let bytes = decode_source(self.domain, &stored, &file.path, &mut requirements)?
                .ok_or(TransferError::Requirements(requirements))?;
            let bytes = Zeroizing::new(bytes);
            validate_source_document(self.domain, &file.relative, &bytes)?;
            documents.push((&file.relative, bytes));
        }
        let backup = if self.mode == TransferMode::Replace {
            Some(filesystem::snapshot(self.domain)?)
        } else {
            None
        };
        let mut report = TransferReport {
            backup,
            ..TransferReport::default()
        };
        let result = (|| -> Result<()> {
            match self.domain {
                Domain::Vault => {
                    let store = vault_store()?;
                    if self.mode == TransferMode::Replace {
                        for item in store.list_items()? {
                            store.trash_item(item.id)?;
                        }
                    }
                    for (path, bytes) in documents {
                        let preferred = self
                            .identity_map
                            .iter()
                            .find(|item| item.path == *path)
                            .map(|item| item.id.to_string().parse::<VaultItemId>())
                            .transpose()
                            .map_err(|_| TransferError::Invalid("invalid Vault item ID".into()))?;
                        let item = store.import_item_at(path, &bytes, preferred)?;
                        report.paths.push(item.relative_path);
                        report.imported += 1;
                    }
                }
                #[cfg(feature = "memory")]
                Domain::Memories => {
                    let store = MemoryStore::open_default()?;
                    if self.mode == TransferMode::Replace {
                        for item in store.list_entries()? {
                            store.trash_entry(item.id)?;
                        }
                    }
                    for (path, bytes) in documents {
                        if path.extension().is_some_and(|ext| ext == "tcui-memory") {
                            let item = store.import_document(&bytes)?;
                            report.paths.push(item.logical_path);
                        } else {
                            let markdown = std::str::from_utf8(&bytes).map_err(|_| {
                                TransferError::Invalid("memory text is not UTF-8".into())
                            })?;
                            let names: HashSet<_> = store
                                .list_entries()?
                                .into_iter()
                                .map(|item| item.logical_path)
                                .collect();
                            let logical = available_path(&path.with_extension("md"), &names)?;
                            store.write(&logical, markdown, false)?;
                            report.paths.push(logical);
                        }
                        report.imported += 1;
                    }
                }
                #[cfg(not(feature = "memory"))]
                Domain::Memories => {
                    return invalid("Memories support is unavailable without the memory feature")
                }
                Domain::Chats => return invalid("Chats are not a transfer domain"),
            }
            if report.backup.is_some() {
                filesystem::finish(self.domain)?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            #[cfg(feature = "memory")]
            if matches!(
                &error,
                TransferError::Memory(crate::memory::MemoryError::SavedButNotIndexed { .. })
            ) {
                report.imported += 1;
            }
            return Err(TransferError::CommitIncomplete {
                imported: report.imported,
                backup: report.backup,
                reason: error.to_string(),
            });
        }
        Ok(report)
    }

    fn check_source(&self) -> Result<()> {
        let current = filesystem::scan_source(self.domain, &self.source, self.same_native_root)?;
        if current != self.observed
            || filesystem::is_directory(&self.source)? != self.source_is_directory
        {
            return Err(TransferError::SourceChanged(self.source.clone()));
        }
        Ok(())
    }
}

fn decode_source(
    domain: Domain,
    stored: &[u8],
    path: &Path,
    requirements: &mut Vec<TransferRequirement>,
) -> Result<Option<Vec<u8>>> {
    if domain == Domain::Memories && stored.starts_with(b"enc:v1:") {
        return invalid("legacy Memories needs its original chat.key; upgrade it on the source instance before portable export");
    }
    match protection::inspect_bytes(stored)? {
        // Never decode plain external bytes under the destination's policy.
        StoredFormat::Plaintext => Ok(Some(stored.to_vec())),
        StoredFormat::LegacyInstanceBound => {
            requirements.push(TransferRequirement::LegacySourceWrapping {
                path: path.to_path_buf(),
            });
            Ok(None)
        }
        StoredFormat::Portable { domain: actual } if actual != domain => {
            invalid("portable source domain mismatch")
        }
        StoredFormat::Portable { .. } => match protection::decode(domain, stored) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error)
                if error
                    .downcast_ref::<protection::PortableUnlockNeeded>()
                    .is_some() =>
            {
                let requirement = TransferRequirement::SourcePassword {
                    path: path.to_path_buf(),
                };
                if !requirements.contains(&requirement) {
                    requirements.push(requirement);
                }
                Ok(None)
            }
            Err(error) => Err(error.into()),
        },
    }
}

#[cfg(not(feature = "memory"))]
fn validate_source_document(domain: Domain, _path: &Path, _bytes: &[u8]) -> Result<()> {
    require_domain(domain)
}

#[cfg(feature = "memory")]
fn validate_source_document(domain: Domain, path: &Path, bytes: &[u8]) -> Result<()> {
    if domain != Domain::Memories {
        return Ok(());
    }
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("tcui-memory") => {
            MemoryStore::validate_import_document(bytes)?;
        }
        Some("md" | "txt") => {
            let text = std::str::from_utf8(bytes)
                .map_err(|_| TransferError::Invalid("memory text is not UTF-8".into()))?;
            if text.trim().is_empty() || text.len() > 1024 * 1024 {
                return invalid("memory text must be nonempty and at most 1 MiB");
            }
        }
        _ => return invalid("Memories accepts native .tcui-memory, Markdown, or text files only"),
    }
    Ok(())
}

fn encode_export(domain: Domain, bytes: &[u8], export: ExportProtection) -> Result<Vec<u8>> {
    Ok(match export {
        ExportProtection::Preserve => protection::encode(domain, bytes)?,
        ExportProtection::PlaintextConfirmed => bytes.to_vec(),
    })
}

fn account_export(count: usize, total: &mut u64, bytes: &[u8]) -> Result<()> {
    *total = total
        .checked_add(bytes.len() as u64)
        .ok_or_else(|| TransferError::Invalid("export size overflow".into()))?;
    if count >= MAX_FILES || bytes.len() as u64 > MAX_FILE_BYTES || *total > MAX_TOTAL_BYTES {
        return invalid("export exceeds the file count or byte limits");
    }
    Ok(())
}

fn require_domain(domain: Domain) -> Result<()> {
    if domain == Domain::Chats {
        return invalid("Chats are not a transfer domain");
    }
    #[cfg(not(feature = "memory"))]
    if domain == Domain::Memories {
        return invalid("Memories support is unavailable without the memory feature");
    }
    Ok(())
}

/// The only managed root for this domain. Transfer input paths do not change it.
pub(crate) fn native_root(domain: Domain) -> Result<PathBuf> {
    require_domain(domain)?;
    Ok(protection::root()?.join(match domain {
        Domain::Vault => "vault",
        Domain::Memories => "memories",
        Domain::Chats => return invalid("Chats are not a transfer domain"),
    }))
}

fn vault_store() -> Result<ChatSavedFileStore> {
    let paths = TcuiDataPaths::from_root(protection::root()?);
    Ok(ChatSavedFileStore::new(paths.root.join("vault"), paths))
}

fn absolute_path(path: &Path) -> Result<PathBuf> {
    if path.as_os_str().is_empty() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return invalid("empty paths and parent traversal are not allowed");
    }
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    Ok(path
        .components()
        .filter(|c| !matches!(c, Component::CurDir))
        .collect())
}

fn validate_item_path(path: &Path) -> Result<()> {
    if !crate::vault::VaultRelativePath::parse(path).is_ok_and(|parsed| parsed.as_path() == path) {
        return invalid("source item path is not a valid native relative path");
    }
    if path.as_os_str().is_empty() || path.to_str().is_none() || path.components().count() > 64
        || path.components().any(|component| !matches!(component, Component::Normal(name) if !name.as_encoded_bytes().starts_with(b"."))) {
        return invalid("source item path is unsafe or contains reserved dotfiles");
    }
    Ok(())
}

#[cfg(feature = "memory")]
fn available_path(path: &Path, names: &HashSet<PathBuf>) -> Result<PathBuf> {
    if !names.contains(path) {
        return Ok(path.to_path_buf());
    }
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| TransferError::Invalid("invalid memory filename".into()))?;
    for suffix in 2..=MAX_FILES + 1 {
        let candidate = path.with_file_name(format!("{stem}-{suffix}.md"));
        if !names.contains(&candidate) {
            return Ok(candidate);
        }
    }
    invalid("no available memory logical path")
}

fn require_no_pending(domain: Domain) -> Result<()> {
    if let Some(backup) = filesystem::pending(domain)? {
        return Err(TransferError::RecoveryNeeded { backup });
    }
    Ok(())
}

fn hash(bytes: &[u8]) -> [u8; 32] {
    *blake3::hash(bytes).as_bytes()
}
fn invalid<T>(message: &str) -> Result<T> {
    Err(TransferError::Invalid(message.to_string()))
}
