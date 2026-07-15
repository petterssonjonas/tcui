use super::format::{self, LoadedKeysFile, StoredKeysFile};
#[cfg(unix)]
use super::path_security::ParentIdentity;
use super::path_security::{open_parent, parent_and_file_name};
use super::rollback::PreviousFileGuard;
use super::KeyStoreError;
#[cfg(unix)]
use cap_std::fs::OpenOptionsExt;
use cap_std::fs::{Dir, File, OpenOptions};
use std::ffi::OsStr;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

#[cfg(unix)]
const TEMPORARY_FILE_ATTEMPTS: u8 = 8;

#[cfg(all(test, unix))]
thread_local! {
    static INJECT_CLEANUP_DIRECTORY_CLONE_FAILURE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub(super) fn read_file(path: &Path) -> std::result::Result<LoadedKeysFile, KeyStoreError> {
    let (parent, file_name) = parent_and_file_name(path)?;
    let Some(directory) = open_parent(&parent, false)? else {
        return Ok(LoadedKeysFile::Current(StoredKeysFile::default()));
    };
    let Some(mut file) = open_existing_file(&directory, &file_name)? else {
        return Ok(LoadedKeysFile::Current(StoredKeysFile::default()));
    };
    let mut content = String::new();
    file.read_to_string(&mut content)
        .map_err(|_| KeyStoreError::Read)?;
    format::parse(&content)
}

pub(super) fn write_file(
    path: &Path,
    file: &StoredKeysFile,
) -> std::result::Result<(), KeyStoreError> {
    #[cfg(unix)]
    {
        write_file_inner(path, file, || {})
    }

    #[cfg(not(unix))]
    {
        let _ = (path, file);
        Err(KeyStoreError::UnsupportedPlatform)
    }
}

#[cfg(test)]
pub(super) fn write_file_with_before_rename<F>(
    path: &Path,
    file: &StoredKeysFile,
    before_rename: F,
) -> std::result::Result<(), KeyStoreError>
where
    F: FnOnce(),
{
    write_file_with_after_final_check(path, file, before_rename)
}

#[cfg(test)]
pub(super) fn write_file_with_after_final_check<F>(
    path: &Path,
    file: &StoredKeysFile,
    after_final_check: F,
) -> std::result::Result<(), KeyStoreError>
where
    F: FnOnce(),
{
    #[cfg(unix)]
    {
        write_file_inner(path, file, after_final_check)
    }

    #[cfg(not(unix))]
    {
        let _ = (path, file, after_final_check);
        Err(KeyStoreError::UnsupportedPlatform)
    }
}

#[cfg(unix)]
fn write_file_inner<F>(
    path: &Path,
    file: &StoredKeysFile,
    after_final_check: F,
) -> std::result::Result<(), KeyStoreError>
where
    F: FnOnce(),
{
    let (parent, file_name) = parent_and_file_name(path)?;
    let directory = open_parent(&parent, true)?.ok_or(KeyStoreError::CreateDirectory)?;
    let parent_identity = ParentIdentity::capture(&directory)?;
    reject_symlink(&directory, &file_name)?;
    let mut previous_file_guard = PreviousFileGuard::capture(&directory, &file_name)?;

    let contents = toml::to_string_pretty(file).map_err(|_| KeyStoreError::Serialize)?;
    let cleanup_directory = clone_cleanup_directory(&directory)?;
    let (temporary_name, mut temporary_file) = create_temporary_file(&directory)?;
    let mut temporary_file_guard = TemporaryFileGuard::new(cleanup_directory, temporary_name);

    temporary_file
        .write_all(contents.as_bytes())
        .map_err(|_| KeyStoreError::Write)?;
    temporary_file.flush().map_err(|_| KeyStoreError::Write)?;
    temporary_file.sync_all().map_err(|_| KeyStoreError::Sync)?;
    drop(temporary_file);

    parent_identity.matches(&parent)?;
    after_final_check();
    reject_symlink(&directory, &file_name)?;
    directory
        .rename(
            temporary_file_guard.path(),
            &directory,
            Path::new(&file_name),
        )
        .map_err(|_| KeyStoreError::Write)?;
    previous_file_guard.mark_replaced();

    if let Err(error) = parent_identity.matches(&parent) {
        previous_file_guard.restore()?;
        return Err(error);
    }

    sync_parent(&directory)?;
    previous_file_guard.commit()?;
    temporary_file_guard.disarm();
    Ok(())
}

#[cfg(unix)]
fn clone_cleanup_directory(directory: &Dir) -> std::result::Result<Dir, KeyStoreError> {
    #[cfg(test)]
    if INJECT_CLEANUP_DIRECTORY_CLONE_FAILURE.with(|failure| failure.replace(false)) {
        return Err(KeyStoreError::Write);
    }

    directory.try_clone().map_err(|_| KeyStoreError::Write)
}

#[cfg(all(test, unix))]
pub(super) fn inject_cleanup_directory_clone_failure() {
    INJECT_CLEANUP_DIRECTORY_CLONE_FAILURE.with(|failure| failure.set(true));
}

fn open_existing_file(
    directory: &Dir,
    file_name: &OsStr,
) -> std::result::Result<Option<File>, KeyStoreError> {
    match directory.symlink_metadata(Path::new(file_name)) {
        Ok(metadata) if metadata.file_type().is_symlink() => return Err(KeyStoreError::UnsafePath),
        Ok(metadata) if !metadata.is_file() => return Err(KeyStoreError::Read),
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(KeyStoreError::Read),
    }

    let mut options = OpenOptions::new();
    options.read(true);
    set_no_follow(&mut options);
    directory
        .open_with(Path::new(file_name), &options)
        .map(Some)
        .map_err(|_| KeyStoreError::Read)
}

#[cfg(unix)]
fn reject_symlink(directory: &Dir, file_name: &OsStr) -> std::result::Result<(), KeyStoreError> {
    match directory.symlink_metadata(Path::new(file_name)) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(KeyStoreError::UnsafePath),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(KeyStoreError::Write),
    }
}

#[cfg(unix)]
fn create_temporary_file(directory: &Dir) -> std::result::Result<(PathBuf, File), KeyStoreError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true).mode(0o600);
    set_no_follow(&mut options);

    for _ in 0..TEMPORARY_FILE_ATTEMPTS {
        let name = PathBuf::from(format!(".tcui-keys-{}.tmp", rand::random::<u64>()));
        match directory.open_with(&name, &options) {
            Ok(file) => return Ok((name, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(KeyStoreError::Write),
        }
    }

    Err(KeyStoreError::Write)
}

#[cfg(unix)]
fn set_no_follow(options: &mut OpenOptions) {
    options.custom_flags(libc::O_NOFOLLOW);
}

#[cfg(not(unix))]
fn set_no_follow(_: &mut OpenOptions) {}

#[cfg(unix)]
struct TemporaryFileGuard {
    directory: Dir,
    path: PathBuf,
    remove_on_drop: bool,
}

#[cfg(unix)]
impl TemporaryFileGuard {
    fn new(directory: Dir, path: PathBuf) -> Self {
        Self {
            directory,
            path,
            remove_on_drop: true,
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn disarm(&mut self) {
        self.remove_on_drop = false;
    }
}

#[cfg(unix)]
impl Drop for TemporaryFileGuard {
    fn drop(&mut self) {
        if self.remove_on_drop {
            let _ = self.directory.remove_file(&self.path);
        }
    }
}

#[cfg(unix)]
fn sync_parent(directory: &Dir) -> std::result::Result<(), KeyStoreError> {
    let syncable_directory = rustix::fs::openat(
        directory,
        ".",
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::DIRECTORY,
        rustix::fs::Mode::empty(),
    )
    .map_err(|_| KeyStoreError::Sync)?;
    rustix::fs::fsync(&syncable_directory).map_err(|_| KeyStoreError::Sync)
}
