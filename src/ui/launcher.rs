use ratatui::{prelude::*, widgets::*, Frame};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LauncherAction {
    Vault,
    #[cfg(feature = "memory")]
    Memories,
    Settings,
    Terminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LauncherFeature {
    label: &'static str,
    action: LauncherAction,
}

const FEATURES: &[LauncherFeature] = &[
    LauncherFeature {
        label: "Vault",
        action: LauncherAction::Vault,
    },
    #[cfg(feature = "memory")]
    LauncherFeature {
        label: "Memories",
        action: LauncherAction::Memories,
    },
    LauncherFeature {
        label: "Settings",
        action: LauncherAction::Settings,
    },
    LauncherFeature {
        label: "Terminal",
        action: LauncherAction::Terminal,
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LauncherHitArea {
    pub action: LauncherAction,
    pub area: Rect,
}

pub fn render(f: &mut Frame, area: Rect) -> Vec<LauncherHitArea> {
    let theme = crate::theme::active_theme();
    f.render_widget(
        Block::default().style(Style::default().bg(theme.background)),
        area,
    );

    let cards = cards(area);
    for (feature, card_area) in &cards {
        render_card(f, *card_area, feature.label);
    }
    cards
        .into_iter()
        .map(|(feature, area)| LauncherHitArea {
            action: feature.action,
            area,
        })
        .collect()
}

pub fn hit_areas(area: Rect) -> Vec<LauncherHitArea> {
    cards(area)
        .into_iter()
        .map(|(feature, area)| LauncherHitArea {
            action: feature.action,
            area,
        })
        .collect()
}

fn cards(area: Rect) -> Vec<(LauncherFeature, Rect)> {
    let rows = u16::try_from(FEATURES.len().div_ceil(2)).unwrap_or(u16::MAX);
    let row_height: u16 = if area.height >= 7 { 3 } else { 2 };
    let total_height = row_height
        .saturating_mul(rows)
        .saturating_add(rows.saturating_sub(1));
    let width = area.width.saturating_sub(4).min(60);
    let grid = Rect::new(
        area.x.saturating_add(area.width.saturating_sub(width) / 2),
        area.y
            .saturating_add(area.height.saturating_sub(total_height) / 2),
        width,
        total_height.min(area.height),
    );
    let column_gap: u16 = 2;
    let column_width = grid.width.saturating_sub(column_gap) / 2;
    let mut cards = Vec::with_capacity(FEATURES.len());
    if column_width == 0 || grid.height < total_height {
        return cards;
    }

    for row in 0..rows {
        let y = grid
            .y
            .saturating_add(row.saturating_mul(row_height.saturating_add(1)));
        for column in 0..2_u16 {
            let index = usize::from(row.saturating_mul(2).saturating_add(column));
            let x = grid
                .x
                .saturating_add(column.saturating_mul(column_width.saturating_add(column_gap)));
            if let Some(feature) = FEATURES.get(index) {
                cards.push((*feature, Rect::new(x, y, column_width, row_height)));
            }
        }
    }

    cards
}

fn render_card(f: &mut Frame, area: Rect, label: &'static str) {
    let theme = crate::theme::active_theme();
    f.render_widget(
        Block::default().style(Style::default().bg(theme.panel)),
        area,
    );
    let label_area = Rect::new(
        area.x,
        area.y + area.height.saturating_sub(1) / 2,
        area.width,
        1.min(area.height),
    );
    f.render_widget(
        Paragraph::new(label)
            .style(Style::default().fg(theme.foreground).bg(theme.panel))
            .alignment(Alignment::Center),
        label_area,
    );
}

#[cfg(test)]
mod tests {
    use ratatui::{backend::TestBackend, layout::Rect, Terminal};

    #[test]
    fn app_tab_id_launcher_renders_all_feature_cards_at_minimum_size() {
        let mut terminal = Terminal::new(TestBackend::new(64, 15)).expect("terminal");

        terminal
            .draw(|frame| {
                let _ = super::render(frame, Rect::new(0, 0, 64, 15));
            })
            .expect("render launcher");

        let screen = terminal.backend().to_string();
        for feature in super::FEATURES {
            assert!(
                screen.contains(feature.label),
                "missing launcher card: {}",
                feature.label
            );
        }
    }

    #[test]
    fn app_tab_id_launcher_cards_expose_stable_actions() {
        let actions = super::hit_areas(Rect::new(0, 0, 64, 15))
            .into_iter()
            .map(|hit| hit.action)
            .collect::<Vec<_>>();

        assert_eq!(
            actions,
            super::FEATURES
                .iter()
                .map(|feature| feature.action)
                .collect::<Vec<_>>()
        );
        assert_eq!(actions[0], super::LauncherAction::Vault);
    }

    #[test]
    fn app_tab_id_launcher_tiny_width_hit_areas_are_bounded() {
        for width in 0..=8 {
            let area = Rect::new(40, 3, width, 15);
            assert!(super::hit_areas(area).iter().all(|hit| {
                hit.area.x >= area.x
                    && hit.area.right() <= area.right()
                    && hit.area.y >= area.y
                    && hit.area.bottom() <= area.bottom()
            }));
        }
    }
}
