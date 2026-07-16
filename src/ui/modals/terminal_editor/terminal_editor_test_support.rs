use std::io;
use std::sync::{Arc, Mutex};

use portable_pty::PtySize;
use ratatui::layout::Rect;

use super::{SessionChild, SessionMaster, SpawnedSession, TerminalEditorState};

#[derive(Debug, Default)]
struct SessionCounts {
    kills: usize,
    waits: usize,
    child_drops: usize,
    master_drops: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct TerminalEditorTestProbe(Arc<Mutex<SessionCounts>>);

impl TerminalEditorTestProbe {
    pub(crate) fn kills(&self) -> usize {
        self.0.lock().expect("test probe lock").kills
    }

    pub(crate) fn waits(&self) -> usize {
        self.0.lock().expect("test probe lock").waits
    }

    pub(crate) fn child_drops(&self) -> usize {
        self.0.lock().expect("test probe lock").child_drops
    }

    pub(crate) fn master_drops(&self) -> usize {
        self.0.lock().expect("test probe lock").master_drops
    }
}

#[derive(Debug)]
struct FakeChild {
    counts: Arc<Mutex<SessionCounts>>,
    exited: bool,
}

impl SessionChild for FakeChild {
    fn terminate(&mut self) -> io::Result<()> {
        self.counts.lock().expect("test probe lock").kills += 1;
        self.exited = true;
        Ok(())
    }

    fn has_exited(&mut self) -> io::Result<bool> {
        Ok(self.exited)
    }

    fn wait(&mut self) -> io::Result<()> {
        self.counts.lock().expect("test probe lock").waits += 1;
        self.exited = true;
        Ok(())
    }
}

impl Drop for FakeChild {
    fn drop(&mut self) {
        self.counts.lock().expect("test probe lock").child_drops += 1;
    }
}

#[derive(Debug)]
struct FakeMaster(Arc<Mutex<SessionCounts>>);

impl SessionMaster for FakeMaster {
    fn resize(&self, _size: PtySize) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for FakeMaster {
    fn drop(&mut self) {
        self.0.lock().expect("test probe lock").master_drops += 1;
    }
}

pub(crate) fn live_editor() -> (TerminalEditorState, TerminalEditorTestProbe) {
    let counts = Arc::new(Mutex::new(SessionCounts::default()));
    let (input_tx, _input_rx) = tokio::sync::mpsc::channel(4);
    let (_output_tx, output_rx) = tokio::sync::mpsc::channel(4);
    let session = SpawnedSession {
        child: Box::new(FakeChild {
            counts: Arc::clone(&counts),
            exited: false,
        }),
        master: Box::new(FakeMaster(Arc::clone(&counts))),
        input_tx,
        output_rx,
    };
    (
        TerminalEditorState::from_spawned(session, Rect::new(0, 0, 80, 24)),
        TerminalEditorTestProbe(counts),
    )
}
