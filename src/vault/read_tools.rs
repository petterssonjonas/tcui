use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use super::{
    VaultEntry, VaultFs, VaultFsError, VaultLimitReached, VaultRelativePath, VaultSearchLimits,
    VaultSearchMatch, VaultWalkLimits,
};
use crate::config::vault_profiles::{VaultProfile, VaultProfileId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VaultReadOnlyLimits {
    pub(crate) walk: VaultWalkLimits,
    pub(crate) search: VaultSearchLimits,
    pub(crate) max_read_bytes: u64,
}

impl Default for VaultReadOnlyLimits {
    fn default() -> Self {
        let search = VaultSearchLimits::default();
        Self {
            walk: VaultWalkLimits::default(),
            max_read_bytes: search.max_file_bytes,
            search,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum VaultReadOnlyOperation {
    List {
        profile_id: VaultProfileId,
        path: Option<VaultRelativePath>,
    },
    Read {
        profile_id: VaultProfileId,
        path: VaultRelativePath,
    },
    Search {
        profile_id: VaultProfileId,
        query: String,
    },
}

impl VaultReadOnlyOperation {
    fn profile_id(&self) -> &VaultProfileId {
        match self {
            Self::List { profile_id, .. }
            | Self::Read { profile_id, .. }
            | Self::Search { profile_id, .. } => profile_id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VaultReadOnlyStatus {
    Complete,
    Partial,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum VaultReadOnlyData {
    List(Vec<VaultEntry>),
    Read {
        path: PathBuf,
        content: String,
    },
    Search {
        matches: Vec<VaultSearchMatch>,
        files_scanned: usize,
        total_bytes_read: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VaultReadOnlyResult {
    pub(crate) display_name: String,
    pub(crate) status: VaultReadOnlyStatus,
    pub(crate) limits: Vec<VaultLimitReached>,
    pub(crate) data: VaultReadOnlyData,
}

impl VaultReadOnlyResult {
    pub(crate) fn render(&self) -> String {
        let status = match self.status {
            VaultReadOnlyStatus::Complete => "complete",
            VaultReadOnlyStatus::Partial => "partial",
        };
        let limits = self
            .limits
            .iter()
            .map(|limit| {
                json!({
                    "kind": format!("{:?}", limit.kind),
                    "path": limit.path.as_ref().map(|path| path.display().to_string()),
                })
            })
            .collect::<Vec<_>>();
        let data = match &self.data {
            VaultReadOnlyData::List(entries) => json!({
                "operation": "list",
                "entries": entries.iter().map(|entry| json!({
                    "path": entry.path.display().to_string(),
                    "kind": format!("{:?}", entry.kind),
                })).collect::<Vec<_>>(),
            }),
            VaultReadOnlyData::Read { path, content } => json!({
                "operation": "read",
                "path": path.display().to_string(),
                "content": content,
            }),
            VaultReadOnlyData::Search {
                matches,
                files_scanned,
                total_bytes_read,
            } => json!({
                "operation": "search",
                "matches": matches.iter().map(|entry| json!({
                    "path": entry.path.display().to_string(),
                    "byte_offset": entry.byte_offset,
                })).collect::<Vec<_>>(),
                "files_scanned": files_scanned,
                "total_bytes_read": total_bytes_read,
            }),
        };
        json!({
            "vault": self.display_name,
            "status": status,
            "limits": limits,
            "data": data,
        })
        .to_string()
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum VaultReadOnlyError {
    #[error("unknown Markdown Vault read-only tool '{0}'")]
    UnknownTool(String),
    #[error("missing or invalid '{name}' argument for Markdown Vault tool")]
    InvalidArgument { name: &'static str },
    #[error(transparent)]
    InvalidPath(#[from] super::VaultPathError),
    #[error("Markdown Vault profile is no longer configured")]
    UnknownProfile,
    #[error("Markdown Vault profile '{display_name}' is unavailable")]
    Unavailable { display_name: String },
    #[error("failed to open Markdown Vault profile '{display_name}'")]
    OpenProfile { display_name: String },
    #[error(transparent)]
    FileSystem(#[from] VaultFsError),
}

pub(crate) fn parse_read_only_operation(
    profile_id: VaultProfileId,
    tool: &str,
    arguments: &Map<String, Value>,
) -> Result<VaultReadOnlyOperation, VaultReadOnlyError> {
    match tool {
        "list_notes" => Ok(VaultReadOnlyOperation::List {
            profile_id,
            path: optional_path(arguments)?,
        }),
        "read_note" => Ok(VaultReadOnlyOperation::Read {
            profile_id,
            path: required_path(arguments)?,
        }),
        "search_notes" => Ok(VaultReadOnlyOperation::Search {
            profile_id,
            query: required_string(arguments, "query")?.to_owned(),
        }),
        unknown => Err(VaultReadOnlyError::UnknownTool(unknown.to_owned())),
    }
}

pub(crate) fn execute_read_only(
    profiles: &HashMap<VaultProfileId, VaultProfile>,
    operation: &VaultReadOnlyOperation,
    limits: VaultReadOnlyLimits,
) -> Result<VaultReadOnlyResult, VaultReadOnlyError> {
    let profile = profiles
        .get(operation.profile_id())
        .ok_or(VaultReadOnlyError::UnknownProfile)?;
    if !profile.available {
        return Err(VaultReadOnlyError::Unavailable {
            display_name: profile.display_name.clone(),
        });
    }
    let vault =
        VaultFs::open_profile(&profile.root).map_err(|_| VaultReadOnlyError::OpenProfile {
            display_name: profile.display_name.clone(),
        })?;
    let (data, reached) = match operation {
        VaultReadOnlyOperation::List { path, .. } => {
            let result = vault.list(path.as_ref(), limits.walk)?;
            (VaultReadOnlyData::List(result.entries), result.limits)
        }
        VaultReadOnlyOperation::Read { path, .. } => {
            let result = vault.read_text_limited(path, limits.max_read_bytes)?;
            (
                VaultReadOnlyData::Read {
                    path: path.as_path().to_path_buf(),
                    content: result.text,
                },
                result.limits,
            )
        }
        VaultReadOnlyOperation::Search { query, .. } => {
            let result = vault.search(query, limits.search)?;
            (
                VaultReadOnlyData::Search {
                    matches: result.matches,
                    files_scanned: result.files_scanned,
                    total_bytes_read: result.total_bytes_read,
                },
                result.limits,
            )
        }
    };
    let status = if reached.is_empty() {
        VaultReadOnlyStatus::Complete
    } else {
        VaultReadOnlyStatus::Partial
    };
    Ok(VaultReadOnlyResult {
        display_name: profile.display_name.clone(),
        status,
        limits: reached,
        data,
    })
}

fn required_string<'a>(
    arguments: &'a Map<String, Value>,
    name: &'static str,
) -> Result<&'a str, VaultReadOnlyError> {
    arguments
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or(VaultReadOnlyError::InvalidArgument { name })
}

fn required_path(arguments: &Map<String, Value>) -> Result<VaultRelativePath, VaultReadOnlyError> {
    Ok(VaultRelativePath::parse(Path::new(required_string(
        arguments, "path",
    )?))?)
}

fn optional_path(
    arguments: &Map<String, Value>,
) -> Result<Option<VaultRelativePath>, VaultReadOnlyError> {
    arguments
        .get("path")
        .and_then(Value::as_str)
        .filter(|path| !path.trim().is_empty())
        .map(|path| VaultRelativePath::parse(Path::new(path)).map_err(Into::into))
        .transpose()
}
