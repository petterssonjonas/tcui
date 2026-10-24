use std::borrow::Cow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm,
};
use color_eyre::eyre::{bail, eyre, Result};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::config::key_store::KeyStore;
use crate::config::AppConfig;

mod io;
mod migration;
pub(crate) mod ui;

const MAGIC: &[u8] = b"TCUI-PROTECTED\x01";
const VERSION: u32 = 1;
struct Session {
    key: Zeroizing<[u8; 32]>,
    manifest: Option<Manifest>,
}

static SESSIONS: LazyLock<Mutex<HashMap<PathBuf, Session>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Domain {
    Chats,
    Memories,
    Vault,
}

impl Domain {
    fn context(self) -> &'static [u8] {
        match self {
            Self::Chats => b"tcui/protection/v1/chats",
            Self::Memories => b"tcui/protection/v1/memories",
            Self::Vault => b"tcui/protection/v1/vault",
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProtectionPolicy {
    pub app_password: bool,
    pub credentials: bool,
    pub chats: bool,
    pub memories: bool,
    pub vault: bool,
}

impl ProtectionPolicy {
    pub(crate) fn any(self) -> bool {
        self.app_password || self.credentials || self.chats || self.memories || self.vault
    }

    fn protects(self, domain: Domain) -> bool {
        match domain {
            Domain::Chats => self.chats,
            Domain::Memories => self.memories,
            Domain::Vault => self.vault,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    policy: ProtectionPolicy,
    authentication: Option<[u8; 32]>,
}

impl Manifest {
    fn new(policy: ProtectionPolicy, key: Option<&[u8; 32]>) -> Result<Self> {
        let mut manifest = Self {
            version: VERSION,
            policy,
            authentication: None,
        };
        if let Some(key) = key {
            manifest.authentication = Some(manifest.tag(key)?);
        }
        Ok(manifest)
    }

    fn validate(&self) -> Result<()> {
        if self.version != VERSION {
            bail!("Unsupported application protection version; restore compatible TCUI software.");
        }
        if self.policy.any() && self.authentication.is_none() {
            bail!(
                "Application protection metadata is incomplete; do not create a replacement store."
            );
        }
        Ok(())
    }

    fn tag(&self, key: &[u8; 32]) -> Result<[u8; 32]> {
        let bytes = serde_json::to_vec(&("tcui/protection-policy", self.version, self.policy))?;
        Ok(*blake3::keyed_hash(key, &bytes).as_bytes())
    }

    fn authenticate(&self, key: &[u8; 32]) -> Result<()> {
        self.validate()?;
        let actual = self
            .authentication
            .ok_or_else(|| eyre!("Protection metadata has no password authentication."))?;
        if blake3::Hash::from(actual) != blake3::Hash::from(self.tag(key)?) {
            bail!("Application protection metadata authentication failed.");
        }
        Ok(())
    }
}

pub(crate) fn root() -> Result<PathBuf> {
    Ok(dirs::data_dir()
        .ok_or_else(|| eyre!("Cannot discover the TCUI data directory; refusing a CWD fallback."))?
        .join("tcui"))
}

pub(crate) fn internal_vault_root() -> PathBuf {
    crate::storage::paths::TcuiDataPaths::discover()
        .root
        .join("vault")
}

fn manifest_path() -> Result<PathBuf> {
    Ok(root()?.join("protection.json"))
}

fn load_manifest() -> Result<Option<Manifest>> {
    let manifest = if let Some(pending) = migration::pending()? {
        pending.target.validate()?;
        Some(pending.target)
    } else {
        match io::read_regular(&manifest_path()?) {
            Ok(bytes) => {
                let manifest: Manifest = serde_json::from_slice(&bytes)?;
                manifest.validate()?;
                Some(manifest)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        }
    };
    if let Some(session) = SESSIONS
        .lock()
        .map_err(|_| eyre!("Protection session lock is unavailable."))?
        .get(&root()?)
    {
        if session.manifest != manifest {
            bail!("Protection metadata changed outside this session. Restart and authenticate before accessing data.");
        }
        if let Some(manifest) = manifest
            .as_ref()
            .filter(|manifest| manifest.authentication.is_some())
        {
            manifest.authenticate(&session.key)?;
        }
    }
    Ok(manifest)
}

pub(crate) fn policy() -> Result<Option<ProtectionPolicy>> {
    Ok(load_manifest()?.map(|manifest| manifest.policy))
}

pub(crate) fn configured() -> Result<bool> {
    Ok(policy()?.is_some())
}

pub(crate) fn is_protected(domain: Domain) -> Result<bool> {
    Ok(policy()?.is_some_and(|policy| policy.protects(domain)))
}

pub(crate) fn is_unlocked() -> Result<bool> {
    Ok(SESSIONS
        .lock()
        .map_err(|_| eyre!("Protection session lock is unavailable."))?
        .contains_key(&root()?))
}

pub(crate) fn startup_requires_unlock() -> Result<bool> {
    if migration::pending()?.is_some() {
        return Ok(true);
    }
    Ok(policy()?.is_some_and(|policy| policy.app_password || policy.chats))
}

pub(crate) fn require_app() -> Result<()> {
    if migration::pending()?.is_some() {
        bail!("An interrupted protection migration needs recovery. Unlock TCUI to continue.");
    }
    if policy()?.is_some_and(|policy| policy.app_password || policy.chats) && !is_unlocked()? {
        bail!("TCUI is locked. Enter the application password before accessing its data.");
    }
    Ok(())
}

pub(crate) fn require(domain: Domain) -> Result<()> {
    require_app()?;
    if is_protected(domain)? && !is_unlocked()? {
        bail!("{domain:?} storage is locked. Use /unlock and enter the application password.");
    }
    Ok(())
}

fn with_key<T>(action: impl FnOnce(&[u8; 32]) -> Result<T>) -> Result<T> {
    let sessions = SESSIONS
        .lock()
        .map_err(|_| eyre!("Protection session lock is unavailable."))?;
    let key = sessions
        .get(&root()?)
        .ok_or_else(|| eyre!("Protected storage is locked; no decryption key is available."))?;
    action(&key.key)
}

pub(crate) fn unlock(password: &str) -> Result<()> {
    // A failed attempt must not leave an earlier unlocked session usable.
    lock()?;
    let result = (|| {
        let config = AppConfig::default();
        let manifest = load_manifest()?;
        let status = KeyStore::vault_status(&config)?;
        if status.initialized {
            KeyStore::unlock_vault(&config, password)?;
        } else if manifest
            .as_ref()
            .is_some_and(|manifest| manifest.authentication.is_some())
        {
            bail!("The password vault is missing; restore its database instead of creating a replacement.");
        }
        if let Some(manifest) = manifest.filter(|manifest| manifest.authentication.is_some()) {
            let key = KeyStore::protection_key(false)?;
            manifest.authenticate(&key)?;
            SESSIONS
                .lock()
                .map_err(|_| eyre!("Protection session lock is unavailable."))?
                .insert(
                    root()?,
                    Session {
                        key,
                        manifest: Some(manifest),
                    },
                );
        }
        migration::recover()
    })();
    if result.is_err() {
        let _ = lock();
    }
    result
}

pub(crate) fn lock() -> Result<()> {
    SESSIONS
        .lock()
        .map_err(|_| eyre!("Protection session lock is unavailable."))?
        .remove(&root()?);
    KeyStore::lock_vault(&AppConfig::default())?;
    Ok(())
}

pub(crate) fn configure(target: ProtectionPolicy, password: &str) -> Result<()> {
    let _lock = io::migration_lock()?;
    if migration::pending()?.is_some() {
        bail!("Recover the interrupted protection migration with /unlock before changing options.");
    }
    let old = load_manifest()?;
    let config = AppConfig::default();
    let status = KeyStore::vault_status(&config)?;
    if old.as_ref().is_some_and(|old| old.authentication.is_some()) && !status.initialized {
        bail!("The password vault is missing; restore its database before changing protection.");
    }
    let needs_key = target.any() || old.as_ref().is_some_and(|old| old.authentication.is_some());
    let key = if needs_key || status.initialized {
        if password.is_empty() {
            bail!("Enter the existing password, or choose a password for the protected stores.");
        }
        lock()?;
        if status.initialized {
            KeyStore::unlock_vault(&config, password)?;
        } else {
            KeyStore::initialize_vault(&config, password)?;
        }
        let key = KeyStore::protection_key(
            old.is_none() || !old.as_ref().is_some_and(|m| m.authentication.is_some()),
        )?;
        if let Some(old) = old.as_ref().filter(|old| old.authentication.is_some()) {
            old.authenticate(&key)?;
        }
        Some(key)
    } else {
        None
    };
    if let Some(key) = &key {
        SESSIONS
            .lock()
            .map_err(|_| eyre!("Protection session lock is unavailable."))?
            .insert(
                root()?,
                Session {
                    key: Zeroizing::new(**key),
                    manifest: old.clone(),
                },
            );
    }
    if old.as_ref().is_some_and(|old| old.policy == target) {
        let storage = if target.credentials {
            crate::config::key_store::CredentialStorage::Encrypted
        } else {
            crate::config::key_store::CredentialStorage::Plaintext
        };
        KeyStore::set_storage_default(&config, storage)?;
        return Ok(());
    }
    let target = Manifest::new(target, key.as_deref())?;
    migration::prepare(old.as_ref(), &target, key.as_deref())?;
    if let Some(session) = SESSIONS
        .lock()
        .map_err(|_| eyre!("Protection session lock is unavailable."))?
        .get_mut(&root()?)
    {
        session.manifest = Some(target.clone());
    }
    migration::recover_locked(key.as_deref())
}

/// Enrolls the shared password without changing any selected data protection or existing records.
pub(crate) fn initialize_password(password: &str) -> Result<()> {
    if password.is_empty() || password.len() > 4096 {
        bail!("Choose a nonempty password of at most 4096 UTF-8 bytes.");
    }
    let _lock = io::migration_lock()?;
    if migration::pending()?.is_some() {
        bail!("Recover the interrupted protection migration before initializing a password.");
    }
    let old = load_manifest()?
        .ok_or_else(|| eyre!("Choose application protection options with /security first."))?;
    if old.authentication.is_some() {
        return unlock(password);
    }
    let config = AppConfig::default();
    let status = KeyStore::vault_status(&config)?;
    lock()?;
    if status.initialized {
        KeyStore::unlock_vault(&config, password)?;
    } else {
        KeyStore::initialize_vault(&config, password)?;
    }
    let key = KeyStore::protection_key(true)?;
    let manifest = Manifest::new(old.policy, Some(&key))?;
    io::atomic_write(&manifest_path()?, &serde_json::to_vec(&manifest)?)?;
    SESSIONS
        .lock()
        .map_err(|_| eyre!("Protection session lock is unavailable."))?
        .insert(
            root()?,
            Session {
                key,
                manifest: Some(manifest),
            },
        );
    Ok(())
}

pub(crate) fn session_guard() -> Result<std::fs::File> {
    require_app()?;
    Ok(io::session_lock()?)
}

pub(crate) fn open_directory(path: &Path, create: bool) -> Result<cap_std::fs::Dir> {
    Ok(io::directory(path, create)?)
}

pub(crate) fn read_regular(path: &Path) -> Result<Vec<u8>> {
    Ok(io::read_regular(path)?)
}

/// An explicit plaintext export, not a write into a managed protected store.
pub(crate) fn write_plain(path: &Path, plaintext: &[u8]) -> Result<()> {
    Ok(io::atomic_write(path, plaintext)?)
}

/// Publishes an explicitly requested plaintext export without replacing existing data.
pub(crate) fn write_new_plain(path: &Path, plaintext: &[u8]) -> Result<()> {
    Ok(io::atomic_write_new(path, plaintext)?)
}

pub(crate) fn encode(domain: Domain, plaintext: &[u8]) -> Result<Vec<u8>> {
    Ok(encode_bytes(domain, plaintext)?.into_owned())
}

fn encode_bytes(domain: Domain, plaintext: &[u8]) -> Result<Cow<'_, [u8]>> {
    require(domain)?;
    if !is_protected(domain)? {
        return Ok(Cow::Borrowed(plaintext));
    }
    with_key(|key| seal(domain.context(), plaintext, key)).map(Cow::Owned)
}

pub(crate) fn decode(domain: Domain, stored: &[u8]) -> Result<Vec<u8>> {
    Ok(decode_bytes(domain, stored)?.into_owned())
}

fn decode_bytes(domain: Domain, stored: &[u8]) -> Result<Cow<'_, [u8]>> {
    require(domain)?;
    if is_protected(domain)? {
        return with_key(|key| open(domain.context(), stored, key)).map(Cow::Owned);
    }
    Ok(Cow::Borrowed(stored))
}

pub(crate) fn read(domain: Domain, path: &Path) -> Result<Vec<u8>> {
    require(domain)?;
    let stored = io::read_regular(path)?;
    match decode_bytes(domain, &stored)? {
        Cow::Borrowed(_) => Ok(stored),
        Cow::Owned(plaintext) => Ok(plaintext),
    }
}

/// Explicitly decrypts an archived document even when its managed store is now plaintext.
/// Routine store reads must use `read` and the representation selected by their policy.
pub(crate) fn read_for_explicit_decryption(domain: Domain, path: &Path) -> Result<Vec<u8>> {
    require(domain)?;
    let stored = io::read_regular(path)?;
    if stored.starts_with(MAGIC) {
        return with_key(|key| open(domain.context(), &stored, key));
    }
    match decode_bytes(domain, &stored)? {
        Cow::Borrowed(_) => Ok(stored),
        Cow::Owned(plaintext) => Ok(plaintext),
    }
}

pub(crate) fn write(domain: Domain, path: &Path, plaintext: &[u8]) -> Result<()> {
    let stored = encode_bytes(domain, plaintext)?;
    io::atomic_write(path, &stored)?;
    Ok(())
}

pub(crate) fn write_new(domain: Domain, path: &Path, plaintext: &[u8]) -> Result<()> {
    let stored = encode_bytes(domain, plaintext)?;
    Ok(io::atomic_write_new(path, &stored)?)
}

fn seal(context: &[u8], plaintext: &[u8], master: &[u8; 32]) -> Result<Vec<u8>> {
    let key = Zeroizing::new(*blake3::keyed_hash(master, context).as_bytes());
    let cipher =
        Aes256Gcm::new_from_slice(key.as_ref()).map_err(|_| eyre!("Invalid protection key."))?;
    let nonce = rand::random::<[u8; 12]>();
    let ciphertext = cipher
        .encrypt(
            (&nonce).into(),
            Payload {
                msg: plaintext,
                aad: context,
            },
        )
        .map_err(|_| eyre!("Failed to encrypt protected data."))?;
    let mut stored = Vec::with_capacity(MAGIC.len() + nonce.len() + ciphertext.len());
    stored.extend_from_slice(MAGIC);
    stored.extend_from_slice(&nonce);
    stored.extend_from_slice(&ciphertext);
    Ok(stored)
}

fn open(context: &[u8], stored: &[u8], master: &[u8; 32]) -> Result<Vec<u8>> {
    let payload = stored
        .strip_prefix(MAGIC)
        .filter(|payload| payload.len() >= 12 + 16)
        .ok_or_else(|| eyre!("Invalid protected file envelope."))?;
    let (nonce, ciphertext) = payload
        .split_first_chunk::<12>()
        .ok_or_else(|| eyre!("Invalid protected file nonce."))?;
    let key = Zeroizing::new(*blake3::keyed_hash(master, context).as_bytes());
    let cipher =
        Aes256Gcm::new_from_slice(key.as_ref()).map_err(|_| eyre!("Invalid protection key."))?;
    cipher
        .decrypt(
            nonce.into(),
            Payload {
                msg: ciphertext,
                aad: context,
            },
        )
        .map_err(|_| eyre!("Protected data authentication failed; no data was replaced."))
}

#[cfg(test)]
mod tests;
