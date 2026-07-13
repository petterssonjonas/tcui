use super::KeyStoreError;
use cap_std::ambient_authority;
use cap_std::fs::Dir;
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Component, Path, PathBuf};

pub(super) fn parent_and_file_name(
    path: &Path,
) -> std::result::Result<(PathBuf, OsString), KeyStoreError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or(KeyStoreError::KeyPath)?;
    let file_name = path.file_name().ok_or(KeyStoreError::KeyPath)?;
    Ok((parent.to_path_buf(), file_name.to_os_string()))
}

#[cfg(unix)]
pub(super) fn open_parent(
    parent: &Path,
    create_missing: bool,
) -> std::result::Result<Option<Dir>, KeyStoreError> {
    let mut directory = if parent.is_absolute() {
        Dir::open_ambient_dir("/", ambient_authority()).map_err(|_| KeyStoreError::KeyPath)?
    } else {
        Dir::open_ambient_dir(".", ambient_authority()).map_err(|_| KeyStoreError::KeyPath)?
    };

    for component in parent.components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(name) => {
                match open_or_create_component(&directory, name, create_missing) {
                    Ok(next) => directory = next,
                    Err(KeyStoreError::Read) if !create_missing => return Ok(None),
                    Err(error) => return Err(error),
                }
            }
            Component::ParentDir | Component::Prefix(_) => return Err(KeyStoreError::UnsafePath),
        }
    }

    Ok(Some(directory))
}

#[cfg(unix)]
fn open_or_create_component(
    directory: &Dir,
    name: &OsStr,
    create_missing: bool,
) -> std::result::Result<Dir, KeyStoreError> {
    match open_directory_component(directory, name) {
        Ok(next) => Ok(next),
        Err(error) if error.kind() == io::ErrorKind::NotFound && !create_missing => {
            Err(KeyStoreError::Read)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if let Err(error) = directory.create_dir(Path::new(name)) {
                if error.kind() != io::ErrorKind::AlreadyExists {
                    return Err(KeyStoreError::CreateDirectory);
                }
            }
            open_directory_component(directory, name).map_err(|_| KeyStoreError::UnsafePath)
        }
        Err(_) => Err(KeyStoreError::UnsafePath),
    }
}

#[cfg(unix)]
fn open_directory_component(directory: &Dir, name: &OsStr) -> io::Result<Dir> {
    let descriptor = rustix::fs::openat(
        directory,
        Path::new(name),
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::DIRECTORY | rustix::fs::OFlags::NOFOLLOW,
        rustix::fs::Mode::empty(),
    )?;
    Ok(Dir::from(descriptor))
}

#[cfg(not(unix))]
pub(super) fn open_parent(
    parent: &Path,
    create_missing: bool,
) -> std::result::Result<Option<Dir>, KeyStoreError> {
    if create_missing {
        Dir::create_ambient_dir_all(parent, ambient_authority())
            .map_err(|_| KeyStoreError::CreateDirectory)?;
    }
    match Dir::open_ambient_dir(parent, ambient_authority()) {
        Ok(directory) => Ok(Some(directory)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(KeyStoreError::KeyPath),
    }
}

#[cfg(unix)]
pub(super) struct ParentIdentity {
    device: u64,
    inode: u64,
}

#[cfg(unix)]
impl ParentIdentity {
    pub(super) fn capture(directory: &Dir) -> std::result::Result<Self, KeyStoreError> {
        use cap_std::fs::MetadataExt;

        let metadata = directory
            .dir_metadata()
            .map_err(|_| KeyStoreError::ParentChanged)?;
        Ok(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }

    pub(super) fn matches(&self, parent: &Path) -> std::result::Result<(), KeyStoreError> {
        use cap_std::fs::MetadataExt;

        let directory = open_parent(parent, false).map_err(|_| KeyStoreError::ParentChanged)?;
        let Some(directory) = directory else {
            return Err(KeyStoreError::ParentChanged);
        };
        let metadata = directory
            .dir_metadata()
            .map_err(|_| KeyStoreError::ParentChanged)?;
        if metadata.dev() == self.device && metadata.ino() == self.inode {
            Ok(())
        } else {
            Err(KeyStoreError::ParentChanged)
        }
    }
}
