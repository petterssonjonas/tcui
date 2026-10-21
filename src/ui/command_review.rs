use super::command_blocks::CommandBlock;
use ratatui::{prelude::*, widgets::*, Frame};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewControl {
    Block(usize),
    Destination,
    Copy,
    Approve,
    Transfer,
    Cancel,
}

pub struct CommandReview {
    pub blocks: Vec<CommandBlock>,
    pub selected: Vec<bool>,
    pub cursor: usize,
    pub destination: Option<u64>,
    pub cwd: PathBuf,
    pub host: String,
    pub default_shell: String,
    pub destination_shell: Option<String>,
    pub cwd_is_current: bool,
    pub scroll: u16,
    pub controls: Vec<(Rect, ReviewControl)>,
}

impl CommandReview {
    pub fn new(
        blocks: Vec<CommandBlock>,
        cwd: PathBuf,
        host: String,
        destination: Option<u64>,
    ) -> Self {
        let selected = (0..blocks.len()).map(|index| index == 0).collect();
        Self {
            blocks,
            selected,
            cursor: 0,
            destination,
            cwd,
            host,
            scroll: 0,
            controls: Vec::new(),
            default_shell:
                crate::ui::modals::terminal_editor::TerminalEditorState::default_shell_program(),
            destination_shell: None,
            cwd_is_current: true,
        }
    }

    pub fn command(&self) -> String {
        let mut text = String::new();
        for (block, selected) in self.blocks.iter().zip(&self.selected) {
            if *selected {
                if !text.is_empty() && !text.ends_with('\n') {
                    text.push('\n');
                }
                text.push_str(&block.code);
            }
        }
        text
    }

    pub fn shell(&self) -> Result<String, &'static str> {
        let mut shell: Option<&str> = None;
        for (block, selected) in self.blocks.iter().zip(&self.selected) {
            if !*selected {
                continue;
            }
            let program = match block.language.as_str() {
                "sh" => "sh",
                "bash" => "bash",
                "zsh" => "zsh",
                "fish" => "fish",
                _ => self.default_shell.as_str(),
            };
            let name = std::path::Path::new(program)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(program);
            if shell.is_some_and(|previous| {
                std::path::Path::new(previous)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(previous)
                    != name
            }) {
                return Err("Selected blocks require different shells; select compatible blocks.");
            }
            shell = Some(program);
        }
        Ok(shell.unwrap_or(&self.default_shell).to_string())
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let theme = crate::theme::active_theme();
        let popup = crate::tui::components::centered_rect(94, 92, area);
        frame.render_widget(Clear, popup);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Review shell commands — no execution before approval ")
            .style(theme.panel_style())
            .border_style(Style::default().fg(theme.warning));
        let inner = block.inner(popup);
        frame.render_widget(block, popup);
        self.controls.clear();
        let destination = self.destination.map_or_else(
            || "new terminal".to_string(),
            |id| format!("existing terminal #{id}"),
        );
        let mut lines = vec![
            Line::from(format!(
                "Host: {} · Shell: {}",
                self.host,
                self.shell().unwrap_or_else(str::to_string)
            )),
            Line::from(format!(
                "{}: {}",
                if self.cwd_is_current {
                    "Directory"
                } else {
                    "Initial directory (current unavailable)"
                },
                self.cwd.display()
            )),
            Line::from(format!("Target: {destination} · Tab changes target")),
            Line::styled(
                if self.destination.is_some() {
                    format!("Paste only; no Enter. Foreground programs may receive input. Target shell: {}.",
                        self.destination_shell.as_deref().unwrap_or("unknown"))
                } else {
                    "Run sources a private script in a clean shell. Commands can modify or delete data."
                        .into()
                },
                Style::default().fg(theme.warning),
            ),
            Line::from(
                "Up/Down block · Space select · PgUp/PgDn scroll · Ctrl+R Run · Ctrl+V Paste · Ctrl+C copy · Esc cancel",
            ),
            Line::from(""),
        ];
        let block_line_start = lines.len();
        for (index, block) in self.blocks.iter().enumerate() {
            let marker = if self.selected[index] { "x" } else { " " };
            lines.push(Line::styled(
                format!(
                    "{} [{marker}] Block {} ({})",
                    if index == self.cursor { ">" } else { " " },
                    index + 1,
                    block.language
                ),
                if index == self.cursor {
                    theme.selected_style()
                } else {
                    Style::default().fg(theme.foreground)
                },
            ));
        }
        lines.push(Line::from(""));
        lines.push(Line::styled(
            if self.destination.is_some() {
                "Exact paste payload — inspect before pressing Enter:"
            } else {
                "Exact command payload — Run submits these commands once:"
            },
            Style::default().fg(theme.warning),
        ));
        let command = self.command();
        for line in command.split('\n') {
            if line.chars().any(char::is_control) {
                let mut visible = String::with_capacity(line.len());
                for ch in line.chars() {
                    if ch.is_control() {
                        visible.extend(ch.escape_default());
                    } else {
                        visible.push(ch);
                    }
                }
                lines.push(Line::from(visible));
            } else {
                lines.push(Line::from(line));
            }
        }
        lines.push(Line::from("[end of command payload]"));
        let body = Rect::new(
            inner.x,
            inner.y,
            inner.width,
            inner.height.saturating_sub(2),
        );
        // Metadata and block labels each contain one span. Measure their actual
        // wrapped rows before moving the text into the rendered paragraph.
        let mut block_rows = Vec::with_capacity(self.blocks.len());
        let mut row = 0usize;
        for (index, line) in lines
            .iter()
            .enumerate()
            .take(block_line_start + self.blocks.len())
        {
            let text = line.spans.first().map_or("", |span| span.content.as_ref());
            let height = Paragraph::new(text)
                .wrap(Wrap { trim: false })
                .line_count(body.width);
            if index >= block_line_start {
                block_rows.push((index - block_line_start, row, height));
            }
            row += height;
        }
        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
        let total_rows = paragraph.line_count(body.width);
        self.scroll = self.scroll.min(
            total_rows
                .saturating_sub(body.height as usize)
                .min(u16::MAX as usize) as u16,
        );
        for (index, row, height) in block_rows {
            let top = row.max(self.scroll as usize);
            let bottom = (row + height).min(self.scroll as usize + body.height as usize);
            if top < bottom {
                self.controls.push((
                    Rect::new(
                        body.x,
                        body.y + (top - self.scroll as usize) as u16,
                        body.width,
                        (bottom - top) as u16,
                    ),
                    ReviewControl::Block(index),
                ));
            }
        }
        frame.render_widget(paragraph.scroll((self.scroll, 0)), body);
        let y = inner.bottom().saturating_sub(1);
        let mut x = inner.x;
        for (label, control) in [
            ("[Dest]", ReviewControl::Destination),
            ("[Copy]", ReviewControl::Copy),
            ("[Run]", ReviewControl::Approve),
            ("[Paste]", ReviewControl::Transfer),
            ("[Cancel]", ReviewControl::Cancel),
        ] {
            if (control == ReviewControl::Approve && self.destination.is_some())
                || (control == ReviewControl::Transfer && self.destination.is_none())
            {
                continue;
            }
            let width = (label.len() as u16).min(inner.right().saturating_sub(x));
            let rect = Rect::new(x, y, width, 1);
            frame.render_widget(
                Paragraph::new(label).style(Style::default().fg(
                    if control == ReviewControl::Approve {
                        theme.warning
                    } else {
                        theme.accent
                    },
                )),
                rect,
            );
            if width > 0 {
                self.controls.push((rect, control));
            }
            x = x.saturating_add(width + 2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn review() -> CommandReview {
        let mut review = CommandReview::new(
            vec![
                CommandBlock {
                    language: "bash".into(),
                    code: "printf '%s' 'first'".into(),
                },
                CommandBlock {
                    language: "bash".into(),
                    code: "printf second\n\n".into(),
                },
            ],
            PathBuf::from("/tmp"),
            "test-host".into(),
            None,
        );
        review.default_shell = "/bin/bash".into();
        review
    }

    #[test]
    fn selected_commands_preserve_bytes_and_explicit_join() {
        let mut review = review();
        assert_eq!(review.command(), "printf '%s' 'first'");
        review.selected[1] = true;
        assert_eq!(review.command(), "printf '%s' 'first'\nprintf second\n\n");
        assert_eq!(review.shell().expect("compatible shell"), "bash");
        review.blocks[1].language = "fish".into();
        assert!(review.shell().is_err());
    }

    #[test]
    fn run_only_appears_for_new_and_paste_only_for_existing() {
        let mut review = review();
        let mut terminal = Terminal::new(TestBackend::new(100, 35)).expect("terminal");
        terminal
            .draw(|frame| review.render(frame, frame.area()))
            .expect("review new");
        assert!(review
            .controls
            .iter()
            .any(|(_, control)| *control == ReviewControl::Approve));
        assert!(!review
            .controls
            .iter()
            .any(|(_, control)| *control == ReviewControl::Transfer));
        review.destination = Some(1);
        terminal
            .draw(|frame| review.render(frame, frame.area()))
            .expect("review existing");
        assert!(!review
            .controls
            .iter()
            .any(|(_, control)| *control == ReviewControl::Approve));
        assert!(review
            .controls
            .iter()
            .any(|(_, control)| *control == ReviewControl::Transfer));
    }
}
