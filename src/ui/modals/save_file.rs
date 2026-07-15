use crate::ui::artifact_sidebar::ArtifactEntry;
use ratatui::{layout::Rect, prelude::*, widgets::*, Frame};
use std::path::PathBuf;

#[derive(Debug, Clone, Default)]
pub struct SaveFileDialogHitAreas {
    pub save: Option<Rect>,
    pub cancel: Option<Rect>,
}

#[derive(Debug, Clone)]
pub struct SaveFileDialog {
    pub artifact: ArtifactEntry,
    pub path_input: String,
    pub save_label: String,
    pub hit_areas: SaveFileDialogHitAreas,
}

impl SaveFileDialog {
    pub fn new(artifact: &ArtifactEntry, base_dir: PathBuf, save_label: impl Into<String>) -> Self {
        let path_input = base_dir.join(&artifact.name).display().to_string();

        Self {
            artifact: artifact.clone(),
            path_input,
            save_label: save_label.into(),
            hit_areas: SaveFileDialogHitAreas::default(),
        }
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect) {
        let theme = crate::theme::active_theme();
        let popup_area = Self::popup_area(area);
        let block = Block::default().style(Style::default().bg(theme.panel));
        let inner = Rect::new(
            popup_area.x,
            popup_area.y + 1,
            popup_area.width,
            popup_area.height.saturating_sub(1),
        );

        f.render_widget(Clear, popup_area);
        f.render_widget(block, popup_area);
        f.render_widget(
            Paragraph::new(Line::from(" Save Artifact ")).style(
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Rect::new(popup_area.x, popup_area.y, popup_area.width, 1),
        );

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(1),
            ])
            .margin(1)
            .split(inner);

        f.render_widget(
            Paragraph::new(format!("File: {}", self.artifact.name))
                .style(Style::default().fg(Color::White)),
            chunks[0],
        );

        let path_area = chunks[1];
        f.render_widget(
            Paragraph::new(Line::from(" Path ")).style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Rect::new(path_area.x, path_area.y, path_area.width, 1),
        );
        f.render_widget(
            Paragraph::new(self.path_input.as_str())
                .style(Style::default().fg(Color::White).bg(theme.panel)),
            Rect::new(
                path_area.x,
                path_area.y + 1,
                path_area.width,
                path_area.height.saturating_sub(1),
            ),
        );

        let buttons = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(chunks[2]);
        let save_area = centered_rect_in(50, 100, buttons[0]);
        let cancel_area = centered_rect_in(50, 100, buttons[1]);

        f.render_widget(
            Paragraph::new(format!(" {} ", self.save_label))
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::Green).bg(theme.panel)),
            save_area,
        );
        f.render_widget(
            Paragraph::new(" Cancel ")
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::White).bg(theme.panel)),
            cancel_area,
        );
        f.render_widget(
            Paragraph::new("Enter saves, Esc cancels")
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::DarkGray)),
            chunks[3],
        );

        self.hit_areas = SaveFileDialogHitAreas {
            save: Some(save_area),
            cancel: Some(cancel_area),
        };
    }

    pub fn popup_area(area: Rect) -> Rect {
        centered_rect_in(60, 24, area)
    }
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
