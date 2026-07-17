use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
    Frame,
};

use crate::app::submission::{PendingVaultMutation, VaultMutationConfirmationState};

#[derive(Debug, Clone, Copy)]
pub(crate) struct VaultMutationPreviewAreas {
    pub(crate) cancel: Rect,
    pub(crate) confirm: Option<Rect>,
}

pub(crate) struct VaultMutationPreviewModal<'a> {
    pending: &'a PendingVaultMutation,
}

impl<'a> VaultMutationPreviewModal<'a> {
    pub(crate) const fn new(pending: &'a PendingVaultMutation) -> Self {
        Self { pending }
    }

    pub(crate) fn render(&self, frame: &mut Frame, area: Rect) -> VaultMutationPreviewAreas {
        let theme = crate::theme::active_theme();
        let popup = centered(area);
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Block::default().style(Style::default().fg(theme.foreground).bg(theme.panel)),
            popup,
        );
        if popup.height == 0 || popup.width == 0 {
            return VaultMutationPreviewAreas {
                cancel: popup,
                confirm: None,
            };
        }

        frame.render_widget(
            Paragraph::new(" Vault mutation preview ").style(
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Rect::new(popup.x, popup.y, popup.width, 1),
        );
        let footer_height = u16::from(popup.height >= 2);
        let content = Rect::new(
            popup.x,
            popup.y.saturating_add(1),
            popup.width,
            popup.height.saturating_sub(1 + footer_height),
        );
        let mut lines = self
            .pending
            .preview
            .rows
            .iter()
            .skip(self.pending.scroll)
            .cloned()
            .map(Line::from)
            .collect::<Vec<_>>();
        if let VaultMutationConfirmationState::Reviewing { error: Some(error) } =
            &self.pending.state
        {
            lines.push(Line::styled(error.clone(), Style::default().fg(Color::Red)));
        }
        frame.render_widget(Paragraph::new(lines), content);

        let footer = Rect::new(
            popup.x,
            popup.y.saturating_add(popup.height.saturating_sub(1)),
            popup.width,
            footer_height,
        );
        let confirm_style = if self.pending.can_confirm() {
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.muted)
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("[Cancel]", Style::default().fg(Color::Red)),
                Span::raw("  "),
                Span::styled("[Confirm]", confirm_style),
            ])),
            footer,
        );
        let cancel = Rect::new(footer.x, footer.y, footer.width.min(8), footer.height);
        let confirm = self.pending.can_confirm().then(|| {
            Rect::new(
                footer.x.saturating_add(10),
                footer.y,
                footer.width.saturating_sub(10).min(9),
                footer.height,
            )
        });
        VaultMutationPreviewAreas { cancel, confirm }
    }
}

fn centered(area: Rect) -> Rect {
    let width = area.width.saturating_sub(4).min(100);
    let height = area.height.saturating_sub(2).min(28);
    Rect::new(
        area.x.saturating_add(area.width.saturating_sub(width) / 2),
        area.y
            .saturating_add(area.height.saturating_sub(height) / 2),
        width,
        height,
    )
}
