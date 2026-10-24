use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::tui::components::SearchField;
use crate::vault::VaultEntryKind;

use super::interaction::VaultInputAction;
use super::state::VaultWorkspaceState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VaultMoveMode {
    Move,
    Rename,
}

#[derive(Debug, Clone)]
pub(crate) struct VaultMoveInput {
    pub(crate) mode: VaultMoveMode,
    pub(crate) source: PathBuf,
    pub(crate) destination: SearchField,
    pristine: bool,
}

impl VaultMoveInput {
    fn new(mode: VaultMoveMode, source: PathBuf) -> Self {
        let mut destination = SearchField::new();
        destination.insert(&source.to_string_lossy());
        Self {
            mode,
            source,
            destination,
            pristine: true,
        }
    }

    pub(crate) const fn title(&self) -> &'static str {
        match self.mode {
            VaultMoveMode::Move => " Move file ",
            VaultMoveMode::Rename => " Rename file ",
        }
    }

    fn destination_path(&self) -> Result<PathBuf, &'static str> {
        let raw = self.destination.query().trim();
        if raw.is_empty() {
            return Err("Destination path cannot be empty");
        }
        let entered = PathBuf::from(raw);
        let destination = match self.mode {
            VaultMoveMode::Move => entered,
            VaultMoveMode::Rename => rename_destination(&self.source, &entered)?,
        };
        if destination == self.source {
            return Err("Choose a different destination path");
        }
        Ok(destination)
    }
}

impl VaultWorkspaceState {
    pub(super) fn open_move_input(&mut self, mode: VaultMoveMode) -> VaultInputAction {
        let Some(node) = self.tree.selected_node() else {
            return VaultInputAction::Notice("Select a file to move".to_string());
        };
        if node.entry_kind != VaultEntryKind::File {
            return VaultInputAction::Notice("Select a file to move".to_string());
        }
        self.move_input = Some(VaultMoveInput::new(mode, node.path.clone()));
        VaultInputAction::None
    }

    pub(super) fn handle_move_input_key(&mut self, key: KeyEvent) -> VaultInputAction {
        let Some(input) = self.move_input.as_mut() else {
            return VaultInputAction::None;
        };
        match key.code {
            KeyCode::Esc => {
                self.move_input = None;
                VaultInputAction::None
            }
            KeyCode::Enter => match input.destination_path() {
                Ok(destination) => {
                    let source = input.source.clone();
                    self.move_input = None;
                    VaultInputAction::Move {
                        source,
                        destination,
                    }
                }
                Err(message) => VaultInputAction::Notice(message.to_string()),
            },
            KeyCode::Left => {
                input.pristine = false;
                input.destination.left();
                VaultInputAction::None
            }
            KeyCode::Right => {
                input.pristine = false;
                input.destination.right();
                VaultInputAction::None
            }
            KeyCode::Home => {
                input.pristine = false;
                input.destination.home();
                VaultInputAction::None
            }
            KeyCode::End => {
                input.pristine = false;
                input.destination.end();
                VaultInputAction::None
            }
            KeyCode::Backspace => {
                input.pristine = false;
                input.destination.backspace();
                VaultInputAction::None
            }
            KeyCode::Delete => {
                input.pristine = false;
                input.destination.delete();
                VaultInputAction::None
            }
            KeyCode::Char(character)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                if input.pristine {
                    input.destination.clear();
                    input.pristine = false;
                }
                input.destination.insert(character.encode_utf8(&mut [0; 4]));
                VaultInputAction::None
            }
            _ => VaultInputAction::None,
        }
    }

    #[cfg(test)]
    pub(crate) fn move_input_value_for_test(&self) -> Option<&str> {
        self.move_input
            .as_ref()
            .map(|input| input.destination.query())
    }
}

fn rename_destination(source: &Path, entered: &Path) -> Result<PathBuf, &'static str> {
    let source_parent = source.parent().unwrap_or_else(|| Path::new(""));
    if entered.components().count() == 1 {
        return Ok(source_parent.join(entered));
    }
    if entered.parent() == Some(source_parent) {
        Ok(entered.to_path_buf())
    } else {
        Err("Rename must keep the file in its current directory")
    }
}
