#![expect(
    clippy::await_holding_lock,
    reason = "Tests serialize process-global HOME and PATH changes while exercising child processes."
)]

use std::os::unix::fs::PermissionsExt;

use super::codex::{logout_external_cli, CodexCliError};
use super::codex_test_support::TestEnvironment;
use crate::llm::auth::oauth::oauth_cancellation;

#[tokio::test]
async fn nonzero_external_logout_returns_typed_error_without_saving_credentials(
) -> Result<(), Box<dyn std::error::Error>> {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock poisoned");
    let environment = TestEnvironment::new("codex-cli-failure")?;
    install_failing_codex(&environment, "exit 7")?;
    let (cancellation, _) = oauth_cancellation();

    let error = logout_external_cli(&cancellation).await.unwrap_err();

    assert!(matches!(error, CodexCliError::NonzeroExit));
    assert!(
        crate::config::KeyStore::get_oauth(&crate::config::AppConfig::default(), "Codex")?
            .is_none()
    );
    Ok(())
}

#[tokio::test]
async fn cancelled_external_logout_kills_the_child_and_returns_typed_error(
) -> Result<(), Box<dyn std::error::Error>> {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock poisoned");
    let environment = TestEnvironment::new("codex-cli-cancel")?;
    install_failing_codex(&environment, "sleep 30")?;
    let (cancellation, handle) = oauth_cancellation();
    handle.cancel();

    let error = logout_external_cli(&cancellation).await.unwrap_err();

    assert!(matches!(error, CodexCliError::Cancelled));
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn cancelled_codex_process_group_terminates_the_grandchild(
) -> Result<(), Box<dyn std::error::Error>> {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock poisoned");
    let environment = TestEnvironment::new("codex-cli-process-group")?;
    let pids = install_grouped_codex(&environment)?;
    let (cancellation, canceller) = oauth_cancellation();
    let login = tokio::spawn(async move { logout_external_cli(&cancellation).await });
    let (leader, grandchild) = read_pids(&pids).await?;
    let leader = rustix::process::Pid::from_raw(leader)
        .ok_or_else(|| std::io::Error::other("invalid process-group leader"))?;

    assert_eq!(rustix::process::getpgid(Some(leader))?, leader);
    assert_eq!(rustix::process::getpgid(Some(grandchild))?, leader);
    canceller.cancel();
    let error = login.await?.unwrap_err();

    assert!(matches!(error, CodexCliError::Cancelled));
    assert!(wait_for_exit(grandchild).await);
    Ok(())
}

#[tokio::test]
async fn missing_external_cli_returns_explicit_logout_guidance(
) -> Result<(), Box<dyn std::error::Error>> {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock poisoned");
    let environment = TestEnvironment::new("codex-cli-missing")?;
    let empty_path = environment.root.join("empty-path");
    std::fs::create_dir_all(&empty_path)?;
    environment.write_external_auth("not-json")?;
    std::env::set_var("PATH", empty_path);
    let (cancellation, _) = oauth_cancellation();

    let error = logout_external_cli(&cancellation).await.unwrap_err();

    assert!(matches!(error, CodexCliError::MissingCli));
    assert!(error.to_string().contains("explicitly log out"));
    Ok(())
}

#[tokio::test]
async fn external_logout_delegates_to_codex_and_requires_auth_file_removal(
) -> Result<(), Box<dyn std::error::Error>> {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock poisoned");
    let environment = TestEnvironment::new("codex-cli-logout")?;
    let arguments = install_logout_codex(&environment)?;
    environment.write_external_auth(r#"{"tokens":{"access_token":"external-access"}}"#)?;
    let (cancellation, _) = oauth_cancellation();

    logout_external_cli(&cancellation).await?;

    assert_eq!(std::fs::read_to_string(arguments)?, "logout\n");
    assert!(!environment.auth_path().exists());
    Ok(())
}

fn install_failing_codex(environment: &TestEnvironment, body: &str) -> std::io::Result<()> {
    let bin = environment.root.join("bin");
    std::fs::create_dir_all(&bin)?;
    let executable = bin.join("codex");
    std::fs::write(&executable, format!("#!/bin/sh\n{body}\n"))?;
    let mut permissions = std::fs::metadata(&executable)?.permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&executable, permissions)?;
    environment.prepend_path(&bin);
    Ok(())
}

#[cfg(unix)]
fn install_grouped_codex(environment: &TestEnvironment) -> std::io::Result<std::path::PathBuf> {
    let bin = environment.root.join("bin");
    let pids = environment.root.join("codex-pids");
    std::fs::create_dir_all(&bin)?;
    let executable = bin.join("codex");
    std::fs::write(
        &executable,
        format!(
            "#!/bin/sh\ntrap - INT\nsleep 30 &\ngrandchild=$!\nprintf '%s %s\\n' \"$$\" \"$grandchild\" > {}\nwait \"$grandchild\"\n",
            pids.display()
        ),
    )?;
    let mut permissions = std::fs::metadata(&executable)?.permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&executable, permissions)?;
    environment.prepend_path(&bin);
    Ok(pids)
}

#[cfg(unix)]
async fn read_pids(path: &std::path::Path) -> Result<(i32, rustix::process::Pid), std::io::Error> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(1);
    loop {
        if let Ok(contents) = std::fs::read_to_string(path) {
            let mut values = contents.split_whitespace();
            let leader = values
                .next()
                .ok_or_else(|| std::io::Error::other("missing process-group leader"))?
                .parse::<i32>()
                .map_err(|_| std::io::Error::other("invalid process-group leader"))?;
            let grandchild = values
                .next()
                .ok_or_else(|| std::io::Error::other("missing grandchild"))?
                .parse::<i32>()
                .map_err(|_| std::io::Error::other("invalid grandchild"))?;
            let grandchild = rustix::process::Pid::from_raw(grandchild)
                .ok_or_else(|| std::io::Error::other("invalid grandchild PID"))?;
            return Ok((leader, grandchild));
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "fake Codex did not report process IDs",
            ));
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}

#[cfg(unix)]
async fn wait_for_exit(pid: rustix::process::Pid) -> bool {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(1);
    loop {
        if rustix::process::test_kill_process(pid).is_err() || process_is_zombie(pid) {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}

#[cfg(unix)]
fn process_is_zombie(pid: rustix::process::Pid) -> bool {
    let path = format!("/proc/{}/stat", pid.as_raw_nonzero().get());
    std::fs::read_to_string(path).ok().is_some_and(|status| {
        status
            .rsplit_once(") ")
            .is_some_and(|(_, fields)| fields.starts_with('Z'))
    })
}

fn install_logout_codex(environment: &TestEnvironment) -> std::io::Result<std::path::PathBuf> {
    let bin = environment.root.join("bin");
    let arguments = environment.root.join("codex-arguments");
    std::fs::create_dir_all(&bin)?;
    let executable = bin.join("codex");
    std::fs::write(
        &executable,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}\nrm -f \"$HOME/.codex/auth.json\"\n",
            arguments.display()
        ),
    )?;
    let mut permissions = std::fs::metadata(&executable)?.permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&executable, permissions)?;
    environment.prepend_path(&bin);
    Ok(arguments)
}
