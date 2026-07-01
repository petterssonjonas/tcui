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

    pub fn render(&mut self, f: &mut Frame, area: Rect, props: ArtifactViewerProps<'_>) {
        let popup_area = popup_area(area);
        let block = Block::default()
            .title(format!(" {} ", self.artifact.name))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .style(Style::default().bg(Color::Black));
        let inner = block.inner(popup_area);
        f.render_widget(Clear, popup_area);
        f.render_widget(block, popup_area);

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2),
                Constraint::Min(0),
                Constraint::Length(1),
            ])
            .margin(1)
            .split(inner);

        let mut x = layout[0].x;
        let save = if self.artifact.can_save(true) {
            let label = if self.artifact.is_markdown() {
                "Save"
            } else {
                "Export"
            };
            let rect = Rect::new(x, layout[0].y, label.len() as u16 + 2, 1);
            x = x.saturating_add(rect.width + 1);
            f.render_widget(
                Paragraph::new(format!("[{label}]")).style(Style::default().fg(Color::Green)),
                rect,
            );
            Some(rect)
        } else {
            None
        };
        let delete = if self.artifact.can_delete() {
            let rect = Rect::new(x, layout[0].y, 5, 1);
            x = x.saturating_add(rect.width + 1);
            f.render_widget(
                Paragraph::new("[Del]").style(Style::default().fg(Color::Red)),
                rect,
            );
            Some(rect)
        } else {
            None
        };
        let close = Rect::new(x, layout[0].y, 7, 1);
        f.render_widget(
            Paragraph::new("[Close]").style(Style::default().fg(Color::Gray)),
            close,
        );

        let badge = format!(
            "{} · {}",
            self.artifact.origin_label(),
            kind_label(self.artifact.kind)
        );
        f.render_widget(
            Paragraph::new(badge)
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Right),
            layout[0],
        );

        match self.artifact.kind {
            ArtifactKind::Image => {
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
                    state.render(f, layout[1]);
                } else {
                    f.render_widget(
                        Paragraph::new("Image preview unavailable")
                            .alignment(Alignment::Center)
                            .style(Style::default().fg(Color::DarkGray)),
                        layout[1],
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
                    layout[1].width.saturating_sub(2) as usize,
                    false,
                    props.kitty_heading_downscale,
                    !props.image_protocol.eq_ignore_ascii_case("off"),
                );
                f.render_widget(
                    Paragraph::new(rendered.lines)
                        .wrap(Wrap { trim: false })
                        .scroll((self.scroll as u16, 0)),
                    layout[1],
                );
            }
        }

        f.render_widget(
            Paragraph::new("Esc closes, mouse wheel scrolls")
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Center),
            layout[2],
        );

        self.hit_areas = ArtifactViewerHitAreas {
            close: Some(close),
            save,
            delete,
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
