use std::io::{self, Read, Write};
use std::path::Path;

use bytes::Bytes;
use portable_pty::{
    Child, CommandBuilder, MasterPty, NativePtySystem, PtyPair, PtySize, PtySystem,
};
use tokio::sync::mpsc::{Receiver, Sender};

use super::{SessionChild, SessionMaster, SpawnedSession, TerminalEditorError};

const OUTPUT_CHANNEL_SIZE: usize = 64;
const INPUT_CHANNEL_SIZE: usize = 64;

pub(super) struct PreparedSession {
    pair: PtyPair,
    command: CommandBuilder,
}

impl PreparedSession {
    pub(super) fn new(
        working_directory: &Path,
        size: PtySize,
    ) -> Result<Self, TerminalEditorError> {
        let editor_parts = resolve_editor()?;
        let pair = NativePtySystem::default()
            .openpty(size)
            .map_err(|error| TerminalEditorError::OpenPty(error.to_string()))?;
        let mut command = CommandBuilder::new(&editor_parts[0]);
        for arg in &editor_parts[1..] {
            command.arg(arg);
        }
        command.cwd(working_directory);
        command.env("TERM", "xterm-256color");
        Ok(Self { pair, command })
    }

    pub(super) fn spawn(mut self, path: &Path) -> Result<SpawnedSession, TerminalEditorError> {
        self.command.arg(path);
        let child = self
            .pair
            .slave
            .spawn_command(self.command)
            .map_err(|error| TerminalEditorError::Spawn(error.to_string()))?;
        drop(self.pair.slave);
        let reader = self
            .pair
            .master
            .try_clone_reader()
            .map_err(|error| TerminalEditorError::PrepareIo(error.to_string()))?;
        let writer = self
            .pair
            .master
            .take_writer()
            .map_err(|error| TerminalEditorError::PrepareIo(error.to_string()))?;
        let (output_tx, output_rx) = tokio::sync::mpsc::channel(OUTPUT_CHANNEL_SIZE);
        let (input_tx, input_rx) = tokio::sync::mpsc::channel(INPUT_CHANNEL_SIZE);
        spawn_reader(reader, output_tx);
        spawn_writer(writer, input_rx);
        Ok(SpawnedSession {
            child: Box::new(PortableChild(child)),
            master: Box::new(PortableMaster(self.pair.master)),
            input_tx,
            output_rx,
        })
    }
}

struct PortableChild(Box<dyn Child + Send + Sync>);

impl SessionChild for PortableChild {
    fn terminate(&mut self) -> io::Result<()> {
        self.0.kill()
    }

    fn has_exited(&mut self) -> io::Result<bool> {
        self.0.try_wait().map(|status| status.is_some())
    }

    fn wait(&mut self) -> io::Result<()> {
        self.0.wait().map(|_| ())
    }
}

struct PortableMaster(Box<dyn MasterPty + Send>);

impl SessionMaster for PortableMaster {
    fn resize(&self, size: PtySize) -> io::Result<()> {
        self.0
            .resize(size)
            .map_err(|error| io::Error::other(error.to_string()))
    }
}

fn spawn_reader(mut reader: Box<dyn Read + Send>, tx: Sender<Vec<u8>>) {
    tokio::task::spawn_blocking(move || {
        let mut buffer = [0u8; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(size) => {
                    if tx.blocking_send(buffer[..size].to_vec()).is_err() {
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
            if writer.write_all(&bytes).is_err() || writer.flush().is_err() {
                break;
            }
        }
    });
}

fn resolve_editor() -> Result<Vec<String>, TerminalEditorError> {
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
    Err(TerminalEditorError::EditorNotFound)
}

fn command_exists(command: &str) -> bool {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
        .any(|directory| directory.join(command).is_file())
}
