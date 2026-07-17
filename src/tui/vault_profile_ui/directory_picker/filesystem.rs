use std::path::Path;

use super::{directory_error, DirectoryEntry};
use crate::tui::vault_profile_ui::VaultProfileUiError;

pub(super) fn directory_entries(path: &Path) -> Result<Vec<DirectoryEntry>, VaultProfileUiError> {
    let read_dir = std::fs::read_dir(path).map_err(|error| directory_error(path, error))?;
    let mut directories = Vec::new();
    for entry in read_dir {
        let entry = entry.map_err(|error| directory_error(path, error))?;
        if entry
            .metadata()
            .map_err(|error| directory_error(&entry.path(), error))?
            .is_dir()
        {
            directories.push(DirectoryEntry {
                label: entry.file_name().to_string_lossy().into_owned(),
                path: entry.path(),
            });
        }
    }
    directories.sort_by(|left, right| left.label.cmp(&right.label));
    let parent = path.parent().unwrap_or(path).to_path_buf();
    directories.insert(
        0,
        DirectoryEntry {
            label: "../".to_owned(),
            path: parent,
        },
    );
    Ok(directories)
}
