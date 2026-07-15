use super::{KeyStoreError, CURRENT_STORE_VERSION};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredKeysFile {
    #[serde(default = "current_store_version")]
    pub(super) version: u32,
    #[serde(default)]
    pub(super) keys: BTreeMap<String, String>,
    #[serde(default)]
    pub(super) oauth: BTreeMap<String, String>,
    #[serde(default)]
    pub(super) credentials: BTreeMap<String, String>,
}

impl Default for StoredKeysFile {
    fn default() -> Self {
        Self {
            version: CURRENT_STORE_VERSION,
            keys: BTreeMap::new(),
            oauth: BTreeMap::new(),
            credentials: BTreeMap::new(),
        }
    }
}

pub(super) enum LoadedKeysFile {
    Current(StoredKeysFile),
    Future(FutureStoredKeysFile),
}

impl LoadedKeysFile {
    pub(super) fn keys(&self) -> &BTreeMap<String, String> {
        match self {
            Self::Current(file) => &file.keys,
            Self::Future(file) => &file.keys,
        }
    }

    pub(super) fn into_current(self) -> std::result::Result<StoredKeysFile, KeyStoreError> {
        match self {
            Self::Current(file) => Ok(file),
            Self::Future(file) => Err(KeyStoreError::UnsupportedVersion {
                version: file.version,
            }),
        }
    }
}

#[derive(Deserialize)]
struct StoreVersion {
    #[serde(default = "current_store_version")]
    version: u32,
}

#[derive(Deserialize)]
pub(super) struct FutureStoredKeysFile {
    #[serde(default = "current_store_version")]
    version: u32,
    #[serde(default)]
    keys: BTreeMap<String, String>,
}

pub(super) fn parse(content: &str) -> std::result::Result<LoadedKeysFile, KeyStoreError> {
    let version = toml::from_str::<StoreVersion>(content).map_err(|_| KeyStoreError::Parse)?;

    if version.version == CURRENT_STORE_VERSION {
        toml::from_str::<StoredKeysFile>(content)
            .map(LoadedKeysFile::Current)
            .map_err(|_| KeyStoreError::Parse)
    } else {
        toml::from_str::<FutureStoredKeysFile>(content)
            .map(LoadedKeysFile::Future)
            .map_err(|_| KeyStoreError::Parse)
    }
}

fn current_store_version() -> u32 {
    CURRENT_STORE_VERSION
}
