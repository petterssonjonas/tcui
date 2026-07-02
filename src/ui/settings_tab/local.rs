use super::*;

impl SettingsPopup {
pub(super) fn render_local(&mut self, f: &mut Frame, area: Rect) {
    self.local_hit_areas = LocalHitAreas::default();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(2),
        ])
        .margin(1)
        .split(area);

    let enabled_focused =
        self.active_tab == SettingsTab::Local && self.local_focus == LocalFocus::Enabled;
    let enabled = Paragraph::new(vec![Line::from(vec![
        Span::raw(if self.local_enabled { "[✓] " } else { "[ ] " }),
        Span::styled(
            "Enable local inference",
            if enabled_focused {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            },
        ),
    ])])
    .block(Block::default().borders(Borders::ALL).border_style(
        if enabled_focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        },
    ));
    self.local_hit_areas.enabled = Some(chunks[0]);
    f.render_widget(enabled, chunks[0]);

    Self::render_local_text_field(
        f,
        chunks[1],
        " Host ",
        &self.local_host,
        self.local_focus == LocalFocus::Host,
        &mut self.local_hit_areas.host,
    );
    Self::render_local_text_field(
        f,
        chunks[2],
        " Port ",
        &self.local_port,
        self.local_focus == LocalFocus::Port,
        &mut self.local_hit_areas.port,
    );
    Self::render_local_text_field(
        f,
        chunks[3],
        " Server Type ",
        self.local_server_type.label(),
        self.local_focus == LocalFocus::ServerType,
        &mut self.local_hit_areas.server_type,
    );
    Self::render_local_text_field(
        f,
        chunks[4],
        " Selected Model ",
        &self.local_selected_model,
        self.local_focus == LocalFocus::SelectedModel,
        &mut self.local_hit_areas.selected_model,
    );
    Self::render_local_text_field(
        f,
        chunks[5],
        " Model Directory ",
        &self.local_model_directory,
        self.local_focus == LocalFocus::ModelDirectory,
        &mut self.local_hit_areas.model_directory,
    );
    Self::render_local_text_field(
        f,
        chunks[6],
        " Health Interval (s) ",
        &self.local_health_interval_seconds,
        self.local_focus == LocalFocus::HealthInterval,
        &mut self.local_hit_areas.health_interval,
    );
    Self::render_local_text_field(
        f,
        chunks[7],
        " Connect Timeout (ms) ",
        &self.local_connect_timeout_ms,
        self.local_focus == LocalFocus::ConnectTimeout,
        &mut self.local_hit_areas.connect_timeout,
    );
    Self::render_local_text_field(
        f,
        chunks[8],
        " Request Timeout (ms) ",
        &self.local_request_timeout_ms,
        self.local_focus == LocalFocus::RequestTimeout,
        &mut self.local_hit_areas.request_timeout,
    );
    Self::render_local_text_field(
        f,
        chunks[9],
        " API Token Env ",
        &self.local_api_token_env,
        self.local_focus == LocalFocus::ApiTokenEnv,
        &mut self.local_hit_areas.api_token_env,
    );

    let detected_label = self
        .detected_local_server
        .map(LocalServerType::label)
        .unwrap_or("Not checked");
    f.render_widget(
        Paragraph::new(format!(
            "Detected: {detected_label}    server command management is disabled"
        ))
        .style(Style::default().fg(Color::DarkGray)),
        chunks[10],
    );
}

pub(super) fn render_local_text_field(
    f: &mut Frame,
    area: Rect,
    title: &str,
    value: &str,
    focused: bool,
    hit_area: &mut Option<Rect>,
) {
    *hit_area = Some(area);
    f.render_widget(
        Paragraph::new(if value.trim().is_empty() { " " } else { value })
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(if focused {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    }),
            )
            .style(if focused {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::Gray)
            }),
        area,
    );
}
}

pub(crate) fn next_local_server_type(current: LocalServerType) -> LocalServerType {
    match current {
        LocalServerType::Auto => LocalServerType::Ollama,
        LocalServerType::Ollama => LocalServerType::LlamaCpp,
        LocalServerType::LlamaCpp => LocalServerType::LmStudio,
        LocalServerType::LmStudio => LocalServerType::OpenAiCompat,
        LocalServerType::OpenAiCompat => LocalServerType::Auto,
    }
}
