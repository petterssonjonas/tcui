use super::format::{self, LoadedKeysFile, StoredKeysFile};
use super::path_security::{open_parent, parent_and_file_name};
use super::KeyStoreError;
use cap_std::fs::OpenOptions;
use std::io::{self, Read};
use std::path::Path;

pub(super) fn read_file(path: &Path) -> std::result::Result<LoadedKeysFile, KeyStoreError> {
    let (parent, file_name) = parent_and_file_name(path)?;
    let Some(directory) = open_parent(&parent, false)? else {
        return Ok(LoadedKeysFile::Current(StoredKeysFile::default()));
    };
    match directory.symlink_metadata(Path::new(&file_name)) {
        Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => {
            return Err(KeyStoreError::UnsafePath)
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(LoadedKeysFile::Current(StoredKeysFile::default()))
        }
        Err(_) => return Err(KeyStoreError::Read),
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = directory
        .open_with(Path::new(&file_name), &options)
        .map_err(|_| KeyStoreError::Read)?;
    let mut content = zeroize::Zeroizing::new(String::new());
    file.read_to_string(&mut content)
        .map_err(|_| KeyStoreError::Read)?;
    format::parse(&content)
}
