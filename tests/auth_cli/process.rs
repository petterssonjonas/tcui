/// RAII guard that kills and reaps a child process on drop.
///
/// Ensures no child process leaks when a test panics or returns early before
/// the explicit `wait_with_output` / `wait` call.
pub(super) struct ProcessGuard {
    child: Option<std::process::Child>,
}

impl ProcessGuard {
    pub(super) fn new(child: std::process::Child) -> Self {
        Self { child: Some(child) }
    }

    pub(super) fn id(&self) -> u32 {
        self.child
            .as_ref()
            .map(|child| child.id())
            .expect("child already consumed")
    }

    pub(super) fn take_stdin(&mut self) -> std::io::Result<std::process::ChildStdin> {
        self.child
            .as_mut()
            .and_then(|child| child.stdin.take())
            .ok_or_else(|| std::io::Error::other("child stdin unavailable"))
    }

    pub(super) fn take_stdout(&mut self) -> std::io::Result<std::process::ChildStdout> {
        self.child
            .as_mut()
            .and_then(|child| child.stdout.take())
            .ok_or_else(|| std::io::Error::other("child stdout unavailable"))
    }

    pub(super) fn wait(mut self) -> std::io::Result<std::process::ExitStatus> {
        let mut child = self.child.take().expect("child already consumed");
        child.wait()
    }

    pub(super) fn wait_with_output(mut self) -> std::io::Result<std::process::Output> {
        let child = self.child.take().expect("child already consumed");
        child.wait_with_output()
    }
}

impl Drop for ProcessGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
