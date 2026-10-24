use ratatui::{prelude::*, widgets::Paragraph, Frame};

pub(crate) struct VaultFooterProps {
    pub(crate) move_available: bool,
}

pub(crate) fn render_footer(frame: &mut Frame, area: Rect, props: VaultFooterProps) {
    let theme = crate::theme::active_theme();
    let label = if props.move_available {
        "Tab focus · Enter view · e edit · c copy · d Trash · + context · i transfer · m move · r rename"
    } else {
        "Tab focus · Enter view · e edit · c copy · d Trash · + context · i transfer"
    };
    frame.render_widget(
        Paragraph::new(label).style(Style::default().fg(theme.muted).bg(theme.panel)),
        area,
    );
}
