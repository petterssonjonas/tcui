use crossterm::event::KeyCode;
use ratatui::{backend::TestBackend, layout::Position, Terminal};

use super::view::picker_areas;
use super::{expand_tilde, DirectoryPickerAction, PickerFocus, VaultDirectoryPickerState};

struct TestDir(std::path::PathBuf);

impl TestDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "tcui-directory-picker-{label}-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&path).expect("create test directory");
        Self(path)
    }

    fn child(&self, name: &str) -> std::path::PathBuf {
        let path = self.0.join(name);
        std::fs::create_dir_all(&path).expect("create child directory");
        path
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn new_lists_parent_first_and_filters_files() {
    // Given
    let root = TestDir::new("picker-listing");
    root.child("notes");
    std::fs::write(root.path().join("ignore.md"), "not a directory").expect("write file");

    // When
    let picker = VaultDirectoryPickerState::new(root.path()).expect("picker");

    // Then
    assert_eq!(
        picker
            .entries
            .iter()
            .map(|entry| entry.label.as_str())
            .collect::<Vec<_>>(),
        vec!["../", "notes"]
    );
}

#[test]
fn down_and_enter_navigate_into_selected_directory() {
    // Given
    let root = TestDir::new("picker-keyboard");
    let notes = root.child("notes");
    let mut picker = VaultDirectoryPickerState::new(root.path()).expect("picker");

    // When
    picker.handle_key(KeyCode::Down).expect("select parent");
    picker.handle_key(KeyCode::Down).expect("select notes");
    picker.handle_key(KeyCode::Enter).expect("open notes");

    // Then
    assert_eq!(
        picker.cwd,
        std::fs::canonicalize(notes).expect("canonical notes")
    );
}

#[test]
fn enter_on_typed_path_moves_browser_to_directory() {
    // Given
    let root = TestDir::new("picker-path");
    let projects = root.child("projects");
    let mut picker = VaultDirectoryPickerState::new(root.path()).expect("picker");
    picker.path_input.clear();
    picker
        .path_input
        .insert(projects.to_str().expect("utf-8 test path"));
    picker.focus = PickerFocus::Path;

    // When
    picker.handle_key(KeyCode::Enter).expect("open typed path");

    // Then
    assert_eq!(
        picker.cwd,
        std::fs::canonicalize(projects).expect("canonical projects")
    );
}

#[test]
fn typing_replaces_initial_path_before_navigation() {
    // Given
    let home = dirs::home_dir().expect("home directory");
    let root = TestDir::new("picker-type-tilde");
    let mut picker = VaultDirectoryPickerState::new(root.path()).expect("picker");

    // When
    picker.handle_key(KeyCode::Char('~')).expect("type tilde");
    picker.handle_key(KeyCode::Enter).expect("open home");

    // Then
    assert_eq!(
        picker.cwd,
        std::fs::canonicalize(home).expect("canonical home")
    );
}

#[test]
fn expand_tilde_uses_supplied_home_directory() {
    // Given
    let home = std::path::Path::new("/tmp/tcui-home");

    // When
    let expanded = expand_tilde(std::path::Path::new("~/Documents"), Some(home));

    // Then
    assert_eq!(expanded, home.join("Documents"));
}

#[test]
fn parent_entry_moves_browser_one_directory_up() {
    // Given
    let root = TestDir::new("picker-parent");
    let child = root.child("child");
    let mut picker = VaultDirectoryPickerState::new(&child).expect("picker");
    picker.handle_key(KeyCode::Down).expect("select parent");

    // When
    picker.handle_key(KeyCode::Enter).expect("open parent");

    // Then
    assert_eq!(
        picker.cwd,
        std::fs::canonicalize(root.path()).expect("canonical root")
    );
}

#[test]
fn mouse_click_selects_directory_row() {
    // Given
    let root = TestDir::new("picker-click");
    let notes = root.child("notes");
    let area = ratatui::layout::Rect::new(0, 0, 60, 20);
    let areas = picker_areas(area).expect("picker areas");
    let mut picker = VaultDirectoryPickerState::new(root.path()).expect("picker");
    let notes_row = Position::new(areas.list.x, areas.list.y.saturating_add(1));

    // When
    let action = picker
        .handle_mouse_click(area, notes_row)
        .expect("click directory");

    // Then
    assert_eq!(action, DirectoryPickerAction::None);
    assert_eq!(picker.selected_directory().expect("selection"), notes);
}

#[test]
fn mouse_scroll_moves_list_selection() {
    // Given
    let root = TestDir::new("picker-scroll");
    root.child("alpha");
    let area = ratatui::layout::Rect::new(0, 0, 60, 20);
    let areas = picker_areas(area).expect("picker areas");
    let mut picker = VaultDirectoryPickerState::new(root.path()).expect("picker");
    let list_position = Position::new(areas.list.x, areas.list.y);

    // When
    picker.handle_mouse_scroll(area, list_position, true);
    picker.handle_mouse_scroll(area, list_position, true);

    // Then
    assert_eq!(picker.selected, Some(1));
}

#[test]
fn add_button_click_confirms_current_selection() {
    // Given
    let root = TestDir::new("picker-add");
    let area = ratatui::layout::Rect::new(0, 0, 60, 20);
    let areas = picker_areas(area).expect("picker areas");
    let mut picker = VaultDirectoryPickerState::new(root.path()).expect("picker");
    let add_position = Position::new(areas.add.x, areas.add.y);

    // When
    let action = picker
        .handle_mouse_click(area, add_position)
        .expect("click Add");

    // Then
    assert_eq!(action, DirectoryPickerAction::Confirm);
    assert_eq!(picker.selected_directory().expect("selection"), root.path());
}

#[test]
fn render_shows_path_parent_add_and_escape_hint() {
    // Given
    let root = TestDir::new("picker-render");
    let picker = VaultDirectoryPickerState::new(root.path()).expect("picker");
    let mut terminal = Terminal::new(TestBackend::new(60, 20)).expect("terminal");

    // When
    terminal
        .draw(|frame| picker.render(frame, frame.area()))
        .expect("render picker");

    // Then
    let buffer = terminal.backend().buffer();
    let rendered = (0..buffer.area.height)
        .flat_map(|y| (0..buffer.area.width).map(move |x| buffer[(x, y)].symbol()))
        .collect::<String>();
    assert!(rendered.contains("Path:"));
    assert!(rendered.contains("../"));
    assert!(rendered.contains("Add"));
    assert!(rendered.contains("Esc close"));
}
