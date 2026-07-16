use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::atomic::{sync_directory, AtomicWriteTarget, AtomicWriter};
use super::model::{
    enforce_conversation_quota, record_is_structurally_valid, sanitize_name, ChatFileError,
    ChatFileId, ChatFileManifestV1, ChatFileRecord, StoreLimits, MANIFEST_VERSION,
};
use super::ChatFileStore;

const MANIFEST_NAME: &str = "manifest.json";

pub(super) enum ManifestInspection {
    Current(ChatFileManifestV1),
    Missing,
    Corrupt,
    Future(u64),
}

pub(super) struct ConversationManifest<'a> {
    directory: &'a Path,
    conversation_id: i64,
    limits: StoreLimits,
    writer: &'a AtomicWriter,
}

impl<'a> ConversationManifest<'a> {
    pub(super) fn new(directory: &'a Path, conversation_id: i64, store: &'a ChatFileStore) -> Self {
        Self {
            directory,
            conversation_id,
            limits: store.limits,
            writer: &store.writer,
        }
    }

    pub(super) fn inspect(&self) -> Result<ManifestInspection, ChatFileError> {
        let bytes = match fs::read(self.path()) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ManifestInspection::Missing);
            }
            Err(error) => return Err(error.into()),
        };
        let Ok(header) = serde_json::from_slice::<VersionHeader>(&bytes) else {
            return Ok(ManifestInspection::Corrupt);
        };
        if header.version > u64::from(MANIFEST_VERSION) {
            return Ok(ManifestInspection::Future(header.version));
        }
        if header.version != u64::from(MANIFEST_VERSION) {
            return Ok(ManifestInspection::Corrupt);
        }
        let Ok(manifest) = serde_json::from_slice::<ChatFileManifestV1>(&bytes) else {
            return Ok(ManifestInspection::Corrupt);
        };
        if !self.is_consistent(&manifest)? {
            return Ok(ManifestInspection::Corrupt);
        }
        Ok(ManifestInspection::Current(manifest))
    }

    pub(super) fn load_or_rebuild(&self) -> Result<ChatFileManifestV1, ChatFileError> {
        match self.inspect()? {
            ManifestInspection::Current(manifest) => Ok(manifest),
            ManifestInspection::Future(version) => {
                Err(ChatFileError::UnsupportedManifestVersion { version })
            }
            ManifestInspection::Missing => self.scan_and_write(),
            ManifestInspection::Corrupt => {
                self.quarantine()?;
                self.scan_and_write()
            }
        }
    }

    pub(super) fn force_rebuild(&self) -> Result<ChatFileManifestV1, ChatFileError> {
        match self.inspect()? {
            ManifestInspection::Future(version) => {
                Err(ChatFileError::UnsupportedManifestVersion { version })
            }
            ManifestInspection::Corrupt => {
                self.quarantine()?;
                self.scan_and_write()
            }
            ManifestInspection::Current(_) | ManifestInspection::Missing => self.scan_and_write(),
        }
    }

    pub(super) fn write(&self, manifest: &ChatFileManifestV1) -> Result<(), ChatFileError> {
        let bytes = serde_json::to_vec_pretty(manifest)?;
        self.writer
            .write(&self.path(), &bytes, AtomicWriteTarget::Manifest)
    }

    fn scan_and_write(&self) -> Result<ChatFileManifestV1, ChatFileError> {
        let mut manifest = ChatFileManifestV1::empty(self.conversation_id);
        let mut seen_ids = HashSet::new();
        let mut entries = fs::read_dir(self.directory)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(fs::DirEntry::file_name);

        for entry in entries {
            if !entry.file_type()?.is_file() {
                continue;
            }
            let Some(record) = record_from_payload(&entry.path())? else {
                continue;
            };
            if !seen_ids.insert(record.id) {
                return Err(ChatFileError::DuplicatePayloadId { id: record.id });
            }
            enforce_conversation_quota(&manifest.files, record.size_bytes, self.limits)?;
            manifest.files.push(record);
        }
        self.write(&manifest)?;
        Ok(manifest)
    }

    fn quarantine(&self) -> Result<(), ChatFileError> {
        let manifest_path = self.path();
        loop {
            let quarantine = self
                .directory
                .join(format!("manifest.corrupt-{}.json", rand::random::<u64>()));
            if quarantine.exists() {
                continue;
            }
            fs::rename(&manifest_path, quarantine)?;
            sync_directory(self.directory)?;
            return Ok(());
        }
    }

    fn is_consistent(&self, manifest: &ChatFileManifestV1) -> Result<bool, ChatFileError> {
        if manifest.version != MANIFEST_VERSION || manifest.conversation_id != self.conversation_id
        {
            return Ok(false);
        }
        let mut ids = HashSet::new();
        let mut total_size = 0_u64;
        for record in &manifest.files {
            if !record_is_structurally_valid(record, self.limits) || !ids.insert(record.id) {
                return Ok(false);
            }
            let Some(updated_total) = total_size.checked_add(record.size_bytes) else {
                return Ok(false);
            };
            total_size = updated_total;
            if total_size > self.limits.per_conversation {
                return Ok(false);
            }
            let metadata = match fs::symlink_metadata(self.directory.join(record.payload_name())) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
                Err(error) => return Err(error.into()),
            };
            if !metadata.file_type().is_file() || metadata.len() != record.size_bytes {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn path(&self) -> PathBuf {
        self.directory.join(MANIFEST_NAME)
    }
}

#[derive(Deserialize)]
struct VersionHeader {
    version: u64,
}

fn record_from_payload(path: &Path) -> Result<Option<ChatFileRecord>, ChatFileError> {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return Ok(None);
    };
    let Some((id, name)) = file_name.split_once("--") else {
        return Ok(None);
    };
    let Some(id) = ChatFileId::parse(id) else {
        return Ok(None);
    };
    if name.is_empty() || name != sanitize_name(name) {
        return Ok(None);
    }
    let size_bytes = fs::symlink_metadata(path)?.len();
    Ok(Some(ChatFileRecord {
        id,
        name: name.to_string(),
        size_bytes,
    }))
}
