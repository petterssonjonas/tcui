use super::*;

impl SettingsPopup {
pub(super) fn render_mcp(&mut self, f: &mut Frame, area: Rect) {
    self.mcp_hit_areas = McpHitAreas::default();
    let visible = area.height.saturating_sub(2) as usize;
    if visible == 0 {
        return;
    }
    self.mcp_focus = self.mcp_focus.min(self.mcp_servers.len().saturating_sub(1));
    let start = self
        .mcp_focus
        .saturating_sub(visible.saturating_sub(1))
        .min(self.mcp_servers.len().saturating_sub(visible));
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            self.mcp_servers
                .iter()
                .skip(start)
                .take(visible)
                .map(|_| Constraint::Length(1))
                .collect::<Vec<_>>(),
        )
        .margin(1)
        .split(area);

    for ((idx, server), row) in self
        .mcp_servers
        .iter()
        .enumerate()
        .skip(start)
        .zip(rows.iter())
    {
        let focused = idx == self.mcp_focus;
        let line = Line::from(vec![
            Span::raw(if server.enabled { "[x] " } else { "[ ] " }),
            Span::styled(
                server.name.clone(),
                if focused {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                },
            ),
        ]);
        f.render_widget(
            Paragraph::new(line).style(if focused {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            }),
            *row,
        );
        self.mcp_hit_areas.rows.push((idx, *row));
    }
}
}
