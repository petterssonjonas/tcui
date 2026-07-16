use std::path::{Path, PathBuf};

use crossterm::event::KeyCode;
use ratatui::{layout::Rect, widgets::FrameExt as _, Frame};

use super::VaultProfileUiError;

#[derive(Debug, Clone)]
pub(super) struct VaultDirectoryPickerState {
    explorer: ratatui_explorer::FileExplorer,
}

impl VaultDirectoryPickerState {
    pub(super) fn new(start: &Path) -> Result<Self, VaultProfileUiError> {
        let explorer = ratatui_explorer::FileExplorerBuilder::default()
            .working_dir(start)
            .filter_map(|file| file.is_dir.then_some(file))
            .build()
            .map_err(|error| VaultProfileUiError::DirectoryUnavailable {
                path: start.to_path_buf(),
                message: error.to_string(),
            })?;
        Ok(Self { explorer })
    }

    pub(super) fn handle_key(&mut self, code: KeyCode) -> Result<(), VaultProfileUiError> {
        let input = match code {
            KeyCode::Up => ratatui_explorer::Input::Up,
            KeyCode::Down => ratatui_explorer::Input::Down,
            KeyCode::Left => ratatui_explorer::Input::Left,
            KeyCode::Right => ratatui_explorer::Input::Right,
            KeyCode::Home => ratatui_explorer::Input::Home,
            KeyCode::End => ratatui_explorer::Input::End,
            KeyCode::PageUp => ratatui_explorer::Input::PageUp,
            KeyCode::PageDown => ratatui_explorer::Input::PageDown,
            KeyCode::Char('.') => ratatui_explorer::Input::ToggleShowHidden,
            _ => ratatui_explorer::Input::None,
        };
        self.explorer
            .handle(input)
            .map_err(|error| VaultProfileUiError::DirectoryUnavailable {
                path: self.explorer.cwd().clone(),
                message: error.to_string(),
            })
    }

    pub(super) fn selected_directory(&self) -> Result<PathBuf, VaultProfileUiError> {
        let selected = self.explorer.current();
        if !selected.is_dir {
            return Err(VaultProfileUiError::NotDirectory {
                path: selected.path.clone(),
            });
        }
        canonical_directory(&selected.path)
    }

    pub(super) fn render(&self, frame: &mut Frame, area: Rect) {
        if area.width < 2 || area.height < 2 {
            return;
        }
        frame.render_widget_ref(self.explorer.widget(), area);
    }
}

pub(super) fn canonical_directory(path: &Path) -> Result<PathBuf, VaultProfileUiError> {
    let metadata =
        std::fs::metadata(path).map_err(|error| VaultProfileUiError::DirectoryUnavailable {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
    if !metadata.is_dir() {
        return Err(VaultProfileUiError::NotDirectory {
            path: path.to_path_buf(),
        });
    }
    std::fs::canonicalize(path).map_err(|error| VaultProfileUiError::DirectoryUnavailable {
        path: path.to_path_buf(),
        message: error.to_string(),
    })
}
