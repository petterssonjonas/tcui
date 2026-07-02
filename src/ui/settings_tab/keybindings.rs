use super::*;

impl SettingsPopup {
    pub(super) fn render_keybindings(&self, f: &mut Frame, area: Rect) {
        let bindings = vec![
            ("Ctrl+T", "New Tab"),
            ("Ctrl+N", "New Chat"),
            ("Ctrl+W", "Close Tab"),
            ("Ctrl+Shift+W", "Close Chat"),
            ("Ctrl+B", "Toggle Sidebar"),
            ("Ctrl+,", "Toggle Settings"),
            ("Ctrl+Q", "Quit"),
            ("Ctrl+C", "Cancel / Quit (press twice)"),
            ("Enter", "Send message"),
            ("/quit, /exit, /q", "Quit via chat"),
            ("/theme", "Choose and apply a theme"),
            ("/skills", "Show installed skills"),
            #[cfg(feature = "memory")]
            ("memory-mcp", "Expose memory tools over stdio MCP"),
            ("/mcp", "Show MCP servers"),
            ("/vault <query>", "Search the configured vault"),
            ("/web, /web on, /web off", "Toggle local web search"),
            ("@obsidian", "Search, read, or update the configured vault"),
            #[cfg(feature = "memory")]
            ("@remember", "Save one durable fact or preference"),
            #[cfg(feature = "memory")]
            (
                "@memory / @memorize",
                "Search, read, write, or forget memory",
            ),
            ("@websearch", "Use local web search"),
            ("@save", "Create a sidebar markdown artifact"),
        ];

        let lines: Vec<Line> = bindings
            .iter()
            .map(|&(key, desc)| {
                Line::from(vec![
                    Span::styled(
                        format!("{:22}", key),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(desc),
                ])
            })
            .collect();

        let paragraph = Paragraph::new(lines)
            .block(
                Block::default()
                    .title(" Keyboard Shortcuts ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::DarkGray)),
            )
            .wrap(Wrap { trim: true });

        f.render_widget(paragraph, area);
    }
}
