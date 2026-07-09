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
            left_thin_width: 28,
            left_wide_width: 42,
            right_thin_width: 32,
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
        self.left = next_mode(self.left);
    }

    pub fn toggle_right(&mut self) {
        self.right = next_mode(self.right);
    }

    pub fn expand_left(&mut self) {
        self.left = PanelMode::Wide;
    }

    pub fn expand_right(&mut self) {
        self.right = PanelMode::Wide;
    }

    pub fn collapse_left(&mut self) {
        self.left = PanelMode::Closed;
    }

    pub fn collapse_right(&mut self) {
        self.right = PanelMode::Closed;
    }

    pub fn clamped_for_area(mut self, area: Rect) -> Self {
        if area.width < 80 {
            self.right = PanelMode::Closed;
        }
        self
    }

    pub fn handle_left_rect(self, area: Rect) -> Rect {
        Rect::new(
            area.x.saturating_add(self.left_width()),
            midpoint_y(area),
            1,
            1,
        )
    }

    pub fn handle_right_rect(self, area: Rect) -> Rect {
        let width = self.right_width();
        let x = if width == 0 {
            area.right().saturating_sub(1)
        } else {
            area.right().saturating_sub(width).saturating_sub(1)
        };
        Rect::new(x, midpoint_y(area), 1, 1)
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

fn midpoint_y(area: Rect) -> u16 {
    area.y.saturating_add(area.height / 2)
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
        assert_eq!(panels.left_width(), 28);

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
        assert_eq!(panels.left, PanelMode::Wide);
        panels.toggle_left();
        assert_eq!(panels.left, PanelMode::Closed);

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
        assert_eq!(panels.handle_right_rect(area), Rect::new(47, 12, 1, 1));
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
    fn narrow_terminal_forces_right_sidebar_closed() {
        let mut panels = PanelState::new();
        panels.right = PanelMode::Wide;
        let clamped = panels.clamped_for_area(Rect::new(0, 0, 79, 24));
        assert_eq!(clamped.right, PanelMode::Closed);
    }
}
