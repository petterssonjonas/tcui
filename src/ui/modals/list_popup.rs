use ratatui::{Frame, layout::Rect, prelude::*, widgets::*};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListPopupAction {
    InsertText(String),
    ReplaceInput(String),
    SetTheme(String),
}

#[derive(Debug, Clone)]
pub struct ListPopupItem {
    pub label: String,
    pub action: Option<ListPopupAction>,
}

impl ListPopupItem {
    pub fn insert(text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            label: text.trim().to_string(),
            action: Some(ListPopupAction::InsertText(text)),
        }
    }

    pub fn action(label: impl Into<String>, action: ListPopupAction) -> Self {
        Self {
            label: label.into(),
            action: Some(action),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ListPopupPlacement {
    Centered,
    Anchored(Option<Rect>),
}

#[derive(Debug, Clone)]
pub struct ListPopup {
    pub title: String,
    pub empty_label: String,
    pub items: Vec<ListPopupItem>,
    pub scroll: usize,
    pub selected: Option<usize>,
    pub placement: ListPopupPlacement,
    pub live_input: bool,
}

impl ListPopup {
    pub fn new(
        title: impl Into<String>,
        empty_label: impl Into<String>,
        items: Vec<String>,
    ) -> Self {
        Self {
            title: title.into(),
            empty_label: empty_label.into(),
            items: items
                .into_iter()
                .map(|label| ListPopupItem {
                    label,
                    action: None,
                })
                .collect(),
            scroll: 0,
            selected: None,
            placement: ListPopupPlacement::Centered,
            live_input: false,
        }
    }

    pub fn selectable(
        title: impl Into<String>,
        empty_label: impl Into<String>,
        items: Vec<ListPopupItem>,
    ) -> Self {
        let selected = (!items.is_empty()).then_some(0);
        Self {
            title: title.into(),
            empty_label: empty_label.into(),
            items,
            scroll: 0,
            selected,
            placement: ListPopupPlacement::Centered,
            live_input: false,
        }
    }

    pub fn anchored_selectable(
        title: impl Into<String>,
        empty_label: impl Into<String>,
        items: Vec<ListPopupItem>,
        anchor: Option<Rect>,
    ) -> Self {
        let selected = (!items.is_empty()).then_some(0);
        Self {
            title: title.into(),
            empty_label: empty_label.into(),
            items,
            scroll: 0,
            selected,
            placement: ListPopupPlacement::Anchored(anchor),
            live_input: true,
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let theme = crate::theme::active_theme();
        let popup_area = self.popup_area_in(area);
        let block = Block::default().style(theme.panel_style());
        let inner = Rect::new(
            popup_area.x,
            popup_area.y + 1,
            popup_area.width,
            popup_area.height.saturating_sub(1),
        );

        f.render_widget(Clear, popup_area);
        f.render_widget(block, popup_area);
        f.render_widget(
            Paragraph::new(Line::from(format!(" {} ", self.title))).style(
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Rect::new(popup_area.x, popup_area.y, popup_area.width, 1),
        );

        if self.items.is_empty() {
            f.render_widget(
                Paragraph::new(self.empty_label.as_str())
                    .style(Style::default().fg(theme.muted).bg(theme.panel))
                    .alignment(Alignment::Center),
                inner,
            );
            return;
        }

        let visible = inner.height as usize;
        let offset = self.scroll.min(self.items.len().saturating_sub(visible));
        let lines: Vec<Line> = self
            .items
            .iter()
            .enumerate()
            .skip(offset)
            .take(visible)
            .map(|(idx, item)| {
                let style = if self.selected == Some(idx) {
                    theme.selected_style()
                } else {
                    Style::default().fg(theme.foreground).bg(theme.panel)
                };
                Line::from(Span::styled(item.label.clone(), style))
            })
            .collect();
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
    }

    pub fn popup_area(area: Rect) -> Rect {
        let width = area.width.saturating_mul(70).saturating_div(100).max(30);
        let height = area.height.saturating_mul(70).saturating_div(100).max(10);
        Rect::new(
            area.x + area.width.saturating_sub(width) / 2,
            area.y + area.height.saturating_sub(height) / 2,
            width.min(area.width),
            height.min(area.height),
        )
    }

    pub fn popup_area_in(&self, area: Rect) -> Rect {
        match self.placement {
            ListPopupPlacement::Centered => Self::popup_area(area),
            ListPopupPlacement::Anchored(Some(anchor)) => {
                let width = anchor.width.max(36).min(area.width);
                let desired_height = (self.items.len() as u16 + 2).clamp(4, 12);
                let height = desired_height.min(area.height);
                let x = anchor.x.min(area.right().saturating_sub(width));
                let y = anchor
                    .y
                    .saturating_sub(height)
                    .max(area.y)
                    .min(area.bottom().saturating_sub(height));
                Rect::new(x, y, width, height)
            }
            ListPopupPlacement::Anchored(None) => Self::popup_area(area),
        }
    }

    pub fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
    }

    pub fn scroll_down(&mut self, visible_rows: usize) {
        let max_scroll = self.items.len().saturating_sub(visible_rows);
        self.scroll = (self.scroll + 1).min(max_scroll);
    }

    pub fn move_up(&mut self) {
        if let Some(selected) = self.selected {
            self.selected = Some(selected.saturating_sub(1));
            self.scroll = self.scroll.min(self.selected.unwrap_or(0));
        } else {
            self.scroll_up();
        }
    }

    pub fn move_down(&mut self, visible_rows: usize) {
        if let Some(selected) = self.selected {
            let next = (selected + 1).min(self.items.len().saturating_sub(1));
            self.selected = Some(next);
            if next >= self.scroll + visible_rows {
                self.scroll = next + 1 - visible_rows;
            }
        } else {
            self.scroll_down(visible_rows);
        }
    }

    pub fn selected_action(&self) -> Option<ListPopupAction> {
        self.selected
            .and_then(|idx| self.items.get(idx))
            .and_then(|item| item.action.clone())
    }

    pub fn action_at(&mut self, area: Rect, position: Position) -> Option<ListPopupAction> {
        let popup_area = self.popup_area_in(area);
        let inner = Rect::new(
            popup_area.x,
            popup_area.y + 1,
            popup_area.width,
            popup_area.height.saturating_sub(1),
        );
        if !inner.contains(position) {
            return None;
        }
        let idx = self.scroll + position.y.saturating_sub(inner.y) as usize;
        self.selected = (idx < self.items.len()).then_some(idx);
        self.selected_action()
    }
}

#[cfg(test)]
mod tests {
    use super::{ListPopup, ListPopupAction, ListPopupItem};

    #[test]
    fn selectable_popup_returns_the_highlighted_action() {
        // Given
        let mut popup = ListPopup::selectable(
            "Skills",
            "No skills",
            vec![
                ListPopupItem::insert("@caveman "),
                ListPopupItem::insert("@obsidian "),
            ],
        );

        // When
        popup.move_down(4);

        // Then
        assert_eq!(
            popup.selected_action(),
            Some(ListPopupAction::InsertText("@obsidian ".to_string()))
        );
    }
}
