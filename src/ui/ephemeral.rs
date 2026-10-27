use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Wrap},
};

use super::{
    app_tabs::AppTabId,
    chat_tab::{ChatTab, ChatTabProps},
    ChatTabState, UI,
};
use crate::tui::shell::PanelMode;

pub(crate) struct EphemeralPane {
    pub(crate) tab_index: usize,
    pub(crate) main_tab_index: usize,
    pub(crate) conversation_id: i64,
    pub(crate) origin_conversation_id: i64,
    pub(crate) copied_message_count: usize,
    pub(crate) title: String,
    pub(crate) input_id: AppTabId,
    pub(crate) previous_right_mode: PanelMode,
    pub(crate) area: Option<Rect>,
    pub(crate) close_area: Option<Rect>,
}

impl ChatTabState {
    pub(crate) fn clear_hit_areas(&mut self) {
        self.provider_hit_area = None;
        self.model_hit_area = None;
        self.reasoning_hit_area = None;
        self.input_area = None;
        self.input_text_area = None;
        self.pending_document_chip_hit_areas.clear();
        self.dropdown_item_areas.clear();
        self.chat_scrollbar_area = None;
        self.chat_scrollbar_thumb = None;
        self.thinking_hit_areas.clear();
        self.link_hit_areas.clear();
        self.copy_hit_areas.clear();
        self.command_hit_areas.clear();
    }
}

impl UI {
    pub(crate) fn main_chat_index(&self) -> usize {
        self.ephemeral
            .as_ref()
            .map_or(self.active_tab, |pane| pane.main_tab_index)
    }

    pub(crate) fn focused_ephemeral(&self) -> bool {
        self.ephemeral
            .as_ref()
            .is_some_and(|pane| pane.tab_index == self.active_tab)
    }

    pub(crate) fn focus_chat_pane(&mut self, index: usize) -> bool {
        if index >= self.tabs.len()
            || !self
                .ephemeral
                .as_ref()
                .is_some_and(|pane| index == pane.tab_index || index == pane.main_tab_index)
        {
            return false;
        }
        if self.active_tab != index {
            if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                tab.provider_dropdown_open = false;
                tab.model_dropdown_open = false;
                tab.reasoning_dropdown_open = false;
                tab.provider_hit_area = None;
                tab.model_hit_area = None;
                tab.reasoning_hit_area = None;
                tab.dropdown_item_areas.clear();
            }
            self.active_tab = index;
        }
        self.focus = crate::tui::focus::Focus::Chat;
        self.text_selection.cancel();
        true
    }

    pub(crate) fn chat_pane_at(&self, pos: Position) -> Option<usize> {
        if !self.app_tabs.active_view().is_chat() {
            return None;
        }
        if let Some(pane) = self.ephemeral.as_ref() {
            if pane.area.is_some_and(|area| area.contains(pos)) {
                return Some(pane.tab_index);
            }
        }
        self.chat_area
            .filter(|area| area.contains(pos))
            .map(|_| self.main_chat_index())
    }

    pub(super) fn render_chat_pane(
        &mut self,
        f: &mut Frame,
        index: usize,
        area: Rect,
        show_cursor: bool,
    ) {
        let Some(tab) = self.tabs.get_mut(index) else {
            return;
        };
        if area.is_empty() {
            return;
        }
        let theme = crate::theme::active_theme();
        f.render_widget(
            Block::default().style(Style::default().fg(theme.foreground).bg(theme.background)),
            area,
        );
        let mut chat = ChatTab::new(
            tab,
            ChatTabProps {
                user_alignment: self.user_alignment,
                ai_alignment: self.ai_alignment,
                markdown_mode: self.markdown_mode,
                collapse_thinking: self.collapse_thinking,
                show_chat_scrollbar: self.show_chat_scrollbar,
                kitty_enhanced_text: self.kitty_enhanced_text,
                kitty_heading_downscale: self.kitty_heading_downscale,
                image_protocol: &self.image_protocol,
                terminal_capabilities: self.terminal_capabilities,
                frame_tick: self.frame_tick,
                providers: &self.visible_providers,
                models: &self.current_models,
                reasoning_options: &self.current_reasoning_options,
            },
        );
        chat.show_cursor = show_cursor;
        chat.render(f, area);
    }

    pub(super) fn render_ephemeral(&mut self, f: &mut Frame, area: Rect, show_cursor: bool) {
        let focused = self.focused_ephemeral();
        let Some(pane) = self.ephemeral.as_mut() else {
            return;
        };
        let theme = crate::theme::active_theme();
        let border = Block::default()
            .borders(Borders::LEFT)
            .border_style(Style::default().fg(if focused {
                theme.foreground
            } else {
                theme.border
            }))
            .style(Style::default().bg(theme.background));
        let inner = border.inner(area);
        f.render_widget(border, area);
        pane.area = Some(area);
        let title_width = inner.width.saturating_sub(4).max(1);
        let title = Paragraph::new(pane.title.as_str())
            .wrap(Wrap { trim: false })
            .style(Style::default().fg(theme.foreground).bold());
        let title_height = u16::try_from(title.line_count(title_width))
            .unwrap_or(u16::MAX)
            .min(inner.height.saturating_sub(5))
            .max(1);
        f.render_widget(
            title,
            Rect::new(inner.x, inner.y, title_width, title_height),
        );
        let close = Rect::new(
            inner.right().saturating_sub(3),
            inner.y,
            3.min(inner.width),
            1,
        );
        f.render_widget(
            Paragraph::new("[X]").style(Style::default().fg(theme.foreground)),
            close,
        );
        pane.close_area = Some(close);
        let tab_index = pane.tab_index;
        let body = Rect::new(
            inner.x,
            inner.y.saturating_add(title_height),
            inner.width,
            inner.height.saturating_sub(title_height),
        );
        self.render_chat_pane(f, tab_index, body, show_cursor && focused);
    }
}
