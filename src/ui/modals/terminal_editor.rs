use std::io;
use std::path::Path;
use std::sync::{Arc, RwLock};

use bytes::Bytes;
use crossterm::event::KeyEvent;
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
    #[error("Failed to spawn editor: {0}")]
    Spawn(String),
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
}

impl TerminalEditorState {
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
            parser: Arc::new(RwLock::new(Parser::new(size.rows, size.cols, 0))),
            input_tx: Some(session.input_tx),
            output_rx: Some(session.output_rx),
            child: Some(session.child),
            master: Some(session.master),
            child_exited: false,
            close_pending: false,
            closed: false,
            current_size: (size.rows, size.cols),
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        self.resize(area);
        let parser = self
            .parser
            .read()
            .unwrap_or_else(|poison| poison.into_inner());
        frame.render_widget(PseudoTerminal::new(parser.screen()), area);
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
            let _ = master.resize(size);
        }
        self.current_size = (size.rows, size.cols);
    }

    pub fn poll_output(&mut self) -> bool {
        if let Some(output_rx) = self.output_rx.as_mut() {
            loop {
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
        self.refresh_child_status();
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
        if key.kind != crossterm::event::KeyEventKind::Press {
            return true;
        }
        let bytes = input::key_to_bytes(key);
        if !bytes.is_empty() {
            if let Some(input_tx) = &self.input_tx {
                let _ = input_tx.try_send(Bytes::from(bytes));
            }
        }
        true
    }

    pub fn send_scroll(&mut self, down: bool) {
        let sequence = if down {
            vec![0x1b, b'[', b'B']
        } else {
            vec![0x1b, b'[', b'A']
        };
        if let Some(input_tx) = &self.input_tx {
            let _ = input_tx.try_send(Bytes::from(sequence));
        }
    }

    #[cfg(test)]
    const fn current_size(&self) -> (u16, u16) {
        self.current_size
    }

    fn refresh_child_status(&mut self) {
        if self.child_exited || self.closed {
            return;
        }
        if self
            .child
            .as_mut()
            .is_some_and(|child| child.has_exited().unwrap_or(false))
        {
            self.child_exited = true;
            self.close_pending = false;
        }
    }

    fn teardown(&mut self) {
        if self.closed {
            return;
        }
        self.refresh_child_status();
        if let Some(mut child) = self.child.take() {
            if !self.child_exited {
                let _ = child.terminate();
                let _ = child.wait();
            }
        }
        self.input_tx.take();
        if let Some(mut output_rx) = self.output_rx.take() {
            output_rx.close();
        }
        self.master.take();
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
}

trait SessionMaster: Send {
    fn resize(&self, size: PtySize) -> io::Result<()>;
}

struct SpawnedSession {
    child: Box<dyn SessionChild>,
    master: Box<dyn SessionMaster>,
    input_tx: Sender<Bytes>,
    output_rx: Receiver<Vec<u8>>,
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
