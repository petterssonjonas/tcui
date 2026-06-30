#![allow(dead_code)]
use ratatui::{prelude::*, widgets::*, Frame};
use unicode_width::UnicodeWidthStr;

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
}

#[derive(Debug, Default, Clone, Copy)]
pub struct StatusBarAreas {
    pub web_search: Option<Rect>,
    pub provider: Option<Rect>,
    pub model: Option<Rect>,
    pub reasoning: Option<Rect>,
}

impl StatusBar {
    pub fn new(status: ConnectionStatus) -> Self {
        Self {
            status,
            message: None,
            mcps: vec![],
            working: false,
            tick: 0,
            provider: String::new(),
            model: String::new(),
            reasoning_effort: None,
            show_reasoning_selector: false,
            show_selector: false,
            web_search_enabled: false,
            context_window: None,
            context_used_tokens: None,
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect) -> StatusBarAreas {
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

        let status_text = status_text.to_string();
        let left_label = if self.web_search_enabled {
            " web on ".to_string()
        } else {
            " web off ".to_string()
        };
        let right_label = self.right_label(&status_text);
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length((UnicodeWidthStr::width(left_label.as_str()) as u16).min(area.width)),
                Constraint::Min(0),
                Constraint::Length((UnicodeWidthStr::width(right_label.as_str()) as u16).min(area.width)),
            ])
            .split(area);

        let mut areas = if chunks[0].width > 0 {
            self.render_left_controls(f, chunks[0], &left_label)
        } else {
            f.render_widget(
                Paragraph::new("").style(Style::default().bg(theme.panel)),
                chunks[0],
            );
            StatusBarAreas::default()
        };

        if chunks[1].width > 0 {
            let middle_areas = self.render_middle_controls(f, chunks[1]);
            areas.provider = middle_areas.provider;
            areas.model = middle_areas.model;
        } else {
            f.render_widget(
                Paragraph::new("").style(Style::default().bg(theme.panel)),
                chunks[1],
            );
        }

        let status_widget = Paragraph::new(self.right_line(&status_text, dot_color))
            .style(Style::default().bg(theme.panel).fg(theme.foreground))
            .alignment(Alignment::Right);
        f.render_widget(status_widget, chunks[2]);
        areas
    }

    fn render_left_controls(&self, f: &mut Frame, area: Rect, label: &str) -> StatusBarAreas {
        let theme = crate::theme::active_theme();
        f.render_widget(
            Paragraph::new("").style(Style::default().bg(theme.panel)),
            area,
        );
        let mut x = area.x;
        let mut remaining = area.width;
        let mut areas = StatusBarAreas::default();

        let search_color = if self.web_search_enabled {
            theme.success
        } else {
            theme.muted
        };
        if let Some(control_area) = take_control_area(&mut x, area.y, &mut remaining, label)
        {
            f.render_widget(
                Paragraph::new(label)
                    .style(Style::default().fg(search_color).bg(theme.panel)),
                control_area,
            );
            areas.web_search = Some(control_area);
        }
        areas
    }

    fn render_middle_controls(&self, f: &mut Frame, area: Rect) -> StatusBarAreas {
        let theme = crate::theme::active_theme();
        f.render_widget(
            Paragraph::new("").style(Style::default().bg(theme.panel)),
            area,
        );
        let provider_label = if self.show_selector {
            let provider = if self.provider.is_empty() {
                "none"
            } else {
                self.provider.as_str()
            };
            Some(format!(" {provider} v "))
        } else {
            None
        };
        let model_label = if self.show_selector {
            let model = if self.model.is_empty() {
                "none"
            } else {
                self.model.as_str()
            };
            Some(format!(" {model} v "))
        } else {
            None
        };
        let reasoning_label = if self.show_reasoning_selector {
            Some(format!(
                " {} v ",
                self.reasoning_effort.as_deref().unwrap_or("medium")
            ))
        } else {
            None
        };
        let context_label = self.context_label().map(|label| format!(" {label} "));
        let total_width = provider_label
            .as_deref()
            .map(UnicodeWidthStr::width)
            .unwrap_or(0)
            + model_label
                .as_deref()
                .map(UnicodeWidthStr::width)
                .unwrap_or(0)
            + reasoning_label
                .as_deref()
                .map(UnicodeWidthStr::width)
                .unwrap_or(0)
            + context_label
                .as_deref()
                .map(UnicodeWidthStr::width)
                .unwrap_or(0);
        let centered_offset = area
            .width
            .saturating_sub(total_width.min(area.width as usize) as u16)
            / 2;
        let mut x = area.x.saturating_add(centered_offset);
        let mut remaining = area.width.saturating_sub(centered_offset);
        let mut areas = StatusBarAreas::default();

        if let Some(provider_label) = provider_label {
            if let Some(control_area) =
                take_control_area(&mut x, area.y, &mut remaining, &provider_label)
            {
                let provider_color = if self.provider.is_empty() {
                    theme.warning
                } else {
                    theme.accent
                };
                f.render_widget(
                    Paragraph::new(provider_label)
                        .style(Style::default().fg(provider_color).bg(theme.panel)),
                    control_area,
                );
                areas.provider = Some(control_area);
            }
        }
        if let Some(model_label) = model_label {
            if let Some(control_area) =
                take_control_area(&mut x, area.y, &mut remaining, &model_label)
            {
                let model_color = if self.model.is_empty() {
                    theme.warning
                } else {
                    theme.accent
                };
                f.render_widget(
                    Paragraph::new(model_label)
                        .style(Style::default().fg(model_color).bg(theme.panel)),
                    control_area,
                );
                areas.model = Some(control_area);
            }
        }
        if let Some(reasoning_label) = reasoning_label {
            if let Some(control_area) =
                take_control_area(&mut x, area.y, &mut remaining, &reasoning_label)
            {
                f.render_widget(
                    Paragraph::new(reasoning_label)
                        .style(Style::default().fg(theme.accent).bg(theme.panel)),
                    control_area,
                );
                areas.reasoning = Some(control_area);
            }
        }
        if let Some(context_label) = context_label {
            if let Some(control_area) =
                take_control_area(&mut x, area.y, &mut remaining, &context_label)
            {
                f.render_widget(
                    Paragraph::new(context_label)
                        .style(Style::default().fg(theme.muted).bg(theme.panel)),
                    control_area,
                );
            }
        }
        areas
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

    fn right_label(&self, status_text: &str) -> String {
        if self.working {
            format!(" {}  ● {status_text} ", thinking_frame(self.tick))
        } else {
            format!(" ● {status_text} ")
        }
    }

    fn right_line(&self, status_text: &str, dot_color: Color) -> Line<'static> {
        let theme = crate::theme::active_theme();
        let mut spans = Vec::new();
        if self.working {
            spans.push(Span::styled(
                format!(" {}  ", thinking_frame(self.tick)),
                Style::default().fg(theme.accent),
            ));
        }
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

fn take_control_area(x: &mut u16, y: u16, remaining: &mut u16, label: &str) -> Option<Rect> {
    let width = UnicodeWidthStr::width(label);
    if width > *remaining as usize {
        return None;
    }
    let width = width as u16;
    let area = Rect::new(*x, y, width, 1);
    *x = x.saturating_add(width);
    *remaining = remaining.saturating_sub(width);
    Some(area)
}

fn spinner_frame(tick: u64) -> &'static str {
    const FRAMES: [&str; 4] = ["-", "\\", "|", "/"];
    FRAMES[((tick / 4) as usize) % FRAMES.len()]
}

fn thinking_frame(tick: u64) -> String {
    let dots = ".".repeat(((tick / 8) as usize % 3) + 1);
    format!("Thinking{dots}")
}

fn format_token_window(window: u32) -> String {
    if window >= 1_000 {
        format!("{}K", window / 1_000)
    } else {
        window.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn rendered_row(status: StatusBar) -> String {
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("test terminal");
        terminal
            .draw(|frame| {
                status.render(frame, Rect::new(0, 29, 100, 1));
            })
            .expect("render status");
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .skip(29 * 100)
            .take(100)
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn status_and_selector_controls_fit_at_100_columns() {
        // Given
        let status = StatusBar {
            status: ConnectionStatus::Failed,
            message: None,
            mcps: vec![],
            working: false,
            tick: 0,
            provider: "OpenAI".to_string(),
            model: "deepseek-v4-flash".to_string(),
            reasoning_effort: None,
            show_reasoning_selector: false,
            show_selector: true,
            web_search_enabled: true,
            context_window: Some(256_000),
            context_used_tokens: Some(128_000),
        };

        // When
        let row = rendered_row(status);

        // Then
        assert!(row.contains("deepseek-v4-flash"));
        assert!(row.contains("Context: 50% used of 256K"));
        assert!(row.contains("Not connected, check settings"));
    }

    #[test]
    fn status_width_counts_wide_characters() {
        // Given / When
        let row = rendered_row(StatusBar {
            status: ConnectionStatus::Checking,
            message: Some("连接成功".to_string()),
            mcps: vec![],
            working: false,
            tick: 0,
            provider: String::new(),
            model: String::new(),
            reasoning_effort: None,
            show_reasoning_selector: false,
            show_selector: false,
            web_search_enabled: false,
            context_window: None,
            context_used_tokens: None,
        });

        // Then
        assert!(row.replace(' ', "").contains("连接成功"));
    }

    #[test]
    fn selector_toggle_hides_only_provider_and_model() {
        let row = rendered_row(StatusBar {
            status: ConnectionStatus::CloudConnected,
            message: None,
            mcps: vec![],
            working: false,
            tick: 0,
            provider: "OpenCode Go".to_string(),
            model: "deepseek-v4-flash".to_string(),
            reasoning_effort: None,
            show_reasoning_selector: false,
            show_selector: false,
            web_search_enabled: true,
            context_window: None,
            context_used_tokens: None,
        });

        assert!(row.contains("web on"));
        assert!(row.contains("Connected"));
        assert!(!row.contains("provider OpenCode Go"));
        assert!(!row.contains("model deepseek-v4-flash"));
    }
}
