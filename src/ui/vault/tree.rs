use std::collections::HashSet;
use std::path::{Path, PathBuf};

use ratatui::{prelude::*, widgets::Paragraph, Frame};

use crate::ui::artifact_sidebar::{display_label, ArtifactKind};
use crate::vault::VaultEntryKind;

use super::hit_areas::{VaultHitAreas, VaultMouseAction};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TreeSourceEntry {
    pub(crate) path: PathBuf,
    pub(crate) entry_kind: VaultEntryKind,
    pub(crate) artifact_kind: Option<ArtifactKind>,
}

impl TreeSourceEntry {
    pub(crate) const fn new(
        path: PathBuf,
        entry_kind: VaultEntryKind,
        artifact_kind: Option<ArtifactKind>,
    ) -> Self {
        Self {
            path,
            entry_kind,
            artifact_kind,
        }
    }

    pub(crate) const fn file(path: PathBuf, artifact_kind: ArtifactKind) -> Self {
        Self::new(path, VaultEntryKind::File, Some(artifact_kind))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VaultTreeNode {
    pub(crate) path: PathBuf,
    pub(crate) depth: usize,
    pub(crate) entry_kind: VaultEntryKind,
    pub(crate) artifact_kind: Option<ArtifactKind>,
    pub(crate) label: String,
    pub(crate) expanded: bool,
}

impl VaultTreeNode {
    pub(crate) const fn is_directory(&self) -> bool {
        matches!(self.entry_kind, VaultEntryKind::Directory)
    }

    pub(crate) const fn is_editable(&self) -> bool {
        matches!(
            (self.entry_kind, self.artifact_kind),
            (
                VaultEntryKind::File,
                Some(ArtifactKind::Markdown | ArtifactKind::Text)
            )
        )
    }

    pub(crate) fn metadata(&self) -> String {
        match (self.entry_kind, self.artifact_kind) {
            (VaultEntryKind::Directory, _) => "Directory".to_string(),
            (VaultEntryKind::File, Some(kind)) => format!("{kind:?} regular file"),
            (VaultEntryKind::File, None) => "Regular file".to_string(),
            (VaultEntryKind::UnsupportedSymlink, _) => "Unsupported symbolic link".to_string(),
            (VaultEntryKind::Unsupported, _) => "Unsupported filesystem entry".to_string(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct VaultTreeModel {
    sources: Vec<TreeSourceEntry>,
    collapsed: HashSet<PathBuf>,
    nodes: Vec<VaultTreeNode>,
    selected: usize,
    pub(crate) scroll: usize,
}

impl VaultTreeModel {
    pub(crate) fn new(sources: Vec<TreeSourceEntry>) -> Self {
        let nodes = build_tree_nodes(&sources, &HashSet::new());
        Self {
            sources,
            nodes,
            ..Self::default()
        }
    }

    pub(crate) fn nodes(&self) -> &[VaultTreeNode] {
        &self.nodes
    }

    pub(crate) fn selected_node(&self) -> Option<&VaultTreeNode> {
        self.nodes.get(self.selected)
    }

    pub(crate) fn selected_path(&self) -> Option<&Path> {
        self.selected_node().map(|node| node.path.as_path())
    }

    pub(crate) fn node_for_path(&self, path: &Path) -> Option<&VaultTreeNode> {
        self.nodes.iter().find(|node| node.path == path)
    }

    pub(crate) fn entry_kind(&self, path: &Path) -> Option<VaultEntryKind> {
        self.node_for_path(path).map(|node| node.entry_kind)
    }

    pub(crate) fn replace_sources(&mut self, sources: Vec<TreeSourceEntry>) {
        let selected_path = self.selected_path().map(Path::to_path_buf);
        let selected_index = self.selected;
        self.sources = sources;
        self.nodes = build_tree_nodes(&self.sources, &self.collapsed);
        self.selected = selected_path
            .as_deref()
            .and_then(|path| self.nodes.iter().position(|node| node.path == path))
            .unwrap_or_else(|| selected_index.min(self.nodes.len().saturating_sub(1)));
        self.scroll = self.scroll.min(self.nodes.len().saturating_sub(1));
    }

    pub(crate) fn select(&mut self, index: usize) {
        if !self.nodes.is_empty() {
            self.selected = index.min(self.nodes.len().saturating_sub(1));
        }
    }

    pub(crate) fn select_path(&mut self, path: &Path) {
        if let Some(index) = self.nodes.iter().position(|node| node.path == path) {
            self.selected = index;
        }
    }

    pub(crate) fn move_selection(&mut self, delta: isize, viewport_height: usize) {
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(self.nodes.len().saturating_sub(1));
        self.ensure_visible(viewport_height);
    }

    pub(crate) fn toggle_selected(&mut self) -> bool {
        let Some(node) = self.selected_node() else {
            return false;
        };
        if !node.is_directory() {
            return false;
        }
        let selected_path = node.path.clone();
        if !self.collapsed.insert(selected_path.clone()) {
            self.collapsed.remove(&selected_path);
        }
        self.nodes = build_tree_nodes(&self.sources, &self.collapsed);
        self.select_path(&selected_path);
        true
    }

    pub(crate) fn ensure_visible(&mut self, viewport_height: usize) {
        let visible = viewport_height.max(1);
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll.saturating_add(visible) {
            self.scroll = self.selected.saturating_add(1).saturating_sub(visible);
        }
        self.scroll = self.scroll.min(self.nodes.len().saturating_sub(visible));
    }
}

pub(crate) fn build_tree_nodes(
    entries: &[TreeSourceEntry],
    collapsed: &HashSet<PathBuf>,
) -> Vec<VaultTreeNode> {
    let mut sorted = entries.to_vec();
    sorted.sort_by(|left, right| left.path.cmp(&right.path));
    let mut nodes = Vec::new();
    let mut seen = HashSet::new();
    for entry in sorted {
        let mut path = PathBuf::new();
        let components = entry.path.components().collect::<Vec<_>>();
        for (depth, component) in components.iter().enumerate() {
            let label = component.as_os_str().to_string_lossy().into_owned();
            if label.starts_with('.') {
                break;
            }
            path.push(component.as_os_str());
            let last = depth.saturating_add(1) == components.len();
            let entry_kind = if last {
                entry.entry_kind
            } else {
                VaultEntryKind::Directory
            };
            if seen.insert(path.clone()) {
                nodes.push(VaultTreeNode {
                    path: path.clone(),
                    depth,
                    entry_kind,
                    artifact_kind: last.then_some(entry.artifact_kind).flatten(),
                    label: display_label(label),
                    expanded: entry_kind == VaultEntryKind::Directory && !collapsed.contains(&path),
                });
            }
            if collapsed.contains(&path) {
                break;
            }
        }
    }
    nodes
}

pub(crate) fn render_tree(
    frame: &mut Frame,
    area: Rect,
    model: &mut VaultTreeModel,
    focused: bool,
    hit_areas: &mut VaultHitAreas,
) {
    let theme = crate::theme::active_theme();
    frame.render_widget(
        Paragraph::new("").style(Style::default().bg(theme.panel)),
        area,
    );
    hit_areas.push(area, VaultMouseAction::FocusTree);
    let visible = usize::from(area.height).max(1);
    model.ensure_visible(visible);
    for (row, (index, node)) in model
        .nodes()
        .iter()
        .enumerate()
        .skip(model.scroll)
        .take(visible)
        .enumerate()
    {
        let row_area = Rect::new(area.x, area.y.saturating_add(row as u16), area.width, 1);
        let marker = match node.entry_kind {
            VaultEntryKind::Directory if node.expanded => "v ",
            VaultEntryKind::Directory => "> ",
            VaultEntryKind::File => "- ",
            VaultEntryKind::UnsupportedSymlink | VaultEntryKind::Unsupported => "! ",
        };
        let indent = "  ".repeat(node.depth);
        let label = format!("{indent}{marker}{}", node.label);
        let style = if index == model.selected {
            Style::default()
                .fg(if focused {
                    theme.accent
                } else {
                    theme.foreground
                })
                .bg(theme.card_bg)
        } else {
            Style::default().fg(theme.foreground).bg(theme.panel)
        };
        frame.render_widget(Paragraph::new(label).style(style), row_area);
        hit_areas.push(row_area, VaultMouseAction::TreeRow(index));
    }
}
