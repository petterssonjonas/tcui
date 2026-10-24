mod interaction;
mod move_input;
mod mutation;
mod reconcile;
mod registry;
mod state;

pub(crate) use interaction::VaultInputAction;
pub(crate) use move_input::VaultMoveInput;
pub(crate) use registry::VaultWorkspaceRegistry;
pub(crate) use state::{VaultContentPane, VaultFocus, VaultWorkspaceState};

#[cfg(test)]
pub(crate) use registry::{VaultWorkspaceError, VaultWorkspacePtyPoll, VaultWorkspaceRemoval};
