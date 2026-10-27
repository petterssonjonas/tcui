use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use notify::event::{CreateKind, ModifyKind, RemoveKind, RenameMode};
use notify::{Event, EventKind};

use super::{
    is_tcui_operation_path, VaultWatchChange, VaultWatchEvent, VaultWatcher, WATCHER_DRAIN_LIMIT,
    WATCHER_QUEUE_CAPACITY,
};
use crate::ui::app_tabs::AppTabs;

fn tab_id() -> crate::ui::app_tabs::AppTabId {
    AppTabs::default().add_launcher()
}

fn event(kind: EventKind, paths: &[&str]) -> Event {
    paths.iter().fold(Event::new(kind), |event, path| {
        event.add_path(PathBuf::from(*path))
    })
}

#[test]
fn vault_watcher_queue_overflow_is_bounded_and_requests_full_rescan() {
    let mut watcher = VaultWatcher::detached_for_test(tab_id());
    for index in 0..=WATCHER_QUEUE_CAPACITY {
        watcher.enqueue_for_test(VaultWatchEvent::for_test(
            watcher.tab_id(),
            VaultWatchChange::Modify,
            PathBuf::from(format!("note-{index}.md")),
        ));
    }

    let batch = watcher.drain();

    assert_eq!(batch.event_count(), WATCHER_DRAIN_LIMIT);
    assert_eq!(batch.paths().len(), WATCHER_DRAIN_LIMIT);
    assert!(batch.requires_full_rescan());
    assert!(watcher.overflowed());
    watcher.clear_overflow_after_success(batch.overflow_generation());
    assert!(!watcher.overflowed());
}

#[test]
fn vault_watcher_newer_overflow_survives_older_rescan_completion() {
    let mut watcher = VaultWatcher::detached_for_test(tab_id());
    for index in 0..=WATCHER_QUEUE_CAPACITY {
        watcher.enqueue_for_test(VaultWatchEvent::for_test(
            watcher.tab_id(),
            VaultWatchChange::Modify,
            PathBuf::from(format!("first-{index}.md")),
        ));
    }
    let first = watcher.drain();
    for index in 0..=WATCHER_QUEUE_CAPACITY {
        watcher.enqueue_for_test(VaultWatchEvent::for_test(
            watcher.tab_id(),
            VaultWatchChange::Modify,
            PathBuf::from(format!("second-{index}.md")),
        ));
    }

    watcher.clear_overflow_after_success(first.overflow_generation());

    assert!(watcher.overflowed());
}

#[test]
fn vault_watcher_drain_coalesces_paths_and_maps_event_kinds() {
    let mut watcher = VaultWatcher::detached_for_test(tab_id());
    for raw in [
        event(EventKind::Create(CreateKind::File), &["a.md"]),
        event(
            EventKind::Modify(ModifyKind::Data(notify::event::DataChange::Any)),
            &["a.md"],
        ),
        event(EventKind::Remove(RemoveKind::File), &["b.md"]),
        event(
            EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
            &["old.md", "new.md"],
        ),
    ] {
        watcher.normalize_and_enqueue_for_test(raw);
    }

    let batch = watcher.drain();

    assert_eq!(batch.event_count(), 4);
    assert_eq!(
        batch.change_for(Path::new("a.md")),
        Some(VaultWatchChange::Create)
    );
    assert!(batch.is_modified(Path::new("a.md")));
    assert_eq!(
        batch.change_for(Path::new("b.md")),
        Some(VaultWatchChange::Remove)
    );
    assert_eq!(
        batch.change_for(Path::new("old.md")),
        Some(VaultWatchChange::Rename)
    );
    assert_eq!(
        batch.change_for(Path::new("new.md")),
        Some(VaultWatchChange::Rename)
    );
    assert!(batch.requires_tree_rescan());
    assert!(!batch.requires_full_rescan());
}

#[test]
fn vault_watcher_ambiguous_rename_requests_full_rescan() {
    let mut watcher = VaultWatcher::detached_for_test(tab_id());
    watcher.normalize_and_enqueue_for_test(event(
        EventKind::Modify(ModifyKind::Name(RenameMode::From)),
        &["old.md"],
    ));

    assert!(watcher.drain().requires_full_rescan());
}

#[test]
fn vault_watcher_ignores_only_exact_tcui_operation_ids() {
    for ignored in [
        ".tcui-write-0123456789abcdef0123456789abcdef.tmp",
        ".tcui-remove-0123456789abcdef0123456789abcdef.tmp",
        ".tcui-recover-0123456789abcdef0123456789abcdef.tmp",
        ".tcui-move-0123456789abcdef0123456789abcdef-003.stage",
    ] {
        assert!(is_tcui_operation_path(Path::new(ignored)), "{ignored}");
    }
    for visible in [
        "draft.tmp",
        "notes.md~",
        ".note.md.swp",
        ".tcui-write-user.tmp",
        ".tcui-move-not-an-id-003.stage",
    ] {
        assert!(!is_tcui_operation_path(Path::new(visible)), "{visible}");
    }
}

#[test]
fn vault_watcher_native_notify_reports_without_fixed_sleep() {
    let root = std::env::temp_dir().join(format!(
        "tcui-vault-watcher-native-{}-{}",
        std::process::id(),
        rand::random::<u64>()
    ));
    std::fs::create_dir_all(&root).expect("create native watcher root");
    let mut watcher = VaultWatcher::new(&root, tab_id()).expect("start native watcher");
    std::fs::write(root.join("native.md"), "native").expect("write watched file");
    let started = Instant::now();
    let mut attempts = 0_u64;
    let observed = loop {
        attempts = attempts.saturating_add(1);
        if watcher.drain().paths().contains_key(Path::new("native.md")) {
            break true;
        }
        if started.elapsed() >= Duration::from_secs(2) {
            break false;
        }
        std::thread::yield_now();
    };
    let elapsed = started.elapsed();
    let _ = std::fs::remove_dir_all(&root);

    assert!(
        observed,
        "native notify event missing after {attempts} attempts in {elapsed:?}"
    );
    println!("native notify observed after {attempts} attempts in {elapsed:?}");
}

#[test]
fn vault_watcher_drop_unwatches_exactly_once() {
    let drops = Arc::new(AtomicUsize::new(0));
    let watcher = VaultWatcher::detached_with_drop_probe_for_test(tab_id(), Arc::clone(&drops));

    drop(watcher);

    assert_eq!(drops.load(Ordering::SeqCst), 1);
}
