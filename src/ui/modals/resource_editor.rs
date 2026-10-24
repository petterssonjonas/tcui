use std::borrow::Cow;
use std::ops::Range;

use base64::{engine::general_purpose::STANDARD, Engine};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{prelude::*, widgets::*, Frame};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::tui::components::SearchField;
use crate::ui::artifact_sidebar::ArtifactHandle;

const MAX_EDITOR_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Encoding {
    Text,
    Base64,
}

pub(crate) enum EditorAction {
    None,
    Save,
    Close,
    Copy,
}

/// Decrypted edits stay in memory. No plaintext temporary file or external editor is used.
pub(crate) struct ResourceEditor {
    pub(crate) handle: ArtifactHandle,
    pub(crate) source_digest: blake3::Hash,
    title: String,
    buffer: SearchField,
    original: blake3::Hash,
    edited: bool,
    encoding: Encoding,
    line_starts: Vec<usize>,
    select_all: bool,
    confirm_close: bool,
    scroll: (usize, usize),
    notice: Option<String>,
}

impl ResourceEditor {
    pub(crate) fn new(
        handle: ArtifactHandle,
        name: String,
        bytes: Vec<u8>,
    ) -> Result<Self, String> {
        if bytes.len() > MAX_EDITOR_BYTES {
            return Err("The entry exceeds the 64 MiB in-app editor limit.".into());
        }
        let source_digest = blake3::hash(&bytes);
        let (encoding, text) = match String::from_utf8(bytes) {
            Ok(text)
                if !text
                    .chars()
                    .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t')) =>
            {
                (Encoding::Text, text)
            }
            Ok(text) => (Encoding::Base64, STANDARD.encode(text.as_bytes())),
            Err(error) => (Encoding::Base64, STANDARD.encode(error.as_bytes())),
        };
        let original = if encoding == Encoding::Text {
            source_digest
        } else {
            blake3::hash(text.as_bytes())
        };
        let line_starts = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(index, _)| index + 1))
            .collect();
        let title = format!(
            " Edit {}{} ",
            name.escape_debug(),
            if encoding == Encoding::Base64 {
                " (base64)"
            } else {
                ""
            }
        );
        Ok(Self {
            handle,
            source_digest,
            title,
            buffer: SearchField::from_text(text),
            original,
            edited: false,
            encoding,
            line_starts,
            select_all: false,
            confirm_close: false,
            scroll: (0, 0),
            notice: None,
        })
    }

    fn require_access(&self) -> color_eyre::Result<()> {
        crate::protection::require(match self.handle {
            ArtifactHandle::Memory(_) => crate::protection::Domain::Memories,
            _ => crate::protection::Domain::Vault,
        })
    }

    pub(crate) fn is_dirty(&self) -> bool {
        self.edited && blake3::hash(self.buffer.query().as_bytes()) != self.original
    }

    pub(crate) fn request_close(&mut self) -> bool {
        if self.is_dirty() {
            self.confirm_close = true;
            false
        } else {
            true
        }
    }

    pub(crate) fn bytes(&self) -> Result<Cow<'_, [u8]>, String> {
        if self.encoding == Encoding::Text {
            return Ok(Cow::Borrowed(self.buffer.query().as_bytes()));
        }
        let text = self.buffer.query();
        let compact = if text.bytes().any(|byte| byte.is_ascii_whitespace()) {
            Cow::Owned(
                text.bytes()
                    .filter(|byte| !byte.is_ascii_whitespace())
                    .collect::<Vec<_>>(),
            )
        } else {
            Cow::Borrowed(text.as_bytes())
        };
        let bytes = STANDARD
            .decode(compact.as_ref())
            .map_err(|error| format!("Invalid base64; the stored file was not changed: {error}"))?;
        if bytes.len() > MAX_EDITOR_BYTES {
            return Err("Decoded content exceeds the 64 MiB editor limit.".into());
        }
        Ok(Cow::Owned(bytes))
    }

    pub(crate) fn saved(&mut self, source_digest: blake3::Hash) {
        self.source_digest = source_digest;
        self.original = blake3::hash(self.buffer.query().as_bytes());
        self.edited = false;
        self.notice = None;
    }

    pub(crate) fn text(&self) -> &str {
        self.buffer.query()
    }

    pub(crate) fn insert(&mut self, text: &str) {
        if self.confirm_close {
            return;
        }
        if let Err(error) = self.require_access() {
            self.notice = Some(error.to_string());
            return;
        }
        self.insert_text(text);
    }

    fn insert_text(&mut self, text: &str) {
        let range = if self.select_all {
            0..self.buffer.len()
        } else {
            self.buffer.cursor()..self.buffer.cursor()
        };
        self.replace(range, text);
        self.select_all = false;
    }

    fn replace(&mut self, range: Range<usize>, text: &str) {
        let limit = if self.encoding == Encoding::Text {
            MAX_EDITOR_BYTES
        } else {
            MAX_EDITOR_BYTES.div_ceil(3) * 4
        };
        if self
            .buffer
            .len()
            .saturating_sub(range.len())
            .saturating_add(text.len())
            > limit
        {
            self.notice = Some("Edit exceeds the 64 MiB limit. The buffer was not changed.".into());
            return;
        }
        let first = self
            .line_starts
            .partition_point(|offset| *offset <= range.start);
        let last = self
            .line_starts
            .partition_point(|offset| *offset <= range.end);
        self.line_starts.drain(first..last);
        let delta = text.len() as isize - range.len() as isize;
        for offset in &mut self.line_starts[first..] {
            *offset = offset
                .checked_add_signed(delta)
                .expect("bounded edit offsets");
        }
        self.line_starts.splice(
            first..first,
            text.match_indices('\n')
                .map(|(index, _)| range.start + index + 1),
        );
        self.buffer.replace_range(range, text);
        self.edited = true;
        self.notice = None;
    }

    fn cursor_line(&self) -> usize {
        self.line_starts
            .partition_point(|start| *start <= self.buffer.cursor())
            .saturating_sub(1)
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> EditorAction {
        if self.confirm_close {
            return match key.code {
                KeyCode::Enter | KeyCode::Char('y' | 'Y') => EditorAction::Close,
                KeyCode::Esc | KeyCode::Char('n' | 'N') => {
                    self.confirm_close = false;
                    EditorAction::None
                }
                _ => EditorAction::None,
            };
        }
        if key.code == KeyCode::Esc
            || (key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('w'))
        {
            return if self.request_close() {
                EditorAction::Close
            } else {
                EditorAction::None
            };
        }
        if let Err(error) = self.require_access() {
            self.notice = Some(error.to_string());
            return EditorAction::None;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('s') => return EditorAction::Save,
                KeyCode::Char('c') => return EditorAction::Copy,
                KeyCode::Char('a') => self.select_all = true,
                KeyCode::Home => {
                    self.select_all = false;
                    self.buffer.home();
                }
                KeyCode::End => {
                    self.select_all = false;
                    self.buffer.end();
                }
                _ => {}
            }
            return EditorAction::None;
        }
        match key.code {
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::ALT) => {
                let mut encoded = [0; 4];
                self.insert_text(ch.encode_utf8(&mut encoded));
            }
            KeyCode::Enter => self.insert_text("\n"),
            KeyCode::Tab => self.insert_text("\t"),
            KeyCode::Backspace | KeyCode::Delete if self.select_all => {
                self.replace(0..self.buffer.len(), "");
                self.select_all = false;
            }
            KeyCode::Backspace => {
                let cursor = self.buffer.cursor();
                if let Some((previous, _)) =
                    self.buffer.query()[..cursor].char_indices().next_back()
                {
                    self.replace(previous..cursor, "");
                }
            }
            KeyCode::Delete => {
                let cursor = self.buffer.cursor();
                if let Some(ch) = self.buffer.query()[cursor..].chars().next() {
                    self.replace(cursor..cursor + ch.len_utf8(), "");
                }
            }
            KeyCode::Left => {
                self.select_all = false;
                self.buffer.left();
            }
            KeyCode::Right => {
                self.select_all = false;
                self.buffer.right();
            }
            KeyCode::Home => {
                self.select_all = false;
                self.buffer.set_cursor(self.line_starts[self.cursor_line()]);
            }
            KeyCode::End => {
                self.select_all = false;
                let end = self
                    .line_starts
                    .get(self.cursor_line() + 1)
                    .map_or(self.buffer.len(), |start| start - 1);
                self.buffer.set_cursor(end);
            }
            KeyCode::Up | KeyCode::Down => self.move_line(key.code == KeyCode::Down),
            _ => {}
        }
        EditorAction::None
    }

    fn move_line(&mut self, down: bool) {
        self.select_all = false;
        let row = self.cursor_line();
        let target = if down { row + 1 } else { row.saturating_sub(1) };
        let Some(&start) = self.line_starts.get(target) else {
            return;
        };
        if target == row {
            return;
        }
        let column = self.buffer.query()[self.line_starts[row]..self.buffer.cursor()]
            .chars()
            .count();
        let end = self
            .line_starts
            .get(target + 1)
            .map_or(self.buffer.len(), |start| start - 1);
        let offset = self.buffer.query()[start..end]
            .char_indices()
            .nth(column)
            .map_or(end - start, |(index, _)| index);
        self.buffer.set_cursor(start + offset);
    }

    pub(crate) fn render(&mut self, frame: &mut Frame, area: Rect) {
        let theme = crate::theme::active_theme();
        frame.render_widget(Clear, area);
        if let Err(error) = self.require_access() {
            let close = if self.confirm_close {
                "Discard unsaved edits? Enter/y: discard   Esc/n: keep the buffer"
            } else {
                "Esc: close this editor, then unlock before continuing."
            };
            frame.render_widget(
                Paragraph::new(format!("Locked entry: {error}\n{close}"))
                    .wrap(Wrap { trim: false }),
                area,
            );
            return;
        }
        let block = Block::default()
            .borders(Borders::ALL)
            .title(self.title.as_str())
            .style(Style::default().bg(theme.panel));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let parts = Layout::vertical([Constraint::Min(1), Constraint::Length(2)]).split(inner);
        let visible = usize::from(parts[0].height);
        let row = self.cursor_line();
        self.scroll.0 = self
            .scroll
            .0
            .min(row)
            .max(row.saturating_sub(visible.saturating_sub(1)));
        let cursor = self.buffer.cursor();
        let start = self.line_starts[row];
        let column = cursor - start;
        let max_bytes = usize::from(parts[0].width).saturating_mul(32).max(32);
        self.scroll.1 = self
            .scroll
            .1
            .min(column)
            .max(column.saturating_sub(max_bytes));
        self.scroll.1 = self
            .buffer
            .query()
            .floor_char_boundary(start + self.scroll.1)
            - start;
        if UnicodeWidthStr::width(&self.buffer.query()[start + self.scroll.1..cursor])
            >= usize::from(parts[0].width)
        {
            let mut width = 0;
            let mut left = cursor;
            for (index, ch) in self.buffer.query()[start + self.scroll.1..cursor]
                .char_indices()
                .rev()
            {
                let next = ch.width().unwrap_or(0);
                if width + next >= usize::from(parts[0].width) {
                    break;
                }
                width += next;
                left = start + self.scroll.1 + index;
            }
            self.scroll.1 = left - start;
        }
        let style = if self.select_all {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
        };
        for line in self.scroll.0..self.line_starts.len().min(self.scroll.0 + visible) {
            let begin = self.line_starts[line];
            let end = self
                .line_starts
                .get(line + 1)
                .map_or(self.buffer.len(), |start| start - 1);
            let left = self
                .buffer
                .query()
                .floor_char_boundary((begin + self.scroll.1).min(end));
            let right = self
                .buffer
                .query()
                .floor_char_boundary(left.saturating_add(max_bytes).min(end));
            frame.render_widget(
                Paragraph::new(&self.buffer.query()[left..right]).style(style),
                Rect::new(
                    parts[0].x,
                    parts[0].y + (line - self.scroll.0) as u16,
                    parts[0].width,
                    1,
                ),
            );
        }
        let help = self.notice.as_deref().unwrap_or(if self.confirm_close {
            "Discard unsaved edits? Enter/y: discard   Esc/n: keep editing"
        } else { "Ctrl+S: save   Ctrl+A: select all   Ctrl+C: copy   Esc: close\nBinary files use base64. Changes stay in memory until saved." });
        frame.render_widget(
            Paragraph::new(help).style(Style::default().fg(theme.muted)),
            parts[1],
        );
        if parts[0].width != 0 && parts[0].height != 0 {
            let x = UnicodeWidthStr::width(&self.buffer.query()[start + self.scroll.1..cursor])
                .min(usize::from(parts[0].width - 1)) as u16;
            frame.set_cursor_position((parts[0].x + x, parts[0].y + (row - self.scroll.0) as u16));
        }
    }
}
