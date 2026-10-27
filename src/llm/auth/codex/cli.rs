use std::time::Duration;

#[cfg(unix)]
use std::time::Instant;

use tokio::process::{Child, Command};

use super::credential::{read_external_credential, CodexCredential, CodexCredentialError};
use crate::llm::auth::oauth::OAuthCancellation;

const CLI_TIMEOUT: Duration = Duration::from_secs(15 * 60);
#[cfg(unix)]
const TERMINATION_GRACE: Duration = Duration::from_secs(2);
#[cfg(unix)]
const REAP_POLL_INTERVAL: Duration = Duration::from_millis(10);
#[cfg(unix)]
const KILL_REAP_TIMEOUT: Duration = Duration::from_secs(1);

pub(crate) async fn login_with_cli(
    headless: bool,
    cancellation: &OAuthCancellation,
) -> Result<CodexCredential, CodexCliError> {
    match read_external_credential() {
        Ok(Some(credential)) => return Ok(credential),
        Ok(None) | Err(CodexCredentialError::Malformed) => {}
        Err(CodexCredentialError::Read) => return Err(CodexCliError::CredentialRead),
        Err(CodexCredentialError::UnsafeFile) => return Err(CodexCliError::UnsafeExternalFile),
    }
    let arguments = if headless {
        ["login", "--device-auth"].as_slice()
    } else {
        ["login"].as_slice()
    };
    run_cli(arguments, cancellation).await?;
    match read_external_credential() {
        Ok(Some(credential)) => Ok(credential),
        Ok(None) => Err(CodexCliError::PostLoginMissing),
        Err(CodexCredentialError::Malformed) => Err(CodexCliError::PostLoginMalformed),
        Err(CodexCredentialError::Read) => Err(CodexCliError::CredentialRead),
        Err(CodexCredentialError::UnsafeFile) => Err(CodexCliError::UnsafeExternalFile),
    }
}

pub(crate) async fn logout_external_cli(
    cancellation: &OAuthCancellation,
) -> Result<(), CodexCliError> {
    if matches!(
        read_external_credential(),
        Err(CodexCredentialError::UnsafeFile)
    ) {
        return Err(CodexCliError::UnsafeExternalFile);
    }
    run_cli(&["logout"], cancellation).await?;
    match read_external_credential() {
        Ok(None) => Ok(()),
        Ok(Some(_)) | Err(CodexCredentialError::Malformed) => {
            Err(CodexCliError::ExternalCredentialStillPresent)
        }
        Err(CodexCredentialError::Read) => Err(CodexCliError::CredentialRead),
        Err(CodexCredentialError::UnsafeFile) => Err(CodexCliError::UnsafeExternalFile),
    }
}

async fn run_cli(
    arguments: &[&str],
    cancellation: &OAuthCancellation,
) -> Result<(), CodexCliError> {
    let mut command = Command::new("codex");
    command
        .args(arguments)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit());
    #[cfg(unix)]
    command.kill_on_drop(false).process_group(0);
    #[cfg(not(unix))]
    command.kill_on_drop(true);
    let child = command.spawn().map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => CodexCliError::MissingCli,
        _ => CodexCliError::Launch,
    })?;
    let child = CodexChildGuard::new(child).await?;
    let status = wait_for_cli(child, cancellation).await?;
    if status.success() {
        Ok(())
    } else {
        Err(CodexCliError::NonzeroExit)
    }
}

async fn wait_for_cli(
    mut child: CodexChildGuard,
    cancellation: &OAuthCancellation,
) -> Result<std::process::ExitStatus, CodexCliError> {
    let mut cancellation = cancellation.clone();
    let outcome = {
        let process = child.process_mut()?;
        tokio::select! {
            result = process.wait() => WaitOutcome::Exited(result),
            _ = tokio::time::sleep(CLI_TIMEOUT) => WaitOutcome::TimedOut,
            _ = cancellation.cancelled() => WaitOutcome::Cancelled,
        }
    };
    match outcome {
        WaitOutcome::Exited(Ok(status)) => {
            child.disarm();
            Ok(status)
        }
        WaitOutcome::Exited(Err(error)) if is_absent_io_error(&error) => {
            child.disarm();
            Err(CodexCliError::Wait)
        }
        WaitOutcome::Exited(Err(_)) => Err(CodexCliError::Wait),
        WaitOutcome::TimedOut => {
            child.terminate().await;
            Err(CodexCliError::TimedOut)
        }
        WaitOutcome::Cancelled => {
            child.terminate().await;
            Err(CodexCliError::Cancelled)
        }
    }
}

enum WaitOutcome {
    Exited(std::io::Result<std::process::ExitStatus>),
    TimedOut,
    Cancelled,
}

struct CodexChildGuard {
    child: Option<Child>,
    #[cfg(unix)]
    process_group: rustix::process::Pid,
    #[cfg(unix)]
    term_sent_at: Option<Instant>,
    #[cfg(unix)]
    kill_sent: bool,
}

impl CodexChildGuard {
    async fn new(mut child: Child) -> Result<Self, CodexCliError> {
        #[cfg(unix)]
        let process_group = match child
            .id()
            .and_then(|pid| i32::try_from(pid).ok())
            .and_then(rustix::process::Pid::from_raw)
        {
            Some(process_group) => process_group,
            None => {
                let _ = child.kill().await;
                return Err(CodexCliError::Wait);
            }
        };
        Ok(Self {
            child: Some(child),
            #[cfg(unix)]
            process_group,
            #[cfg(unix)]
            term_sent_at: None,
            #[cfg(unix)]
            kill_sent: false,
        })
    }

    fn process_mut(&mut self) -> Result<&mut Child, CodexCliError> {
        self.child.as_mut().ok_or(CodexCliError::Wait)
    }

    fn disarm(&mut self) {
        self.child.take();
    }

    async fn terminate(&mut self) {
        #[cfg(unix)]
        {
            if self.finish_if_done() {
                return;
            }
            self.send_term();
            if self.wait_for_exit(TERMINATION_GRACE).await {
                return;
            }
            self.send_kill();
            self.wait_for_exit(KILL_REAP_TIMEOUT).await;
        }
        #[cfg(not(unix))]
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill().await;
            self.disarm();
        }
    }

    #[cfg(unix)]
    fn finish_if_done(&mut self) -> bool {
        let state = match self.child.as_mut() {
            Some(child) => child.try_wait(),
            None => return true,
        };
        match state {
            Ok(Some(_)) => {
                self.disarm();
                true
            }
            Err(error) if is_absent_io_error(&error) => {
                self.disarm();
                true
            }
            Ok(None) | Err(_) => false,
        }
    }

    #[cfg(unix)]
    fn send_term(&mut self) {
        if self.term_sent_at.is_none() && !self.kill_sent {
            let _ = signal_group(self.process_group, rustix::process::Signal::TERM);
            self.term_sent_at = Some(Instant::now());
        }
    }

    #[cfg(unix)]
    fn send_kill(&mut self) {
        let _ = signal_group(self.process_group, rustix::process::Signal::KILL);
        if let Some(child) = self.child.as_mut() {
            let _ = child.start_kill();
        }
        self.kill_sent = true;
    }

    #[cfg(unix)]
    async fn wait_for_exit(&mut self, timeout: Duration) -> bool {
        let result = match self.child.as_mut() {
            Some(child) => tokio::time::timeout(timeout, child.wait()).await,
            None => return true,
        };
        let finished = match result {
            Ok(Ok(_)) => true,
            Ok(Err(error)) => is_absent_io_error(&error),
            Err(_) => false,
        };
        if finished {
            self.disarm();
        }
        finished
    }

    #[cfg(unix)]
    fn cleanup_blocking(&mut self) {
        if self.finish_if_done() {
            return;
        }
        if !self.kill_sent {
            self.send_term();
            let remaining_grace = self.term_sent_at.map_or(Duration::ZERO, |sent_at| {
                TERMINATION_GRACE.saturating_sub(sent_at.elapsed())
            });
            let deadline = Instant::now() + remaining_grace;
            while Instant::now() < deadline {
                if self.finish_if_done() {
                    return;
                }
                std::thread::sleep(REAP_POLL_INTERVAL);
            }
        }
        self.send_kill();
        loop {
            match rustix::process::waitpid(
                Some(self.process_group),
                rustix::process::WaitOptions::empty(),
            ) {
                Ok(Some(_)) | Err(rustix::io::Errno::CHILD | rustix::io::Errno::SRCH) => break,
                Err(rustix::io::Errno::INTR) => {}
                Ok(None) | Err(_) => break,
            }
        }
        self.disarm();
    }
}

impl Drop for CodexChildGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        self.cleanup_blocking();
        #[cfg(not(unix))]
        if let Some(child) = self.child.as_mut() {
            let _ = child.start_kill();
        }
    }
}

#[cfg(unix)]
fn signal_group(process_group: rustix::process::Pid, signal: rustix::process::Signal) -> bool {
    match rustix::process::kill_process_group(process_group, signal) {
        Ok(()) => true,
        Err(rustix::io::Errno::SRCH) => false,
        Err(_) => true,
    }
}

fn is_absent_io_error(error: &std::io::Error) -> bool {
    matches!(error.raw_os_error(), Some(libc::ECHILD | libc::ESRCH))
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CodexCliError {
    #[error(
        "Codex CLI is unavailable. Install Codex and run `codex login`, or choose `--native`."
    )]
    MissingCli,
    #[error("Codex CLI could not be launched")]
    Launch,
    #[error("Codex CLI exited unsuccessfully")]
    NonzeroExit,
    #[error("Codex CLI login timed out")]
    TimedOut,
    #[error("Codex CLI login was cancelled")]
    Cancelled,
    #[error("Codex CLI process could not be observed")]
    Wait,
    #[error("Codex CLI did not create a usable credential after login")]
    PostLoginMissing,
    #[error("Codex CLI created malformed credentials after login")]
    PostLoginMalformed,
    #[error("Codex CLI credential file could not be read")]
    CredentialRead,
    #[error("Codex CLI credential file is unsafe; correct its ownership and permissions first")]
    UnsafeExternalFile,
    #[error("Codex CLI credentials remain after `codex logout`")]
    ExternalCredentialStillPresent,
}
