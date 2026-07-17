use crossterm::event::KeyEvent;
use ratatui::layout::Position;

use crate::app::{Action, TuiApp};
use crate::ui::app_tabs::AppView;
use crate::ui::markdown_vault_workspace::MarkdownVaultInputAction;

impl TuiApp {
    pub(crate) fn handle_markdown_vault_key(&mut self, key: KeyEvent) -> Option<Action> {
        let tab_id = self.ui.app_tabs.active_id();
        let action = self
            .ui
            .markdown_vault_workspaces
            .get_mut(tab_id)
            .map_or(MarkdownVaultInputAction::None, |state| {
                state.handle_key(key)
            });
        self.apply_markdown_vault_input(tab_id, action)
    }

    pub(crate) fn handle_markdown_vault_mouse_click(
        &mut self,
        position: Position,
    ) -> Option<Action> {
        let tab_id = self.ui.app_tabs.active_id();
        let action = self
            .ui
            .markdown_vault_workspaces
            .get_mut(tab_id)
            .and_then(|state| {
                state
                    .hit_areas
                    .action_at(position)
                    .map(|action| state.handle_mouse(action))
            })
            .unwrap_or(MarkdownVaultInputAction::None);
        self.apply_markdown_vault_input(tab_id, action)
    }

    pub(crate) fn handle_markdown_vault_mouse_scroll(
        &mut self,
        position: Position,
        down: bool,
    ) -> bool {
        let tab_id = self.ui.app_tabs.active_id();
        self.ui
            .markdown_vault_workspaces
            .get_mut(tab_id)
            .is_some_and(|state| state.scroll_at(position, down))
    }

    pub(crate) fn markdown_vault_is_active(&self) -> bool {
        matches!(
            self.ui.app_tabs.active_view(),
            AppView::MarkdownVault { .. }
        )
    }

    fn apply_markdown_vault_input(
        &mut self,
        tab_id: crate::ui::app_tabs::AppTabId,
        action: MarkdownVaultInputAction,
    ) -> Option<Action> {
        match action {
            MarkdownVaultInputAction::None => None,
            MarkdownVaultInputAction::AddVault => Some(Action::OpenVaultProfileManager),
            MarkdownVaultInputAction::Notice(message) => {
                self.ui.show_toast(message);
                None
            }
            MarkdownVaultInputAction::Rebind(profile_id) => {
                let catalog = self.vault_catalog();
                if let Err(error) = self.ui.markdown_vault_workspaces.request_rebind(
                    &mut self.ui.app_tabs,
                    &catalog,
                    tab_id,
                    &profile_id,
                ) {
                    self.ui.show_toast(error.to_string());
                }
                None
            }
            MarkdownVaultInputAction::ConfirmRebind => {
                let catalog = self.vault_catalog();
                if let Err(error) = self.ui.markdown_vault_workspaces.confirm_rebind(
                    &mut self.ui.app_tabs,
                    &catalog,
                    tab_id,
                ) {
                    self.ui.show_toast(error.to_string());
                }
                None
            }
            MarkdownVaultInputAction::CancelRebind => {
                let _ = self.ui.markdown_vault_workspaces.cancel_rebind(tab_id);
                None
            }
            MarkdownVaultInputAction::Move {
                source,
                destination,
            } => {
                let pending = (|| -> color_eyre::Result<_> {
                    let profile_id = self
                        .ui
                        .markdown_vault_workspaces
                        .get(tab_id)
                        .map(|workspace| workspace.profile_id().clone())
                        .ok_or_else(|| {
                            color_eyre::eyre::eyre!("Markdown Vault workspace is unavailable")
                        })?;
                    let catalog = self.vault_catalog();
                    let source_path = crate::vault::VaultRelativePath::parse(&source)?;
                    let destination_path = crate::vault::VaultRelativePath::parse(&destination)?;
                    let pair = crate::vault::move_plan::MovePair::new(
                        crate::vault::move_plan::VaultFileRef::new(profile_id.clone(), source_path),
                        crate::vault::move_plan::VaultFileRef::new(profile_id, destination_path),
                        false,
                    );
                    let kind = crate::vault::mutation_plan::VaultMutationKind::Move(
                        crate::vault::move_plan::MovePlan::build(vec![pair], &catalog, |_| false)?,
                    );
                    let instruction = format!(
                        "Move workspace Vault file {} to {}",
                        source.display(),
                        destination.display()
                    );
                    let submission = crate::app::submission::InputSubmission::new(
                        &instruction,
                        crate::app::submission::PendingDocuments::default(),
                    );
                    crate::app::submission::PendingVaultMutation::new_workspace(
                        crate::app::submission::PreparedUserTurn::prepare(&submission)?,
                        kind,
                        &catalog,
                    )
                    .map_err(Into::into)
                })();
                match pending {
                    Ok(pending) => self.ui.pending_vault_mutation = Some(pending),
                    Err(error) => self.ui.show_toast(error.to_string()),
                }
                None
            }
        }
    }
}
