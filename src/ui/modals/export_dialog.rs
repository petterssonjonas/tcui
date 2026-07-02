use std::path::PathBuf;

use ratatui::{layout::Rect, prelude::*, widgets::*, Frame};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportDialogFocus {
    Path,
    Markdown,
    Json,
    Export,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportTarget {
    Conversation(i64),
    #[cfg(feature = "memory")]
    Memory(std::path::PathBuf),
}

#[derive(Debug, Clone, Default)]
pub struct ExportDialogHitAreas {
    pub markdown: Option<Rect>,
    pub json: Option<Rect>,
    pub export: Option<Rect>,
    pub cancel: Option<Rect>,
}

#[derive(Debug, Clone)]
pub struct ExportDialog {
    pub target: ExportTarget,
    pub item_name: String,
    pub directory_input: String,
    pub format: crate::export::OutputFormat,
    pub focus: ExportDialogFocus,
    pub hit_areas: ExportDialogHitAreas,
}

impl ExportDialog {
    pub fn new(target: ExportTarget, item_name: String, base_dir: PathBuf) -> Self {
        Self {
            target,
            item_name,
            directory_input: base_dir.display().to_string(),
            format: crate::export::OutputFormat::Markdown,
            focus: ExportDialogFocus::Path,
            hit_areas: ExportDialogHitAreas::default(),
        }
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect) {
        let popup_area = Self::popup_area(area);
        let block = Block::default()
            .title(" Export Cleartext ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .style(Style::default().bg(Color::Black));
        let inner = block.inner(popup_area);

        f.render_widget(Clear, popup_area);
        f.render_widget(block, popup_area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(1),
            ])
            .margin(1)
            .split(inner);

        f.render_widget(
            Paragraph::new(format!("Item: {}", self.item_name))
                .style(Style::default().fg(Color::White)),
            chunks[0],
        );

        let path_border = if self.focus == ExportDialogFocus::Path {
            Color::Yellow
        } else {
            Color::DarkGray
        };
        f.render_widget(
            Paragraph::new(self.directory_input.as_str())
                .block(
                    Block::default()
                        .title(" Directory ")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(path_border)),
                )
                .style(Style::default().fg(Color::White)),
            chunks[1],
        );

        let format_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(chunks[2]);
        let markdown_area = centered_rect_in(70, 100, format_chunks[0]);
        let json_area = centered_rect_in(70, 100, format_chunks[1]);
        render_format_button(
            f,
            markdown_area,
            "Markdown",
            self.format == crate::export::OutputFormat::Markdown,
            self.focus == ExportDialogFocus::Markdown,
        );
        render_format_button(
            f,
            json_area,
            "JSON",
            self.format == crate::export::OutputFormat::Json,
            self.focus == ExportDialogFocus::Json,
        );

        let buttons = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(chunks[3]);
        let export_area = centered_rect_in(50, 100, buttons[0]);
        let cancel_area = centered_rect_in(50, 100, buttons[1]);
        render_action_button(
            f,
            export_area,
            "Export",
            self.focus == ExportDialogFocus::Export,
            Color::Green,
        );
        render_action_button(
            f,
            cancel_area,
            "Cancel",
            self.focus == ExportDialogFocus::Cancel,
            Color::DarkGray,
        );

        f.render_widget(
            Paragraph::new("Exports are unencrypted. Tab/Arrows move focus, Enter confirms.")
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::DarkGray)),
            chunks[4],
        );

        self.hit_areas = ExportDialogHitAreas {
            markdown: Some(markdown_area),
            json: Some(json_area),
            export: Some(export_area),
            cancel: Some(cancel_area),
        };
    }

    pub fn popup_area(area: Rect) -> Rect {
        centered_rect_in(64, 18, area)
    }

    pub fn cycle_focus(&mut self, forward: bool) {
        let order = [
            ExportDialogFocus::Path,
            ExportDialogFocus::Markdown,
            ExportDialogFocus::Json,
            ExportDialogFocus::Export,
            ExportDialogFocus::Cancel,
        ];
        let current = order
            .iter()
            .position(|focus| *focus == self.focus)
            .unwrap_or(0);
        let next = if forward {
            (current + 1) % order.len()
        } else {
            (current + order.len() - 1) % order.len()
        };
        self.focus = order[next];
    }
}

fn render_format_button(f: &mut Frame, area: Rect, label: &str, selected: bool, focused: bool) {
    let border = if focused {
        Color::Yellow
    } else {
        Color::DarkGray
    };
    let style = if selected {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::White)
    };
    f.render_widget(
        Paragraph::new(format!(" {label} "))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(border)),
            )
            .style(style),
        area,
    );
}

fn render_action_button(f: &mut Frame, area: Rect, label: &str, focused: bool, color: Color) {
    let border = if focused { Color::Yellow } else { color };
    f.render_widget(
        Paragraph::new(format!(" {label} "))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(border)),
            )
            .style(Style::default().fg(color)),
        area,
    );
}

fn centered_rect_in(percent_x: u16, height: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(height.min(area.height.saturating_sub(2)).max(4)),
            Constraint::Min(0),
        ])
        .split(area);

    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1]);

    horizontal[1]
}
