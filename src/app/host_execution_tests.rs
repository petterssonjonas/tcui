#![cfg(target_os = "linux")]

use std::os::fd::OwnedFd;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rustix::process::{Pid, WaitId, WaitIdOptions};
use serde_json::{json, Value};
use tokio::io::unix::AsyncFd;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::unix::pipe;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

use super::action::Action;
use super::submission::SubmissionId;
use super::tests::with_test_app_config;
use super::TuiApp;
use crate::config::{AppConfig, ProviderConfig};

const DEADLINE: Duration = Duration::from_secs(5);

#[test]
fn cancelling_host_request_during_planning_prevents_later_execution() {
    with_test_app_config("host-cancel-planning", fixture_config(), |app| {
        let root = scratch_directory();
        let marker = root.join("executed");
        let program = install_helper(&root, "printf executed > \"$1\"");
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let planner = planner_fixture(vec![marker.to_str().unwrap().to_owned()]).await;
                configure_app(app, &program, &planner.endpoint).await;

                tokio::time::timeout(
                    DEADLINE,
                    app.dispatch(Action::SendMessage(format!(
                        "@command {} create the fixture marker",
                        program.display()
                    ))),
                )
                .await
                .expect("SendMessage returns while the planner gate remains closed")
                .unwrap();
                let id = app.active_request_id().expect("preparation owns a request");
                tokio::time::timeout(DEADLINE, planner.request)
                    .await
                    .expect("planner receives the actual HTTP request")
                    .unwrap();
                assert!(!marker.exists());

                tokio::time::timeout(DEADLINE, app.dispatch(Action::CancelRequest))
                    .await
                    .expect("root cancellation remains responsive during planning")
                    .unwrap();
                assert_request_cleared(app, id);
                observe_worker_stop(app, id).await;
                planner.release.send(()).expect("release stalled planner");
                tokio::time::timeout(DEADLINE, planner.finished)
                    .await
                    .expect("planner fixture finishes after release")
                    .unwrap();

                // Worker destruction precedes gate release: a late valid plan has
                // no remaining consumer capable of launching the helper.
                assert!(!marker.exists());
                assert_request_cleared(app, id);
            });
    });
}

#[test]
fn cancelling_executing_host_request_terminates_group_without_final_side_effect() {
    with_test_app_config("host-cancel-executing", fixture_config(), |app| {
        let root = scratch_directory();
        let started_path = root.join("started.fifo");
        let release_path = root.join("release.fifo");
        let marker = root.join("completed");
        for path in [&started_path, &release_path] {
            rustix::fs::mkfifoat(
                rustix::fs::CWD,
                path,
                rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
            )
            .unwrap();
        }
        let program = install_helper(
            &root,
            "/bin/sleep 120 &\nchild=$!\nprintf '%s %s\\n' \"$$\" \"$child\" > \"$1\"\nIFS= read -r release < \"$2\"\nprintf completed > \"$3\"\nwait \"$child\"",
        );
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let started = pipe::OpenOptions::new()
                    .read_write(true)
                    .open_receiver(&started_path)
                    .unwrap();
                let mut release = pipe::OpenOptions::new()
                    .read_write(true)
                    .open_sender(&release_path)
                    .unwrap();
                let arguments = [&started_path, &release_path, &marker]
                    .map(|path| path.to_str().unwrap().to_owned())
                    .to_vec();
                let planner = planner_fixture(arguments).await;
                configure_app(app, &program, &planner.endpoint).await;
                tokio::time::timeout(
                    DEADLINE,
                    app.dispatch(Action::SendMessage(format!(
                        "@command {} run the gated fixture helper",
                        program.display()
                    ))),
                )
                .await
                .expect("SendMessage registers the cancellable preparation task")
                .unwrap();
                let id = app.active_request_id().expect("host request registered");
                tokio::time::timeout(DEADLINE, planner.request)
                    .await
                    .expect("planner request")
                    .unwrap();
                planner.release.send(()).unwrap();
                tokio::time::timeout(DEADLINE, planner.finished)
                    .await
                    .unwrap()
                    .unwrap();

                let mut started = BufReader::new(started);
                let mut pids = String::new();
                tokio::time::timeout(DEADLINE, started.read_line(&mut pids))
                    .await
                    .expect("helper explicitly signals leader and descendant startup")
                    .unwrap();
                let pids = pids
                    .split_whitespace()
                    .map(|pid| Pid::from_raw(pid.parse().unwrap()).unwrap())
                    .collect::<Vec<_>>();
                assert_eq!(pids.len(), 2);
                assert_eq!(rustix::process::getpgid(Some(pids[1])).unwrap(), pids[0]);
                let exits = [watch_process_exit(pids[0]), watch_process_exit(pids[1])];
                assert!(!marker.exists());
                assert_eq!(app.active_request_id(), Some(id));

                tokio::time::timeout(DEADLINE, app.dispatch(Action::CancelRequest))
                    .await
                    .expect("root cancellation remains responsive during execution")
                    .unwrap();
                assert_request_cleared(app, id);
                observe_worker_stop(app, id).await;
                for exit in &exits {
                    let _ready = tokio::time::timeout(DEADLINE, exit.readable())
                        .await
                        .expect("cancellation terminates each group member")
                        .unwrap();
                }
                observe_leader_reaped(pids[0]).await;

                // Release only after both processes have exited. A surviving
                // helper would pass its gate and create this forbidden effect.
                release.write_all(b"continue\n").await.unwrap();
                assert!(!marker.exists());
                assert_request_cleared(app, id);
            });
    });
}

fn fixture_config() -> AppConfig {
    let mut config = AppConfig {
        default_provider: "Ollama".to_owned(),
        default_model: "host-fixture".to_owned(),
        providers: vec![ProviderConfig {
            name: "Ollama".to_owned(),
            endpoint: "http://127.0.0.1:9".to_owned(),
            env_var: String::new(),
            backend_type: "openai".to_owned(),
            auth_type: "api_key".to_owned(),
        }],
        ..AppConfig::default()
    };
    config.web_search.enabled = false;
    #[cfg(feature = "memory")]
    {
        config.memory.enabled = false;
    }
    config
}

fn scratch_directory() -> PathBuf {
    // with_test_app_config owns and removes this private HOME, including FIFOs.
    let root = PathBuf::from(std::env::var_os("HOME").unwrap()).join("host-execution");
    std::fs::create_dir(&root).unwrap();
    root
}

fn install_helper(root: &Path, body: &str) -> PathBuf {
    let program = root.join("fixture-helper");
    std::fs::write(&program, format!("#!/bin/sh\nset -eu\n{body}\n")).unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
    program
}

async fn configure_app(app: &mut TuiApp, program: &Path, endpoint: &str) {
    app.config.write().await.commands.whitelist = vec![program.to_str().unwrap().to_owned()];
    let tab = &mut app.ui.tabs[app.ui.active_tab];
    tab.tab.provider = "Ollama".to_owned();
    tab.tab.model = "host-fixture".to_owned();
    tab.tab.endpoint = Some(endpoint.to_owned());
}

struct PlannerFixture {
    endpoint: String,
    request: oneshot::Receiver<Value>,
    release: oneshot::Sender<()>,
    finished: tokio::task::JoinHandle<()>,
}

async fn planner_fixture(arguments: Vec<String>) -> PlannerFixture {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let (request_tx, request) = oneshot::channel();
    let (release, release_rx) = oneshot::channel();
    let finished = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        request_tx.send(read_http_json(&mut stream).await).unwrap();
        release_rx.await.expect("explicit planner release");
        let plan = json!({"arguments": arguments}).to_string();
        let event = json!({"choices": [{"delta": {"content": plan}}]});
        let body = format!("data: {event}\n\ndata: [DONE]\n\n");
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        // Cancellation deliberately closes the first test's HTTP client before
        // the gated response. BrokenPipe/connection reset is expected there.
        let _ = stream.write_all(response.as_bytes()).await;
        let _ = stream.shutdown().await;
    });
    PlannerFixture {
        endpoint,
        request,
        release,
        finished,
    }
}

async fn read_http_json(stream: &mut TcpStream) -> Value {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4096];
    let header_end = loop {
        if let Some(end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
            break end + 4;
        }
        let count = stream.read(&mut buffer).await.unwrap();
        assert!(
            count > 0 && request.len() < 64 * 1024,
            "bounded HTTP header"
        );
        request.extend_from_slice(&buffer[..count]);
    };
    let headers = std::str::from_utf8(&request[..header_end]).unwrap();
    assert!(headers.starts_with("POST /chat/completions HTTP/1.1\r\n"));
    let length = headers
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().unwrap())
        })
        .expect("JSON request has a content length");
    assert!(length <= 64 * 1024, "bounded planner request body");
    while request.len() < header_end + length {
        let count = stream.read(&mut buffer).await.unwrap();
        assert!(count > 0, "complete HTTP body");
        request.extend_from_slice(&buffer[..count]);
    }
    serde_json::from_slice(&request[header_end..header_end + length]).unwrap()
}

async fn observe_worker_stop(app: &mut TuiApp, id: SubmissionId) {
    tokio::time::timeout(DEADLINE, async {
        loop {
            let action = app
                .action_rx
                .recv()
                .await
                .expect("worker completion action");
            let stopped = matches!(&action, Action::StopStream(stopped) if *stopped == id);
            app.dispatch(action).await.unwrap();
            if stopped {
                break;
            }
        }
    })
    .await
    .expect("cancelled worker drops its completion guard");
}

fn assert_request_cleared(app: &TuiApp, id: SubmissionId) {
    assert!(app.active_request_id().is_none());
    assert!(!app.requests.contains_key(&id));
    assert!(!app.ui.tabs[app.ui.active_tab].streaming);
    assert!(!app.request_accepts_events(id));
}

fn watch_process_exit(pid: Pid) -> AsyncFd<OwnedFd> {
    let fd = rustix::process::pidfd_open(pid, rustix::process::PidfdFlags::NONBLOCK).unwrap();
    // SAFETY: OwnedFd keeps this nonblocking pidfd open and unchanged throughout
    // registration. Only readiness is observed; no blocking descriptor I/O occurs.
    unsafe { AsyncFd::register(fd) }.unwrap()
}

async fn observe_leader_reaped(pid: Pid) {
    tokio::time::timeout(DEADLINE, async {
        loop {
            match rustix::process::waitid(
                WaitId::Pid(pid),
                WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
            ) {
                Err(rustix::io::Errno::CHILD) => break,
                Ok(_) | Err(rustix::io::Errno::INTR) => tokio::task::yield_now().await,
                Err(error) => panic!("inspect direct-child reaping: {error}"),
            }
        }
    })
    .await
    .expect("Tokio reaps the cancelled helper without a blocking waitpid");
}
