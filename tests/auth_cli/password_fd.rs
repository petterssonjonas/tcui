use std::fs::{File, OpenOptions};
use std::io::{Seek, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::process::ProcessGuard;
use super::support::CliEnvironment;

fn password_source(environment: &CliEnvironment, pipe: bool) -> std::io::Result<File> {
    if pipe {
        let mut descriptors = [-1; 2];
        // SAFETY: pipe2 writes exactly two descriptors into the supplied array.
        if unsafe { libc::pipe2(descriptors.as_mut_ptr(), libc::O_CLOEXEC) } < 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: successful pipe2 returned two newly owned, distinct descriptors.
        let reader = unsafe { File::from_raw_fd(descriptors[0]) };
        let mut writer = unsafe { File::from_raw_fd(descriptors[1]) };
        writer.write_all(b"synthetic-inherited-password-canary\n")?;
        return Ok(reader);
    }
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(environment.root_path().join("password-input"))?;
    file.write_all(b"synthetic-inherited-password-canary\n")?;
    file.rewind()?;
    Ok(file)
}

fn inherited_descriptor(source: &File) -> std::io::Result<File> {
    // SAFETY: fcntl validates the live descriptor and returns a new descriptor owned here.
    let descriptor = unsafe { libc::fcntl(source.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 64) };
    if descriptor < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: the successful duplication above transferred sole ownership of this descriptor.
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

fn pass_descriptor(command: &mut Command, descriptor: &File) {
    let raw = descriptor.as_raw_fd();
    // SAFETY: the closure performs only async-signal-safe fcntl after fork. The owned
    // descriptor remains live until the child exits; only the child's flags change.
    unsafe {
        command.pre_exec(move || {
            if libc::fcntl(raw, libc::F_SETFD, 0) < 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
}

#[test]
fn password_fd_is_not_inherited_by_real_browser_helpers() -> Result<(), Box<dyn std::error::Error>>
{
    for pipe in [false, true] {
        let environment = CliEnvironment::new(if pipe {
            "password-pipe"
        } else {
            "password-file"
        })?;
        let source = password_source(&environment, pipe)?;
        let metadata = source.metadata()?;
        let descriptor = inherited_descriptor(&source)?;
        let browser = environment.root_path().join("bin/password-browser");
        let marker = environment.root_path().join("browser-fd-result");
        std::fs::write(
            &browser,
            r#"#!/bin/sh
result=contained
for descriptor in /proc/self/fd/*; do
    identity=$(/usr/bin/stat -Lc '%d:%i' "$descriptor" 2>/dev/null) || continue
    if [ "$identity" = "$TCUI_PASSWORD_FD_PROBE_OBJECT" ]; then
        result=exposed
        break
    fi
done
printf '%s' "$result" > "$TCUI_PASSWORD_FD_PROBE_MARKER"
exit 1
"#,
        )?;
        std::fs::set_permissions(&browser, std::fs::Permissions::from_mode(0o700))?;
        let mut command = environment.command();
        command
            .args([
                "--unlock-password-fd",
                &descriptor.as_raw_fd().to_string(),
                "auth",
                "--plaintext",
                "login",
                "codex",
                "--open-browser",
            ])
            .env("BROWSER", &browser)
            .env("TCUI_PASSWORD_FD_PROBE_MARKER", &marker)
            .env(
                "TCUI_PASSWORD_FD_PROBE_OBJECT",
                format!("{}:{}", metadata.dev(), metadata.ino()),
            )
            .stdin(Stdio::null());
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        pass_descriptor(&mut command, &descriptor);
        let mut child = ProcessGuard::new(command.spawn()?);
        let deadline = Instant::now() + Duration::from_secs(5);
        let observed = loop {
            if let Ok(value) = std::fs::read_to_string(&marker) {
                if value == "contained" || value == "exposed" {
                    break value;
                }
            }
            if Instant::now() >= deadline {
                return Err("browser descriptor probe did not complete".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        child.kill()?;
        let output = child.wait_with_output()?;
        assert_eq!(
            observed, "contained",
            "password input object must not reach any browser/helper descriptor"
        );
        assert!(!output
            .stdout
            .windows(b"synthetic-inherited-password-canary".len())
            .any(|bytes| bytes == b"synthetic-inherited-password-canary"));
    }
    Ok(())
}

#[test]
fn unused_password_transports_are_rejected_before_external_logout(
) -> Result<(), Box<dyn std::error::Error>> {
    for transport in ["fd", "stdin", "auth-stdin"] {
        let environment = CliEnvironment::new("unused-password-transport")?;
        environment.install_codex("printf invoked > \"$HOME/codex-started\"")?;
        let mut source = password_source(&environment, false)?;
        let descriptor = inherited_descriptor(&source)?;
        let mut modes = vec![vec![], vec!["auth", "logout", "codex", "--external"]];
        if transport == "auth-stdin" {
            modes.push(vec!["--export-all-chats", "auth", "status", "codex"]);
        }
        for mut arguments in modes {
            if transport == "auth-stdin" && arguments.is_empty() {
                continue;
            }
            let mut command = environment.command();
            if transport == "fd" {
                command.args(["--unlock-password-fd", &descriptor.as_raw_fd().to_string()]);
                pass_descriptor(&mut command, &descriptor);
            } else {
                if transport == "auth-stdin" {
                    let auth = arguments
                        .iter()
                        .position(|argument| *argument == "auth")
                        .expect("auth-specific transport requires an auth subcommand");
                    arguments.insert(auth + 1, "--vault-password-stdin");
                } else {
                    command.arg("--unlock-password-stdin");
                }
                command.stdin(Stdio::from(source.try_clone()?));
            }
            command.args(arguments);
            let status = command.status()?;
            assert!(!status.success());
            assert!(
                !environment.marker_path().exists(),
                "rejected password transport must not launch external Codex"
            );
            assert!(
                !environment.key_path().exists(),
                "rejected mode must not initialize application storage"
            );
            assert_eq!(
                source.stream_position()?,
                0,
                "rejected mode must not consume a password"
            );
        }
    }
    Ok(())
}
