use std::path::{Path, PathBuf};

use super::VaultProfileUiError;
use crate::tui::components::SearchField;

mod filesystem;
mod navigation;
mod view;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DirectoryPickerAction {
    None,
    Confirm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PickerFocus {
    Path,
    List,
    Add,
}

#[derive(Debug, Clone)]
struct DirectoryEntry {
    label: String,
    path: PathBuf,
}

#[derive(Debug, Clone)]
pub(super) struct VaultDirectoryPickerState {
    cwd: PathBuf,
    entries: Vec<DirectoryEntry>,
    selected: Option<usize>,
    path_input: SearchField,
    focus: PickerFocus,
    path_pristine: bool,
}

impl VaultDirectoryPickerState {
    pub(super) fn new(start: &Path) -> Result<Self, VaultProfileUiError> {
        let cwd = canonical_directory(start)?;
        let entries = filesystem::directory_entries(&cwd)?;
        let mut path_input = SearchField::new();
        path_input.insert(&cwd.to_string_lossy());
        Ok(Self {
            cwd,
            entries,
            selected: None,
            path_input,
            focus: PickerFocus::Path,
            path_pristine: true,
        })
    }

    pub(super) fn selected_directory(&self) -> Result<PathBuf, VaultProfileUiError> {
        let selected = self
            .selected
            .and_then(|index| self.entries.get(index))
            .map_or(self.cwd.as_path(), |entry| entry.path.as_path());
        canonical_directory(selected)
    }

    fn set_directory(&mut self, path: &Path) -> Result<(), VaultProfileUiError> {
        let cwd = canonical_directory(path)?;
        let entries = filesystem::directory_entries(&cwd)?;
        self.cwd = cwd;
        self.entries = entries;
        self.selected = None;
        self.path_input.clear();
        self.path_input.insert(&self.cwd.to_string_lossy());
        self.path_pristine = true;
        Ok(())
    }

    fn visible_start(&self, height: usize) -> usize {
        let Some(selected) = self.selected else {
            return 0;
        };
        selected
            .saturating_add(1)
            .saturating_sub(height.max(1))
            .min(self.entries.len().saturating_sub(height))
    }
}

fn expand_tilde(path: &Path, home: Option<&Path>) -> PathBuf {
    let Some(text) = path.to_str() else {
        return path.to_path_buf();
    };
    match (text, home) {
        ("~", Some(home)) => home.to_path_buf(),
        (_, Some(home)) => text
            .strip_prefix("~/")
            .map_or_else(|| path.to_path_buf(), |relative| home.join(relative)),
        (_, None) => path.to_path_buf(),
    }
}

pub(super) fn directory_error(path: &Path, error: std::io::Error) -> VaultProfileUiError {
    VaultProfileUiError::DirectoryUnavailable {
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}

pub(super) fn canonical_directory(path: &Path) -> Result<PathBuf, VaultProfileUiError> {
    let metadata = std::fs::metadata(path).map_err(|error| directory_error(path, error))?;
    if !metadata.is_dir() {
        return Err(VaultProfileUiError::NotDirectory {
            path: path.to_path_buf(),
        });
    }
    std::fs::canonicalize(path).map_err(|error| directory_error(path, error))
}
