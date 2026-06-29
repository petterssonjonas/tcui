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
    pub show_selector: bool,
    pub web_search_enabled: bool,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct StatusBarAreas {
    pub settings: Option<Rect>,
    pub web_search: Option<Rect>,
    pub provider: Option<Rect>,
    pub model: Option<Rect>,
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
            show_selector: false,
            web_search_enabled: false,
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

        let indicator = if self.working {
            spinner_frame(self.tick)
        } else {
            "●"
        };
        let status_span = Line::from(vec![
            Span::styled(format!(" {indicator} "), Style::default().fg(dot_color)),
            Span::styled(
                status_text,
                if self.status == ConnectionStatus::LocalDisabled {
                    Style::default()
                        .fg(dot_color)
                        .add_modifier(Modifier::CROSSED_OUT)
                } else {
                    Style::default().fg(dot_color)
                },
            ),
            Span::raw(" "),
        ]);

        let status_width = (UnicodeWidthStr::width(status_text) as u16 + 4).min(area.width);
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(area.width.saturating_sub(status_width)),
                Constraint::Length(status_width),
            ])
            .split(area);

        let areas = if chunks[0].width > 0 {
            self.render_controls(f, chunks[0])
        } else {
            f.render_widget(
                Paragraph::new("").style(Style::default().bg(theme.panel)),
                chunks[0],
            );
            StatusBarAreas::default()
        };

        let status_widget = Paragraph::new(status_span)
            .style(Style::default().bg(theme.panel).fg(theme.foreground))
            .alignment(Alignment::Right);
        f.render_widget(status_widget, chunks[1]);
        areas
    }

    fn render_controls(&self, f: &mut Frame, area: Rect) -> StatusBarAreas {
        let theme = crate::theme::active_theme();
        f.render_widget(
            Paragraph::new("").style(Style::default().bg(theme.panel)),
            area,
        );
        let mut x = area.x;
        let mut remaining = area.width;
        let mut areas = StatusBarAreas::default();

        let settings_label = " ⚙ Settings";
        if let Some(control_area) =
            take_control_area(&mut x, area.y, &mut remaining, settings_label)
        {
            f.render_widget(
                Paragraph::new(settings_label)
                    .style(Style::default().fg(theme.muted).bg(theme.panel)),
                control_area,
            );
            areas.settings = Some(control_area);
        }

        let search_label = if self.web_search_enabled {
            " web on "
        } else {
            " web off"
        };
        let search_color = if self.web_search_enabled {
            theme.success
        } else {
            theme.muted
        };
        if let Some(control_area) = take_control_area(&mut x, area.y, &mut remaining, search_label)
        {
            f.render_widget(
                Paragraph::new(search_label)
                    .style(Style::default().fg(search_color).bg(theme.panel)),
                control_area,
            );
            areas.web_search = Some(control_area);
        }

        if !self.show_selector {
            return areas;
        }

        let provider = if self.provider.is_empty() {
            "none"
        } else {
            &self.provider
        };
        let provider_color = if self.provider.is_empty() {
            theme.warning
        } else {
            theme.accent
        };
        let provider_label = format!(" provider {provider} v");
        if let Some(control_area) =
            take_control_area(&mut x, area.y, &mut remaining, &provider_label)
        {
            f.render_widget(
                Paragraph::new(provider_label)
                    .style(Style::default().fg(provider_color).bg(theme.panel)),
                control_area,
            );
            areas.provider = Some(control_area);
        }

        let model = if self.model.is_empty() {
            "none"
        } else {
            &self.model
        };
        let model_color = if self.model.is_empty() {
            theme.warning
        } else {
            theme.accent
        };
        let model_label = format!(" model {model} v");
        if let Some(control_area) = take_control_area(&mut x, area.y, &mut remaining, &model_label)
        {
            f.render_widget(
                Paragraph::new(model_label).style(Style::default().fg(model_color).bg(theme.panel)),
                control_area,
            );
            areas.model = Some(control_area);
        }
        areas
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
            show_selector: true,
            web_search_enabled: true,
        };

        // When
        let row = rendered_row(status);

        // Then
        assert!(row.contains("model deepseek-v4-flash v"));
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
            show_selector: false,
            web_search_enabled: false,
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
            show_selector: false,
            web_search_enabled: true,
        });

        assert!(row.contains("Settings"));
        assert!(row.contains("web on"));
        assert!(row.contains("Connected"));
        assert!(!row.contains("provider OpenCode Go"));
        assert!(!row.contains("model deepseek-v4-flash"));
    }
}
