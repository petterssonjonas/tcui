use std::io::{self, IsTerminal, Write};

use base64::{engine::general_purpose::STANDARD, Engine as _};

/// Copies text through a desktop helper, or requests the terminal clipboard with OSC 52.
/// OSC 52 has no acknowledgement: success means the request was written, not accepted.
/// Remote sessions target the client terminal instead of the remote desktop clipboard.
pub(crate) fn copy(text: &str) -> io::Result<()> {
    let remote = ["SSH_CONNECTION", "SSH_CLIENT", "SSH_TTY", "MOSH_CONNECTION"]
        .iter()
        .any(|name| std::env::var_os(name).is_some());
    #[cfg(unix)]
    if !remote {
        if let Some(result) = copy_native(text) {
            return result;
        }
    }
    #[cfg(not(unix))]
    let _ = remote;

    let stdout = io::stdout();
    if !stdout.is_terminal() || std::env::var("TERM").is_ok_and(|term| term == "dumb") {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "clipboard requires a desktop clipboard helper or an OSC 52 terminal",
        ));
    }
    let sequence = osc52_sequence(text, std::env::var_os("TMUX").is_some());
    let mut stdout = stdout.lock();
    stdout.write_all(sequence.as_bytes())?;
    stdout.flush()
}

fn osc52_sequence(text: &str, tmux: bool) -> String {
    let mut sequence =
        String::with_capacity(text.len().saturating_add(text.len() / 3).saturating_add(32));
    // Match the terminal renderer's tmux DCS passthrough framing. The tmux server
    // must permit passthrough, and the outer terminal must enable OSC 52 writes.
    sequence.push_str(if tmux {
        "\x1bPtmux;\x1b\x1b]52;c;"
    } else {
        "\x1b]52;c;"
    });
    STANDARD.encode_string(text, &mut sequence);
    sequence.push('\x07');
    if tmux {
        sequence.push_str("\x1b\\");
    }
    sequence
}

#[cfg(unix)]
fn copy_native(text: &str) -> Option<io::Result<()>> {
    use std::time::{Duration, Instant};

    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let x11 = std::env::var_os("DISPLAY").is_some();
    let helpers: [(bool, &str, &[&str]); 4] = [
        (cfg!(target_os = "macos"), "pbcopy", &[]),
        (
            cfg!(target_os = "linux") && wayland,
            "wl-copy",
            &["--type", "text/plain;charset=utf-8"],
        ),
        (
            cfg!(target_os = "linux") && x11,
            "xclip",
            &["-selection", "clipboard"],
        ),
        (
            cfg!(target_os = "linux") && x11,
            "xsel",
            &["--clipboard", "--input"],
        ),
    ];
    let deadline = Instant::now() + Duration::from_millis(750);
    let mut failure = None;
    for (enabled, program, args) in helpers {
        if !enabled {
            continue;
        }
        match run_helper(program, args, text.as_bytes(), deadline) {
            Ok(()) => return Some(Ok(())),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                failure = Some(io::Error::new(error.kind(), format!("{program}: {error}")));
            }
        }
    }
    // Missing helpers can fall back to OSC 52. A helper that actually failed must
    // not be reported as a successful copy merely because stdout is writable.
    failure.map(Err)
}

#[cfg(unix)]
fn run_helper(
    program: &str,
    args: &[&str],
    text: &[u8],
    deadline: std::time::Instant,
) -> io::Result<()> {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    use rustix::fs::{fcntl_getfl, fcntl_setfl, OFlags};
    if Instant::now() >= deadline {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "clipboard helper timed out",
        ));
    }

    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let result = (|| {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("missing clipboard stdin"))?;
        fcntl_setfl(&stdin, fcntl_getfl(&stdin)? | OFlags::NONBLOCK)?;
        let mut remaining = text;
        while !remaining.is_empty() {
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "clipboard helper timed out",
                ));
            }
            match stdin.write(remaining) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "clipboard helper closed stdin",
                    ))
                }
                Ok(written) => remaining = &remaining[written..],
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => return Err(error),
            }
        }
        drop(stdin);
        loop {
            if let Some(status) = child.try_wait()? {
                return if status.success() {
                    Ok(())
                } else {
                    Err(io::Error::other(format!(
                        "clipboard helper exited with {status}"
                    )))
                };
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "clipboard helper timed out",
                ));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    })();
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osc52_encodes_utf8_and_control_characters_without_escape_injection() {
        let text = "hello\n世界\x1b]52;c;malicious\x07";
        let sequence = osc52_sequence(text, false);
        let payload = sequence
            .strip_prefix("\x1b]52;c;")
            .unwrap()
            .strip_suffix('\x07')
            .unwrap();
        assert_eq!(STANDARD.decode(payload).unwrap(), text.as_bytes());
        assert!(!payload.chars().any(char::is_control));
    }

    #[test]
    fn osc52_uses_standard_padded_base64() {
        assert_eq!(osc52_sequence("a", false), "\x1b]52;c;YQ==\x07");
    }

    #[test]
    fn osc52_empty_text_clears_clipboard() {
        assert_eq!(osc52_sequence("", false), "\x1b]52;c;\x07");
    }

    #[test]
    fn tmux_passthrough_doubles_escape_and_terminates_dcs() {
        assert_eq!(
            osc52_sequence("a", true),
            "\x1bPtmux;\x1b\x1b]52;c;YQ==\x07\x1b\\"
        );
    }

    #[cfg(unix)]
    #[test]
    fn helper_failure_is_not_success() {
        let result = run_helper(
            "sh",
            &["-c", "cat >/dev/null; exit 7"],
            b"clipboard text",
            std::time::Instant::now() + std::time::Duration::from_secs(1),
        );
        assert!(result.is_err());
    }

    #[cfg(unix)]
    #[test]
    fn helper_that_does_not_read_stdin_times_out() {
        let result = run_helper(
            "sh",
            &["-c", "exec sleep 10"],
            &vec![b'x'; 256 * 1024],
            std::time::Instant::now() + std::time::Duration::from_millis(25),
        );
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
    }
}
