use ratatui::{prelude::*, widgets::*, Frame};
use unicode_width::UnicodeWidthStr;

pub struct TopBar<'a> {
    pub tabs: &'a [crate::ui::ChatTabState],
    pub active: usize,
}

#[derive(Debug, Clone)]
pub struct TabHitArea {
    pub index: usize,
    pub area: Rect,
}

const NAV_BUTTON_WIDTH: u16 = 3;
const BRAND_WIDTH: u16 = 8;

impl<'a> TopBar<'a> {
    pub fn new(
        tabs: &'a [crate::ui::ChatTabState],
        active: usize,
        _sidebar_open: bool,
        _artifact_sidebar_open: bool,
    ) -> Self {
        Self { tabs, active }
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let theme = crate::theme::active_theme();
        let chunks = top_bar_chunks(area);

        let bar_style = Style::default().fg(theme.foreground).bg(theme.background);
        f.render_widget(Block::default().style(bar_style), area);

        let brand = Paragraph::new("TCUI")
            .style(
                Style::default()
                    .fg(theme.accent)
                    .bg(theme.background)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Left);
        f.render_widget(brand, chunks[0]);

        let title = Paragraph::new(fit_label(&self.active_title(), chunks[1].width))
            .style(Style::default().fg(theme.foreground).bg(theme.background))
            .alignment(Alignment::Center);
        f.render_widget(title, chunks[1]);

        let close = Paragraph::new("X")
            .style(Style::default().fg(theme.error).bg(theme.sidebar))
            .alignment(Alignment::Center);
        f.render_widget(close, chunks[2]);
    }

    pub fn hamburger_area(&self, area: Rect) -> Rect {
        empty_rect(area)
    }

    pub fn settings_area(&self, area: Rect) -> Rect {
        empty_rect(area)
    }

    pub fn artifact_toggle_area(&self, area: Rect) -> Rect {
        empty_rect(area)
    }

    pub fn close_area(&self, area: Rect) -> Rect {
        let chunks = top_bar_chunks(area);
        chunks[2]
    }

    pub fn tab_hit_areas(&self, area: Rect) -> Vec<TabHitArea> {
        let chunks = top_bar_chunks(area);
        vec![TabHitArea {
            index: self.active,
            area: chunks[1],
        }]
    }

    fn active_title(&self) -> String {
        self.tabs
            .get(self.active)
            .map(|tab| {
                tab.generated_title
                    .as_deref()
                    .unwrap_or(tab.tab.name.as_str())
                    .to_string()
            })
            .unwrap_or_else(|| "Chat".to_string())
    }
}

fn top_bar_chunks(area: Rect) -> [Rect; 3] {
    let close_x = area.right().saturating_sub(NAV_BUTTON_WIDTH);
    let brand_width = BRAND_WIDTH.min(area.width.saturating_sub(NAV_BUTTON_WIDTH));
    let center_x = area.x.saturating_add(brand_width);
    let center_width = close_x.saturating_sub(center_x);
    [
        Rect::new(area.x, area.y, brand_width, area.height),
        Rect::new(center_x, area.y, center_width, area.height),
        Rect::new(
            close_x,
            area.y,
            NAV_BUTTON_WIDTH.min(area.width),
            area.height,
        ),
    ]
}

fn empty_rect(area: Rect) -> Rect {
    Rect::new(area.x, area.y, 0, 0)
}

fn fit_label(input: &str, width: u16) -> String {
    let width = usize::from(width);
    if input.width() <= width {
        return input.to_string();
    }
    if width == 0 {
        return String::new();
    }
    if width == 1 {
        return "…".to_string();
    }

    let mut output = String::new();
    for ch in input.chars() {
        let next_width = output.width() + ch.to_string().width() + 1;
        if next_width > width {
            break;
        }
        output.push(ch);
    }
    output.push('…');
    output
}
