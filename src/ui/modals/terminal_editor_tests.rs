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
        };
        Self {
            editor: TerminalEditorState::from_spawned(session, Rect::new(0, 0, 80, 24)),
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
