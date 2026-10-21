use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use portable_pty::PtySize;
use ratatui::layout::Rect;

use super::{CloseRequest, SessionChild, SessionMaster, SpawnedSession, TerminalEditorState};
use crate::vault::ValidatedEditorTarget;

#[derive(Debug, Default)]
struct SessionCounts {
    kills: usize,
    waits: usize,
    child_drops: usize,
    master_drops: usize,
    resizes: Vec<PtySize>,
}

#[derive(Debug)]
struct FakeChild {
    counts: Arc<Mutex<SessionCounts>>,
    exited: bool,
}

impl SessionChild for FakeChild {
    fn terminate(&mut self) -> io::Result<()> {
        self.counts.lock().expect("counts lock").kills += 1;
        self.exited = true;
        Ok(())
    }

    fn has_exited(&mut self) -> io::Result<bool> {
        Ok(self.exited)
    }

    fn wait(&mut self) -> io::Result<()> {
        self.counts.lock().expect("counts lock").waits += 1;
        self.exited = true;
        Ok(())
    }
}

impl Drop for FakeChild {
    fn drop(&mut self) {
        self.counts.lock().expect("counts lock").child_drops += 1;
    }
}

#[derive(Debug)]
struct FakeMaster {
    counts: Arc<Mutex<SessionCounts>>,
}

impl SessionMaster for FakeMaster {
    fn resize(&self, size: PtySize) -> io::Result<()> {
        self.counts.lock().expect("counts lock").resizes.push(size);
        Ok(())
    }
}

impl Drop for FakeMaster {
    fn drop(&mut self) {
        self.counts.lock().expect("counts lock").master_drops += 1;
    }
}

struct Fixture {
    editor: TerminalEditorState,
    counts: Arc<Mutex<SessionCounts>>,
    input_rx: tokio::sync::mpsc::Receiver<bytes::Bytes>,
    output_tx: tokio::sync::mpsc::Sender<Vec<u8>>,
}

impl Fixture {
    fn new(exited: bool) -> Self {
        let counts = Arc::new(Mutex::new(SessionCounts::default()));
        let (input_tx, input_rx) = tokio::sync::mpsc::channel(4);
        let (output_tx, output_rx) = tokio::sync::mpsc::channel(4);
        let session = SpawnedSession {
            child: Box::new(FakeChild {
                counts: Arc::clone(&counts),
                exited,
            }),
            master: Box::new(FakeMaster {
                counts: Arc::clone(&counts),
            }),
            input_tx,
            output_rx,
            transport: None,
        };
        let mut editor = TerminalEditorState::from_spawned(session, Rect::new(0, 0, 80, 24));
        editor.command_ready = true;
        Self {
            editor,
            counts,
            input_rx,
            output_tx,
        }
    }
}

#[cfg(unix)]
#[test]
fn target_rejection_prevents_spawn_after_symlink_race() {
    use std::os::unix::fs::symlink;

    // Given
    let base = temp_dir("target-race");
    let target_path = base.join("note.md");
    let outside = base.join("outside.md");
    std::fs::write(&target_path, b"inside").expect("write target");
    std::fs::write(&outside, b"outside").expect("write outside");
    let target = ValidatedEditorTarget::from_trusted_path(&target_path).expect("validate target");
    std::fs::remove_file(&target_path).expect("remove target");
    symlink(&outside, &target_path).expect("race target");
    let mut spawn_calls = 0;

    // When
    let result =
        TerminalEditorState::new_with_spawn(target, &base, Rect::new(0, 0, 80, 24), |_, _, _| {
            spawn_calls += 1;
            panic!("rejected target must not reach spawn")
        });

    // Then
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("raced target unexpectedly spawned"),
    };
    let error = error.to_string();
    assert!(!error.contains(&base.to_string_lossy().to_string()));
    assert!(!error.contains(&target_path.to_string_lossy().to_string()));
    assert_eq!(spawn_calls, 0);
    assert_eq!(std::fs::read(&outside).expect("read outside"), b"outside");
    std::fs::remove_dir_all(base).expect("remove fixture");
}

#[test]
fn cancel_preserves_live_child_and_session() {
    // Given
    let mut fixture = Fixture::new(false);
    assert_eq!(
        fixture.editor.request_close(),
        CloseRequest::ConfirmationRequired
    );

    // When
    fixture.editor.cancel_close();

    // Then
    assert!(fixture.editor.has_unsaved_risk());
    assert!(!fixture.editor.close_is_pending());
    assert_eq!(fixture.counts.lock().expect("counts lock").kills, 0);
    assert!(!fixture.input_rx.is_closed());
}

#[test]
fn confirm_closes_master_child_and_channels_exactly_once() {
    // Given
    let mut fixture = Fixture::new(false);
    assert_eq!(
        fixture.editor.request_close(),
        CloseRequest::ConfirmationRequired
    );

    // When
    fixture.editor.confirm_close();
    fixture.editor.confirm_close();

    // Then
    assert!(fixture.editor.is_done());
    assert!(fixture.input_rx.is_closed());
    assert!(fixture.output_tx.is_closed());
    let counts = fixture.counts.lock().expect("counts lock");
    assert_eq!(counts.kills, 1);
    assert_eq!(counts.waits, 1);
    assert_eq!(counts.child_drops, 1);
    assert_eq!(counts.master_drops, 1);
}

#[test]
fn exited_child_closes_without_confirmation() {
    // Given
    let mut fixture = Fixture::new(true);

    // When
    let request = fixture.editor.request_close();

    // Then
    assert_eq!(request, CloseRequest::Closed);
    assert!(!fixture.editor.has_unsaved_risk());
    assert_eq!(fixture.counts.lock().expect("counts lock").kills, 0);
}

#[test]
fn resize_updates_parser_and_pty_dimensions() {
    // Given
    let mut fixture = Fixture::new(false);

    // When
    fixture.editor.resize(Rect::new(5, 7, 132, 41));

    // Then
    assert_eq!(fixture.editor.current_size(), (41, 132));
    let counts = fixture.counts.lock().expect("counts lock");
    assert_eq!(counts.resizes.len(), 1);
    assert_eq!(counts.resizes[0].rows, 41);
    assert_eq!(counts.resizes[0].cols, 132);
}

#[test]
fn channel_close_after_child_exit_is_clean() {
    // Given
    let mut fixture = Fixture::new(true);

    // When
    drop(fixture.output_tx);
    let done = fixture.editor.poll_output();

    // Then
    assert!(done);
    assert!(!fixture.editor.has_unsaved_risk());
}

#[test]
fn repeated_teardown_and_drop_are_idempotent() {
    // Given
    let fixture = Fixture::new(false);
    let counts = Arc::clone(&fixture.counts);
    let mut editor = fixture.editor;

    // When
    editor.confirm_close();
    editor.confirm_close();
    drop(editor);

    // Then
    let counts = counts.lock().expect("counts lock");
    assert_eq!(counts.kills, 1);
    assert_eq!(counts.waits, 1);
    assert_eq!(counts.child_drops, 1);
    assert_eq!(counts.master_drops, 1);
}

fn temp_dir(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "tcui-terminal-editor-{label}-{}-{}",
        std::process::id(),
        rand::random::<u64>()
    ));
    std::fs::create_dir_all(&path).expect("create fixture");
    path
}

#[cfg(unix)]
#[test]
fn approved_scripts_are_private_and_removed_when_the_terminal_closes() {
    use std::os::unix::fs::PermissionsExt;

    let mut fixture = Fixture::new(false);
    fixture
        .editor
        .send_approved_commands("printf secret")
        .expect("approve");
    let path = fixture.editor.approved_scripts[0].path.clone();
    assert_eq!(
        std::fs::metadata(&path)
            .expect("script")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    fixture.editor.confirm_close();
    assert!(
        !path.exists(),
        "closing a terminal must remove its approved script"
    );
}

#[test]
fn full_input_queue_rejects_command_without_disturbing_pending_input() {
    let mut fixture = Fixture::new(false);
    for _ in 0..4 {
        fixture.editor.write_input(b"x").expect("fill queue");
    }
    assert!(matches!(
        fixture.editor.send_approved_commands("never dropped"),
        Err(super::TerminalEditorError::InputQueueFull)
    ));
    for _ in 0..4 {
        assert_eq!(
            fixture
                .input_rx
                .try_recv()
                .expect("retained input")
                .as_ref(),
            b"x"
        );
    }
    assert!(fixture.input_rx.try_recv().is_err());
    assert!(fixture.editor.approved_scripts.is_empty());
}

#[test]
fn closed_input_channel_reports_failure() {
    let mut fixture = Fixture::new(false);
    fixture.input_rx.close();
    assert!(matches!(
        fixture.editor.write_input(b"input"),
        Err(super::TerminalEditorError::NotRunning)
    ));
}

#[test]
fn paste_is_bracketed_when_enabled_and_never_adds_enter() {
    let mut fixture = Fixture::new(false);
    fixture
        .output_tx
        .try_send(b"\x1b[?2004h".to_vec())
        .expect("enable paste");
    fixture.editor.poll_output();
    fixture.editor.paste("line one\nline two").expect("paste");
    assert_eq!(
        fixture.input_rx.try_recv().expect("paste bytes").as_ref(),
        b"\x1b[200~line one\nline two\x1b[201~"
    );
    assert!(fixture.input_rx.try_recv().is_err());
}

#[test]
fn paste_rejects_execution_controls_without_bracketed_support() {
    let mut fixture = Fixture::new(false);
    for text in [
        "echo hidden\n",
        "echo hidden\r",
        "\x1b[201~echo hidden\n",
        "\u{85}",
    ] {
        assert!(matches!(
            fixture.editor.paste(text),
            Err(super::TerminalEditorError::UnsafePaste)
        ));
    }
    assert!(fixture.input_rx.try_recv().is_err());
    fixture.editor.paste("plain text").expect("plain paste");
    assert_eq!(
        fixture.input_rx.try_recv().expect("plain bytes").as_ref(),
        b"plain text"
    );
}

#[test]
fn bracketed_paste_rejects_embedded_terminator() {
    let mut fixture = Fixture::new(false);
    fixture
        .output_tx
        .try_send(b"\x1b[?2004h".to_vec())
        .expect("enable paste");
    fixture.editor.poll_output();
    assert!(matches!(
        fixture.editor.paste("\x1b[201~touch hidden\n"),
        Err(super::TerminalEditorError::UnsafePaste)
    ));
    assert!(fixture.input_rx.try_recv().is_err());
}

#[test]
fn ctrl_c_and_application_cursor_keys_reach_child() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut fixture = Fixture::new(false);
    fixture
        .editor
        .handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert_eq!(
        fixture.input_rx.try_recv().expect("interrupt").as_ref(),
        b"\x03"
    );
    fixture
        .output_tx
        .try_send(b"\x1b[?1h".to_vec())
        .expect("cursor mode");
    fixture.editor.poll_output();
    fixture
        .editor
        .handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(
        fixture.input_rx.try_recv().expect("cursor").as_ref(),
        b"\x1bOA"
    );
    assert!(!fixture.editor.is_done());
}

#[test]
fn child_osc_requests_never_become_rendered_host_escapes_or_input() {
    let mut fixture = Fixture::new(false);
    for bytes in [
        b"before\x1b]52;c;".as_slice(),
        b"Y2xpcGJvYXJk\x07\x1b]8;;https://example.invalid\x1b\\",
        b"link\x1b]8;;\x1b\\\x1b]2;host-title\x07after",
    ] {
        fixture.output_tx.try_send(bytes.to_vec()).expect("output");
        fixture.editor.poll_output();
    }
    let parser = fixture.editor.parser.read().expect("parser");
    assert_eq!(parser.screen().contents(), "beforelinkafter");
    assert!(fixture.input_rx.try_recv().is_err());
}

#[test]
fn scrollback_is_bounded_and_typing_returns_to_live_screen() {
    let mut fixture = Fixture::new(false);
    fixture.editor.resize(Rect::new(0, 0, 40, 2));
    fixture
        .editor
        .parser
        .write()
        .expect("parser")
        .process("line\r\n".repeat(super::SCROLLBACK_LINES + 30).as_bytes());
    fixture.editor.scroll(isize::MAX);
    assert_eq!(
        fixture
            .editor
            .parser
            .read()
            .expect("parser")
            .screen()
            .scrollback(),
        super::SCROLLBACK_LINES
    );
    fixture.editor.write_input(b"x").expect("input");
    assert_eq!(
        fixture
            .editor
            .parser
            .read()
            .expect("parser")
            .screen()
            .scrollback(),
        0
    );
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn real_shell_retains_cwd_resizes_and_executes_only_literal_approved_input() {
    let base = temp_dir("shell-boundary");
    let mut shell =
        TerminalEditorState::new_command_shell("/bin/sh", &base, Rect::new(0, 0, 80, 24))
            .await
            .expect("spawn shell");
    shell.resize(Rect::new(0, 0, 100, 30));
    assert_eq!(shell.current_directory(), Some(base.clone()));
    assert_eq!(shell.shell_program(), Some("/bin/sh"));
    shell.send_approved_commands(
        "printf '%s' '$HOME; literal' > result; pwd > actual-cwd; stty size > dimensions; printf done > ready"
    ).expect("approve command");
    wait_for_file(&mut shell, &base.join("ready")).await;
    assert_eq!(
        std::fs::read_to_string(base.join("result")).expect("literal output"),
        "$HOME; literal"
    );
    assert_eq!(
        std::fs::read_to_string(base.join("actual-cwd"))
            .expect("cwd")
            .trim(),
        base.to_str().expect("fixture path")
    );
    assert_eq!(
        std::fs::read_to_string(base.join("dimensions"))
            .expect("dimensions")
            .trim(),
        "30 100"
    );
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    shell.poll_output();
    assert!(!shell.is_done());
    shell
        .send_approved_commands("mkdir nested; cd nested; printf done > ready")
        .expect("change cwd");
    wait_for_file(&mut shell, &base.join("nested/ready")).await;
    assert_eq!(shell.current_directory(), Some(base.join("nested")));
    shell.confirm_close();
    assert!(shell.is_done());
    std::fs::remove_dir_all(base).expect("cleanup");
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn approved_batch_keeps_interactive_answers_separate_from_later_commands() {
    let base = temp_dir("interactive-batch");
    let mut shell =
        TerminalEditorState::new_command_shell("/bin/sh", &base, Rect::new(0, 0, 80, 24))
            .await
            .expect("spawn shell");
    shell
        .send_approved_commands(
            r#"printf waiting > waiting
IFS= read -r answer
printf '%s' "$answer" > answer
cat > literal <<'TCUI_LITERAL'
$HOME; 'quotes' \ stay literal
TCUI_LITERAL
TCUI_BATCH_STATE='still alive'
printf done > complete
"#,
        )
        .expect("approve batch");
    wait_for_file(&mut shell, &base.join("waiting")).await;
    shell
        .write_input(b"interactive reply\r")
        .expect("answer the interactive prompt");
    wait_for_file(&mut shell, &base.join("complete")).await;
    let answer = std::fs::read_to_string(base.join("answer"));
    let literal = std::fs::read_to_string(base.join("literal"));
    shell
        .write_input(b"printf '%s' \"$TCUI_BATCH_STATE\" > state\r")
        .expect("inspect shell state");
    wait_for_file(&mut shell, &base.join("state")).await;
    let state = std::fs::read_to_string(base.join("state"));
    shell.confirm_close();
    std::fs::remove_dir_all(base).expect("cleanup");
    assert_eq!(
        answer.expect("answer command must not become terminal input"),
        "interactive reply"
    );
    assert_eq!(
        literal.expect("heredoc output"),
        "$HOME; 'quotes' \\ stay literal\n"
    );
    assert_eq!(state.expect("retained shell variable"), "still alive");
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn closing_real_shell_does_not_submit_unexecuted_paste_and_reaps_jobs() {
    let base = temp_dir("shell-close");
    let mut shell =
        TerminalEditorState::new_command_shell("/bin/sh", &base, Rect::new(0, 0, 80, 24))
            .await
            .expect("spawn shell");
    let leader = shell
        .child
        .as_ref()
        .and_then(|child| child.process_id())
        .expect("shell PID");
    shell
        .send_approved_commands("sleep 60 & echo $! > background-pid; printf ready > ready")
        .expect("start background job");
    wait_for_file(&mut shell, &base.join("ready")).await;
    let background: u32 = std::fs::read_to_string(base.join("background-pid"))
        .expect("job PID")
        .trim()
        .parse()
        .expect("numeric job PID");
    shell
        .paste("touch must-not-run")
        .expect("queue unsubmitted text");
    shell.confirm_close();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while process_is_live(leader) || process_is_live(background) {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("process cleanup deadline");
    assert!(!base.join("must-not-run").exists());
    std::fs::remove_dir_all(base).expect("cleanup");
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn invalid_shell_and_missing_working_directory_return_clear_errors() {
    let base = temp_dir("shell-errors");
    assert!(matches!(
        TerminalEditorState::new_shell(Some("/does/not/exist"), &base, Rect::default()).await,
        Err(super::TerminalEditorError::Spawn(_))
    ));
    assert!(matches!(
        TerminalEditorState::new_shell(Some("/bin/sh"), &base.join("missing"), Rect::default())
            .await,
        Err(super::TerminalEditorError::WorkingDirectory(_))
    ));
    std::fs::remove_dir_all(base).expect("cleanup");
}

#[cfg(target_os = "linux")]
async fn wait_for_file(shell: &mut TerminalEditorState, path: &std::path::Path) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while !path.exists() {
            shell.poll_output();
            assert!(!shell.is_done(), "shell exited before output");
            assert!(shell.take_error().is_none(), "terminal transport failed");
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("shell output deadline");
}

#[cfg(target_os = "linux")]
fn process_is_live(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|stat| {
            stat.rsplit_once(") ")
                .map(|(_, state)| !state.starts_with('Z'))
        })
        .unwrap_or(false)
}

#[test]
fn oversized_approved_input_is_rejected_without_partial_write() {
    let mut fixture = Fixture::new(false);
    let command = "x".repeat(super::MAX_INPUT_BYTES);
    assert!(matches!(
        fixture.editor.send_approved_commands(&command),
        Err(super::TerminalEditorError::InputTooLarge)
    ));
    assert!(fixture.input_rx.try_recv().is_err());
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn natural_shell_exit_reports_done_and_reaps_remaining_session_jobs() {
    let base = temp_dir("natural-shell-exit");
    let mut shell =
        TerminalEditorState::new_command_shell("/bin/sh", &base, Rect::new(0, 0, 80, 24))
            .await
            .expect("spawn shell");
    let leader = shell
        .child
        .as_ref()
        .and_then(|child| child.process_id())
        .expect("shell PID");
    shell
        .send_approved_commands("sleep 60 & echo $! > background-pid; printf ready > ready; exit")
        .expect("approved exit");
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while !shell.poll_output() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("exit status deadline");
    let background: u32 = std::fs::read_to_string(base.join("background-pid"))
        .expect("job PID")
        .trim()
        .parse()
        .expect("numeric job PID");
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while process_is_live(leader) || process_is_live(background) {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("natural exit cleanup deadline");
    assert!(!shell.has_unsaved_risk());
    assert!(matches!(
        shell.write_input(b"after exit"),
        Err(super::TerminalEditorError::NotRunning)
    ));
    assert_eq!(shell.request_close(), CloseRequest::Closed);
    std::fs::remove_dir_all(base).expect("cleanup");
}

#[test]
fn child_mouse_reporting_uses_local_sgr_coordinates_and_release_suffix() {
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    let mut fixture = Fixture::new(false);
    let area = Rect::new(10, 5, 40, 20);
    let event = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 12,
        row: 6,
        modifiers: KeyModifiers::NONE,
    };
    assert!(!fixture.editor.handle_mouse(event, area));
    fixture
        .output_tx
        .try_send(b"\x1b[?1000h\x1b[?1006h".to_vec())
        .expect("enable mouse");
    fixture.editor.poll_output();
    assert!(fixture.editor.handle_mouse(event, area));
    assert_eq!(
        fixture.input_rx.try_recv().expect("press").as_ref(),
        b"\x1b[<0;3;2M"
    );
    assert!(fixture.editor.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            ..event
        },
        area
    ));
    assert_eq!(
        fixture.input_rx.try_recv().expect("release").as_ref(),
        b"\x1b[<0;3;2m"
    );
    assert!(fixture.editor.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::ScrollUp,
            modifiers: KeyModifiers::CONTROL,
            ..event
        },
        area
    ));
    assert_eq!(
        fixture.input_rx.try_recv().expect("wheel").as_ref(),
        b"\x1b[<80;3;2M"
    );
    assert!(!fixture
        .editor
        .handle_mouse(MouseEvent { column: 0, ..event }, area));
    assert!(fixture.input_rx.try_recv().is_err());
}

#[test]
fn child_mouse_reporting_obeys_motion_mode_and_legacy_encoding_limits() {
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use tui_term::vt100::{MouseProtocolEncoding, MouseProtocolMode};
    let event = MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Right),
        column: 500,
        row: 300,
        modifiers: KeyModifiers::SHIFT,
    };
    let area = Rect::new(0, 0, 600, 400);
    assert_eq!(
        super::input::mouse_to_bytes(
            event,
            area,
            MouseProtocolMode::PressRelease,
            MouseProtocolEncoding::Default
        ),
        Some(Vec::new())
    );
    assert_eq!(
        super::input::mouse_to_bytes(
            event,
            area,
            MouseProtocolMode::ButtonMotion,
            MouseProtocolEncoding::Default
        ),
        Some(vec![0x1b, b'[', b'M', 70, 255, 255])
    );
    assert_eq!(
        super::input::mouse_to_bytes(
            event,
            area,
            MouseProtocolMode::ButtonMotion,
            MouseProtocolEncoding::Sgr
        ),
        Some(b"\x1b[<38;501;301M".to_vec())
    );
    let utf8 = super::input::mouse_to_bytes(
        event,
        area,
        MouseProtocolMode::ButtonMotion,
        MouseProtocolEncoding::Utf8,
    )
    .expect("UTF-8 mouse");
    assert_eq!(
        String::from_utf8(utf8).expect("valid UTF-8"),
        format!(
            "\x1b[M{}{}{}",
            char::from_u32(70).expect("button"),
            char::from_u32(533).expect("x"),
            char::from_u32(333).expect("y")
        )
    );
}

#[test]
fn approved_commands_reject_sessions_without_confirmed_clean_startup() {
    let mut fixture = Fixture::new(false);
    fixture.editor.command_ready = false;
    assert!(!fixture.editor.command_session_ready());
    assert!(matches!(
        fixture.editor.send_approved_commands("echo must-not-send"),
        Err(super::TerminalEditorError::CommandShellNotReady)
    ));
    assert!(fixture.input_rx.try_recv().is_err());
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn command_shell_startup_read_times_out_without_receiving_input_and_cleans_up() {
    use std::os::unix::fs::PermissionsExt;
    let base = temp_dir("startup-read");
    let executable = base.join("sh");
    std::fs::write(&executable,
        "#!/bin/sh\nprintf '%s' \"$$\" > startup-pid\nread -r input\nprintf '%s' \"$input\" > stolen\n")
        .expect("startup reader");
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
        .expect("executable");
    let result = TerminalEditorState::new_command_shell(
        executable.to_str().expect("path"),
        &base,
        Rect::new(0, 0, 80, 24),
    )
    .await;
    assert!(matches!(
        result,
        Err(super::TerminalEditorError::CommandShellStartup(_))
    ));
    assert!(!base.join("stolen").exists());
    let pid: u32 = std::fs::read_to_string(base.join("startup-pid"))
        .expect("started child")
        .parse()
        .expect("child PID");
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while process_is_live(pid) {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("startup cancellation cleanup");
    std::fs::remove_dir_all(base).expect("cleanup");
}
