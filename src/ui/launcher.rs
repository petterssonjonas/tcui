use ratatui::{prelude::*, widgets::*, Frame};

const FEATURES: [&str; 9] = [
    "Obsidian",
    "Memory management",
    "Settings",
    "System",
    "Research",
    "TODO",
    "Kanban",
    "News",
    "Pomodoro",
];

pub fn render(f: &mut Frame, area: Rect) {
    let theme = crate::theme::active_theme();
    f.render_widget(
        Block::default().style(Style::default().bg(theme.background)),
        area,
    );

    let row_height = if area.height >= 19 { 3 } else { 2 };
    let total_height = row_height * 5 + 4;
    let width = area.width.saturating_sub(4).min(60);
    let grid = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(total_height) / 2,
        width,
        total_height.min(area.height),
    );
    let column_gap = 2;
    let column_width = grid.width.saturating_sub(column_gap) / 2;

    for row in 0..4_u16 {
        let y = grid.y + row * (row_height + 1);
        for column in 0..2_u16 {
            let index = usize::from(row * 2 + column);
            let x = grid.x + column * (column_width + column_gap);
            render_card(
                f,
                Rect::new(x, y, column_width, row_height),
                FEATURES[index],
            );
        }
    }

    let last_y = grid.y + 4 * (row_height + 1);
    render_card(
        f,
        Rect::new(grid.x, last_y, grid.width, row_height),
        FEATURES[8],
    );
}

fn render_card(f: &mut Frame, area: Rect, label: &'static str) {
    let theme = crate::theme::active_theme();
    f.render_widget(
        Block::default().style(Style::default().bg(theme.panel)),
        area,
    );
    let label_area = Rect::new(
        area.x,
        area.y + area.height.saturating_sub(1) / 2,
        area.width,
        1.min(area.height),
    );
    f.render_widget(
        Paragraph::new(label)
            .style(Style::default().fg(theme.foreground).bg(theme.panel))
            .alignment(Alignment::Center),
        label_area,
    );
}

#[cfg(test)]
mod tests {
    use ratatui::{backend::TestBackend, layout::Rect, Terminal};

    #[test]
    fn launcher_renders_all_feature_cards_at_minimum_size() {
        let mut terminal = Terminal::new(TestBackend::new(64, 15)).expect("terminal");

        terminal
            .draw(|frame| super::render(frame, Rect::new(0, 0, 64, 15)))
            .expect("render launcher");

        let screen = terminal.backend().to_string();
        for label in super::FEATURES {
            assert!(screen.contains(label), "missing launcher card: {label}");
        }
    }
}
