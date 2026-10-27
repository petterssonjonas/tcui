use std::io::Read;
use std::process::{Child, Command, Output, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use super::ReminderError;
use crate::config::AppConfig;

#[derive(Default)]
pub(super) struct Cancellation {
    cancelled: Mutex<bool>,
    deadline: Option<Instant>,
}

impl Cancellation {
    pub(super) fn cleanup() -> Self {
        Self {
            cancelled: Mutex::new(false),
            deadline: Some(Instant::now() + Duration::from_secs(5)),
        }
    }

    pub(super) fn check(&self) -> Result<(), ReminderError> {
        let cancelled = *self
            .cancelled
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.check_state(cancelled)
    }

    fn check_state(&self, cancelled: bool) -> Result<(), ReminderError> {
        if cancelled {
            Err(ReminderError::Cancelled)
        } else if self
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            Err(ReminderError::Schedule(
                "reminder cleanup timed out".to_owned(),
            ))
        } else {
            Ok(())
        }
    }

    fn cancel(&self) {
        *self
            .cancelled
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
    }

    fn spawn(&self, command: &mut Command) -> Result<Child, ReminderError> {
        // Serialize spawning against request cancellation. Once cancellation wins,
        // no later scheduler command can start (rollback uses its own context).
        let cancelled = self
            .cancelled
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.check_state(*cancelled)?;
        Ok(command.spawn()?)
    }
}

#[derive(Default)]
struct RequestState {
    cancellation: Cancellation,
    registration: Mutex<Option<super::Registration>>,
}

impl RequestState {
    fn take_registration(&self) -> Option<super::Registration> {
        self.registration
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }
}

pub(super) struct RequestOwner {
    state: Arc<RequestState>,
    observed: Option<mpsc::Sender<()>>,
}

impl RequestOwner {
    fn observe(mut self) {
        if let Some(registration) = self.state.take_registration() {
            // Release the job lock before returning success to the host loop.
            registration.commit();
        }
        if let Some(observed) = self.observed.take() {
            let _ = observed.send(());
        }
    }
}

impl Drop for RequestOwner {
    fn drop(&mut self) {
        if self.observed.is_some() {
            self.state.cancellation.cancel();
        }
        // Dropping the acknowledgement sender also wakes a completed worker
        // whose registration has not been observed by the requesting future.
    }
}

pub(super) fn start(
    config: AppConfig,
    arguments: serde_json::Value,
) -> (
    RequestOwner,
    tokio::sync::oneshot::Receiver<Result<String, ReminderError>>,
    tokio::task::JoinHandle<()>,
) {
    let state = Arc::new(RequestState::default());
    let (result, receiver) = tokio::sync::oneshot::channel();
    let (observed, acknowledgement) = mpsc::channel();
    let worker_state = Arc::clone(&state);
    let worker = tokio::task::spawn_blocking(move || {
        match super::agent_tool_inner(&config, &arguments, &worker_state.cancellation) {
            Ok(completed) => {
                // Keep the private job guard in this worker until the caller
                // actually observes success. A dropped receiver or missing
                // acknowledgement drops the registration and rolls it back.
                let super::Completed {
                    receipt,
                    registration,
                } = completed;
                *worker_state
                    .registration
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = registration;
                if result.send(Ok(receipt)).is_err() || acknowledgement.recv().is_err() {
                    drop(worker_state.take_registration());
                }
            }
            Err(error) => {
                let _ = result.send(Err(error));
            }
        }
    });
    (
        RequestOwner {
            state,
            observed: Some(observed),
        },
        receiver,
        worker,
    )
}

pub(super) async fn run(
    config: AppConfig,
    arguments: serde_json::Value,
) -> Result<String, ReminderError> {
    let (owner, result, _worker) = start(config, arguments);
    let result = result.await.map_err(|_| {
        ReminderError::Schedule("reminder worker stopped before returning a result".to_owned())
    })?;
    // No suspension between receiving the receipt and acknowledging ownership.
    owner.observe();
    result
}

/// Run a native scheduler child on the blocking worker, retaining its process
/// group identity until every descendant has been stopped and the leader reaped.
pub(super) fn output(
    command: &mut Command,
    cancellation: &Cancellation,
) -> Result<Output, ReminderError> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut process = SchedulerProcess::new(cancellation.spawn(command)?)?;
    let mut stdout = process
        .child
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("scheduler stdout is unavailable"))?;
    let mut stderr = process
        .child
        .stderr
        .take()
        .ok_or_else(|| std::io::Error::other("scheduler stderr is unavailable"))?;
    std::thread::scope(move |scope| {
        // Keep ownership inside the scope so unwinding stops the child before
        // scoped reader threads are joined.
        let mut process = process;
        // Drain both pipes while waiting; verbose scheduler errors must not
        // deadlock cancellation behind a full pipe.
        let stdout = scope.spawn(move || {
            let mut bytes = Vec::new();
            stdout.read_to_end(&mut bytes).map(|_| bytes)
        });
        let stderr = scope.spawn(move || {
            let mut bytes = Vec::new();
            stderr.read_to_end(&mut bytes).map(|_| bytes)
        });
        let waited = process.wait(cancellation);
        let status = process.finish();
        let stdout = stdout
            .join()
            .map_err(|_| std::io::Error::other("scheduler stdout reader failed"))?;
        let stderr = stderr
            .join()
            .map_err(|_| std::io::Error::other("scheduler stderr reader failed"))?;
        waited?;
        cancellation.check()?;
        Ok(Output {
            status: status?,
            stdout: stdout?,
            stderr: stderr?,
        })
    })
}

struct SchedulerProcess {
    child: Child,
    #[cfg(unix)]
    group: Option<rustix::process::Pid>,
    reaped: bool,
}

impl SchedulerProcess {
    fn new(mut child: Child) -> std::io::Result<Self> {
        #[cfg(unix)]
        let group = match i32::try_from(child.id())
            .ok()
            .and_then(rustix::process::Pid::from_raw)
        {
            Some(group) => group,
            None => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(std::io::Error::other("scheduler process ID is unavailable"));
            }
        };
        Ok(Self {
            #[cfg(unix)]
            group: Some(group),
            child,
            reaped: false,
        })
    }

    fn wait(&mut self, cancellation: &Cancellation) -> Result<(), ReminderError> {
        loop {
            cancellation.check()?;
            #[cfg(unix)]
            {
                use rustix::process::{waitid, WaitId, WaitIdOptions};
                let group = self.group.ok_or_else(|| {
                    std::io::Error::other("scheduler process group is unavailable")
                })?;
                match waitid(
                    WaitId::Pid(group),
                    WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
                ) {
                    Ok(Some(_)) => return Ok(()),
                    Ok(None) | Err(rustix::io::Errno::INTR) => {}
                    Err(rustix::io::Errno::CHILD) => {
                        self.group = None;
                        return Err(std::io::Error::other(
                            "scheduler child was unexpectedly reaped",
                        )
                        .into());
                    }
                    Err(error) => return Err(std::io::Error::from(error).into()),
                }
            }
            #[cfg(not(unix))]
            if self.child.try_wait()?.is_some() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn finish(&mut self) -> std::io::Result<std::process::ExitStatus> {
        #[cfg(unix)]
        if let Some(group) = self.group.take() {
            match rustix::process::kill_process_group(group, rustix::process::Signal::KILL) {
                Ok(()) | Err(rustix::io::Errno::SRCH) => {}
                Err(error) => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    self.reaped = true;
                    return Err(error.into());
                }
            }
        }
        #[cfg(not(unix))]
        if self.child.try_wait()?.is_none() {
            self.child.kill()?;
        }
        let status = self.child.wait()?;
        self.reaped = true;
        Ok(status)
    }
}

impl Drop for SchedulerProcess {
    fn drop(&mut self) {
        if !self.reaped {
            let _ = self.finish();
        }
    }
}
