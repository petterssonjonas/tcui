use std::collections::HashSet;
use std::path::Path;

use color_eyre::eyre::{eyre, Result};
use serde::Deserialize;
use serde_json::Value;

use crate::vault::move_plan::{MovePair, MovePlan, VaultFileRef};
use crate::vault::mutation_plan::{VaultMutationKind, VaultRestorePlan, VaultWritePlan};
use crate::vault::VaultRelativePath;

use super::ToolPlan;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VaultMutationAction {
    Write,
    Restore,
    Move,
    Copy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VaultMutationTool {
    Write,
    Restore,
    Move,
    Copy,
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
        if !words.contains("vault") {
            return Ok(None);
        }
        let actions = [
            ("write", VaultMutationAction::Write),
            ("restore", VaultMutationAction::Restore),
            ("move", VaultMutationAction::Move),
            ("copy", VaultMutationAction::Copy),
        ]
        .into_iter()
        .filter_map(|(verb, action)| {
            let present = if action == VaultMutationAction::Copy {
                user_request.split_whitespace().any(|token| {
                    token
                        .trim_matches(|c: char| {
                            !c.is_alphanumeric() && !matches!(c, '.' | '/' | '\\')
                        })
                        .eq_ignore_ascii_case("copy")
                })
            } else {
                words.contains(verb)
            };
            present.then_some(action)
        })
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

    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "authorization inspection is test-visible")
    )]
    pub(crate) const fn action(self) -> VaultMutationAction {
        self.action
    }

    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "authorization inspection is test-visible")
    )]
    pub(crate) const fn overwrite_allowed(self) -> bool {
        self.overwrite_allowed
    }

    pub(crate) const fn permits(self, tool: VaultMutationTool) -> bool {
        matches!(
            (self.action, tool),
            (VaultMutationAction::Write, VaultMutationTool::Write)
                | (VaultMutationAction::Restore, VaultMutationTool::Restore)
                | (VaultMutationAction::Move, VaultMutationTool::Move)
                | (VaultMutationAction::Copy, VaultMutationTool::Copy)
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
        VaultMutationAction::Move => vec![transfer_tool("move_notes", "moves")],
        VaultMutationAction::Copy => vec![transfer_tool("copy_notes", "copies")],
    }
}

fn transfer_tool(name: &str, collection: &str) -> Value {
    serde_json::json!({
        "name": name,
        "input_schema": {
            "type": "object",
            "properties": {
                (collection): {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "source_path": {"type": "string"},
                            "destination_path": {"type": "string"}
                        },
                        "required": ["source_path", "destination_path"],
                        "additionalProperties": false
                    }
                }
            },
            "required": [collection]
        }
    })
}

pub(crate) fn build_kind(
    authorization: VaultMutationAuthorization,
    plan: &ToolPlan,
) -> Result<VaultMutationKind> {
    let tool = match plan.tool.as_str() {
        "write_note" => VaultMutationTool::Write,
        "restore_note" => VaultMutationTool::Restore,
        "move_notes" => VaultMutationTool::Move,
        "copy_notes" => VaultMutationTool::Copy,
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
                &crate::storage::paths::TcuiDataPaths::discover().root,
                path,
                replacement,
                authorization.overwrite_allowed,
            )?))
        }
        VaultMutationTool::Restore => {
            let path = required_path(plan, "path")?;
            let backup = required_string(plan, "backup")?.parse()?;
            Ok(VaultMutationKind::Restore(VaultRestorePlan::build(
                &crate::storage::paths::TcuiDataPaths::discover().root,
                path,
                backup,
            )?))
        }
        VaultMutationTool::Move | VaultMutationTool::Copy => {
            let collection = if tool == VaultMutationTool::Copy {
                "copies"
            } else {
                "moves"
            };
            let moves = plan
                .arguments
                .get(collection)
                .cloned()
                .ok_or_else(|| eyre!("missing {collection} argument"))?;
            let moves: Vec<MoveInput> = serde_json::from_value(moves)?;
            let pairs = moves
                .into_iter()
                .map(|input| input.into_pair(authorization.overwrite_allowed))
                .collect::<Result<Vec<_>>>()?;
            let transfer = if tool == VaultMutationTool::Copy {
                MovePlan::build_copy(
                    pairs,
                    &crate::storage::paths::TcuiDataPaths::discover().root,
                    |_| false,
                )?
            } else {
                MovePlan::build(
                    pairs,
                    &crate::storage::paths::TcuiDataPaths::discover().root,
                    |_| false,
                )?
            };
            Ok(VaultMutationKind::Move(transfer))
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MoveInput {
    source_path: String,
    destination_path: String,
}

impl MoveInput {
    fn into_pair(self, overwrite: bool) -> Result<MovePair> {
        Ok(MovePair::new(
            VaultFileRef::new(VaultRelativePath::parse(Path::new(&self.source_path))?),
            VaultFileRef::new(VaultRelativePath::parse(Path::new(&self.destination_path))?),
            overwrite,
        ))
    }
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
#[error("Vault mutation request must name exactly one of write, restore, move, or copy")]
pub(crate) struct VaultMutationAuthorizationError;
