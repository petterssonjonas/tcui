#![allow(dead_code)]
use ratatui::{prelude::*, widgets::*, Frame};

pub struct QuitConfirmModal {
    title: String,
    message: String,
}

#[derive(Debug, Clone, Copy)]
pub struct QuitConfirmAreas {
    pub yes: Rect,
    pub no: Rect,
}

impl QuitConfirmModal {
    pub fn new() -> Self {
        Self {
            title: "Quit".to_string(),
            message: "Are you sure you want to quit?".to_string(),
        }
    }

    pub fn title(mut self, title: &str) -> Self {
        self.title = title.to_string();
        self
    }

    pub fn message(mut self, message: &str) -> Self {
        self.message = message.to_string();
        self
    }

    pub fn render(&self, f: &mut Frame) -> QuitConfirmAreas {
        let theme = crate::theme::active_theme();
        let area = f.area();
        let width = area.width.saturating_sub(4).min(76);
        let height = area.height.min(9);
        let popup_area = Rect::new(
            area.x + area.width.saturating_sub(width) / 2,
            area.y + area.height.saturating_sub(height) / 2,
            width,
            height,
        );
        f.render_widget(Clear, popup_area);
        f.render_widget(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} ", self.title))
                .border_style(Style::default().fg(theme.warning))
                .style(theme.panel_style()),
            popup_area,
        );
        let content = popup_area.inner(Margin::new(2, 1));
        f.render_widget(
            Paragraph::new(self.message.as_str())
                .wrap(Wrap { trim: true })
                .alignment(Alignment::Center)
                .style(Style::default().fg(theme.foreground)),
            Rect::new(
                content.x,
                content.y,
                content.width,
                content.height.saturating_sub(2),
            ),
        );
        let yes_width = 7_u16;
        let no_width = 6_u16;
        let gap = 5_u16;
        let start_x =
            popup_area.x + popup_area.width.saturating_sub(yes_width + gap + no_width) / 2;
        let button_y = popup_area.bottom().saturating_sub(2);
        let yes = Rect::new(start_x, button_y, yes_width, 1);
        let no = Rect::new(start_x + yes_width + gap, button_y, no_width, 1);
        f.render_widget(
            Paragraph::new(" [Y]es ").style(
                Style::default()
                    .fg(theme.warning)
                    .add_modifier(Modifier::BOLD),
            ),
            yes,
        );
        f.render_widget(
            Paragraph::new(" [N]o ").style(Style::default().fg(theme.accent)),
            no,
        );
        QuitConfirmAreas { yes, no }
    }
}
