use ratatui::{prelude::*, widgets::*, Frame};

use crate::config::app_config::{HeadingDownscale, MarkdownMode};
use crate::ui::artifact_sidebar::{ArtifactEntry, ArtifactHandle, ArtifactKind};
use crate::ui::components::terminal_capabilities::TerminalCapabilities;

use self::document_viewer::{
    DocumentKind, DocumentSource, DocumentViewerCapabilities, DocumentViewerProps,
    DocumentViewerState,
};

pub mod document_viewer {
    include!("document_viewer.rs");
}

#[derive(Debug, Clone, Default)]
pub struct ArtifactViewerHitAreas {
    pub close: Option<Rect>,
    pub edit: Option<Rect>,
    pub save: Option<Rect>,
    pub delete: Option<Rect>,
    pub insert_to_chat: Option<Rect>,
}

pub struct ArtifactViewerState {
    pub artifact: ArtifactEntry,
    pub scroll: usize,
    pub view_only: bool,
    pub hit_areas: ArtifactViewerHitAreas,
    document: DocumentViewerState,
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
        let document_kind = match artifact.kind {
            ArtifactKind::Markdown => DocumentKind::Markdown,
            ArtifactKind::Text => {
                DocumentKind::infer(artifact.path.as_deref(), artifact.content.as_deref())
            }
            ArtifactKind::Image => DocumentKind::Image,
            ArtifactKind::Video | ArtifactKind::Audio | ArtifactKind::Binary => DocumentKind::Text,
        };
        let document = DocumentViewerState::new(DocumentSource::new(
            document_kind,
            artifact.content.clone(),
            artifact.path.clone(),
        ));
        Self {
            artifact,
            scroll: 0,
            view_only: false,
            hit_areas: ArtifactViewerHitAreas::default(),
            document,
        }
    }

    pub fn handle(&self) -> &ArtifactHandle {
        &self.artifact.handle
    }

    pub fn clamp_scroll(&mut self, line_count: usize, viewport_height: usize) {
        self.document.scroll = self.scroll;
        self.document.clamp_scroll(line_count, viewport_height);
        self.scroll = self.document.scroll;
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect, props: ArtifactViewerProps<'_>) {
        let access = crate::protection::require_app().and_then(|()| match &self.artifact.handle {
            ArtifactHandle::Memory(_) => {
                crate::protection::require(crate::protection::Domain::Memories)
            }
            ArtifactHandle::ChatFile { .. } | ArtifactHandle::Saved(_) => {
                crate::protection::require(crate::protection::Domain::Vault)
            }
            _ => {
                if let Some(path) = &self.artifact.path {
                    crate::vault::require_root(path)?;
                }
                Ok(())
            }
        });
        if let Err(error) = access {
            let popup = popup_area(area);
            f.render_widget(Clear, popup);
            f.render_widget(
                Paragraph::new(format!("{error}\nUse /unlock to view this artifact.")).block(
                    Block::default()
                        .title("Locked artifact")
                        .borders(Borders::ALL),
                ),
                popup,
            );
            self.hit_areas = ArtifactViewerHitAreas::default();
            return;
        }
        let theme = crate::theme::active_theme();
        let popup_area = popup_area(area);
        let block = Block::default().style(Style::default().bg(theme.code_bg));
        let inner = Rect::new(
            popup_area.x + 1,
            popup_area.y + 2,
            popup_area.width.saturating_sub(2),
            popup_area.height.saturating_sub(3),
        );
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

        let close_label = "[x]";
        let close_len = close_label.len() as u16;
        let close_x = popup_area.x + popup_area.width.saturating_sub(close_len + 1);
        let close = Rect::new(close_x, title_y, close_len, 1);
        f.render_widget(
            Paragraph::new(close_label).style(Style::default().fg(Color::Red)),
            close,
        );

        let document_area = if self.view_only {
            Rect::new(
                inner.x,
                inner.y,
                inner.width,
                inner.height.saturating_sub(1),
            )
        } else {
            inner
        };
        self.document.scroll = self.scroll;
        self.document.render(
            f,
            document_area,
            DocumentViewerProps {
                markdown_mode: props.markdown_mode,
                kitty_enhanced_text: false,
                kitty_heading_downscale: props.kitty_heading_downscale,
                image_protocol: props.image_protocol,
                terminal_capabilities: props.terminal_capabilities,
            },
            DocumentViewerCapabilities {
                close: false,
                edit: !self.view_only && self.artifact.path.is_some(),
                save: false,
                delete: !self.view_only,
                insert_to_chat: self.can_insert_to_chat(),
            },
        );
        self.scroll = self.document.scroll;

        self.hit_areas = ArtifactViewerHitAreas {
            close: Some(close),
            edit: self.document.hit_areas.edit,
            save: self.document.hit_areas.save,
            delete: self.document.hit_areas.delete,
            insert_to_chat: self.document.hit_areas.insert_to_chat,
        };
    }

    fn can_insert_to_chat(&self) -> bool {
        !self.view_only
            && matches!(
                self.artifact.kind,
                ArtifactKind::Markdown | ArtifactKind::Text
            )
            && self.artifact.content.as_ref().is_some_and(|content| {
                !content.is_empty() && content.len() <= crate::app::submission::MAX_DOCUMENT_BYTES
            })
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
            Constraint::Percentage(10),
            Constraint::Percentage(80),
            Constraint::Percentage(10),
        ])
        .split(popup_layout[1])[1]
}

#[cfg(test)]
mod tests {
    use super::{ArtifactViewerProps, ArtifactViewerState};
    use crate::config::app_config::{HeadingDownscale, MarkdownMode};
    use crate::ui::components::terminal_capabilities::{TerminalCapabilities, TerminalKind};
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn markdown_vault_baseline_viewer_content_stays_inside_inner_gutter() {
        let _guard = crate::test_support::env_lock().lock().expect("env lock");
        let artifact = crate::ui::artifact_sidebar::ArtifactEntry::temp_markdown(
            1,
            "short.md".to_string(),
            "gutterproof".to_string(),
        );
        let mut viewer = ArtifactViewerState::new(artifact);

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
                )
            })
            .expect("render viewer");

        let buffer = terminal.backend().buffer();
        let popup_area = super::popup_area(buffer.area);
        let gutter_row = popup_area.y + 1;
        let inner_row = popup_area.y + 2;
        let inner_col = popup_area.x + 1;
        let body_x = (0..buffer.area.width)
            .find(|&x| buffer[(x, inner_row)].symbol().contains('g'))
            .expect("viewer body");

        assert!(body_x >= inner_col);
        assert!(!buffer[(body_x, gutter_row)].symbol().contains('g'));
        assert!(buffer[(body_x, inner_row)].symbol().contains('g'));
    }

    #[test]
    fn markdown_vault_baseline_viewer_handles_tiny_rectangles() {
        let _guard = crate::test_support::env_lock().lock().expect("env lock");
        let artifact = crate::ui::artifact_sidebar::ArtifactEntry::temp_markdown(
            1,
            "short.md".to_string(),
            "x".to_string(),
        );
        let mut viewer = ArtifactViewerState::new(artifact);

        let mut terminal = Terminal::new(TestBackend::new(12, 6)).expect("test terminal");
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
                )
            })
            .expect("render viewer");
    }

    #[test]
    fn preview_scroll_is_clamped_to_rendered_content() {
        let _guard = crate::test_support::env_lock().lock().expect("env lock");
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

#[cfg(test)]
#[path = "artifact_viewer_insert_to_chat_tests.rs"]
mod insert_to_chat_tests;
