use ratatui::{prelude::*, widgets::*, Frame};
use unicode_width::UnicodeWidthStr;

use crate::ui::app_tabs::{AppTabId, AppTabs};

const BRAND_WIDTH: u16 = 6;
const ADD_WIDTH: u16 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopBarAction {
    Select(AppTabId),
    Add,
    Close(AppTabId),
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
        let layout = self.layout(area);
        if layout.brand.width > 0 {
            f.render_widget(
                Paragraph::new("TCUI").style(
                    Style::default()
                        .fg(theme.accent)
                        .bg(theme.background)
                        .add_modifier(Modifier::BOLD),
                ),
                layout.brand,
            );
        }
        for tab in &layout.tabs {
            let Some(descriptor) = self.tabs.tab(tab.id) else {
                continue;
            };
            let style = if tab.id == self.tabs.active_id() {
                theme.selected_style()
            } else {
                Style::default().fg(theme.foreground).bg(theme.panel)
            };
            f.render_widget(Block::default().style(style), tab.area);
            let label_width = tab
                .close
                .map_or(tab.area.width, |close| close.x.saturating_sub(tab.area.x));
            f.render_widget(
                Paragraph::new(descriptor.view().title())
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
        if layout.add.width > 0 {
            f.render_widget(
                Paragraph::new("+")
                    .style(Style::default().fg(theme.foreground).bg(theme.panel))
                    .alignment(Alignment::Center),
                layout.add,
            );
        }
    }

    pub fn hit_areas(&self, area: Rect) -> Vec<TopBarHitArea> {
        let layout = self.layout(area);
        let mut hits = Vec::with_capacity(layout.tabs.len() * 2 + 1);
        for tab in layout.tabs {
            hits.push(TopBarHitArea {
                action: TopBarAction::Select(tab.id),
                area: tab.area,
            });
            if let Some(close) = tab.close {
                hits.push(TopBarHitArea {
                    action: TopBarAction::Close(tab.id),
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
        let Some(active_index) = self
            .tabs
            .tabs()
            .iter()
            .position(|tab| tab.id() == self.tabs.active_id())
        else {
            return TopBarLayout::empty(area);
        };
        if area.width == 0 {
            return TopBarLayout::empty(area);
        }

        let add_width = if area.width >= 2 {
            ADD_WIDTH.min(area.width.saturating_sub(1))
        } else {
            0
        };
        let active_width = self.tab_width(active_index);
        let brand_width = if area.width
            >= BRAND_WIDTH
                .saturating_add(add_width)
                .saturating_add(active_width)
        {
            BRAND_WIDTH
        } else {
            0
        };
        let tab_budget = area
            .width
            .saturating_sub(brand_width)
            .saturating_sub(add_width);
        let active_render_width = active_width.min(tab_budget);
        let mut remaining = tab_budget.saturating_sub(active_render_width);
        let mut visible = vec![(active_index, active_render_width)];
        let mut left = active_index;
        let mut right = active_index.saturating_add(1);

        loop {
            let mut added = false;
            if left > 0 {
                let candidate = left - 1;
                let width = self.tab_width(candidate);
                if width <= remaining {
                    visible.push((candidate, width));
                    remaining = remaining.saturating_sub(width);
                    left = candidate;
                    added = true;
                }
            }
            if right < self.tabs.tabs().len() {
                let width = self.tab_width(right);
                if width <= remaining {
                    visible.push((right, width));
                    remaining = remaining.saturating_sub(width);
                    right = right.saturating_add(1);
                    added = true;
                }
            }
            if !added {
                break;
            }
        }
        visible.sort_unstable_by_key(|(index, _)| *index);

        let brand = Rect::new(area.x, area.y, brand_width, area.height);
        let mut x = area.x.saturating_add(brand_width);
        let mut tabs = Vec::with_capacity(visible.len());
        for (index, width) in visible {
            let descriptor = &self.tabs.tabs()[index];
            let tab_area = Rect::new(x, area.y, width, area.height);
            let close = (!descriptor.view().is_chat() && width >= 4)
                .then(|| Rect::new(tab_area.right().saturating_sub(2), area.y, 2, area.height));
            tabs.push(TabLayout {
                id: descriptor.id(),
                area: tab_area,
                close,
            });
            x = x.saturating_add(width);
        }
        let add = Rect::new(x, area.y, add_width, area.height);
        TopBarLayout { brand, tabs, add }
    }

    fn tab_width(&self, index: usize) -> u16 {
        let tab = &self.tabs.tabs()[index];
        let title_width = u16::try_from(tab.view().title().width()).unwrap_or(u16::MAX);
        title_width.saturating_add(if tab.view().is_chat() { 2 } else { 4 })
    }
}

struct TopBarLayout {
    brand: Rect,
    tabs: Vec<TabLayout>,
    add: Rect,
}

impl TopBarLayout {
    fn empty(area: Rect) -> Self {
        Self {
            brand: Rect::new(area.x, area.y, 0, area.height),
            tabs: Vec::new(),
            add: Rect::new(area.x, area.y, 0, area.height),
        }
    }
}

struct TabLayout {
    id: AppTabId,
    area: Rect,
    close: Option<Rect>,
}

#[cfg(test)]
mod tests {
    use super::{TopBar, TopBarAction};
    use crate::ui::app_tabs::{AppTabs, SelectTabResult};
    use ratatui::{backend::TestBackend, layout::Rect, Terminal};

    #[test]
    fn app_tab_id_top_bar_renders_launcher_and_add_control() {
        let mut tabs = AppTabs::default();
        tabs.add_launcher();
        let top_bar = TopBar::new(&tabs);
        let mut terminal = Terminal::new(TestBackend::new(64, 1)).expect("terminal");

        terminal
            .draw(|frame| top_bar.render(frame, Rect::new(0, 0, 64, 1)))
            .expect("render top bar");

        let screen = terminal.backend().to_string();
        assert!(screen.contains("TCUI"));
        assert!(screen.contains("Chat"));
        assert!(screen.contains("Launcher"));
        assert!(screen.contains('+'));
    }

    #[test]
    fn app_tab_id_top_bar_hit_areas_carry_stable_identities() {
        let mut tabs = AppTabs::default();
        let chat_id = tabs.active_id();
        let launcher_id = tabs.add_launcher();
        let hits = TopBar::new(&tabs).hit_areas(Rect::new(0, 0, 64, 1));

        assert!(hits
            .iter()
            .any(|hit| hit.action == TopBarAction::Select(chat_id)));
        assert!(hits
            .iter()
            .any(|hit| hit.action == TopBarAction::Select(launcher_id)));
        assert!(hits
            .iter()
            .any(|hit| hit.action == TopBarAction::Close(launcher_id)));
        assert!(hits.iter().any(|hit| hit.action == TopBarAction::Add));
        assert!(!hits
            .iter()
            .any(|hit| hit.action == TopBarAction::Close(chat_id)));
    }

    #[test]
    fn app_tab_id_stale_hit_area_cannot_select_replacement_tab() {
        let mut tabs = AppTabs::default();
        let stale_id = tabs.add_launcher();
        let stale_action = TopBar::new(&tabs)
            .hit_areas(Rect::new(0, 0, 64, 1))
            .into_iter()
            .find_map(|hit| (hit.action == TopBarAction::Select(stale_id)).then_some(hit.action))
            .expect("launcher select action");
        let _ = tabs.close(stale_id);
        let replacement_id = tabs.add_launcher();

        let TopBarAction::Select(stale_id) = stale_action else {
            panic!("expected stale select action");
        };
        let result = tabs.select(stale_id);

        assert_eq!(result, SelectTabResult::Unknown(stale_id));
        assert_eq!(tabs.active_id(), replacement_id);
    }

    #[test]
    fn app_tab_id_active_tab_stays_visible_at_minimum_width() {
        let mut tabs = AppTabs::default();
        for _ in 0..8 {
            tabs.add_launcher();
        }
        let active_id = tabs.active_id();
        let top_bar = TopBar::new(&tabs);
        let hits = top_bar.hit_areas(Rect::new(0, 0, 8, 1));
        let mut terminal = Terminal::new(TestBackend::new(8, 1)).expect("terminal");

        terminal
            .draw(|frame| top_bar.render(frame, Rect::new(0, 0, 8, 1)))
            .expect("render narrow top bar");

        let active_area = hits
            .iter()
            .find_map(|hit| (hit.action == TopBarAction::Select(active_id)).then_some(hit.area))
            .expect("active tab hit area");
        assert!(active_area.width > 0);
        assert!(hits.iter().any(|hit| hit.action == TopBarAction::Add));
        assert!(terminal.backend().buffer()[(active_area.x, active_area.y)]
            .modifier
            .contains(ratatui::style::Modifier::BOLD));
    }

    #[test]
    fn app_tab_id_tiny_widths_do_not_underflow_or_hide_active_tab() {
        let mut tabs = AppTabs::default();
        tabs.add_launcher();
        let active_id = tabs.active_id();

        for width in 1..=8 {
            let area = Rect::new(40, 3, width, 1);
            let hits = TopBar::new(&tabs).hit_areas(area);
            assert!(hits.iter().all(|hit| hit.area.right() <= area.right()));
            assert!(hits.iter().any(|hit| {
                hit.action == TopBarAction::Select(active_id) && hit.area.width > 0
            }));
            if width >= 2 {
                assert!(hits.iter().any(|hit| hit.action == TopBarAction::Add));
            }
        }
    }
}
