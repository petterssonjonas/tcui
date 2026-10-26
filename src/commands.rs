//! Direct process launches only. Approval does not sandbox a program or its descendants.
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use color_eyre::eyre::{eyre, Result};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};

use crate::config::AppConfig;

const TIMEOUT: Duration = Duration::from_secs(30);
const MAX_OUTPUT_BYTES: usize = 64 * 1024;
const MAX_ARGUMENT_BYTES: usize = 32 * 1024;

#[derive(Debug)]
pub(crate) struct CommandOutput {
    pub(crate) success: bool,
    pub(crate) code: Option<i32>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

/// Resolve an approved executable, never granting approval by basename alone.
pub(crate) fn authorize(config: &AppConfig, program: &str) -> Result<PathBuf> {
    let resolved = resolve(program)?;
    for denied in &config.commands.blacklist {
        validate_program(denied)?;
        let path = Path::new(denied);
        let blocked = if path.is_absolute() {
            std::fs::canonicalize(path).is_ok_and(|target| target == resolved)
        } else {
            Path::new(program).file_name() == path.file_name()
                || resolved.file_name() == path.file_name()
                || resolve(denied).is_ok_and(|target| target == resolved)
        };
        if blocked {
            return Err(eyre!("command is blacklisted"));
        }
    }
    if config
        .commands
        .whitelist
        .iter()
        .any(|allowed| resolve(allowed).is_ok_and(|target| target == resolved))
    {
        return Ok(resolved);
    }
    Err(eyre!("command is not whitelisted"))
}

fn validate_program(program: &str) -> Result<()> {
    let path = Path::new(program);
    if program.is_empty()
        || program.chars().any(char::is_control)
        || (!path.is_absolute()
            && (program.contains(['/', '\\'])
                || program.chars().any(char::is_whitespace)
                || matches!(program, "." | "..")
                || program.starts_with('-')))
    {
        return Err(eyre!(
            "use an absolute executable path or a simple program name"
        ));
    }
    Ok(())
}

fn executable(path: &Path) -> Result<PathBuf> {
    let canonical = std::fs::canonicalize(path)?;
    let metadata = std::fs::metadata(&canonical)?;
    if !metadata.is_file() {
        return Err(eyre!("command is not a regular executable file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(eyre!("command is not executable"));
        }
    }
    Ok(canonical)
}

fn resolve(program: &str) -> Result<PathBuf> {
    validate_program(program)?;
    if Path::new(program).is_absolute() {
        return executable(Path::new(program));
    }
    let path = std::env::var_os("PATH").ok_or_else(|| eyre!("PATH is unavailable"))?;
    for directory in std::env::split_paths(&path).filter(|directory| directory.is_absolute()) {
        if let Ok(resolved) = executable(&directory.join(program)) {
            return Ok(resolved);
        }
    }
    Err(eyre!("command executable was not found on PATH"))
}

pub(crate) async fn run(
    config: &AppConfig,
    program: &str,
    arguments: &[String],
) -> Result<CommandOutput> {
    run_with_timeout(config, program, arguments, None, TIMEOUT).await
}

/// Send bounded private input without placing it in arguments or the child environment.
pub(crate) async fn run_with_input(
    config: &AppConfig,
    program: &str,
    arguments: &[String],
    input: &[u8],
) -> Result<CommandOutput> {
    run_with_timeout(config, program, arguments, Some(input), TIMEOUT).await
}

async fn run_with_timeout(
    config: &AppConfig,
    program: &str,
    arguments: &[String],
    input: Option<&[u8]>,
    timeout: Duration,
) -> Result<CommandOutput> {
    let program = authorize(config, program)?;
    if arguments.len() > 128
        || arguments.iter().map(String::len).sum::<usize>() > MAX_ARGUMENT_BYTES
        || arguments.iter().any(|argument| argument.contains('\0'))
        || input.is_some_and(|input| input.len() > MAX_ARGUMENT_BYTES)
    {
        return Err(eyre!("invalid or excessive command arguments or input"));
    }
    let mut command = Command::new(program);
    command
        .args(arguments)
        .env_clear()
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    // Never inherit API keys, tokens, shell startup hooks, or loader overrides.
    for name in [
        "HOME",
        "PATH",
        "LANG",
        "LANGUAGE",
        "LC_ALL",
        "LC_CTYPE",
        "LC_MESSAGES",
        "LC_TIME",
        "LC_NUMERIC",
        "LC_COLLATE",
        "LC_MONETARY",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
        "XDG_STATE_HOME",
        "XDG_RUNTIME_DIR",
        "TZ",
    ] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    #[cfg(unix)]
    command.process_group(0);
    let mut process = ProcessGuard::new(command.spawn()?)?;
    let stdout = process
        .child
        .stdout
        .take()
        .ok_or_else(|| eyre!("missing command stdout"))?;
    let stderr = process
        .child
        .stderr
        .take()
        .ok_or_else(|| eyre!("missing command stderr"))?;
    let stdin = process.child.stdin.take();
    let completion = async {
        let ((), (), stdout, stderr) = tokio::try_join!(
            process.wait_for_exit(),
            async {
                if let (Some(mut stdin), Some(input)) = (stdin, input) {
                    stdin.write_all(input).await?;
                    stdin.shutdown().await?;
                }
                Ok::<_, color_eyre::Report>(())
            },
            read_bounded(stdout),
            read_bounded(stderr),
        )?;
        let status = process.finish().await?;
        Ok::<_, color_eyre::Report>((status, stdout, stderr))
    };
    let (status, stdout, stderr) = tokio::time::timeout(timeout, completion)
        .await
        .map_err(|_| eyre!("command timed out"))??;
    Ok(CommandOutput {
        success: status.success(),
        code: status.code(),
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    })
}

async fn read_bounded(mut reader: impl AsyncRead + Unpin) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        let count = reader.read(&mut buffer).await?;
        if count == 0 {
            return Ok(output);
        }
        if output.len() + count > MAX_OUTPUT_BYTES {
            return Err(eyre!("command output limit exceeded"));
        }
        output.extend_from_slice(&buffer[..count]);
    }
}

struct ProcessGuard {
    child: Child,
    #[cfg(unix)]
    group: Option<rustix::process::Pid>,
}

impl ProcessGuard {
    fn new(mut child: Child) -> Result<Self> {
        #[cfg(unix)]
        let group = match child
            .id()
            .and_then(|id| i32::try_from(id).ok())
            .and_then(rustix::process::Pid::from_raw)
        {
            Some(group) => group,
            None => {
                let _ = child.start_kill();
                return Err(eyre!("command process ID unavailable"));
            }
        };
        Ok(Self {
            child,
            #[cfg(unix)]
            group: Some(group),
        })
    }

    async fn wait_for_exit(&mut self) -> Result<()> {
        #[cfg(unix)]
        {
            use rustix::process::{waitid, WaitId, WaitIdOptions};
            let group = self
                .group
                .ok_or_else(|| eyre!("command process is already completed"))?;
            loop {
                // Keep the exited leader waitable until group cleanup. Its PID/PGID
                // cannot be reused while this child remains unreaped.
                match waitid(
                    WaitId::Pid(group),
                    WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
                ) {
                    Ok(Some(_)) => return Ok(()),
                    Ok(None) | Err(rustix::io::Errno::INTR) => {}
                    Err(rustix::io::Errno::CHILD) => {
                        // An unexpected external reaper invalidates our group identity.
                        self.group = None;
                        return Err(eyre!("command child was unexpectedly reaped"));
                    }
                    Err(error) => return Err(error.into()),
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
        #[cfg(not(unix))]
        {
            self.child.wait().await?;
            Ok(())
        }
    }

    async fn finish(&mut self) -> Result<std::process::ExitStatus> {
        #[cfg(unix)]
        self.stop_group()?;
        Ok(self.child.wait().await?)
    }

    #[cfg(unix)]
    fn stop_group(&mut self) -> Result<()> {
        if let Some(group) = self.group.take() {
            match rustix::process::kill_process_group(group, rustix::process::Signal::KILL) {
                Ok(()) | Err(rustix::io::Errno::SRCH) => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }
}

impl Drop for ProcessGuard {
    fn drop(&mut self) {
        // Never block a runtime worker in Drop. Signal before dropping the still
        // unreaped leader; Tokio's kill_on_drop child cleanup schedules its reaping.
        #[cfg(unix)]
        let _ = self.stop_group();
    }
}

#[cfg(all(test, unix))]
mod tests;
