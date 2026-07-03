use ratatui::{prelude::*, widgets::*, Frame};

use crate::config::app_config::{HeadingDownscale, MarkdownMode};
use crate::ui::artifact_sidebar::{ArtifactEntry, ArtifactHandle, ArtifactKind};
use crate::ui::components::{
    image_block::ImageBlockState, markdown::MarkdownRenderer,
    terminal_capabilities::TerminalCapabilities,
};

#[derive(Debug, Clone, Default)]
pub struct ArtifactViewerHitAreas {
    pub close: Option<Rect>,
    pub save: Option<Rect>,
    pub delete: Option<Rect>,
}

pub struct ArtifactViewerState {
    pub artifact: ArtifactEntry,
    pub scroll: usize,
    pub hit_areas: ArtifactViewerHitAreas,
    image_state: Option<ImageBlockState>,
}

pub struct ArtifactViewerProps<'a> {
    pub markdown_mode: MarkdownMode,
    pub kitty_enhanced_text: bool,
    pub kitty_heading_downscale: HeadingDownscale,
    pub image_protocol: &'a str,
    pub terminal_capabilities: TerminalCapabilities,
}

impl ArtifactViewerState {
    pub fn new(artifact: ArtifactEntry) -> Self {
        Self {
            artifact,
            scroll: 0,
            hit_areas: ArtifactViewerHitAreas::default(),
            image_state: None,
        }
    }

    pub fn handle(&self) -> &ArtifactHandle {
        &self.artifact.handle
    }

    pub fn clamp_scroll(&mut self, line_count: usize, viewport_height: usize) {
        self.scroll = self
            .scroll
            .min(line_count.saturating_sub(viewport_height.max(1)));
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect, props: ArtifactViewerProps<'_>) {
        let popup_area = popup_area(area);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .style(Style::default().bg(Color::Black));
        let inner = block.inner(popup_area);
        f.render_widget(Clear, popup_area);
        f.render_widget(block, popup_area);
        let title_y = popup_area.y;

        let name_label = format!(" {} ", self.artifact.name);
        f.render_widget(
            Paragraph::new(name_label).style(Style::default().fg(Color::Cyan)),
            Rect::new(
                popup_area.x + 1,
                title_y,
                self.artifact.name.len() as u16 + 2,
                1,
            ),
        );

        let center_label = " Viewer ";
        let center_x = popup_area.x + popup_area.width / 2 - center_label.len() as u16 / 2;
        f.render_widget(
            Paragraph::new(center_label).style(Style::default().fg(Color::Cyan)),
            Rect::new(center_x, title_y, center_label.len() as u16, 1),
        );

        let right_label = " Esc to close [x]";
        let right_len = right_label.len() as u16;
        let right_x = popup_area.x + popup_area.width.saturating_sub(right_len);
        f.render_widget(
            Paragraph::new(right_label).style(Style::default().fg(Color::Gray)),
            Rect::new(right_x, title_y, right_len, 1),
        );
        let close = Rect::new(right_x + right_len - 3, title_y, 3, 1);

        match self.artifact.kind {
            ArtifactKind::Image => {
                self.scroll = 0;
                if self.image_state.is_none() {
                    if let Some(path) = self.artifact.path.as_ref().and_then(|path| path.to_str()) {
                        self.image_state = ImageBlockState::from_source(
                            path,
                            props.image_protocol,
                            props.terminal_capabilities,
                        );
                    }
                }
                if let Some(state) = &mut self.image_state {
                    state.render(f, inner);
                } else {
                    f.render_widget(
                        Paragraph::new("Image preview unavailable")
                            .alignment(Alignment::Center)
                            .style(Style::default().fg(Color::DarkGray)),
                        inner,
                    );
                }
            }
            _ => {
                let file_content = self
                    .artifact
                    .path
                    .as_ref()
                    .and_then(|path| std::fs::read_to_string(path).ok());
                let content = self
                    .artifact
                    .content
                    .as_deref()
                    .or(file_content.as_deref())
                    .unwrap_or("Preview unavailable");
                let rendered = MarkdownRenderer::new(props.terminal_capabilities).render(
                    content,
                    props.markdown_mode,
                    inner.width.saturating_sub(2) as usize,
                    false,
                    props.kitty_heading_downscale,
                    !props.image_protocol.eq_ignore_ascii_case("off"),
                );
                self.clamp_scroll(rendered.lines.len(), usize::from(inner.height));
                f.render_widget(
                    Paragraph::new(rendered.lines)
                        .wrap(Wrap { trim: false })
                        .scroll((self.scroll as u16, 0)),
                    inner,
                );
            }
        }

        self.hit_areas = ArtifactViewerHitAreas {
            close: Some(close),
            save: None,
            delete: None,
        };
    }
}

pub fn popup_area(area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(8),
            Constraint::Percentage(84),
            Constraint::Percentage(8),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(6),
            Constraint::Percentage(88),
            Constraint::Percentage(6),
        ])
        .split(popup_layout[1])[1]
}

fn kind_label(kind: ArtifactKind) -> &'static str {
    match kind {
        ArtifactKind::Markdown => "md",
        ArtifactKind::Text => "text",
        ArtifactKind::Image => "image",
        ArtifactKind::Video => "video",
        ArtifactKind::Audio => "audio",
        ArtifactKind::Binary => "binary",
    }
}

#[cfg(test)]
mod tests {
    use super::{ArtifactViewerProps, ArtifactViewerState};
    use crate::config::app_config::{HeadingDownscale, MarkdownMode};
    use crate::ui::components::terminal_capabilities::{TerminalCapabilities, TerminalKind};
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn preview_scroll_is_clamped_to_rendered_content() {
        // Given
        let artifact = crate::ui::artifact_sidebar::ArtifactEntry::temp_markdown(
            1,
            "short.md".to_string(),
            "one line".to_string(),
        );
        let mut viewer = ArtifactViewerState::new(artifact);
        viewer.scroll = 100;

        // When
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| {
                viewer.render(
                    frame,
                    frame.area(),
                    ArtifactViewerProps {
                        markdown_mode: MarkdownMode::Full,
                        kitty_enhanced_text: false,
                        kitty_heading_downscale: HeadingDownscale::None,
                        image_protocol: "off",
                        terminal_capabilities: TerminalCapabilities {
                            terminal: TerminalKind::Unknown,
                            multiplexer: None,
                            kitty_graphics: false,
                            kitty_text_sizing: false,
                            tmux_passthrough: false,
                        },
                    },
                );
            })
            .expect("render artifact viewer");

        // Then
        assert_eq!(viewer.scroll, 0);
    }
}
