use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{prelude::*, widgets::*, Frame};

use crate::ui::artifact_sidebar::{ArtifactEntry, ArtifactHandle};

#[derive(Debug, Clone, Copy)]
pub(crate) enum MemoryManagerAction {
    Select(usize),
    View,
    Edit,
    Copy,
    Delete,
    Context,
    Transfer,
    Refresh,
}

#[derive(Default)]
pub(crate) struct MemoryManagerState {
    selected: usize,
    selected_id: Option<u64>,
    scroll: usize,
    pub(crate) hit_areas: Vec<(Rect, MemoryManagerAction)>,
}

impl MemoryManagerState {
    pub(crate) fn reconcile(&mut self, entries: &[ArtifactEntry]) {
        if let Some(index) = entries.iter().position(|entry| matches!(entry.handle, ArtifactHandle::Memory(id) if Some(id) == self.selected_id)) {
            self.selected = index;
        }
        self.select(self.selected, entries);
    }

    pub(crate) fn select(&mut self, index: usize, entries: &[ArtifactEntry]) {
        self.selected = index.min(entries.len().saturating_sub(1));
        self.selected_id = entries
            .get(self.selected)
            .and_then(|entry| match entry.handle {
                ArtifactHandle::Memory(id) => Some(id),
                _ => None,
            });
    }

    pub(crate) fn selected(&self, entries: &[ArtifactEntry]) -> Option<ArtifactHandle> {
        entries.get(self.selected).map(|entry| entry.handle.clone())
    }

    pub(crate) fn key(
        &mut self,
        key: KeyEvent,
        entries: &[ArtifactEntry],
    ) -> Option<MemoryManagerAction> {
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return None;
        }
        match key.code {
            KeyCode::Up => self.select(self.selected.saturating_sub(1), entries),
            KeyCode::Down => self.select(self.selected.saturating_add(1), entries),
            KeyCode::PageUp => self.select(self.selected.saturating_sub(10), entries),
            KeyCode::PageDown => self.select(self.selected.saturating_add(10), entries),
            KeyCode::Home => self.select(0, entries),
            KeyCode::End => self.select(entries.len().saturating_sub(1), entries),
            KeyCode::Enter => return Some(MemoryManagerAction::View),
            KeyCode::Char('e') => return Some(MemoryManagerAction::Edit),
            KeyCode::Char('c') => return Some(MemoryManagerAction::Copy),
            KeyCode::Delete | KeyCode::Char('d') => return Some(MemoryManagerAction::Delete),
            KeyCode::Char('+') => return Some(MemoryManagerAction::Context),
            KeyCode::Char('i') => return Some(MemoryManagerAction::Transfer),
            KeyCode::Char('r') => return Some(MemoryManagerAction::Refresh),
            _ => {}
        }
        None
    }

    pub(crate) fn render(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        entries: &[ArtifactEntry],
        notice: Option<&str>,
    ) {
        self.hit_areas.clear();
        let theme = crate::theme::active_theme();
        let layout = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(2),
        ])
        .split(area);
        frame.render_widget(
            Block::default().style(Style::default().bg(theme.background)),
            area,
        );
        frame.render_widget(
            Paragraph::new(" Memories\n Saved memory content, independent from the Vault."),
            layout[0],
        );
        let list = Block::default().borders(Borders::ALL).title(" Entries ");
        let inner = list.inner(layout[1]);
        frame.render_widget(list, layout[1]);
        let access = crate::protection::require(crate::protection::Domain::Memories);
        if let Err(error) = access {
            frame.render_widget(
                Paragraph::new(format!(
                    "{error}\nPress i to open unlock and transfer options."
                ))
                .wrap(Wrap { trim: false }),
                inner,
            );
        } else if let Some(notice) = notice {
            frame.render_widget(
                Paragraph::new(format!(
                    "{notice}\nPress i to unlock or adopt copied encrypted files."
                ))
                .wrap(Wrap { trim: false }),
                inner,
            );
        } else if entries.is_empty() {
            frame.render_widget(
                Paragraph::new("No memories. Press i to import native records or Markdown."),
                inner,
            );
        } else {
            let visible = usize::from(inner.height);
            self.scroll = self
                .scroll
                .min(self.selected)
                .max(self.selected.saturating_sub(visible.saturating_sub(1)));
            for (index, entry) in entries.iter().enumerate().skip(self.scroll).take(visible) {
                let row = Rect::new(
                    inner.x,
                    inner.y + (index - self.scroll) as u16,
                    inner.width,
                    1,
                );
                frame.render_widget(
                    Paragraph::new(entry.name.as_str()).style(if index == self.selected {
                        Style::default()
                            .fg(theme.accent)
                            .add_modifier(Modifier::REVERSED)
                    } else {
                        Style::default().fg(theme.foreground)
                    }),
                    row,
                );
                self.hit_areas
                    .push((row, MemoryManagerAction::Select(index)));
            }
        }
        let mut x = layout[2].x;
        for (label, action) in [
            ("View", MemoryManagerAction::View),
            ("Edit", MemoryManagerAction::Edit),
            ("Copy", MemoryManagerAction::Copy),
            ("Delete", MemoryManagerAction::Delete),
            ("+ Context", MemoryManagerAction::Context),
            ("Import / export", MemoryManagerAction::Transfer),
        ] {
            let width = label.len() as u16 + 2;
            if width > layout[2].right().saturating_sub(x) {
                break;
            }
            let rect = Rect::new(x, layout[2].y, width, 1);
            frame.render_widget(
                Paragraph::new(format!("[{label}]")).style(Style::default().fg(theme.accent)),
                rect,
            );
            self.hit_areas.push((rect, action));
            x = x.saturating_add(width + 1);
        }
        frame.render_widget(Paragraph::new("Arrows: select  Enter: view  e: edit  c: copy  d: Trash  +: context  i: transfer  r: refresh").style(Style::default().fg(theme.muted)), Rect::new(layout[2].x, layout[2].y.saturating_add(1), layout[2].width, layout[2].height.saturating_sub(1)));
    }
}
