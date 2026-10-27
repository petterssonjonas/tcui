use super::{code::render_code, parse_blocks, render_markdown, tests::opts, Block};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::Line,
    widgets::{Paragraph, Widget, Wrap},
};
use unicode_width::UnicodeWidthStr;

// The reported mockup, including its original uneven right-hand padding.
const SCREENSHOT_BOX: &str = "┌ Chat ───────── Terminal ───────── Decisions [1] ──────┐\n│                                                     │\n│ Question                                            │\n│ [ How should I install Node.js on this machine?    ] │\n│                                                     │\n│ Options                                             │\n│  1. [ Distribution packages                       ] │\n│  2. [ Per-user version manager                    ] │\n│  3. [ Official upstream binary                    ] │\n│     [+ Add option]                                  │\n│                                                     │\n│ Priorities                                          │\n│ [x] Avoid root access   [x] Support multiple versions │\n│ [ ] Minimize changes   [x] Easy rollback             │\n│                                                     │\n│ Context: distro unknown · shell bash · dev machine  │\n│                                                     │\n│ [ Evaluate ] [ Ask for missing context ] [ Cancel ] │\n└─────────────────────────────────────────────────────┘";

fn code(content: &str, language: Option<&str>, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    render_code(&mut lines, language, content, width);
    lines
}

fn frame_rows(lines: &[Line<'static>]) -> Vec<String> {
    lines
        .iter()
        .map(ToString::to_string)
        .filter(|row| matches!(row.trim_start().chars().next(), Some('┌' | '│' | '└')))
        .collect()
}

#[test]
fn screenshot_mockup_has_consistent_frame_columns_at_wide_and_narrow_widths() {
    for width in [80, 56, 40, 20, 8, 4] {
        let rendered = render_markdown(&format!("```text\n{SCREENSHOT_BOX}\n```"), opts(width));
        let rows = frame_rows(&rendered.lines);
        let expected_width = rows[0].width();
        assert!(expected_width <= width);
        assert!(
            rows.iter().all(|row| row.width() == expected_width),
            "{rows:#?}"
        );
        assert!(rows.first().unwrap().ends_with('┐'));
        assert!(rows.last().unwrap().ends_with('┘'));
        assert!(rows[1..rows.len() - 1].iter().all(|row| row.ends_with('│')));
        assert!(rendered.lines.iter().all(|line| line.width() <= width));
        let interior: String = rows
            .iter()
            .skip(1)
            .take(rows.len() - 2)
            .map(|row| row.trim_start_matches('│').trim_end_matches('│'))
            .collect::<String>()
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .collect();
        for content in [
            "HowshouldIinstallNode.jsonthismachine?",
            "Distributionpackages",
            "Supportmultipleversions",
            "Askformissingcontext",
            "Decisions[1]",
        ] {
            // The title remains on the top edge when it fits there.
            let all: String = rows
                .join("")
                .chars()
                .filter(|ch| !ch.is_whitespace())
                .collect();
            assert!(
                interior.contains(content) || all.contains(content),
                "{content}: {rows:#?}"
            );
        }
    }
}

#[test]
fn preformatted_source_keeps_original_padding_tabs_and_indentation_for_copying() {
    let original = format!("{SCREENSHOT_BOX}\n\tindented  \n");
    let markdown = format!("```text\n{original}```");
    let blocks = parse_blocks(&markdown);
    assert!(
        matches!(&blocks[0], Block::Code { content, .. } if content == original.trim_end_matches('\n'))
    );
    let input = markdown.clone();
    let _ = render_markdown(&markdown, opts(20));
    assert_eq!(markdown, input);
}

#[test]
fn unicode_tabs_and_long_frame_contents_keep_graphemes_and_indentation() {
    let source =
        "  ╭────────╮\n  │\t界👩‍💻é family 👨‍👩‍👧‍👦 and long content│\n  │  second row │\n  ╰────────╯";
    for width in [80, 25, 12, 6] {
        let lines = code(source, Some("text"), width);
        assert!(lines.iter().all(|line| line.width() <= width));
        let rows: Vec<String> = lines
            .iter()
            .map(ToString::to_string)
            .filter(|row| matches!(row.trim_start().chars().next(), Some('╭' | '│' | '╰')))
            .collect();
        assert!(rows.iter().all(|row| row.width() == rows[0].width()));
        let joined = rows.join("\n");
        for grapheme in ["界", "👩‍💻", "é", "👨‍👩‍👧‍👦"] {
            assert_eq!(joined.matches(grapheme).count(), 1, "{joined}");
        }
        assert!(rows[0].starts_with("  ╭"));
        assert!(rows
            .iter()
            .skip(1)
            .take(rows.len() - 2)
            .all(|row| row.starts_with("  │")));
    }
    let expanded = code("界\tvalue", None, 80);
    assert_eq!(expanded[0].to_string(), "界      value");
}

#[test]
fn ordinary_code_preserves_every_character_and_marks_wrapped_continuations() {
    let original = "    let value = \"👩‍💻é世界\";  ";
    let wide = code(original, Some("rust"), 80);
    assert_eq!(wide[1].to_string(), original);
    let wrapped = code(original, Some("rust"), 10);
    assert!(wrapped[0].to_string().contains("rust"));
    assert!(wrapped.iter().any(|line| line.to_string().starts_with('↪')));
    let header_lines = code("", Some("rust · wrapped"), 10).len() - 1;
    let recovered: String = wrapped
        .iter()
        .skip(header_lines)
        .map(|line| {
            let row = line.to_string();
            row.strip_prefix("↪ ")
                .or_else(|| row.strip_prefix('↪'))
                .unwrap_or(&row)
                .to_string()
        })
        .collect();
    assert_eq!(recovered, original);
    assert!(wrapped.iter().all(|line| line.width() <= 10));
}

#[test]
fn normal_language_and_unrecognized_art_are_not_rewritten_as_frames() {
    for source in ["┌──┐\n│ x │\n└──┘", "┌──┐\nnot a framed row\n└──┘"]
    {
        let lines = code(source, Some("rust"), 80);
        assert_eq!(
            lines
                .iter()
                .skip(1)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
            source
        );
    }
}

#[test]
fn tiny_viewports_escape_wide_graphemes_and_reflow_boxes_without_broken_borders() {
    for width in [0, 1, 2, 3] {
        let lines = code("┌────┐\n│界🙂 content│\n└────┘", Some("text"), width);
        assert!(lines.iter().all(|line| line.width() <= width.max(1)));
        assert!(!lines.iter().any(|line| line.to_string().contains('┌')));
    }
    let lines = code("🙂", None, 1);
    assert!(lines
        .iter()
        .map(ToString::to_string)
        .collect::<String>()
        .contains("\\u{1f642}"));
}

#[test]
fn rendered_code_controls_cannot_escape_to_terminal_and_blank_lines_survive() {
    let lines = code("a\u{1b}[31mb\n\n  \n", None, 80);
    assert_eq!(
        lines.iter().map(ToString::to_string).collect::<Vec<_>>(),
        ["a\\u{1b}[31mb", "", "  ", ""]
    );
    assert!(!lines
        .iter()
        .flat_map(|line| line.to_string().chars().collect::<Vec<_>>())
        .any(char::is_control));
}

#[test]
fn chat_and_viewer_paragraphs_keep_preformatted_geometry_and_row_counts() {
    let width = 24;
    let lines = code(SCREENSHOT_BOX, Some("text"), width);
    let area = Rect::new(0, 0, width as u16, lines.len() as u16);
    let mut chat = Buffer::empty(area);
    let mut viewer = Buffer::empty(area);
    Paragraph::new(lines.clone()).render(area, &mut chat);
    Paragraph::new(lines.clone())
        .wrap(Wrap { trim: false })
        .render(area, &mut viewer);
    assert_eq!(chat, viewer);
    for (y, line) in lines.iter().enumerate() {
        let text = line.to_string();
        if text.starts_with('│') {
            assert_eq!(chat[(width as u16 - 1, y as u16)].symbol(), "│");
        }
    }
}

#[test]
fn aligned_ascii_and_double_frames_preserve_geometry_and_internal_spaces() {
    for source in [
        "+--------+\n| a  b   |\n+--------+",
        "╔════════╗\n║ 界  x  ║\n╚════════╝",
    ] {
        let lines = code(source, None, 80);
        assert_eq!(
            lines
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
            source
        );
    }
}

#[test]
fn frame_padding_allocation_stays_proportional_to_input_size() {
    let source = format!(
        "┌{}┐\n{}└{}┘",
        "─".repeat(10_000),
        "│ │\n".repeat(1_000),
        "─".repeat(10_000)
    );
    let lines = code(&source, Some("text"), usize::MAX);
    let output_bytes: usize = lines.iter().map(|line| line.to_string().len()).sum();
    assert!(output_bytes < source.len() * 16);
    assert!(lines.iter().all(|line| line.width() <= u16::MAX as usize));
}

fn emphasized_commands(lines: &[Line<'_>]) -> String {
    lines
        .iter()
        .skip(1)
        .flat_map(|line| &line.spans)
        .filter(|span| {
            span.style
                .add_modifier
                .contains(ratatui::style::Modifier::BOLD)
        })
        .map(|span| span.content.as_ref())
        .collect()
}

#[test]
fn shell_commands_stand_out_from_screenshot_labels_and_conditionals() {
    let source = concat!(
        "(\n",
        "  printf '%s\\n' '=== Local IP addresses ==='\n",
        "  if ip -brief address 2>&1; then\n",
        "    printf '%s\\n' 'ip address exit status: 0'\n",
        "  else\n",
        "    printf 'ip address exit status: %s\\n' \"$?\"\n",
        "  fi\n",
        "  printf '\\n%s\\n' '=== MAC addresses ==='\n",
        "  if ip -brief link 2>&1; then\n",
        "    printf '%s\\n' 'ip link exit status: 0'\n",
        "  fi\n",
        ")",
    );
    let rendered = code(source, Some("bash"), 100);
    let emphasized = emphasized_commands(&rendered);
    assert!(emphasized.contains("ip -brief address"));
    assert!(emphasized.contains("ip -brief link"));
    for scaffolding in ["printf", "then", "else", "fi", "Local IP", "MAC addresses"] {
        assert!(!emphasized.contains(scaffolding), "{emphasized:?}");
    }
    let command_color = rendered
        .iter()
        .flat_map(|line| &line.spans)
        .find(|span| span.content.contains("ip -brief address"))
        .unwrap()
        .style
        .fg;
    let label_color = rendered
        .iter()
        .flat_map(|line| &line.spans)
        .find(|span| span.content.contains("=== Local IP"))
        .unwrap()
        .style
        .fg;
    assert_ne!(command_color, label_color);
}

#[test]
fn shell_emphasis_excludes_comments_literal_labels_and_heredoc_data() {
    let source = concat!(
        "# ip -brief address is only a comment\n",
        "printf '\\n%s\\n' '--- if ip -brief link; then ---'\n",
        "cat <<'EOF'\n",
        "ip -brief address\n",
        "if quoted text; then\n",
        "EOF\n",
        "ip -brief link\n",
    );
    let emphasized = emphasized_commands(&code(source, Some("sh"), 100));
    assert!(emphasized.contains("cat"));
    assert!(emphasized.contains("ip -brief link"));
    assert!(!emphasized.contains("ip -brief address"), "{emphasized:?}");
    assert!(!emphasized.contains("then"), "{emphasized:?}");
    assert!(!emphasized.contains("printf"), "{emphasized:?}");
}

#[test]
fn printing_commands_with_real_work_are_not_mistaken_for_annotations() {
    for source in [
        "printf '%s\\n' 'hello'",
        "echo hello",
        "printf '%s\\n' '--- report ---' > report.txt",
        "printf '%s\\n' \"--- $(id) ---\"",
        "printf '%s\\n' '--- not a label ---' | cat",
    ] {
        let emphasized = emphasized_commands(&code(source, Some("bash"), 120));
        assert!(
            emphasized.starts_with("printf") || emphasized.starts_with("echo"),
            "{source}: {emphasized:?}"
        );
    }
}

#[test]
fn shell_emphasis_tracks_wrapped_unicode_and_expanded_tabs() {
    let command = "ip -brief address dev '界👩‍💻é接口-long-interface-name'";
    for language in ["sh", "bash", "shell", "zsh", "fish", "BASH"] {
        for width in [24, 40, 100] {
            let rendered = code(&format!("\t{command}"), Some(language), width);
            assert_eq!(
                emphasized_commands(&rendered),
                command,
                "{language}: {width}"
            );
            assert!(rendered.iter().all(|line| line.width() <= width));
        }
    }
}

#[test]
fn fish_conditionals_keep_commands_distinct_from_flow_keywords() {
    let source = "if ip -brief address\nprintf '%s\\n' '=== IP addresses ==='\nend\n";
    assert_eq!(
        emphasized_commands(&code(source, Some("fish"), 100)),
        "ip -brief address"
    );
}

#[test]
fn incomplete_shell_syntax_is_preserved_not_hidden_by_highlighting() {
    for source in [
        "if ip -brief address; then\n",
        "printf 'unterminated\n",
        "echo $(ip",
    ] {
        let rendered = code(source, Some("bash"), 100);
        assert_eq!(
            rendered
                .iter()
                .skip(1)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
            source
        );
    }
}
