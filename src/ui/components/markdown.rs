use crate::config::app_config::MarkdownMode;
use crate::ui::components::markdown_model::{render_markdown, RenderOptions, RenderedMarkdown};
use crate::ui::components::terminal_capabilities::TerminalCapabilities;

pub struct MarkdownRenderer {
    terminal_capabilities: TerminalCapabilities,
}

impl MarkdownRenderer {
    pub fn new(terminal_capabilities: TerminalCapabilities) -> Self {
        Self {
            terminal_capabilities,
        }
    }

    pub fn render(
        &self,
        content: &str,
        mode: MarkdownMode,
        width: usize,
        kitty_enhanced_text: bool,
        kitty_text_max_scale: u8,
        image_protocol_enabled: bool,
    ) -> RenderedMarkdown {
        render_markdown(
            content,
            RenderOptions {
                mode,
                width,
                kitty_enhanced_text,
                kitty_text_max_scale,
                image_protocol_enabled,
                terminal_capabilities: self.terminal_capabilities,
            },
        )
    }
}

impl Default for MarkdownRenderer {
    fn default() -> Self {
        Self::new(TerminalCapabilities::detect())
    }
}

#[cfg(test)]
mod tests {
    use crate::ui::components::terminal_capabilities::{TerminalCapabilities, TerminalKind};

    use super::*;

    fn test_renderer() -> MarkdownRenderer {
        MarkdownRenderer::new(TerminalCapabilities {
            terminal: TerminalKind::Unknown,
            multiplexer: None,
            kitty_graphics: false,
            kitty_text_sizing: false,
            tmux_passthrough: false,
        })
    }

    #[test]
    fn markdown_renderer_handles_large_heading_markers() {
        let output = test_renderer().render(
            "####### heading\nbody",
            MarkdownMode::Full,
            80,
            false,
            3,
            false,
        );

        assert!(!output.lines.is_empty());
    }

    #[test]
    fn markdown_renderer_tracks_links_and_images() {
        let output = test_renderer().render(
            "# Demo\n\n[Link](https://example.com)\n\n![alt](image.png)",
            MarkdownMode::Full,
            80,
            false,
            3,
            true,
        );

        assert_eq!(output.link_targets[0].target, "https://example.com");
        assert_eq!(output.images.len(), 1);
    }

    #[test]
    fn markdown_renderer_tracks_skill_mentions_when_markdown_is_off() {
        // Given / When
        let output =
            test_renderer().render("Use @caveman here.", MarkdownMode::Off, 80, false, 3, false);

        // Then
        assert_eq!(output.link_targets.len(), 1);
        assert_eq!(output.link_targets[0].target, "skill:caveman");
        assert_eq!(output.link_targets[0].column, 4);
        assert_eq!(output.link_targets[0].width, 8);
    }

    #[test]
    fn markdown_renderer_tracks_multiple_targets_on_one_line() {
        // Given / When
        let output = test_renderer().render(
            "Use @caveman, [docs](https://example.com), then @save.",
            MarkdownMode::Full,
            80,
            false,
            3,
            false,
        );

        // Then
        assert_eq!(
            output
                .link_targets
                .iter()
                .map(|target| (target.column, target.width, target.target.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (4, 8, "skill:caveman"),
                (14, 4, "https://example.com"),
                (25, 5, "skill:save"),
            ]
        );
    }
}
