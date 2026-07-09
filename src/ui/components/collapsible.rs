use ratatui::{layout::Rect, prelude::*, widgets::*, Frame};

pub struct Collapsible<'a> {
    pub title: &'a str,
    pub content: &'a str,
    pub collapsed: bool,
    pub char_collapsed: char,
    pub char_expanded: char,
}

impl<'a> Collapsible<'a> {
    pub fn new(title: &'a str, content: &'a str) -> Self {
        Self {
            title,
            content,
            collapsed: true,
            char_collapsed: '▸',
            char_expanded: '▾',
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let theme = crate::theme::active_theme();
        let indicator = if self.collapsed {
            self.char_collapsed
        } else {
            self.char_expanded
        };
        let title = format!(" {indicator} {} ", self.title);
        let block_area = Rect::new(
            area.x.saturating_add(area.width / 10),
            area.y,
            area.width.saturating_mul(8) / 10,
            if self.collapsed {
                area.height.min(1)
            } else {
                area.height
            },
        );

        let text = if self.collapsed { "" } else { self.content };

        let paragraph = Paragraph::new(text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .title_style(
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    )
                    .border_style(Style::default().fg(theme.border).bg(theme.code_bg))
                    .style(Style::default().fg(theme.foreground).bg(theme.code_bg)),
            )
            .style(Style::default().fg(theme.foreground).bg(theme.code_bg))
            .wrap(Wrap { trim: true });

        f.render_widget(paragraph, block_area);
    }
}
