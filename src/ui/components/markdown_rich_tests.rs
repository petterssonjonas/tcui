use super::{render_markdown, tests::opts, RenderedMarkdown};
use crate::config::app_config::MarkdownMode;
use unicode_width::UnicodeWidthStr;

fn text(rendered: &RenderedMarkdown) -> String {
    rendered
        .lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

fn diagram(source: &str, width: usize) -> RenderedMarkdown {
    render_markdown(&format!("```mermaid\n{source}\n```"), opts(width))
}

#[test]
fn table_boxes_include_vertical_borders_corners_and_alignment() {
    let rendered = render_markdown("| Left | Right |\n| :--- | ---: |\n| a | b |", opts(30));
    assert_eq!(
        text(&rendered),
        "┌──────┬───────┐\n│ Left │ Right │\n├──────┼───────┤\n│ a    │     b │\n└──────┴───────┘"
    );
}

#[test]
fn table_cells_keep_html_and_tcui_tags_literal_without_autolinking_them() {
    let rendered = render_markdown(
        "| Literal | Link |\n| --- | --- |\n\
         | A<tag></tag>B | [docs](https://example.com/docs) |\n\
         | <tcui:remember>keep</tcui:remember> | <!-- comment --> |\n\
         | <tcui:chat-title>title</tcui:chat-title> | `<tcui:remember>` |",
        opts(120),
    );
    let output = text(&rendered);
    for expected in [
        "A<tag></tag>B",
        "<tcui:remember>keep</tcui:remember>",
        "<tcui:chat-title>title</tcui:chat-title>",
        "<!-- comment -->",
    ] {
        assert!(output.contains(expected), "missing {expected}:\n{output}");
    }
    assert_eq!(
        rendered
            .link_targets
            .iter()
            .map(|target| target.target.as_str())
            .collect::<Vec<_>>(),
        ["https://example.com/docs"]
    );
}

#[test]
fn table_cells_keep_escaped_and_inline_code_pipes_and_markdown_styles() {
    let rendered = render_markdown(
        "| Value | Code |\n| --- | --- |\n| a\\|b | `x|y` |\n| **bold** | ``a`b`` |",
        opts(40),
    );
    let output = text(&rendered);
    for expected in ["a|b", "x|y", "bold", "a`b"] {
        assert!(output.contains(expected), "missing {expected}:\n{output}");
    }
    assert!(!output.contains("**") && !output.contains("`x"));
}

#[test]
fn table_links_keep_display_cell_hit_targets_after_unicode_padding_and_wrapping() {
    let rendered = render_markdown(
        "| 名 | Link |\n| :---: | ---: |\n| 界 | [文档🙂abcdef](https://example.com/docs) |",
        opts(20),
    );
    assert!(!rendered.link_targets.is_empty());
    let mut recovered = String::new();
    for target in &rendered.link_targets {
        assert_eq!(target.target, "https://example.com/docs");
        let line = rendered.lines[target.line].to_string();
        let mut column = 0;
        for character in line.chars() {
            let width = unicode_width::UnicodeWidthChar::width(character).unwrap_or(0);
            if column >= target.column && column < target.column + target.width {
                recovered.push(character);
            }
            column += width;
        }
        assert!(target.column + target.width <= rendered.lines[target.line].width());
    }
    assert_eq!(recovered, "文档🙂abcdef");
    assert!(rendered.lines.iter().all(|line| line.width() <= 20));
}

#[test]
fn narrow_tables_use_boxed_column_panels_without_losing_content() {
    let rendered = render_markdown(
        "| First | Second | Third |\n| --- | --- | --- |\n| alpha | beta | gamma |",
        opts(8),
    );
    let output = text(&rendered);
    assert_eq!(output.matches('┌').count(), 3);
    let compact = output
        .chars()
        .filter(|ch| ch.is_alphabetic())
        .collect::<String>();
    for expected in ["First", "Second", "Third", "alpha", "beta", "gamma"] {
        assert!(compact.contains(expected), "missing {expected}: {output}");
    }
    assert!(rendered.lines.iter().all(|line| line.width() <= 8));
}

#[test]
fn table_after_paragraph_and_crlf_keeps_block_boundaries() {
    let rendered = render_markdown(
        "Before\r\n| Header |\r\n| --- |\r\n| cell |\r\n\r\nAfter",
        opts(40),
    );
    let output = text(&rendered);
    assert!(output.starts_with("Before\n\n┌"), "{output}");
    assert!(output.ends_with("┘\n\nAfter"), "{output}");
}

#[test]
fn unicode_graphemes_wrap_without_splitting_emoji_or_combining_marks() {
    let rendered = render_markdown("| Glyph |\n| --- |\n| 👩‍💻é👩‍💻é |", opts(6));
    let output = text(&rendered);
    assert_eq!(output.matches("👩‍💻").count(), 2);
    assert_eq!(output.matches("é").count(), 2);
    assert!(rendered.lines.iter().all(|line| line.width() <= 6));
}

#[test]
fn flowchart_directions_change_node_positions_and_keep_graphical_connections() {
    for direction in ["TD", "TB", "BT", "LR", "RL"] {
        let rendered = diagram(&format!("flowchart {direction}\nA[Alpha] --> B[Beta]"), 80);
        let output = text(&rendered);
        assert!(!output.contains("Mermaid source"), "{direction}:\n{output}");
        let alpha = rendered
            .lines
            .iter()
            .position(|line| line.to_string().contains("Alpha"))
            .expect("Alpha visible");
        let beta = rendered
            .lines
            .iter()
            .position(|line| line.to_string().contains("Beta"))
            .expect("Beta visible");
        match direction {
            "TD" | "TB" => assert!(alpha < beta, "{output}"),
            "BT" => assert!(beta < alpha, "{output}"),
            _ => {
                assert_eq!(alpha, beta, "{output}");
                let row = rendered.lines[alpha].to_string();
                let a = row.find("Alpha").unwrap();
                let b = row.find("Beta").unwrap();
                assert_eq!(a < b, direction == "LR", "{output}");
            }
        }
        assert!(
            output.chars().any(|ch| "▼▲►◄▶◀↓↑→←><^v".contains(ch)),
            "missing arrow: {output}"
        );
        assert!(rendered.lines.iter().all(|line| line.width() <= 80));
    }
}

#[test]
fn graph_alias_renders_branch_labels_and_node_shapes_not_raw_mermaid() {
    let rendered = diagram(
        "graph TD\nA[Start] --> B{Ready?}\nB -->|yes| C(Done)\nB -.->|no| D[Wait]",
        100,
    );
    let output = text(&rendered);
    assert!(!output.contains("Mermaid source"), "{output}");
    for label in ["Start", "Ready?", "Done", "Wait", "yes", "no"] {
        assert!(output.contains(label), "missing {label}: {output}");
    }
    assert!(output.contains('│') || output.contains('─'), "{output}");
    assert!(!output.contains("-->") && !output.contains("-.->"));
}

#[test]
fn mermaid_unicode_labels_and_multiline_nodes_fit_available_width() {
    let rendered = diagram("flowchart TD\nA[世界🙂<br>Next] --> B[終了]", 30);
    let output = text(&rendered);
    assert!(!output.contains("Mermaid source"), "{output}");
    for label in ["世界🙂", "Next", "終了"] {
        assert!(output.contains(label), "{output}");
    }
    assert!(rendered.lines.iter().all(|line| line.width() <= 30));
}

#[test]
fn unsupported_malformed_streamed_and_too_wide_diagrams_remain_readable_source() {
    for (source, width) in [
        ("sequenceDiagram\nAlice->>Bob: Hello", 80),
        ("flowchart TD\nA[unfinished", 80),
        ("flowchart LR\nA[Alpha] --> B[Beta]", 8),
        (
            "flowchart TD\nA[Start]\nclick A \"https://example.com\"",
            80,
        ),
    ] {
        let rendered = diagram(source, width);
        let output = text(&rendered);
        let stripped = output.replace(['\n', ' '], "");
        assert!(
            stripped.contains(&source.replace(['\n', ' '], "")),
            "source missing: {output}"
        );
        assert!(rendered.lines.iter().all(|line| line.width() <= width));
    }
    let output = text(&render_markdown(
        "```mermaid\nflowchart TD\nA --> B",
        opts(80),
    ));
    assert!(output.contains("A --> B"));
}

#[test]
fn mermaid_control_sequences_never_reach_terminal_output() {
    let rendered = diagram("flowchart TD\nA[hello\u{1b}] --> B[world]", 60);
    assert!(!text(&rendered)
        .chars()
        .any(|ch| ch.is_control() && ch != '\n'));
}

#[test]
fn oversized_mermaid_uses_bounded_source_fallback() {
    let source = format!("flowchart TD\nA[{}] --> B", "x".repeat(9000));
    let rendered = diagram(&source, 40);
    assert!(text(&rendered).contains("rendering limits"));
    assert!(rendered.lines.iter().all(|line| line.width() <= 40));
}

#[test]
fn markdown_off_keeps_table_and_mermaid_source_unchanged() {
    let source = "| a | b |\n| --- | --- |\n\n```mermaid\nflowchart TD\nA --> B\n```";
    let mut options = opts(80);
    options.mode = MarkdownMode::Off;
    assert_eq!(text(&render_markdown(source, options)), source);
}

#[test]
fn rich_markdown_plain_terminal_smoke_fixture() {
    let source = "# Terminal rendering\n\n| Feature | Result |\n| :--- | ---: |\n| escaped a\\|b | **boxed** |\n| `x|y` | [Docs](https://example.com/docs) |\n| 世界🙂 | wrapped content stays readable |\n\n```mermaid\nflowchart TD\nA[Request] --> B{Ready?}\nB -->|yes| C[Reply]\nB -->|no| D[Wait]\n```";
    let rendered = render_markdown(source, opts(80));
    let output = text(&rendered);
    assert!(output.contains('┌') && output.contains('└'));
    assert!(output.contains("Request") && output.contains("Ready?"));
    assert!(!output.contains("Mermaid source"), "{output}");
    assert!(rendered
        .lines
        .iter()
        .all(|line| UnicodeWidthStr::width(line.to_string().as_str()) <= 80));
    assert!(rendered
        .link_targets
        .iter()
        .any(|target| target.target == "https://example.com/docs"));
}

#[test]
fn standard_tilde_and_long_backtick_mermaid_fences_render() {
    for fence in ["~~~", "````"] {
        let source = format!("{fence}mermaid\nflowchart LR; A[First] --> B[Second]\n{fence}");
        let output = text(&render_markdown(&source, opts(80)));
        assert!(!output.contains("Mermaid source"), "{output}");
        assert!(
            output.contains("First") && output.contains("Second"),
            "{output}"
        );
    }
}

#[test]
fn graph_subgraphs_and_chained_edges_keep_group_and_node_labels() {
    let rendered = diagram(
        "flowchart LR\nsubgraph Worker\nA[Read] --> B[Write]\nend\nB --> C[Done]",
        100,
    );
    let output = text(&rendered);
    assert!(!output.contains("Mermaid source"), "{output}");
    for label in ["Worker", "Read", "Write", "Done"] {
        assert!(output.contains(label), "{output}");
    }
}

#[test]
fn unsafe_table_control_characters_never_reach_terminal() {
    let rendered = render_markdown("| Header |\n| --- |\n| a\u{1b}[31mb |", opts(30));
    assert!(!text(&rendered)
        .chars()
        .any(|ch| ch.is_control() && ch != '\n'));
}
