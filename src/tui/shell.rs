use ratatui::layout::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelMode {
    Closed,
    Thin,
    Wide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelState {
    pub left: PanelMode,
    pub right: PanelMode,
    pub left_thin_width: u16,
    pub left_wide_width: u16,
    pub right_thin_width: u16,
    pub right_wide_width: u16,
}

impl PanelState {
    pub const fn new() -> Self {
        Self {
            left: PanelMode::Thin,
            right: PanelMode::Closed,
            left_thin_width: 24,
            left_wide_width: 42,
            right_thin_width: 28,
            right_wide_width: 56,
        }
    }

    pub fn from_config(config: &crate::config::TuiConfig) -> Self {
        Self {
            left: panel_mode_from_config(config.left_sidebar_mode),
            right: panel_mode_from_config(config.right_sidebar_mode),
            left_thin_width: config.left_thin_width,
            left_wide_width: config.left_wide_width,
            right_thin_width: config.right_thin_width,
            right_wide_width: config.right_wide_width,
        }
    }

    pub const fn left_width(self) -> u16 {
        match self.left {
            PanelMode::Closed => 0,
            PanelMode::Thin => self.left_thin_width,
            PanelMode::Wide => self.left_wide_width,
        }
    }

    pub const fn right_width(self) -> u16 {
        match self.right {
            PanelMode::Closed => 0,
            PanelMode::Thin => self.right_thin_width,
            PanelMode::Wide => self.right_wide_width,
        }
    }

    pub fn toggle_left(&mut self) {
        self.left = next_mode_left(self.left);
    }

    pub fn toggle_right(&mut self) {
        self.right = next_mode(self.right);
    }

    #[allow(dead_code)]
    pub fn expand_left(&mut self) {
        self.left = PanelMode::Wide;
    }

    #[allow(dead_code)]
    pub fn expand_right(&mut self) {
        self.right = PanelMode::Wide;
    }

    #[allow(dead_code)]
    pub fn collapse_left(&mut self) {
        self.left = PanelMode::Closed;
    }

    #[allow(dead_code)]
    pub fn collapse_right(&mut self) {
        self.right = PanelMode::Closed;
    }

    pub fn clamped_for_area(mut self, area: Rect) -> Self {
        let right_sidebar_minimum = if self.left == PanelMode::Closed {
            63
        } else {
            88
        };
        if area.width < right_sidebar_minimum {
            self.right = PanelMode::Closed;
        }
        let left_consumed = if self.left == PanelMode::Thin {
            self.left_width()
        } else {
            0
        };
        if self.right == PanelMode::Thin
            && area.width
                < left_consumed
                    .saturating_add(self.right_width())
                    .saturating_add(35)
        {
            self.right = PanelMode::Closed;
        }
        self
    }

    pub fn handle_left_rect(self, area: Rect) -> Rect {
        let right_x = self.right_handle_x(area);
        let mut x = area
            .x
            .saturating_add(self.left_width())
            .min(area.right().saturating_sub(1));
        if x == right_x && x > area.x {
            x = x.saturating_sub(1);
        }
        Rect::new(x, midpoint_y(area), 1, 1)
    }

    pub fn handle_right_rect(self, area: Rect) -> Rect {
        Rect::new(self.right_handle_x(area), midpoint_y(area), 1, 1)
    }

    pub fn handle_left_rects(self, area: Rect) -> [Rect; 3] {
        vertical_handle_rects(self.handle_left_rect(area), area)
    }

    pub fn handle_right_rects(self, area: Rect) -> [Rect; 3] {
        vertical_handle_rects(self.handle_right_rect(area), area)
    }

    fn right_handle_x(self, area: Rect) -> u16 {
        let width = self.right_width();
        if width == 0 {
            area.right().saturating_sub(1)
        } else {
            area.right()
                .saturating_sub(width)
                .saturating_sub(1)
                .max(area.x)
        }
    }
}

impl Default for PanelState {
    fn default() -> Self {
        Self::new()
    }
}

fn panel_mode_from_config(mode: crate::config::PanelMode) -> PanelMode {
    match mode {
        crate::config::PanelMode::Closed => PanelMode::Closed,
        crate::config::PanelMode::Thin => PanelMode::Thin,
        crate::config::PanelMode::Wide => PanelMode::Wide,
    }
}

const fn next_mode(mode: PanelMode) -> PanelMode {
    match mode {
        PanelMode::Closed => PanelMode::Thin,
        PanelMode::Thin => PanelMode::Wide,
        PanelMode::Wide => PanelMode::Closed,
    }
}

const fn next_mode_left(mode: PanelMode) -> PanelMode {
    match mode {
        PanelMode::Closed => PanelMode::Thin,
        PanelMode::Thin => PanelMode::Closed,
        PanelMode::Wide => PanelMode::Closed,
    }
}

fn midpoint_y(area: Rect) -> u16 {
    area.y.saturating_add(area.height / 2)
}

fn vertical_handle_rects(middle: Rect, area: Rect) -> [Rect; 3] {
    let top = middle.y.saturating_sub(1).max(area.y);
    let bottom = middle
        .y
        .saturating_add(1)
        .min(area.bottom().saturating_sub(1));
    [
        Rect::new(middle.x, top, 1, 1),
        middle,
        Rect::new(middle.x, bottom, 1, 1),
    ]
}

#[cfg(test)]
mod tests {
    use super::{PanelMode, PanelState};
    use ratatui::layout::Rect;

    #[test]
    fn left_width_returns_zero_thin_and_wide_widths() {
        let mut panels = PanelState::new();
        panels.left = PanelMode::Closed;
        assert_eq!(panels.left_width(), 0);

        panels.left = PanelMode::Thin;
        assert_eq!(panels.left_width(), 24);

        panels.left = PanelMode::Wide;
        assert_eq!(panels.left_width(), 42);
    }

    #[test]
    fn right_width_matches_mode_when_modes_change() {
        let mut panels = PanelState::new();
        panels.right = PanelMode::Closed;
        assert_eq!(panels.right_width(), 0);

        panels.right = PanelMode::Thin;
        assert_eq!(panels.right_width(), panels.right_thin_width);

        panels.right = PanelMode::Wide;
        assert_eq!(panels.right_width(), panels.right_wide_width);
    }

    #[test]
    fn toggles_cycle_closed_to_thin_to_wide_to_closed() {
        let mut panels = PanelState::new();

        panels.left = PanelMode::Closed;
        panels.toggle_left();
        assert_eq!(panels.left, PanelMode::Thin);
        panels.toggle_left();
        assert_eq!(panels.left, PanelMode::Closed);
        panels.toggle_left();
        assert_eq!(panels.left, PanelMode::Thin);

        panels.right = PanelMode::Closed;
        panels.toggle_right();
        assert_eq!(panels.right, PanelMode::Thin);
        panels.toggle_right();
        assert_eq!(panels.right, PanelMode::Wide);
        panels.toggle_right();
        assert_eq!(panels.right, PanelMode::Closed);
    }

    #[test]
    fn closed_left_handle_stays_on_left_edge() {
        let mut panels = PanelState::new();
        panels.left = PanelMode::Closed;
        assert_eq!(
            panels.handle_left_rect(Rect::new(0, 0, 80, 24)),
            Rect::new(0, 12, 1, 1)
        );
    }

    #[test]
    fn open_left_handle_protrudes_after_left_sidebar() {
        let panels = PanelState::new();
        assert_eq!(
            panels.handle_left_rect(Rect::new(0, 0, 80, 24)),
            Rect::new(panels.left_width(), 12, 1, 1)
        );
    }

    #[test]
    fn right_handle_tracks_closed_and_open_sidebar_edges() {
        let mut panels = PanelState::new();
        let area = Rect::new(0, 0, 80, 24);

        assert_eq!(panels.handle_right_rect(area), Rect::new(79, 12, 1, 1));

        panels.right = PanelMode::Thin;
        assert_eq!(panels.handle_right_rect(area), Rect::new(51, 12, 1, 1));
    }

    #[test]
    fn chat_area_has_room_at_eighty_columns() {
        let panels = PanelState::new();
        let chat_width = 80u16
            .saturating_sub(panels.left_width())
            .saturating_sub(panels.right_width());
        assert!(chat_width >= 40);
    }

    #[test]
    fn right_sidebar_closes_below_eighty_eight_columns_when_left_is_open() {
        let mut panels = PanelState::new();
        panels.right = PanelMode::Wide;
        let clamped = panels.clamped_for_area(Rect::new(0, 0, 87, 24));
        assert_eq!(clamped.right, PanelMode::Closed);
    }

    #[test]
    fn right_sidebar_stays_available_at_eighty_eight_columns_when_left_is_open() {
        let mut panels = PanelState::new();
        panels.right = PanelMode::Thin;

        let clamped = panels.clamped_for_area(Rect::new(0, 0, 88, 24));

        assert_eq!(clamped.right, PanelMode::Thin);
    }

    #[test]
    fn right_sidebar_uses_sixty_three_column_threshold_when_left_is_closed() {
        let mut panels = PanelState::new();
        panels.left = PanelMode::Closed;
        panels.right = PanelMode::Thin;

        assert_eq!(
            panels.clamped_for_area(Rect::new(0, 0, 62, 16)).right,
            PanelMode::Closed
        );
        assert_eq!(
            panels.clamped_for_area(Rect::new(0, 0, 63, 16)).right,
            PanelMode::Thin
        );
    }
}
