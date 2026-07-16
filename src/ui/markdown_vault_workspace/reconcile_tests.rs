use std::path::{Path, PathBuf};

use crate::vault::{VaultEntry, VaultEntryKind};

use super::{PendingTreeRescan, FULL_RESCAN_ENTRIES_PER_TICK};

#[test]
fn vault_watcher_full_rescan_processes_at_most_one_thousand_entries_per_tick() {
    let entries = (0..2_501)
        .map(|index| VaultEntry {
            path: PathBuf::from(format!("note-{index:04}.md")),
            kind: VaultEntryKind::File,
        })
        .collect::<Vec<_>>();
    let mut pending = PendingTreeRescan {
        remaining: entries.into_iter(),
        staged: Vec::new(),
        overflow_generation: None,
    };
    let mut processed_per_tick = Vec::new();

    while !pending.is_complete() {
        let before = pending.staged.len();
        pending.advance(Path::new("/vault"));
        processed_per_tick.push(pending.staged.len().saturating_sub(before));
    }

    assert_eq!(processed_per_tick, vec![1_000, 1_000, 501]);
    assert_eq!(pending.staged.len(), 2_501);
    assert_eq!(processed_per_tick.len(), 3);
    assert!(processed_per_tick
        .iter()
        .all(|processed| *processed <= FULL_RESCAN_ENTRIES_PER_TICK));
}

#[test]
fn vault_watcher_full_rescan_finishes_fifty_thousand_entries_within_bound() {
    let entries = (0..50_000)
        .map(|index| VaultEntry {
            path: PathBuf::from(format!("note-{index:05}.md")),
            kind: VaultEntryKind::File,
        })
        .collect::<Vec<_>>();
    let mut pending = PendingTreeRescan {
        remaining: entries.into_iter(),
        staged: Vec::new(),
        overflow_generation: None,
    };
    let mut ticks = 0_usize;

    while !pending.is_complete() {
        pending.advance(Path::new("/vault"));
        ticks = ticks.saturating_add(1);
    }

    assert_eq!(ticks, 50);
    assert!(ticks <= 50_000_usize.div_ceil(FULL_RESCAN_ENTRIES_PER_TICK) + 2);
}
