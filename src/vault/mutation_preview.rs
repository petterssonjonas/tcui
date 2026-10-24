use crate::config::vault_profiles::VaultCatalog;

use super::move_plan::{MoveCollision, MoveFileKind, MovePlan};
use super::mutation_plan::{
    profile, FileIdentity, VaultMutationKind, VaultMutationPlanError, VaultMutationPreview,
};

impl VaultMutationKind {
    pub(crate) fn preview(
        &self,
        catalog: &VaultCatalog,
    ) -> Result<VaultMutationPreview, VaultMutationPlanError> {
        match self {
            Self::Write(plan) => {
                let profile = profile(catalog, &plan.profile_id)?;
                let confirm_enabled =
                    !matches!(plan.original, FileIdentity::Present(_)) || plan.overwrite_allowed;
                let mut rows = vec![format!(
                    "Write {}:{}",
                    profile.display_name,
                    plan.path.display()
                )];
                rows.extend(plan.diff.lines().map(str::to_owned));
                if !confirm_enabled {
                    rows.push("unapproved collision: explicit overwrite required".to_owned());
                }
                Ok(VaultMutationPreview {
                    rows,
                    confirm_enabled,
                })
            }
            Self::Restore(plan) => {
                let profile = profile(catalog, &plan.profile_id)?;
                let mut rows = vec![format!(
                    "Restore {}:{}",
                    profile.display_name,
                    plan.path.display()
                )];
                rows.extend(plan.diff.lines().map(str::to_owned));
                Ok(VaultMutationPreview {
                    rows,
                    confirm_enabled: true,
                })
            }
            Self::Move(plan) => move_preview(plan, catalog),
        }
    }
}

fn move_preview(
    plan: &MovePlan,
    catalog: &VaultCatalog,
) -> Result<VaultMutationPreview, VaultMutationPlanError> {
    let mut rows = Vec::with_capacity(plan.file_count().saturating_mul(2).saturating_add(1));
    if plan.copy_only() {
        rows.push("Copy files; source documents and their chat links remain unchanged".to_string());
    }
    let mut confirm_enabled = true;
    for ((pair, precondition), collision) in plan
        .pairs()
        .iter()
        .zip(plan.preconditions())
        .zip(plan.collisions())
    {
        let source = profile(catalog, pair.source().profile_id())?;
        let destination = profile(catalog, pair.destination().profile_id())?;
        let collision = match collision {
            MoveCollision::Create => "create",
            MoveCollision::ExplicitOverwrite => "explicit overwrite",
            MoveCollision::UnapprovedCollision => {
                confirm_enabled = false;
                "unapproved collision"
            }
            MoveCollision::InvalidType(MoveFileKind::Directory) => {
                confirm_enabled = false;
                "invalid directory"
            }
            MoveCollision::InvalidType(MoveFileKind::Symlink) => {
                confirm_enabled = false;
                "invalid symlink"
            }
            MoveCollision::InvalidType(MoveFileKind::Unsupported) => {
                confirm_enabled = false;
                "invalid type"
            }
        };
        rows.push(format!(
            "{}:{} -> {}:{}",
            source.display_name,
            pair.source().path().display(),
            destination.display_name,
            pair.destination().path().display()
        ));
        rows.push(format!(
            "  {} bytes | blake3:{} | {collision}",
            precondition.size,
            precondition.blake3.to_hex()
        ));
    }
    rows.push(format!(
        "Total: {} {}, {} bytes",
        plan.file_count(),
        if plan.file_count() == 1 {
            "file"
        } else {
            "files"
        },
        plan.total_size()
    ));
    Ok(VaultMutationPreview {
        rows,
        confirm_enabled,
    })
}
