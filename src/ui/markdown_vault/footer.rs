use ratatui::{prelude::*, widgets::Paragraph, Frame};

use crate::config::vault_profiles::VaultProfileId;

use super::hit_areas::{MarkdownVaultHitAreas, MarkdownVaultMouseAction};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MarkdownVaultProfileOption {
    pub(crate) id: VaultProfileId,
    pub(crate) label: String,
    pub(crate) available: bool,
}

pub(crate) struct MarkdownVaultFooterProps<'a> {
    pub(crate) current_label: &'a str,
    pub(crate) expanded: bool,
    pub(crate) selected_profile: usize,
    pub(crate) add_selected: bool,
    pub(crate) profiles: &'a [MarkdownVaultProfileOption],
}

pub(crate) fn render_footer(
    frame: &mut Frame,
    area: Rect,
    props: MarkdownVaultFooterProps<'_>,
    hit_areas: &mut MarkdownVaultHitAreas,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let theme = crate::theme::active_theme();
    let profile_label = format!(
        "[Vault: {} {}]",
        props.current_label,
        if props.expanded { "^" } else { "v" }
    );
    let profile_width = u16::try_from(profile_label.chars().count())
        .unwrap_or(u16::MAX)
        .min(area.width);
    let add_label = "+ Add vault";
    let add_width = u16::try_from(add_label.len()).unwrap_or(u16::MAX);
    let profile_area = Rect::new(area.x, area.y, profile_width, 1);
    let add_area = Rect::new(
        area.right().saturating_sub(add_width),
        area.y,
        add_width.min(area.width),
        1,
    );
    let profile_style = Style::default()
        .fg(if props.add_selected {
            theme.foreground
        } else {
            theme.accent
        })
        .bg(theme.panel);
    let add_style = Style::default()
        .fg(if props.add_selected {
            theme.accent
        } else {
            theme.foreground
        })
        .bg(theme.panel);
    frame.render_widget(
        Paragraph::new("").style(Style::default().bg(theme.panel)),
        area,
    );
    frame.render_widget(
        Paragraph::new(profile_label).style(profile_style),
        profile_area,
    );
    frame.render_widget(Paragraph::new(add_label).style(add_style), add_area);
    hit_areas.push(profile_area, MarkdownVaultMouseAction::ProfileDropdown);
    hit_areas.push(add_area, MarkdownVaultMouseAction::AddVault);

    if !props.expanded || props.profiles.is_empty() {
        return;
    }
    let visible = usize::from(area.y).min(props.profiles.len());
    let start = props.profiles.len().saturating_sub(visible);
    for (row, (index, profile)) in props.profiles.iter().enumerate().skip(start).enumerate() {
        let y = area
            .y
            .saturating_sub(u16::try_from(visible - row).unwrap_or(u16::MAX));
        let row_area = Rect::new(area.x, y, profile_width.max(1), 1);
        let marker = if index == props.selected_profile {
            ">"
        } else {
            " "
        };
        let availability = if profile.available {
            ""
        } else {
            " (unavailable)"
        };
        let style = Style::default()
            .fg(if profile.available {
                theme.foreground
            } else {
                theme.muted
            })
            .bg(if index == props.selected_profile {
                theme.card_bg
            } else {
                theme.panel
            });
        frame.render_widget(
            Paragraph::new(format!("{marker} {}{availability}", profile.label)).style(style),
            row_area,
        );
        hit_areas.push(row_area, MarkdownVaultMouseAction::ProfileOption(index));
    }
}
