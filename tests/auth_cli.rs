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
fn auth_cli_delegates_codex_then_preserves_external_credentials_until_external_logout(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let environment = CliEnvironment::new("codex-round-trip")?;
    environment.install_codex(
        "umask 077\ncase \"$1\" in\nlogin) printf '%s' '{\"tokens\":{\"access_token\":\"external-token-canary\",\"account_id\":\"account\"}}' > \"$HOME/.codex/auth.json\" ;;\nlogout) /bin/rm -f \"$HOME/.codex/auth.json\" ;;\nesac",
    )?;

    // When
    let login = environment
        .command()
        .args(["auth", "login", "codex"])
        .output()?;
    let status = environment
        .command()
        .args(["auth", "status", "codex"])
        .output()?;
    let local_logout = environment
        .command()
        .args(["auth", "logout", "codex"])
        .output()?;
    let preserved_after_local_logout = environment.auth_path().exists();
    let repeated_local_logout = environment
        .command()
        .args(["auth", "logout", "codex"])
        .output()?;
    let external_logout = environment
        .command()
        .args(["auth", "logout", "codex", "--external"])
        .output()?;

    // Then
    assert!(login.status.success());
    assert!(status.status.success());
    assert!(local_logout.status.success());
    assert!(repeated_local_logout.status.success());
    assert!(external_logout.status.success());
    assert!(preserved_after_local_logout);
    assert!(!environment.auth_path().exists());
    assert!(String::from_utf8(status.stdout)?.contains("source=external-cli"));
    assert!(!String::from_utf8(login.stdout)?.contains("external-token-canary"));
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
        .args(["auth", "login", "openrouter", "--headless"])
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
        .args(["auth", "status", "openrouter"])
        .output()?;
    let logout = environment
        .command()
        .args(["auth", "logout", "openrouter"])
        .output()?;
    let repeated_logout = environment
        .command()
        .args(["auth", "logout", "openrouter"])
        .output()?;
    let after_logout = environment
        .command()
        .args(["auth", "status", "openrouter"])
        .output()?;

    // Then
    assert!(status.status.success());
    assert!(logout.status.success());
    assert!(repeated_logout.status.success());
    assert_eq!(after_logout.status.code(), Some(10));
    assert!(String::from_utf8(after_logout.stderr)?.contains("OpenRouter is not authenticated."));
    assert!(String::from_utf8(login.stdout)?.contains("OpenRouter authorization URL:"));
    assert!(String::from_utf8(status.stdout)?.contains("source=tcui-pkce expires_at=none"));
    assert!(!std::fs::read_to_string(environment.key_path())?.contains("openrouter-key-canary"));
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
        .args(["auth", "login", "claude"])
        .output()?;

    // Then
    assert_eq!(output.status.code(), Some(12));
    assert!(String::from_utf8(output.stderr)?.contains("Supported providers: codex, openrouter"));
    Ok(())
}
