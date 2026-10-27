use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use bytes::Bytes;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent};
use portable_pty::PtySize;
use ratatui::{layout::Rect, Frame};
use tokio::sync::mpsc::{Receiver, Sender};
use tui_term::vt100::Parser;
use tui_term::widget::PseudoTerminal;

use crate::vault::{EditorTargetError, ValidatedEditorTarget};

#[path = "terminal_editor_input.rs"]
mod input;
#[path = "terminal_editor_process.rs"]
mod process;

const INITIAL_ROWS: u16 = 24;
const INITIAL_COLS: u16 = 80;
const SCROLLBACK_LINES: usize = 10_000;
const MAX_INPUT_BYTES: usize = 1024 * 1024;
const OUTPUT_CHUNKS_PER_POLL: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseRequest {
    ConfirmationRequired,
    Closed,
}

#[derive(Debug, thiserror::Error)]
pub enum TerminalEditorError {
    #[error(transparent)]
    Target(#[from] EditorTargetError),
    #[error("No editor found. Set $EDITOR or install nvim, vim, or nano.")]
    EditorNotFound,
    #[error("Failed to open PTY: {0}")]
    OpenPty(String),
    #[error("Failed to prepare PTY I/O: {0}")]
    PrepareIo(String),
    #[error("Failed to spawn terminal process: {0}")]
    Spawn(String),
    #[error("Terminal input queue is full; nothing was sent. Wait and try again.")]
    InputQueueFull,
    #[error("Terminal session is not running; nothing was sent.")]
    NotRunning,
    #[error("Terminal input exceeds the 1 MiB limit; nothing was sent.")]
    InputTooLarge,
    #[error("Terminal paste contains execution controls; review it as a command instead.")]
    UnsafePaste,
    #[error("Cannot use terminal working directory: {0}")]
    WorkingDirectory(String),
    #[error("Approved command sessions require sh, bash, zsh, or fish.")]
    UnsupportedCommandShell,
    #[error("Command shell did not reach a ready prompt; nothing was sent: {0}")]
    CommandShellStartup(String),
    #[error("Approved commands require a clean command shell with confirmed startup readiness; nothing was sent.")]
    CommandShellNotReady,
    #[error("Could not prepare approved command script; nothing was sent: {0}")]
    CommandScript(#[source] io::Error),
}

struct ApprovedScript {
    path: PathBuf,
}

impl ApprovedScript {
    fn create(command: &str) -> io::Result<Self> {
        let path = std::env::temp_dir()
            .canonicalize()?
            .join(format!("tcui-approved-{:032x}.sh", rand::random::<u128>()));
        if path
            .as_os_str()
            .as_encoded_bytes()
            .iter()
            .any(u8::is_ascii_control)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "temporary script path contains terminal control characters",
            ));
        }
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path)?;
        let script = Self { path };
        file.write_all(command.as_bytes())?;
        Ok(script)
    }

    fn invocation(&self, fish: bool) -> Vec<u8> {
        let path = self.path.as_os_str().as_encoded_bytes();
        let mut input = Vec::with_capacity(path.len() + 16);
        input.extend_from_slice(if fish { b"source '" } else { b". '" });
        for &byte in path {
            match byte {
                b'\'' if !fish => input.extend_from_slice(b"'\\''"),
                b'\'' | b'\\' if fish => input.extend_from_slice(&[b'\\', byte]),
                _ => input.push(byte),
            }
        }
        input.extend_from_slice(b"'\r");
        input
    }
}

impl Drop for ApprovedScript {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub struct TerminalEditorState {
    parser: Arc<RwLock<Parser>>,
    input_tx: Option<Sender<Bytes>>,
    output_rx: Option<Receiver<Vec<u8>>>,
    child: Option<Box<dyn SessionChild>>,
    master: Option<Box<dyn SessionMaster>>,
    child_exited: bool,
    close_pending: bool,
    closed: bool,
    current_size: (u16, u16),
    transport: Option<process::TransportState>,
    last_error: Option<String>,
    shell_program: Option<String>,
    command_ready: bool,
    approved_scripts: Vec<ApprovedScript>,
}

impl TerminalEditorState {
    pub fn default_shell_program() -> String {
        process::default_shell_program()
    }

    pub fn new(
        target: ValidatedEditorTarget,
        working_directory: &Path,
        area: Rect,
    ) -> Result<Self, TerminalEditorError> {
        let prepared = process::PreparedSession::new(working_directory, initial_pty_size(area))?;
        Self::new_with_spawn(target, working_directory, area, move |path, _, _| {
            prepared.spawn(path)
        })
    }

    /// Starts a persistent shell on this application's host, never a model/tool host.
    pub async fn new_shell(
        shell: Option<&str>,
        working_directory: &Path,
        area: Rect,
    ) -> Result<Self, TerminalEditorError> {
        let shell = shell.map(str::to_owned);
        let directory = working_directory.to_path_buf();
        let (session, program) = tokio::task::spawn_blocking(move || {
            let program = shell
                .as_deref()
                .filter(|shell| !shell.trim().is_empty())
                .map(str::to_owned)
                .unwrap_or_else(process::default_shell_program);
            let session = process::PreparedSession::shell(
                shell.as_deref(),
                &directory,
                initial_pty_size(area),
            )?
            .start()?;
            Ok::<_, TerminalEditorError>((session, program))
        })
        .await
        .map_err(|error| TerminalEditorError::Spawn(error.to_string()))??;
        let mut state = Self::from_spawned(session, area);
        state.shell_program = Some(program);
        Ok(state)
    }

    /// Starts a shell without user startup files and waits for its first prompt.
    /// Approved code must not be sent through ordinary interactive startup.
    pub async fn new_command_shell(
        shell: &str,
        working_directory: &Path,
        area: Rect,
    ) -> Result<Self, TerminalEditorError> {
        let shell = shell.to_owned();
        let directory = working_directory.to_path_buf();
        let readiness = format!("tcui-ready-{:032x}", rand::random::<u128>());
        let startup_marker = format!("\x1b]133;A;{readiness}\x07").into_bytes();
        let (session, program) = tokio::task::spawn_blocking(move || {
            let session = process::PreparedSession::command_shell(
                &shell,
                &directory,
                initial_pty_size(area),
                &readiness,
            )?
            .start()?;
            Ok::<_, TerminalEditorError>((session, shell))
        })
        .await
        .map_err(|error| TerminalEditorError::Spawn(error.to_string()))??;
        let mut state = Self::from_spawned(session, area);
        state.shell_program = Some(program);
        let ready = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let mut boundary = Vec::with_capacity(4096 + startup_marker.len());
            loop {
                let bytes = state
                    .output_rx
                    .as_mut()
                    .ok_or_else(|| {
                        TerminalEditorError::CommandShellStartup("output closed".into())
                    })?
                    .recv()
                    .await
                    .ok_or_else(|| {
                        TerminalEditorError::CommandShellStartup("shell exited".into())
                    })?;
                state
                    .parser
                    .write()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .process(&bytes);
                boundary.extend_from_slice(&bytes);
                let found = boundary
                    .windows(startup_marker.len())
                    .any(|window| window == startup_marker.as_slice());
                state.refresh_child_status();
                if state.is_done() {
                    return Err(TerminalEditorError::CommandShellStartup(
                        "shell exited".into(),
                    ));
                }
                if found {
                    return Ok(());
                }
                let discard = boundary.len().saturating_sub(startup_marker.len() - 1);
                boundary.drain(..discard);
            }
        })
        .await
        .map_err(|_| {
            TerminalEditorError::CommandShellStartup(
                "startup timed out (a startup program may be waiting for input)".into(),
            )
        })?;
        ready?;
        state.command_ready = true;
        Ok(state)
    }

    pub fn command_session_ready(&self) -> bool {
        self.command_ready && !self.is_done()
    }

    fn new_with_spawn(
        target: ValidatedEditorTarget,
        working_directory: &Path,
        area: Rect,
        spawn: impl FnOnce(&Path, &Path, PtySize) -> Result<SpawnedSession, TerminalEditorError>,
    ) -> Result<Self, TerminalEditorError> {
        let size = initial_pty_size(area);
        let path = target.revalidate_for_spawn()?;
        let session = spawn(&path, working_directory, size)?;
        Ok(Self::from_spawned(session, area))
    }

    fn from_spawned(session: SpawnedSession, area: Rect) -> Self {
        let size = initial_pty_size(area);
        Self {
            // Default vt100 callbacks are inert: OSC clipboard, title, browser,
            // and unknown escape requests never reach the host terminal.
            parser: Arc::new(RwLock::new(Parser::new(
                size.rows,
                size.cols,
                SCROLLBACK_LINES,
            ))),
            input_tx: Some(session.input_tx),
            output_rx: Some(session.output_rx),
            child: Some(session.child),
            master: Some(session.master),
            child_exited: false,
            close_pending: false,
            closed: false,
            current_size: (size.rows, size.cols),
            transport: session.transport,
            last_error: None,
            shell_program: None,
            command_ready: false,
            approved_scripts: Vec::new(),
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        self.render_with_cursor(frame, area, true);
    }

    pub fn render_with_cursor(&mut self, frame: &mut Frame, area: Rect, show_cursor: bool) {
        self.resize(area);
        let parser = self
            .parser
            .read()
            .unwrap_or_else(|poison| poison.into_inner());
        frame.render_widget(
            PseudoTerminal::new(parser.screen())
                .cursor(tui_term::widget::Cursor::default().visibility(show_cursor)),
            area,
        );
        if let Some(error) = self.last_error.as_deref().filter(|_| area.height > 0) {
            let status = Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1);
            frame.render_widget(
                ratatui::widgets::Paragraph::new(error)
                    .style(ratatui::style::Style::default().fg(ratatui::style::Color::Red)),
                status,
            );
        }
    }

    pub fn resize(&mut self, area: Rect) {
        let size = pty_size(area);
        if (size.rows, size.cols) == self.current_size {
            return;
        }
        self.parser
            .write()
            .unwrap_or_else(|poison| poison.into_inner())
            .screen_mut()
            .set_size(size.rows, size.cols);
        if let Some(master) = &self.master {
            if let Err(error) = master.resize(size) {
                self.last_error = Some(format!("Terminal resize failed: {error}"));
            }
        }
        self.current_size = (size.rows, size.cols);
    }

    pub fn poll_output(&mut self) -> bool {
        if let Some(output_rx) = self.output_rx.as_mut() {
            // Bound work so a continuously noisy child cannot starve the UI.
            for _ in 0..OUTPUT_CHUNKS_PER_POLL {
                match output_rx.try_recv() {
                    Ok(bytes) => self
                        .parser
                        .write()
                        .unwrap_or_else(|poison| poison.into_inner())
                        .process(&bytes),
                    Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                    Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => break,
                }
            }
        }
        if let Some(transport) = self.transport.as_mut() {
            while let Ok(error) = transport.errors.try_recv() {
                self.last_error = Some(error);
            }
        }
        self.refresh_child_status();
        if self.child_exited {
            self.input_tx.take();
        }
        self.is_done()
    }

    pub const fn is_done(&self) -> bool {
        self.child_exited || self.closed
    }

    pub const fn has_unsaved_risk(&self) -> bool {
        !self.child_exited && !self.closed
    }

    pub const fn close_is_pending(&self) -> bool {
        self.close_pending
    }

    pub fn request_close(&mut self) -> CloseRequest {
        self.refresh_child_status();
        if self.has_unsaved_risk() {
            self.close_pending = true;
            CloseRequest::ConfirmationRequired
        } else {
            self.teardown();
            CloseRequest::Closed
        }
    }

    pub fn cancel_close(&mut self) {
        self.close_pending = false;
    }

    pub fn confirm_close(&mut self) {
        self.teardown();
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        if key.kind == crossterm::event::KeyEventKind::Release {
            return true;
        }
        if key.modifiers.contains(KeyModifiers::SHIFT) {
            match key.code {
                KeyCode::PageUp => {
                    self.scroll(self.current_size.0 as isize);
                    return true;
                }
                KeyCode::PageDown => {
                    self.scroll(-(self.current_size.0 as isize));
                    return true;
                }
                _ => {}
            }
        }
        let mut bytes = input::key_to_bytes(key);
        let application_cursor = self
            .parser
            .read()
            .unwrap_or_else(|poison| poison.into_inner())
            .screen()
            .application_cursor();
        if application_cursor
            && bytes.len() == 3
            && bytes.starts_with(b"\x1b[")
            && matches!(bytes[2], b'A' | b'B' | b'C' | b'D' | b'H' | b'F')
        {
            bytes[1] = b'O';
        }
        if !bytes.is_empty() {
            if let Err(error) = self.write_input(&bytes) {
                self.last_error = Some(error.to_string());
            }
        }
        true
    }

    /// Routes mouse events only while the child requests terminal mouse reporting.
    pub fn handle_mouse(&mut self, event: MouseEvent, area: Rect) -> bool {
        let bytes = {
            let parser = self
                .parser
                .read()
                .unwrap_or_else(|poison| poison.into_inner());
            input::mouse_to_bytes(
                event,
                area,
                parser.screen().mouse_protocol_mode(),
                parser.screen().mouse_protocol_encoding(),
            )
        };
        let Some(bytes) = bytes else { return false };
        if !bytes.is_empty() {
            if let Err(error) = self.write_input(&bytes) {
                self.last_error = Some(error.to_string());
            }
        }
        true
    }

    /// Queues bytes atomically, or rejects the entire write without sending any.
    /// Later transport failures are reported by poll_output/take_error.
    pub fn write_input(&mut self, bytes: &[u8]) -> Result<(), TerminalEditorError> {
        if bytes.len() > MAX_INPUT_BYTES {
            return Err(TerminalEditorError::InputTooLarge);
        }
        self.reserve_input()?.send(Bytes::copy_from_slice(bytes));
        self.return_to_live_screen();
        Ok(())
    }

    fn reserve_input(&self) -> Result<tokio::sync::mpsc::Permit<'_, Bytes>, TerminalEditorError> {
        if self.is_done() {
            return Err(TerminalEditorError::NotRunning);
        }
        self.input_tx
            .as_ref()
            .ok_or(TerminalEditorError::NotRunning)?
            .try_reserve()
            .map_err(|error| match error {
                tokio::sync::mpsc::error::TrySendError::Full(_) => {
                    TerminalEditorError::InputQueueFull
                }
                tokio::sync::mpsc::error::TrySendError::Closed(_) => {
                    TerminalEditorError::NotRunning
                }
            })
    }

    fn return_to_live_screen(&self) {
        self.parser
            .write()
            .unwrap_or_else(|poison| poison.into_inner())
            .screen_mut()
            .set_scrollback(0);
    }

    /// Source approved code from a private file, never from terminal typeahead.
    /// Interactive programs keep stdin connected to the user's terminal.
    pub fn send_approved_commands(&mut self, command: &str) -> Result<(), TerminalEditorError> {
        if !self.command_ready {
            return Err(TerminalEditorError::CommandShellNotReady);
        }
        if command.is_empty() {
            return Ok(());
        }
        if command.len() >= MAX_INPUT_BYTES {
            return Err(TerminalEditorError::InputTooLarge);
        }
        let permit = self.reserve_input()?;
        let script = ApprovedScript::create(command).map_err(TerminalEditorError::CommandScript)?;
        let fish = self.shell_program.as_deref().is_some_and(|shell| {
            Path::new(shell)
                .file_name()
                .is_some_and(|name| name == "fish")
        });
        let input = script.invocation(fish);
        if input.len() > MAX_INPUT_BYTES {
            return Err(TerminalEditorError::InputTooLarge);
        }
        permit.send(Bytes::from(input));
        self.approved_scripts.push(script);
        self.return_to_live_screen();
        Ok(())
    }

    /// Paste never adds Enter and never permits an embedded bracketed-paste terminator.
    pub fn paste(&mut self, text: &str) -> Result<(), TerminalEditorError> {
        if text.len() > MAX_INPUT_BYTES.saturating_sub(12) {
            return Err(TerminalEditorError::InputTooLarge);
        }
        let bracketed = self
            .parser
            .read()
            .unwrap_or_else(|poison| poison.into_inner())
            .screen()
            .bracketed_paste();
        if text
            .chars()
            .any(|ch| ch.is_control() && !(bracketed && matches!(ch, '\n' | '\r' | '\t')))
        {
            return Err(TerminalEditorError::UnsafePaste);
        }
        if !bracketed {
            return self.write_input(text.as_bytes());
        }
        let mut bytes = Vec::with_capacity(text.len() + 12);
        bytes.extend_from_slice(b"\x1b[200~");
        bytes.extend_from_slice(text.as_bytes());
        bytes.extend_from_slice(b"\x1b[201~");
        self.write_input(&bytes)
    }

    /// Positive lines scroll backward; negative lines scroll toward live output.
    pub fn scroll(&mut self, lines: isize) {
        let mut parser = self
            .parser
            .write()
            .unwrap_or_else(|poison| poison.into_inner());
        let position = parser.screen().scrollback().saturating_add_signed(lines);
        parser
            .screen_mut()
            .set_scrollback(position.min(SCROLLBACK_LINES));
    }

    pub fn send_scroll(&mut self, down: bool) {
        let bytes: &[u8] = if down { b"\x1b[B" } else { b"\x1b[A" };
        if let Err(error) = self.write_input(bytes) {
            self.last_error = Some(error.to_string());
        }
    }

    pub fn take_error(&mut self) -> Option<String> {
        self.last_error.take()
    }

    pub fn current_directory(&self) -> Option<PathBuf> {
        self.child
            .as_ref()
            .and_then(|child| child.current_directory())
    }

    pub fn shell_program(&self) -> Option<&str> {
        self.shell_program.as_deref()
    }

    /// Foreground ownership alone cannot prove that a shell is at an empty prompt.
    pub fn shell_is_foreground(&self) -> bool {
        if self.is_done() || self.shell_program.is_none() {
            return false;
        }
        let child_pid = self.child.as_ref().and_then(|child| child.process_id());
        let foreground = self
            .master
            .as_ref()
            .and_then(|master| master.foreground_group());
        matches!((child_pid, foreground), (Some(child), Some(group)) if child == group)
    }

    #[cfg(test)]
    const fn current_size(&self) -> (u16, u16) {
        self.current_size
    }

    fn refresh_child_status(&mut self) {
        if self.child_exited || self.closed {
            return;
        }
        match self.child.as_mut().map(|child| child.has_exited()) {
            Some(Ok(true)) => {
                self.child_exited = true;
                self.close_pending = false;
            }
            Some(Err(error)) => self.last_error = Some(format!("Terminal status failed: {error}")),
            _ => {}
        }
    }

    fn teardown(&mut self) {
        if self.closed {
            return;
        }
        self.refresh_child_status();
        self.transport.take();
        if let Some(master) = &self.master {
            if let Err(error) = master.terminate_foreground() {
                self.last_error = Some(format!("Terminal foreground cleanup failed: {error}"));
            }
        }
        if let Some(mut child) = self.child.take() {
            if !self.child_exited {
                if let Err(error) = child.terminate() {
                    self.last_error = Some(format!("Terminal close failed: {error}"));
                }
                if let Err(error) = child.wait() {
                    self.last_error = Some(format!("Terminal reap failed: {error}"));
                }
            }
        }
        self.input_tx.take();
        if let Some(mut output_rx) = self.output_rx.take() {
            output_rx.close();
        }
        self.master.take();
        self.approved_scripts.clear();
        self.close_pending = false;
        self.closed = true;
    }
}

impl Drop for TerminalEditorState {
    fn drop(&mut self) {
        self.teardown();
    }
}

trait SessionChild: Send {
    fn terminate(&mut self) -> io::Result<()>;
    fn has_exited(&mut self) -> io::Result<bool>;
    fn wait(&mut self) -> io::Result<()>;
    fn current_directory(&self) -> Option<PathBuf> {
        None
    }
    fn process_id(&self) -> Option<u32> {
        None
    }
}

trait SessionMaster: Send {
    fn resize(&self, size: PtySize) -> io::Result<()>;
    fn terminate_foreground(&self) -> io::Result<()> {
        Ok(())
    }
    fn foreground_group(&self) -> Option<u32> {
        None
    }
}

struct SpawnedSession {
    child: Box<dyn SessionChild>,
    master: Box<dyn SessionMaster>,
    input_tx: Sender<Bytes>,
    output_rx: Receiver<Vec<u8>>,
    transport: Option<process::TransportState>,
}

fn pty_size(area: Rect) -> PtySize {
    PtySize {
        rows: area.height.max(1),
        cols: area.width.max(1),
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn initial_pty_size(area: Rect) -> PtySize {
    PtySize {
        rows: area.height.max(INITIAL_ROWS),
        cols: area.width.max(INITIAL_COLS),
        pixel_width: 0,
        pixel_height: 0,
    }
}

#[cfg(test)]
#[path = "terminal_editor_tests.rs"]
mod terminal_editor_core;

#[cfg(test)]
#[path = "terminal_editor/terminal_editor_test_support.rs"]
pub(crate) mod terminal_editor_test_support;
