use ratatui::{backend::TestBackend, Terminal};

use super::with_test_app;
use crate::config::vault_profiles::VaultCatalog;
use crate::tui::vault_profile_ui::{VaultProfileManagerState, VaultProfilePhase};

#[test]
fn vault_profile_picker_mouse_scroll_and_add_use_rendered_popup() {
    with_test_app("vault-picker-mouse", |app| {
        // Given
        let root = std::env::temp_dir().join(format!(
            "tcui-vault-picker-mouse-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let selected = root.join("alpha");
        std::fs::create_dir_all(&selected).expect("create picker directory");
        let mut manager = VaultProfileManagerState::for_settings(VaultCatalog::default(), &root)
            .expect("manager");
        manager.begin_add(&root).expect("open picker");
        app.ui.vault_profile_manager = Some(manager);
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("terminal");
        terminal
            .draw(|frame| app.ui.render(frame))
            .expect("render picker");

        // When
        app.handle_mouse_scroll(20, 8, crossterm::event::KeyModifiers::NONE, true);
        app.handle_mouse_scroll(20, 8, crossterm::event::KeyModifiers::NONE, true);
        app.handle_mouse_click(50, 24);

        // Then
        let manager = app
            .ui
            .vault_profile_manager
            .as_ref()
            .expect("manager remains open");
        assert_eq!(manager.phase(), VaultProfilePhase::Name);
        assert_eq!(manager.input(), "alpha");
        std::fs::remove_dir_all(root).expect("remove picker directory");
    });
}
