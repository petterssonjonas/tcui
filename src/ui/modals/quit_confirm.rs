#![allow(dead_code)]
use ratatui::{prelude::*, widgets::*, Frame};

pub struct QuitConfirmModal;

#[derive(Debug, Clone, Copy)]
pub struct QuitConfirmAreas {
    pub yes: Rect,
    pub no: Rect,
}

impl QuitConfirmModal {
    pub fn new() -> Self {
        Self
    }

    pub fn render(&self, f: &mut Frame) -> QuitConfirmAreas {
        let area = f.area();
        let popup_area = Self::centered_rect(40, 20, area);

        // Clear background
        f.render_widget(
            Block::default().style(Style::default().bg(Color::Black)),
            area,
        );

        let block = Block::default()
            .title(" Quit ")
            .title_style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Red))
            .style(Style::default().bg(Color::Black));

        let text = vec![
            Line::from(""),
            Line::from("Are you sure you want to quit?").alignment(Alignment::Center),
            Line::from(""),
            Line::from(vec![
                Span::styled(
                    " [Y]es ",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("     "),
                Span::styled(" [N]o ", Style::default().fg(Color::Red)),
            ])
            .alignment(Alignment::Center),
            Line::from(""),
        ];

        let paragraph = Paragraph::new(text)
            .block(block)
            .alignment(Alignment::Center);

        f.render_widget(Clear, popup_area);
        f.render_widget(paragraph, popup_area);

        // Compute button hit areas relative to popup
        let yes_width = 7u16; // " [Y]es "
        let no_width = 6u16; // " [N]o "
        let gap = 5u16;
        let total_width = yes_width + gap + no_width;
        let start_x = popup_area.x + (popup_area.width.saturating_sub(total_width)) / 2;
        let button_y = popup_area.y + 4; // line 4 of the popup content

        let yes = Rect::new(start_x, button_y, yes_width, 1);
        let no = Rect::new(start_x + yes_width + gap, button_y, no_width, 1);

        QuitConfirmAreas { yes, no }
    }

    fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
        let popup_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage((100 - percent_y) / 2),
                Constraint::Percentage(percent_y),
                Constraint::Percentage((100 - percent_y) / 2),
            ])
            .split(r);

        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage((100 - percent_x) / 2),
                Constraint::Percentage(percent_x),
                Constraint::Percentage((100 - percent_x) / 2),
            ])
            .split(popup_layout[1])[1]
    }
}
