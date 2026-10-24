use std::io;
use std::path::{Component, Path, PathBuf};

use color_eyre::eyre::{bail, eyre, Result, WrapErr};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use super::{io as storage_io, Domain, Manifest, VERSION};
use crate::config::key_store::{CredentialStorage, KeyStore};
use crate::config::AppConfig;
use crate::storage::crypto::{decrypt_serialized, SharedKey};

const JOURNAL_CONTEXT: &[u8] = b"tcui/protection/v1/migration-journal";
const BACKUP_CONTEXT: &[u8] = b"tcui/protection/v1/migration-backup";
const STAGE_CONTEXT: &[u8] = b"tcui/protection/v1/migration-target";

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Pending {
    pub(super) target: Manifest,
    stage: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    version: u32,
    target: Manifest,
    entries: Vec<Entry>,
    convert_credentials: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    relative: PathBuf,
    original_hash: [u8; 32],
    target_hash: [u8; 32],
    staged_hash: [u8; 32],
    staged_wrapped: bool,
    backup_hash: [u8; 32],
}

fn pending_path() -> Result<PathBuf> {
    Ok(super::root()?.join("protection-pending.json"))
}

pub(super) fn pending() -> Result<Option<Pending>> {
    match storage_io::read_regular(&pending_path()?) {
        Ok(bytes) => {
            let pending: Pending = serde_json::from_slice(&bytes)?;
            if !valid_stage_name(&pending.stage) {
                bail!("Invalid protection recovery directory.");
            }
            pending.target.validate()?;
            Ok(Some(pending))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn prepare(
    old: Option<&Manifest>,
    target: &Manifest,
    key: Option<&[u8; 32]>,
) -> Result<()> {
    let root = super::root()?;
    cleanup_orphans(&root)?;
    let config = AppConfig::load()?;
    crate::vault::prepare_protection_migration(&config)?;
    crate::storage::saved_file_migration::migrate_saved_files(&config)?;
    crate::storage::chat_store::retire_legacy_database()?;
    let stage_name = format!(".protection-migration-{:032x}", rand::random::<u128>());
    let stage = root.join(&stage_name);
    storage_io::atomic_write(&stage.join("owner"), b"tcui-protection-staging-v1")?;
    let sources = sources(&root)?;
    let mut entries = Vec::with_capacity(sources.len());
    let mut legacy_key = None;
    for (path, domain) in sources {
        let original = Zeroizing::new(storage_io::read_regular(&path)?);
        let plaintext = Zeroizing::new(
            plaintext(old, domain, &path, &original, key, &mut legacy_key).wrap_err_with(|| {
                format!(
                    "Cannot migrate {}; original data is preserved",
                    path.display()
                )
            })?,
        );
        let stored = if target.policy.protects(domain) {
            super::seal(
                domain.context(),
                &plaintext,
                key.ok_or_else(|| eyre!("Missing migration key."))?,
            )?
        } else {
            plaintext.to_vec()
        };
        if target.policy.protects(domain) {
            let verified = Zeroizing::new(super::open(
                domain.context(),
                &stored,
                key.ok_or_else(|| eyre!("Missing migration key."))?,
            )?);
            if verified.as_slice() != plaintext.as_slice() {
                bail!("Protection migration verification failed before replacing data.");
            }
        }
        let backup = match key {
            Some(key) => super::seal(BACKUP_CONTEXT, &original, key)?,
            None => original.to_vec(),
        };
        let target_hash = *blake3::hash(&stored).as_bytes();
        let staged_wrapped = key.is_some() && !target.policy.protects(domain);
        let staged = if staged_wrapped {
            super::seal(
                STAGE_CONTEXT,
                &stored,
                key.ok_or_else(|| eyre!("Missing staging key."))?,
            )?
        } else {
            stored
        };
        let number = entries.len();
        storage_io::atomic_write(&stage.join(format!("{number}.new")), &staged)?;
        storage_io::atomic_write(&stage.join(format!("{number}.old")), &backup)?;
        entries.push(Entry {
            relative: path.strip_prefix(&root)?.to_path_buf(),
            original_hash: *blake3::hash(&original).as_bytes(),
            target_hash,
            staged_hash: *blake3::hash(&staged).as_bytes(),
            staged_wrapped,
            backup_hash: *blake3::hash(&backup).as_bytes(),
        });
    }
    let journal = Journal {
        version: VERSION,
        target: target.clone(),
        entries,
        convert_credentials: old
            .is_none_or(|old| old.policy.credentials != target.policy.credentials),
    };
    let journal_bytes = Zeroizing::new(serde_json::to_vec(&journal)?);
    let journal_bytes = match key {
        Some(key) => super::seal(JOURNAL_CONTEXT, &journal_bytes, key)?,
        None => journal_bytes.to_vec(),
    };
    storage_io::atomic_write(&stage.join("journal"), &journal_bytes)?;
    // No source replacement happens until the complete, durable recovery plan exists.
    storage_io::atomic_write(
        &pending_path()?,
        &serde_json::to_vec(&Pending {
            target: target.clone(),
            stage: stage_name,
        })?,
    )?;
    Ok(())
}

fn sources(root: &Path) -> Result<Vec<(PathBuf, Domain)>> {
    let mut sources = Vec::new();
    for (name, domain) in [
        ("chats", Domain::Chats),
        ("memories", Domain::Memories),
        ("vault", Domain::Vault),
        ("artifacts", Domain::Vault),
        ("chat-file-links", Domain::Vault),
        ("vault-backups", Domain::Vault),
        ("vault-ops", Domain::Vault),
    ] {
        let directory = root.join(name);
        match std::fs::symlink_metadata(&directory) {
            Ok(metadata) if metadata.is_symlink() || !metadata.is_dir() => {
                bail!(
                    "Refusing an unsafe protection migration root: {}",
                    directory.display()
                );
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        }
        for entry in walkdir::WalkDir::new(&directory).follow_links(false) {
            let entry = entry?;
            if entry.file_type().is_symlink() {
                bail!(
                    "Protection migration will not follow symbolic links: {}",
                    entry.path().display()
                );
            }
            if entry.file_type().is_file() {
                if entry.path() == root.join("chat-file-links/.lock") {
                    continue;
                }
                sources.push((entry.into_path(), domain));
            } else if !entry.file_type().is_dir() {
                bail!("Protection migration found a non-regular storage entry.");
            }
        }
    }
    sources.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(sources)
}

fn plaintext(
    old: Option<&Manifest>,
    domain: Domain,
    path: &Path,
    stored: &[u8],
    key: Option<&[u8; 32]>,
    legacy_key: &mut Option<SharedKey>,
) -> Result<Vec<u8>> {
    if old.is_some_and(|old| old.policy.protects(domain)) {
        return super::open(
            domain.context(),
            stored,
            key.ok_or_else(|| eyre!("Missing protected-data key."))?,
        );
    }
    let kind = match (domain, path.extension().and_then(|value| value.to_str())) {
        (Domain::Chats, Some("tcui-chat")) => Some("chat"),
        (Domain::Memories, Some("tcui-memory")) => Some("memory"),
        _ => None,
    };
    if let Some(kind) = kind {
        if stored.starts_with(b"enc:v1:") {
            if legacy_key.is_none() {
                *legacy_key = Some(SharedKey::load_from_path(&super::root()?.join("chat.key"))?);
            }
            let document: serde_json::Value = decrypt_serialized(
                legacy_key
                    .as_ref()
                    .ok_or_else(|| eyre!("Missing legacy data key."))?,
                kind,
                std::str::from_utf8(stored)?,
            )?;
            return Ok(serde_json::to_vec(&document)?);
        }
        let _: serde_json::Value = serde_json::from_slice(stored)?;
    }
    Ok(stored.to_vec())
}

pub(super) fn recover() -> Result<()> {
    if pending()?.is_none() {
        return Ok(());
    }
    let _lock = storage_io::migration_lock()?;
    let manifest = pending()?.ok_or_else(|| eyre!("Recovery plan disappeared."))?;
    if manifest.target.authentication.is_some() {
        super::with_key(|key| recover_locked(Some(key)))
    } else {
        recover_locked(None)
    }
}

pub(super) fn recover_locked(key: Option<&[u8; 32]>) -> Result<()> {
    let Some(pending) = pending()? else {
        return Ok(());
    };
    if let Some(key) = key {
        pending.target.authenticate(key)?;
    } else if pending.target.authentication.is_some() {
        bail!("The protection migration requires its original password.");
    }
    let root = super::root()?;
    let stage = root.join(&pending.stage);
    let stored_journal = storage_io::read_regular(&stage.join("journal"))?;
    let journal_bytes = Zeroizing::new(match key {
        Some(key) => super::open(JOURNAL_CONTEXT, &stored_journal, key)?,
        None => stored_journal,
    });
    let journal: Journal = serde_json::from_slice(&journal_bytes)?;
    if journal.version != VERSION {
        bail!("Unsupported protection recovery journal version.");
    }
    if journal.target != pending.target {
        bail!("Protection recovery policy does not match its authenticated journal.");
    }
    // Validate every staged file and current source before replacing any source.
    for (number, entry) in journal.entries.iter().enumerate() {
        validate_relative(&entry.relative)?;
        let stored = storage_io::read_regular(&stage.join(format!("{number}.new")))?;
        let backup = storage_io::read_regular(&stage.join(format!("{number}.old")))?;
        if *blake3::hash(&stored).as_bytes() != entry.staged_hash
            || *blake3::hash(&backup).as_bytes() != entry.backup_hash
        {
            bail!("Protection recovery staging was modified; original backups are retained.");
        }
        let target = staged_target(entry, stored, key)?;
        if *blake3::hash(&target).as_bytes() != entry.target_hash {
            bail!("Protection recovery target verification failed.");
        }
        let original = Zeroizing::new(match key {
            Some(key) => super::open(BACKUP_CONTEXT, &backup, key)?,
            None => backup,
        });
        if *blake3::hash(&original).as_bytes() != entry.original_hash {
            bail!("Protection recovery backup verification failed.");
        }
        let current = storage_io::read_regular(&root.join(&entry.relative))?;
        let hash = *blake3::hash(&current).as_bytes();
        if hash != entry.original_hash && hash != entry.target_hash {
            bail!("A file changed during protection migration: {}. Both migration copies are retained.", entry.relative.display());
        }
    }
    for (number, entry) in journal.entries.iter().enumerate() {
        install_entry(&root, &stage, number, entry, key)?;
    }
    let storage = if pending.target.policy.credentials {
        CredentialStorage::Encrypted
    } else {
        CredentialStorage::Plaintext
    };
    if journal.convert_credentials {
        KeyStore::set_storage_preference(&AppConfig::default(), storage)?;
    } else {
        KeyStore::set_storage_default(&AppConfig::default(), storage)?;
    }
    clear_derived_memory_indexes()?;
    storage_io::atomic_write(
        &super::manifest_path()?,
        &serde_json::to_vec(&pending.target)?,
    )?;
    // Once this durable marker is gone, every source is in the published format.
    std::fs::remove_file(pending_path()?)?;
    sync_root(&root)?;
    // This is our own verified staging, not a user-supplied data directory.
    cleanup_stage(&root, &stage)?;
    Ok(())
}

fn validate_relative(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("Invalid relative path in protection recovery plan.");
    }
    let allowed = [
        "chats",
        "memories",
        "vault",
        "artifacts",
        "chat-file-links",
        "vault-backups",
        "vault-ops",
    ];
    if !path.components().next().is_some_and(|component| {
        allowed
            .iter()
            .any(|allowed| component.as_os_str() == *allowed)
    }) {
        bail!("Protection recovery path is outside the managed data stores.");
    }
    Ok(())
}

fn clear_derived_memory_indexes() -> Result<()> {
    let mut databases = vec![super::root()?.join("memory.sqlite3")];
    #[cfg(feature = "memory")]
    databases.push(crate::memory::MemoryStore::legacy_cache_path());
    #[cfg(not(feature = "memory"))]
    if let Some(cache) = dirs::cache_dir() {
        databases.push(cache.join("tcui/memory.sqlite3"));
    }
    for database in databases {
        let parent = database
            .parent()
            .ok_or_else(|| eyre!("Memory cache has no parent directory."))?;
        let directory = match storage_io::directory(parent, false) {
            Ok(directory) => directory,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        let filename = database
            .file_name()
            .ok_or_else(|| eyre!("Memory cache has no filename."))?;
        for suffix in ["", "-wal", "-shm", "-journal"] {
            let mut name = filename.to_os_string();
            name.push(suffix);
            match directory.symlink_metadata(&name) {
                Ok(metadata) if metadata.is_file() && !metadata.is_symlink() => {
                    directory.remove_file(&name)?
                }
                Ok(_) => bail!("Refusing an unsafe memory cache entry."),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        rustix::fs::fsync(&directory)?;
    }
    Ok(())
}

fn sync_root(root: &Path) -> Result<()> {
    let directory = std::fs::File::open(root)?;
    directory.sync_all()?;
    Ok(())
}

fn staged_target(entry: &Entry, stored: Vec<u8>, key: Option<&[u8; 32]>) -> Result<Vec<u8>> {
    if entry.staged_wrapped {
        super::open(
            STAGE_CONTEXT,
            &stored,
            key.ok_or_else(|| eyre!("Recovery staging requires its original password."))?,
        )
    } else {
        Ok(stored)
    }
}

fn install_entry(
    root: &Path,
    stage: &Path,
    number: usize,
    entry: &Entry,
    key: Option<&[u8; 32]>,
) -> Result<()> {
    validate_relative(&entry.relative)?;
    let path = root.join(&entry.relative);
    let current = storage_io::read_regular(&path)?;
    let hash = *blake3::hash(&current).as_bytes();
    if hash == entry.target_hash {
        return Ok(());
    }
    if hash != entry.original_hash {
        bail!("A file changed after protection recovery preflight: {}. No conflicting data was overwritten.", entry.relative.display());
    }
    let staged = storage_io::read_regular(&stage.join(format!("{number}.new")))?;
    if *blake3::hash(&staged).as_bytes() != entry.staged_hash {
        bail!("Protection recovery staging changed after preflight.");
    }
    let target = Zeroizing::new(staged_target(entry, staged, key)?);
    if *blake3::hash(&target).as_bytes() != entry.target_hash {
        bail!("Protection recovery target changed after preflight.");
    }
    storage_io::atomic_write(&path, &target)?;
    Ok(())
}

fn valid_stage_name(name: &str) -> bool {
    name.strip_prefix(".protection-migration-")
        .is_some_and(|suffix| {
            suffix.len() == 32 && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
}

fn atomic_temporary(name: &str) -> bool {
    name.strip_prefix(".tcui-protection-")
        .and_then(|name| name.strip_suffix(".tmp"))
        .is_some_and(|suffix| {
            suffix.len() == 32 && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
}

fn cleanup_orphans(root: &Path) -> Result<()> {
    let directory = storage_io::directory(root, false)?;
    for entry in directory.entries()? {
        let entry = entry?;
        let name = entry.file_name();
        if !name.to_str().is_some_and(valid_stage_name) {
            continue;
        }
        if !directory.symlink_metadata(&name)?.is_dir() {
            bail!(
                "Unverified protection staging prevents a safe cutover; preserve and inspect {}.",
                root.join(&name).display()
            );
        }
        cleanup_stage(root, &root.join(name))?;
    }
    Ok(())
}

fn cleanup_stage(root: &Path, stage: &Path) -> Result<()> {
    let directory = storage_io::directory(stage, false)?;
    let names = directory
        .entries()?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<std::io::Result<Vec<_>>>()?;
    let owner = match storage_io::read_regular(&stage.join("owner")) {
        Ok(owner) if owner == b"tcui-protection-staging-v1" => true,
        Ok(_) => bail!(
            "Unverified protection staging owner at {}; no data was removed.",
            stage.display()
        ),
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => return Err(error.into()),
    };
    for name in &names {
        let Some(text) = name.to_str() else {
            bail!(
                "Unknown protection staging entry; preserve and inspect {}.",
                stage.display()
            );
        };
        let numbered = text
            .strip_suffix(".new")
            .or_else(|| text.strip_suffix(".old"))
            .is_some_and(|number| {
                !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
            });
        let metadata = directory.symlink_metadata(name)?;
        if !metadata.is_file()
            || metadata.is_symlink()
            || !(text == "owner" || text == "journal" || numbered || atomic_temporary(text))
        {
            bail!(
                "Unknown protection staging data at {}; refusing to claim a clean cutover.",
                stage.display()
            );
        }
        if !owner
            && !(atomic_temporary(text)
                && storage_io::read_regular(&stage.join(name))? == b"tcui-protection-staging-v1")
        {
            bail!("Protection staging lost its ownership marker at {}; retain it for recovery before enabling protection.", stage.display());
        }
    }
    // Keep the ownership marker until every payload deletion is durable.
    for name in &names {
        if name != "owner" {
            directory.remove_file(name)?;
        }
    }
    rustix::fs::fsync(&directory)?;
    if owner {
        directory.remove_file("owner")?;
        rustix::fs::fsync(&directory)?;
    }
    let parent = storage_io::directory(root, false)?;
    parent.remove_dir(
        stage
            .file_name()
            .ok_or_else(|| eyre!("Invalid staging directory."))?,
    )?;
    rustix::fs::fsync(&parent)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (PathBuf, PathBuf, Entry) {
        let root = std::env::temp_dir().join(format!(
            "tcui-recovery-install-{:032x}",
            rand::random::<u128>()
        ));
        let stage = root.join("stage");
        storage_io::atomic_write(&root.join("vault/note.md"), b"original")
            .expect("original source");
        storage_io::atomic_write(&stage.join("0.new"), b"target").expect("staged target");
        let entry = Entry {
            relative: PathBuf::from("vault/note.md"),
            original_hash: *blake3::hash(b"original").as_bytes(),
            target_hash: *blake3::hash(b"target").as_bytes(),
            staged_hash: *blake3::hash(b"target").as_bytes(),
            staged_wrapped: false,
            backup_hash: [0; 32],
        };
        (root, stage, entry)
    }

    #[test]
    fn install_refuses_source_changes_after_preflight() {
        let (root, stage, entry) = fixture();
        storage_io::atomic_write(&root.join(&entry.relative), b"concurrent change")
            .expect("external edit");
        assert!(install_entry(&root, &stage, 0, &entry, None).is_err());
        assert_eq!(
            storage_io::read_regular(&root.join(&entry.relative)).expect("preserved edit"),
            b"concurrent change"
        );
        std::fs::remove_dir_all(root).expect("fixture cleanup");
    }

    #[test]
    fn install_refuses_staging_changes_after_preflight() {
        let (root, stage, entry) = fixture();
        storage_io::atomic_write(&stage.join("0.new"), b"tampered target").expect("staging edit");
        assert!(install_entry(&root, &stage, 0, &entry, None).is_err());
        assert_eq!(
            storage_io::read_regular(&root.join(&entry.relative)).expect("original retained"),
            b"original"
        );
        std::fs::remove_dir_all(root).expect("fixture cleanup");
    }
}
