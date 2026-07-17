use ratatui::{
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};
use unicode_width::UnicodeWidthStr;

use crate::ui::markdown_vault_workspace::MarkdownVaultMoveInput;

pub(super) fn render_move_input(frame: &mut Frame, area: Rect, input: &MarkdownVaultMoveInput) {
    let width = area.width.saturating_sub(4).min(76);
    let height = area.height.min(5);
    let popup = Rect::new(
        area.x.saturating_add(area.width.saturating_sub(width) / 2),
        area.y
            .saturating_add(area.height.saturating_sub(height) / 2),
        width,
        height,
    );
    if popup.width < 4 || popup.height < 4 {
        return;
    }
    let theme = crate::theme::active_theme();
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(input.title())
            .border_style(Style::default().fg(theme.accent))
            .style(Style::default().fg(theme.foreground).bg(theme.panel)),
        popup,
    );
    let query = input.destination.query();
    let (before_cursor, after_cursor) = query.split_at(input.destination.cursor());
    let cursor_text = after_cursor
        .chars()
        .next()
        .map_or_else(|| " ".to_string(), |character| character.to_string());
    let after_cursor = after_cursor
        .strip_prefix(&cursor_text)
        .unwrap_or(after_cursor);
    let content = Rect::new(
        popup.x.saturating_add(1),
        popup.y.saturating_add(1),
        popup.width.saturating_sub(2),
        1,
    );
    let label_width = 13_u16.min(content.width);
    let label_area = Rect::new(content.x, content.y, label_width, 1);
    let input_area = Rect::new(
        content.x.saturating_add(label_width),
        content.y,
        content.width.saturating_sub(label_width),
        1,
    );
    frame.render_widget(
        Paragraph::new("Destination: ").style(Style::default().fg(theme.muted)),
        label_area,
    );
    let scroll = u16::try_from(horizontal_scroll(
        query,
        input.destination.cursor(),
        usize::from(input_area.width),
    ))
    .unwrap_or(u16::MAX);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw(before_cursor),
            Span::styled(
                cursor_text,
                Style::default().fg(theme.panel).bg(theme.accent),
            ),
            Span::raw(after_cursor),
        ]))
        .scroll((0, scroll)),
        input_area,
    );
    frame.render_widget(
        Paragraph::new("Enter preview  Esc cancel")
            .alignment(Alignment::Center)
            .style(Style::default().fg(theme.muted).add_modifier(Modifier::DIM)),
        Rect::new(
            popup.x.saturating_add(1),
            popup.y.saturating_add(popup.height.saturating_sub(2)),
            popup.width.saturating_sub(2),
            1,
        ),
    );
}

fn horizontal_scroll(query: &str, cursor: usize, width: usize) -> usize {
    let before_cursor = &query[..cursor];
    let cursor_width = query[cursor..].chars().next().map_or(1, |character| {
        UnicodeWidthStr::width(character.encode_utf8(&mut [0; 4])).max(1)
    });
    UnicodeWidthStr::width(before_cursor)
        .saturating_add(cursor_width)
        .saturating_sub(width)
}

#[cfg(test)]
mod tests {
    #[test]
    fn move_input_horizontal_scroll_keeps_long_unicode_cursor_visible() {
        // Given
        let query = "项目/archive/very-long-markdown-note.md";
        let cursor = query.len();

        // When
        let scroll = super::horizontal_scroll(query, cursor, 12);

        // Then
        assert!(scroll > 0);
        assert_eq!(super::horizontal_scroll(query, 0, 12), 0);
    }
}
