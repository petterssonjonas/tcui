use super::with_test_app_config;
use crate::config::AppConfig;
use crate::ui::artifact_sidebar::{
    ArtifactEntry, ArtifactHandle, ArtifactSidebar, ArtifactSidebarAction, ArtifactSidebarState,
    ArtifactSidebarVisibility,
};
use ratatui::{backend::TestBackend, buffer::Buffer, layout::Position, layout::Rect, Terminal};
use std::path::{Path, PathBuf};

mod geometry;
mod lifecycle;
mod rendering;

fn with_sidebar_app(label: &str, test: impl FnOnce(&mut crate::app::TuiApp, &Path)) {
    with_test_app_config(label, AppConfig::default(), |app| {
        let saved_dir = crate::protection::internal_vault_root();
        test(app, &saved_dir);
    });
}

fn create_link(
    app: &mut crate::app::TuiApp,
    name: &str,
    contents: &[u8],
) -> (ArtifactHandle, PathBuf) {
    let conversation_id = app.ui.tabs[0].active_conversation;
    let link = app
        .chat_files
        .create_for_chat(conversation_id, name, contents)
        .expect("create canonical chat file");
    let path = app.chat_files.path_for_link(&link);
    app.refresh_canonical_file_catalogs(0, conversation_id)
        .expect("refresh canonical catalogs");
    (
        ArtifactHandle::ChatFile {
            conversation_id,
            id: link.id,
        },
        path,
    )
}

fn render_sidebar(
    chat_files: &[ArtifactEntry],
    vault_files: &[ArtifactEntry],
    visibility: ArtifactSidebarVisibility,
    state: &mut ArtifactSidebarState,
    width: u16,
    height: u16,
) -> Terminal<TestBackend> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
    {
        let mut sidebar = ArtifactSidebar::new(chat_files, &[], vault_files, true, state)
            .with_visibility(visibility);
        terminal
            .draw(|frame| sidebar.render(frame, Rect::new(0, 0, width, height)))
            .expect("render artifact sidebar");
    }
    terminal
}

fn render_app(app: &mut crate::app::TuiApp, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
    terminal
        .draw(|frame| app.ui.render(frame))
        .expect("render application");
    terminal.backend().to_string()
}

fn label_position(buffer: &Buffer, label: &str) -> Position {
    let symbols = label
        .chars()
        .map(|character| character.to_string())
        .collect::<Vec<_>>();
    for y in buffer.area.y..buffer.area.bottom() {
        for x in buffer.area.x..buffer.area.right() {
            let fits = x.saturating_add(symbols.len() as u16) <= buffer.area.right();
            if fits
                && symbols
                    .iter()
                    .enumerate()
                    .all(|(offset, symbol)| buffer[(x + offset as u16, y)].symbol() == symbol)
            {
                return Position::new(x, y);
            }
        }
    }
    panic!("label {label:?} was not rendered");
}

const ALL_SECTIONS: ArtifactSidebarVisibility = ArtifactSidebarVisibility {
    memories: true,
    vault: true,
};
