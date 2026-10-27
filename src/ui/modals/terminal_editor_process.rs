use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use bytes::Bytes;
use portable_pty::{
    Child, CommandBuilder, MasterPty, NativePtySystem, PtyPair, PtySize, PtySystem,
};
use tokio::sync::mpsc::{Receiver, Sender};

use super::{SessionChild, SessionMaster, SpawnedSession, TerminalEditorError};

const OUTPUT_CHANNEL_SIZE: usize = 64;
const INPUT_CHANNEL_SIZE: usize = 64;

pub(super) fn default_shell_program() -> String {
    CommandBuilder::new_default_prog().get_shell()
}

pub(super) struct TransportState {
    pub(super) errors: Receiver<String>,
    cancelled: Arc<AtomicBool>,
}

impl Drop for TransportState {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

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
        let mut command = CommandBuilder::new(&editor_parts[0]);
        command.args(&editor_parts[1..]);
        Self::prepare(command, working_directory, size)
    }

    pub(super) fn shell(
        shell: Option<&str>,
        working_directory: &Path,
        size: PtySize,
    ) -> Result<Self, TerminalEditorError> {
        let command = match shell {
            Some(executable) if !executable.trim().is_empty() => CommandBuilder::new(executable),
            _ => CommandBuilder::new_default_prog(),
        };
        Self::prepare(command, working_directory, size)
    }

    pub(super) fn command_shell(
        shell: &str,
        working_directory: &Path,
        size: PtySize,
        readiness: &str,
    ) -> Result<Self, TerminalEditorError> {
        Self::prepare(
            clean_shell_command(shell, readiness)?,
            working_directory,
            size,
        )
    }

    fn prepare(
        mut command: CommandBuilder,
        working_directory: &Path,
        size: PtySize,
    ) -> Result<Self, TerminalEditorError> {
        let working_directory = working_directory
            .canonicalize()
            .map_err(|error| TerminalEditorError::WorkingDirectory(error.to_string()))?;
        if !working_directory.is_dir() {
            return Err(TerminalEditorError::WorkingDirectory(
                "Not a directory".into(),
            ));
        }
        let pair = NativePtySystem::default()
            .openpty(size)
            .map_err(|error| TerminalEditorError::OpenPty(error.to_string()))?;
        command.cwd(&working_directory);
        command.env("TERM", "xterm-256color");
        Ok(Self { pair, command })
    }

    pub(super) fn spawn(mut self, path: &Path) -> Result<SpawnedSession, TerminalEditorError> {
        self.command.arg(path);
        self.start()
    }

    pub(super) fn start(self) -> Result<SpawnedSession, TerminalEditorError> {
        // Prepare every fallible I/O resource before spawning a child.
        let reader = self
            .pair
            .master
            .try_clone_reader()
            .map_err(|error| TerminalEditorError::PrepareIo(error.to_string()))?;
        let writer = terminal_writer(self.pair.master.as_ref())
            .map_err(|error| TerminalEditorError::PrepareIo(error.to_string()))?;
        #[cfg(unix)]
        if let Some(fd) = self.pair.master.as_raw_fd() {
            // All master duplicates share these flags. Nonblocking I/O lets the workers
            // observe cancellation even when descendants retain the slave terminal.
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
            {
                return Err(TerminalEditorError::PrepareIo(
                    io::Error::last_os_error().to_string(),
                ));
            }
        }
        let child = self
            .pair
            .slave
            .spawn_command(self.command)
            .map_err(|error| TerminalEditorError::Spawn(error.to_string()))?;
        drop(self.pair.slave);
        let (output_tx, output_rx) = tokio::sync::mpsc::channel(OUTPUT_CHANNEL_SIZE);
        let (input_tx, input_rx) = tokio::sync::mpsc::channel(INPUT_CHANNEL_SIZE);
        let (error_tx, errors) = tokio::sync::mpsc::channel(8);
        let cancelled = Arc::new(AtomicBool::new(false));
        spawn_reader(reader, output_tx, error_tx.clone(), Arc::clone(&cancelled));
        spawn_writer(writer, input_rx, error_tx, Arc::clone(&cancelled));
        Ok(SpawnedSession {
            child: Box::new(PortableChild(Some(child))),
            master: Box::new(PortableMaster(self.pair.master)),
            input_tx,
            output_rx,
            transport: Some(TransportState { errors, cancelled }),
        })
    }
}

fn clean_shell_command(
    shell: &str,
    readiness: &str,
) -> Result<CommandBuilder, TerminalEditorError> {
    let name = Path::new(shell)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(TerminalEditorError::UnsupportedCommandShell)?;
    let mut command = CommandBuilder::new(shell);
    match name {
        "sh" => command.arg("-i"),
        "bash" => command.args(["--noprofile", "--norc", "-i"]),
        "zsh" => command.args(["-f", "-d", "-i"]),
        "fish" => {
            command.args(["--no-config", "--interactive", "--init-command"]);
            command.arg(format!(
                "function fish_prompt; printf '\\033]133;A;{readiness}\\007$ '; end"
            ));
        }
        _ => return Err(TerminalEditorError::UnsupportedCommandShell),
    }
    clear_startup_environment(&mut command);
    // Readline configuration can install startup hooks or rewrite typed input.
    command.env("INPUTRC", "/dev/null");
    let marker = format!("\x1b]133;A;{readiness}\x07");
    let prompt = match name {
        "bash" => format!("\\[{marker}\\]$ "),
        "zsh" => format!("%{{{marker}%}}$ "),
        _ => format!("{marker}$ "),
    };
    command.env("PS1", prompt);
    Ok(command)
}

fn clear_startup_environment(command: &mut CommandBuilder) {
    for variable in [
        "ENV",
        "BASH_ENV",
        "ZDOTDIR",
        "SHELLOPTS",
        "BASHOPTS",
        "BASH_COMPAT",
        "PROMPT_COMMAND",
        "PS0",
        "PS1",
        "PS2",
        "PS4",
    ] {
        command.env_remove(variable);
    }
    let functions: Vec<_> = command
        .iter_full_env_as_str()
        .filter(|(key, _)| key.starts_with("BASH_FUNC_"))
        .map(|(key, _)| key.to_owned())
        .collect();
    for variable in functions {
        command.env_remove(variable);
    }
    // The string iterator omits non-UTF-8 values. Remove inherited function
    // exports by their OS-native names as well, regardless of definition bytes.
    for (variable, _) in std::env::vars_os() {
        if variable.as_encoded_bytes().starts_with(b"BASH_FUNC_") {
            command.env_remove(variable);
        }
    }
}

struct PortableChild(Option<Box<dyn Child + Send + Sync>>);

impl SessionChild for PortableChild {
    fn terminate(&mut self) -> io::Result<()> {
        let Some(child) = self.0.as_mut() else {
            return Ok(());
        };
        #[cfg(unix)]
        if let Some(pid) = child.process_id().and_then(|pid| i32::try_from(pid).ok()) {
            // portable-pty establishes a new session/process group for this child.
            // SIGKILL avoids portable-pty's synchronous SIGHUP grace-period wait.
            let result = unsafe { libc::kill(-pid, libc::SIGKILL) };
            if result != 0 && io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH) {
                return Err(io::Error::last_os_error());
            }
            return Ok(());
        }
        child.kill()
    }

    fn has_exited(&mut self) -> io::Result<bool> {
        #[cfg(target_os = "linux")]
        {
            let Some(pid) = self.process_id() else {
                return Ok(true);
            };
            let mut status: libc::siginfo_t = unsafe { std::mem::zeroed() };
            // Observe without reaping so the PID/session ID cannot be reused
            // before the background reaper terminates remaining session jobs.
            if unsafe {
                libc::waitid(
                    libc::P_PID,
                    pid,
                    &mut status,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            } != 0
            {
                return Err(io::Error::last_os_error());
            }
            let exited = unsafe { status.si_pid() } != 0;
            if exited {
                self.wait()?;
            }
            Ok(exited)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let exited = self.0.as_mut().map_or(Ok(true), |child| {
                child.try_wait().map(|status| status.is_some())
            })?;
            if exited {
                self.0.take();
            }
            Ok(exited)
        }
    }

    fn wait(&mut self) -> io::Result<()> {
        #[cfg(target_os = "linux")]
        let session_id = self
            .0
            .as_ref()
            .and_then(|child| child.process_id())
            .and_then(|pid| i32::try_from(pid).ok());
        if let Some(mut child) = self.0.take() {
            // Reap outside the UI thread, including during application shutdown.
            std::thread::spawn(move || {
                #[cfg(target_os = "linux")]
                if let Some(session_id) = session_id {
                    terminate_session_processes(session_id);
                }
                let _ = child.wait();
            });
        }
        Ok(())
    }

    fn current_directory(&self) -> Option<std::path::PathBuf> {
        #[cfg(target_os = "linux")]
        {
            let pid = self.0.as_ref()?.process_id()?;
            std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
        }
        #[cfg(not(target_os = "linux"))]
        {
            None
        }
    }

    fn process_id(&self) -> Option<u32> {
        self.0.as_ref().and_then(|child| child.process_id())
    }
}

impl Drop for PortableChild {
    fn drop(&mut self) {
        if self.0.is_some() {
            let _ = self.terminate();
            let _ = self.wait();
        }
    }
}

struct PortableMaster(Box<dyn MasterPty + Send>);

impl SessionMaster for PortableMaster {
    fn resize(&self, size: PtySize) -> io::Result<()> {
        self.0
            .resize(size)
            .map_err(|error| io::Error::other(error.to_string()))
    }

    fn terminate_foreground(&self) -> io::Result<()> {
        #[cfg(unix)]
        if let Some(group) = self.0.process_group_leader().filter(|group| *group > 0) {
            if unsafe { libc::kill(-group, libc::SIGKILL) } != 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() != Some(libc::ESRCH) {
                    return Err(error);
                }
            }
        }
        Ok(())
    }

    fn foreground_group(&self) -> Option<u32> {
        #[cfg(unix)]
        {
            self.0
                .process_group_leader()
                .and_then(|group| u32::try_from(group).ok())
        }
        #[cfg(not(unix))]
        {
            None
        }
    }
}

fn terminal_writer(master: &dyn MasterPty) -> io::Result<Box<dyn Write + Send>> {
    #[cfg(unix)]
    {
        use std::os::fd::FromRawFd;
        let fd = master
            .as_raw_fd()
            .ok_or_else(|| io::Error::other("PTY has no file descriptor"))?;
        // portable-pty's Unix writer Drop injects newline + EOF, which can execute
        // unsubmitted input. Own a plain duplicate instead; closing sends no bytes.
        let duplicate = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) };
        if duplicate < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Box::new(unsafe { std::fs::File::from_raw_fd(duplicate) }))
    }
    #[cfg(not(unix))]
    master
        .take_writer()
        .map_err(|error| io::Error::other(error.to_string()))
}

#[cfg(target_os = "linux")]
fn terminate_session_processes(session_id: i32) {
    // The unreaped leader still owns its PID, preventing session-ID reuse while
    // this background cleanup also terminates shell jobs in other process groups.
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return;
    };
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<i32>().ok())
        else {
            continue;
        };
        if pid > 0 && unsafe { libc::getsid(pid) } == session_id {
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
        }
    }
}

fn retry_io(cancelled: &AtomicBool) -> bool {
    if cancelled.load(Ordering::Acquire) {
        return false;
    }
    std::thread::sleep(std::time::Duration::from_millis(5));
    !cancelled.load(Ordering::Acquire)
}

fn spawn_reader(
    mut reader: Box<dyn Read + Send>,
    tx: Sender<Vec<u8>>,
    errors: Sender<String>,
    cancelled: Arc<AtomicBool>,
) {
    tokio::task::spawn_blocking(move || {
        let mut buffer = [0u8; 4096];
        while !cancelled.load(Ordering::Acquire) {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(size) => {
                    if tx.blocking_send(buffer[..size].to_vec()).is_err() {
                        break;
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    if !retry_io(&cancelled) {
                        break;
                    }
                }
                // Linux PTYs report EIO after the last slave closes.
                #[cfg(unix)]
                Err(error) if error.raw_os_error() == Some(libc::EIO) => break,
                Err(error) => {
                    let _ = errors.blocking_send(format!("Terminal output failed: {error}"));
                    break;
                }
            }
        }
    });
}

fn spawn_writer(
    mut writer: Box<dyn Write + Send>,
    mut rx: Receiver<Bytes>,
    errors: Sender<String>,
    cancelled: Arc<AtomicBool>,
) {
    // PTY writes can block even when the queue itself is asynchronous.
    tokio::task::spawn_blocking(move || {
        while let Some(bytes) = rx.blocking_recv() {
            let mut remaining = bytes.as_ref();
            while !remaining.is_empty() && !cancelled.load(Ordering::Acquire) {
                match writer.write(remaining) {
                    Ok(0) => {
                        let _ = errors.blocking_send("Terminal input failed: write returned zero; queued input may be incomplete".into());
                        return;
                    }
                    Ok(size) => remaining = &remaining[size..],
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        if !retry_io(&cancelled) {
                            return;
                        }
                    }
                    Err(error) => {
                        let _ = errors.blocking_send(format!(
                            "Terminal input failed: {error}; queued input may be incomplete"
                        ));
                        return;
                    }
                }
            }
            if cancelled.load(Ordering::Acquire) {
                return;
            }
            if let Err(error) = writer.flush() {
                let _ = errors.blocking_send(format!(
                    "Terminal input flush failed: {error}; queued input may be incomplete"
                ));
                return;
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

#[cfg(test)]
mod transport_tests {
    use super::*;

    struct FailedWriter;

    impl Write for FailedWriter {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed PTY"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn worker_reports_input_failure_and_closes_queue() {
        let (input_tx, input_rx) = tokio::sync::mpsc::channel(4);
        let (error_tx, mut error_rx) = tokio::sync::mpsc::channel(4);
        spawn_writer(
            Box::new(FailedWriter),
            input_rx,
            error_tx,
            Arc::new(AtomicBool::new(false)),
        );
        input_tx
            .send(Bytes::from_static(b"approved command\r"))
            .await
            .expect("queue");
        let error = tokio::time::timeout(std::time::Duration::from_secs(2), error_rx.recv())
            .await
            .expect("failure deadline")
            .expect("reported error");
        assert!(error.contains("Terminal input failed: closed PTY"));
        assert!(error.contains("queued input may be incomplete"));
        input_tx.closed().await;
    }
}

#[cfg(test)]
mod command_startup_tests {
    use super::*;

    #[test]
    fn clean_command_shell_disables_supported_user_startup_paths() {
        for (shell, expected) in [
            ("sh", vec!["sh", "-i"]),
            ("bash", vec!["bash", "--noprofile", "--norc", "-i"]),
            ("zsh", vec!["zsh", "-f", "-d", "-i"]),
        ] {
            let command = clean_shell_command(shell, "test-marker").expect("clean shell");
            let argv: Vec<_> = command
                .get_argv()
                .iter()
                .map(|argument| argument.to_str().expect("argument"))
                .collect();
            assert_eq!(argv, expected);
            assert_eq!(
                command.get_env("INPUTRC"),
                Some(std::ffi::OsStr::new("/dev/null"))
            );
            assert!(command
                .get_env("PS1")
                .expect("prompt")
                .to_string_lossy()
                .contains("test-marker"));
        }
        let fish = clean_shell_command("fish", "test-marker").expect("fish");
        assert_eq!(fish.get_argv()[1], "--no-config");
        assert_eq!(fish.get_argv()[2], "--interactive");
        assert!(fish.get_argv()[4]
            .to_string_lossy()
            .contains("function fish_prompt"));
        assert!(matches!(
            clean_shell_command("python", "test-marker"),
            Err(TerminalEditorError::UnsupportedCommandShell)
        ));
    }

    #[test]
    fn startup_environment_removes_hooks_and_exported_functions_but_keeps_path() {
        let mut command = CommandBuilder::new("bash");
        for variable in [
            "ENV",
            "BASH_ENV",
            "ZDOTDIR",
            "SHELLOPTS",
            "BASHOPTS",
            "BASH_COMPAT",
            "PROMPT_COMMAND",
            "PS0",
            "PS1",
            "PS2",
            "PS4",
            "BASH_FUNC_read%%",
        ] {
            command.env(variable, "untrusted startup hook");
        }
        command.env("PATH", "/trusted/bin");
        clear_startup_environment(&mut command);
        for variable in [
            "ENV",
            "BASH_ENV",
            "ZDOTDIR",
            "SHELLOPTS",
            "BASHOPTS",
            "BASH_COMPAT",
            "PROMPT_COMMAND",
            "PS0",
            "PS1",
            "PS2",
            "PS4",
            "BASH_FUNC_read%%",
        ] {
            assert!(command.get_env(variable).is_none(), "{variable}");
        }
        assert_eq!(
            command.get_env("PATH"),
            Some(std::ffi::OsStr::new("/trusted/bin"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn startup_paths_are_removed_even_when_values_are_not_utf8() {
        use std::os::unix::ffi::OsStringExt;
        let mut command = CommandBuilder::new("sh");
        command.env("ENV", std::ffi::OsString::from_vec(vec![b'/', 0xff]));
        command.env("BASH_ENV", std::ffi::OsString::from_vec(vec![b'/', 0xfe]));
        clear_startup_environment(&mut command);
        assert!(command.get_env("ENV").is_none());
        assert!(command.get_env("BASH_ENV").is_none());
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn real_clean_bash_does_not_enter_user_rc_reads() {
        if !Path::new("/bin/bash").is_file() {
            return;
        }
        let base = std::env::temp_dir().join(format!(
            "tcui-command-startup-{}-{}",
            std::process::id(),
            rand::random::<u64>(),
        ));
        std::fs::create_dir_all(&base).expect("fixture");
        for file in [".bashrc", ".bash_profile", ".profile"] {
            std::fs::write(base.join(file),
                "printf entered > startup-entered; read -r stolen; printf '%s' \"$stolen\" > stolen\n")
                .expect("startup trap");
        }
        let area = ratatui::layout::Rect::new(0, 0, 80, 24);
        let mut prepared = PreparedSession::command_shell(
            "/bin/bash",
            &base,
            super::super::initial_pty_size(area),
            "startup-regression",
        )
        .expect("prepare");
        prepared.command.env("HOME", &base);
        let mut state =
            super::super::TerminalEditorState::from_spawned(prepared.start().expect("spawn"), area);
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let mut output = Vec::new();
            loop {
                let bytes = state
                    .output_rx
                    .as_mut()
                    .expect("output")
                    .recv()
                    .await
                    .expect("bytes");
                output.extend_from_slice(&bytes);
                if output
                    .windows(b"\x1b]133;A;startup-regression\x07".len())
                    .any(|window| window == b"\x1b]133;A;startup-regression\x07")
                {
                    break;
                }
            }
        })
        .await
        .expect("clean prompt deadline");
        assert!(!base.join("startup-entered").exists());
        state.confirm_close();
        std::fs::remove_dir_all(base).expect("cleanup");
    }
}
