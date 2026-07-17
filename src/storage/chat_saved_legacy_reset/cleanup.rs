use std::io;
use std::path::Path;

pub(super) fn remove_legacy_cache(path: &Path) -> io::Result<()> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        std::fs::remove_file(path)?;
    } else {
        std::fs::remove_dir_all(path)?;
    }
    if let Some(parent) = path.parent() {
        sync_directory(parent)?;
    }
    Ok(())
}

pub(super) fn remove_saved_root_files(saved_root: &Path) -> io::Result<()> {
    let entries = match std::fs::read_dir(saved_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let mut removed = false;
    for entry in entries {
        let entry = entry?;
        let metadata = std::fs::symlink_metadata(entry.path())?;
        if metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            std::fs::remove_file(entry.path())?;
            removed = true;
        }
    }
    if removed {
        sync_directory(saved_root)?;
    }
    Ok(())
}

fn sync_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    std::fs::OpenOptions::new()
        .read(true)
        .open(path)?
        .sync_all()?;

    #[cfg(not(unix))]
    let _ = path;

    Ok(())
}
