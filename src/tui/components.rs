//! App-owned widget primitives for the new TUI namespace.
//!
//! Plain structs and free functions -- no trait abstractions. Each primitive
//! is a building block for later todos (command palette, settings panel, etc.).
// TODO: Building-block primitives preserved for future TUI surfaces. Prune
// unused items as they are superseded by integrated implementations.
#![allow(dead_code)]

use crossterm::event::KeyCode;
use ratatui::{Frame, prelude::*, widgets::*};

// ---------------------------------------------------------------------------
// Centered popup geometry
// ---------------------------------------------------------------------------

/// Compute a centered rect inside `r` occupying `percent_x`% width and
/// `percent_y`% height. Mirrors the pattern in `src/ui/modals/quit_confirm.rs`.
pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

// ---------------------------------------------------------------------------
// Search field
// ---------------------------------------------------------------------------

/// Single-line text input state for search/filter fields. No debounce, no
/// fuzzy scoring -- pure cursor + buffer state.
#[derive(Debug, Clone)]
pub struct SearchField {
    query: String,
    cursor: usize,
}

impl SearchField {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            cursor: 0,
        }
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn len(&self) -> usize {
        self.query.len()
    }

    pub fn is_empty(&self) -> bool {
        self.query.is_empty()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Insert `text` at the cursor position. Empty input is a no-op.
    pub fn insert(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.query.insert_str(self.cursor, text);
        self.cursor += text.len();
    }

    /// Delete the character to the left of the cursor. No-op at position 0.
    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let prev_len = self.query[..self.cursor]
            .chars()
            .last()
            .map(char::len_utf8)
            .unwrap_or(0);
        let cut = self.cursor - prev_len;
        self.query.replace_range(cut..self.cursor, "");
        self.cursor = cut;
    }

    /// Delete the character to the right of the cursor. No-op at end.
    pub fn delete(&mut self) {
        if self.cursor >= self.query.len() {
            return;
        }
        let next_len = self.query[self.cursor..]
            .chars()
            .next()
            .map(char::len_utf8)
            .unwrap_or(0);
        self.query
            .replace_range(self.cursor..self.cursor + next_len, "");
    }

    /// Move cursor one character left. No-op at position 0.
    pub fn left(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let prev_len = self.query[..self.cursor]
            .chars()
            .last()
            .map(char::len_utf8)
            .unwrap_or(0);
        self.cursor -= prev_len;
    }

    /// Move cursor one character right. No-op at end.
    pub fn right(&mut self) {
        if self.cursor >= self.query.len() {
            return;
        }
        let next_len = self.query[self.cursor..]
            .chars()
            .next()
            .map(char::len_utf8)
            .unwrap_or(0);
        self.cursor += next_len;
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.query.len();
    }

    pub fn clear(&mut self) {
        self.query.clear();
        self.cursor = 0;
    }
}

impl Default for SearchField {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Selectable list
// ---------------------------------------------------------------------------

/// Pure selection + scroll-offset state over a flat `Vec<T>`. No filtering,
/// no rendering -- the caller maps `selected()` / `offset()` to rows.
#[derive(Debug, Clone)]
pub struct SelectList<T> {
    items: Vec<T>,
    selected: usize,
    offset: usize,
}

impl<T> SelectList<T> {
    pub fn new(items: Vec<T>) -> Self {
        Self {
            items,
            selected: 0,
            offset: 0,
        }
    }

    pub fn items(&self) -> &[T] {
        &self.items
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn selected_item(&self) -> Option<&T> {
        self.items.get(self.selected)
    }

    /// Set selection to `idx` if in bounds. Out-of-bounds is ignored.
    pub fn select_at(&mut self, idx: usize) {
        if idx < self.items.len() {
            self.selected = idx;
        }
    }

    /// Move selection up by one. Clamps at 0.
    pub fn up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    /// Move selection down by one. Clamps at last item.
    pub fn down(&mut self) {
        if self.selected + 1 < self.items.len() {
            self.selected += 1;
        }
    }

    /// Move selection up by `page` rows. Clamps at 0.
    pub fn page_up(&mut self, page: usize) {
        self.selected = self.selected.saturating_sub(page);
    }

    /// Move selection down by `page` rows. Clamps at last item.
    pub fn page_down(&mut self, page: usize) {
        if self.items.is_empty() {
            return;
        }
        self.selected = (self.selected + page).min(self.items.len() - 1);
    }

    pub fn home(&mut self) {
        self.selected = 0;
    }

    pub fn end(&mut self) {
        self.selected = self.items.len().saturating_sub(1);
    }
}

/// A group header label rendered above a group of list rows.
#[derive(Debug, Clone, Copy)]
pub struct GroupHeader<'a>(pub &'a str);

// ---------------------------------------------------------------------------
// Helper box geometry
// ---------------------------------------------------------------------------

/// Compute the area for a helper/info box at the bottom of `panel_area`.
///
/// `max_rows` is the user-configured maximum (clamped to 1..=6).
/// `text_rows` is the number of rows the helper text needs.
/// The result is clamped to `1..=max_rows` and positioned at the bottom.
pub fn helper_area(panel_area: Rect, max_rows: u16, text_rows: u16) -> Rect {
    let max = max_rows.clamp(1, 6);
    let rows = text_rows.min(max).max(1);
    Rect::new(
        panel_area.x,
        panel_area.y + panel_area.height.saturating_sub(rows),
        panel_area.width,
        rows,
    )
}

// ---------------------------------------------------------------------------
// Confirm modal
// ---------------------------------------------------------------------------

/// Which confirm-dialog button was activated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmButton {
    Confirm,
    Cancel,
}

/// Button hit areas returned by `ConfirmModal::areas` / `render`.
#[derive(Debug, Clone, Copy)]
pub struct ConfirmAreas {
    pub confirm: Rect,
    pub cancel: Rect,
}

/// A reusable confirmation modal. Renders a centered popup with a title,
/// body message, and two buttons. `danger` flags destructive actions
/// (controls styling, not geometry).
#[derive(Debug, Clone)]
pub struct ConfirmModal {
    title: String,
    body: String,
    danger: bool,
}

impl ConfirmModal {
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            danger: false,
        }
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn body(&self) -> &str {
        &self.body
    }

    pub fn danger(&self) -> bool {
        self.danger
    }

    pub fn set_danger(&mut self, danger: bool) {
        self.danger = danger;
    }

    /// Builder-style danger flag setter.
    pub fn with_danger(mut self, danger: bool) -> Self {
        self.danger = danger;
        self
    }

    /// Map a key press to a confirm/cancel action. `Enter` confirms,
    /// `Esc` cancels. All other keys return `None`.
    pub fn key_button(key: KeyCode) -> Option<ConfirmButton> {
        match key {
            KeyCode::Enter => Some(ConfirmButton::Confirm),
            KeyCode::Esc => Some(ConfirmButton::Cancel),
            _ => None,
        }
    }

    /// Compute button hit areas inside `area` without rendering.
    pub fn areas(&self, area: Rect) -> ConfirmAreas {
        let popup = centered_rect(40, 20, area);
        let confirm_w: u16 = 11; // " [Confirm] "
        let cancel_w: u16 = 9; //  " [Cancel] "
        let gap: u16 = 5;
        let total = confirm_w + gap + cancel_w;
        let start_x = popup.x + (popup.width.saturating_sub(total)) / 2;
        let button_y = popup.y + popup.height.saturating_sub(3);
        ConfirmAreas {
            confirm: Rect::new(start_x, button_y, confirm_w, 1),
            cancel: Rect::new(start_x + confirm_w + gap, button_y, cancel_w, 1),
        }
    }

    /// Render the modal into `f` and return button hit areas.
    pub fn render(&self, f: &mut Frame, area: Rect) -> ConfirmAreas {
        let popup = centered_rect(40, 20, area);

        f.render_widget(
            Block::default().style(Style::default().bg(Color::Black)),
            area,
        );

        let border_color = if self.danger {
            Color::Red
        } else {
            Color::Yellow
        };
        let theme = crate::theme::active_theme();
        let block = Block::default().style(Style::default().bg(theme.panel));

        let text = vec![
            Line::from(format!(" {} ", self.title)).style(
                Style::default()
                    .fg(border_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Line::from(""),
            Line::from(self.body.clone()).alignment(Alignment::Center),
            Line::from(""),
            Line::from(vec![
                Span::styled(
                    " [Confirm] ",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("     "),
                Span::styled(" [Cancel] ", Style::default().fg(Color::Red)),
            ])
            .alignment(Alignment::Center),
            Line::from(""),
        ];

        let paragraph = Paragraph::new(text)
            .block(block)
            .alignment(Alignment::Center);

        f.render_widget(Clear, popup);
        f.render_widget(paragraph, popup);

        self.areas(area)
    }
}

// ---------------------------------------------------------------------------
// Mouse hit mapping
// ---------------------------------------------------------------------------

/// Map a screen y-coordinate to a list row index. Returns `None` if `y`
/// is outside `list_area`.
pub fn row_at(list_area: Rect, y: u16) -> Option<usize> {
    if y < list_area.y || y >= list_area.y + list_area.height {
        return None;
    }
    Some((y - list_area.y) as usize)
}

/// Map a screen coordinate to a confirm-dialog button. Returns `None` if
/// the coordinate is outside both buttons.
pub fn button_at(areas: &ConfirmAreas, x: u16, y: u16) -> Option<ConfirmButton> {
    let pos = Position::new(x, y);
    if areas.confirm.contains(pos) {
        Some(ConfirmButton::Confirm)
    } else if areas.cancel.contains(pos) {
        Some(ConfirmButton::Cancel)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- centered_rect ---

    #[test]
    fn centered_rect_fits_inside_parent() {
        let parent = Rect::new(0, 0, 100, 100);
        let rect = centered_rect(60, 50, parent);
        assert!(rect.x >= parent.x);
        assert!(rect.y >= parent.y);
        assert!(rect.x + rect.width <= parent.x + parent.width);
        assert!(rect.y + rect.height <= parent.y + parent.height);
    }

    #[test]
    fn centered_rect_known_values() {
        let parent = Rect::new(0, 0, 100, 100);
        let rect = centered_rect(60, 50, parent);
        // 60% of 100 = 60 wide, 50% of 100 = 50 tall
        assert_eq!(rect.width, 60);
        assert_eq!(rect.height, 50);
        // centered: (100-60)/2 = 20 x, (100-50)/2 = 25 y
        assert_eq!(rect.x, 20);
        assert_eq!(rect.y, 25);
    }

    // --- SearchField ---

    #[test]
    fn search_field_insert_appends_at_cursor() {
        let mut sf = SearchField::new();
        sf.insert("hello");
        assert_eq!(sf.query(), "hello");
        assert_eq!(sf.cursor(), 5);
    }

    #[test]
    fn search_field_insert_empty_is_noop() {
        let mut sf = SearchField::new();
        sf.insert("ab");
        sf.insert("");
        assert_eq!(sf.query(), "ab");
        assert_eq!(sf.cursor(), 2);
    }

    #[test]
    fn search_field_backspace_on_empty_is_noop() {
        let mut sf = SearchField::new();
        sf.backspace();
        assert_eq!(sf.query(), "");
        assert_eq!(sf.cursor(), 0);
    }

    #[test]
    fn search_field_backspace_removes_last_char() {
        let mut sf = SearchField::new();
        sf.insert("abc");
        sf.backspace();
        assert_eq!(sf.query(), "ab");
        assert_eq!(sf.cursor(), 2);
    }

    #[test]
    fn search_field_backspace_at_cursor_middle() {
        let mut sf = SearchField::new();
        sf.insert("abc");
        sf.left(); // cursor at 2 (between b and c)
        sf.backspace();
        assert_eq!(sf.query(), "ac");
        assert_eq!(sf.cursor(), 1);
    }

    #[test]
    fn search_field_left_right_home_end() {
        let mut sf = SearchField::new();
        sf.insert("abc");
        sf.home();
        assert_eq!(sf.cursor(), 0);
        sf.right();
        assert_eq!(sf.cursor(), 1);
        sf.end();
        assert_eq!(sf.cursor(), 3);
        sf.left();
        assert_eq!(sf.cursor(), 2);
    }

    #[test]
    fn search_field_clear_resets() {
        let mut sf = SearchField::new();
        sf.insert("hello");
        sf.clear();
        assert_eq!(sf.query(), "");
        assert_eq!(sf.cursor(), 0);
    }

    #[test]
    fn search_field_delete_at_end_is_noop() {
        let mut sf = SearchField::new();
        sf.insert("abc");
        sf.delete();
        assert_eq!(sf.query(), "abc");
    }

    #[test]
    fn search_field_delete_in_middle() {
        let mut sf = SearchField::new();
        sf.insert("abc");
        sf.home();
        sf.delete();
        assert_eq!(sf.query(), "bc");
        assert_eq!(sf.cursor(), 0);
    }

    // --- SelectList ---

    #[test]
    fn select_list_down_changes_selected_index() {
        let mut list = SelectList::new(vec!["a", "b", "c"]);
        assert_eq!(list.selected(), 0);
        list.down();
        assert_eq!(list.selected(), 1, "down must advance selection");
        list.down();
        assert_eq!(list.selected(), 2, "down must advance selection");
    }

    #[test]
    fn select_list_down_clamps_at_last() {
        let mut list = SelectList::new(vec!["a", "b"]);
        list.down();
        list.down();
        list.down();
        assert_eq!(list.selected(), 1, "clamped at last index");
    }

    #[test]
    fn select_list_up_changes_selected_index() {
        let mut list = SelectList::new(vec!["a", "b", "c"]);
        list.end();
        assert_eq!(list.selected(), 2);
        list.up();
        assert_eq!(list.selected(), 1, "up must decrease selection");
        list.up();
        assert_eq!(list.selected(), 0, "up must decrease selection");
    }

    #[test]
    fn select_list_up_clamps_at_zero() {
        let mut list = SelectList::new(vec!["a", "b"]);
        list.up();
        list.up();
        assert_eq!(list.selected(), 0, "clamped at 0");
    }

    #[test]
    fn select_list_page_up_clamps() {
        let mut list = SelectList::new(vec![0, 1, 2, 3, 4]);
        list.end();
        assert_eq!(list.selected(), 4);
        list.page_up(2);
        assert_eq!(list.selected(), 2);
        list.page_up(10);
        assert_eq!(list.selected(), 0, "page_up clamps at 0");
    }

    #[test]
    fn select_list_page_down_clamps() {
        let mut list = SelectList::new(vec![0, 1, 2, 3, 4]);
        list.page_down(2);
        assert_eq!(list.selected(), 2);
        list.page_down(10);
        assert_eq!(list.selected(), 4, "page_down clamps at last");
    }

    #[test]
    fn select_list_select_at_in_bounds() {
        let mut list = SelectList::new(vec!["a", "b", "c"]);
        list.select_at(2);
        assert_eq!(list.selected(), 2);
    }

    #[test]
    fn select_list_select_at_out_of_bounds_ignored() {
        let mut list = SelectList::new(vec!["a", "b"]);
        list.select_at(5);
        assert_eq!(list.selected(), 0, "out-of-bounds select_at ignored");
    }

    #[test]
    fn select_list_home_end() {
        let mut list = SelectList::new(vec!["a", "b", "c"]);
        list.end();
        assert_eq!(list.selected(), 2);
        list.home();
        assert_eq!(list.selected(), 0);
    }

    #[test]
    fn select_list_empty_does_not_panic() {
        let mut list: SelectList<i32> = SelectList::new(vec![]);
        list.down();
        list.up();
        list.page_down(5);
        list.page_up(5);
        list.end();
        list.home();
        assert_eq!(list.selected(), 0);
    }

    // --- helper_area ---

    #[test]
    fn helper_area_clamps_to_max_six_rows() {
        let panel = Rect::new(0, 0, 80, 24);
        let area = helper_area(panel, 6, 10);
        assert_eq!(area.height, 6, "text_rows=10 clamped to max_rows=6");
    }

    #[test]
    fn helper_area_clamps_to_min_one_row() {
        let panel = Rect::new(0, 0, 80, 24);
        let area = helper_area(panel, 6, 0);
        assert_eq!(area.height, 1, "text_rows=0 clamped to min 1");
    }

    #[test]
    fn helper_area_max_rows_clamped_to_six() {
        let panel = Rect::new(0, 0, 80, 24);
        let area = helper_area(panel, 99, 10);
        assert_eq!(area.height, 6, "max_rows=99 clamped to 6");
    }

    #[test]
    fn helper_area_max_rows_zero_does_not_panic() {
        let panel = Rect::new(0, 0, 80, 24);
        let area = helper_area(panel, 0, 3);
        assert_eq!(area.height, 1, "max_rows=0 clamped to 1");
    }

    #[test]
    fn helper_area_positioned_at_bottom() {
        let panel = Rect::new(5, 10, 80, 24);
        let area = helper_area(panel, 3, 2);
        assert_eq!(area.y, 10 + 24 - 2, "positioned at bottom of panel");
        assert_eq!(area.x, 5);
        assert_eq!(area.width, 80);
    }

    // --- row_at ---

    #[test]
    fn row_at_first_middle_last() {
        let list_area = Rect::new(0, 10, 40, 5);
        assert_eq!(row_at(list_area, 10), Some(0), "first row");
        assert_eq!(row_at(list_area, 12), Some(2), "middle row");
        assert_eq!(row_at(list_area, 14), Some(4), "last row");
    }

    #[test]
    fn row_at_out_of_range_returns_none() {
        let list_area = Rect::new(0, 10, 40, 5);
        assert_eq!(row_at(list_area, 9), None, "above");
        assert_eq!(row_at(list_area, 15), None, "below");
    }

    // --- ConfirmModal ---

    #[test]
    fn confirm_key_enter_is_confirm() {
        assert_eq!(
            ConfirmModal::key_button(KeyCode::Enter),
            Some(ConfirmButton::Confirm),
            "Enter must map to Confirm"
        );
    }

    #[test]
    fn confirm_key_esc_is_cancel() {
        assert_eq!(
            ConfirmModal::key_button(KeyCode::Esc),
            Some(ConfirmButton::Cancel),
            "Esc must map to Cancel"
        );
    }

    #[test]
    fn confirm_key_other_is_none() {
        assert_eq!(ConfirmModal::key_button(KeyCode::Char('x')), None);
        assert_eq!(ConfirmModal::key_button(KeyCode::Tab), None);
    }

    #[test]
    fn confirm_areas_returns_distinct_confirm_cancel_rects() {
        let modal = ConfirmModal::new("Delete", "Are you sure?");
        let area = Rect::new(0, 0, 100, 100);
        let areas = modal.areas(area);
        assert!(areas.confirm.width > 0);
        assert!(areas.cancel.width > 0);
        // Confirm is left of cancel
        assert!(areas.confirm.x < areas.cancel.x);
        // No overlap
        assert!(areas.confirm.x + areas.confirm.width <= areas.cancel.x);
    }

    #[test]
    fn confirm_modal_danger_flag() {
        let modal = ConfirmModal::new("Quit", "Sure?").with_danger(true);
        assert!(modal.danger());
        let mut modal = ConfirmModal::new("Info", "OK?");
        modal.set_danger(true);
        assert!(modal.danger());
    }

    // --- button_at ---

    #[test]
    fn button_at_confirm_button() {
        let modal = ConfirmModal::new("Delete", "Sure?");
        let areas = modal.areas(Rect::new(0, 0, 100, 100));
        let cx = areas.confirm.x + areas.confirm.width / 2;
        let cy = areas.confirm.y;
        assert_eq!(
            button_at(&areas, cx, cy),
            Some(ConfirmButton::Confirm),
            "click inside confirm rect -> Confirm"
        );
    }

    #[test]
    fn button_at_cancel_button() {
        let modal = ConfirmModal::new("Delete", "Sure?");
        let areas = modal.areas(Rect::new(0, 0, 100, 100));
        let cx = areas.cancel.x + areas.cancel.width / 2;
        let cy = areas.cancel.y;
        assert_eq!(
            button_at(&areas, cx, cy),
            Some(ConfirmButton::Cancel),
            "click inside cancel rect -> Cancel"
        );
    }

    #[test]
    fn button_at_outside_returns_none() {
        let modal = ConfirmModal::new("Delete", "Sure?");
        let areas = modal.areas(Rect::new(0, 0, 100, 100));
        assert_eq!(button_at(&areas, 0, 0), None, "top-left corner is outside");
    }
}
