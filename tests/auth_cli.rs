#![cfg(unix)]

#[path = "auth_cli/failures.rs"]
mod failures;
#[path = "auth_cli/process.rs"]
mod process;
#[path = "auth_cli/support.rs"]
mod support;

use std::io::Write;
use std::process::Stdio;

use support::{configure_openrouter, CliEnvironment, JsonEndpoint};

#[test]
fn auth_cli_ignores_external_codex_credentials_until_explicit_external_logout(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let environment = CliEnvironment::new("codex-round-trip")?;
    environment
        .install_codex("if [ \"$1\" = logout ]; then /bin/rm -f \"$HOME/.codex/auth.json\"; fi")?;
    std::fs::write(
        environment.auth_path(),
        r#"{"tokens":{"access_token":"external-token-canary","account_id":"account"}}"#,
    )?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(
        environment.auth_path(),
        std::fs::Permissions::from_mode(0o600),
    )?;
    let status = environment
        .command()
        .args(["auth", "--plaintext", "status", "codex"])
        .output()?;
    let local_logout = environment
        .command()
        .args(["auth", "--plaintext", "logout", "codex"])
        .output()?;
    let preserved_after_local_logout = environment.auth_path().exists();
    let repeated_local_logout = environment
        .command()
        .args(["auth", "--plaintext", "logout", "codex"])
        .output()?;
    let external_logout = environment
        .command()
        .args(["auth", "--plaintext", "logout", "codex", "--external"])
        .output()?;

    // Then
    assert_eq!(status.status.code(), Some(10));
    assert!(local_logout.status.success());
    assert!(repeated_local_logout.status.success());
    assert!(external_logout.status.success());
    assert!(preserved_after_local_logout);
    assert!(!environment.auth_path().exists());
    assert!(!String::from_utf8(status.stdout)?.contains("external-token-canary"));
    Ok(())
}

#[test]
fn auth_cli_exchanges_openrouter_headless_code_then_reports_and_removes_only_local_credential(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let environment = CliEnvironment::new("openrouter-round-trip")?;
    let endpoint = JsonEndpoint::respond_once(r#"{"key":"openrouter-key-canary"}"#)?;
    let mut login = environment.command();
    configure_openrouter(&mut login, &endpoint);
    login
        .args(["auth", "--plaintext", "login", "openrouter", "--headless"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());

    // When
    let mut child = login.spawn()?;
    let mut stdin = child.stdin.take().ok_or("headless stdin missing")?;
    stdin.write_all(b"headless-code\n")?;
    drop(stdin);
    let login = child.wait_with_output()?;
    assert!(login.status.success());
    endpoint.finish()?;
    let status = environment
        .command()
        .args(["auth", "--plaintext", "status", "openrouter"])
        .output()?;
    let logout = environment
        .command()
        .args(["auth", "--plaintext", "logout", "openrouter"])
        .output()?;
    let repeated_logout = environment
        .command()
        .args(["auth", "--plaintext", "logout", "openrouter"])
        .output()?;
    let after_logout = environment
        .command()
        .args(["auth", "--plaintext", "status", "openrouter"])
        .output()?;

    // Then
    assert!(status.status.success());
    assert!(logout.status.success());
    assert!(repeated_logout.status.success());
    assert_eq!(after_logout.status.code(), Some(10));
    assert!(String::from_utf8(after_logout.stderr)?.contains("OpenRouter is not authenticated."));
    assert!(String::from_utf8(login.stdout)?.contains("OpenRouter authorization URL:"));
    assert!(String::from_utf8(status.stdout)?.contains("source=tcui-pkce expires_at=none"));
    assert!(environment.key_path().exists());
    assert!(!environment.root_path().join("data/tcui/keys.toml").exists());
    Ok(())
}

#[test]
fn auth_cli_rejects_unsupported_provider_without_opening_a_browser(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let environment = CliEnvironment::new("unsupported")?;

    // When
    let output = environment
        .command()
        .args(["auth", "--plaintext", "login", "claude"])
        .output()?;

    // Then
    assert_eq!(output.status.code(), Some(12));
    assert!(String::from_utf8(output.stderr)?.contains("Supported providers: codex, openrouter"));
    Ok(())
}

#[test]
fn auth_cli_requires_explicit_storage_choice_before_browser_login(
) -> Result<(), Box<dyn std::error::Error>> {
    let environment = CliEnvironment::new("storage-choice")?;
    let output = environment
        .command()
        .args(["auth", "login", "openrouter"])
        .output()?;
    assert_eq!(output.status.code(), Some(14));
    let error = String::from_utf8(output.stderr)?;
    assert!(error.contains("--plaintext") && error.contains("--vault-password-stdin"));
    assert!(!String::from_utf8(output.stdout)?.contains("authorization URL"));
    Ok(())
}

#[test]
fn auth_cli_encrypts_console_login_with_explicit_password_stdin(
) -> Result<(), Box<dyn std::error::Error>> {
    let environment = CliEnvironment::new("encrypted-login")?;
    let endpoint = JsonEndpoint::respond_once(r#"{"key":"encrypted-oauth-canary"}"#)?;
    let mut command = environment.command();
    configure_openrouter(&mut command, &endpoint);
    let mut child = command
        .args([
            "auth",
            "--vault-password-stdin",
            "login",
            "openrouter",
            "--headless",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or("stdin missing")?
        .write_all(b"fixture vault password\nheadless-code\n")?;
    let output = child.wait_with_output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    endpoint.finish()?;
    let bytes = std::fs::read(environment.key_path())?;
    assert!(!bytes
        .windows(b"encrypted-oauth-canary".len())
        .any(|window| window == b"encrypted-oauth-canary"));
    let locked = environment
        .command()
        .args(["auth", "status", "openrouter"])
        .output()?;
    assert_eq!(locked.status.code(), Some(14));
    let mut child = environment
        .command()
        .args(["auth", "--vault-password-stdin", "status", "openrouter"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or("stdin missing")?
        .write_all(b"fixture vault password\n")?;
    assert!(child.wait_with_output()?.status.success());
    Ok(())
}
