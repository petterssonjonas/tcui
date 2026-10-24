use std::collections::HashSet;
use std::path::{Path, PathBuf};

use ratatui::{prelude::*, widgets::*, Frame};

use crate::storage::chat_files::{VaultItemId, VaultItemRecord};
use crate::ui::vault::tree::{build_tree_nodes, TreeSourceEntry, VaultTreeNode};
use crate::vault::VaultEntryKind;

const ROW_HEIGHT: u16 = 3;
const ROW_GAP: u16 = 1;
const ROW_STRIDE: u16 = ROW_HEIGHT + ROW_GAP;
const ROW_MARGIN_HORIZONTAL: u16 = 1;
const FLAT_SECTION_MAX_ROWS: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ArtifactHandle {
    Temporary(u64),
    ChatFile {
        conversation_id: i64,
        id: crate::storage::chat_files::ChatFileId,
    },
    Media(String),
    Memory(u64),
    Vault(VaultItemId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactKind {
    Markdown,
    Text,
    Image,
    Video,
    Audio,
    Binary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactOrigin {
    Temporary,
    Chat,
    Memory,
    Vault,
}

#[derive(Debug, Clone)]
pub struct ArtifactEntry {
    pub handle: ArtifactHandle,
    pub name: String,
    pub kind: ArtifactKind,
    pub origin: ArtifactOrigin,
    pub content: Option<String>,
    pub path: Option<PathBuf>,
    pub relative_path: Option<PathBuf>,
}

impl ArtifactEntry {
    pub fn temp_markdown(id: u64, name: String, content: String) -> Self {
        Self {
            handle: ArtifactHandle::Temporary(id),
            name,
            kind: ArtifactKind::Markdown,
            origin: ArtifactOrigin::Temporary,
            content: Some(content),
            path: None,
            relative_path: None,
        }
    }

    pub(crate) fn chat_file(
        conversation_id: i64,
        id: crate::storage::chat_files::ChatFileId,
        name: String,
        path: PathBuf,
    ) -> Self {
        Self {
            handle: ArtifactHandle::ChatFile {
                conversation_id,
                id,
            },
            name,
            kind: infer_kind(&path, None),
            origin: ArtifactOrigin::Chat,
            content: None,
            path: Some(path),
            relative_path: None,
        }
    }

    pub fn temp_media(source: &str) -> Option<Self> {
        let path = resolve_local_path(source)?;
        Some(Self {
            handle: ArtifactHandle::Media(source.trim().to_string()),
            name: display_name(&path),
            kind: infer_kind(&path, None),
            origin: ArtifactOrigin::Temporary,
            content: None,
            path: Some(path),
            relative_path: None,
        })
    }

    pub fn memory_file(id: u64, title: String) -> Self {
        Self {
            handle: ArtifactHandle::Memory(id),
            name: display_label(title),
            kind: ArtifactKind::Markdown,
            origin: ArtifactOrigin::Memory,
            content: None,
            path: None,
            relative_path: None,
        }
    }

    pub fn vault_file(record: VaultItemRecord) -> Self {
        Self {
            handle: ArtifactHandle::Vault(record.id),
            name: display_label(record.relative_path.to_string_lossy().into_owned()),
            kind: infer_kind(&record.path, None),
            origin: ArtifactOrigin::Vault,
            content: None,
            path: Some(record.path),
            relative_path: Some(record.relative_path),
        }
    }

    pub fn is_markdown(&self) -> bool {
        self.kind == ArtifactKind::Markdown
    }

    pub fn action_label(&self, has_vault: bool) -> Option<&'static str> {
        match self.origin {
            ArtifactOrigin::Temporary | ArtifactOrigin::Chat => {
                Some(if has_vault { "Vault" } else { "Unlock" })
            }
            ArtifactOrigin::Memory => Some("Exp"),
            ArtifactOrigin::Vault => None,
        }
    }

    pub fn save_label(&self, has_vault: bool) -> &'static str {
        self.action_label(has_vault).unwrap_or("")
    }

    pub fn can_save(&self, has_vault: bool) -> bool {
        self.action_label(has_vault).is_some()
    }

    pub fn can_delete(&self) -> bool {
        true
    }

    pub fn origin_label(&self) -> &'static str {
        match self.origin {
            ArtifactOrigin::Temporary | ArtifactOrigin::Chat => "Chat Artifacts",
            ArtifactOrigin::Memory => "Memory",
            ArtifactOrigin::Vault => "Vault",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactSection {
    Temporary,
    Memories,
    Vault,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactSidebarAction {
    ToggleSection(ArtifactSection),
    ToggleVaultDir(PathBuf),
    Open(ArtifactHandle),
    Edit(ArtifactHandle),
    Save(ArtifactHandle),
    Delete(ArtifactHandle),
    Context(ArtifactHandle),
    Copy(ArtifactHandle),
}

#[derive(Debug, Clone)]
struct ArtifactRowHitAreas {
    handle: ArtifactHandle,
    open: Option<Rect>,
    edit: Option<Rect>,
    save: Option<Rect>,
    delete: Option<Rect>,
    context: Option<Rect>,
    copy: Option<Rect>,
}

#[derive(Debug, Clone)]
struct VaultNodeHitArea {
    action: ArtifactSidebarAction,
    area: Rect,
}

#[derive(Debug, Default, Clone)]
pub struct ArtifactSidebarState {
    pub temp_scroll: usize,
    pub memory_scroll: usize,
    pub vault_scroll: usize,
    pub temp_collapsed: bool,
    pub memory_collapsed: bool,
    pub vault_collapsed: bool,
    pub collapsed_vault_dirs: HashSet<PathBuf>,
    pub temp_header: Option<Rect>,
    pub memory_header: Option<Rect>,
    pub vault_header: Option<Rect>,
    pub temp_body: Option<Rect>,
    pub memory_body: Option<Rect>,
    pub vault_body: Option<Rect>,
    temp_rows: Vec<ArtifactRowHitAreas>,
    memory_rows: Vec<ArtifactRowHitAreas>,
    vault_nodes: Vec<VaultNodeHitArea>,
}

impl ArtifactSidebarState {
    pub(crate) fn clear_hit_areas(&mut self) {
        self.temp_rows.clear();
        self.memory_rows.clear();
        self.vault_nodes.clear();
        self.temp_header = None;
        self.memory_header = None;
        self.vault_header = None;
        self.temp_body = None;
        self.memory_body = None;
        self.vault_body = None;
    }

    pub fn action_at(&self, pos: Position) -> Option<ArtifactSidebarAction> {
        if let Some(section) = self.header_at(pos) {
            return Some(ArtifactSidebarAction::ToggleSection(section));
        }

        for row in self.temp_rows.iter().chain(self.memory_rows.iter()) {
            if row.open.is_some_and(|area| area.contains(pos)) {
                return Some(ArtifactSidebarAction::Open(row.handle.clone()));
            }
            if row.save.is_some_and(|area| area.contains(pos)) {
                return Some(ArtifactSidebarAction::Save(row.handle.clone()));
            }
            if row.edit.is_some_and(|area| area.contains(pos)) {
                return Some(ArtifactSidebarAction::Edit(row.handle.clone()));
            }
            if row.context.is_some_and(|area| area.contains(pos)) {
                return Some(ArtifactSidebarAction::Context(row.handle.clone()));
            }
            if row.copy.is_some_and(|area| area.contains(pos)) {
                return Some(ArtifactSidebarAction::Copy(row.handle.clone()));
            }
            if row.delete.is_some_and(|area| area.contains(pos)) {
                return Some(ArtifactSidebarAction::Delete(row.handle.clone()));
            }
        }

        self.vault_nodes
            .iter()
            .find_map(|hit| hit.area.contains(pos).then(|| hit.action.clone()))
    }

    pub fn section_at(&self, pos: Position) -> Option<ArtifactSection> {
        [
            (ArtifactSection::Temporary, self.temp_body),
            (ArtifactSection::Memories, self.memory_body),
            (ArtifactSection::Vault, self.vault_body),
        ]
        .into_iter()
        .find_map(|(section, area)| area.filter(|rect| rect.contains(pos)).map(|_| section))
    }

    pub fn scroll(&mut self, section: ArtifactSection, down: bool, total: usize, visible: usize) {
        let max_offset = total.saturating_sub(visible.max(1));
        let offset = match section {
            ArtifactSection::Temporary => &mut self.temp_scroll,
            ArtifactSection::Memories => &mut self.memory_scroll,
            ArtifactSection::Vault => &mut self.vault_scroll,
        };
        if down {
            *offset = (*offset + 1).min(max_offset);
        } else {
            *offset = offset.saturating_sub(1);
        }
        self.clear_hit_areas();
    }

    pub fn toggle(&mut self, section: ArtifactSection) {
        match section {
            ArtifactSection::Temporary => self.temp_collapsed = !self.temp_collapsed,
            ArtifactSection::Memories => self.memory_collapsed = !self.memory_collapsed,
            ArtifactSection::Vault => self.vault_collapsed = !self.vault_collapsed,
        }
        self.clear_hit_areas();
    }

    pub fn toggle_vault_dir(&mut self, path: &Path) {
        if !self.collapsed_vault_dirs.insert(path.to_path_buf()) {
            self.collapsed_vault_dirs.remove(path);
        }
        self.clear_hit_areas();
    }

    fn header_at(&self, pos: Position) -> Option<ArtifactSection> {
        [
            (ArtifactSection::Temporary, self.temp_header),
            (ArtifactSection::Memories, self.memory_header),
            (ArtifactSection::Vault, self.vault_header),
        ]
        .into_iter()
        .find_map(|(section, area)| area.filter(|rect| rect.contains(pos)).map(|_| section))
    }
}

pub struct ArtifactSidebar<'a> {
    temporary: &'a [ArtifactEntry],
    memories: &'a [ArtifactEntry],
    vault: &'a [ArtifactEntry],
    has_vault: bool,
    visibility: ArtifactSidebarVisibility,
    state: &'a mut ArtifactSidebarState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArtifactSidebarVisibility {
    pub memories: bool,
    pub vault: bool,
}

impl ArtifactSidebarVisibility {
    pub const fn all() -> Self {
        Self {
            memories: true,
            vault: true,
        }
    }
}

impl<'a> ArtifactSidebar<'a> {
    pub fn new(
        temporary: &'a [ArtifactEntry],
        memories: &'a [ArtifactEntry],
        vault: &'a [ArtifactEntry],
        has_vault: bool,
        state: &'a mut ArtifactSidebarState,
    ) -> Self {
        Self {
            temporary,
            memories,
            vault,
            has_vault,
            visibility: ArtifactSidebarVisibility::all(),
            state,
        }
    }

    pub const fn with_visibility(mut self, visibility: ArtifactSidebarVisibility) -> Self {
        self.visibility = visibility;
        self
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect) {
        self.clear_hit_state();

        let theme = crate::theme::active_theme();
        let block = Block::default().style(theme.sidebar_style());
        let inner = block.inner(area);
        f.render_widget(block, area);

        let temp_body_height = section_body_height(
            self.state.temp_collapsed,
            self.temporary.len(),
            FLAT_SECTION_MAX_ROWS,
            inner.height,
        );
        let memory_body_height = self.visibility.memories.then(|| {
            section_body_height(
                self.state.memory_collapsed,
                self.memories.len(),
                FLAT_SECTION_MAX_ROWS,
                inner.height,
            )
        });
        let vault_body_constraint = if self.state.vault_collapsed {
            Constraint::Length(0)
        } else {
            Constraint::Min(0)
        };

        let mut constraints = vec![Constraint::Length(1), Constraint::Length(temp_body_height)];
        if let Some(height) = memory_body_height {
            constraints.extend([Constraint::Length(1), Constraint::Length(height)]);
        }
        if self.visibility.vault {
            constraints.extend([Constraint::Length(1), vault_body_constraint]);
        }
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(inner);

        let mut chunk = 0;
        self.render_flat_section(
            f,
            chunks[chunk],
            chunks[chunk + 1],
            SectionConfig {
                section: ArtifactSection::Temporary,
                title: "Chat Artifacts",
                entries: self.temporary,
                scroll: self.state.temp_scroll,
                collapsed: self.state.temp_collapsed,
                max_rows: FLAT_SECTION_MAX_ROWS,
            },
        );
        chunk += 2;
        if self.visibility.memories {
            self.render_flat_section(
                f,
                chunks[chunk],
                chunks[chunk + 1],
                SectionConfig {
                    section: ArtifactSection::Memories,
                    title: "Memories",
                    entries: self.memories,
                    scroll: self.state.memory_scroll,
                    collapsed: self.state.memory_collapsed,
                    max_rows: FLAT_SECTION_MAX_ROWS,
                },
            );
            chunk += 2;
        }
        if self.visibility.vault {
            self.render_vault_section(f, chunks[chunk], chunks[chunk + 1]);
        }
    }

    pub fn visible_rows(area: Rect) -> usize {
        usize::from(area.height.saturating_add(ROW_GAP) / ROW_STRIDE).max(1)
    }

    pub fn visible_vault_rows(area: Rect) -> usize {
        usize::from(area.height).max(1)
    }

    pub(crate) fn clear_hit_state(&mut self) {
        self.state.clear_hit_areas();
    }

    fn render_flat_section(
        &mut self,
        f: &mut Frame,
        header_area: Rect,
        body_area: Rect,
        cfg: SectionConfig<'_>,
    ) {
        self.set_header(cfg.section, header_area);
        self.set_body(
            cfg.section,
            if cfg.collapsed { None } else { Some(body_area) },
        );
        self.render_header(f, header_area, cfg.title, cfg.collapsed);
        if cfg.collapsed || body_area.height == 0 {
            return;
        }

        let visible = Self::visible_rows(body_area).min(cfg.max_rows);
        let scroll = cfg.scroll.min(cfg.entries.len().saturating_sub(visible));
        self.set_scroll(cfg.section, scroll);
        self.render_flat_body(f, body_area, cfg.entries, scroll, cfg.section);
    }

    fn render_vault_section(&mut self, f: &mut Frame, header_area: Rect, body_area: Rect) {
        self.set_header(ArtifactSection::Vault, header_area);
        self.set_body(
            ArtifactSection::Vault,
            if self.state.vault_collapsed {
                None
            } else {
                Some(body_area)
            },
        );
        self.render_header(f, header_area, "Vault", self.state.vault_collapsed);
        if self.state.vault_collapsed || body_area.height == 0 {
            return;
        }

        let nodes = build_vault_nodes(self.vault, &self.state.collapsed_vault_dirs);
        let visible = Self::visible_vault_rows(body_area);
        let scroll = self
            .state
            .vault_scroll
            .min(nodes.len().saturating_sub(visible));
        self.state.vault_scroll = scroll;
        self.render_vault_body(f, body_area, &nodes, scroll);
    }

    fn render_header(&self, f: &mut Frame, area: Rect, title: &str, collapsed: bool) {
        let theme = crate::theme::active_theme();
        let arrow = if collapsed { "->" } else { "v>" };
        let label = format!(" {arrow} {title} ");
        let fill_width = area.width.saturating_sub(label.chars().count() as u16) as usize;
        let fill = "─".repeat(fill_width);
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    label,
                    Style::default()
                        .fg(theme.muted)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(fill, Style::default().fg(theme.border)),
            ])),
            area,
        );
    }

    fn render_flat_body(
        &mut self,
        f: &mut Frame,
        area: Rect,
        entries: &[ArtifactEntry],
        scroll: usize,
        section: ArtifactSection,
    ) {
        let theme = crate::theme::active_theme();
        f.render_widget(Paragraph::new("").style(theme.sidebar_style()), area);
        if entries.is_empty() {
            f.render_widget(
                Paragraph::new(" No items")
                    .style(Style::default().fg(theme.muted))
                    .alignment(Alignment::Center),
                area,
            );
            return;
        }

        let visible = Self::visible_rows(area);
        for (row_idx, entry) in entries.iter().skip(scroll).take(visible).enumerate() {
            let row_y = area.y + row_idx as u16 * ROW_STRIDE;
            let row_area = Rect::new(area.x, row_y, area.width, ROW_HEIGHT).inner(Margin {
                vertical: 0,
                horizontal: ROW_MARGIN_HORIZONTAL,
            });
            self.render_flat_row(f, row_area, entry, section);
        }
    }

    fn render_flat_row(
        &mut self,
        f: &mut Frame,
        area: Rect,
        entry: &ArtifactEntry,
        section: ArtifactSection,
    ) {
        let theme = crate::theme::active_theme();
        if area.height < 3 || area.width == 0 {
            return;
        }
        f.render_widget(Paragraph::new("").style(theme.card_style()), area);

        let title = truncate_label(&entry.name, area.width.saturating_sub(1) as usize);
        let meta = truncate_label(
            &format!("{} · {}", entry.origin_label(), kind_label(entry.kind)),
            area.width as usize,
        );
        f.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    format!(" {title}"),
                    Style::default()
                        .fg(theme.foreground)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled(
                    format!(" {meta}"),
                    Style::default().fg(theme.muted),
                )),
            ])
            .style(theme.card_style()),
            Rect::new(area.x, area.y, area.width, 2),
        );

        let mut x = area.x;
        let y = area.y + 2;
        let open = button_area(&mut x, y, "View", area.right());
        if let Some(rect) = open {
            f.render_widget(button("View", theme.info, theme.card_bg), rect);
        }

        let context = if matches!(entry.origin, ArtifactOrigin::Memory | ArtifactOrigin::Vault) {
            let rect = button_area(&mut x, y, "+", area.right());
            if let Some(rect) = rect {
                f.render_widget(button("+", theme.info, theme.card_bg), rect);
            }
            rect
        } else {
            None
        };
        let copy = button_area(&mut x, y, "Copy", area.right());
        if let Some(rect) = copy {
            f.render_widget(button("Copy", theme.info, theme.card_bg), rect);
        }

        let edit = if matches!(entry.origin, ArtifactOrigin::Memory | ArtifactOrigin::Vault) {
            let rect = button_area(&mut x, y, "Edit", area.right());
            if let Some(rect) = rect {
                f.render_widget(button("Edit", theme.warning, theme.card_bg), rect);
            }
            rect
        } else {
            None
        };

        let save = if entry.can_save(self.has_vault) {
            let label = entry.save_label(self.has_vault);
            let rect = button_area(&mut x, y, label, area.right());
            if let Some(rect) = rect {
                f.render_widget(button(label, theme.success, theme.card_bg), rect);
            }
            rect
        } else {
            None
        };

        let delete = if entry.can_delete() {
            let rect = button_area(&mut x, y, "Del", area.right());
            if let Some(rect) = rect {
                f.render_widget(button("Del", theme.error, theme.card_bg), rect);
            }
            rect
        } else {
            None
        };

        self.row_hits_mut(section).push(ArtifactRowHitAreas {
            handle: entry.handle.clone(),
            open,
            edit,
            save,
            delete,
            context,
            copy,
        });
    }

    fn render_vault_body(
        &mut self,
        f: &mut Frame,
        area: Rect,
        nodes: &[VaultTreeNode],
        scroll: usize,
    ) {
        let theme = crate::theme::active_theme();
        f.render_widget(Paragraph::new("").style(theme.sidebar_style()), area);
        if nodes.is_empty() {
            f.render_widget(
                Paragraph::new(" No items")
                    .style(Style::default().fg(theme.muted))
                    .alignment(Alignment::Center),
                area,
            );
            return;
        }

        let visible = Self::visible_vault_rows(area);
        for (idx, node) in nodes.iter().skip(scroll).take(visible).enumerate() {
            let row = Rect::new(area.x, area.y + idx as u16, area.width, 1);
            if node.entry_kind == VaultEntryKind::Directory {
                f.render_widget(
                    Paragraph::new(vault_node_label(node, area.width as usize))
                        .style(Style::default().fg(theme.foreground)),
                    row,
                );
                self.state.vault_nodes.push(VaultNodeHitArea {
                    action: ArtifactSidebarAction::ToggleVaultDir(node.path.clone()),
                    area: row,
                });
                continue;
            }
            // Catalog refresh sorts these native entries by relative path.
            let Ok(index) = self.vault.binary_search_by(|entry| {
                entry
                    .relative_path
                    .as_deref()
                    .cmp(&Some(node.path.as_path()))
            }) else {
                continue;
            };
            let handle = self.vault[index].handle.clone();
            let context_width = if row.width >= 4 { 4 } else { 0 };
            let open = Rect::new(row.x, row.y, row.width - context_width, 1);
            f.render_widget(
                Paragraph::new(vault_node_label(node, open.width as usize))
                    .style(Style::default().fg(theme.foreground)),
                open,
            );
            self.state.vault_nodes.push(VaultNodeHitArea {
                action: ArtifactSidebarAction::Open(handle.clone()),
                area: open,
            });
            if context_width != 0 {
                let context = Rect::new(row.right() - 3, row.y, 3, 1);
                f.render_widget(button("+", theme.info, theme.sidebar), context);
                self.state.vault_nodes.push(VaultNodeHitArea {
                    action: ArtifactSidebarAction::Context(handle),
                    area: context,
                });
            }
        }
    }

    fn row_hits_mut(&mut self, section: ArtifactSection) -> &mut Vec<ArtifactRowHitAreas> {
        match section {
            ArtifactSection::Temporary => &mut self.state.temp_rows,
            ArtifactSection::Memories => &mut self.state.memory_rows,
            ArtifactSection::Vault => unreachable!("vault uses line hit areas"),
        }
    }

    fn set_header(&mut self, section: ArtifactSection, area: Rect) {
        match section {
            ArtifactSection::Temporary => self.state.temp_header = Some(area),
            ArtifactSection::Memories => self.state.memory_header = Some(area),
            ArtifactSection::Vault => self.state.vault_header = Some(area),
        }
    }

    fn set_body(&mut self, section: ArtifactSection, area: Option<Rect>) {
        match section {
            ArtifactSection::Temporary => self.state.temp_body = area,
            ArtifactSection::Memories => self.state.memory_body = area,
            ArtifactSection::Vault => self.state.vault_body = area,
        }
    }

    fn set_scroll(&mut self, section: ArtifactSection, value: usize) {
        match section {
            ArtifactSection::Temporary => self.state.temp_scroll = value,
            ArtifactSection::Memories => self.state.memory_scroll = value,
            ArtifactSection::Vault => self.state.vault_scroll = value,
        }
    }
}

struct SectionConfig<'a> {
    section: ArtifactSection,
    title: &'a str,
    entries: &'a [ArtifactEntry],
    scroll: usize,
    collapsed: bool,
    max_rows: usize,
}

fn build_vault_nodes(
    entries: &[ArtifactEntry],
    collapsed_dirs: &HashSet<PathBuf>,
) -> Vec<VaultTreeNode> {
    let sources = entries
        .iter()
        .filter_map(|entry| {
            entry
                .relative_path
                .as_ref()
                .map(|path| TreeSourceEntry::file(path.clone(), entry.kind))
        })
        .collect::<Vec<_>>();
    build_tree_nodes(&sources, collapsed_dirs)
}

fn vault_node_label(node: &VaultTreeNode, width: usize) -> String {
    let indent = "  ".repeat(node.depth);
    let marker = match node.entry_kind {
        VaultEntryKind::Directory => {
            if node.expanded {
                "v "
            } else {
                "> "
            }
        }
        VaultEntryKind::File => "- ",
        VaultEntryKind::UnsupportedSymlink | VaultEntryKind::Unsupported => "! ",
    };
    truncate_label(&format!("{indent}{marker}{}", node.label), width)
}

fn section_body_height(
    collapsed: bool,
    entry_count: usize,
    max_rows: usize,
    available: u16,
) -> u16 {
    if collapsed {
        0
    } else if entry_count == 0 {
        available.min(1)
    } else {
        let rows = entry_count.min(max_rows) as u16;
        let gaps = rows.saturating_sub(1).saturating_mul(ROW_GAP);
        available.min(rows.saturating_mul(ROW_HEIGHT).saturating_add(gaps))
    }
}

fn kind_label(kind: ArtifactKind) -> &'static str {
    match kind {
        ArtifactKind::Markdown => "md",
        ArtifactKind::Text => "txt",
        ArtifactKind::Image => "img",
        ArtifactKind::Video => "vid",
        ArtifactKind::Audio => "aud",
        ArtifactKind::Binary => "bin",
    }
}

fn button(label: &str, color: Color, background: Color) -> Paragraph<'static> {
    Paragraph::new(format!("[{label}]")).style(Style::default().fg(color).bg(background))
}

fn button_area(x: &mut u16, y: u16, label: &str, right: u16) -> Option<Rect> {
    let width = label.len() as u16 + 2;
    if width > right.saturating_sub(*x) {
        *x = right;
        return None;
    }
    let rect = Rect::new(*x, y, width, 1);
    *x = (*x).saturating_add(width + 1).min(right);
    Some(rect)
}

fn truncate_label(label: &str, width: usize) -> String {
    if label.chars().count() <= width {
        return label.to_string();
    }
    label
        .chars()
        .take(width.saturating_sub(3))
        .collect::<String>()
        + "..."
}

pub(crate) fn infer_kind(path: &Path, content: Option<&str>) -> ArtifactKind {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase());
    match extension.as_deref() {
        Some("md") => ArtifactKind::Markdown,
        Some("txt") | Some("log") | Some("json") | Some("yaml") | Some("yml") => ArtifactKind::Text,
        Some("png") | Some("jpg") | Some("jpeg") | Some("gif") | Some("webp") | Some("bmp") => {
            ArtifactKind::Image
        }
        Some("mp4") | Some("mkv") | Some("mov") | Some("webm") => ArtifactKind::Video,
        Some("mp3") | Some("wav") | Some("ogg") | Some("flac") => ArtifactKind::Audio,
        _ => {
            if content.is_some() {
                ArtifactKind::Text
            } else {
                ArtifactKind::Binary
            }
        }
    }
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(ToString::to_string)
        .unwrap_or_else(|| path.display().to_string())
}

pub(crate) fn display_label(value: String) -> String {
    if value
        .chars()
        .any(|character| character.is_control() || character == '\\')
    {
        value.escape_debug().to_string()
    } else {
        value
    }
}

fn resolve_local_path(source: &str) -> Option<PathBuf> {
    crate::ui::components::image_block::is_local_image_source(source).then(|| {
        let trimmed = source.trim().trim_matches('<').trim_matches('>');
        if let Some(path) = trimmed.strip_prefix("file://") {
            PathBuf::from(path)
        } else {
            PathBuf::from(trimmed)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{build_vault_nodes, ArtifactEntry, ArtifactHandle, ArtifactKind, ArtifactOrigin};
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};

    fn entry(name: &str, origin: ArtifactOrigin) -> ArtifactEntry {
        ArtifactEntry {
            handle: match origin {
                ArtifactOrigin::Temporary => ArtifactHandle::Temporary(1),
                ArtifactOrigin::Chat => ArtifactHandle::Temporary(1),
                ArtifactOrigin::Memory => ArtifactHandle::Memory(432),
                ArtifactOrigin::Vault => ArtifactHandle::Vault("32".parse().expect("item ID")),
            },
            name: name.to_string(),
            kind: ArtifactKind::Markdown,
            origin,
            content: Some("content".to_string()),
            path: None,
            relative_path: (origin == ArtifactOrigin::Vault).then(|| PathBuf::from(name)),
        }
    }

    #[test]
    fn vault_nodes_build_nested_tree_and_respect_collapsed_dirs() {
        let entries = vec![
            entry("notes/a.md", ArtifactOrigin::Vault),
            entry("notes/projects/b.md", ArtifactOrigin::Vault),
        ];

        let expanded = build_vault_nodes(&entries, &HashSet::new());
        assert!(expanded.iter().any(|node| node.path == Path::new("notes")));
        assert!(expanded
            .iter()
            .any(|node| node.path == Path::new("notes/projects")));
        assert!(expanded
            .iter()
            .any(|node| node.path == Path::new("notes/projects/b.md")));

        let mut collapsed_dirs = HashSet::new();
        collapsed_dirs.insert(PathBuf::from("notes"));
        let collapsed = build_vault_nodes(&entries, &collapsed_dirs);
        assert!(collapsed.iter().any(|node| node.path == Path::new("notes")));
        assert!(!collapsed
            .iter()
            .any(|node| node.path == Path::new("notes/projects")));
        assert!(!collapsed
            .iter()
            .any(|node| node.path == Path::new("notes/a.md")));
    }
}
