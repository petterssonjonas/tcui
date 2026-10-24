use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_APP_TAB_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AppTabId(u64);

impl AppTabId {
    pub(crate) fn next() -> Self {
        match NEXT_APP_TAB_ID.try_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            current.checked_add(1)
        }) {
            Ok(id) => Self(id),
            Err(_) => std::process::abort(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppView {
    Chat,
    Launcher,
    Terminal {
        session_id: u64,
    },
    Settings,
    Vault,
    #[cfg(feature = "memory")]
    Memories,
}

impl AppView {
    pub const fn is_chat(&self) -> bool {
        matches!(self, Self::Chat)
    }

    pub const fn title(&self) -> &'static str {
        match self {
            Self::Chat => "Chat",
            Self::Launcher => "Launcher",
            Self::Terminal { .. } => "Terminal",
            Self::Settings => "Settings",
            Self::Vault => "Vault",
            #[cfg(feature = "memory")]
            Self::Memories => "Memories",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppTab {
    id: AppTabId,
    view: AppView,
}

impl AppTab {
    pub const fn id(&self) -> AppTabId {
        self.id
    }

    pub const fn view(&self) -> &AppView {
        &self.view
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectTabResult {
    Selected(AppTabId),
    Unknown(AppTabId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplaceLauncherError {
    Unknown(AppTabId),
    NotLauncher(AppTabId),
    InvalidReplacement(AppTabId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseTabResult {
    Closed { id: AppTabId, active: AppTabId },
    Permanent { id: AppTabId },
    Unknown { id: AppTabId },
}

#[doc(hidden)]
pub enum AppTabSelection {
    Id(AppTabId),
    LegacyIndex(usize),
}

impl From<AppTabId> for AppTabSelection {
    fn from(id: AppTabId) -> Self {
        Self::Id(id)
    }
}

impl From<usize> for AppTabSelection {
    fn from(index: usize) -> Self {
        Self::LegacyIndex(index)
    }
}

impl From<i32> for AppTabSelection {
    fn from(index: i32) -> Self {
        Self::LegacyIndex(usize::try_from(index).unwrap_or(usize::MAX))
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct AppTabs {
    tabs: Vec<AppTab>,
    active: AppTabId,
    chat_id: AppTabId,
}

impl Default for AppTabs {
    fn default() -> Self {
        let chat_id = AppTabId::next();
        Self {
            tabs: vec![AppTab {
                id: chat_id,
                view: AppView::Chat,
            }],
            active: chat_id,
            chat_id,
        }
    }
}

impl AppTabs {
    pub fn tabs(&self) -> &[AppTab] {
        &self.tabs
    }

    pub const fn active_id(&self) -> AppTabId {
        self.active
    }

    pub fn active_tab(&self) -> Option<&AppTab> {
        self.tab(self.active)
    }

    pub fn active_view(&self) -> AppView {
        let Some(tab) = self.active_tab() else {
            return AppView::Chat;
        };
        tab.view.clone()
    }

    pub fn tab(&self, id: AppTabId) -> Option<&AppTab> {
        self.tabs.iter().find(|tab| tab.id == id)
    }

    pub fn select(&mut self, selection: impl Into<AppTabSelection>) -> SelectTabResult {
        let id = match selection.into() {
            AppTabSelection::Id(id) => id,
            AppTabSelection::LegacyIndex(index) => {
                let Some(tab) = self.tabs.get(index) else {
                    return SelectTabResult::Unknown(self.chat_id);
                };
                tab.id
            }
        };
        if self.tab(id).is_none() {
            return SelectTabResult::Unknown(id);
        }
        self.active = id;
        SelectTabResult::Selected(id)
    }

    pub fn add_launcher(&mut self) -> AppTabId {
        let id = AppTabId::next();
        self.tabs.push(AppTab {
            id,
            view: AppView::Launcher,
        });
        self.active = id;
        id
    }

    pub fn open_view(&mut self, view: AppView) -> AppTabId {
        if let Some(id) = self
            .tabs
            .iter()
            .find(|tab| tab.view == view)
            .map(AppTab::id)
        {
            self.active = id;
            return id;
        }
        if self
            .active_tab()
            .is_some_and(|tab| tab.view == AppView::Launcher)
            && self.replace_launcher(self.active, view.clone()).is_ok()
        {
            return self.active;
        }
        let id = AppTabId::next();
        self.tabs.push(AppTab { id, view });
        self.active = id;
        id
    }

    pub fn replace_launcher(
        &mut self,
        id: AppTabId,
        replacement: AppView,
    ) -> Result<AppTabId, ReplaceLauncherError> {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) else {
            return Err(ReplaceLauncherError::Unknown(id));
        };
        if tab.view != AppView::Launcher {
            return Err(ReplaceLauncherError::NotLauncher(id));
        }
        let valid = matches!(
            replacement,
            AppView::Vault | AppView::Terminal { .. } | AppView::Settings
        );
        #[cfg(feature = "memory")]
        let valid = valid || matches!(replacement, AppView::Memories);
        if !valid {
            return Err(ReplaceLauncherError::InvalidReplacement(id));
        }
        tab.view = replacement;
        Ok(id)
    }

    pub fn close(&mut self, id: AppTabId) -> CloseTabResult {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return CloseTabResult::Unknown { id };
        };
        if self.tabs[index].view.is_chat() {
            return CloseTabResult::Permanent { id };
        }

        self.tabs.remove(index);
        if self.active == id {
            self.active = self
                .tabs
                .get(index)
                .or_else(|| self.tabs.last())
                .map_or(self.chat_id, AppTab::id);
        }
        CloseTabResult::Closed {
            id,
            active: self.active,
        }
    }

    #[cfg(test)]
    pub fn views(&self) -> Vec<AppView> {
        self.tabs.iter().map(|tab| tab.view.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{AppTabs, AppView, CloseTabResult, ReplaceLauncherError, SelectTabResult};

    #[test]
    fn app_tab_id_is_never_reused_after_close() {
        let mut tabs = AppTabs::default();
        let first = tabs.add_launcher();

        let _ = tabs.close(first);
        let second = tabs.add_launcher();

        assert!(second > first);
    }

    #[test]
    fn app_tab_id_replacement_preserves_identity_and_active_state() {
        let mut tabs = AppTabs::default();
        let launcher_id = tabs.add_launcher();
        let original_position = tabs.tabs().iter().position(|tab| tab.id() == launcher_id);

        let result = tabs.replace_launcher(launcher_id, AppView::Vault);

        assert_eq!(result, Ok(launcher_id));
        assert_eq!(tabs.active_id(), launcher_id);
        assert_eq!(
            tabs.tabs().iter().position(|tab| tab.id() == launcher_id),
            original_position
        );
        assert_eq!(
            tabs.active_tab().map(super::AppTab::view),
            Some(&AppView::Vault)
        );
    }

    #[test]
    fn app_tab_id_replacement_rejects_non_launcher_identity() {
        let tabs = AppTabs::default();
        let chat_id = tabs.active_id();
        let mut tabs = tabs;

        let result = tabs.replace_launcher(chat_id, AppView::Vault);

        assert_eq!(result, Err(ReplaceLauncherError::NotLauncher(chat_id)));
    }

    #[test]
    fn app_tab_id_close_selects_right_neighbor_when_available() {
        let mut tabs = AppTabs::default();
        let left = tabs.add_launcher();
        let middle = tabs.add_launcher();
        let right = tabs.add_launcher();
        assert_eq!(tabs.select(middle), SelectTabResult::Selected(middle));

        let result = tabs.close(middle);

        assert_eq!(
            result,
            CloseTabResult::Closed {
                id: middle,
                active: right,
            }
        );
        assert_eq!(tabs.active_id(), right);
        assert!(tabs.tabs().iter().any(|tab| tab.id() == left));
    }

    #[test]
    fn app_tab_id_chat_is_permanent() {
        let mut tabs = AppTabs::default();
        let chat_id = tabs.active_id();

        let result = tabs.close(chat_id);

        assert_eq!(result, CloseTabResult::Permanent { id: chat_id });
        assert_eq!(tabs.tabs().len(), 1);
    }

    #[test]
    fn app_tab_id_closing_last_closable_tab_returns_focus_to_chat() {
        let mut tabs = AppTabs::default();
        let chat_id = tabs.active_id();
        let launcher_id = tabs.add_launcher();

        let result = tabs.close(launcher_id);

        assert_eq!(
            result,
            CloseTabResult::Closed {
                id: launcher_id,
                active: chat_id,
            }
        );
        assert_eq!(tabs.active_id(), chat_id);
    }

    #[test]
    fn app_tab_id_unknown_close_is_a_typed_no_op() {
        let mut tabs = AppTabs::default();
        let stale_id = tabs.add_launcher();
        let _ = tabs.close(stale_id);

        let result = tabs.close(stale_id);

        assert_eq!(result, CloseTabResult::Unknown { id: stale_id });
    }
}
