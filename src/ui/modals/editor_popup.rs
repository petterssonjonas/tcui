use std::io::{Read, Write};
use std::sync::{Arc, RwLock};

use bytes::Bytes;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use portable_pty::{Child, CommandBuilder, NativePtySystem, PtySize, PtySystem};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};
use tokio::sync::mpsc::{Receiver, Sender};
use tui_term::vt100::Parser;
use tui_term::widget::PseudoTerminal;

const OUTPUT_CHANNEL_SIZE: usize = 64;
const INPUT_CHANNEL_SIZE: usize = 64;

pub struct EditorPopupState {
    pub artifact_name: String,
    parser: Arc<RwLock<Parser>>,
    input_tx: Sender<Bytes>,
    output_rx: Receiver<Vec<u8>>,
    child: Box<dyn Child + Send>,
}

impl EditorPopupState {
    pub fn new(path: &std::path::Path) -> Result<Self, String> {
        let command_line = build_editor_command_line(path)?;
        let artifact_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("editor")
            .to_string();

        let pty_system = NativePtySystem::default();
        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| format!("Failed to open PTY: {error}"))?;

        let mut cmd = CommandBuilder::new("sh");
        cmd.arg("-lc");
        cmd.arg(&command_line);
        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|error| format!("Failed to spawn editor: {error}"))?;
        drop(pair.slave);

        let parser = Arc::new(RwLock::new(Parser::new(24, 80, 0)));
        let (output_tx, output_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(OUTPUT_CHANNEL_SIZE);
        let (input_tx, input_rx) = tokio::sync::mpsc::channel::<Bytes>(INPUT_CHANNEL_SIZE);

        spawn_reader(
            pair.master
                .try_clone_reader()
                .map_err(|error| error.to_string())?,
            output_tx,
        );
        spawn_writer(
            pair.master
                .take_writer()
                .map_err(|error| error.to_string())?,
            input_rx,
        );

        Ok(Self {
            artifact_name,
            parser,
            input_tx,
            output_rx,
            child,
        })
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect) {
        let popup_area = popup_area(area);
        let title = Line::from(format!(" Editing: {} ", self.artifact_name));
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .style(Style::default().bg(Color::Black))
            .title(title);
        f.render_widget(Clear, popup_area);
        f.render_widget(block.clone(), popup_area);

        let inner = block.inner(popup_area);
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(1)])
            .margin(1)
            .split(inner);

        let parser = self.parser.read().expect("parser lock poisoned");
        let screen = parser.screen();
        let pseudo_term = PseudoTerminal::new(screen).block(block);
        f.render_widget(pseudo_term, layout[0]);

        f.render_widget(
            Paragraph::new("Esc closes the editor popup")
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Center),
            layout[1],
        );
    }

    pub fn poll_output(&mut self) {
        while let Ok(bytes) = self.output_rx.try_recv() {
            let mut parser = self.parser.write().expect("parser lock poisoned");
            parser.process(&bytes);
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        if key.kind != crossterm::event::KeyEventKind::Press {
            return true;
        }
        if key.code == KeyCode::Esc {
            return false;
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
}

impl Drop for EditorPopupState {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn popup_area(area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(8),
            Constraint::Percentage(84),
            Constraint::Percentage(8),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(6),
            Constraint::Percentage(88),
            Constraint::Percentage(6),
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

fn build_editor_command_line(path: &std::path::Path) -> Result<String, String> {
    let path_str = shell_escape(path);
    if let Ok(editor) = std::env::var("EDITOR") {
        let trimmed = editor.trim();
        if !trimmed.is_empty() {
            return Ok(format!("{trimmed} {path_str}"));
        }
    }
    for editor in ["nvim", "vim", "nano"] {
        if command_exists(editor) {
            return Ok(format!("{editor} {path_str}"));
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

fn shell_escape(path: &std::path::Path) -> String {
    let value = path.display().to_string();
    format!("'{}'", value.replace('\'', "'\"'\"'"))
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
