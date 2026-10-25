use std::fs::File;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use cap_std::fs::Dir;
use rustix::fs::{mkdirat, openat, Mode, OFlags};
use serde::{Deserialize, Serialize};

use super::{
    hash, invalid, native_root, Domain, Result, TransferError, MAX_FILES, MAX_FILE_BYTES,
    MAX_TOTAL_BYTES, METADATA,
};
use crate::protection;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ObservedFile {
    pub(super) path: PathBuf,
    pub(super) relative: PathBuf,
    pub(super) size: u64,
    pub(super) hash: [u8; 32],
}

/// Held only while stores/session guards are absent. Normal store sessions use
/// this same lock in shared mode; a competing live app therefore blocks transfer.
pub(super) fn exclusive() -> Result<File> {
    let directory = protection::open_directory(&protection::root()?, true)?;
    let descriptor = openat(
        &directory,
        "protection.lock",
        OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::from_bits_truncate(0o600),
    )
    .map_err(std::io::Error::from)?;
    let file = File::from(descriptor);
    if !file.metadata()?.is_file() {
        return invalid("protection lock is not a regular file");
    }
    file.try_lock().map_err(|error| {
        std::io::Error::other(format!("Close native stores before transfer: {error}"))
    })?;
    Ok(file)
}

pub(super) fn is_directory(path: &Path) -> Result<bool> {
    let (directory, name) = parent(path, false)?;
    let metadata = directory.symlink_metadata(&name)?;
    if metadata.is_symlink() || (!metadata.is_dir() && !metadata.is_file()) {
        return invalid("source must be a real directory or regular file");
    }
    Ok(metadata.is_dir())
}

/// Directory traversal is capability-based and never follows symlinks. Ignored
/// native Trash is inspected for its own type, but its bodies are not imported.
pub(super) fn scan(path: &Path, importing: bool) -> Result<Vec<ObservedFile>> {
    scan_with_limits(path, importing, MAX_FILE_BYTES, Some(MAX_TOTAL_BYTES))
}

pub(super) fn scan_with_limits(
    path: &Path,
    importing: bool,
    file_limit: u64,
    total_limit: Option<u64>,
) -> Result<Vec<ObservedFile>> {
    if !is_directory(path)? {
        let name = path
            .file_name()
            .ok_or_else(|| TransferError::Invalid("source has no filename".into()))?;
        let (directory, _) = parent(path, false)?;
        let (size, hash) = observe_at(&directory, name, file_limit)?;
        if total_limit.is_some_and(|limit| size > limit) {
            return invalid("transfer exceeds the total byte limit");
        }
        return Ok(vec![ObservedFile {
            path: path.to_path_buf(),
            relative: PathBuf::from(name),
            size,
            hash,
        }]);
    }
    let mut pending = vec![(protection::open_directory(path, false)?, PathBuf::new())];
    let mut files = Vec::new();
    let mut visited = 0usize;
    let mut total = 0u64;
    while let Some((directory, relative)) = pending.pop() {
        for entry in directory.entries()? {
            visited += 1;
            if visited > MAX_FILES * 2 {
                return invalid("transfer directory exceeds the entry limit");
            }
            let entry = entry?;
            let name = entry.file_name();
            let next = relative.join(&name);
            if next.components().count() > 64 || name.to_str().is_none() {
                return invalid("transfer directory path is too deep or not UTF-8");
            }
            let metadata = directory.symlink_metadata(&name)?;
            if metadata.is_symlink() || (!metadata.is_file() && !metadata.is_dir()) {
                return invalid("transfer source contains a symlink or special file");
            }
            if importing && name.as_encoded_bytes().starts_with(b".") {
                if relative.as_os_str().is_empty()
                    && metadata.is_dir()
                    && (name == ".Trash" || name == ".trash")
                {
                    continue;
                }
                if !(relative.as_os_str().is_empty() && name == METADATA && metadata.is_file()) {
                    return invalid(
                        "transfer source contains an unsupported hidden file or directory",
                    );
                }
            }
            if metadata.is_dir() {
                use cap_fs_ext::DirExt;
                pending.push((directory.open_dir_nofollow(&name)?, next));
            } else {
                if files.len() == MAX_FILES {
                    return invalid("transfer exceeds the file limit");
                }
                let (size, hash) = observe_at(&directory, &name, file_limit)?;
                total = total
                    .checked_add(size)
                    .ok_or_else(|| TransferError::Invalid("transfer size overflow".into()))?;
                if total_limit.is_some_and(|limit| total > limit) {
                    return invalid("transfer exceeds the total byte limit");
                }
                files.push(ObservedFile {
                    path: path.join(&next),
                    relative: next,
                    size,
                    hash,
                });
            }
        }
    }
    files.sort_by(|left, right| left.relative.cmp(&right.relative));
    Ok(files)
}

pub(super) fn scan_source(
    domain: Domain,
    source: &Path,
    same_native_root: bool,
) -> Result<Vec<ObservedFile>> {
    let mut files = if same_native_root {
        scan_with_limits(source, false, super::MAX_NATIVE_FILE_BYTES, None)?
    } else {
        scan(source, true)?
    };
    if same_native_root {
        let path = auxiliary(domain)?;
        if let Some((size, hash)) = observe_optional(&path, super::MAX_NATIVE_FILE_BYTES)? {
            files.push(ObservedFile {
                path,
                relative: PathBuf::from(".tcui-native-auxiliary"),
                size,
                hash,
            });
        }
    }
    if files.len() > MAX_FILES
        || (!same_native_root && files.iter().map(|file| file.size).sum::<u64>() > MAX_TOTAL_BYTES)
    {
        return invalid("native adoption sources exceed transfer bounds");
    }
    Ok(files)
}

pub(super) fn read_file(path: &Path) -> Result<Vec<u8>> {
    read_source_file(path, false)
}

/// Native adoption has no cumulative byte cap. One stored native file is bounded
/// to 1 GiB and authenticated independently; external imports retain 64 MiB.
pub(super) fn read_source_file(path: &Path, same_native_root: bool) -> Result<Vec<u8>> {
    let (directory, name) = parent(path, false)?;
    read_at(
        &directory,
        &name,
        if same_native_root {
            super::MAX_NATIVE_FILE_BYTES
        } else {
            MAX_FILE_BYTES
        },
    )
}

fn open_source(
    directory: &Dir,
    name: &std::ffi::OsStr,
    limit: u64,
) -> Result<(File, std::fs::Metadata)> {
    let descriptor = openat(
        directory,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(std::io::Error::from)?;
    let file = File::from(descriptor);
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > limit {
        return invalid("transfer source is not a bounded regular file");
    }
    Ok((file, metadata))
}

fn source_unchanged(
    before: &std::fs::Metadata,
    after: &std::fs::Metadata,
    size: u64,
) -> Result<()> {
    if size != before.len()
        || before.len() != after.len()
        || before.modified()? != after.modified()?
    {
        return invalid("transfer source changed while reading");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if before.dev() != after.dev()
            || before.ino() != after.ino()
            || before.ctime() != after.ctime()
            || before.ctime_nsec() != after.ctime_nsec()
        {
            return invalid("transfer source identity changed while reading");
        }
    }
    Ok(())
}

fn read_at(directory: &Dir, name: &std::ffi::OsStr, limit: u64) -> Result<Vec<u8>> {
    let (mut file, before) = open_source(directory, name, limit)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(before.len() as usize)
        .map_err(|_| {
            TransferError::Invalid("insufficient memory to authenticate this native file".into())
        })?;
    Read::by_ref(&mut file)
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    source_unchanged(&before, &file.metadata()?, bytes.len() as u64)?;
    Ok(bytes)
}

fn observe_at(directory: &Dir, name: &std::ffi::OsStr, limit: u64) -> Result<(u64, [u8; 32])> {
    let (mut file, before) = open_source(directory, name, limit)?;
    let mut buffer = [0u8; 64 * 1024];
    let mut hasher = blake3::Hasher::new();
    let mut size = 0u64;
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        size += count as u64;
        if size > limit {
            return invalid("transfer file grew beyond its byte limit");
        }
        hasher.update(&buffer[..count]);
    }
    source_unchanged(&before, &file.metadata()?, size)?;
    Ok((size, *hasher.finalize().as_bytes()))
}

fn observe_optional(path: &Path, limit: u64) -> Result<Option<(u64, [u8; 32])>> {
    let observed = (|| {
        let (directory, name) = parent(path, false)?;
        observe_at(&directory, &name, limit)
    })();
    match observed {
        Ok(value) => Ok(Some(value)),
        Err(TransferError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(TransferError::Protection(error))
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

pub(super) fn create_private_directory(path: &Path) -> Result<()> {
    let (directory, name) = parent(path, true)?;
    mkdirat(&directory, &name, Mode::from_bits_truncate(0o700)).map_err(std::io::Error::from)?;
    sync(&directory)
}

pub(super) fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    protection::write_new_plain(path, bytes)?;
    Ok(())
}

/// Copy retained stored bytes without decoding or buffering a whole native file.
/// Publish only after the bounded stream matches its observed identity and hash.
pub(super) fn copy_verified(
    source: &Path,
    destination: &Path,
    expected_hash: [u8; 32],
    limit: u64,
    replace: bool,
) -> Result<()> {
    let (source_directory, source_name) = parent(source, false)?;
    let (mut source_file, before) = open_source(&source_directory, &source_name, limit)?;
    let (directory, name) = parent(destination, true)?;
    match directory.symlink_metadata(&name) {
        Ok(_) if !replace => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "retained native destination already exists",
            )
            .into());
        }
        Ok(metadata) if !metadata.is_file() || metadata.is_symlink() => {
            return invalid("refusing to replace a non-regular native file");
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let temporary = format!(".tcui-transfer-{:032x}.tmp", rand::random::<u128>());
    let descriptor = openat(
        &directory,
        temporary.as_str(),
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::from_bits_truncate(0o600),
    )
    .map_err(std::io::Error::from)?;
    let mut target = File::from(descriptor);
    let result = (|| {
        let mut buffer = [0u8; 64 * 1024];
        let mut hasher = blake3::Hasher::new();
        let mut size = 0u64;
        loop {
            let count = source_file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            size += count as u64;
            if size > limit {
                return invalid("retained native file grew beyond its byte limit");
            }
            hasher.update(&buffer[..count]);
            target.write_all(&buffer[..count])?;
        }
        source_unchanged(&before, &source_file.metadata()?, size)?;
        if *hasher.finalize().as_bytes() != expected_hash {
            return Err(TransferError::SourceChanged(source.to_path_buf()));
        }
        target.sync_all()?;
        if replace {
            directory.rename(&temporary, &directory, &name)?;
        } else {
            #[cfg(any(target_os = "linux", target_os = "android"))]
            rustix::fs::renameat_with(
                &directory,
                temporary.as_str(),
                &directory,
                &name,
                rustix::fs::RenameFlags::NOREPLACE,
            )
            .map_err(std::io::Error::from)?;
            #[cfg(not(any(target_os = "linux", target_os = "android")))]
            {
                directory.hard_link(&temporary, &directory, &name)?;
                directory.remove_file(&temporary)?;
            }
        }
        sync(&directory)
    })();
    if result.is_err() {
        let _ = directory.remove_file(&temporary);
    }
    result
}

fn parent(path: &Path, create: bool) -> Result<(Dir, std::ffi::OsString)> {
    let parent = path
        .parent()
        .ok_or_else(|| TransferError::Invalid("transfer path has no parent".into()))?;
    let name = path
        .file_name()
        .ok_or_else(|| TransferError::Invalid("transfer path has no filename".into()))?;
    Ok((
        protection::open_directory(parent, create)?,
        name.to_os_string(),
    ))
}

fn sync(directory: &Dir) -> Result<()> {
    rustix::fs::fsync(directory).map_err(std::io::Error::from)?;
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Backup {
    version: u32,
    domain: Domain,
    id: String,
    files: Vec<BackupFile>,
    root_existed: bool,
    auxiliary_existed: bool,
    protected: bool,
    #[serde(default = "complete_by_default")]
    complete: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BackupFile {
    path: PathBuf,
    hash: [u8; 32],
}

fn complete_by_default() -> bool {
    true
}

fn pending_path(domain: Domain) -> Result<PathBuf> {
    Ok(protection::root()?.join(match domain {
        Domain::Vault => ".native-transfer-vault.pending",
        Domain::Memories => ".native-transfer-memories.pending",
        Domain::Chats => return invalid("Chats are not a transfer domain"),
    }))
}

fn backup_root(id: &str) -> Result<PathBuf> {
    if id.len() != 32 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return invalid("invalid replacement backup identity");
    }
    Ok(protection::root()?
        .join(".native-transfer-backups")
        .join(id))
}

fn auxiliary(domain: Domain) -> Result<PathBuf> {
    Ok(match domain {
        Domain::Vault => protection::root()?.join("chat-file-links/manifest.json"),
        // The cache is derived. Retain it in the backup, but discard it during
        // restoration so the restored documents determine all future results.
        Domain::Memories => protection::root()?.join("memory.sqlite3"),
        Domain::Chats => return invalid("Chats are not a transfer domain"),
    })
}

fn optional_file(path: &Path) -> Result<Option<Vec<u8>>> {
    match read_source_file(path, true) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(TransferError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(TransferError::Protection(error))
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn load_backup(domain: Domain) -> Result<Option<Backup>> {
    let Some(stored) = optional_file(&pending_path(domain)?)? else {
        return Ok(None);
    };
    let decoded = zeroize::Zeroizing::new(protection::decode(domain, &stored)?);
    let backup: Backup = serde_json::from_slice(&decoded)?;
    if backup.version != 1
        || backup.domain != domain
        || !backup.complete
        || backup.files.len() > MAX_FILES + 1
    {
        return invalid("invalid or incomplete replacement recovery metadata");
    }
    if backup.protected != protection::is_protected(domain)? {
        return invalid(
            "restore the original domain protection setting before replacement recovery",
        );
    }
    backup_root(&backup.id)?;
    let mut paths = std::collections::HashSet::new();
    for file in &backup.files {
        if file.path.as_os_str().is_empty()
            || file
                .path
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
            || !paths.insert(&file.path)
        {
            return invalid("unsafe or duplicate replacement recovery path");
        }
        if !file.path.starts_with("root") && file.path != Path::new("auxiliary") {
            return invalid("unexpected recovery file location");
        }
    }
    Ok(Some(backup))
}

pub(super) fn pending(domain: Domain) -> Result<Option<PathBuf>> {
    load_backup(domain)?
        .map(|backup| backup_root(&backup.id))
        .transpose()
}

pub(super) fn restore_saved(domain: Domain, path: &Path) -> Result<PathBuf> {
    let id = path
        .file_name()
        .and_then(|id| id.to_str())
        .ok_or_else(|| TransferError::Invalid("invalid replacement backup path".into()))?;
    if backup_root(id)? != path {
        return invalid("replacement backup is outside this instance");
    }
    let stored = read_source_file(&path.join("recovery.json"), true)?;
    let decoded = zeroize::Zeroizing::new(protection::decode(domain, &stored)?);
    let backup: Backup = serde_json::from_slice(&decoded)?;
    if backup.domain != domain
        || backup.id != id
        || backup.protected != protection::is_protected(domain)?
    {
        return invalid("backup domain or protection policy differs; restore the original domain protection setting first");
    }
    if !backup.complete {
        return invalid("the interrupted snapshot is incomplete and cannot replace native data");
    }
    // Publication is no-clobber. The normal recovery validator then checks every
    // descriptor path and content hash before it changes native storage.
    protection::write_new(domain, &pending_path(domain)?, &decoded)?;
    recover(domain)?
        .ok_or_else(|| TransferError::Invalid("replacement recovery pointer disappeared".into()))
}

/// The pointer is published only after every original file and directory entry
/// is durable. No target mutation occurs until this function returns success.
pub(super) fn snapshot(domain: Domain) -> Result<PathBuf> {
    snapshot_with_limit(domain, true, super::MAX_NATIVE_FILE_BYTES)
}

pub(super) fn snapshot_with_limit(
    domain: Domain,
    publish_pending: bool,
    file_limit: u64,
) -> Result<PathBuf> {
    let native = native_root(domain)?;
    let root_existed = match is_directory(&native) {
        Ok(true) => true,
        Ok(false) => return invalid("native store root is not a directory"),
        Err(TransferError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(error),
    };
    let auxiliary_path = auxiliary(domain)?;
    let auxiliary_observed = observe_optional(&auxiliary_path, file_limit)?;
    let auxiliary_existed = auxiliary_observed.is_some();
    let id = format!("{:032x}", rand::random::<u128>());
    let destination = backup_root(&id)?;
    create_private_directory(&destination)?;
    // Establish the domain before copying any retained native bytes. A security
    // transition can therefore protect even an interrupted snapshot safely.
    let initial = Backup {
        version: 1,
        domain,
        id: id.clone(),
        files: Vec::new(),
        root_existed,
        auxiliary_existed,
        protected: protection::is_protected(domain)?,
        complete: false,
    };
    let initial_bytes = zeroize::Zeroizing::new(serde_json::to_vec(&initial)?);
    protection::write_new(domain, &destination.join("recovery.json"), &initial_bytes)?;
    create_private_directory(&destination.join("root"))?;
    let mut files = Vec::new();
    if root_existed {
        for file in scan_with_limits(&native, false, file_limit, None)? {
            let relative = Path::new("root").join(&file.relative);
            copy_verified(
                &file.path,
                &destination.join(&relative),
                file.hash,
                file_limit,
                false,
            )?;
            files.push(BackupFile {
                path: relative,
                hash: file.hash,
            });
        }
    }
    if let Some((_, hash)) = auxiliary_observed {
        copy_verified(
            &auxiliary_path,
            &destination.join("auxiliary"),
            hash,
            file_limit,
            false,
        )?;
        files.push(BackupFile {
            path: PathBuf::from("auxiliary"),
            hash,
        });
    }
    let backup = Backup {
        version: 1,
        domain,
        id,
        files,
        root_existed,
        auxiliary_existed,
        protected: protection::is_protected(domain)?,
        complete: true,
    };
    let bytes = zeroize::Zeroizing::new(serde_json::to_vec(&backup)?);
    // A second, immutable descriptor makes completed backups independently
    // inspectable. It contains native data metadata only, never provider keys.
    protection::write(domain, &destination.join("recovery.json"), &bytes)?;
    if publish_pending {
        protection::write_new(domain, &pending_path(domain)?, &bytes)?;
    }
    Ok(destination)
}

pub(super) fn finish(domain: Domain) -> Result<()> {
    let path = pending_path(domain)?;
    let (directory, name) = parent(&path, false)?;
    directory.remove_file(name)?;
    sync(&directory)
}

/// Recovery itself is restartable: the pointer stays present until the restored
/// root and auxiliary metadata are durable. Partial new roots are never deleted.
pub(super) fn recover(domain: Domain) -> Result<Option<PathBuf>> {
    recover_with_limit(domain, super::MAX_NATIVE_FILE_BYTES)
}

pub(super) fn recover_with_limit(domain: Domain, file_limit: u64) -> Result<Option<PathBuf>> {
    let Some(backup) = load_backup(domain)? else {
        return Ok(None);
    };
    let saved = backup_root(&backup.id)?;
    // Authenticate the complete backup set before changing native paths.
    for file in &backup.files {
        let (directory, name) = parent(&saved.join(&file.path), false)?;
        if observe_at(&directory, &name, file_limit)?.1 != file.hash {
            return invalid("replacement backup has changed; no restoration was started");
        }
    }
    // Preserve the displaced source and its matching identity/cache metadata as
    // a complete independently recoverable snapshot before replacing either.
    // Repeated recovery may create another snapshot, but never loses the first.
    let displaced = snapshot_with_limit(domain, false, file_limit)?;
    let displaced_id = displaced
        .file_name()
        .and_then(|id| id.to_str())
        .ok_or_else(|| TransferError::Invalid("displaced backup has no identity".into()))?;
    protection::write_new(
        domain,
        &saved.join(format!("displaced-{displaced_id}.json")),
        &serde_json::to_vec(&displaced_id)?,
    )?;
    let restore_name = format!("restore-{:032x}", rand::random::<u128>());
    let restore = saved.join(&restore_name);
    create_private_directory(&restore)?;
    for file in &backup.files {
        let Ok(relative) = file.path.strip_prefix("root") else {
            continue;
        };
        if relative.as_os_str().is_empty() {
            return invalid("backup root cannot be a file");
        }
        copy_verified(
            &saved.join(&file.path),
            &restore.join(relative),
            file.hash,
            file_limit,
            false,
        )?;
    }
    let native = native_root(domain)?;
    let (native_parent, native_name) = parent(&native, true)?;
    let saved_directory = protection::open_directory(&saved, false)?;
    match native_parent.symlink_metadata(&native_name) {
        Ok(metadata) if metadata.is_dir() && !metadata.is_symlink() => {
            let retained = format!("interrupted-{:032x}", rand::random::<u128>());
            native_parent.rename(&native_name, &saved_directory, retained)?;
            sync(&saved_directory)?;
            sync(&native_parent)?;
        }
        Ok(_) => return invalid("refusing to restore over a non-directory native root"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    saved_directory.rename(&restore_name, &native_parent, &native_name)?;
    sync(&saved_directory)?;
    sync(&native_parent)?;
    let auxiliary_path = auxiliary(domain)?;
    if domain == Domain::Vault && backup.auxiliary_existed {
        let recorded = backup
            .files
            .iter()
            .find(|file| file.path == Path::new("auxiliary"))
            .ok_or_else(|| TransferError::Invalid("backup auxiliary metadata is missing".into()))?;
        copy_verified(
            &saved.join("auxiliary"),
            &auxiliary_path,
            recorded.hash,
            file_limit,
            true,
        )?;
    } else {
        remove_regular_if_present(&auxiliary_path)?;
    }
    if domain == Domain::Memories {
        for suffix in ["-wal", "-shm"] {
            let mut name = auxiliary_path.as_os_str().to_os_string();
            name.push(suffix);
            remove_regular_if_present(Path::new(&name))?;
        }
    }
    finish(domain)?;
    Ok(Some(saved))
}

fn remove_regular_if_present(path: &Path) -> Result<()> {
    let (directory, name) = parent(path, true)?;
    match directory.symlink_metadata(&name) {
        Ok(metadata) if metadata.is_file() && !metadata.is_symlink() => {
            directory.remove_file(name)?;
            sync(&directory)
        }
        Ok(_) => invalid("refusing to remove a non-regular cache or metadata file"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// Check framing-independent existence before protection changes keys or policy.
/// Even an unreadable or hostile pending pointer requires explicit recovery.
pub(crate) fn assert_no_pending_recovery() -> color_eyre::eyre::Result<()> {
    let directory = match protection::open_directory(&protection::root()?, false) {
        Ok(directory) => directory,
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Ok(())
        }
        Err(error) => return Err(error),
    };
    for name in [
        ".native-transfer-vault.pending",
        ".native-transfer-memories.pending",
    ] {
        match directory.symlink_metadata(name) {
            Ok(_) => color_eyre::eyre::bail!(
                "Recover the interrupted native transfer before changing protection: {name}"
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

/// Add retained bodies and their updated recovery descriptors to the caller's
/// authenticated protection transaction. Neither callback may publish a source.
/// Original/target body buffers are dropped after each staged entry.
pub(crate) fn stage_protection_migration(
    selected: Option<Domain>,
    target: protection::ProtectionPolicy,
    format_only: bool,
    encode: impl FnMut(Domain, &[u8]) -> color_eyre::eyre::Result<Vec<u8>>,
    stage: impl FnMut(Domain, &Path, &[u8], &[u8]) -> color_eyre::eyre::Result<()>,
) -> color_eyre::eyre::Result<()> {
    stage_protection_migration_impl(selected, target, format_only, false, encode, stage)
}

pub(crate) fn stage_password_rotation(
    target: protection::ProtectionPolicy,
    encode: impl FnMut(Domain, &[u8]) -> color_eyre::eyre::Result<Vec<u8>>,
    stage: impl FnMut(Domain, &Path, &[u8], &[u8]) -> color_eyre::eyre::Result<()>,
) -> color_eyre::eyre::Result<()> {
    stage_protection_migration_impl(None, target, false, true, encode, stage)
}

fn stage_protection_migration_impl(
    selected: Option<Domain>,
    target: protection::ProtectionPolicy,
    format_only: bool,
    password_rotation: bool,
    mut encode: impl FnMut(Domain, &[u8]) -> color_eyre::eyre::Result<Vec<u8>>,
    mut stage: impl FnMut(Domain, &Path, &[u8], &[u8]) -> color_eyre::eyre::Result<()>,
) -> color_eyre::eyre::Result<()> {
    assert_no_pending_recovery()?;
    let root = protection::root()?.join(".native-transfer-backups");
    let directory = match protection::open_directory(&root, false) {
        Ok(directory) => directory,
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Ok(())
        }
        Err(error) => return Err(error),
    };
    for entry in directory.entries()? {
        let entry = entry?;
        let name = entry.file_name();
        let metadata = directory.symlink_metadata(&name)?;
        if !metadata.is_dir() || metadata.is_symlink() {
            color_eyre::eyre::bail!("Native transfer retention contains an unsafe backup entry");
        }
        let id = name
            .to_str()
            .ok_or_else(|| color_eyre::eyre::eyre!("Invalid native backup identity"))?;
        let path = backup_root(id)?;
        let observed = scan_with_limits(&path, false, super::MAX_NATIVE_FILE_BYTES, None)?;
        let descriptor_path = path.join("recovery.json");
        let Some(original_descriptor) = optional_file(&descriptor_path)? else {
            if observed.is_empty() {
                continue;
            }
            color_eyre::eyre::bail!("Native backup {id} has no domain descriptor; recover it before changing protection");
        };
        let original_descriptor = zeroize::Zeroizing::new(original_descriptor);
        let descriptor_format = protection::inspect_bytes(&original_descriptor)?;
        if format_only && descriptor_format != protection::StoredFormat::LegacyInstanceBound {
            // An ordinary V1 unlock has no authority to request a foreign V2
            // password or rewrite already-portable retained history.
            continue;
        }
        let decoded_descriptor = zeroize::Zeroizing::new(match descriptor_format {
            protection::StoredFormat::Plaintext => original_descriptor.to_vec(),
            protection::StoredFormat::Portable { domain } => {
                if !password_rotation
                    && !should_migrate_backup(selected, target, format_only, domain)?
                {
                    continue;
                }
                protection::decode_for_explicit_decryption(domain, &original_descriptor)?
            }
            protection::StoredFormat::LegacyInstanceBound => {
                protection::decode_for_explicit_decryption(Domain::Vault, &original_descriptor)
                    .or_else(|_| {
                        protection::decode_for_explicit_decryption(
                            Domain::Memories,
                            &original_descriptor,
                        )
                    })?
            }
        });
        let mut backup: Backup = serde_json::from_slice(&decoded_descriptor)?;
        if backup.version != 1
            || backup.id != id
            || !matches!(backup.domain, Domain::Vault | Domain::Memories)
        {
            color_eyre::eyre::bail!("Invalid retained native backup descriptor");
        }
        if !password_rotation
            && !should_migrate_backup(selected, target, format_only, backup.domain)?
        {
            continue;
        }
        let mut listed = std::collections::HashSet::new();
        for file in &backup.files {
            if file
                .path
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
                || (!file.path.starts_with("root") && file.path != Path::new("auxiliary"))
                || file.path == Path::new("root")
                || !listed.insert(file.path.clone())
            {
                color_eyre::eyre::bail!("Unsafe or duplicate retained native backup path");
            }
        }
        for file in observed {
            if file.relative == Path::new("recovery.json") {
                continue;
            }
            let original = zeroize::Zeroizing::new(read_source_file(&file.path, true)?);
            if hash(&original) != file.hash {
                color_eyre::eyre::bail!(
                    "Retained native backup changed during protection preparation"
                );
            }
            let recorded = backup
                .files
                .iter_mut()
                .find(|recorded| recorded.path == file.relative);
            if recorded
                .as_ref()
                .is_some_and(|recorded| recorded.hash != file.hash)
            {
                color_eyre::eyre::bail!(
                    "Retained native backup content does not match its recovery descriptor"
                );
            }
            if password_rotation && !backup.protected {
                listed.remove(&file.relative);
                continue;
            }
            if format_only
                && (!backup.protected
                    || matches!(
                        protection::inspect_bytes(&original)?,
                        protection::StoredFormat::Portable { .. }
                    ))
            {
                listed.remove(&file.relative);
                continue;
            }
            let plaintext = zeroize::Zeroizing::new(if backup.protected {
                protection::decode_for_explicit_decryption(backup.domain, &original)?
            } else {
                // A plaintext backup may contain arbitrary binary user bytes,
                // including literal V1 prefixes. Its descriptor is authoritative.
                original.to_vec()
            });
            let encoded = zeroize::Zeroizing::new(encode(backup.domain, &plaintext)?);
            if encoded.len() as u64 > super::MAX_NATIVE_FILE_BYTES {
                color_eyre::eyre::bail!(
                    "Reencoded retained native file exceeds the 1 GiB stored-file limit"
                );
            }
            if let Some(recorded) = recorded {
                recorded.hash = hash(&encoded);
            }
            listed.remove(&file.relative);
            stage(backup.domain, &file.path, &original, &encoded)?;
        }
        if !listed.is_empty() {
            color_eyre::eyre::bail!("Retained native backup is missing a recorded body");
        }
        if password_rotation
            && !backup.protected
            && descriptor_format == protection::StoredFormat::Plaintext
        {
            continue;
        }
        backup.protected = if format_only || password_rotation {
            backup.protected
        } else {
            match backup.domain {
                Domain::Vault => target.vault,
                Domain::Memories => target.memories,
                Domain::Chats => false,
            }
        };
        let updated = zeroize::Zeroizing::new(serde_json::to_vec(&backup)?);
        let encoded = zeroize::Zeroizing::new(encode(backup.domain, &updated)?);
        stage(
            backup.domain,
            &descriptor_path,
            &original_descriptor,
            &encoded,
        )?;
    }
    Ok(())
}

fn should_migrate_backup(
    selected: Option<Domain>,
    target: protection::ProtectionPolicy,
    format_only: bool,
    domain: Domain,
) -> color_eyre::eyre::Result<bool> {
    if let Some(selected) = selected {
        return Ok(selected == domain);
    }
    let target_protected = match domain {
        Domain::Vault => target.vault,
        Domain::Memories => target.memories,
        Domain::Chats => false,
    };
    // Changing an unrelated option cannot authorize decryption of copied
    // encrypted history in a domain whose managed policy remains plaintext.
    let current_protected = protection::is_protected(domain)?;
    if !format_only
        && !protection::needs_portable_upgrade()?
        && current_protected == target_protected
    {
        return Ok(false);
    }
    Ok(target_protected || current_protected)
}
