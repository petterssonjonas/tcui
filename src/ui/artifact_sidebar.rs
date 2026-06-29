use std::path::{Path, PathBuf};

use ratatui::{prelude::*, widgets::*, Frame};

const ROW_HEIGHT: u16 = 3;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ArtifactHandle {
    Temporary(u64),
    Media(String),
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

    pub fn save_label(&self, has_vault: bool) -> &'static str {
        match self.origin {
            ArtifactOrigin::Temporary if self.is_markdown() && has_vault => "Vault",
            ArtifactOrigin::Temporary => "Save",
            ArtifactOrigin::Vault => "",
        }
    }

    pub fn can_save(&self, _has_vault: bool) -> bool {
        matches!(self.origin, ArtifactOrigin::Temporary)
    }

    pub fn can_delete(&self) -> bool {
        true
    }

    pub fn origin_label(&self) -> &'static str {
        match self.origin {
            ArtifactOrigin::Temporary => "Temporary",
            ArtifactOrigin::Vault => "Vault",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ArtifactSection {
    Temporary,
    Vault,
}

#[derive(Debug, Clone)]
pub enum ArtifactSidebarAction {
    Open(ArtifactHandle),
    Save(ArtifactHandle),
    Delete(ArtifactHandle),
}

#[derive(Debug, Clone)]
struct ArtifactRowHitAreas {
    handle: ArtifactHandle,
    open: Rect,
    save: Option<Rect>,
    delete: Option<Rect>,
}

#[derive(Debug, Default, Clone)]
pub struct ArtifactSidebarState {
    pub temp_scroll: usize,
    pub vault_scroll: usize,
    pub temp_body: Option<Rect>,
    pub vault_body: Option<Rect>,
    temp_rows: Vec<ArtifactRowHitAreas>,
    vault_rows: Vec<ArtifactRowHitAreas>,
}

impl ArtifactSidebarState {
    pub fn action_at(&self, pos: Position) -> Option<ArtifactSidebarAction> {
        for row in self.temp_rows.iter().chain(self.vault_rows.iter()) {
            if row.open.contains(pos) {
                return Some(ArtifactSidebarAction::Open(row.handle.clone()));
            }
            if row.save.is_some_and(|area| area.contains(pos)) {
                return Some(ArtifactSidebarAction::Save(row.handle.clone()));
            }
            if row.delete.is_some_and(|area| area.contains(pos)) {
                return Some(ArtifactSidebarAction::Delete(row.handle.clone()));
            }
        }
        None
    }

    pub fn section_at(&self, pos: Position) -> Option<ArtifactSection> {
        if self.temp_body.is_some_and(|area| area.contains(pos)) {
            return Some(ArtifactSection::Temporary);
        }
        if self.vault_body.is_some_and(|area| area.contains(pos)) {
            return Some(ArtifactSection::Vault);
        }
        None
    }

    pub fn scroll(&mut self, section: ArtifactSection, down: bool, total: usize, visible: usize) {
        let max_offset = total.saturating_sub(visible.max(1));
        let offset = match section {
            ArtifactSection::Temporary => &mut self.temp_scroll,
            ArtifactSection::Vault => &mut self.vault_scroll,
        };
        if down {
            *offset = (*offset + 1).min(max_offset);
        } else {
            *offset = offset.saturating_sub(1);
        }
    }
}

pub struct ArtifactSidebar<'a> {
    temporary: &'a [ArtifactEntry],
    vault: &'a [ArtifactEntry],
    has_vault: bool,
    state: &'a mut ArtifactSidebarState,
}

impl<'a> ArtifactSidebar<'a> {
    pub fn new(
        temporary: &'a [ArtifactEntry],
        vault: &'a [ArtifactEntry],
        has_vault: bool,
        state: &'a mut ArtifactSidebarState,
    ) -> Self {
        Self {
            temporary,
            vault,
            has_vault,
            state,
        }
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect) {
        self.state.temp_rows.clear();
        self.state.vault_rows.clear();

        let block = Block::default()
            .title(" Artifacts ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .style(Style::default().bg(Color::Black));
        let inner = block.inner(area);
        f.render_widget(block, area);

        let temp_height = (inner.height.saturating_sub(3))
            .min(inner.height / 2)
            .max(6);
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(temp_height),
                Constraint::Length(1),
                Constraint::Min(4),
            ])
            .split(inner);

        f.render_widget(
            Paragraph::new(" Temporary").style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            chunks[0],
        );
        self.state.temp_scroll = self.state.temp_scroll.min(
            self.temporary
                .len()
                .saturating_sub(Self::visible_rows(chunks[1])),
        );
        self.state.temp_body = Some(chunks[1]);
        Self::render_section(
            f,
            chunks[1],
            self.temporary,
            self.state.temp_scroll,
            self.has_vault,
            &mut self.state.temp_rows,
        );

        f.render_widget(
            Paragraph::new(" Vault")
                .style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
                .block(
                    Block::default()
                        .borders(Borders::TOP)
                        .border_style(Style::default().fg(Color::DarkGray)),
                ),
            chunks[2],
        );
        self.state.vault_scroll = self.state.vault_scroll.min(
            self.vault
                .len()
                .saturating_sub(Self::visible_rows(chunks[3])),
        );
        self.state.vault_body = Some(chunks[3]);
        Self::render_section(
            f,
            chunks[3],
            self.vault,
            self.state.vault_scroll,
            self.has_vault,
            &mut self.state.vault_rows,
        );
    }

    pub fn visible_rows(area: Rect) -> usize {
        usize::from(area.height / ROW_HEIGHT).max(1)
    }

    fn render_section(
        f: &mut Frame,
        area: Rect,
        entries: &[ArtifactEntry],
        scroll: usize,
        has_vault: bool,
        hit_areas: &mut Vec<ArtifactRowHitAreas>,
    ) {
        f.render_widget(
            Paragraph::new("").style(Style::default().bg(Color::Black)),
            area,
        );
        if entries.is_empty() {
            f.render_widget(
                Paragraph::new(" No files yet")
                    .style(Style::default().fg(Color::DarkGray))
                    .alignment(Alignment::Center),
                area,
            );
            return;
        }

        let visible = Self::visible_rows(area);
        let scroll = scroll.min(entries.len().saturating_sub(visible));
        for (row_idx, entry) in entries.iter().skip(scroll).take(visible).enumerate() {
            let row_y = area.y + row_idx as u16 * ROW_HEIGHT;
            let row_area = Rect::new(area.x, row_y, area.width, ROW_HEIGHT);
            Self::render_row(f, row_area, entry, has_vault, hit_areas);
        }
    }

    fn render_row(
        f: &mut Frame,
        area: Rect,
        entry: &ArtifactEntry,
        has_vault: bool,
        hit_areas: &mut Vec<ArtifactRowHitAreas>,
    ) {
        let title = truncate_label(&entry.name, area.width.saturating_sub(2) as usize);
        let meta = format!(" {} · {}", entry.origin_label(), kind_label(entry.kind));
        f.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    format!(" {title}"),
                    Style::default().fg(Color::White),
                )),
                Line::from(Span::styled(meta, Style::default().fg(Color::DarkGray))),
            ]),
            Rect::new(area.x, area.y, area.width, 2),
        );

        let mut x = area.x + 1;
        let y = area.y + 2;
        let open = button_area(&mut x, y, "Open");
        f.render_widget(button("Open", Color::Cyan), open);

        let save = if entry.can_save(has_vault) {
            let label = entry.save_label(has_vault);
            let rect = button_area(&mut x, y, label);
            f.render_widget(button(label, Color::Green), rect);
            Some(rect)
        } else {
            None
        };

        let delete = if entry.can_delete() {
            let rect = button_area(&mut x, y, "Del");
            f.render_widget(button("Del", Color::Red), rect);
            Some(rect)
        } else {
            None
        };

        hit_areas.push(ArtifactRowHitAreas {
            handle: entry.handle.clone(),
            open,
            save,
            delete,
        });
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
    *x = x.saturating_add(width + 1);
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
