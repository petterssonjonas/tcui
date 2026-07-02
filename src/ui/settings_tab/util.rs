use super::*;

impl SettingsPopup {
    pub(super) fn dropdown_area_below(anchor: Rect, height: u16) -> Rect {
        Rect::new(anchor.x, anchor.y + anchor.height, anchor.width, height)
    }

    pub(super) fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
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

    pub(super) fn centered_rect_in(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
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

    pub(super) fn backend_label(value: &str) -> String {
        match value {
            "openai" => "OpenAI".to_string(),
            "anthropic" => "Anthropic".to_string(),
            "gemini" => "Gemini".to_string(),
            "ollama" => "Ollama".to_string(),
            "openai-responses" => "OpenAI Responses".to_string(),
            "alibaba" => "Alibaba".to_string(),
            _ => value.to_string(),
        }
    }
}
