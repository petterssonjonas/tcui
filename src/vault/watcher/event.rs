use std::path::{Path, PathBuf};

use notify::event::{ModifyKind, RenameMode};
use notify::{Event, EventKind};

use crate::ui::app_tabs::AppTabId;

use super::batch::{VaultWatchChange, VaultWatchEvent};

pub(super) fn normalize_event(
    root: &Path,
    tab_id: AppTabId,
    event: Event,
) -> Option<VaultWatchEvent> {
    let (change, ambiguous) = match event.kind {
        EventKind::Create(_) => (VaultWatchChange::Create, false),
        EventKind::Remove(_) => (VaultWatchChange::Remove, false),
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) if event.paths.len() == 2 => {
            (VaultWatchChange::Rename, false)
        }
        EventKind::Modify(ModifyKind::Name(_)) => (VaultWatchChange::Rename, true),
        EventKind::Modify(_) => (VaultWatchChange::Modify, false),
        EventKind::Access(_) => return None,
        EventKind::Any | EventKind::Other => (VaultWatchChange::Modify, true),
    };
    let paths = event
        .paths
        .into_iter()
        .filter_map(|path| relative_event_path(root, path))
        .filter(|path| !is_tcui_operation_path(path))
        .collect::<Vec<_>>();
    (!paths.is_empty()).then_some(VaultWatchEvent {
        tab_id,
        change,
        paths,
        ambiguous,
    })
}

fn relative_event_path(root: &Path, path: PathBuf) -> Option<PathBuf> {
    if root.as_os_str().is_empty() {
        return Some(path);
    }
    path.strip_prefix(root).ok().map(Path::to_path_buf)
}

pub(crate) fn is_tcui_operation_path(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    matches_generated_id(name, ".tcui-write-", ".tmp")
        || matches_generated_id(name, ".tcui-remove-", ".tmp")
        || matches_generated_id(name, ".tcui-recover-", ".tmp")
        || matches_move_stage(name)
}

fn matches_generated_id(name: &str, prefix: &str, suffix: &str) -> bool {
    name.strip_prefix(prefix)
        .and_then(|rest| rest.strip_suffix(suffix))
        .is_some_and(is_lower_hex_id)
}

fn matches_move_stage(name: &str) -> bool {
    let Some(rest) = name.strip_prefix(".tcui-move-") else {
        return false;
    };
    let Some((id, stage)) = rest.split_once('-') else {
        return false;
    };
    is_lower_hex_id(id)
        && stage.strip_suffix(".stage").is_some_and(|index| {
            index.len() == 3 && index.bytes().all(|byte| byte.is_ascii_digit())
        })
}

fn is_lower_hex_id(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
