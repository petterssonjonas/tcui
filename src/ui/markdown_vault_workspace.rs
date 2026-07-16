mod interaction;
mod rebind;
mod reconcile;
mod registry;
mod state;

pub(crate) use interaction::MarkdownVaultInputAction;
pub(crate) use registry::MarkdownVaultWorkspaceRegistry;
pub(crate) use state::{
    MarkdownVaultContentPane, MarkdownVaultFocus, MarkdownVaultWorkspaceState,
    MarkdownVaultWorkspaceStatus,
};

#[cfg(test)]
pub(crate) use rebind::{MarkdownVaultWorkspaceRebindCancel, MarkdownVaultWorkspaceRebindRequest};
#[cfg(test)]
pub(crate) use registry::{
    MarkdownVaultWorkspaceError, MarkdownVaultWorkspacePtyPoll, MarkdownVaultWorkspaceRemoval,
};
