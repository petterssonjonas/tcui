use ratatui::{prelude::*, widgets::*, Frame};
use unicode_width::UnicodeWidthStr;

use crate::ui::app_tabs::AppTabs;

const BRAND_WIDTH: u16 = 6;
const ADD_WIDTH: u16 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopBarAction {
    Select(usize),
    Add,
    Close(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TopBarHitArea {
    pub action: TopBarAction,
    pub area: Rect,
}

pub struct TopBar<'a> {
    tabs: &'a AppTabs,
}

impl<'a> TopBar<'a> {
    pub const fn new(tabs: &'a AppTabs) -> Self {
        Self { tabs }
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let theme = crate::theme::active_theme();
        f.render_widget(
            Block::default().style(Style::default().fg(theme.foreground).bg(theme.background)),
            area,
        );
        f.render_widget(
            Paragraph::new("TCUI").style(
                Style::default()
                    .fg(theme.accent)
                    .bg(theme.background)
                    .add_modifier(Modifier::BOLD),
            ),
            Rect::new(area.x, area.y, BRAND_WIDTH.min(area.width), area.height),
        );

        let layout = self.layout(area);
        for tab in &layout.tabs {
            let style = if tab.index == self.tabs.active_index() {
                theme.selected_style()
            } else {
                Style::default().fg(theme.foreground).bg(theme.panel)
            };
            f.render_widget(Block::default().style(style), tab.area);
            let label_width = tab
                .close
                .map_or(tab.area.width, |close| close.x.saturating_sub(tab.area.x));
            f.render_widget(
                Paragraph::new(self.tabs.views()[tab.index].title())
                    .style(style)
                    .alignment(Alignment::Center),
                Rect::new(tab.area.x, tab.area.y, label_width, tab.area.height),
            );
            if let Some(close) = tab.close {
                f.render_widget(
                    Paragraph::new("x")
                        .style(style.fg(theme.muted))
                        .alignment(Alignment::Center),
                    close,
                );
            }
        }
        f.render_widget(
            Paragraph::new("+")
                .style(Style::default().fg(theme.foreground).bg(theme.panel))
                .alignment(Alignment::Center),
            layout.add,
        );
    }

    pub fn hit_areas(&self, area: Rect) -> Vec<TopBarHitArea> {
        let layout = self.layout(area);
        let mut hits = Vec::with_capacity(layout.tabs.len() * 2 + 1);
        for tab in layout.tabs {
            hits.push(TopBarHitArea {
                action: TopBarAction::Select(tab.index),
                area: tab.area,
            });
            if let Some(close) = tab.close {
                hits.push(TopBarHitArea {
                    action: TopBarAction::Close(tab.index),
                    area: close,
                });
            }
        }
        if layout.add.width > 0 {
            hits.push(TopBarHitArea {
                action: TopBarAction::Add,
                area: layout.add,
            });
        }
        hits
    }

    fn layout(&self, area: Rect) -> TopBarLayout {
        let right = area.right();
        let mut x = area.x.saturating_add(BRAND_WIDTH.min(area.width));
        let mut tabs = Vec::new();
        for (index, view) in self.tabs.views().iter().enumerate() {
            let title_width = view.title().width() as u16;
            let width = title_width.saturating_add(if index == 0 { 2 } else { 4 });
            if x.saturating_add(width).saturating_add(ADD_WIDTH) > right {
                break;
            }
            let area = Rect::new(x, area.y, width, area.height);
            let close = (index != 0).then(|| {
                Rect::new(
                    area.right().saturating_sub(2),
                    area.y,
                    2.min(area.width),
                    area.height,
                )
            });
            tabs.push(TabLayout { index, area, close });
            x = x.saturating_add(width);
        }
        let add = Rect::new(
            x,
            area.y,
            ADD_WIDTH.min(right.saturating_sub(x)),
            area.height,
        );
        TopBarLayout { tabs, add }
    }
}

struct TopBarLayout {
    tabs: Vec<TabLayout>,
    add: Rect,
}

struct TabLayout {
    index: usize,
    area: Rect,
    close: Option<Rect>,
}

#[cfg(test)]
mod tests {
    use super::{TopBar, TopBarAction};
    use crate::ui::app_tabs::AppTabs;
    use ratatui::{backend::TestBackend, layout::Rect, Terminal};

    #[test]
    fn top_bar_renders_chat_tabs_and_add_control_without_chat_title() {
        let mut tabs = AppTabs::default();
        tabs.add_placeholder();
        let top_bar = TopBar::new(&tabs);
        let mut terminal = Terminal::new(TestBackend::new(64, 1)).expect("terminal");

        terminal
            .draw(|frame| top_bar.render(frame, Rect::new(0, 0, 64, 1)))
            .expect("render top bar");

        let screen = terminal.backend().to_string();
        assert!(screen.contains("TCUI"));
        assert!(screen.contains("Chat"));
        assert!(screen.contains("Tab 1"));
        assert!(screen.contains('+'));
        assert!(!screen.contains("Generated conversation title"));
    }

    #[test]
    fn top_bar_exposes_distinct_add_select_and_close_hit_areas() {
        let mut tabs = AppTabs::default();
        tabs.add_placeholder();
        let hits = TopBar::new(&tabs).hit_areas(Rect::new(0, 0, 64, 1));

        assert!(hits.iter().any(|hit| hit.action == TopBarAction::Select(0)));
        assert!(hits.iter().any(|hit| hit.action == TopBarAction::Select(1)));
        assert!(hits.iter().any(|hit| hit.action == TopBarAction::Close(1)));
        assert!(hits.iter().any(|hit| hit.action == TopBarAction::Add));
        assert!(!hits.iter().any(|hit| hit.action == TopBarAction::Close(0)));
    }

    #[test]
    fn every_created_tab_is_selectable_at_minimum_width() {
        let mut tabs = AppTabs::default();
        for _ in 0..6 {
            tabs.add_placeholder();
        }

        let hits = TopBar::new(&tabs).hit_areas(Rect::new(0, 0, 64, 1));

        for index in 0..tabs.views().len() {
            assert!(
                hits.iter()
                    .any(|hit| hit.action == TopBarAction::Select(index)),
                "tab {index} has no selectable hit area"
            );
        }
    }
}
