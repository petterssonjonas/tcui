#![cfg(unix)]

#[path = "auth_cli/failures.rs"]
mod failures;
#[cfg(target_os = "linux")]
#[path = "auth_cli/password_fd.rs"]
mod password_fd;
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

    let plaintext_endpoint = JsonEndpoint::respond_once(r#"{"key":"plaintext-oauth-canary"}"#)?;
    let mut command = environment.command();
    configure_openrouter(&mut command, &plaintext_endpoint);
    let mut child = command
        .args(["auth", "--plaintext", "login", "openrouter", "--headless"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or("stdin missing")?
        .write_all(b"headless-code\n")?;
    let output = child.wait_with_output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    plaintext_endpoint.finish()?;
    let database = rusqlite::Connection::open(environment.key_path())?;
    let mut statement = database.prepare(
        "SELECT id,encrypted FROM tcui_credentials WHERE kind='api_key_owned' ORDER BY id",
    )?;
    let records = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(
        records,
        vec![
            ("OpenRouter OAuth".to_string(), true),
            ("OpenRouter OAuth::plaintext".to_string(), false),
        ]
    );
    drop(statement);
    drop(database);
    let bytes = std::fs::read(environment.key_path())?;
    assert!(!bytes
        .windows(b"encrypted-oauth-canary".len())
        .any(|window| window == b"encrypted-oauth-canary"));
    assert!(bytes
        .windows(b"plaintext-oauth-canary".len())
        .any(|window| window == b"plaintext-oauth-canary"));
    assert!(environment
        .command()
        .args(["auth", "--plaintext", "status", "openrouter"])
        .output()?
        .status
        .success());
    assert!(environment
        .command()
        .args(["auth", "--plaintext", "logout", "openrouter"])
        .output()?
        .status
        .success());
    let database = rusqlite::Connection::open(environment.key_path())?;
    let retained: bool = database.query_row(
        "SELECT encrypted FROM tcui_credentials WHERE kind='api_key_owned' AND id='OpenRouter OAuth'",
        [], |row| row.get(0),
    )?;
    assert!(retained);
    Ok(())
}

#[test]
fn auth_cli_initializes_configured_plaintext_password_and_preserves_callback_and_sibling(
) -> Result<(), Box<dyn std::error::Error>> {
    let environment = CliEnvironment::new("configured-plaintext-initialize")?;
    let plaintext = JsonEndpoint::respond_once(r#"{"key":"existing-plaintext-oauth"}"#)?;
    let mut command = environment.command();
    configure_openrouter(&mut command, &plaintext);
    let mut child = command
        .args(["auth", "--plaintext", "login", "openrouter", "--headless"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or("stdin missing")?
        .write_all(b"first-code\n")?;
    let output = child.wait_with_output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    plaintext.finish()?;

    let policy = serde_json::json!({
        "app_password": false,
        "credentials": false,
        "chats": false,
        "memories": false,
        "vault": false
    });
    let manifest_path = environment.root_path().join("data/tcui/protection.json");
    std::fs::write(
        &manifest_path,
        serde_json::to_vec(&serde_json::json!({
            "version": 1,
            "policy": policy,
            "authentication": null
        }))?,
    )?;
    let encrypted = JsonEndpoint::respond_once(r#"{"key":"first-encrypted-oauth-canary"}"#)?;
    let mut command = environment.command();
    configure_openrouter(&mut command, &encrypted);
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
        .write_all(b"first standalone password\nsecond-code\n")?;
    let output = child.wait_with_output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    encrypted.finish()?;

    let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(manifest_path)?)?;
    assert_eq!(manifest["policy"], policy);
    assert_eq!(
        manifest["authentication"].as_array().map(Vec::len),
        Some(32)
    );
    let database = rusqlite::Connection::open(environment.key_path())?;
    let records: Vec<(String, bool)> = database
        .prepare(
            "SELECT id,encrypted FROM tcui_credentials WHERE kind='api_key_owned' ORDER BY id",
        )?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    assert_eq!(
        records,
        [
            ("OpenRouter OAuth".into(), false),
            ("OpenRouter OAuth::encrypted".into(), true)
        ]
    );
    let original: Vec<u8> = database.query_row(
        "SELECT payload FROM tcui_credentials WHERE kind='api_key_owned' AND id='OpenRouter OAuth'",
        [],
        |row| row.get(0),
    )?;
    assert!(String::from_utf8(original)?.contains("existing-plaintext-oauth"));
    let master_encrypted: bool = database.query_row(
        "SELECT encrypted FROM tcui_credentials WHERE kind='app_data_key' AND id='master'",
        [],
        |row| row.get(0),
    )?;
    assert!(master_encrypted);
    assert!(!std::fs::read(environment.key_path())?
        .windows(b"first-encrypted-oauth-canary".len())
        .any(|bytes| bytes == b"first-encrypted-oauth-canary"));
    let mut child = environment
        .command()
        .args(["auth", "--vault-password-stdin", "status", "openrouter"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or("stdin missing")?
        .write_all(b"first standalone password\n")?;
    let status = child.wait_with_output()?;
    assert!(
        status.status.success(),
        "{}",
        String::from_utf8_lossy(&status.stderr)
    );
    let mut child = environment
        .command()
        .args(["auth", "--vault-password-stdin", "status", "openrouter"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or("stdin missing")?
        .write_all(b"wrong standalone password\n")?;
    let denied = child.wait_with_output()?;
    assert_eq!(denied.status.code(), Some(14));
    Ok(())
}

#[test]
fn auth_cli_rejects_double_password_transports_before_reading_or_creating_stores(
) -> Result<(), Box<dyn std::error::Error>> {
    for global in [
        vec!["--unlock-password-stdin"],
        vec!["--unlock-password-fd", "3"],
    ] {
        let environment = CliEnvironment::new("conflicting-password-transports")?;
        let output = environment
            .command()
            .args(global)
            .args([
                "auth",
                "--vault-password-stdin",
                "login",
                "openrouter",
                "--headless",
            ])
            .stdin(Stdio::null())
            .output()?;
        assert!(!output.status.success());
        assert!(!environment.key_path().exists());
    }
    Ok(())
}

#[test]
fn auth_cli_pending_encrypted_login_cannot_overwrite_concurrent_plaintext_connection(
) -> Result<(), Box<dyn std::error::Error>> {
    let environment = CliEnvironment::new("pending-login-storage-collision")?;
    let (pending_endpoint, release) =
        JsonEndpoint::respond_once_gated(r#"{"key":"pending-encrypted-login-canary"}"#)?;
    let mut command = environment.command();
    support::configure_openrouter_url(&mut command, &pending_endpoint.url, "10000");
    let mut pending = command
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
    pending
        .stdin
        .take()
        .ok_or("pending stdin missing")?
        .write_all(b"concurrent-login-password\npending-code\n")?;
    pending_endpoint.wait_for_request()?;

    let database = rusqlite::Connection::open(environment.key_path())?;
    let initially_absent: bool = database.query_row(
        "SELECT NOT EXISTS(SELECT 1 FROM tcui_credentials WHERE id='OpenRouter OAuth')",
        [],
        |row| row.get(0),
    )?;
    assert!(initially_absent);
    drop(database);
    let sibling_endpoint =
        JsonEndpoint::respond_once(r#"{"key":"concurrent-plaintext-original"}"#)?;
    let mut command = environment.command();
    configure_openrouter(&mut command, &sibling_endpoint);
    let mut sibling = command
        .args(["auth", "--plaintext", "login", "openrouter", "--headless"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    sibling
        .stdin
        .take()
        .ok_or("sibling stdin missing")?
        .write_all(b"sibling-code\n")?;
    let sibling = sibling.wait_with_output()?;
    assert!(
        sibling.status.success(),
        "{}",
        String::from_utf8_lossy(&sibling.stderr)
    );
    sibling_endpoint.finish()?;
    release.send(())?;
    let rejected = pending.wait_with_output()?;
    assert_eq!(rejected.status.code(), Some(14));
    pending_endpoint.finish()?;

    let database = rusqlite::Connection::open(environment.key_path())?;
    let (encrypted, original): (bool, Vec<u8>) = database.query_row(
        "SELECT encrypted,payload FROM tcui_credentials WHERE kind='api_key_owned' AND id='OpenRouter OAuth'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    assert!(!encrypted);
    assert!(String::from_utf8(original)?.contains("concurrent-plaintext-original"));
    let records: i64 = database.query_row(
        "SELECT COUNT(*) FROM tcui_credentials WHERE kind='api_key_owned'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(records, 1);
    assert!(!std::fs::read(environment.key_path())?
        .windows(b"pending-encrypted-login-canary".len())
        .any(|bytes| bytes == b"pending-encrypted-login-canary"));
    Ok(())
}
