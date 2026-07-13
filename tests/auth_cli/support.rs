use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

pub(super) struct CliEnvironment {
    root: PathBuf,
}

impl CliEnvironment {
    pub(super) fn new(label: &str) -> std::io::Result<Self> {
        let root = std::env::temp_dir().join(format!(
            "tcui-auth-cli-{label}-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(root.join("bin"))?;
        std::fs::create_dir_all(root.join(".codex"))?;
        Ok(Self { root })
    }

    pub(super) fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_tcui"));
        command
            .env_clear()
            .env("HOME", &self.root)
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .env("XDG_DATA_HOME", self.root.join("data"))
            .env("PATH", self.root.join("bin"))
            .env(
                "TCUI_AUTH_TEST_SIGNAL_READY",
                self.root.join(".signal-ready"),
            );
        command
    }

    pub(super) fn install_codex(&self, body: &str) -> std::io::Result<()> {
        let executable = self.root.join("bin/codex");
        std::fs::write(&executable, format!("#!/bin/sh\n{body}\n"))?;
        let mut permissions = std::fs::metadata(&executable)?.permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(executable, permissions)
    }

    pub(super) fn auth_path(&self) -> PathBuf {
        self.root.join(".codex/auth.json")
    }

    pub(super) fn key_path(&self) -> PathBuf {
        self.root.join("data/tcui/keys.toml")
    }

    pub(super) fn marker_path(&self) -> PathBuf {
        self.root.join("codex-started")
    }

    pub(super) fn root_path(&self) -> PathBuf {
        self.root.clone()
    }

    pub(super) fn signal_ready_path(&self) -> PathBuf {
        self.root.join(".signal-ready")
    }
}

impl Drop for CliEnvironment {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

pub(super) struct JsonEndpoint {
    pub(super) url: String,
    request_seen: mpsc::Receiver<()>,
    worker: Option<thread::JoinHandle<std::io::Result<()>>>,
    shutdown: Arc<AtomicBool>,
}

impl JsonEndpoint {
    pub(super) fn respond_once(body: &'static str) -> std::io::Result<Self> {
        Self::respond_once_after("200 OK", body, Duration::ZERO)
    }

    pub(super) fn respond_once_after(
        status: &'static str,
        body: &'static str,
        delay: Duration,
    ) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let url = format!("http://{}", listener.local_addr()?);
        let (request_sender, request_seen) = mpsc::channel();
        let shutdown = Arc::new(AtomicBool::new(false));
        let worker_shutdown = shutdown.clone();
        let worker = thread::spawn(move || {
            let (mut stream, _) = accept_until(&listener, &worker_shutdown)?;
            let _ = request_sender.send(());
            let mut request = [0_u8; 4_096];
            let _ = stream.read(&mut request)?;
            thread::sleep(delay);
            write_response(&mut stream, status, body)
        });
        Ok(Self {
            url,
            request_seen,
            worker: Some(worker),
            shutdown,
        })
    }

    /// Responds to the first connection immediately, then accepts a second
    /// connection and holds it open without responding.
    ///
    /// This is designed for the native device flow: the first request
    /// (device/usercode) gets an immediate JSON response, while the second
    /// request (device/token polling) stays pending — keeping the child
    /// process blocked in its polling loop so SIGINT can be tested against
    /// a genuinely pending state.
    pub(super) fn respond_then_hold(first_body: &'static str) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let url = format!("http://{}", listener.local_addr()?);
        let (request_sender, request_seen) = mpsc::channel();
        let shutdown = Arc::new(AtomicBool::new(false));
        let worker_shutdown = shutdown.clone();
        let worker = thread::spawn(move || {
            let (mut first, _) = accept_until(&listener, &worker_shutdown)?;
            let _ = request_sender.send(());
            let mut request = [0_u8; 4_096];
            let _ = first.read(&mut request)?;
            write_response(&mut first, "200 OK", first_body)?;
            drop(first);

            let (_held, _) = accept_until(&listener, &worker_shutdown)?;
            let _ = request_sender.send(());
            hold_until_shutdown(&worker_shutdown);
            Ok(())
        });
        Ok(Self {
            url,
            request_seen,
            worker: Some(worker),
            shutdown,
        })
    }

    pub(super) fn finish(mut self) -> std::io::Result<()> {
        self.shutdown.store(true, Ordering::Relaxed);
        let worker = self.worker.take().expect("worker already consumed");
        match worker.join() {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) if e.kind() == std::io::ErrorKind::Interrupted => Ok(()),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(std::io::Error::other("fixture panicked")),
        }
    }

    pub(super) fn wait_for_request(&self) -> std::io::Result<()> {
        self.request_seen
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::TimedOut, "fixture was not called")
            })
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

fn accept_until(
    listener: &TcpListener,
    shutdown: &AtomicBool,
) -> std::io::Result<(std::net::TcpStream, std::net::SocketAddr)> {
    listener.set_nonblocking(true)?;
    loop {
        if shutdown.load(Ordering::Relaxed) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "fixture shut down before connection",
            ));
        }
        match listener.accept() {
            Ok(connection) => {
                listener.set_nonblocking(false)?;
                return Ok(connection);
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(e),
        }
    }
}

fn write_response(
    stream: &mut std::net::TcpStream,
    status: &str,
    body: &str,
) -> std::io::Result<()> {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes())
}

fn hold_until_shutdown(shutdown: &AtomicBool) {
    while !shutdown.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(50));
    }
}

pub(super) fn configure_openrouter(command: &mut Command, endpoint: &JsonEndpoint) {
    configure_openrouter_url(command, &endpoint.url, "100");
}

pub(super) fn configure_openrouter_url(command: &mut Command, url: &str, timeout_ms: &str) {
    command
        .env(
            "TCUI_AUTH_TEST_OPENROUTER_AUTHORIZATION",
            format!("{url}/auth"),
        )
        .env(
            "TCUI_AUTH_TEST_OPENROUTER_CODE_CREATION",
            format!("{url}/auth/keys/code"),
        )
        .env(
            "TCUI_AUTH_TEST_OPENROUTER_EXCHANGE",
            format!("{url}/auth/keys"),
        )
        .env("TCUI_AUTH_TEST_OPENROUTER_TIMEOUT_MS", timeout_ms);
}

pub(super) fn configure_codex_native(command: &mut Command, endpoint: &JsonEndpoint) {
    command
        .env(
            "TCUI_AUTH_TEST_CODEX_AUTHORIZATION",
            "https://authorization.example.test/authorize",
        )
        .env(
            "TCUI_AUTH_TEST_CODEX_TOKEN",
            format!("{}/token", endpoint.url),
        )
        .env(
            "TCUI_AUTH_TEST_CODEX_DEVICE_USER_CODE",
            format!("{}/device/usercode", endpoint.url),
        )
        .env(
            "TCUI_AUTH_TEST_CODEX_DEVICE_TOKEN",
            format!("{}/device/token", endpoint.url),
        );
}
