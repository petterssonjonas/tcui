use ratatui::{
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Clear, Paragraph, Wrap},
    Frame,
};

use super::manager::{VaultProfileManagerState, VaultProfilePhase};

impl VaultProfileManagerState {
    pub(crate) fn render(&self, frame: &mut Frame, area: Rect) {
        let popup = popup_area(area);
        if popup.width == 0 || popup.height == 0 {
            return;
        }
        frame.render_widget(Clear, popup);
        let theme = crate::theme::active_theme();
        frame.render_widget(
            Block::default().style(Style::default().fg(theme.foreground).bg(theme.panel)),
            popup,
        );
        match self.phase {
            VaultProfilePhase::Directory | VaultProfilePhase::ChangeFolder => {
                if let Some(picker) = self.picker.as_ref() {
                    picker.render(frame, popup.inner(ratatui::layout::Margin::new(1, 1)));
                }
            }
            VaultProfilePhase::Name | VaultProfilePhase::Rename => {
                self.render_input(frame, popup);
            }
            VaultProfilePhase::Profiles | VaultProfilePhase::LauncherChoice => {
                self.render_profiles(frame, popup);
            }
            VaultProfilePhase::Actions => self.render_actions(frame, popup),
        }
        if let Some(error) = self.error.as_ref() {
            frame.render_widget(
                Paragraph::new(error.to_string())
                    .style(Style::default().fg(theme.error).bg(theme.panel))
                    .wrap(Wrap { trim: true }),
                Rect::new(
                    popup.x.saturating_add(1),
                    popup.bottom().saturating_sub(2),
                    popup.width.saturating_sub(2),
                    1,
                ),
            );
        }
    }

    fn render_input(&self, frame: &mut Frame, popup: Rect) {
        let theme = crate::theme::active_theme();
        let title = match self.phase {
            VaultProfilePhase::Name => " Name vault ",
            VaultProfilePhase::Rename => " Rename vault ",
            VaultProfilePhase::Profiles
            | VaultProfilePhase::Actions
            | VaultProfilePhase::Directory
            | VaultProfilePhase::ChangeFolder
            | VaultProfilePhase::LauncherChoice => " Vault profile ",
        };
        let lines = vec![
            Line::styled(
                title,
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Line::from(""),
            Line::from(format!("> {}", self.input)),
            Line::from(""),
            Line::styled("Enter save · Esc back", Style::default().fg(theme.muted)),
        ];
        frame.render_widget(
            Paragraph::new(lines)
                .alignment(Alignment::Left)
                .wrap(Wrap { trim: false }),
            popup.inner(ratatui::layout::Margin::new(2, 2)),
        );
    }

    fn render_profiles(&self, frame: &mut Frame, popup: Rect) {
        let theme = crate::theme::active_theme();
        let mut lines = vec![Line::styled(
            " Markdown Vault profiles ",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        )];
        for (index, id) in self.profile_ids().into_iter().enumerate() {
            let Some(profile) = self.catalog.get(id) else {
                continue;
            };
            let marker = if self.catalog.default_id.as_ref() == Some(id) {
                "default"
            } else {
                ""
            };
            let style = if index == self.selected {
                Style::default().fg(theme.background).bg(theme.accent)
            } else {
                Style::default().fg(theme.foreground)
            };
            lines.push(Line::styled(
                format!(
                    " {}  {} {marker}",
                    if index == self.selected { "›" } else { " " },
                    profile.display_name
                ),
                style,
            ));
        }
        if self.catalog.profiles.is_empty() {
            lines.push(Line::styled(
                " No profiles configured",
                Style::default().fg(theme.muted),
            ));
        }
        lines.push(Line::from(""));
        let hint = if self.phase == VaultProfilePhase::LauncherChoice {
            "Enter open · Esc cancel"
        } else {
            "Enter manage · a add · Esc close"
        };
        lines.push(Line::styled(hint, Style::default().fg(theme.muted)));
        frame.render_widget(
            Paragraph::new(lines).wrap(Wrap { trim: true }),
            popup.inner(ratatui::layout::Margin::new(1, 1)),
        );
    }

    fn render_actions(&self, frame: &mut Frame, popup: Rect) {
        let theme = crate::theme::active_theme();
        let actions = ["Rename", "Change folder", "Set default", "Remove"];
        let lines = actions.into_iter().enumerate().map(|(index, action)| {
            let style = if index == self.action_selected {
                Style::default().fg(theme.background).bg(theme.accent)
            } else {
                Style::default().fg(theme.foreground)
            };
            Line::styled(
                format!(
                    " {} {action}",
                    if index == self.action_selected {
                        "›"
                    } else {
                        " "
                    }
                ),
                style,
            )
        });
        frame.render_widget(
            Paragraph::new(lines.collect::<Vec<_>>()).wrap(Wrap { trim: true }),
            popup.inner(ratatui::layout::Margin::new(2, 2)),
        );
    }
}

pub(super) fn popup_area(area: Rect) -> Rect {
    let width = area.width.min(72);
    let height = area.height.min(24);
    Rect::new(
        area.x.saturating_add(area.width.saturating_sub(width) / 2),
        area.y
            .saturating_add(area.height.saturating_sub(height) / 2),
        width,
        height,
    )
}
