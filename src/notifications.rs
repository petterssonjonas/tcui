pub(crate) mod pushover;

use std::{
    io::{self, IsTerminal, Write},
    path::PathBuf,
    process::Stdio,
    time::Duration,
};

use base64::{engine::general_purpose::STANDARD, Engine};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
    sync::{Mutex, OnceCell},
};

use crate::config::AppConfig;

const FINISHED_TITLE: &str = "TermChatUI response ready";
const REMINDER_TITLE: &str = "TermChatUI reminder";
const MAX_BODY_CHARS: usize = 280;
const HELPER_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_DIAGNOSTIC_BYTES: usize = 4096;
static DESKTOP_ROUTE: OnceCell<Mutex<DesktopRoute>> = OnceCell::const_new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TerminalProtocol {
    Kitty,
    Osc9,
}

#[derive(Debug, PartialEq, Eq)]
enum DesktopRoute {
    Disabled,
    LocalDbus,
    Terminal(TerminalProtocol),
    Herdr(PathBuf),
}

struct NotificationEnvironment {
    remote: bool,
    terminal: Option<TerminalProtocol>,
    multiplexer: bool,
    output_terminal: bool,
    has_bus: bool,
    inside_herdr: bool,
    herdr_binary: Option<PathBuf>,
    herdr_socket: Option<PathBuf>,
    herdr_pane: bool,
}

impl NotificationEnvironment {
    fn read(get: impl Fn(&str) -> Option<String>, output_terminal: bool) -> Self {
        let term = get("TERM").unwrap_or_default();
        let program = get("TERM_PROGRAM").unwrap_or_default();
        let terminal = if term == "xterm-kitty" || program.eq_ignore_ascii_case("kitty") {
            Some(TerminalProtocol::Kitty)
        } else if term == "xterm-ghostty"
            || program.eq_ignore_ascii_case("ghostty")
            || program.eq_ignore_ascii_case("wezterm")
        {
            Some(TerminalProtocol::Osc9)
        } else {
            None
        };
        let nonempty = |key| get(key).is_some_and(|value| !value.is_empty());
        Self {
            remote: ["SSH_CONNECTION", "SSH_CLIENT", "SSH_TTY"]
                .iter()
                .any(|key| nonempty(key)),
            terminal,
            multiplexer: ["TMUX", "STY", "ZELLIJ", "ZELLIJ_SESSION_NAME"]
                .iter()
                .any(|key| get(key).is_some())
                || term.starts_with("screen")
                || term.starts_with("tmux"),
            output_terminal,
            has_bus: nonempty("DBUS_SESSION_BUS_ADDRESS") || nonempty("XDG_RUNTIME_DIR"),
            inside_herdr: get("HERDR_ENV").as_deref() == Some("1"),
            herdr_binary: get("HERDR_BIN_PATH")
                .filter(|s| !s.is_empty())
                .map(PathBuf::from),
            herdr_socket: get("HERDR_SOCKET_PATH")
                .filter(|s| !s.is_empty())
                .map(PathBuf::from),
            herdr_pane: nonempty("HERDR_PANE_ID"),
        }
    }

    fn route(&self, dbus_reachable: bool) -> DesktopRoute {
        // Herdr owns delivery, including forwarding to its attached remote clients.
        // Never bypass an incomplete Herdr context with an outer-terminal escape.
        if self.inside_herdr {
            return match (&self.herdr_binary, &self.herdr_socket) {
                (Some(binary), Some(socket))
                    if binary.is_absolute() && socket.is_absolute() && self.herdr_pane =>
                {
                    DesktopRoute::Herdr(binary.clone())
                }
                _ => DesktopRoute::Disabled,
            };
        }
        // An SSH server's desktop is not the user's laptop, even with a live bus.
        if !self.remote && dbus_reachable {
            return DesktopRoute::LocalDbus;
        }
        if self.output_terminal && !self.multiplexer {
            if let Some(protocol) = self.terminal {
                return DesktopRoute::Terminal(protocol);
            }
        }
        DesktopRoute::Disabled
    }
}

pub(crate) async fn notify_finished(
    config: &AppConfig,
    prompt: &str,
    answer: &str,
) -> color_eyre::Result<()> {
    let body = notification_body(prompt, answer);
    if body.is_empty() {
        return Ok(());
    }
    notify_text(config, FINISHED_TITLE, &body, false).await
}

pub(crate) async fn notify_reminder(config: &AppConfig, message: &str) -> color_eyre::Result<()> {
    let body = clean_text(message, MAX_BODY_CHARS);
    if body.is_empty() {
        return Ok(());
    }
    notify_text(config, REMINDER_TITLE, &body, true).await
}

/// Delivers a scheduled reminder without reopening its credential vault.
pub(crate) async fn notify_reminder_with_delivery(
    config: &AppConfig,
    message: &str,
    delivery: &pushover::Delivery,
) -> color_eyre::Result<()> {
    let body = clean_text(message, MAX_BODY_CHARS);
    let mut failure = if body.is_empty() {
        None
    } else {
        notify_desktop_and_ntfy(config, REMINDER_TITLE, &body, true)
            .await
            .err()
    };
    if let Err(error) = delivery.send(config, "reminder", message).await {
        failure.get_or_insert(error);
    }
    match failure {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

fn notification_body(prompt: &str, answer: &str) -> String {
    let summary = answer
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or(prompt);
    clean_text(summary, MAX_BODY_CHARS)
}

fn clean_text(text: &str, max_chars: usize) -> String {
    let mut chars = text.trim().chars().filter_map(|ch| {
        if matches!(ch, '\r' | '\n' | '\t') {
            Some(' ')
        } else if ch.is_control() {
            None
        } else {
            Some(ch)
        }
    });
    let mut clean: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        clean.push_str("...");
    }
    clean
}

fn ntfy_url(config: &AppConfig) -> Option<String> {
    let ntfy = &config.notifications.ntfy;
    if !ntfy.enabled {
        return None;
    }
    let host = ntfy.host.trim();
    if host.is_empty() {
        return None;
    }
    let scheme_host = if host.starts_with("http://") || host.starts_with("https://") {
        host.to_string()
    } else {
        format!("http://{host}")
    };
    let base = scheme_host.trim_end_matches('/');
    let topic = ntfy.topic.trim().trim_matches('/');
    let topic = if topic.is_empty() { "tcui" } else { topic };
    Some(format!("{base}:{ntfy_port}/{topic}", ntfy_port = ntfy.port))
}

async fn notify_text(
    config: &AppConfig,
    title: &str,
    body: &str,
    audible_bell: bool,
) -> color_eyre::Result<()> {
    let mut failure = notify_desktop_and_ntfy(config, title, body, audible_bell)
        .await
        .err();
    let delivery = async {
        if let Some(delivery) = pushover::prepare(config)? {
            delivery.send(config, title, body).await?;
        }
        color_eyre::Result::<()>::Ok(())
    }
    .await;
    if let Err(error) = delivery {
        failure.get_or_insert(error);
    }
    match failure {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

async fn notify_desktop_and_ntfy(
    config: &AppConfig,
    title: &str,
    body: &str,
    audible_bell: bool,
) -> color_eyre::Result<()> {
    let mut failure = None;
    if config.notifications.desktop {
        if let Err(error) = send_desktop(title, body, audible_bell).await {
            failure = Some(color_eyre::Report::from(error));
        }
    }
    // Explicit ntfy delivery stays independent of local desktop availability.
    if let Some(url) = ntfy_url(config) {
        if let Err(error) = send_ntfy(&url, title, body).await {
            failure.get_or_insert_with(|| color_eyre::Report::from(error));
        }
    }
    match failure {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

async fn detect_desktop_route() -> Mutex<DesktopRoute> {
    let environment =
        NotificationEnvironment::read(|key| std::env::var(key).ok(), io::stdout().is_terminal());
    let dbus_reachable = !environment.remote
        && !environment.inside_herdr
        && environment.has_bus
        && run_helper(&mut dbus_probe(), HELPER_TIMEOUT).await.is_ok();
    let mut route = environment.route(dbus_reachable);
    if matches!(route, DesktopRoute::Herdr(_)) && !herdr_socket_exists(&environment) {
        route = DesktopRoute::Disabled;
    }
    Mutex::new(route)
}

fn herdr_socket_exists(environment: &NotificationEnvironment) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        environment.herdr_socket.as_ref().is_some_and(|socket| {
            std::fs::metadata(socket).is_ok_and(|metadata| metadata.file_type().is_socket())
        }) && environment
            .herdr_binary
            .as_ref()
            .is_some_and(|binary| binary.is_file())
    }
    #[cfg(not(unix))]
    {
        let _ = environment;
        false
    }
}

fn dbus_probe() -> Command {
    let mut command = Command::new("busctl");
    // No service activation, bus forwarding, or guessed session addresses.
    command.args([
        "--user",
        "--auto-start=no",
        "--timeout=2",
        "call",
        "org.freedesktop.Notifications",
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
        "GetServerInformation",
    ]);
    command
}

async fn send_desktop(title: &str, body: &str, audible_bell: bool) -> io::Result<()> {
    let state = DESKTOP_ROUTE.get_or_init(detect_desktop_route).await;
    let mut route = state.lock().await;
    let result = deliver_desktop(&route, title, body, audible_bell).await;
    // Do not repeatedly spawn a broken backend or pollute a running TUI.
    if result
        .as_ref()
        .is_err_and(|error| error.kind() != io::ErrorKind::WouldBlock)
    {
        *route = DesktopRoute::Disabled;
    }
    result
}

async fn deliver_desktop(
    route: &DesktopRoute,
    title: &str,
    body: &str,
    audible_bell: bool,
) -> io::Result<()> {
    match route {
        DesktopRoute::Disabled => Ok(()),
        DesktopRoute::LocalDbus => {
            let mut command = Command::new("notify-send");
            // libnotify treats bodies as markup, unlike terminal notification protocols.
            let body = body
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            command.args(["--app-name=TermChatUI", "--", title, &body]);
            run_helper(&mut command, HELPER_TIMEOUT).await?;
            if audible_bell {
                send_local_sound().await;
            }
            Ok(())
        }
        DesktopRoute::Herdr(binary) => {
            let mut command = Command::new(binary);
            command.args([
                "notification",
                "show",
                "--body",
                body,
                "--sound",
                if audible_bell { "request" } else { "none" },
                "--",
                title,
            ]);
            let output = run_helper(&mut command, HELPER_TIMEOUT).await?;
            herdr_delivery_result(&output)
        }
        DesktopRoute::Terminal(protocol) => {
            let sequence = terminal_sequence(*protocol, title, body);
            // Stdout's shared lock also serializes ratatui's writes. Never use a
            // separately opened /dev/tty, which could interleave escape sequences.
            let stdout = io::stdout();
            let mut output = stdout.lock();
            output.write_all(sequence.as_bytes())?;
            if audible_bell {
                output.write_all(b"\x07")?;
            }
            output.flush()
        }
    }
}

fn herdr_delivery_result(output: &[u8]) -> io::Result<()> {
    let response: serde_json::Value = serde_json::from_slice(output).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid Herdr notification response",
        )
    })?;
    let result = &response["result"];
    if result["type"] != "notification_show" || !result["shown"].is_boolean() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "missing Herdr notification acknowledgement",
        ));
    }
    if result["shown"] == true {
        Ok(())
    } else {
        Err(io::Error::new(
            if result["reason"] == "disabled" {
                io::ErrorKind::NotConnected
            } else {
                io::ErrorKind::WouldBlock
            },
            format!(
                "Herdr notification not shown: {}",
                clean_text(
                    result["reason"].as_str().unwrap_or("unknown"),
                    MAX_BODY_CHARS
                ),
            ),
        ))
    }
}

fn terminal_sequence(protocol: TerminalProtocol, title: &str, body: &str) -> String {
    let title = clean_text(title, MAX_BODY_CHARS);
    let body = clean_text(body, MAX_BODY_CHARS);
    match protocol {
        // Encoding isolates untrusted answer text from OSC metadata/terminators.
        // No activation reports: these would otherwise enter the TUI input stream.
        TerminalProtocol::Kitty => format!(
            "\x1b]99;i=tcui:d=0:e=1;{}\x1b\\\x1b]99;i=tcui:p=body:e=1;{}\x1b\\",
            STANDARD.encode(title),
            STANDARD.encode(body),
        ),
        TerminalProtocol::Osc9 => format!("\x1b]9;{title}: {body}\x1b\\"),
    }
}

async fn send_ntfy(url: &str, title: &str, body: &str) -> reqwest::Result<()> {
    reqwest::Client::new()
        .post(url)
        .timeout(Duration::from_secs(5))
        .header("Title", title)
        .body(body.to_string())
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

async fn send_local_sound() {
    for (program, args) in [
        ("canberra-gtk-play", &["-i", "bell"][..]),
        (
            "paplay",
            &["/usr/share/sounds/freedesktop/stereo/bell.oga"][..],
        ),
    ] {
        let mut command = Command::new(program);
        command.args(args);
        if run_helper(&mut command, HELPER_TIMEOUT).await.is_ok() {
            break;
        }
    }
}

async fn read_diagnostics(mut stream: impl AsyncRead + Unpin) -> io::Result<Vec<u8>> {
    let mut captured = Vec::new();
    let mut chunk = [0; 1024];
    loop {
        let count = stream.read(&mut chunk).await?;
        if count == 0 {
            return Ok(captured);
        }
        let keep = count.min(MAX_DIAGNOSTIC_BYTES.saturating_sub(captured.len()));
        captured.extend_from_slice(&chunk[..keep]);
    }
}

async fn run_helper(command: &mut Command, timeout: Duration) -> io::Result<Vec<u8>> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("missing helper stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("missing helper stderr"))?;
    let work = async {
        let (status, stdout, stderr) = tokio::try_join!(
            child.wait(),
            read_diagnostics(stdout),
            read_diagnostics(stderr),
        )?;
        if status.success() {
            Ok(stdout)
        } else {
            Err(io::Error::other(format!(
                "notification helper exited {status}: {}",
                clean_text(&String::from_utf8_lossy(&stderr), MAX_DIAGNOSTIC_BYTES),
            )))
        }
    };
    match tokio::time::timeout(timeout, work).await {
        Ok(result) => result,
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "notification helper timed out",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    mod captured_reminders {
        use super::*;
        use crate::config::key_store::{KeyStore, SecretKind};
        use std::{ffi::OsString, os::unix::fs::PermissionsExt};

        struct Environment {
            root: PathBuf,
            previous: [(&'static str, Option<OsString>); 4],
        }

        impl Environment {
            fn new() -> Self {
                let root = std::env::temp_dir().join(format!(
                    "tcui-captured-reminder-{}-{:032x}",
                    std::process::id(),
                    rand::random::<u128>()
                ));
                std::fs::create_dir(&root).expect("fixture directory");
                std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))
                    .expect("private fixture");
                let previous = ["HOME", "XDG_DATA_HOME", "XDG_CONFIG_HOME", "XDG_CACHE_HOME"]
                    .map(|name| (name, std::env::var_os(name)));
                for (name, _) in &previous {
                    let path = root.join(*name);
                    std::fs::create_dir(&path).expect("isolated environment directory");
                    std::env::set_var(name, path);
                }
                Self { root, previous }
            }
        }

        impl Drop for Environment {
            fn drop(&mut self) {
                let _ = crate::protection::lock();
                for (name, value) in &self.previous {
                    match value {
                        Some(value) => std::env::set_var(name, value),
                        None => std::env::remove_var(name),
                    }
                }
                let _ = std::fs::remove_dir_all(&self.root);
            }
        }

        fn backend_case(failed_backend: Option<&str>) {
            let _guard = crate::test_support::env_lock()
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let environment = Environment::new();
            let mut config = AppConfig::default();
            let program = environment.root.join("pushover-helper");
            let desktop = environment.root.join("desktop-helper");
            let user_key = "u".repeat(30);
            let app_key = "a".repeat(30);
            std::fs::write(&program, format!(
                "#!/bin/sh\n[ \"$PUSHOVER_USER_KEY\" = '{user_key}' ] || exit 2\n[ \"$PUSHOVER_TCUI_APP_TOKEN\" = '{app_key}' ] || exit 3\n[ \"${{PUSHOVER_ENV_FILE+x}}\" = x ] && [ -z \"$PUSHOVER_ENV_FILE\" ] || exit 4\n[ \"$#\" = 6 ] && [ \"$1\" = --app ] && [ \"$2\" = tcui ] && [ \"$3\" = --title ] && [ \"$4\" = reminder ] && [ \"$5\" = -- ] || exit 5\nprintf '%s' \"$6\" > \"$0.sentinel\"\nexit {}\n",
                if failed_backend == Some("pushover") { 1 } else { 0 }
            )).expect("Pushover fixture helper");
            std::fs::write(&desktop, format!(
                "#!/bin/sh\nprintf 'desktop attempted' > \"$0.sentinel\"\nprintf '%s' '{{\"result\":{{\"type\":\"notification_show\",\"shown\":{},\"reason\":\"disabled\"}}}}'\n",
                failed_backend != Some("desktop")
            )).expect("desktop fixture helper");
            for path in [&program, &desktop] {
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
                    .expect("executable helper");
            }
            config.notifications.desktop = true;
            config.notifications.ntfy.enabled = true;
            config.notifications.ntfy.host = "127.0.0.1".into();
            config.notifications.pushover.enabled = true;
            config.notifications.pushover.use_vault = true;
            config.notifications.pushover.command = program.to_string_lossy().into_owned();
            config.commands.whitelist = vec![config.notifications.pushover.command.clone()];
            KeyStore::initialize_vault(&config, "isolated reminder vault password")
                .expect("fixture vault");
            KeyStore::set_secret(
                &config,
                pushover::USER_KEY_ID,
                "Pushover user",
                SecretKind::Password,
                &user_key,
            )
            .expect("fixture user key");
            KeyStore::set_secret(
                &config,
                pushover::APP_KEY_ID,
                "Pushover app",
                SecretKind::Password,
                &app_key,
            )
            .expect("fixture app key");
            let delivery = pushover::prepare(&config)
                .expect("prepare snapshot")
                .expect("enabled");
            KeyStore::lock_vault(&config).expect("cold lock");
            assert!(KeyStore::get_secret(&config, pushover::USER_KEY_ID).is_err());
            // Pushover must retain the full message; existing local backends retain their excerpt.
            let message = format!(
                "{}\ncomplete scheduled reminder",
                "m".repeat(MAX_BODY_CHARS + 20)
            );
            let local_body = clean_text(&message, MAX_BODY_CHARS);
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime");
            runtime.block_on(async {
                use tokio::io::AsyncWriteExt;
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("local ntfy fixture");
                config.notifications.ntfy.port = listener.local_addr().expect("fixture address").port();
                let expected_body = local_body.clone();
                let status = if failed_backend == Some("ntfy") { "500 Failed" } else { "200 OK" };
                let server = tokio::spawn(async move {
                    let (mut stream, _) = listener.accept().await.expect("ntfy connection");
                    let mut request = Vec::new();
                    let mut chunk = [0; 1024];
                    loop {
                        let count = stream.read(&mut chunk).await.expect("fixture request");
                        assert_ne!(count, 0, "request ended before the reminder body");
                        request.extend_from_slice(&chunk[..count]);
                        assert!(request.len() <= 16_384, "unexpected fixture request size");
                        if request.ends_with(expected_body.as_bytes()) {
                            break;
                        }
                    }
                    stream.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await.expect("fixture response");
                    request
                });
                // Install a local desktop fixture, never probe a real terminal, bus or Herdr session.
                let route = DESKTOP_ROUTE.get_or_init(|| async { Mutex::new(DesktopRoute::Disabled) }).await;
                let previous = std::mem::replace(&mut *route.lock().await, DesktopRoute::Herdr(desktop.clone()));
                let result = notify_reminder_with_delivery(&config, &message, &delivery).await;
                let request = tokio::time::timeout(Duration::from_secs(2), server).await;
                *route.lock().await = previous;
                assert_eq!(result.is_err(), failed_backend.is_some());
                let request = request.expect("ntfy must be attempted").expect("ntfy task");
                assert!(request.starts_with(b"POST /tcui HTTP/1.1\r\n"));
                assert!(request.ends_with(local_body.as_bytes()));
                assert_eq!(std::fs::read(program.with_extension("sentinel")).expect("captured Pushover attempted"), message.as_bytes());
                assert_eq!(std::fs::read(desktop.with_extension("sentinel")).expect("desktop attempted"), b"desktop attempted");
                assert!(KeyStore::get_secret(&config, pushover::APP_KEY_ID).is_err(), "dispatch reopened the vault");
            });
        }

        #[test]
        fn cold_locked_reminder_delivers_all_enabled_backends() {
            backend_case(None);
        }

        #[test]
        fn desktop_failure_does_not_skip_ntfy_or_captured_pushover() {
            backend_case(Some("desktop"));
        }

        #[test]
        fn ntfy_failure_does_not_skip_desktop_or_captured_pushover() {
            backend_case(Some("ntfy"));
        }

        #[test]
        fn pushover_failure_does_not_skip_desktop_or_ntfy() {
            backend_case(Some("pushover"));
        }
    }

    fn environment(values: &[(&str, &str)], output_terminal: bool) -> NotificationEnvironment {
        NotificationEnvironment::read(
            |key| {
                values
                    .iter()
                    .find(|(name, _)| *name == key)
                    .map(|(_, value)| value.to_string())
            },
            output_terminal,
        )
    }

    #[test]
    fn local_session_prefers_reachable_dbus() {
        assert_eq!(
            environment(&[("TERM", "xterm-kitty")], true).route(true),
            DesktopRoute::LocalDbus
        );
    }

    #[test]
    fn ssh_never_delivers_to_server_dbus() {
        assert_eq!(
            environment(
                &[("SSH_CONNECTION", "fixture"), ("TERM", "xterm-256color")],
                true
            )
            .route(true),
            DesktopRoute::Disabled,
        );
    }

    #[test]
    fn ssh_known_terminals_route_to_client_protocol() {
        for (key, value, protocol) in [
            ("TERM", "xterm-kitty", TerminalProtocol::Kitty),
            ("TERM", "xterm-ghostty", TerminalProtocol::Osc9),
            ("TERM_PROGRAM", "ghostty", TerminalProtocol::Osc9),
            ("TERM_PROGRAM", "WezTerm", TerminalProtocol::Osc9),
        ] {
            assert_eq!(
                environment(&[("SSH_TTY", "/dev/pts/1"), (key, value)], true).route(true),
                DesktopRoute::Terminal(protocol),
            );
        }
    }

    #[test]
    fn missing_bus_and_unknown_terminal_disable_desktop() {
        assert_eq!(environment(&[], true).route(false), DesktopRoute::Disabled);
    }

    #[test]
    fn terminal_protocol_is_never_written_to_redirected_output() {
        assert_eq!(
            environment(&[("TERM", "xterm-kitty")], false).route(false),
            DesktopRoute::Disabled,
        );
    }

    #[test]
    fn unconfirmed_multiplexer_passthrough_is_disabled() {
        for key in ["TMUX", "STY", "ZELLIJ", "ZELLIJ_SESSION_NAME"] {
            assert_eq!(
                environment(&[("TERM", "xterm-kitty"), (key, "fixture")], true).route(false),
                DesktopRoute::Disabled,
            );
        }
    }

    #[test]
    fn herdr_requires_its_documented_complete_context() {
        let complete = [
            ("HERDR_ENV", "1"),
            ("HERDR_PANE_ID", "w1:p1"),
            ("HERDR_BIN_PATH", "/fixture/herdr"),
            ("HERDR_SOCKET_PATH", "/fixture/herdr.sock"),
            ("SSH_CONNECTION", "fixture"),
        ];
        assert_eq!(
            environment(&complete, true).route(true),
            DesktopRoute::Herdr("/fixture/herdr".into())
        );
        for omitted in 0..4 {
            let incomplete: Vec<_> = complete
                .iter()
                .enumerate()
                .filter_map(|(index, pair)| (index != omitted).then_some(*pair))
                .collect();
            assert_eq!(
                environment(&incomplete, true).route(false),
                DesktopRoute::Disabled
            );
        }
    }

    #[test]
    fn unrelated_herdr_configuration_does_not_claim_integration() {
        assert_eq!(
            environment(&[("HERDR_CONFIG_PATH", "/fixture/config.toml")], true).route(false),
            DesktopRoute::Disabled,
        );
    }

    #[test]
    fn osc9_payload_cannot_inject_terminal_controls() {
        let sequence = terminal_sequence(
            TerminalProtocol::Osc9,
            "Title",
            "hello\x07\x1b\\\u{9c}\nworld",
        );
        assert_eq!(sequence, "\x1b]9;Title: hello\\ world\x1b\\");
    }

    #[test]
    fn kitty_payload_is_base64_isolated_from_metadata() {
        let sequence = terminal_sequence(
            TerminalProtocol::Kitty,
            "Title",
            "evil;d=1\x1b]52;secret\x07",
        );
        assert_eq!(
            sequence,
            format!(
                "\x1b]99;i=tcui:d=0:e=1;VGl0bGU=\x1b\\\x1b]99;i=tcui:p=body:e=1;{}\x1b\\",
                STANDARD.encode("evil;d=1]52;secret")
            ),
        );
    }

    #[test]
    fn herdr_acknowledgement_must_confirm_shown() {
        assert!(herdr_delivery_result(
            br#"{"id":"fixture","result":{"type":"notification_show","shown":true,"reason":"shown"}}"#,
        ).is_ok());
    }

    #[test]
    fn herdr_suppression_is_not_reported_as_delivery() {
        let error = herdr_delivery_result(
            br#"{"id":"fixture","result":{"type":"notification_show","shown":false,"reason":"disabled"}}"#,
        ).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotConnected);
    }

    #[test]
    fn herdr_temporary_suppression_does_not_disable_future_delivery() {
        let error = herdr_delivery_result(
            br#"{"id":"fixture","result":{"type":"notification_show","shown":false,"reason":"no_foreground_client"}}"#,
        ).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
    }

    #[test]
    fn empty_inherited_origin_does_not_claim_ssh_or_herdr() {
        assert_eq!(
            environment(
                &[
                    ("SSH_CONNECTION", ""),
                    ("SSH_CLIENT", ""),
                    ("SSH_TTY", ""),
                    ("HERDR_ENV", "")
                ],
                false
            )
            .route(true),
            DesktopRoute::LocalDbus,
        );
    }

    #[tokio::test]
    async fn explicit_ntfy_still_delivers_without_desktop_notifications() {
        use tokio::io::AsyncWriteExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 1024];
            loop {
                let count = stream.read(&mut chunk).await.unwrap();
                assert_ne!(count, 0, "notification request ended before its body");
                request.extend_from_slice(&chunk[..count]);
                if request.ends_with(b"fixture reminder") {
                    break;
                }
            }
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
            request
        });
        let mut config = AppConfig::default();
        config.notifications.desktop = false;
        config.notifications.ntfy.enabled = true;
        config.notifications.ntfy.host = "127.0.0.1".into();
        config.notifications.ntfy.port = port;
        notify_reminder(&config, "fixture reminder").await.unwrap();
        let request = server.await.unwrap();
        assert!(String::from_utf8_lossy(&request).starts_with("POST /tcui HTTP/1.1\r\n"));
    }

    #[test]
    fn ntfy_url_uses_default_scheme_and_topic() {
        let mut config = AppConfig::default();
        config.notifications.ntfy.enabled = true;
        config.notifications.ntfy.host = "127.0.0.1".to_string();
        config.notifications.ntfy.port = 8080;
        config.notifications.ntfy.topic.clear();
        assert_eq!(
            ntfy_url(&config).as_deref(),
            Some("http://127.0.0.1:8080/tcui")
        );
    }

    #[test]
    fn notification_body_prefers_answer_excerpt() {
        assert_eq!(
            notification_body("original prompt", "\n\nFinished answer\nextra"),
            "Finished answer"
        );
    }

    #[test]
    fn body_truncation_preserves_unicode_and_strips_controls() {
        assert_eq!(clean_text(" \x1b\u{9c}åäö👍\x07 ", 3), "åäö...");
    }

    #[test]
    fn local_bus_probe_cannot_activate_or_forward_a_service() {
        let command = dbus_probe();
        let args: Vec<_> = command
            .as_std()
            .get_args()
            .map(|arg| arg.to_string_lossy())
            .collect();
        assert!(args.contains(&"--user".into()) && args.contains(&"--auto-start=no".into()));
    }

    #[tokio::test]
    async fn disabled_notifications_and_reminders_need_no_helpers() {
        let mut config = AppConfig::default();
        config.notifications.desktop = false;
        notify_finished(&config, "prompt", "done").await.unwrap();
        notify_reminder(&config, "reminder").await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn helper_failure_captures_diagnostics_and_checks_exit_status() {
        let mut command = Command::new("/bin/sh");
        command.args([
            "-c",
            "printf 'captured stdout'; printf '\\033[31mprivate diagnostic' >&2; exit 7",
        ]);
        let error = run_helper(&mut command, HELPER_TIMEOUT).await.unwrap_err();
        assert!(error.to_string().contains("exit status: 7"));
        assert!(error.to_string().contains("private diagnostic"));
        assert!(!error.to_string().contains('\x1b'));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn hung_helper_is_killed_and_reaped() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "exec sleep 30"]);
        let error = run_helper(&mut command, Duration::from_millis(30))
            .await
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn helper_output_is_bounded_without_blocking_pipe_drain() {
        let mut command = Command::new("/bin/sh");
        command.args([
            "-c",
            "i=0; while [ \"$i\" -lt 6000 ]; do printf x; i=$((i+1)); done",
        ]);
        let output = run_helper(&mut command, HELPER_TIMEOUT).await.unwrap();
        assert_eq!(output.len(), MAX_DIAGNOSTIC_BYTES);
    }
}
