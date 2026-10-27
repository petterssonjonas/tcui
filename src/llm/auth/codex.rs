mod cli;
mod credential;
mod native;
mod status;

pub(crate) use cli::{logout_external_cli, CodexCliError};
#[cfg(test)]
pub(crate) use credential::{
    external_metadata_is_safe, read_external_credential, CodexCredentialError,
};
pub(crate) use credential::{resolve_connection, CodexCredential, CodexCredentialSource};
#[cfg(test)]
pub(crate) use native::CodexRevocationFailure;
pub(crate) use native::{CodexNativeAdapter, CodexNativeError, CodexNativeLogout};
pub(crate) use status::{codex_status, CodexStatus};
