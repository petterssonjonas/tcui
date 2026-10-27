use super::{render_markdown, tests::opts, RenderedMarkdown};
use crate::config::app_config::MarkdownMode;
use ratatui::style::Color;

fn chart(source: &str, width: usize) -> RenderedMarkdown {
    render_markdown(&format!("```mermaid\n{source}\n```"), opts(width))
}

fn text(rendered: &RenderedMarkdown) -> String {
    rendered
        .lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

fn assert_graphics(rendered: &RenderedMarkdown, width: usize) {
    let output = text(rendered);
    assert!(!output.contains("Mermaid chart:"), "{output}");
    assert!(!output.contains("Mermaid source:"), "{output}");
    assert!(
        rendered.lines.iter().all(|line| line.width() <= width),
        "{output}"
    );
    assert!(
        rendered
            .lines
            .iter()
            .flat_map(|line| &line.spans)
            .any(|span| {
                span.content.chars().any(|ch| {
                    ch == '█' || ch == '▀' || ch == '▄' || ('\u{2801}'..='\u{28ff}').contains(&ch)
                })
            }),
        "missing graphical cells: {output}"
    );
}

#[test]
fn categorical_bar_charts_render_axes_signed_values_labels_and_colored_cells() {
    let source = "xychart-beta\ntitle \"Quarterly balance\"\nx-axis \"Quarter\" [Q1, Q2, Q3]\ny-axis \"USD\" -20 --> 40\nbar [30, -10, 0]";
    for width in [20, 40, 80, 160] {
        let rendered = chart(source, width);
        assert_graphics(&rendered, width);
        let output = text(&rendered);
        for label in [
            "Quarterly",
            "Y: USD",
            "X: Quarter",
            "Q1",
            "Q2",
            "Q3",
            "-20",
            "40",
        ] {
            assert!(output.contains(label), "missing {label}: {output}");
        }
        assert!(!output.contains("bar ["), "{output}");
        assert!(output.contains('│') && output.contains('─'), "{output}");
    }
}

#[test]
fn numeric_x_line_chart_draws_connected_samples_and_scale() {
    let rendered = chart("xychart-beta\ntitle \"Latency\"\nx-axis \"Time\" 0 --> 10\ny-axis \"ms\" -5 --> 15\nline [0, 10, -3, 12]", 80);
    assert_graphics(&rendered, 80);
    let output = text(&rendered);
    for label in ["Latency", "X: Time", "Y: ms", "-5", "15", "(line)"] {
        assert!(output.contains(label), "missing {label}: {output}");
    }
    let dots = output
        .chars()
        .filter(|ch| ('\u{2801}'..='\u{28ff}').contains(ch))
        .count();
    assert!(
        dots > 12,
        "line must connect samples, not scatter four dots: {output}"
    );
}

#[test]
fn mixed_and_multiple_series_keep_titles_and_distinct_legend_colors() {
    let rendered = chart("xychart-beta\ntitle \"Traffic\"\nx-axis [Mon, Tue, Wed]\ny-axis 0 --> 100\nbar \"Requests\" [30, 80, 50]\nbar \"Errors\" [4, 8, 2]\nline \"Target\" [60, 60, 60]", 80);
    assert_graphics(&rendered, 80);
    let output = text(&rendered);
    for label in ["Requests (bar)", "Errors (bar)", "Target (line)"] {
        assert!(output.contains(label), "{output}");
    }
    let colors = rendered
        .lines
        .iter()
        .flat_map(|line| &line.spans)
        .filter(|span| span.content.starts_with('■'))
        .map(|span| span.style.fg)
        .collect::<Vec<_>>();
    assert_eq!(colors.len(), 3);
    assert_ne!(colors[0], colors[1]);
    assert_ne!(colors[1], colors[2]);
}

#[test]
fn pie_chart_renders_round_colored_sectors_with_values_and_percent_legend() {
    let rendered = chart("pie showData\ntitle Market share\n\"Alpha\" : 60\n\"Beta\" : 30\n\"Other\" : 10\n\"Zero\" : 0", 80);
    assert_graphics(&rendered, 80);
    let output = text(&rendered);
    for label in [
        "Market share",
        "Alpha",
        "60.0%",
        "Beta",
        "30.0%",
        "Other",
        "10.0%",
        "Zero",
        "0.0%",
        "Total: 100",
    ] {
        assert!(output.contains(label), "missing {label}: {output}");
    }
    let colored_cells = rendered
        .lines
        .iter()
        .flat_map(|line| &line.spans)
        .filter(|span| span.style.bg.is_some_and(|color| color != Color::Reset))
        .map(|span| span.content.chars().count())
        .sum::<usize>();
    assert!(
        colored_cells > 50,
        "pie must contain filled sectors, not only legend: {output}"
    );
    assert!(!output.contains("\"Alpha\" :"), "{output}");
}

#[test]
fn pie_header_title_and_single_full_circle_are_supported() {
    let rendered = chart("pie showData title Only slice\n\"Everything\" : 1", 30);
    assert_graphics(&rendered, 30);
    let output = text(&rendered);
    assert!(
        output.contains("Only slice") && output.contains("100.0%"),
        "{output}"
    );
}

#[test]
fn zero_and_constant_series_still_have_finite_visible_scales() {
    for series in ["bar [0, 0, 0]", "bar [-5, -5, -5]", "line [7, 7, 7]"] {
        let rendered = chart(&format!("xychart-beta\nx-axis [A, B, C]\n{series}"), 60);
        assert_graphics(&rendered, 60);
        let output = text(&rendered);
        assert!(
            !output.contains("NaN") && !output.contains("inf"),
            "{output}"
        );
    }
}

#[test]
fn categorical_bars_keep_full_edge_widths_and_zero_values_have_no_height() {
    let rendered = chart(
        "xychart-beta\nx-axis [A, B, C]\ny-axis 0 --> 20\nbar [10, 10, 10]",
        80,
    );
    let output = text(&rendered);
    let widths = output
        .lines()
        .find(|line| line.contains('█'))
        .expect("positive bars are visible")
        .split(|ch| ch != '█')
        .filter(|run| !run.is_empty())
        .map(|run| run.chars().count())
        .collect::<Vec<_>>();
    assert_eq!(widths.len(), 3, "{output}");
    assert!(
        widths.iter().max().unwrap() - widths.iter().min().unwrap() <= 2,
        "{output}"
    );
    let zero = text(&chart("xychart-beta\nx-axis [A, B, C]\nbar [0, 0, 0]", 80));
    assert!(!zero.contains('█'), "{zero}");
}

#[test]
fn dense_and_unicode_categories_have_complete_readable_keys() {
    let rendered = chart("xychart-beta\ntitle \"世界 sales\"\nx-axis [\"North America\", \"日本\", \"Europe\", \"Other\"]\nbar [10, 20, 30, 40]", 40);
    assert_graphics(&rendered, 40);
    let output = text(&rendered);
    // Entries that fit individually stay together when the key wraps.
    for label in [
        "世界",
        "1: North America",
        "2: 日本",
        "3: Europe",
        "4: Other",
    ] {
        assert!(output.contains(label), "missing {label}: {output}");
    }
}

#[test]
fn invalid_pie_totals_and_negative_values_never_render_misleading_sectors() {
    for source in [
        "pie\n\"Zero\" : 0",
        "pie\n\"Negative\" : -1\n\"Positive\" : 2",
        "pie",
    ] {
        let rendered = chart(source, 60);
        let output = text(&rendered);
        assert!(output.contains("Mermaid chart:"), "{output}");
        assert!(!output.contains("Total:"), "{output}");
    }
}

#[test]
fn incomplete_malformed_and_narrow_charts_keep_bounded_readable_source() {
    for (source, width) in [
        ("xychart-beta\nbar [1,", 60),
        ("pie\n\"One\" : NaN", 60),
        ("pie\n\"One\" : 1", 10),
        ("xychart-beta horizontal\nbar [1, 2]", 60),
    ] {
        let rendered = chart(source, width);
        assert!(
            text(&rendered).replace('\n', "").contains("Mermaid chart:"),
            "{}",
            text(&rendered)
        );
        assert!(rendered.lines.iter().all(|line| line.width() <= width));
    }
    let streamed = render_markdown("```mermaid\nxychart-beta\nbar [1, 2]", opts(60));
    assert!(text(&streamed).contains("waiting for closing fence"));
}

#[test]
fn chart_source_points_series_and_slice_limits_are_enforced() {
    let too_many_points = format!("xychart-beta\nline [{}]", vec!["1"; 257].join(","));
    let too_many_series = format!("xychart-beta\n{}", "line [1, 2]\n".repeat(9));
    let too_many_slices = format!(
        "pie\n{}",
        (0..17)
            .map(|index| format!("\"Slice {index}\" : 1\n"))
            .collect::<String>()
    );
    let too_much_source = format!("pie\ntitle {}\n\"One\" : 1", "x".repeat(9000));
    let too_many_categories = format!(
        "xychart-beta\nx-axis [{}]\nbar [{}]",
        (0..65)
            .map(|index| format!("C{index}"))
            .collect::<Vec<_>>()
            .join(","),
        vec!["1"; 65].join(",")
    );
    for source in [
        too_many_points,
        too_many_series,
        too_many_slices,
        too_much_source,
        too_many_categories,
    ] {
        let rendered = chart(&source, 60);
        let output = text(&rendered);
        assert!(output.contains("Mermaid chart:"), "{output}");
        assert!(rendered.lines.iter().all(|line| line.width() <= 60));
        assert!(output.len() < 30_000);
    }
}

#[test]
fn chart_labels_and_errors_never_emit_terminal_control_sequences() {
    for source in [
        "pie\n\"Hello\u{1b}[31m\" : 1",
        "xychart-beta\ntitle \"bad\u{7}\"\nline [1, 2]",
        "pie\n\"#27;[31m\" : 1",
    ] {
        let output = text(&chart(source, 60));
        assert!(
            !output.chars().any(|ch| ch.is_control() && ch != '\n'),
            "{output:?}"
        );
    }
}

#[test]
fn chart_rendering_keeps_original_source_for_copy_and_markdown_off() {
    let source = "```mermaid\nxychart-beta\nx-axis [A, B]\nbar [2, -1]\n```".to_string();
    let original = source.clone();
    let rendered = render_markdown(&source, opts(60));
    assert_graphics(&rendered, 60);
    assert_eq!(source, original);
    let mut options = opts(60);
    options.mode = MarkdownMode::Off;
    assert_eq!(text(&render_markdown(&source, options)), original);
}

#[test]
fn chart_rendering_works_in_textual_mode_and_non_kitty_terminal() {
    let mut options = opts(60);
    options.mode = MarkdownMode::Textual;
    options.kitty_enhanced_text = false;
    options.image_protocol_enabled = false;
    let rendered = render_markdown("~~~mermaid\nxychart-beta\nline [1, 2, 1]\n~~~", options);
    assert_graphics(&rendered, 60);
    assert!(rendered.images.is_empty());
}
