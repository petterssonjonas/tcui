use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use super::{PickerFocus, VaultDirectoryPickerState};

pub(in crate::tui::vault_profile_ui) struct PickerAreas {
    pub(super) path: Rect,
    pub(super) list: Rect,
    pub(super) add: Rect,
}

pub(in crate::tui::vault_profile_ui) fn picker_areas(area: Rect) -> Option<PickerAreas> {
    if area.width < 4 || area.height < 7 {
        return None;
    }
    let inner = area.inner(ratatui::layout::Margin::new(1, 1));
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(2),
        ])
        .split(inner);
    let add_width = 9_u16.min(rows[2].width);
    Some(PickerAreas {
        path: rows[0],
        list: rows[1],
        add: Rect::new(
            rows[2]
                .x
                .saturating_add(rows[2].width.saturating_sub(add_width) / 2),
            rows[2].y.saturating_add(1),
            add_width,
            1,
        ),
    })
}

impl VaultDirectoryPickerState {
    pub(in crate::tui::vault_profile_ui) fn render(&self, frame: &mut Frame, area: Rect) {
        let Some(areas) = picker_areas(area) else {
            return;
        };
        let theme = crate::theme::active_theme();
        frame.render_widget(
            Block::default()
                .borders(Borders::ALL)
                .title(" Select vault directory ")
                .style(Style::default().fg(theme.foreground).bg(theme.panel))
                .border_style(Style::default().fg(theme.accent)),
            area,
        );

        let path_columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(0), Constraint::Length(11)])
            .split(Rect::new(areas.path.x, areas.path.y, areas.path.width, 1));
        let input_style = if self.focus == PickerFocus::Path {
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.foreground)
        };
        let cursor = self.path_input.cursor();
        let query = self.path_input.query();
        let (before_cursor, after_cursor) = query.split_at(cursor);
        let cursor_text = after_cursor
            .chars()
            .next()
            .map_or(" ".to_owned(), |character| character.to_string());
        let after_cursor = after_cursor
            .strip_prefix(&cursor_text)
            .unwrap_or(after_cursor);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("Path: ", Style::default().fg(theme.muted)),
                Span::styled(before_cursor, input_style),
                Span::styled(
                    cursor_text,
                    if self.focus == PickerFocus::Path {
                        Style::default().fg(theme.panel).bg(theme.accent)
                    } else {
                        input_style
                    },
                ),
                Span::styled(after_cursor, input_style),
            ])),
            path_columns[0],
        );
        frame.render_widget(
            Paragraph::new("Esc close")
                .alignment(Alignment::Right)
                .style(Style::default().fg(theme.muted)),
            path_columns[1],
        );
        frame.render_widget(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(theme.muted)),
            areas.path,
        );

        let visible_height = usize::from(areas.list.height);
        let start = self.visible_start(visible_height);
        let lines = self
            .entries
            .iter()
            .enumerate()
            .skip(start)
            .take(visible_height)
            .map(|(index, entry)| {
                let selected = self.selected == Some(index);
                let style = if selected {
                    Style::default().fg(theme.panel).bg(theme.accent)
                } else {
                    Style::default().fg(theme.foreground)
                };
                Line::styled(
                    format!(" {} {}", if selected { "›" } else { " " }, entry.label),
                    style,
                )
            })
            .collect::<Vec<_>>();
        frame.render_widget(Paragraph::new(lines), areas.list);

        let add_style = if self.focus == PickerFocus::Add {
            Style::default()
                .fg(theme.panel)
                .bg(theme.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.accent)
        };
        let footer = Rect::new(
            areas
                .add
                .x
                .saturating_sub(areas.add.x.saturating_sub(area.x.saturating_add(1))),
            areas.add.y.saturating_sub(1),
            area.width.saturating_sub(2),
            2,
        );
        frame.render_widget(
            Block::default()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(theme.muted)),
            footer,
        );
        frame.render_widget(
            Paragraph::new("[ Add ]")
                .alignment(Alignment::Center)
                .style(add_style),
            areas.add,
        );
    }
}
