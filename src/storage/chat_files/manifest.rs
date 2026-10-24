use std::collections::HashSet;

use serde::Deserialize;

use super::atomic::{AtomicWriteTarget, AtomicWriter};
use super::model::{
    link_filename_is_valid, ChatFileError, ChatFileLinkManifestV1, MANIFEST_VERSION,
};
use crate::storage::paths::TcuiDataPaths;

pub(super) struct LinkManifest<'a> {
    paths: &'a TcuiDataPaths,
    writer: &'a AtomicWriter,
}

impl<'a> LinkManifest<'a> {
    pub(super) const fn new(paths: &'a TcuiDataPaths, writer: &'a AtomicWriter) -> Self {
        Self { paths, writer }
    }

    pub(super) fn load(&self) -> Result<ChatFileLinkManifestV1, ChatFileError> {
        crate::protection::require(crate::protection::Domain::Vault)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        let bytes = match crate::protection::read_regular(&self.paths.chat_file_links_manifest) {
            Ok(bytes) => bytes,
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
            {
                return Ok(ChatFileLinkManifestV1::empty());
            }
            Err(error) => return Err(std::io::Error::other(error.to_string()).into()),
        };
        let bytes = crate::protection::decode(crate::protection::Domain::Vault, &bytes)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        let header = serde_json::from_slice::<VersionHeader>(&bytes)
            .map_err(|_| ChatFileError::CorruptManifest)?;
        if header.version > u64::from(MANIFEST_VERSION) {
            return Err(ChatFileError::UnsupportedManifestVersion {
                version: header.version,
            });
        }
        if header.version != u64::from(MANIFEST_VERSION) {
            return Err(ChatFileError::CorruptManifest);
        }
        let manifest = serde_json::from_slice::<ChatFileLinkManifestV1>(&bytes)
            .map_err(|_| ChatFileError::CorruptManifest)?;
        if manifest_is_valid(&manifest) {
            Ok(manifest)
        } else {
            Err(ChatFileError::CorruptManifest)
        }
    }

    pub(super) fn write(&self, manifest: &ChatFileLinkManifestV1) -> Result<(), ChatFileError> {
        let bytes = serde_json::to_vec_pretty(manifest)?;
        let bytes = crate::protection::encode(crate::protection::Domain::Vault, &bytes)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        self.writer.write_replace(
            &self.paths.chat_file_links_manifest,
            &bytes,
            AtomicWriteTarget::Manifest,
        )
    }
}

#[derive(Deserialize)]
struct VersionHeader {
    version: u64,
}

fn manifest_is_valid(manifest: &ChatFileLinkManifestV1) -> bool {
    if manifest.version != MANIFEST_VERSION {
        return false;
    }
    let mut ids = HashSet::new();
    manifest.links.iter().all(|link| {
        link.id.is_nonzero()
            && link.conversation_id > 0
            && ids.insert(link.id)
            && link_filename_is_valid(&link.filename)
    })
}
