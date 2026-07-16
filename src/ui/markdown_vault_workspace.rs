mod rebind;
mod registry;
mod state;

pub(crate) use registry::MarkdownVaultWorkspaceRegistry;

#[cfg(test)]
pub(crate) use rebind::{MarkdownVaultWorkspaceRebindCancel, MarkdownVaultWorkspaceRebindRequest};
#[cfg(test)]
pub(crate) use registry::{
    MarkdownVaultWorkspaceError, MarkdownVaultWorkspacePtyPoll, MarkdownVaultWorkspaceRemoval,
};
