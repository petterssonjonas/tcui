#![cfg(unix)]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

#[test]
fn auth_cli_never_emits_oauth_token_canaries_after_completion(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    const ACCESS_TOKEN_CANARY: &str = "eyJ.tcui-secret-canary";
    const REFRESH_TOKEN_CANARY: &str = "rt.tcui-refresh-canary";
    const DEVICE_CODE_CANARY: &str = "dc.tcui-device-canary";
    let environment = CliEnvironment::new("secret-hygiene")?;
    let endpoint = JsonEndpoint::respond_once(
        r#"{"key":"eyJ.tcui-secret-canary","refresh_token":"rt.tcui-refresh-canary","device_code":"dc.tcui-device-canary"}"#,
    )?;
    let mut login = environment.command();
    configure_openrouter(&mut login, &endpoint);
    login
        .args(["auth", "login", "openrouter", "--headless"])
        .stdin(Stdio::piped());

    // When
    let mut child = login.spawn()?;
    let mut stdin = child.stdin.take().ok_or("headless stdin missing")?;
    stdin.write_all(b"headless-code\n")?;
    drop(stdin);
    let login = child.wait_with_output()?;
    endpoint.finish()?;
    let status = environment
        .command()
        .args(["auth", "status", "openrouter"])
        .output()?;
    let logout = environment
        .command()
        .args(["auth", "logout", "openrouter"])
        .output()?;

    // Then
    assert!(login.status.success());
    assert!(status.status.success());
    assert!(logout.status.success());
    let output = [
        login.stdout,
        login.stderr,
        status.stdout,
        status.stderr,
        logout.stdout,
        logout.stderr,
    ]
    .concat();
    let output = String::from_utf8(output)?;
    for canary in [
        ACCESS_TOKEN_CANARY,
        REFRESH_TOKEN_CANARY,
        DEVICE_CODE_CANARY,
    ] {
        assert!(
            !output.contains(canary),
            "CLI stdout/stderr leaked token canary {canary}"
        );
    }
    Ok(())
}

struct CliEnvironment {
    root: PathBuf,
}

impl CliEnvironment {
    fn new(label: &str) -> std::io::Result<Self> {
        let root = std::env::temp_dir().join(format!(
            "tcui-secret-hygiene-{label}-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_tcui"));
        command
            .env_clear()
            .env("HOME", &self.root)
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .env("XDG_DATA_HOME", self.root.join("data"));
        command
    }
}

impl Drop for CliEnvironment {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

struct JsonEndpoint {
    url: String,
    shutdown: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<std::io::Result<()>>>,
}

impl JsonEndpoint {
    fn respond_once(body: &'static str) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let url = format!("http://{}", listener.local_addr()?);
        let shutdown = Arc::new(AtomicBool::new(false));
        let worker_shutdown = Arc::clone(&shutdown);
        let worker = thread::spawn(move || {
            let mut stream = loop {
                if worker_shutdown.load(Ordering::Relaxed) {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Interrupted,
                        "fixture shut down before connection",
                    ));
                }
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1));
                    }
                    Err(error) => return Err(error),
                }
            };
            let mut request = [0_u8; 4_096];
            let _ = stream.read(&mut request)?;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes())
        });
        Ok(Self {
            url,
            shutdown,
            worker: Some(worker),
        })
    }

    fn finish(mut self) -> std::io::Result<()> {
        let worker = self
            .worker
            .take()
            .ok_or_else(|| std::io::Error::other("fixture worker already consumed"))?;
        match worker.join() {
            Ok(result) => result,
            Err(_) => Err(std::io::Error::other("fixture worker panicked")),
        }
    }
}

impl Drop for JsonEndpoint {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn configure_openrouter(command: &mut Command, endpoint: &JsonEndpoint) {
    command
        .env(
            "TCUI_AUTH_TEST_OPENROUTER_AUTHORIZATION",
            format!("{}/auth", endpoint.url),
        )
        .env(
            "TCUI_AUTH_TEST_OPENROUTER_CODE_CREATION",
            format!("{}/auth/keys/code", endpoint.url),
        )
        .env(
            "TCUI_AUTH_TEST_OPENROUTER_EXCHANGE",
            format!("{}/auth/keys", endpoint.url),
        )
        .env("TCUI_AUTH_TEST_OPENROUTER_TIMEOUT_MS", "100");
}
