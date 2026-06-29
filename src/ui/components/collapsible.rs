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
        let indicator = if self.collapsed {
            self.char_collapsed
        } else {
            self.char_expanded
        };
        let title = format!("{} {}", indicator, self.title);

        let mut spans = vec![Span::raw(title), Span::raw("\n\n")];

        if !self.collapsed {
            spans.push(Span::raw(self.content));
        }

        let paragraph = Paragraph::new(Line::from(spans))
            .block(Block::default().borders(Borders::ALL))
            .wrap(Wrap { trim: true });

        f.render_widget(paragraph, area);
    }
}
