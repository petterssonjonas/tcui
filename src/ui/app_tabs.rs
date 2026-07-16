const MAX_PLACEHOLDER_TABS: u32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppView {
    Chat,
    Placeholder { number: u32 },
}

impl AppView {
    pub const fn is_chat(self) -> bool {
        matches!(self, Self::Chat)
    }

    pub fn title(self) -> String {
        match self {
            Self::Chat => "Chat".to_string(),
            Self::Placeholder { number } => format!("Tab {number}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppTabs {
    tabs: Vec<AppView>,
    active: usize,
}

impl Default for AppTabs {
    fn default() -> Self {
        Self {
            tabs: vec![AppView::Chat],
            active: 0,
        }
    }
}

impl AppTabs {
    pub fn views(&self) -> &[AppView] {
        &self.tabs
    }

    pub const fn active_index(&self) -> usize {
        self.active
    }

    pub fn active_view(&self) -> AppView {
        self.tabs[self.active]
    }

    pub fn select(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active = index;
        }
    }

    pub fn add_placeholder(&mut self) {
        let Some(number) = (1..=MAX_PLACEHOLDER_TABS).find(|candidate| {
            !self
                .tabs
                .contains(&AppView::Placeholder { number: *candidate })
        }) else {
            return;
        };
        let view = AppView::Placeholder { number };
        self.tabs.push(view);
        self.active = self.tabs.len() - 1;
    }

    pub fn close(&mut self, index: usize) {
        if index == 0 || index >= self.tabs.len() {
            return;
        }
        self.tabs.remove(index);
        if self.active > index {
            self.active -= 1;
        } else if self.active == index {
            self.active = index.min(self.tabs.len() - 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AppTabs, AppView};

    #[test]
    fn placeholder_tabs_are_sequential_and_chat_cannot_close() {
        let mut tabs = AppTabs::default();

        tabs.add_placeholder();
        tabs.add_placeholder();
        tabs.close(0);

        assert_eq!(
            tabs.views(),
            &[
                AppView::Chat,
                AppView::Placeholder { number: 1 },
                AppView::Placeholder { number: 2 },
            ]
        );
        assert_eq!(tabs.active_view(), AppView::Placeholder { number: 2 });
    }

    #[test]
    fn closing_active_placeholder_selects_nearest_remaining_tab() {
        let mut tabs = AppTabs::default();
        tabs.add_placeholder();
        tabs.add_placeholder();

        tabs.close(2);

        assert_eq!(tabs.active_view(), AppView::Placeholder { number: 1 });
    }

    #[test]
    fn closed_placeholder_number_is_reused_within_visible_range() {
        let mut tabs = AppTabs::default();
        for _ in 0..5 {
            tabs.add_placeholder();
        }
        tabs.close(3);

        tabs.add_placeholder();

        assert_eq!(tabs.views().len(), 6);
        assert!(tabs.views().contains(&AppView::Placeholder { number: 3 }));
        assert!(tabs.views().iter().all(|view| match view {
            AppView::Chat => true,
            AppView::Placeholder { number } => *number <= 5,
        }));
    }
}
