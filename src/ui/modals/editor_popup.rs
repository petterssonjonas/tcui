use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Clear, Paragraph},
    Frame,
};
use unicode_width::UnicodeWidthStr;

use super::terminal_editor::{CloseRequest, TerminalEditorState};
use crate::vault::ValidatedEditorTarget;

pub struct EditorPopupState {
    artifact_name: String,
    chat_draft: Option<ChatDraftFile>,
    terminal: TerminalEditorState,
    close_hit_area: Option<Rect>,
}

impl EditorPopupState {
    pub fn new_chat_draft(content: &str, area: Rect) -> Result<Self, String> {
        let draft = ChatDraftFile::new(content).map_err(|error| error.to_string())?;
        let mut popup = Self::new(draft.path(), area)?;
        popup.chat_draft = Some(draft);
        Ok(popup)
    }

    pub fn new(path: &Path, area: Rect) -> Result<Self, String> {
        let artifact_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("editor")
            .to_string();
        let target =
            ValidatedEditorTarget::from_trusted_path(path).map_err(|error| error.to_string())?;
        let working_directory = std::env::current_dir()
            .map_err(|error| format!("Could not resolve editor working directory: {error}"))?;
        let terminal = TerminalEditorState::new(target, &working_directory, inner_area(area))
            .map_err(|error| error.to_string())?;
        Ok(Self {
            artifact_name,
            chat_draft: None,
            terminal,
            close_hit_area: None,
        })
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let theme = crate::theme::active_theme();
        let popup_area = popup_area(area);
        frame.render_widget(Clear, popup_area);
        frame.render_widget(
            Block::default().style(Style::default().bg(theme.panel)),
            popup_area,
        );
        self.terminal.render(frame, inner_area(area));

        let title_y = popup_area.y;
        let name_label = format!(" {} ", self.artifact_name);
        let name_width = artifact_title_width(&self.artifact_name);
        frame.render_widget(
            Paragraph::new(name_label).style(Style::default().fg(Color::Cyan)),
            Rect::new(popup_area.x + 1, title_y, name_width, 1),
        );
        let center_label = " Editor ";
        let center_x = popup_area
            .x
            .saturating_add(popup_area.width.saturating_sub(center_label.len() as u16) / 2);
        frame.render_widget(
            Paragraph::new(center_label).style(Style::default().fg(Color::Cyan)),
            Rect::new(center_x, title_y, center_label.len() as u16, 1),
        );
        let close_len = 3;
        let close_x = popup_area.x + popup_area.width.saturating_sub(close_len + 1);
        let close_area = Rect::new(close_x, title_y, close_len, 1);
        frame.render_widget(
            Paragraph::new("[x]").style(Style::default().fg(Color::Red)),
            close_area,
        );
        self.close_hit_area = Some(close_area);

        if self.terminal.close_is_pending() {
            let prompt = " Unsaved editor session - Enter/y close | Esc/n cancel ";
            frame.render_widget(
                Paragraph::new(prompt).style(Style::default().fg(Color::Yellow).bg(theme.panel)),
                Rect::new(
                    popup_area.x + 1,
                    popup_area
                        .y
                        .saturating_add(popup_area.height.saturating_sub(1)),
                    popup_area.width.saturating_sub(2),
                    1,
                ),
            );
        }
    }

    pub fn poll_output(&mut self) -> bool {
        self.terminal.poll_output()
    }

    pub fn is_done(&self) -> bool {
        self.terminal.is_done()
    }

    pub const fn close_is_pending(&self) -> bool {
        self.terminal.close_is_pending()
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        if self.terminal.close_is_pending() {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                    self.terminal.confirm_close();
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    self.terminal.cancel_close();
                }
                _ => {}
            }
            return true;
        }
        self.terminal.handle_key(key)
    }

    pub fn request_close(&mut self) -> CloseRequest {
        self.terminal.request_close()
    }

    pub fn send_scroll(&mut self, down: bool) {
        self.terminal.send_scroll(down);
    }

    pub const fn close_area(&self) -> Option<Rect> {
        self.close_hit_area
    }

    /// Retains ownership of the private draft until the caller has imported its contents.
    pub(crate) fn take_chat_draft(&mut self) -> Option<ChatDraftFile> {
        self.chat_draft.take()
    }
}

impl Drop for EditorPopupState {
    fn drop(&mut self) {
        self.terminal.confirm_close();
    }
}

pub(crate) struct ChatDraftFile {
    directory: PathBuf,
    path: PathBuf,
}

impl ChatDraftFile {
    fn new(content: &str) -> std::io::Result<Self> {
        use std::io::Write;
        let directory =
            std::env::temp_dir().join(format!("tcui-chat-draft-{:032x}", rand::random::<u128>()));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&directory)?;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let path = directory.join("draft.md");
        let mut file = match options.open(&path) {
            Ok(file) => file,
            Err(error) => {
                let _ = std::fs::remove_dir(&directory);
                return Err(error);
            }
        };
        let draft = Self { directory, path };
        file.write_all(content.as_bytes())?;
        Ok(draft)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ChatDraftFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        // Never recursively remove editor-created files or any directory we did not create.
        let _ = std::fs::remove_dir(&self.directory);
    }
}

pub fn popup_area_pub(area: Rect) -> Rect {
    popup_area(area)
}

fn inner_area(area: Rect) -> Rect {
    let popup = popup_area(area);
    Rect::new(
        popup.x + 1,
        popup.y + 2,
        popup.width.saturating_sub(2),
        popup.height.saturating_sub(3),
    )
}

fn artifact_title_width(name: &str) -> u16 {
    u16::try_from(UnicodeWidthStr::width(name))
        .unwrap_or(u16::MAX)
        .saturating_add(2)
}

fn popup_area(area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(6),
            Constraint::Percentage(88),
            Constraint::Length(2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25),
            Constraint::Percentage(75),
            Constraint::Percentage(0),
        ])
        .split(popup_layout[1])[1]
}

#[cfg(test)]
mod tests {
    use super::EditorPopupState;
    use ratatui::{backend::TestBackend, layout::Rect, Terminal};
    use std::io::Write;
    use std::time::Duration;
    use tokio::runtime::Builder;

    #[test]
    #[cfg(unix)]
    fn chat_draft_file_is_private_and_cleanup_never_removes_editor_owned_files() {
        use std::os::unix::fs::PermissionsExt;
        let draft = super::ChatDraftFile::new("private draft").expect("private draft");
        let path = draft.path().to_path_buf();
        let directory = path.parent().unwrap().to_path_buf();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "private draft");
        let editor_owned = directory.join("editor-owned-file");
        std::fs::write(&editor_owned, "keep").unwrap();
        drop(draft);
        assert!(!path.exists());
        assert_eq!(std::fs::read_to_string(&editor_owned).unwrap(), "keep");
        std::fs::remove_file(editor_owned).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn markdown_vault_baseline_editor_content_stays_inside_inner_gutter() {
        let path = std::env::temp_dir().join("編輯器.md");
        std::fs::write(&path, "editor body\n").expect("write temp file");
        let script = std::env::temp_dir().join("markdown-vault-baseline-editor.sh");
        let mut script_file = std::fs::File::create(&script).expect("create script");
        writeln!(script_file, "#!/bin/sh").expect("write script");
        writeln!(script_file, "printf 'gutterproof\\n'").expect("write script");
        drop(script_file);
        let _ = std::process::Command::new("chmod")
            .arg("+x")
            .arg(&script)
            .status();
        std::env::set_var("EDITOR", script.as_os_str());
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        runtime.block_on(async {
            let area = Rect::new(0, 0, 80, 24);
            let mut popup = EditorPopupState::new(&path, area).expect("popup state");
            let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
            tokio::time::sleep(Duration::from_millis(20)).await;
            let _ = popup.poll_output();
            terminal
                .draw(|frame| popup.render(frame, frame.area()))
                .expect("render popup");
            let buffer = terminal.backend().buffer();
            let popup_area = super::popup_area(buffer.area);
            let gutter_row = popup_area.y + 1;
            let inner_row = popup_area.y + 2;
            let inner_col = popup_area.x + 1;
            let body_x = (0..buffer.area.width)
                .find(|&x| buffer[(x, inner_row)].symbol().contains('g'))
                .expect("editor body");
            assert!(body_x >= inner_col);
            assert!(!buffer[(body_x, gutter_row)].symbol().contains('g'));
            assert!(buffer[(body_x, inner_row)].symbol().contains('g'));
        });
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&script);
        std::env::remove_var("EDITOR");
    }

    #[test]
    fn markdown_vault_baseline_editor_title_uses_terminal_column_width() {
        assert_eq!(super::artifact_title_width("編輯器.md"), 11);
    }

    #[test]
    fn markdown_vault_baseline_editor_handles_tiny_rectangles() {
        let path = std::env::temp_dir().join("markdown-vault-baseline-editor-small.txt");
        std::fs::write(&path, "x\n").expect("write temp file");
        let script = std::env::temp_dir().join("markdown-vault-baseline-editor-small.sh");
        let mut script_file = std::fs::File::create(&script).expect("create script");
        writeln!(script_file, "#!/bin/sh").expect("write script");
        writeln!(script_file, "printf 'gutterproof\\n'").expect("write script");
        drop(script_file);
        let _ = std::process::Command::new("chmod")
            .arg("+x")
            .arg(&script)
            .status();
        std::env::set_var("EDITOR", script.as_os_str());
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        runtime.block_on(async {
            let area = Rect::new(0, 0, 4, 2);
            let mut popup = EditorPopupState::new(&path, area).expect("popup state");
            let mut terminal = Terminal::new(TestBackend::new(4, 2)).expect("test terminal");
            tokio::time::sleep(Duration::from_millis(20)).await;
            let _ = popup.poll_output();
            terminal
                .draw(|frame| popup.render(frame, frame.area()))
                .expect("render popup");
        });
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&script);
        std::env::remove_var("EDITOR");
    }
}
