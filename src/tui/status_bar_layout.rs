use ratatui::layout::Rect;

#[derive(Debug, Clone)]
pub struct StatusBarConfig {
    pub rows: u8,
    pub widgets: Vec<crate::config::StatusWidgetPlacement>,
}

impl StatusBarConfig {
    pub fn from_tui(config: &crate::config::TuiConfig) -> Self {
        let widgets = if config.status_widgets.is_empty() {
            default_status_widgets()
        } else {
            config.status_widgets.clone()
        };
        Self {
            rows: config.status_bar_rows.clamp(1, 2),
            widgets,
        }
    }
}

impl Default for StatusBarConfig {
    fn default() -> Self {
        Self {
            rows: 1,
            widgets: default_status_widgets(),
        }
    }
}

pub(super) fn slot_rect(area: Rect, row: u8, slot: u8) -> Option<Rect> {
    if area.width == 0 || area.height == 0 {
        return None;
    }
    let row_offset = u16::from(row.saturating_sub(1));
    if row_offset >= area.height {
        return None;
    }
    let slot_index = u16::from(slot.saturating_sub(1));
    let starts = [0, 8, 18, 38, 46, 71];
    let ends = [8, 18, 38, 46, 71, 100];
    let start = scaled_width(area.width, starts[usize::from(slot_index)]);
    let end = scaled_width(area.width, ends[usize::from(slot_index)]);
    Some(Rect::new(
        area.x.saturating_add(start),
        area.y + row_offset,
        end.saturating_sub(start),
        1,
    ))
}

fn default_status_widgets() -> Vec<crate::config::StatusWidgetPlacement> {
    [
        ("web_search", 1, 1),
        ("provider", 1, 2),
        ("model", 1, 3),
        ("reasoning", 1, 4),
        ("context", 1, 5),
        ("connection", 1, 6),
    ]
    .into_iter()
    .map(|(id, row, area)| crate::config::StatusWidgetPlacement {
        id: id.to_string(),
        row,
        area,
    })
    .collect()
}

fn scaled_width(width: u16, percent: u16) -> u16 {
    ((u32::from(width) * u32::from(percent)) / 100) as u16
}
