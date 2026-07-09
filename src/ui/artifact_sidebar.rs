use std::collections::HashSet;
use std::path::{Path, PathBuf};

use ratatui::{prelude::*, widgets::*, Frame};

const ROW_HEIGHT: u16 = 3;
const FLAT_SECTION_MAX_ROWS: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ArtifactHandle {
    Temporary(u64),
    Media(String),
    Saved(PathBuf),
    Memory(PathBuf),
    Vault(PathBuf),
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
    Saved,
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
        })
    }

    pub fn saved_markdown(path: PathBuf, content: String) -> Self {
        Self {
            handle: ArtifactHandle::Saved(path.clone()),
            name: display_name(&path),
            kind: ArtifactKind::Markdown,
            origin: ArtifactOrigin::Saved,
            content: Some(content),
            path: Some(path),
        }
    }

    pub fn saved_file(path: PathBuf) -> Self {
        Self {
            handle: ArtifactHandle::Saved(path.clone()),
            name: display_name(&path),
            kind: infer_kind(&path, None),
            origin: ArtifactOrigin::Saved,
            content: None,
            path: Some(path),
        }
    }

    pub fn memory_file(
        logical_path: PathBuf,
        title: String,
        markdown: String,
        path: PathBuf,
    ) -> Self {
        Self {
            handle: ArtifactHandle::Memory(logical_path),
            name: title,
            kind: ArtifactKind::Markdown,
            origin: ArtifactOrigin::Memory,
            content: Some(markdown),
            path: Some(path),
        }
    }

    pub fn vault_file(root: &Path, path: &Path) -> Self {
        let relative = path
            .strip_prefix(root)
            .ok()
            .map(PathBuf::from)
            .unwrap_or_else(|| path.to_path_buf());
        Self {
            handle: ArtifactHandle::Vault(relative.clone()),
            name: relative.display().to_string(),
            kind: infer_kind(&relative, None),
            origin: ArtifactOrigin::Vault,
            content: None,
            path: Some(path.to_path_buf()),
        }
    }

    pub fn is_markdown(&self) -> bool {
        self.kind == ArtifactKind::Markdown
    }

    pub fn action_label(&self, has_vault: bool) -> Option<&'static str> {
        match self.origin {
            ArtifactOrigin::Temporary if self.is_markdown() && has_vault => Some("Vault"),
            ArtifactOrigin::Temporary => Some("Save"),
            ArtifactOrigin::Memory => Some("Exp"),
            ArtifactOrigin::Saved => Some("Export"),
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
            ArtifactOrigin::Temporary => "Temporary",
            ArtifactOrigin::Saved => "Saved",
            ArtifactOrigin::Memory => "Memory",
            ArtifactOrigin::Vault => "Vault",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactSection {
    Temporary,
    Saved,
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
}

#[derive(Debug, Clone)]
struct ArtifactRowHitAreas {
    handle: ArtifactHandle,
    open: Rect,
    edit: Option<Rect>,
    save: Option<Rect>,
    delete: Option<Rect>,
}

#[derive(Debug, Clone)]
struct VaultNodeHitArea {
    action: ArtifactSidebarAction,
    area: Rect,
}

#[derive(Debug, Default, Clone)]
pub struct ArtifactSidebarState {
    pub temp_scroll: usize,
    pub saved_scroll: usize,
    pub memory_scroll: usize,
    pub vault_scroll: usize,
    pub temp_collapsed: bool,
    pub saved_collapsed: bool,
    pub memory_collapsed: bool,
    pub vault_collapsed: bool,
    pub collapsed_vault_dirs: HashSet<PathBuf>,
    pub temp_header: Option<Rect>,
    pub saved_header: Option<Rect>,
    pub memory_header: Option<Rect>,
    pub vault_header: Option<Rect>,
    pub temp_body: Option<Rect>,
    pub saved_body: Option<Rect>,
    pub memory_body: Option<Rect>,
    pub vault_body: Option<Rect>,
    temp_rows: Vec<ArtifactRowHitAreas>,
    saved_rows: Vec<ArtifactRowHitAreas>,
    memory_rows: Vec<ArtifactRowHitAreas>,
    vault_nodes: Vec<VaultNodeHitArea>,
}

impl ArtifactSidebarState {
    pub fn action_at(&self, pos: Position) -> Option<ArtifactSidebarAction> {
        if let Some(section) = self.header_at(pos) {
            return Some(ArtifactSidebarAction::ToggleSection(section));
        }

        for row in self
            .temp_rows
            .iter()
            .chain(self.saved_rows.iter())
            .chain(self.memory_rows.iter())
        {
            if row.open.contains(pos) {
                return Some(ArtifactSidebarAction::Open(row.handle.clone()));
            }
            if row.save.is_some_and(|area| area.contains(pos)) {
                return Some(ArtifactSidebarAction::Save(row.handle.clone()));
            }
            if row.edit.is_some_and(|area| area.contains(pos)) {
                return Some(ArtifactSidebarAction::Edit(row.handle.clone()));
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
            (ArtifactSection::Saved, self.saved_body),
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
            ArtifactSection::Saved => &mut self.saved_scroll,
            ArtifactSection::Memories => &mut self.memory_scroll,
            ArtifactSection::Vault => &mut self.vault_scroll,
        };
        if down {
            *offset = (*offset + 1).min(max_offset);
        } else {
            *offset = offset.saturating_sub(1);
        }
    }

    pub fn toggle(&mut self, section: ArtifactSection) {
        match section {
            ArtifactSection::Temporary => self.temp_collapsed = !self.temp_collapsed,
            ArtifactSection::Saved => self.saved_collapsed = !self.saved_collapsed,
            ArtifactSection::Memories => self.memory_collapsed = !self.memory_collapsed,
            ArtifactSection::Vault => self.vault_collapsed = !self.vault_collapsed,
        }
    }

    pub fn toggle_vault_dir(&mut self, path: &Path) {
        if !self.collapsed_vault_dirs.insert(path.to_path_buf()) {
            self.collapsed_vault_dirs.remove(path);
        }
    }

    fn header_at(&self, pos: Position) -> Option<ArtifactSection> {
        [
            (ArtifactSection::Temporary, self.temp_header),
            (ArtifactSection::Saved, self.saved_header),
            (ArtifactSection::Memories, self.memory_header),
            (ArtifactSection::Vault, self.vault_header),
        ]
        .into_iter()
        .find_map(|(section, area)| area.filter(|rect| rect.contains(pos)).map(|_| section))
    }
}

pub struct ArtifactSidebar<'a> {
    temporary: &'a [ArtifactEntry],
    saved: &'a [ArtifactEntry],
    memories: &'a [ArtifactEntry],
    vault: &'a [ArtifactEntry],
    has_vault: bool,
    state: &'a mut ArtifactSidebarState,
}

impl<'a> ArtifactSidebar<'a> {
    pub fn new(
        temporary: &'a [ArtifactEntry],
        saved: &'a [ArtifactEntry],
        memories: &'a [ArtifactEntry],
        vault: &'a [ArtifactEntry],
        has_vault: bool,
        state: &'a mut ArtifactSidebarState,
    ) -> Self {
        Self {
            temporary,
            saved,
            memories,
            vault,
            has_vault,
            state,
        }
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect) {
        self.clear_hit_state();

        let theme = crate::theme::active_theme();
        let block = Block::default().style(theme.panel_style());
        let inner = block.inner(area);
        f.render_widget(block, area);

        let temp_body_height = section_body_height(
            self.state.temp_collapsed,
            self.temporary.len(),
            FLAT_SECTION_MAX_ROWS,
            inner.height,
        );
        let saved_body_height = section_body_height(
            self.state.saved_collapsed,
            self.saved.len(),
            FLAT_SECTION_MAX_ROWS,
            inner.height,
        );
        let memory_body_height = section_body_height(
            self.state.memory_collapsed,
            self.memories.len(),
            FLAT_SECTION_MAX_ROWS,
            inner.height,
        );
        let vault_body_constraint = if self.state.vault_collapsed {
            Constraint::Length(0)
        } else {
            Constraint::Min(0)
        };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(temp_body_height),
                Constraint::Length(1),
                Constraint::Length(saved_body_height),
                Constraint::Length(1),
                Constraint::Length(memory_body_height),
                Constraint::Length(1),
                vault_body_constraint,
            ])
            .split(inner);

        self.render_flat_section(
            f,
            chunks[0],
            chunks[1],
            SectionConfig {
                section: ArtifactSection::Temporary,
                title: "Artifacts",
                entries: self.temporary,
                scroll: self.state.temp_scroll,
                collapsed: self.state.temp_collapsed,
                max_rows: FLAT_SECTION_MAX_ROWS,
            },
        );
        self.render_flat_section(
            f,
            chunks[2],
            chunks[3],
            SectionConfig {
                section: ArtifactSection::Saved,
                title: "Saved files",
                entries: self.saved,
                scroll: self.state.saved_scroll,
                collapsed: self.state.saved_collapsed,
                max_rows: FLAT_SECTION_MAX_ROWS,
            },
        );
        self.render_flat_section(
            f,
            chunks[4],
            chunks[5],
            SectionConfig {
                section: ArtifactSection::Memories,
                title: "Memories",
                entries: self.memories,
                scroll: self.state.memory_scroll,
                collapsed: self.state.memory_collapsed,
                max_rows: FLAT_SECTION_MAX_ROWS,
            },
        );
        self.render_vault_section(f, chunks[6], chunks[7]);
    }

    pub fn visible_rows(area: Rect) -> usize {
        usize::from(area.height / ROW_HEIGHT).max(1)
    }

    pub fn visible_vault_rows(area: Rect) -> usize {
        usize::from(area.height).max(1)
    }

    fn clear_hit_state(&mut self) {
        self.state.temp_rows.clear();
        self.state.saved_rows.clear();
        self.state.memory_rows.clear();
        self.state.vault_nodes.clear();
        self.state.temp_header = None;
        self.state.saved_header = None;
        self.state.memory_header = None;
        self.state.vault_header = None;
        self.state.temp_body = None;
        self.state.saved_body = None;
        self.state.memory_body = None;
        self.state.vault_body = None;
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
        f.render_widget(Paragraph::new("").style(theme.panel_style()), area);
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
            let row_y = area.y + row_idx as u16 * ROW_HEIGHT;
            let row_area = Rect::new(area.x, row_y, area.width, ROW_HEIGHT);
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
        f.render_widget(
            Paragraph::new("").style(Style::default().bg(theme.sidebar).fg(theme.foreground)),
            area,
        );

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
            ]),
            Rect::new(area.x, area.y, area.width, 2),
        );

        let mut x = area.x;
        let y = area.y + 2;
        let open = button_area(&mut x, y, "View");
        f.render_widget(button("View", theme.info), open);

        let edit = if matches!(entry.origin, ArtifactOrigin::Saved) {
            let rect = button_area(&mut x, y, "Edit");
            f.render_widget(button("Edit", theme.warning), rect);
            Some(rect)
        } else {
            None
        };

        let save = if entry.can_save(self.has_vault) {
            let label = entry.save_label(self.has_vault);
            let rect = button_area(&mut x, y, label);
            f.render_widget(button(label, theme.success), rect);
            Some(rect)
        } else {
            None
        };

        let delete = if entry.can_delete() {
            let rect = button_area(&mut x, y, "Del");
            f.render_widget(button("Del", theme.error), rect);
            Some(rect)
        } else {
            None
        };

        self.row_hits_mut(section).push(ArtifactRowHitAreas {
            handle: entry.handle.clone(),
            open,
            edit,
            save,
            delete,
        });
    }

    fn render_vault_body(&mut self, f: &mut Frame, area: Rect, nodes: &[VaultNode], scroll: usize) {
        let theme = crate::theme::active_theme();
        f.render_widget(Paragraph::new("").style(theme.panel_style()), area);
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
            let label = vault_node_label(node, area.width as usize);
            f.render_widget(
                Paragraph::new(label).style(Style::default().fg(theme.foreground)),
                row,
            );
            self.state.vault_nodes.push(VaultNodeHitArea {
                action: match node.kind {
                    VaultNodeKind::Directory => {
                        ArtifactSidebarAction::ToggleVaultDir(node.path.clone())
                    }
                    VaultNodeKind::File => {
                        ArtifactSidebarAction::Open(ArtifactHandle::Vault(node.path.clone()))
                    }
                },
                area: row,
            });
        }
    }

    fn row_hits_mut(&mut self, section: ArtifactSection) -> &mut Vec<ArtifactRowHitAreas> {
        match section {
            ArtifactSection::Temporary => &mut self.state.temp_rows,
            ArtifactSection::Saved => &mut self.state.saved_rows,
            ArtifactSection::Memories => &mut self.state.memory_rows,
            ArtifactSection::Vault => unreachable!("vault uses line hit areas"),
        }
    }

    fn set_header(&mut self, section: ArtifactSection, area: Rect) {
        match section {
            ArtifactSection::Temporary => self.state.temp_header = Some(area),
            ArtifactSection::Saved => self.state.saved_header = Some(area),
            ArtifactSection::Memories => self.state.memory_header = Some(area),
            ArtifactSection::Vault => self.state.vault_header = Some(area),
        }
    }

    fn set_body(&mut self, section: ArtifactSection, area: Option<Rect>) {
        match section {
            ArtifactSection::Temporary => self.state.temp_body = area,
            ArtifactSection::Saved => self.state.saved_body = area,
            ArtifactSection::Memories => self.state.memory_body = area,
            ArtifactSection::Vault => self.state.vault_body = area,
        }
    }

    fn set_scroll(&mut self, section: ArtifactSection, value: usize) {
        match section {
            ArtifactSection::Temporary => self.state.temp_scroll = value,
            ArtifactSection::Saved => self.state.saved_scroll = value,
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum VaultNodeKind {
    Directory,
    File,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct VaultNode {
    path: PathBuf,
    depth: usize,
    kind: VaultNodeKind,
    label: String,
    expanded: bool,
}

fn build_vault_nodes(
    entries: &[ArtifactEntry],
    collapsed_dirs: &HashSet<PathBuf>,
) -> Vec<VaultNode> {
    let mut nodes = Vec::new();
    let mut seen_dirs = HashSet::new();
    let mut files: Vec<PathBuf> = entries
        .iter()
        .filter_map(|entry| match &entry.handle {
            ArtifactHandle::Vault(path) => Some(path.clone()),
            _ => None,
        })
        .collect();
    files.sort();

    for file in files {
        let mut parent = PathBuf::new();
        let components: Vec<_> = file.components().collect();
        for (idx, component) in components.iter().enumerate() {
            let name = component.as_os_str().to_string_lossy().to_string();
            let is_last = idx + 1 == components.len();
            parent.push(component.as_os_str());

            if is_last {
                if !is_hidden_component(&name) {
                    nodes.push(VaultNode {
                        path: parent.clone(),
                        depth: idx,
                        kind: VaultNodeKind::File,
                        label: name,
                        expanded: false,
                    });
                }
                continue;
            }

            if is_hidden_component(&name) {
                break;
            }

            if seen_dirs.insert(parent.clone()) {
                nodes.push(VaultNode {
                    path: parent.clone(),
                    depth: idx,
                    kind: VaultNodeKind::Directory,
                    label: name.clone(),
                    expanded: !collapsed_dirs.contains(&parent),
                });
            }

            if collapsed_dirs.contains(&parent) {
                break;
            }
        }
    }

    nodes
}

fn vault_node_label(node: &VaultNode, width: usize) -> String {
    let indent = "  ".repeat(node.depth);
    let marker = match node.kind {
        VaultNodeKind::Directory => {
            if node.expanded {
                "v "
            } else {
                "> "
            }
        }
        VaultNodeKind::File => "- ",
    };
    truncate_label(&format!("{indent}{marker}{}", node.label), width)
}

fn is_hidden_component(name: &str) -> bool {
    name.starts_with('.')
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
        available.min(rows.saturating_mul(ROW_HEIGHT))
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

fn button(label: &str, color: Color) -> Paragraph<'static> {
    Paragraph::new(format!("[{label}]")).style(Style::default().fg(color))
}

fn button_area(x: &mut u16, y: u16, label: &str) -> Rect {
    let width = label.len() as u16 + 2;
    let rect = Rect::new(*x, y, width, 1);
    *x = (*x).saturating_add(width + 1);
    rect
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

fn infer_kind(path: &Path, content: Option<&str>) -> ArtifactKind {
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
    use super::{
        build_vault_nodes, ArtifactEntry, ArtifactHandle, ArtifactKind, ArtifactOrigin,
        ArtifactSection, ArtifactSidebar, ArtifactSidebarAction, ArtifactSidebarState,
    };
    use ratatui::{backend::TestBackend, layout::Position, layout::Rect, Terminal};
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};

    fn entry(name: &str, origin: ArtifactOrigin) -> ArtifactEntry {
        ArtifactEntry {
            handle: match origin {
                ArtifactOrigin::Temporary => ArtifactHandle::Temporary(1),
                ArtifactOrigin::Saved => ArtifactHandle::Saved(PathBuf::from(name)),
                ArtifactOrigin::Memory => ArtifactHandle::Memory(PathBuf::from(name)),
                ArtifactOrigin::Vault => ArtifactHandle::Vault(PathBuf::from(name)),
            },
            name: name.to_string(),
            kind: ArtifactKind::Markdown,
            origin,
            content: Some("content".to_string()),
            path: None,
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

    #[test]
    fn header_click_returns_toggle_action() {
        let mut state = ArtifactSidebarState::default();
        let temporary = vec![entry("temp.md", ArtifactOrigin::Temporary)];
        let mut sidebar = ArtifactSidebar::new(&temporary, &[], &[], &[], false, &mut state);
        let mut terminal = Terminal::new(TestBackend::new(32, 20)).expect("terminal");
        terminal
            .draw(|frame| sidebar.render(frame, Rect::new(0, 0, 32, 20)))
            .expect("render");

        let action = state.action_at(Position::new(1, 0));
        assert!(matches!(
            action,
            Some(ArtifactSidebarAction::ToggleSection(
                ArtifactSection::Temporary
            ))
        ));
    }

    #[test]
    fn toggling_vault_dir_updates_collapsed_set() {
        let mut state = ArtifactSidebarState::default();
        let path = PathBuf::from("notes");

        state.toggle_vault_dir(&path);
        assert!(state.collapsed_vault_dirs.contains(&path));

        state.toggle_vault_dir(&path);
        assert!(!state.collapsed_vault_dirs.contains(&path));
    }

    #[test]
    fn empty_flat_sections_use_one_row_and_vault_gets_remaining_space() {
        let mut state = ArtifactSidebarState::default();
        let mut sidebar = ArtifactSidebar::new(&[], &[], &[], &[], true, &mut state);
        let mut terminal = Terminal::new(TestBackend::new(40, 30)).expect("terminal");

        terminal
            .draw(|frame| sidebar.render(frame, Rect::new(0, 0, 40, 30)))
            .expect("render");
        let rendered: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();

        assert_eq!(state.temp_body.expect("temp body").height, 1);
        assert_eq!(state.saved_body.expect("saved body").height, 1);
        assert_eq!(state.memory_body.expect("memory body").height, 1);
        assert!(state.vault_body.expect("vault body").height > 1);
        assert!(rendered.contains("Saved files"));
    }

    #[test]
    fn flat_sections_grow_by_card_height_to_three_cards() {
        let temporary = vec![
            entry("one.md", ArtifactOrigin::Temporary),
            entry("two.md", ArtifactOrigin::Temporary),
        ];
        let saved = vec![
            entry("a.md", ArtifactOrigin::Saved),
            entry("b.md", ArtifactOrigin::Saved),
            entry("c.md", ArtifactOrigin::Saved),
            entry("d.md", ArtifactOrigin::Saved),
        ];
        let memories = vec![entry("memory.md", ArtifactOrigin::Memory)];
        let vault = vec![entry("vault.md", ArtifactOrigin::Vault)];
        let mut state = ArtifactSidebarState::default();
        let mut sidebar =
            ArtifactSidebar::new(&temporary, &saved, &memories, &vault, true, &mut state);
        let mut terminal = Terminal::new(TestBackend::new(40, 40)).expect("terminal");

        terminal
            .draw(|frame| sidebar.render(frame, Rect::new(0, 0, 40, 40)))
            .expect("render");

        assert_eq!(state.temp_body.expect("temp body").height, 6);
        assert_eq!(state.saved_body.expect("saved body").height, 9);
        assert_eq!(state.memory_body.expect("memory body").height, 3);
        assert!(state.vault_body.expect("vault body").height > 1);
    }
}
