#![allow(dead_code)]
use ratatui::{prelude::*, widgets::*, Frame};
use unicode_width::UnicodeWidthStr;

pub struct TopBar<'a> {
    pub tabs: &'a [crate::ui::ChatTabState],
    pub active: usize,
    pub sidebar_open: bool,
    pub artifact_sidebar_open: bool,
}

#[derive(Debug, Clone)]
pub struct TabHitArea {
    pub index: usize,
    pub area: Rect,
}

impl<'a> TopBar<'a> {
    pub fn new(
        tabs: &'a [crate::ui::ChatTabState],
        active: usize,
        sidebar_open: bool,
        artifact_sidebar_open: bool,
    ) -> Self {
        Self {
            tabs,
            active,
            sidebar_open,
            artifact_sidebar_open,
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let theme = crate::theme::active_theme();
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(3),
                Constraint::Length(3),
            ])
            .split(area);

        // Hamburger button
        let hamburger_text = if self.sidebar_open { "<" } else { ">" };
        let hamburger = Paragraph::new(hamburger_text)
            .style(Style::default().fg(theme.foreground).bg(theme.sidebar))
            .alignment(Alignment::Center)
            .block(Block::default());
        f.render_widget(hamburger, chunks[0]);

        // Tabs - render manually to track widths
        self.render_tabs(f, chunks[1]);

        let artifact_toggle_text = if self.artifact_sidebar_open { ">" } else { "<" };
        let artifact_toggle = Paragraph::new(artifact_toggle_text)
            .style(Style::default().fg(theme.foreground).bg(theme.sidebar))
            .alignment(Alignment::Center)
            .block(Block::default());
        f.render_widget(artifact_toggle, chunks[2]);

        // Close button
        let close = Paragraph::new(" x ")
            .style(Style::default().fg(theme.error).bg(theme.sidebar))
            .alignment(Alignment::Center)
            .block(Block::default());
        f.render_widget(close, chunks[3]);
    }

    fn render_tabs(&self, f: &mut Frame, area: Rect) {
        let theme = crate::theme::active_theme();
        let mut current_x = area.x;
        let divider = Span::styled("│", Style::default().fg(theme.border));
        let divider_width = 1u16;

        for (i, tab) in self.tabs.iter().enumerate() {
            let display_name = tab
                .generated_title
                .as_ref()
                .map(|gt| {
                    if gt.len() > 12 {
                        format!("{}...", &gt[..12])
                    } else {
                        gt.clone()
                    }
                })
                .unwrap_or_else(|| {
                    let name = &tab.tab.name;
                    if name.len() > 12 {
                        format!("{}...", &name[..12])
                    } else {
                        name.clone()
                    }
                });

            let label = if i == self.active {
                format!("[{}]", display_name)
            } else {
                format!(" {} ", display_name)
            };
            let width = label.width() as u16;

            let style = if i == self.active {
                theme.selected_style()
            } else {
                Style::default().fg(theme.muted).bg(theme.sidebar)
            };

            let tab_area = Rect::new(current_x, area.y, width, area.height);
            let paragraph = Paragraph::new(label)
                .style(style)
                .alignment(Alignment::Left);
            f.render_widget(paragraph, tab_area);

            current_x += width;

            // Render divider after each tab except the last
            if i < self.tabs.len() - 1 {
                let div_area = Rect::new(current_x, area.y, divider_width, area.height);
                let div_para = Paragraph::new(divider.clone().content)
                    .style(Style::default().fg(theme.border).bg(theme.sidebar));
                f.render_widget(div_para, div_area);
                current_x += divider_width;
            }
        }
    }

    pub fn hamburger_area(&self, area: Rect) -> Rect {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(3),
                Constraint::Length(3),
            ])
            .split(area);
        chunks[0]
    }

    pub fn artifact_toggle_area(&self, area: Rect) -> Rect {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(3),
                Constraint::Length(3),
            ])
            .split(area);
        chunks[2]
    }

    pub fn close_area(&self, area: Rect) -> Rect {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(3),
                Constraint::Length(3),
            ])
            .split(area);
        chunks[3]
    }

    pub fn tab_hit_areas(&self, area: Rect) -> Vec<TabHitArea> {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(3),
                Constraint::Length(3),
            ])
            .split(area);
        let tab_area = chunks[1];

        let mut current_x = tab_area.x;
        let divider_width = 1u16;
        let mut hit_areas = Vec::new();

        for (i, tab) in self.tabs.iter().enumerate() {
            let display_name = tab
                .generated_title
                .as_ref()
                .map(|gt| {
                    if gt.len() > 12 {
                        format!("{}...", &gt[..12])
                    } else {
                        gt.clone()
                    }
                })
                .unwrap_or_else(|| {
                    let name = &tab.tab.name;
                    if name.len() > 12 {
                        format!("{}...", &name[..12])
                    } else {
                        name.clone()
                    }
                });

            let label = if i == self.active {
                format!("[{}]", display_name)
            } else {
                format!(" {} ", display_name)
            };
            let width = label.width() as u16;

            hit_areas.push(TabHitArea {
                index: i,
                area: Rect::new(current_x, tab_area.y, width, tab_area.height),
            });

            current_x += width;
            if i < self.tabs.len() - 1 {
                current_x += divider_width;
            }
        }

        hit_areas
    }
}
