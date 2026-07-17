#![cfg(test)]

use std::collections::HashSet;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{backend::TestBackend, layout::Rect, Terminal};

use super::{add_profile, TestDir};
use crate::config::vault_profiles::{VaultCatalog, VaultProfile};
use crate::tui::vault_profile_ui::{
    activate_profile, launcher_entry, LauncherActivation, LauncherEntry, VaultProfileManagerState,
};
use crate::ui::app_tabs::AppTabs;

#[test]
fn launcher_zero_one_many_profiles_choose_deterministically() {
    // Given
    let root = TestDir::new("launcher-counts");
    let first = root.child("first");
    let second = root.child("second");
    let mut tabs = AppTabs::default();
    let launcher_id = tabs.add_launcher();
    let mut catalog = VaultCatalog::default();

    // When / Then: zero.
    assert!(matches!(
        launcher_entry(&catalog, launcher_id, root.path()),
        Ok(LauncherEntry::Add(_))
    ));

    // When / Then: one.
    let first_id = add_profile(&mut catalog, "First", &first);
    assert_eq!(
        launcher_entry(&catalog, launcher_id, root.path()),
        Ok(LauncherEntry::Activate(first_id.clone()))
    );

    // When / Then: many, default preselected.
    let second_id = add_profile(&mut catalog, "Second", &second);
    catalog.set_default(&second_id).expect("set default");
    let LauncherEntry::Choose(chooser) =
        launcher_entry(&catalog, launcher_id, root.path()).expect("chooser")
    else {
        panic!("many profiles must open chooser");
    };
    assert_eq!(chooser.selected_profile_id(), Some(&second_id));
}

#[test]
fn launcher_replacement_preserves_id_and_existing_profile_is_focused() {
    // Given
    let root = TestDir::new("launcher-activation");
    let first = root.child("first");
    let second = root.child("second");
    let first_profile = VaultProfile::new("First", &first).expect("first profile");
    let second_profile = VaultProfile::new("Second", &second).expect("second profile");
    let mut tabs = AppTabs::default();
    let first_launcher = tabs.add_launcher();

    // When / Then: unopened profile replaces in place.
    assert_eq!(
        activate_profile(&mut tabs, first_launcher, first_profile.id.clone()),
        Ok(LauncherActivation::Replaced(first_launcher))
    );
    assert_eq!(tabs.active_id(), first_launcher);

    // When / Then: already-open profile closes redundant launcher and focuses existing tab.
    let second_launcher = tabs.add_launcher();
    let second_workspace = activate_profile(&mut tabs, second_launcher, second_profile.id.clone())
        .expect("open second profile");
    let LauncherActivation::Replaced(second_workspace_id) = second_workspace else {
        panic!("second profile should replace launcher");
    };
    let redundant = tabs.add_launcher();
    assert_eq!(
        activate_profile(&mut tabs, redundant, second_profile.id),
        Ok(LauncherActivation::FocusedExisting(second_workspace_id))
    );
    assert_eq!(tabs.active_id(), second_workspace_id);
    assert!(tabs.tab(redundant).is_none());
}

#[test]
fn directory_picker_render_is_safe_for_tiny_terminal() {
    // Given
    let root = TestDir::new("tiny-picker");
    let mut manager = VaultProfileManagerState::for_settings(VaultCatalog::default(), root.path())
        .expect("settings manager");
    manager.begin_add(root.path()).expect("open picker");
    let mut terminal = Terminal::new(TestBackend::new(1, 1)).expect("tiny terminal");

    // When / Then
    terminal
        .draw(|frame| manager.render(frame, Rect::new(0, 0, 1, 1)))
        .expect("tiny picker render must not panic");
}

#[test]
fn directory_picker_keyboard_add_confirms_current_directory() {
    // Given
    let root = TestDir::new("keyboard-add");
    let mut manager = VaultProfileManagerState::for_settings(VaultCatalog::default(), root.path())
        .expect("settings manager");
    manager.begin_add(root.path()).expect("open picker");

    // When
    manager.handle_key(
        KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
        &HashSet::new(),
    );
    manager.handle_key(
        KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
        &HashSet::new(),
    );
    manager.handle_key(
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        &HashSet::new(),
    );

    // Then
    assert_eq!(
        manager.phase(),
        crate::tui::vault_profile_ui::VaultProfilePhase::Name
    );
}

#[test]
fn directory_picker_escape_returns_to_profiles() {
    // Given
    let root = TestDir::new("keyboard-escape");
    let mut manager = VaultProfileManagerState::for_settings(VaultCatalog::default(), root.path())
        .expect("settings manager");
    manager.begin_add(root.path()).expect("open picker");

    // When
    let command = manager.handle_key(
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        &HashSet::new(),
    );

    // Then
    assert_eq!(
        command,
        crate::tui::vault_profile_ui::VaultProfileUiCommand::None
    );
    assert_eq!(
        manager.phase(),
        crate::tui::vault_profile_ui::VaultProfilePhase::Profiles
    );
}
