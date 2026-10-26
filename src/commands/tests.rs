#![expect(
    clippy::await_holding_lock,
    reason = "Process-global environment changes are serialized for child-process tests."
)]

use std::ffi::OsString;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::sync::atomic::{AtomicU64, Ordering};

use super::*;

struct Fixture {
    root: PathBuf,
    old_path: Option<OsString>,
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "tcui-command-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::create_dir_all(root.join("other")).unwrap();
        let old_path = std::env::var_os("PATH");
        std::env::set_var(
            "PATH",
            format!("{}:/usr/bin:/bin", root.join("bin").display()),
        );
        Self { root, old_path }
    }

    fn script(&self, name: &str, body: &str) -> String {
        let path = self.root.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        path.to_str().unwrap().to_owned()
    }

    fn approved(&self, program: &str) -> AppConfig {
        let mut config = AppConfig::default();
        config.commands.whitelist = vec![program.to_owned()];
        config
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        match &self.old_path {
            Some(value) => std::env::set_var("PATH", value),
            None => std::env::remove_var("PATH"),
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn empty_policy_denies_even_existing_executables() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let fixture = Fixture::new();
    let program = fixture.script("bin/tool", "exit 0");
    assert!(authorize(&AppConfig::default(), &program).is_err());
}

#[tokio::test]
async fn denied_and_invalid_launches_never_start_the_executable() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let fixture = Fixture::new();
    let marker = fixture.root.join("started");
    let program = fixture.script(
        "bin/tool",
        &format!("printf started > '{}'", marker.display()),
    );
    assert!(run(&AppConfig::default(), &program, &[]).await.is_err());
    let mut config = fixture.approved(&program);
    config.commands.blacklist = vec![program.clone()];
    assert!(run(&config, &program, &[]).await.is_err());
    config.commands.blacklist.clear();
    for arguments in [
        vec!["bad\0argument".to_owned()],
        vec!["x".repeat(MAX_ARGUMENT_BYTES + 1)],
        vec![String::new(); 129],
    ] {
        assert!(run(&config, &program, &arguments).await.is_err());
    }
    assert!(
        run_with_input(&config, &program, &[], &vec![0; MAX_ARGUMENT_BYTES + 1])
            .await
            .is_err()
    );
    assert!(!marker.exists());
}

#[test]
fn name_approval_does_not_grant_same_basename_elsewhere() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let fixture = Fixture::new();
    let approved = fixture.script("bin/tool", "exit 0");
    let impostor = fixture.script("other/tool", "exit 0");
    let config = fixture.approved("tool");
    assert_eq!(authorize(&config, "tool").unwrap(), PathBuf::from(approved));
    assert!(authorize(&config, &impostor).is_err());
}

#[test]
fn blacklist_overrides_approval_through_symlink_and_canonical_names() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let fixture = Fixture::new();
    let target = fixture.script("bin/tool", "exit 0");
    let alias = fixture.root.join("bin/alias");
    symlink(&target, &alias).unwrap();
    for denied in [
        "tool".to_owned(),
        "alias".to_owned(),
        target.clone(),
        alias.to_str().unwrap().to_owned(),
    ] {
        let mut config = fixture.approved(&target);
        config.commands.blacklist = vec![denied];
        assert!(authorize(&config, alias.to_str().unwrap()).is_err());
    }
}

#[test]
fn blacklist_name_resolves_alias_target_even_under_different_basename() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let fixture = Fixture::new();
    let target = fixture.script("bin/tool", "exit 0");
    symlink(&target, fixture.root.join("bin/blocked-alias")).unwrap();
    let mut config = fixture.approved(&target);
    config.commands.blacklist = vec!["blocked-alias".to_owned()];
    assert!(authorize(&config, &target).is_err());
}

#[test]
fn relative_invalid_missing_and_nonexecutable_programs_are_rejected() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let fixture = Fixture::new();
    let file = fixture.root.join("plain");
    std::fs::write(&file, "not executable").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
    let config = fixture.approved(file.to_str().unwrap());
    for program in [
        "",
        "./tool",
        "bin/tool",
        "../tool",
        "tool\0suffix",
        "tool\n",
        "two words",
        "-tool",
        "missing-tool",
        file.to_str().unwrap(),
        fixture.root.to_str().unwrap(),
    ] {
        assert!(
            authorize(&config, program).is_err(),
            "authorized {program:?}"
        );
    }
}

#[tokio::test]
async fn direct_argv_does_not_interpret_shell_syntax_and_reports_status() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let fixture = Fixture::new();
    let program = fixture.script(
        "bin/arguments",
        "printf '%s\\n' \"$@\"; printf 'failure detail' >&2; exit 7",
    );
    let marker = fixture.root.join("must-not-exist");
    let arguments = vec![
        format!("$(touch {})", marker.display()),
        "a;b | echo bad > nope".to_owned(),
        "two words".to_owned(),
    ];
    let output = run(&fixture.approved(&program), &program, &arguments)
        .await
        .unwrap();
    assert_eq!(output.stdout, format!("{}\n", arguments.join("\n")));
    assert_eq!(output.stderr, "failure detail");
    assert_eq!(output.code, Some(7));
    assert!(!output.success);
    assert!(!marker.exists());
}

#[tokio::test]
async fn child_environment_omits_ambient_credentials_and_private_input_uses_stdin() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let fixture = Fixture::new();
    let name = "TCUI_COMMAND_TEST_PRIVATE_TOKEN";
    let previous = std::env::var_os(name);
    std::env::set_var(name, "must-not-leak");
    let program = fixture.script(
        "bin/environment",
        "printf '%s|' \"${TCUI_COMMAND_TEST_PRIVATE_TOKEN-unset}\"; /bin/cat",
    );
    let result = run_with_input(&fixture.approved(&program), &program, &[], b"private stdin").await;
    match previous {
        Some(value) => std::env::set_var(name, value),
        None => std::env::remove_var(name),
    }
    assert_eq!(result.unwrap().stdout, "unset|private stdin");
}

#[tokio::test]
async fn output_is_bounded_on_both_streams() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let fixture = Fixture::new();
    for redirect in ["", " >&2"] {
        let program = fixture.script(
            "bin/noisy",
            &format!("while :; do printf '%01000d' 0{redirect}; done"),
        );
        let result = run_with_timeout(
            &fixture.approved(&program),
            &program,
            &[],
            None,
            Duration::from_secs(5),
        )
        .await;
        assert!(result.unwrap_err().to_string().contains("output limit"));
    }
}

#[tokio::test]
async fn stalled_stdin_write_obeys_process_timeout() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let fixture = Fixture::new();
    let program = fixture.script("bin/ignore-input", "exec /bin/sleep 30");
    let input = vec![b'x'; MAX_ARGUMENT_BYTES];
    let result = run_with_timeout(
        &fixture.approved(&program),
        &program,
        &[],
        Some(&input),
        Duration::from_millis(30),
    )
    .await;
    assert!(result.unwrap_err().to_string().contains("timed out"));
}

#[tokio::test]
async fn timeout_kills_and_reaps_the_direct_child() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let fixture = Fixture::new();
    let pids = fixture.root.join("pid");
    let program = fixture.script(
        "bin/slow",
        &format!(
            "printf '%s' \"$$\" > '{}'; exec /bin/sleep 30",
            pids.display()
        ),
    );
    let config = fixture.approved(&program);
    let task = tokio::spawn(async move { run(&config, &program, &[]).await });
    let pid = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(text) = std::fs::read_to_string(&pids) {
                if let Ok(pid) = text.parse::<i32>() {
                    break pid;
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("child started");
    tokio::time::pause();
    tokio::time::advance(TIMEOUT + Duration::from_secs(1)).await;
    let error = task.await.unwrap().unwrap_err();
    tokio::time::resume();
    assert!(error.to_string().contains("timed out"));
    wait_until_reaped(pid).await;
    assert!(!process_running(pid));
}

#[tokio::test]
async fn cancellation_kills_process_group_and_reaps_leader() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let fixture = Fixture::new();
    let pids = fixture.root.join("pids");
    let program = fixture.script(
        "bin/cancel",
        &format!(
            "/bin/sleep 30 &\nprintf '%s %s' \"$$\" \"$!\" > '{}'\nwait",
            pids.display()
        ),
    );
    let config = fixture.approved(&program);
    let task = tokio::spawn(async move { run(&config, &program, &[]).await });
    let pids = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(text) = std::fs::read_to_string(&pids) {
                let pids = text
                    .split_whitespace()
                    .filter_map(|pid| pid.parse::<i32>().ok())
                    .collect::<Vec<_>>();
                if pids.len() == 2 {
                    break pids;
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    tokio::time::timeout(Duration::from_secs(5), async {
        while pids.iter().any(|pid| process_running(*pid)) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("process group terminated");
    wait_until_reaped(pids[0]).await;
}

#[tokio::test]
async fn exited_leader_remains_waitable_until_descendant_cleanup() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let fixture = Fixture::new();
    let path = fixture.root.join("pids");
    let program = fixture.script(
        "bin/exited-leader",
        &format!(
            "/bin/sleep 30 &\nprintf '%s %s' \"$$\" \"$!\" > '{}'\nexit 0",
            path.display()
        ),
    );
    let config = fixture.approved(&program);
    let task = tokio::spawn(async move { run(&config, &program, &[]).await });
    let pids = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(text) = std::fs::read_to_string(&path) {
                let pids = text
                    .split_whitespace()
                    .filter_map(|pid| pid.parse::<i32>().ok())
                    .collect::<Vec<_>>();
                if pids.len() == 2 {
                    break pids;
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let leader = rustix::process::Pid::from_raw(pids[0]).unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let status = rustix::process::waitid(
                rustix::process::WaitId::Pid(leader),
                rustix::process::WaitIdOptions::EXITED
                    | rustix::process::WaitIdOptions::NOHANG
                    | rustix::process::WaitIdOptions::NOWAIT,
            )
            .expect("leader must not be reaped before group cleanup");
            if status.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(!task.is_finished(), "descendant still holds output pipes");
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    wait_until_reaped(pids[0]).await;
    tokio::time::timeout(Duration::from_secs(5), async {
        while process_running(pids[1]) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("descendant terminated despite exited leader");
}

async fn wait_until_reaped(pid: i32) {
    let pid = rustix::process::Pid::from_raw(pid).unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match rustix::process::waitid(
                rustix::process::WaitId::Pid(pid),
                rustix::process::WaitIdOptions::EXITED
                    | rustix::process::WaitIdOptions::NOHANG
                    | rustix::process::WaitIdOptions::NOWAIT,
            ) {
                Err(rustix::io::Errno::CHILD) => break,
                Ok(_) | Err(rustix::io::Errno::INTR) => {}
                Err(error) => panic!("failed to inspect child reaping: {error}"),
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("Tokio reaped direct child");
}

fn process_running(pid: i32) -> bool {
    // Linux can briefly retain an orphaned grandchild as a zombie until init reaps it.
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
            stat.rsplit_once(") ")
                .is_some_and(|(_, rest)| !rest.starts_with(['Z', 'X']))
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        rustix::process::Pid::from_raw(pid)
            .is_some_and(|pid| rustix::process::test_kill_process(pid).is_ok())
    }
}
