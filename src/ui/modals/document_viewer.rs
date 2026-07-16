use std::path::{Path, PathBuf};

use ratatui::{prelude::*, widgets::*, Frame};

use crate::config::app_config::{HeadingDownscale, MarkdownMode};
use crate::ui::components::{
    image_block::ImageBlockState, markdown::MarkdownRenderer,
    terminal_capabilities::TerminalCapabilities,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentKind {
    Markdown,
    Code,
    Image,
    Text,
}

impl DocumentKind {
    pub fn infer(path: Option<&Path>, content: Option<&str>) -> Self {
        let extension = path
            .and_then(Path::extension)
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase);

        match extension.as_deref() {
            Some("md" | "markdown" | "mdown" | "mkd") => Self::Markdown,
            Some(
                "bash" | "c" | "cc" | "cpp" | "cs" | "css" | "fish" | "go" | "h" | "hpp" | "html"
                | "java" | "js" | "jsx" | "kt" | "lua" | "php" | "py" | "rb" | "rs" | "sh" | "sql"
                | "swift" | "toml" | "ts" | "tsx" | "xml" | "zsh",
            ) => Self::Code,
            Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp") => Self::Image,
            None
                if content.is_some_and(|text| {
                    text.starts_with("# ") || text.starts_with("---\n")
                }) =>
            {
                Self::Markdown
            }
            Some(_) | None => Self::Text,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DocumentSource {
    pub kind: DocumentKind,
    pub content: Option<String>,
    pub path: Option<PathBuf>,
}

impl DocumentSource {
    pub fn new(kind: DocumentKind, content: Option<String>, path: Option<PathBuf>) -> Self {
        Self {
            kind,
            content,
            path,
        }
    }

    pub fn inferred(content: Option<String>, path: Option<PathBuf>) -> Self {
        let kind = DocumentKind::infer(path.as_deref(), content.as_deref());
        Self::new(kind, content, path)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DocumentViewerCapabilities {
    pub close: bool,
    pub edit: bool,
    pub save: bool,
    pub delete: bool,
    pub insert_to_chat: bool,
}

impl DocumentViewerCapabilities {
    fn has_footer_actions(self) -> bool {
        self.edit || self.save || self.delete || self.insert_to_chat
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentViewerHitAreas {
    pub close: Option<Rect>,
    pub edit: Option<Rect>,
    pub save: Option<Rect>,
    pub delete: Option<Rect>,
    pub insert_to_chat: Option<Rect>,
}

#[derive(Debug, Clone, Copy)]
pub struct DocumentViewerProps<'a> {
    pub markdown_mode: MarkdownMode,
    pub kitty_enhanced_text: bool,
    pub kitty_heading_downscale: HeadingDownscale,
    pub image_protocol: &'a str,
    pub terminal_capabilities: TerminalCapabilities,
}

pub struct DocumentViewerState {
    pub scroll: usize,
    pub hit_areas: DocumentViewerHitAreas,
    source: DocumentSource,
    image_state: Option<ImageBlockState>,
}

impl DocumentViewerState {
    pub fn new(source: DocumentSource) -> Self {
        Self {
            scroll: 0,
            hit_areas: DocumentViewerHitAreas::default(),
            source,
            image_state: None,
        }
    }

    pub fn clamp_scroll(&mut self, line_count: usize, viewport_height: usize) {
        self.scroll = self
            .scroll
            .min(line_count.saturating_sub(viewport_height.max(1)));
    }

    pub fn render(
        &mut self,
        f: &mut Frame,
        area: Rect,
        props: DocumentViewerProps<'_>,
        capabilities: DocumentViewerCapabilities,
    ) {
        self.hit_areas = DocumentViewerHitAreas::default();
        if area.width == 0 || area.height == 0 {
            self.scroll = 0;
            return;
        }

        let header_height = u16::from(capabilities.close);
        let footer_height = u16::from(
            capabilities.has_footer_actions() && area.height.saturating_sub(header_height) > 0,
        );
        let content_area = Rect::new(
            area.x,
            area.y.saturating_add(header_height),
            area.width,
            area.height
                .saturating_sub(header_height)
                .saturating_sub(footer_height),
        );
        let footer_area = Rect::new(
            area.x,
            area.bottom().saturating_sub(footer_height),
            area.width,
            footer_height,
        );

        if capabilities.close {
            let label = "[x]";
            let width = u16::try_from(label.len()).unwrap_or(u16::MAX);
            if area.width > width {
                let close = Rect::new(
                    area.right().saturating_sub(width).saturating_sub(1),
                    area.y,
                    width,
                    1,
                );
                f.render_widget(
                    Paragraph::new(label).style(Style::default().fg(Color::Red)),
                    close,
                );
                self.hit_areas.close = Some(close);
            }
        }

        self.render_content(f, content_area, props);

        if footer_area.height == 0 {
            return;
        }

        let theme = crate::theme::active_theme();
        let mut left_bound = footer_area.x;
        if capabilities.insert_to_chat {
            let label = "[Insert to chat]";
            let width = u16::try_from(label.len()).unwrap_or(u16::MAX);
            if width <= footer_area.width {
                let rect = Rect::new(footer_area.x, footer_area.y, width, 1);
                f.render_widget(
                    Paragraph::new(label).style(Style::default().fg(Color::Cyan)),
                    rect,
                );
                self.hit_areas.insert_to_chat = Some(rect);
                left_bound = rect.right().saturating_add(1);
            }
        }

        let mut x = footer_area.right().saturating_sub(1);
        for (enabled, label, style, action) in [
            (
                capabilities.edit,
                "[Edit]",
                Style::default().fg(theme.warning),
                DocumentViewerAction::Edit,
            ),
            (
                capabilities.save,
                "[Save]",
                Style::default().fg(Color::Green),
                DocumentViewerAction::Save,
            ),
            (
                capabilities.delete,
                "[Del]",
                Style::default().fg(theme.error),
                DocumentViewerAction::Delete,
            ),
        ] {
            if !enabled {
                continue;
            }
            let width = u16::try_from(label.len()).unwrap_or(u16::MAX);
            let next_x = x.saturating_sub(width);
            if next_x < left_bound || width > footer_area.width {
                continue;
            }
            let rect = Rect::new(next_x, footer_area.y, width, 1);
            f.render_widget(Paragraph::new(label).style(style), rect);
            match action {
                DocumentViewerAction::Edit => self.hit_areas.edit = Some(rect),
                DocumentViewerAction::Save => self.hit_areas.save = Some(rect),
                DocumentViewerAction::Delete => self.hit_areas.delete = Some(rect),
            }
            x = next_x.saturating_sub(1);
        }
    }

    fn render_content(
        &mut self,
        f: &mut Frame,
        content_area: Rect,
        props: DocumentViewerProps<'_>,
    ) {
        if content_area.width == 0 || content_area.height == 0 {
            self.scroll = 0;
            return;
        }

        match self.source.kind {
            DocumentKind::Image => {
                self.scroll = 0;
                if self.image_state.is_none() {
                    if let Some(path) = self.source.path.as_ref().and_then(|path| path.to_str()) {
                        self.image_state = ImageBlockState::from_source(
                            path,
                            props.image_protocol,
                            props.terminal_capabilities,
                        );
                    }
                }
                if let Some(state) = &mut self.image_state {
                    state.render(f, content_area);
                } else {
                    f.render_widget(
                        Paragraph::new("Image preview unavailable")
                            .alignment(Alignment::Center)
                            .style(Style::default().fg(Color::DarkGray)),
                        content_area,
                    );
                }
            }
            DocumentKind::Markdown | DocumentKind::Code | DocumentKind::Text => {
                let file_content = self
                    .source
                    .path
                    .as_ref()
                    .and_then(|path| std::fs::read_to_string(path).ok());
                let content = self
                    .source
                    .content
                    .as_deref()
                    .or(file_content.as_deref())
                    .unwrap_or("Preview unavailable");
                let rendered = MarkdownRenderer::new(props.terminal_capabilities).render(
                    content,
                    props.markdown_mode,
                    usize::from(content_area.width.saturating_sub(2)),
                    props.kitty_enhanced_text,
                    props.kitty_heading_downscale,
                    !props.image_protocol.eq_ignore_ascii_case("off"),
                );
                self.clamp_scroll(rendered.lines.len(), usize::from(content_area.height));
                let vertical_scroll = u16::try_from(self.scroll).unwrap_or(u16::MAX);
                f.render_widget(
                    Paragraph::new(rendered.lines)
                        .wrap(Wrap { trim: false })
                        .scroll((vertical_scroll, 0)),
                    content_area,
                );
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum DocumentViewerAction {
    Edit,
    Save,
    Delete,
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use ratatui::{backend::TestBackend, layout::Rect, Terminal};

    use super::{
        DocumentKind, DocumentSource, DocumentViewerCapabilities, DocumentViewerProps,
        DocumentViewerState,
    };
    use crate::config::app_config::{HeadingDownscale, MarkdownMode};
    use crate::ui::components::terminal_capabilities::{TerminalCapabilities, TerminalKind};

    fn props() -> DocumentViewerProps<'static> {
        DocumentViewerProps {
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
        }
    }

    fn source(kind: DocumentKind, content: &str) -> DocumentSource {
        DocumentSource::new(kind, Some(content.to_string()), None)
    }

    fn row_text(buffer: &ratatui::buffer::Buffer, area: Rect) -> String {
        (area.x..area.right()).fold(String::new(), |mut text, x| {
            text.push_str(buffer[(x, area.y)].symbol());
            text
        })
    }

    #[test]
    fn document_viewer_core_renders_content_at_arbitrary_rect_origin() {
        // Given
        let mut viewer = DocumentViewerState::new(source(DocumentKind::Markdown, "rectproof"));
        let target = Rect::new(5, 3, 12, 2);
        let mut terminal = Terminal::new(TestBackend::new(24, 10)).expect("test terminal");

        // When
        terminal
            .draw(|frame| {
                viewer.render(
                    frame,
                    target,
                    props(),
                    DocumentViewerCapabilities::default(),
                );
            })
            .expect("render document viewer");

        // Then
        let buffer = terminal.backend().buffer();
        assert!(
            row_text(buffer, Rect::new(target.x, target.y, target.width, 1)).contains("rectproof")
        );
        assert_eq!(buffer[(target.x.saturating_sub(1), target.y)].symbol(), " ");
    }

    #[test]
    fn document_viewer_core_clamps_scroll_for_content_and_viewport_sizes() {
        // Given
        let mut viewer = DocumentViewerState::new(source(DocumentKind::Text, "one line"));

        // When / Then
        for (line_count, viewport_height, expected) in
            [(1, 10, 0), (10, 4, 6), (10, 0, 9), (0, 0, 0)]
        {
            viewer.scroll = usize::MAX;
            viewer.clamp_scroll(line_count, viewport_height);
            assert_eq!(viewer.scroll, expected);
        }
    }

    #[test]
    fn document_viewer_core_gates_action_rendering_and_hit_areas() {
        // Given
        let mut viewer = DocumentViewerState::new(source(DocumentKind::Text, "body"));
        let capabilities = DocumentViewerCapabilities {
            close: true,
            edit: true,
            save: false,
            delete: false,
            insert_to_chat: false,
        };
        let mut terminal = Terminal::new(TestBackend::new(40, 8)).expect("test terminal");

        // When
        terminal
            .draw(|frame| viewer.render(frame, frame.area(), props(), capabilities))
            .expect("render document viewer");

        // Then
        let buffer = terminal.backend().buffer();
        let rendered = buffer.content.iter().fold(String::new(), |mut text, cell| {
            text.push_str(cell.symbol());
            text
        });
        assert!(rendered.contains("[x]"));
        assert!(rendered.contains("[Edit]"));
        assert!(!rendered.contains("[Save]"));
        assert!(!rendered.contains("[Del]"));
        assert!(viewer.hit_areas.close.is_some());
        assert!(viewer.hit_areas.edit.is_some());
        assert!(viewer.hit_areas.save.is_none());
        assert!(viewer.hit_areas.delete.is_none());
    }

    #[test]
    fn document_viewer_core_left_action_slot_is_capability_gated() {
        // Given
        let mut viewer = DocumentViewerState::new(source(DocumentKind::Text, "body"));
        let mut terminal = Terminal::new(TestBackend::new(40, 8)).expect("test terminal");

        // When
        terminal
            .draw(|frame| {
                viewer.render(
                    frame,
                    frame.area(),
                    props(),
                    DocumentViewerCapabilities::default(),
                );
            })
            .expect("render without left action");

        // Then
        let rendered_without =
            terminal
                .backend()
                .buffer()
                .content
                .iter()
                .fold(String::new(), |mut text, cell| {
                    text.push_str(cell.symbol());
                    text
                });
        assert!(!rendered_without.contains("[Insert to chat]"));
        assert!(viewer.hit_areas.insert_to_chat.is_none());

        // When
        terminal
            .draw(|frame| {
                viewer.render(
                    frame,
                    frame.area(),
                    props(),
                    DocumentViewerCapabilities {
                        insert_to_chat: true,
                        ..DocumentViewerCapabilities::default()
                    },
                );
            })
            .expect("render with left action");

        // Then
        let rendered_with =
            terminal
                .backend()
                .buffer()
                .content
                .iter()
                .fold(String::new(), |mut text, cell| {
                    text.push_str(cell.symbol());
                    text
                });
        assert!(rendered_with.contains("[Insert to chat]"));
        assert!(viewer.hit_areas.insert_to_chat.is_some());
    }

    #[test]
    fn document_viewer_core_handles_zero_and_tiny_rectangles() {
        // Given
        let mut viewer = DocumentViewerState::new(source(DocumentKind::Text, "body"));
        viewer.scroll = usize::MAX;
        let mut terminal = Terminal::new(TestBackend::new(1, 1)).expect("test terminal");

        // When
        terminal
            .draw(|frame| {
                viewer.render(
                    frame,
                    Rect::new(0, 0, 0, 0),
                    props(),
                    DocumentViewerCapabilities {
                        close: true,
                        edit: true,
                        save: true,
                        delete: true,
                        insert_to_chat: true,
                    },
                );
                viewer.render(
                    frame,
                    Rect::new(0, 0, 1, 1),
                    props(),
                    DocumentViewerCapabilities {
                        close: true,
                        edit: true,
                        save: true,
                        delete: true,
                        insert_to_chat: true,
                    },
                );
            })
            .expect("render tiny document viewer");

        // Then
        assert_eq!(viewer.scroll, 0);
    }

    #[test]
    fn document_viewer_core_infers_supported_content_kinds() {
        // Given / When / Then
        assert_eq!(
            DocumentKind::infer(Some(Path::new("notes.md")), Some("# Notes")),
            DocumentKind::Markdown
        );
        assert_eq!(
            DocumentKind::infer(Some(Path::new("main.rs")), Some("fn main() {}")),
            DocumentKind::Code
        );
        assert_eq!(
            DocumentKind::infer(Some(Path::new("image.png")), None),
            DocumentKind::Image
        );
        assert_eq!(
            DocumentKind::infer(Some(Path::new("notes.txt")), Some("notes")),
            DocumentKind::Text
        );
        assert_eq!(
            DocumentKind::infer(Some(Path::new("README")), Some("notes")),
            DocumentKind::Text
        );
        assert_eq!(
            DocumentSource::inferred(Some("text".to_string()), Some(PathBuf::from("main.rs"))).kind,
            DocumentKind::Code
        );
    }
}
