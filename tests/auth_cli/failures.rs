use std::io::{BufRead, BufReader, Write};
use std::process::Stdio;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::process::ProcessGuard;
use super::support::{
    configure_codex_native, configure_openrouter, configure_openrouter_url, CliEnvironment,
    JsonEndpoint,
};

const STRESS_ITERATIONS: usize = 10;

#[test]
fn auth_cli_maps_openrouter_denial_timeout_and_network_failure_to_stable_exits(
) -> Result<(), Box<dyn std::error::Error>> {
    let denied_environment = CliEnvironment::new("openrouter-denied")?;
    let denied_endpoint = JsonEndpoint::respond_once_after(
        "403 Forbidden",
        r#"{"error":{"message":"denied-code-canary"}}"#,
        Duration::ZERO,
    )?;
    let mut denied = denied_environment.command();
    configure_openrouter(&mut denied, &denied_endpoint);
    denied
        .args(["auth", "login", "openrouter", "--headless"])
        .stdin(Stdio::piped());

    let timeout_environment = CliEnvironment::new("openrouter-timeout")?;
    let timeout_endpoint = JsonEndpoint::respond_once_after(
        "200 OK",
        r#"{"key":"late-key-canary"}"#,
        Duration::from_millis(50),
    )?;
    let mut timeout = timeout_environment.command();
    configure_openrouter_url(&mut timeout, &timeout_endpoint.url, "10");
    timeout
        .args(["auth", "login", "openrouter", "--headless"])
        .stdin(Stdio::piped());

    let no_network_environment = CliEnvironment::new("openrouter-network")?;
    let mut no_network = no_network_environment.command();
    configure_openrouter_url(&mut no_network, "http://127.0.0.1:9", "10");
    no_network
        .args(["auth", "login", "openrouter", "--headless"])
        .stdin(Stdio::piped());

    let denied_output = run_headless(denied)?;
    denied_endpoint.finish()?;
    let timeout_output = run_headless(timeout)?;
    timeout_endpoint.finish()?;
    let no_network_output = run_headless(no_network)?;

    assert_eq!(denied_output.status.code(), Some(11));
    assert_eq!(timeout_output.status.code(), Some(14));
    assert_eq!(no_network_output.status.code(), Some(14));
    assert!(!std::fs::read_to_string(timeout_environment.key_path())
        .unwrap_or_default()
        .contains("late-key-canary"));
    assert!(!std::fs::read_to_string(no_network_environment.key_path())
        .unwrap_or_default()
        .contains("headless-code"));
    Ok(())
}

#[test]
fn auth_cli_maps_native_browser_failure_without_persisting_a_credential(
) -> Result<(), Box<dyn std::error::Error>> {
    let browser_environment = CliEnvironment::new("browser-failure")?;

    let browser_failure = browser_environment
        .command()
        .args(["auth", "login", "codex", "--native"])
        .output()?;

    assert_eq!(browser_failure.status.code(), Some(14));
    assert!(String::from_utf8(browser_failure.stdout)?.contains("Codex native authorization URL:"));
    assert!(!std::fs::read_to_string(browser_environment.key_path())
        .unwrap_or_default()
        .contains("access_token"));
    Ok(())
}

#[test]
fn auth_cli_prints_native_device_details_before_cancellation(
) -> Result<(), Box<dyn std::error::Error>> {
    let device_environment = CliEnvironment::new("device-code")?;
    let device_endpoint = JsonEndpoint::respond_then_hold(
        r#"{"device_auth_id":"device-id","user_code":"DEVICE-CODE","interval":5}"#,
    )?;
    let mut device = device_environment.command();
    configure_codex_native(&mut device, &device_endpoint);
    device
        .args(["auth", "login", "codex", "--native", "--headless"])
        .stdout(Stdio::piped());

    let device_child = device.spawn()?;
    let mut device_guard = ProcessGuard::new(device_child);

    wait_for_file(
        &device_environment.signal_ready_path(),
        Duration::from_secs(10),
    )?;
    device_endpoint.wait_for_request()?;

    let device_stdout = device_guard.take_stdout().expect("stdout piped");
    let details = read_until_markers(
        device_stdout,
        "verification URL:",
        "device code:",
        Duration::from_secs(5),
    )?;

    let signal_result = signal(device_guard.id())?;
    assert!(signal_result.success());
    let device_output = device_guard.wait_with_output()?;
    let _ = device_endpoint.finish();

    assert_eq!(device_output.status.code(), Some(11));
    assert!(details.contains("Codex native verification URL:"));
    assert!(details.contains("Codex native device code: DEVICE-CODE"));
    assert!(!device_environment.auth_path().exists());
    Ok(())
}

#[test]
fn auth_cli_sigint_cancels_codex_subprocess_and_headless_openrouter_input(
) -> Result<(), Box<dyn std::error::Error>> {
    sigint_cancels_codex_and_openrouter()
}

#[test]
fn auth_cli_sigint_stress_matrix() -> Result<(), Box<dyn std::error::Error>> {
    for iteration in 1..=STRESS_ITERATIONS {
        sigint_cancels_codex_and_openrouter()
            .map_err(|e| format!("stress iteration {iteration}/{STRESS_ITERATIONS} failed: {e}"))?;
    }
    Ok(())
}

#[test]
fn auth_cli_sigint_escalates_codex_group_from_term_to_kill(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let environment = CliEnvironment::new("sigint-codex-escalation")?;
    let marker = environment.marker_path();
    let term_marker = environment.root_path().join("codex-term");
    environment.install_codex(
        "trap 'printf term > \"$HOME/codex-term\"' TERM\n(\n  trap '' TERM\n  /bin/sleep 30\n) &\nprintf '%s %s' \"$$\" \"$!\" > \"$HOME/codex-started\"\nwhile /bin/kill -0 \"$!\" 2>/dev/null; do\n  wait \"$!\" || :\ndone",
    )?;
    let mut command = environment.command();
    command
        .args(["auth", "login", "codex"])
        .stdout(Stdio::null());
    let child = command.spawn()?;
    let guard = ProcessGuard::new(child);
    wait_for_file(&environment.signal_ready_path(), Duration::from_secs(10))?;
    wait_for_file(&marker, Duration::from_secs(5))?;
    let process_ids = read_process_ids(&marker)?;
    assert_process_group(&process_ids)?;

    // When
    let signal_result = signal(guard.id())?;
    assert!(signal_result.success());
    let output = guard.wait_with_output()?;

    // Then
    assert_eq!(output.status.code(), Some(11));
    assert!(
        term_marker.exists(),
        "Codex process group was killed before receiving SIGTERM"
    );
    wait_for_processes_absent_or_zombie(&process_ids, Duration::from_secs(3))?;
    Ok(())
}

fn sigint_cancels_codex_and_openrouter() -> Result<(), Box<dyn std::error::Error>> {
    let codex_environment = CliEnvironment::new("sigint-codex")?;
    let marker = codex_environment.marker_path();
    codex_environment.install_codex(
        "/bin/sleep 30 &\nprintf '%s %s' \"$$\" \"$!\" > \"$HOME/codex-started\"\nwait",
    )?;
    let mut codex = codex_environment.command();
    codex.args(["auth", "login", "codex"]).stdout(Stdio::null());
    let codex_child = codex.spawn()?;
    let codex_guard = ProcessGuard::new(codex_child);

    let input_environment = CliEnvironment::new("sigint-headless-input")?;
    let mut input = input_environment.command();
    input
        .args(["auth", "login", "openrouter", "--headless"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    let input_child = input.spawn()?;
    let mut input_guard = ProcessGuard::new(input_child);
    let _input_stdin = input_guard.take_stdin()?;

    wait_for_file(
        &codex_environment.signal_ready_path(),
        Duration::from_secs(10),
    )?;
    wait_for_file(
        &input_environment.signal_ready_path(),
        Duration::from_secs(10),
    )?;
    wait_for_file(&marker, Duration::from_secs(5))?;
    let codex_process_ids = read_process_ids(&marker)?;
    assert_process_group(&codex_process_ids)?;

    let input_stdout = input_guard.take_stdout().expect("input stdout piped");
    let prompt = read_first_line(input_stdout, Duration::from_secs(5))?;
    assert!(
        prompt.contains("OpenRouter authorization URL:"),
        "expected OpenRouter prompt, got: {prompt}"
    );

    let codex_signal = signal(codex_guard.id())?;
    assert!(codex_signal.success());
    let codex_output = codex_guard.wait_with_output()?;

    let input_signal = signal(input_guard.id())?;
    assert!(input_signal.success());
    let input_status = input_guard.wait()?;

    assert_eq!(codex_output.status.code(), Some(11));
    assert!(!codex_environment.auth_path().exists());
    wait_for_processes_absent_or_zombie(&codex_process_ids, Duration::from_secs(3))?;
    assert_eq!(input_status.code(), Some(11));
    let input_status = input_environment
        .command()
        .args(["auth", "status", "openrouter"])
        .output()?;
    assert_eq!(input_status.status.code(), Some(10));

    let codex_root = codex_environment.root_path();
    let input_root = input_environment.root_path();
    drop(codex_environment);
    drop(input_environment);
    assert!(!codex_root.exists() && !input_root.exists());
    Ok(())
}

fn read_process_ids(path: &std::path::Path) -> Result<Vec<u32>, Box<dyn std::error::Error>> {
    std::fs::read_to_string(path)?
        .split_whitespace()
        .map(str::parse)
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn assert_process_group(process_ids: &[u32]) -> Result<(), Box<dyn std::error::Error>> {
    let Some((&leader, members)) = process_ids.split_first() else {
        return Err("Codex fixture did not record a process-group leader".into());
    };
    for &pid in members {
        let snapshot = read_process_snapshot(pid)?
            .ok_or_else(|| format!("Codex fixture process {pid} exited before inspection"))?;
        if snapshot.process_group != leader {
            return Err(format!(
                "Codex fixture process {pid} joined PGID {}, expected {leader}",
                snapshot.process_group
            )
            .into());
        }
    }
    Ok(())
}

fn wait_for_processes_absent_or_zombie(
    process_ids: &[u32],
    timeout: Duration,
) -> Result<(), std::io::Error> {
    let deadline = Instant::now() + timeout;
    loop {
        let mut running = Vec::new();
        for &pid in process_ids {
            if let Some(snapshot) = read_process_snapshot(pid)? {
                if snapshot.state != 'Z' {
                    running.push((pid, snapshot.state));
                }
            }
        }
        if running.is_empty() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("Codex processes remained alive after {timeout:?}: {running:?}"),
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

struct ProcessSnapshot {
    state: char,
    process_group: u32,
}

fn read_process_snapshot(pid: u32) -> Result<Option<ProcessSnapshot>, std::io::Error> {
    let stat = match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => stat,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let fields = stat
        .rsplit_once(") ")
        .map(|(_, fields)| fields)
        .ok_or_else(|| std::io::Error::other(format!("malformed process stat for PID {pid}")))?;
    let mut fields = fields.split_whitespace();
    let state = fields
        .next()
        .and_then(|state| state.chars().next())
        .ok_or_else(|| std::io::Error::other(format!("missing state for PID {pid}")))?;
    let _parent = fields
        .next()
        .ok_or_else(|| std::io::Error::other(format!("missing parent for PID {pid}")))?;
    let process_group = fields
        .next()
        .ok_or_else(|| std::io::Error::other(format!("missing PGID for PID {pid}")))?
        .parse()
        .map_err(|error| std::io::Error::other(format!("invalid PGID for PID {pid}: {error}")))?;
    Ok(Some(ProcessSnapshot {
        state,
        process_group,
    }))
}

fn run_headless(
    mut command: std::process::Command,
) -> Result<std::process::Output, std::io::Error> {
    let mut child = command.spawn()?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| std::io::Error::other("headless stdin missing"))?;
    stdin.write_all(b"headless-code\n")?;
    drop(stdin);
    child.wait_with_output()
}

fn signal(pid: u32) -> Result<std::process::ExitStatus, std::io::Error> {
    std::process::Command::new("/bin/kill")
        .args(["-INT", &pid.to_string()])
        .status()
}

fn wait_for_file(path: &std::path::Path, timeout: Duration) -> Result<(), std::io::Error> {
    let deadline = Instant::now() + timeout;
    while !path.exists() {
        if Instant::now() >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("file not created within {timeout:?}: {}", path.display()),
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

fn read_until_markers(
    stdout: std::process::ChildStdout,
    first: &str,
    second: &str,
    timeout: Duration,
) -> Result<String, Box<dyn std::error::Error>> {
    let (sender, receiver) = mpsc::channel();
    let first = first.to_owned();
    let second = second.to_owned();
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        let mut details = String::new();
        for line in reader.lines() {
            let Ok(line) = line else { break };
            details.push_str(&line);
            details.push('\n');
            if details.contains(&first) && details.contains(&second) {
                break;
            }
        }
        let _ = sender.send(details);
    });
    receiver
        .recv_timeout(timeout)
        .map_err(|e| format!("timed out reading stdout markers: {e}").into())
}

fn read_first_line(
    stdout: std::process::ChildStdout,
    timeout: Duration,
) -> Result<String, Box<dyn std::error::Error>> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = BufReader::new(stdout).read_line(&mut line);
        let _ = sender.send(line);
    });
    receiver
        .recv_timeout(timeout)
        .map_err(|e| format!("timed out reading first stdout line: {e}").into())
}
