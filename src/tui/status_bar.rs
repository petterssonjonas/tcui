use ratatui::{prelude::*, widgets::*, Frame};
use unicode_width::UnicodeWidthStr;

use crate::tui::status_bar_layout::slot_rect;
pub use crate::tui::status_bar_layout::StatusBarConfig;

const SELECTOR_MIN_WIDTH: usize = 30;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConnectionStatus {
    Checking,
    CloudConnected,
    LocalConnected,
    LocalModelUnloaded,
    LocalDisabled,
    Failed,
}

pub struct StatusBar {
    pub status: ConnectionStatus,
    pub message: Option<String>,
    pub mcps: Vec<String>,
    pub working: bool,
    pub tick: u64,
    pub provider: String,
    pub model: String,
    pub reasoning_effort: Option<String>,
    pub show_reasoning_selector: bool,
    pub show_selector: bool,
    pub web_search_enabled: bool,
    pub context_window: Option<u32>,
    pub context_used_tokens: Option<u32>,
    pub config: StatusBarConfig,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct StatusBarAreas {
    pub web_search: Option<Rect>,
    pub provider: Option<Rect>,
    pub model: Option<Rect>,
    pub reasoning: Option<Rect>,
}

impl StatusBar {
    pub fn render(&self, f: &mut Frame, area: Rect) -> StatusBarAreas {
        let theme = crate::theme::active_theme();
        f.render_widget(
            Paragraph::new("").style(Style::default().bg(theme.panel)),
            area,
        );

        let rows = self.config.rows.clamp(1, 2).min(area.height.max(1) as u8);
        let mut areas = StatusBarAreas::default();
        for placement in &self.config.widgets {
            let row = placement.row.clamp(1, rows);
            let area_idx = placement.area.clamp(1, 6);
            let Some(slot) = slot_rect(area, row, area_idx) else {
                continue;
            };
            self.render_widget(f, slot, placement.id.as_str(), &mut areas);
        }
        areas
    }

    fn render_widget(
        &self,
        f: &mut Frame,
        area: Rect,
        widget_id: &str,
        areas: &mut StatusBarAreas,
    ) {
        match widget_id {
            "web_search" => self.render_web_search(f, area, areas),
            "provider" => self.render_provider(f, area, areas),
            "model" => self.render_model(f, area, areas),
            "reasoning" => self.render_reasoning(f, area, areas),
            "connection" => self.render_connection(f, area),
            "context" => self.render_context(f, area),
            "hints" => self.render_hints(f, area),
            "mcps" | "tools" => self.render_tools(f, area),
            _ => {}
        }
    }

    fn render_web_search(&self, f: &mut Frame, area: Rect, areas: &mut StatusBarAreas) {
        let theme = crate::theme::active_theme();
        let label = if self.web_search_enabled {
            " web on "
        } else {
            " web off "
        };
        let color = if self.web_search_enabled {
            theme.success
        } else {
            theme.muted
        };
        areas.web_search = render_control(f, area, label, color);
    }

    fn render_provider(&self, f: &mut Frame, area: Rect, areas: &mut StatusBarAreas) {
        if !self.show_selector {
            return;
        }
        let theme = crate::theme::active_theme();
        let provider = if self.provider.is_empty() {
            "none"
        } else {
            self.provider.as_str()
        };
        let color = if self.provider.is_empty() {
            theme.warning
        } else {
            theme.accent
        };
        areas.provider = render_control_with_min_width(
            f,
            area,
            &format!(" {provider} v "),
            color,
            SELECTOR_MIN_WIDTH,
        );
    }

    fn render_model(&self, f: &mut Frame, area: Rect, areas: &mut StatusBarAreas) {
        if !self.show_selector {
            return;
        }
        let theme = crate::theme::active_theme();
        let model = if self.model.is_empty() {
            "none"
        } else {
            self.model.as_str()
        };
        let color = if self.model.is_empty() {
            theme.warning
        } else {
            theme.accent
        };
        areas.model = render_control_with_min_width(
            f,
            area,
            &format!(" {model} v "),
            color,
            SELECTOR_MIN_WIDTH,
        );
    }

    fn render_reasoning(&self, f: &mut Frame, area: Rect, areas: &mut StatusBarAreas) {
        if !self.show_reasoning_selector {
            return;
        }
        let theme = crate::theme::active_theme();
        let effort = self.reasoning_effort.as_deref().unwrap_or("medium");
        areas.reasoning = render_control(f, area, &format!(" {effort} v "), theme.accent);
    }

    fn render_connection(&self, f: &mut Frame, area: Rect) {
        let theme = crate::theme::active_theme();
        let (dot_color, status_text) = match self.status {
            ConnectionStatus::Checking => (
                theme.warning,
                self.message.as_deref().unwrap_or("Checking connection..."),
            ),
            ConnectionStatus::CloudConnected => (theme.success, "Connected"),
            ConnectionStatus::LocalConnected => (
                theme.success,
                self.message.as_deref().unwrap_or("Connected to Local LLM"),
            ),
            ConnectionStatus::LocalModelUnloaded => (
                theme.warning,
                self.message.as_deref().unwrap_or("Local model unloaded"),
            ),
            ConnectionStatus::LocalDisabled => (theme.muted, "Local LLM"),
            ConnectionStatus::Failed => (
                theme.error,
                self.message
                    .as_deref()
                    .unwrap_or("Not connected, check settings"),
            ),
        };
        let paragraph = Paragraph::new(self.connection_line(status_text, dot_color))
            .style(Style::default().bg(theme.panel).fg(theme.foreground))
            .alignment(Alignment::Right);
        f.render_widget(paragraph, area);
    }

    fn render_context(&self, f: &mut Frame, area: Rect) {
        if let Some(label) = self.context_label() {
            let theme = crate::theme::active_theme();
            render_label(
                f,
                area,
                &format!(" {label} "),
                theme.muted,
                Alignment::Center,
            );
        }
    }

    fn render_hints(&self, f: &mut Frame, area: Rect) {
        let theme = crate::theme::active_theme();
        let label = if self.working {
            format!(" Working {} ", spinner_frame(self.tick))
        } else {
            " Ctrl+P palette ".to_string()
        };
        render_label(f, area, &label, theme.muted, Alignment::Center);
    }

    fn render_tools(&self, f: &mut Frame, area: Rect) {
        let theme = crate::theme::active_theme();
        let label = if self.mcps.is_empty() {
            " Tools: 0 ".to_string()
        } else {
            format!(" Tools: {} ", self.mcps.len())
        };
        render_label(f, area, &label, theme.muted, Alignment::Center);
    }

    fn context_label(&self) -> Option<String> {
        let window = self.context_window?;
        let formatted_window = format_token_window(window);
        match self.context_used_tokens {
            Some(used) if used > 0 => {
                let percent = ((used as f64 / window as f64) * 100.0).round() as u32;
                Some(format!("Context: {percent}% used of {formatted_window}"))
            }
            _ => Some(format!("Context: {formatted_window}")),
        }
    }

    fn connection_line(&self, status_text: &str, dot_color: Color) -> Line<'static> {
        let mut spans = Vec::new();
        spans.push(Span::styled("● ", Style::default().fg(dot_color)));
        spans.push(Span::styled(
            status_text.to_string(),
            if self.status == ConnectionStatus::LocalDisabled {
                Style::default()
                    .fg(dot_color)
                    .add_modifier(Modifier::CROSSED_OUT)
            } else {
                Style::default().fg(dot_color)
            },
        ));
        spans.push(Span::raw(" "));
        Line::from(spans)
    }
}

fn render_control(f: &mut Frame, area: Rect, label: &str, color: Color) -> Option<Rect> {
    render_control_with_min_width(f, area, label, color, 0)
}

fn render_control_with_min_width(
    f: &mut Frame,
    area: Rect,
    label: &str,
    color: Color,
    min_width: usize,
) -> Option<Rect> {
    let width = UnicodeWidthStr::width(label)
        .max(min_width)
        .min(area.width as usize) as u16;
    if width == 0 {
        return None;
    }
    let control_area = Rect::new(area.x, area.y, width, 1);
    render_label(f, control_area, label, color, Alignment::Left);
    Some(control_area)
}

fn render_label(f: &mut Frame, area: Rect, label: &str, color: Color, alignment: Alignment) {
    let theme = crate::theme::active_theme();
    f.render_widget(
        Paragraph::new(label.to_string())
            .style(Style::default().fg(color).bg(theme.panel))
            .alignment(alignment),
        area,
    );
}

fn spinner_frame(tick: u64) -> &'static str {
    const FRAMES: [&str; 4] = ["-", "\\", "|", "/"];
    FRAMES[((tick / 4) as usize) % FRAMES.len()]
}

fn format_token_window(window: u32) -> String {
    if window >= 1_000 {
        format!("{}K", window / 1_000)
    } else {
        window.to_string()
    }
}
