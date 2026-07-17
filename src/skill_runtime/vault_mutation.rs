use std::collections::HashSet;
use std::path::Path;

use color_eyre::eyre::{eyre, Result};
use serde::Deserialize;
use serde_json::Value;

use crate::config::vault_profiles::{normalize_name, VaultCatalog, VaultProfileId};
use crate::config::AppConfig;
use crate::vault::move_plan::{MovePair, MovePlan, VaultFileRef};
use crate::vault::mutation_plan::{VaultMutationKind, VaultRestorePlan, VaultWritePlan};
use crate::vault::VaultRelativePath;

use super::ToolPlan;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VaultMutationAction {
    Write,
    Restore,
    Move,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VaultMutationTool {
    Write,
    Restore,
    Move,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VaultMutationAuthorization {
    action: VaultMutationAction,
    overwrite_allowed: bool,
}

impl VaultMutationAuthorization {
    pub(crate) fn capture(
        user_request: &str,
    ) -> Result<Option<Self>, VaultMutationAuthorizationError> {
        let words = words(user_request);
        if !words.contains("vault") && !words.contains("obsidian") {
            return Ok(None);
        }
        let actions = [
            ("write", VaultMutationAction::Write),
            ("restore", VaultMutationAction::Restore),
            ("move", VaultMutationAction::Move),
        ]
        .into_iter()
        .filter_map(|(verb, action)| words.contains(verb).then_some(action))
        .collect::<Vec<_>>();
        match actions.as_slice() {
            [] => Ok(None),
            [action] => Ok(Some(Self {
                action: *action,
                overwrite_allowed: words.contains("overwrite"),
            })),
            [_, _, ..] => Err(VaultMutationAuthorizationError),
        }
    }

    #[expect(dead_code, reason = "T12C will wire mutation execution")]
    pub(crate) const fn action(self) -> VaultMutationAction {
        self.action
    }

    #[expect(dead_code, reason = "T12C will wire mutation execution")]
    pub(crate) const fn overwrite_allowed(self) -> bool {
        self.overwrite_allowed
    }

    pub(crate) const fn permits(self, tool: VaultMutationTool) -> bool {
        matches!(
            (self.action, tool),
            (VaultMutationAction::Write, VaultMutationTool::Write)
                | (VaultMutationAction::Restore, VaultMutationTool::Restore)
                | (VaultMutationAction::Move, VaultMutationTool::Move)
        )
    }
}

pub(crate) fn tool_catalog(authorization: VaultMutationAuthorization) -> Vec<Value> {
    match authorization.action {
        VaultMutationAction::Write => vec![serde_json::json!({
            "name": "write_note",
            "input_schema": {
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "content": {"type": "string"}
                },
                "required": ["path", "content"]
            }
        })],
        VaultMutationAction::Restore => vec![serde_json::json!({
            "name": "restore_note",
            "input_schema": {
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "backup": {"type": "string"}
                },
                "required": ["path", "backup"]
            }
        })],
        VaultMutationAction::Move => vec![serde_json::json!({
            "name": "move_notes",
            "input_schema": {
                "type": "object",
                "properties": {
                    "moves": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "source_profile": {"type": "string"},
                                "source_path": {"type": "string"},
                                "destination_profile": {"type": "string"},
                                "destination_path": {"type": "string"}
                            },
                            "required": ["source_profile", "source_path", "destination_profile", "destination_path"]
                        }
                    }
                },
                "required": ["moves"]
            }
        })],
    }
}

pub(crate) fn build_kind(
    config: &AppConfig,
    selected_profile_id: VaultProfileId,
    authorization: VaultMutationAuthorization,
    plan: &ToolPlan,
) -> Result<VaultMutationKind> {
    let catalog = VaultCatalog {
        profiles: config.vaults.clone(),
        default_id: config.default_vault_id.clone(),
    };
    let tool = match plan.tool.as_str() {
        "write_note" => VaultMutationTool::Write,
        "restore_note" => VaultMutationTool::Restore,
        "move_notes" => VaultMutationTool::Move,
        _ => {
            return Err(eyre!(
                "planner selected an unauthorized Vault mutation tool"
            ))
        }
    };
    if !authorization.permits(tool) {
        return Err(eyre!(
            "model-selected Vault mutation cannot elevate the user's authorization"
        ));
    }
    match tool {
        VaultMutationTool::Write => {
            let path = required_path(plan, "path")?;
            let replacement = required_string(plan, "content")?.to_owned();
            Ok(VaultMutationKind::Write(VaultWritePlan::build(
                &catalog,
                selected_profile_id,
                path,
                replacement,
                authorization.overwrite_allowed,
            )?))
        }
        VaultMutationTool::Restore => {
            let path = required_path(plan, "path")?;
            let backup = required_string(plan, "backup")?.parse()?;
            Ok(VaultMutationKind::Restore(VaultRestorePlan::build(
                &catalog,
                &crate::storage::paths::TcuiDataPaths::discover().root,
                selected_profile_id,
                path,
                backup,
            )?))
        }
        VaultMutationTool::Move => {
            let moves = plan
                .arguments
                .get("moves")
                .cloned()
                .ok_or_else(|| eyre!("missing moves argument"))?;
            let moves: Vec<MoveInput> = serde_json::from_value(moves)?;
            let pairs = moves
                .into_iter()
                .map(|input| input.into_pair(&catalog, authorization.overwrite_allowed))
                .collect::<Result<Vec<_>>>()?;
            Ok(VaultMutationKind::Move(MovePlan::build(
                pairs,
                &catalog,
                |_| false,
            )?))
        }
    }
}

#[derive(Deserialize)]
struct MoveInput {
    source_profile: String,
    source_path: String,
    destination_profile: String,
    destination_path: String,
}

impl MoveInput {
    fn into_pair(self, catalog: &VaultCatalog, overwrite: bool) -> Result<MovePair> {
        let source = resolve_profile(catalog, &self.source_profile)?;
        let destination = resolve_profile(catalog, &self.destination_profile)?;
        Ok(MovePair::new(
            VaultFileRef::new(
                source,
                VaultRelativePath::parse(Path::new(&self.source_path))?,
            ),
            VaultFileRef::new(
                destination,
                VaultRelativePath::parse(Path::new(&self.destination_path))?,
            ),
            overwrite,
        ))
    }
}

fn resolve_profile(catalog: &VaultCatalog, name: &str) -> Result<VaultProfileId> {
    catalog
        .find_by_name_key(&normalize_name(name))
        .map(|profile| profile.id.clone())
        .ok_or_else(|| eyre!("unknown Vault profile '{name}'"))
}

fn required_string<'a>(plan: &'a ToolPlan, key: &str) -> Result<&'a str> {
    plan.arguments
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| eyre!("missing {key} argument"))
}

fn required_path(plan: &ToolPlan, key: &str) -> Result<VaultRelativePath> {
    Ok(VaultRelativePath::parse(Path::new(required_string(
        plan, key,
    )?))?)
}

fn words(value: &str) -> HashSet<String> {
    value
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("Vault mutation request must name exactly one of write, restore, or move")]
pub(crate) struct VaultMutationAuthorizationError;
