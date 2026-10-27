use std::collections::BTreeMap;
use std::path::PathBuf;

use ratatui::{prelude::*, widgets::*, Frame};

use super::modals::terminal_editor::TerminalEditorState;
use crate::config::tui::TerminalPlacement;

pub struct TerminalSession {
    pub terminal: TerminalEditorState,
    pub cwd: PathBuf,
    pub host: String,
}

impl TerminalSession {
    pub fn directory(&self) -> PathBuf {
        self.terminal
            .current_directory()
            .unwrap_or_else(|| self.cwd.clone())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalControl {
    Tab,
    Right,
    Bottom,
    Focus,
    New,
    Close,
}

pub struct TerminalWorkspace {
    pub sessions: BTreeMap<u64, TerminalSession>,
    pub split_session: Option<u64>,
    pub placement: TerminalPlacement,
    pub focused: bool,
    pub show_cursor: bool,
    pub show_helpers: bool,
    pub ratio: u16,
    pub dragging: bool,
    pub divider: Option<Rect>,
    pub split_bounds: Option<Rect>,
    pub body: Option<(u64, Rect)>,
    pub controls: Vec<(Rect, u64, TerminalControl)>,
    pub close_pending: Option<u64>,
    pub close_areas: Option<super::ModalAreas>,
    next_id: u64,
}

impl Default for TerminalWorkspace {
    fn default() -> Self {
        Self {
            sessions: BTreeMap::new(),
            split_session: None,
            placement: TerminalPlacement::Bottom,
            focused: false,
            ratio: 55,
            show_cursor: true,
            show_helpers: true,
            dragging: false,
            divider: None,
            split_bounds: None,
            body: None,
            controls: Vec::new(),
            close_pending: None,
            close_areas: None,
            next_id: 1,
        }
    }
}

impl TerminalWorkspace {
    pub fn insert(&mut self, session: TerminalSession) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.sessions.insert(id, session);
        id
    }

    pub fn clear_areas(&mut self) {
        self.divider = None;
        self.split_bounds = None;
        self.body = None;
        self.controls.clear();
        self.close_areas = None;
    }

    pub fn has_running(&self) -> bool {
        self.sessions
            .values()
            .any(|session| !session.terminal.is_done())
    }

    pub fn split_chat(&mut self, frame: &mut Frame, area: Rect) -> Rect {
        if area.width < 3 || area.height < 3 {
            return area;
        }
        let Some(id) = self
            .split_session
            .filter(|id| self.sessions.contains_key(id))
        else {
            return area;
        };
        if matches!(self.placement, TerminalPlacement::Tab) {
            return area;
        }
        self.split_bounds = Some(area);
        let (chat, divider, terminal) = match self.placement {
            TerminalPlacement::Right => {
                let width = ((u32::from(area.width) * u32::from(self.ratio)) / 100) as u16;
                let width = width.clamp(1, area.width.saturating_sub(2).max(1));
                (
                    Rect::new(area.x, area.y, width, area.height),
                    Rect::new(area.x + width, area.y, 1, area.height),
                    Rect::new(
                        area.x + width + 1,
                        area.y,
                        area.width.saturating_sub(width + 1),
                        area.height,
                    ),
                )
            }
            _ => {
                let height = ((u32::from(area.height) * u32::from(self.ratio)) / 100) as u16;
                let minimum_terminal = if area.width >= 46 { 5 } else { 6 };
                let maximum_chat = area.height.saturating_sub(minimum_terminal + 1).max(1);
                let height = height.clamp(1, maximum_chat);
                (
                    Rect::new(area.x, area.y, area.width, height),
                    Rect::new(area.x, area.y + height, area.width, 1),
                    Rect::new(
                        area.x,
                        area.y + height + 1,
                        area.width,
                        area.height.saturating_sub(height + 1),
                    ),
                )
            }
        };
        self.divider = Some(divider);
        let theme = crate::theme::active_theme();
        frame.render_widget(
            Block::default().style(Style::default().bg(theme.border)),
            divider,
        );
        self.render_session(frame, terminal, id);
        chat
    }

    pub fn drag(&mut self, pos: Position) {
        let Some(bounds) = self.split_bounds else {
            return;
        };
        let (offset, size) = if matches!(self.placement, TerminalPlacement::Right) {
            (pos.x.saturating_sub(bounds.x), bounds.width)
        } else {
            (pos.y.saturating_sub(bounds.y), bounds.height)
        };
        self.ratio = ((u32::from(offset) * 100 / u32::from(size.max(1))) as u16).clamp(20, 80);
    }

    pub fn render_session(&mut self, frame: &mut Frame, area: Rect, id: u64) {
        let Some(session) = self.sessions.get_mut(&id) else {
            return;
        };
        let theme = crate::theme::active_theme();
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(if self.focused {
                theme.accent
            } else {
                theme.border
            }))
            .style(Style::default().fg(theme.foreground).bg(theme.background));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.height < 3 || inner.width == 0 {
            return;
        }
        let status = if session.terminal.is_done() {
            "exited"
        } else {
            "running"
        };
        let directory = session.terminal.current_directory();
        let title = format!(
            " Terminal #{id} · {status} · {}:{}{} · {} ",
            session.host,
            directory.as_ref().unwrap_or(&session.cwd).display(),
            if directory.is_none() {
                " (initial cwd)"
            } else {
                ""
            },
            session.terminal.shell_program().unwrap_or("unknown shell")
        );
        frame.render_widget(
            Paragraph::new(title),
            Rect::new(inner.x, inner.y, inner.width, 1),
        );
        let mut x = inner.x;
        let mut control_y = inner.y + 1;
        for (label, action) in [
            ("[Tab]", TerminalControl::Tab),
            ("[Right]", TerminalControl::Right),
            ("[Bottom]", TerminalControl::Bottom),
            ("[Focus]", TerminalControl::Focus),
            ("[New]", TerminalControl::New),
            ("[Close]", TerminalControl::Close),
        ] {
            if x > inner.x && x.saturating_add(label.len() as u16) > inner.right() {
                x = inner.x;
                control_y = control_y.saturating_add(1);
            }
            if control_y >= inner.bottom() {
                break;
            }
            let width = (label.len() as u16).min(inner.right().saturating_sub(x));
            let rect = Rect::new(x, control_y, width, 1);
            frame.render_widget(
                Paragraph::new(label).style(Style::default().fg(theme.accent)),
                rect,
            );
            if width > 0 {
                self.controls.push((rect, id, action));
            }
            x = x.saturating_add(width + 1);
        }
        let footer_rows =
            u16::from(self.show_helpers && inner.bottom().saturating_sub(control_y + 1) >= 2);
        let body_y = (control_y + 1).min(inner.bottom().saturating_sub(footer_rows));
        let body = Rect::new(
            inner.x,
            body_y,
            inner.width,
            inner.bottom().saturating_sub(body_y + footer_rows),
        );
        self.body = Some((id, body));
        session
            .terminal
            .render_with_cursor(frame, body, self.focused && self.show_cursor);
        if footer_rows > 0 {
            frame.render_widget(
                Paragraph::new("F6 focus · F7 view · Ctrl+P search · Shift+PgUp/PgDn scroll")
                    .style(Style::default().fg(theme.muted)),
                Rect::new(inner.x, inner.bottom() - 1, inner.width, 1),
            );
        }
    }

    pub fn render_close_confirmation(&mut self, frame: &mut Frame) {
        if self.close_pending.is_none() {
            return;
        }
        let areas = super::modals::quit_confirm::QuitConfirmModal::new()
            .title("Close terminal?")
            .message("A process is still running. Close this terminal and terminate its process?")
            .render(frame);
        self.close_areas = Some(super::ModalAreas {
            yes: areas.yes,
            no: areas.no,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn compact_bottom_split_keeps_output_and_controls_visible() {
        let (terminal, _probe) =
            crate::ui::modals::terminal_editor::terminal_editor_test_support::live_editor();
        let mut workspace = TerminalWorkspace::default();
        let id = workspace.insert(TerminalSession {
            terminal,
            cwd: PathBuf::from("/tmp"),
            host: "test-host".into(),
        });
        workspace.split_session = Some(id);
        let mut screen = Terminal::new(TestBackend::new(64, 16)).expect("terminal");
        screen
            .draw(|frame| {
                workspace.split_chat(frame, Rect::new(0, 0, 30, 13));
            })
            .expect("split");
        assert!(workspace.body.expect("terminal body").1.height >= 1);
        assert_eq!(workspace.controls.len(), 6);
        workspace.drag(Position::new(0, u16::MAX));
        assert_eq!(workspace.ratio, 80);
    }
}
