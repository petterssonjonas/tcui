use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TryRecvError, TrySendError};
use std::sync::Arc;

use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};

use crate::ui::app_tabs::AppTabId;

mod batch;
mod event;

use batch::stronger_change;
pub(crate) use batch::{VaultWatchBatch, VaultWatchChange, VaultWatchEvent};
#[cfg(test)]
pub(crate) use event::is_tcui_operation_path;
use event::normalize_event;

pub(crate) const WATCHER_QUEUE_CAPACITY: usize = 256;
pub(crate) const WATCHER_DRAIN_LIMIT: usize = 256;

#[derive(Debug, thiserror::Error)]
pub(crate) enum VaultWatcherError {
    #[error("failed to watch vault path {path}: {source}")]
    Notify {
        path: PathBuf,
        #[source]
        source: notify::Error,
    },
}

pub(crate) struct VaultWatcher {
    watcher: Option<RecommendedWatcher>,
    root: PathBuf,
    #[cfg(test)]
    tab_id: AppTabId,
    #[cfg(test)]
    sender: SyncSender<VaultWatchEvent>,
    receiver: Receiver<VaultWatchEvent>,
    overflow: Arc<AtomicBool>,
    overflow_generation: Arc<AtomicU64>,
    #[cfg(test)]
    drop_probe: Option<Arc<std::sync::atomic::AtomicUsize>>,
}

impl VaultWatcher {
    pub(crate) fn new(root: &Path, tab_id: AppTabId) -> Result<Self, VaultWatcherError> {
        let (sender, receiver) = sync_channel(WATCHER_QUEUE_CAPACITY);
        let overflow = Arc::new(AtomicBool::new(false));
        let generation = Arc::new(AtomicU64::new(0));
        let callback_root = root.to_path_buf();
        let callback_sender = sender.clone();
        let callback_overflow = Arc::clone(&overflow);
        let callback_generation = Arc::clone(&generation);
        let mut watcher = RecommendedWatcher::new(
            move |result| match result {
                Ok(event) => enqueue_normalized(
                    &callback_root,
                    tab_id,
                    event,
                    &callback_sender,
                    &callback_overflow,
                    &callback_generation,
                ),
                Err(_) => mark_overflow(&callback_overflow, &callback_generation),
            },
            Config::default(),
        )
        .map_err(|source| VaultWatcherError::Notify {
            path: root.to_path_buf(),
            source,
        })?;
        watcher
            .watch(root, RecursiveMode::Recursive)
            .map_err(|source| VaultWatcherError::Notify {
                path: root.to_path_buf(),
                source,
            })?;
        Ok(Self {
            watcher: Some(watcher),
            root: root.to_path_buf(),
            #[cfg(test)]
            tab_id,
            #[cfg(test)]
            sender,
            receiver,
            overflow,
            overflow_generation: generation,
            #[cfg(test)]
            drop_probe: None,
        })
    }

    pub(crate) fn drain(&mut self) -> VaultWatchBatch {
        let mut batch = VaultWatchBatch {
            full_rescan: self.overflow.load(Ordering::Acquire),
            overflow_generation: self.overflow_generation.load(Ordering::Acquire),
            ..VaultWatchBatch::default()
        };
        for _ in 0..WATCHER_DRAIN_LIMIT {
            let event = match self.receiver.try_recv() {
                Ok(event) => event,
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            };
            batch.event_count = batch.event_count.saturating_add(1);
            batch.tab_id.get_or_insert(event.tab_id);
            batch.full_rescan |= event.ambiguous || batch.tab_id != Some(event.tab_id);
            for path in event.paths {
                if event.change == VaultWatchChange::Modify {
                    batch.modified.insert(path.clone());
                }
                batch
                    .changes
                    .entry(path)
                    .and_modify(|current| *current = stronger_change(*current, event.change))
                    .or_insert(event.change);
            }
        }
        batch
    }

    pub(crate) fn clear_overflow_after_success(&self, observed_generation: u64) {
        if self.overflow_generation.load(Ordering::Acquire) == observed_generation {
            self.overflow.store(false, Ordering::Release);
        }
    }

    #[cfg(test)]
    pub(crate) const fn tab_id(&self) -> AppTabId {
        self.tab_id
    }

    #[cfg(test)]
    pub(crate) fn overflowed(&self) -> bool {
        self.overflow.load(Ordering::Acquire)
    }

    #[cfg(test)]
    pub(crate) fn force_overflow_for_test(&self) {
        mark_overflow(&self.overflow, &self.overflow_generation);
    }

    #[cfg(test)]
    pub(crate) fn enqueue_for_test(&mut self, event: VaultWatchEvent) {
        try_enqueue(
            &self.sender,
            event,
            &self.overflow,
            &self.overflow_generation,
        );
    }

    #[cfg(test)]
    pub(crate) fn normalize_and_enqueue_for_test(&mut self, event: Event) {
        enqueue_normalized(
            Path::new(""),
            self.tab_id,
            event,
            &self.sender,
            &self.overflow,
            &self.overflow_generation,
        );
    }

    #[cfg(test)]
    pub(crate) fn detached_for_test(tab_id: AppTabId) -> Self {
        let (sender, receiver) = sync_channel(WATCHER_QUEUE_CAPACITY);
        Self {
            watcher: None,
            root: PathBuf::new(),
            tab_id,
            sender,
            receiver,
            overflow: Arc::new(AtomicBool::new(false)),
            overflow_generation: Arc::new(AtomicU64::new(0)),
            drop_probe: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn detached_with_drop_probe_for_test(
        tab_id: AppTabId,
        probe: Arc<std::sync::atomic::AtomicUsize>,
    ) -> Self {
        let mut watcher = Self::detached_for_test(tab_id);
        watcher.drop_probe = Some(probe);
        watcher
    }

    #[cfg(test)]
    pub(crate) fn set_drop_probe_for_test(&mut self, probe: Arc<std::sync::atomic::AtomicUsize>) {
        self.drop_probe = Some(probe);
    }
}

impl Drop for VaultWatcher {
    fn drop(&mut self) {
        if let Some(watcher) = self.watcher.as_mut() {
            let _ = watcher.unwatch(&self.root);
        }
        #[cfg(test)]
        if let Some(probe) = &self.drop_probe {
            probe.fetch_add(1, Ordering::SeqCst);
        }
    }
}

fn enqueue_normalized(
    root: &Path,
    tab_id: AppTabId,
    event: Event,
    sender: &SyncSender<VaultWatchEvent>,
    overflow: &AtomicBool,
    generation: &AtomicU64,
) {
    let Some(event) = normalize_event(root, tab_id, event) else {
        return;
    };
    try_enqueue(sender, event, overflow, generation);
}

fn try_enqueue(
    sender: &SyncSender<VaultWatchEvent>,
    event: VaultWatchEvent,
    overflow: &AtomicBool,
    generation: &AtomicU64,
) {
    if matches!(
        sender.try_send(event),
        Err(TrySendError::Full(_) | TrySendError::Disconnected(_))
    ) {
        mark_overflow(overflow, generation);
    }
}

fn mark_overflow(overflow: &AtomicBool, generation: &AtomicU64) {
    generation.fetch_add(1, Ordering::AcqRel);
    overflow.store(true, Ordering::Release);
}

#[cfg(test)]
#[path = "watcher_tests.rs"]
mod tests;
