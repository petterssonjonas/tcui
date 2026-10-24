//! Portable Vault/Memory envelope version 2.
//!
//! The fixed header is magic/version, domain, Argon2id v0x13 parameters (three
//! little-endian u32 values), salt, wrapped-master nonce/ciphertext, and content
//! nonce. The complete header is AES-GCM AAD. The wrapping AAD is exactly the
//! credential vault's fixed application-data record metadata. No variable-size
//! header or attacker-selected KDF work is accepted.
use std::collections::HashMap;
use std::fmt;
use std::sync::{LazyLock, Mutex};

use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm,
};
use color_eyre::eyre::{bail, eyre, Result};
use zeroize::Zeroizing;

use super::{Domain, SESSIONS};
use crate::config::key_store::ProtectionWrapping;

pub(super) const MAGIC: &[u8] = b"TCUI-PORTABLE\x02";
const PREFIX: &[u8] = b"TCUI-PORTABLE";
const KDF: [u32; 3] = [65536, 3, 1];
const WRAPPING_LEN: usize = 12 + 16 + 12 + 48;
const HEADER_LEN: usize = MAGIC.len() + 1 + WRAPPING_LEN + 12;
/// Maximum bytes needed for a native-store adoption probe. Read at most this
/// prefix from a no-follow regular file; do not authenticate or decrypt it.
pub(crate) const NATIVE_STORE_ADOPTION_PROBE_LEN: usize = HEADER_LEN + 16;
const CACHE_LIMIT: usize = 32;

type ForeignKeys = HashMap<[u8; 32], Zeroizing<[u8; 32]>>;
#[derive(Default)]
struct ForeignSessions {
    generation: u64,
    roots: HashMap<std::path::PathBuf, ForeignKeys>,
}
static FOREIGN: LazyLock<Mutex<ForeignSessions>> =
    LazyLock::new(|| Mutex::new(ForeignSessions::default()));

/// Inspection does not authenticate bytes or unlock a store. Legacy data needs
/// its original credential vault and protection metadata; a password alone is insufficient.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StoredFormat {
    Plaintext,
    LegacyInstanceBound,
    Portable { domain: Domain },
}

/// Downcast a decode error to this type to request the source data password.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PortableUnlockNeeded {
    pub(crate) domain: Domain,
}
impl fmt::Display for PortableUnlockNeeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Portable {:?} data needs its source password.",
            self.domain
        )
    }
}
impl std::error::Error for PortableUnlockNeeded {}

/// A copied native store must adopt local encryption before normal management
/// can expose its body, build indexes, or change its files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NativeStoreAdoptionNeeded {
    pub(crate) domain: Domain,
}
impl fmt::Display for NativeStoreAdoptionNeeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Portable {:?} data needs protected native-store adoption before management access.",
            self.domain
        )
    }
}
impl std::error::Error for NativeStoreAdoptionNeeded {}

/// Native stores must scan their persisted content and metadata with this
/// helper before returning bodies, writing derived indexes, or mutating files.
/// Generic external import/decode remains independent of destination policy.
/// This only inspects framing; source unlock authenticates the content before
/// configure migrates it under the destination key and protected policy.
/// A prefix of up to `NATIVE_STORE_ADOPTION_PROBE_LEN` bytes is sufficient.
pub(crate) fn require_native_store_adoption(domain: Domain, stored: &[u8]) -> Result<()> {
    super::require(domain)?;
    if let StoredFormat::Portable { domain: actual } = inspect_bytes(stored)? {
        if actual != domain {
            bail!("Portable protected data domain does not match the requested store.");
        }
        if !super::is_protected(domain)? {
            return Err(NativeStoreAdoptionNeeded { domain }.into());
        }
    }
    Ok(())
}

/// A standalone V1 file does not contain password wrapping metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MissingLegacyWrapping;
impl fmt::Display for MissingLegacyWrapping {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Legacy V1 data needs the source credential vault and protection metadata; the file and password alone cannot recover its key.")
    }
}
impl std::error::Error for MissingLegacyWrapping {}

struct Envelope<'a> {
    domain: Domain,
    wrapping: ProtectionWrapping,
    header: &'a [u8],
    nonce: &'a [u8; 12],
    ciphertext: &'a [u8],
}

fn parse(stored: &[u8]) -> Result<Envelope<'_>> {
    if !stored.starts_with(MAGIC) || stored.len() < HEADER_LEN + 16 {
        bail!("Invalid or truncated portable protected envelope.");
    }
    let domain = match stored[MAGIC.len()] {
        1 => Domain::Vault,
        2 => Domain::Memories,
        _ => bail!("Unsupported portable protected data domain."),
    };
    let start = MAGIC.len() + 1;
    for (index, expected) in KDF.iter().enumerate() {
        if stored[start + index * 4..start + (index + 1) * 4] != expected.to_le_bytes() {
            bail!("Unsupported portable Argon2id parameters; no password work was performed.");
        }
    }
    let start = start + 12;
    let wrapping = ProtectionWrapping {
        salt: stored[start..start + 16].try_into()?,
        nonce: stored[start + 16..start + 28].try_into()?,
        ciphertext: stored[start + 28..start + 76].try_into()?,
    };
    Ok(Envelope {
        domain,
        wrapping,
        header: &stored[..HEADER_LEN],
        nonce: stored[HEADER_LEN - 12..HEADER_LEN].try_into()?,
        ciphertext: &stored[HEADER_LEN..],
    })
}

/// Recognizes portable encryption independently of the destination policy.
/// Unknown versions and damaged V2 headers are errors, never plaintext fallback.
/// Literal V1 prefixes remain policy-directed opaque data on ordinary reads.
pub(crate) fn inspect_bytes(stored: &[u8]) -> Result<StoredFormat> {
    if stored.starts_with(MAGIC) {
        return Ok(StoredFormat::Portable {
            domain: parse(stored)?.domain,
        });
    }
    if stored.starts_with(super::MAGIC) {
        return Ok(StoredFormat::LegacyInstanceBound);
    }
    if stored.starts_with(PREFIX) || stored.starts_with(b"TCUI-PROTECTED") {
        bail!("Unknown or truncated protected envelope version.");
    }
    Ok(StoredFormat::Plaintext)
}

fn context(domain: Domain) -> Result<&'static [u8]> {
    match domain {
        Domain::Vault => Ok(b"tcui/protection/v2/vault"),
        Domain::Memories => Ok(b"tcui/protection/v2/memories"),
        Domain::Chats => bail!("Portable envelopes are for Vault and Memories only."),
    }
}

pub(super) fn seal(
    domain: Domain,
    plaintext: &[u8],
    master: &[u8; 32],
    wrapping: &ProtectionWrapping,
) -> Result<Vec<u8>> {
    let key = Zeroizing::new(*blake3::keyed_hash(master, context(domain)?).as_bytes());
    let cipher =
        Aes256Gcm::new_from_slice(key.as_ref()).map_err(|_| eyre!("Invalid portable key."))?;
    let nonce = rand::random::<[u8; 12]>();
    let mut header = [0u8; HEADER_LEN];
    header[..MAGIC.len()].copy_from_slice(MAGIC);
    header[MAGIC.len()] = match domain {
        Domain::Vault => 1,
        Domain::Memories => 2,
        Domain::Chats => bail!("Portable envelopes are for Vault and Memories only."),
    };
    let start = MAGIC.len() + 1;
    for (index, value) in KDF.iter().enumerate() {
        header[start + index * 4..start + (index + 1) * 4].copy_from_slice(&value.to_le_bytes());
    }
    let start = start + 12;
    header[start..start + 16].copy_from_slice(&wrapping.salt);
    header[start + 16..start + 28].copy_from_slice(&wrapping.nonce);
    header[start + 28..start + 76].copy_from_slice(&wrapping.ciphertext);
    header[HEADER_LEN - 12..].copy_from_slice(&nonce);
    let ciphertext = cipher
        .encrypt(
            (&nonce).into(),
            Payload {
                msg: plaintext,
                aad: &header,
            },
        )
        .map_err(|_| eyre!("Failed to encrypt portable protected data."))?;
    let mut stored = Vec::with_capacity(HEADER_LEN + ciphertext.len());
    stored.extend_from_slice(&header);
    stored.extend_from_slice(&ciphertext);
    Ok(stored)
}

fn open_envelope(envelope: &Envelope<'_>, master: &[u8; 32]) -> Result<Vec<u8>> {
    let key = Zeroizing::new(*blake3::keyed_hash(master, context(envelope.domain)?).as_bytes());
    Aes256Gcm::new_from_slice(key.as_ref())
        .map_err(|_| eyre!("Invalid portable key."))?
        .decrypt(
            envelope.nonce.into(),
            Payload {
                msg: envelope.ciphertext,
                aad: envelope.header,
            },
        )
        .map_err(|_| eyre!("Portable data authentication failed; no data was changed."))
}

pub(super) fn open_with_key(domain: Domain, stored: &[u8], master: &[u8; 32]) -> Result<Vec<u8>> {
    let envelope = parse(stored)?;
    if envelope.domain != domain {
        bail!("Portable protected data domain does not match the requested store.");
    }
    open_envelope(&envelope, master)
}

fn wrapping_id(stored: &[u8]) -> [u8; 32] {
    let start = MAGIC.len() + 1;
    *blake3::hash(&stored[start..start + WRAPPING_LEN]).as_bytes()
}

pub(super) fn decode(domain: Domain, stored: &[u8]) -> Result<Vec<u8>> {
    let envelope = parse(stored)?;
    if envelope.domain != domain {
        bail!("Portable protected data domain does not match the requested store.");
    }
    let root = super::root()?;
    {
        let sessions = SESSIONS
            .lock()
            .map_err(|_| eyre!("Protection session lock is unavailable."))?;
        if let Some(session) = sessions.get(&root) {
            if session.wrapping == envelope.wrapping {
                return open_envelope(&envelope, &session.key);
            }
        }
    }
    let foreign = FOREIGN
        .lock()
        .map_err(|_| eyre!("Portable session lock is unavailable."))?;
    if let Some(key) = foreign
        .roots
        .get(&root)
        .and_then(|keys| keys.get(&wrapping_id(stored)))
    {
        return open_envelope(&envelope, key);
    }
    Err(PortableUnlockNeeded { domain }.into())
}

/// Authenticates one source file before caching its zeroized master for this data
/// root/session. This does not unlock the app, modify a source, or write a target.
/// Wrong passwords and damaged files do not change any key or policy.
pub(crate) fn unlock_foreign_file(domain: Domain, stored: &[u8], password: &str) -> Result<()> {
    super::require(domain)?;
    let _guard = super::io::session_lock()?;
    match inspect_bytes(stored)? {
        StoredFormat::LegacyInstanceBound => return Err(MissingLegacyWrapping.into()),
        StoredFormat::Plaintext => bail!("The selected file is not portable encrypted data."),
        StoredFormat::Portable { domain: actual } if actual != domain => {
            bail!("Portable protected data domain does not match the requested store.")
        }
        StoredFormat::Portable { .. } => {}
    }
    let root = super::root()?;
    let generation = FOREIGN
        .lock()
        .map_err(|_| eyre!("Portable session lock is unavailable."))?
        .generation;
    let envelope = parse(stored)?;
    let key = envelope.wrapping.unlock(password)?;
    let _verified = Zeroizing::new(open_envelope(&envelope, &key)?);
    super::require(domain)?;
    let mut foreign = FOREIGN
        .lock()
        .map_err(|_| eyre!("Portable session lock is unavailable."))?;
    if foreign.generation != generation || super::root()? != root {
        bail!("The protection session changed while authenticating; unlock the source again.");
    }
    let keys = foreign.roots.entry(root).or_default();
    let id = wrapping_id(stored);
    if keys.len() >= CACHE_LIMIT && !keys.contains_key(&id) {
        bail!("Too many source keys are open. Lock TCUI before opening another source.");
    }
    keys.insert(id, key);
    Ok(())
}

pub(super) fn clear() -> Result<()> {
    let mut foreign = FOREIGN
        .lock()
        .map_err(|_| eyre!("Portable session lock is unavailable."))?;
    foreign.generation = foreign.generation.wrapping_add(1);
    foreign.roots.remove(&super::root()?);
    Ok(())
}
