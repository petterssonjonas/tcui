use std::ffi::OsString;
use std::io;
use std::path::{Component, Path, PathBuf};

pub(super) fn normalize_absolute(path: &Path) -> io::Result<PathBuf> {
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("path is not absolute: {}", path.display()),
        ));
    }

    let lexical = lexical_normalize(path);
    let mut ancestor = lexical.as_path();
    let mut missing = Vec::<OsString>::new();
    loop {
        match std::fs::symlink_metadata(ancestor) {
            Ok(_) => break,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let name = ancestor.file_name().ok_or(error)?;
                missing.push(name.to_os_string());
                ancestor = ancestor.parent().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "path has no existing ancestor")
                })?;
            }
            Err(error) => return Err(error),
        }
    }

    let mut normalized = std::fs::canonicalize(ancestor)?;
    for component in missing.into_iter().rev() {
        normalized.push(component);
    }
    Ok(normalized)
}

pub(super) fn roots_overlap(left: &Path, right: &Path) -> bool {
    left.starts_with(right) || right.starts_with(left)
}

pub(super) fn is_filesystem_root(path: &Path) -> bool {
    path.parent().is_none()
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir | Component::Normal(_) => normalized.push(component.as_os_str()),
        }
    }
    normalized
}
