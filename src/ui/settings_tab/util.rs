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

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    #[test]
    fn backend_label_maps_known_backends() {
        assert_eq!(SettingsPopup::backend_label("openai"), "OpenAI");
        assert_eq!(SettingsPopup::backend_label("anthropic"), "Anthropic");
        assert_eq!(SettingsPopup::backend_label("gemini"), "Gemini");
        assert_eq!(SettingsPopup::backend_label("ollama"), "Ollama");
        assert_eq!(
            SettingsPopup::backend_label("openai-responses"),
            "OpenAI Responses"
        );
    }

    #[test]
    fn backend_label_passes_through_unknown() {
        assert_eq!(SettingsPopup::backend_label("custom"), "custom");
    }

    #[test]
    fn dropdown_area_below_is_positioned_under_anchor() {
        let anchor = Rect::new(10, 5, 20, 3);
        let area = SettingsPopup::dropdown_area_below(anchor, 4);
        assert_eq!(area, Rect::new(10, 8, 20, 4));
    }

    #[test]
    fn centered_rect_fits_inside_parent() {
        let parent = Rect::new(0, 0, 100, 100);
        let rect = SettingsPopup::centered_rect(60, 50, parent);
        assert!(rect.x >= parent.x);
        assert!(rect.y >= parent.y);
        assert!(rect.x + rect.width <= parent.x + parent.width);
        assert!(rect.y + rect.height <= parent.y + parent.height);
        assert!(rect.width >= 50);
        assert!(rect.height >= 40);
    }
}
