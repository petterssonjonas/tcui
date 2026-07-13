//! New ratatui-based TUI namespace. Submodules land in later todos.

pub mod components;
pub mod focus;
pub mod keybind_capture;
pub mod palette;
pub mod settings_panel;
pub mod shell;
pub mod status_bar;
mod status_bar_layout;

#[cfg(test)]
mod status_bar_tests;
