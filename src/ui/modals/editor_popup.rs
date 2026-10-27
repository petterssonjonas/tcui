use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use bytes::Bytes;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use portable_pty::{Child, CommandBuilder, MasterPty, NativePtySystem, PtySize, PtySystem};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Clear, Paragraph},
    Frame,
};
use tokio::sync::mpsc::{Receiver, Sender};
use tui_term::vt100::Parser;
use tui_term::widget::PseudoTerminal;

const OUTPUT_CHANNEL_SIZE: usize = 64;
const INPUT_CHANNEL_SIZE: usize = 64;

pub struct EditorPopupState {
    artifact_name: String,
    chat_draft_path: Option<PathBuf>,
    parser: Arc<RwLock<Parser>>,
    input_tx: Sender<Bytes>,
    output_rx: Receiver<Vec<u8>>,
    child: Box<dyn Child + Send>,
    master: Box<dyn MasterPty + Send>,
    pub close_hit_area: Option<Rect>,
    child_exited: bool,
    current_size: (u16, u16),
}

impl EditorPopupState {
    pub fn new(path: &Path) -> Result<Self, String> {
        Self::new_with_chat_draft(path, None)
    }

    pub fn new_chat_draft(path: &Path) -> Result<Self, String> {
        Self::new_with_chat_draft(path, Some(path.to_path_buf()))
    }

    fn new_with_chat_draft(path: &Path, chat_draft_path: Option<PathBuf>) -> Result<Self, String> {
        let artifact_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("editor")
            .to_string();
        let editor_parts = resolve_editor()?;
        let pty_system = NativePtySystem::default();
        let initial_rows: u16 = 24;
        let initial_cols: u16 = 80;
        let pair = pty_system
            .openpty(PtySize {
                rows: initial_rows,
                cols: initial_cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| format!("Failed to open PTY: {error}"))?;

        let mut cmd = CommandBuilder::new(&editor_parts[0]);
        for arg in &editor_parts[1..] {
            cmd.arg(arg);
        }
        cmd.arg(path);
        cmd.env("TERM", "xterm-256color");
        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|error| format!("Failed to spawn editor: {error}"))?;
        drop(pair.slave);

        let parser = Arc::new(RwLock::new(Parser::new(initial_rows, initial_cols, 0)));
        let (output_tx, output_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(OUTPUT_CHANNEL_SIZE);
        let (input_tx, input_rx) = tokio::sync::mpsc::channel::<Bytes>(INPUT_CHANNEL_SIZE);

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| error.to_string())?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| error.to_string())?;
        let master = pair.master;

        spawn_reader(reader, output_tx);
        spawn_writer(writer, input_rx);

        Ok(Self {
            artifact_name,
            chat_draft_path,
            parser,
            input_tx,
            output_rx,
            child,
            master,
            close_hit_area: None,
            child_exited: false,
            current_size: (initial_rows, initial_cols),
        })
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect) {
        let theme = crate::theme::active_theme();
        let popup_area = popup_area(area);
        f.render_widget(Clear, popup_area);

        let block = Block::default().style(Style::default().bg(theme.panel));
        let inner = Rect::new(
            popup_area.x,
            popup_area.y + 1,
            popup_area.width,
            popup_area.height.saturating_sub(1),
        );
        f.render_widget(block, popup_area);
        let rows = inner.height.max(1);
        let cols = inner.width.max(1);
        if (rows, cols) != self.current_size {
            self.resize(rows, cols);
        }

        let parser = self.parser.read().expect("parser lock poisoned");
        let screen = parser.screen();
        let pseudo_term = PseudoTerminal::new(screen);
        f.render_widget(pseudo_term, inner);

        let title_y = popup_area.y;

        let name_label = format!(" {} ", self.artifact_name);
        f.render_widget(
            Paragraph::new(name_label).style(Style::default().fg(Color::Cyan)),
            Rect::new(
                popup_area.x + 1,
                title_y,
                self.artifact_name.len() as u16 + 2,
                1,
            ),
        );

        let center_label = " Editor ";
        let center_x = popup_area.x + popup_area.width / 2 - center_label.len() as u16 / 2;
        f.render_widget(
            Paragraph::new(center_label).style(Style::default().fg(Color::Cyan)),
            Rect::new(center_x, title_y, center_label.len() as u16, 1),
        );

        let close_len: u16 = 3;
        let close_x = popup_area.x + popup_area.width.saturating_sub(close_len + 1);
        let close_area = Rect::new(close_x, title_y, close_len, 1);
        f.render_widget(
            Paragraph::new("[x]").style(Style::default().fg(Color::Red)),
            close_area,
        );
        self.close_hit_area = Some(close_area);
    }

    fn resize(&mut self, rows: u16, cols: u16) {
        {
            let mut parser = self.parser.write().expect("parser lock poisoned");
            parser.screen_mut().set_size(rows, cols);
        }
        let _ = self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
        self.current_size = (rows, cols);
    }

    pub fn poll_output(&mut self) -> bool {
        loop {
            match self.output_rx.try_recv() {
                Ok(bytes) => {
                    let mut parser = self.parser.write().expect("parser lock poisoned");
                    parser.process(&bytes);
                }
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                    self.child_exited = true;
                    break;
                }
            }
        }
        self.child_exited
    }

    pub fn is_done(&self) -> bool {
        self.child_exited
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        if key.kind != crossterm::event::KeyEventKind::Press {
            return true;
        }
        let bytes = key_to_bytes(key);
        if !bytes.is_empty() {
            let _ = self.input_tx.try_send(Bytes::from(bytes));
        }
        true
    }

    pub fn close(&mut self) {
        let _ = self.child.kill();
    }

    pub fn send_scroll(&mut self, down: bool) {
        let seq = if down {
            vec![0x1b, b'[', b'B']
        } else {
            vec![0x1b, b'[', b'A']
        };
        let _ = self.input_tx.try_send(Bytes::from(seq));
    }

    pub fn close_area(&self) -> Option<Rect> {
        self.close_hit_area
    }

    pub fn chat_draft_path(&self) -> Option<&Path> {
        self.chat_draft_path.as_deref()
    }

    /// Takes the draft path so `Drop` will not delete the file before the caller reads it.
    pub fn take_chat_draft_path(&mut self) -> Option<PathBuf> {
        self.chat_draft_path.take()
    }
}

impl Drop for EditorPopupState {
    fn drop(&mut self) {
        let _ = self.child.kill();
        if let Some(path) = &self.chat_draft_path {
            let _ = std::fs::remove_file(path);
        }
    }
}

pub fn popup_area_pub(area: Rect) -> Rect {
    popup_area(area)
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

fn spawn_reader(mut reader: Box<dyn Read + Send>, tx: Sender<Vec<u8>>) {
    tokio::task::spawn_blocking(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(size) => {
                    if tx.blocking_send(buf[..size].to_vec()).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
}

fn spawn_writer(mut writer: Box<dyn Write + Send>, mut rx: Receiver<Bytes>) {
    tokio::spawn(async move {
        while let Some(bytes) = rx.recv().await {
            if writer.write_all(&bytes).is_err() {
                break;
            }
            if writer.flush().is_err() {
                break;
            }
        }
    });
}

fn resolve_editor() -> Result<Vec<String>, String> {
    if let Ok(editor) = std::env::var("EDITOR") {
        let trimmed = editor.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.split_whitespace().map(String::from).collect());
        }
    }
    for editor in ["nvim", "vim", "nano"] {
        if command_exists(editor) {
            return Ok(vec![editor.to_string()]);
        }
    }
    Err("No editor found. Set $EDITOR or install nvim, vim, or nano.".to_string())
}

fn command_exists(command: &str) -> bool {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
        .any(|directory| directory.join(command).is_file())
}

fn key_to_bytes(key: KeyEvent) -> Vec<u8> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);

    match key.code {
        KeyCode::Char(c) => char_to_bytes(c, ctrl, alt, shift),
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::Esc => vec![0x1b],
        KeyCode::Left => ansi_seq("D", alt),
        KeyCode::Right => ansi_seq("C", alt),
        KeyCode::Up => ansi_seq("A", alt),
        KeyCode::Down => ansi_seq("B", alt),
        KeyCode::Home => ansi_seq("H", alt),
        KeyCode::End => ansi_seq("F", alt),
        KeyCode::PageUp => ansi_seq("5~", alt),
        KeyCode::PageDown => ansi_seq("6~", alt),
        KeyCode::Delete => ansi_seq("3~", alt),
        KeyCode::Insert => ansi_seq("2~", alt),
        KeyCode::F(n) => f_key_bytes(n),
        _ => Vec::new(),
    }
}

fn char_to_bytes(c: char, ctrl: bool, alt: bool, _shift: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    if alt {
        bytes.push(0x1b);
    }
    if ctrl && c.is_ascii_alphabetic() {
        bytes.push((c as u8) & 0x1f);
    } else if ctrl && c == ' ' {
        bytes.push(0x00);
    } else {
        let mut buf = [0u8; 4];
        bytes.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
    }
    bytes
}

fn ansi_seq(code: &str, alt: bool) -> Vec<u8> {
    let prefix = if alt { "\x1b\x1b[" } else { "\x1b[" };
    format!("{prefix}{code}").into_bytes()
}

fn f_key_bytes(n: u8) -> Vec<u8> {
    match n {
        1 => b"\x1bOP".to_vec(),
        2 => b"\x1bOQ".to_vec(),
        3 => b"\x1bOR".to_vec(),
        4 => b"\x1bOS".to_vec(),
        5 => b"\x1b[15~".to_vec(),
        6 => b"\x1b[17~".to_vec(),
        7 => b"\x1b[18~".to_vec(),
        8 => b"\x1b[19~".to_vec(),
        9 => b"\x1b[20~".to_vec(),
        10 => b"\x1b[21~".to_vec(),
        11 => b"\x1b[23~".to_vec(),
        12 => b"\x1b[24~".to_vec(),
        _ => Vec::new(),
    }
}
