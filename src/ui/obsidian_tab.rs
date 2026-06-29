use ratatui::{Frame, prelude::*, widgets::*, layout::Rect};
use crate::obsidian::Vault;

pub struct ObsidianTab<'a> {
    pub vault: &'a Vault,
    pub selected_path: Option<String>,
}

impl<'a> ObsidianTab<'a> {
    pub fn new(vault: &'a Vault) -> Self {
        Self {
            vault,
            selected_path: None,
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)].as_ref())
            .split(area);

        self.render_file_tree(f, chunks[0]);
        self.render_preview(f, chunks[1]);
    }

    fn render_file_tree(&self, f: &mut Frame, area: Rect) {
        let items = vec![
            Line::from(Span::raw("📁 Vault")),
            Line::from(Span::raw("  └─ README.md")),
        ];

        let list = Paragraph::new(items)
            .block(Block::default().title("Files [Ctrl+S]").borders(Borders::ALL))
            .scrollable(true);

        f.render_widget(list, area);
    }

    fn render_preview(&self, f: &mut Frame, area: Rect) {
        let content = if let Some(path) = &self.selected_path {
            format!("Preview: {}", path)
        } else {
            "Select a file to preview".to_string()
        };

        let paragraph = Paragraph::new(content)
            .block(Block::default().title("Preview").borders(Borders::ALL));

        f.render_widget(paragraph, area);
    }
}