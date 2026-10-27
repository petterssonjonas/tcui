mod cli;
mod credential;
mod native;
mod status;

pub(crate) use cli::{login_with_cli, logout_external_cli, CodexCliError};
#[cfg(test)]
pub(crate) use credential::{external_metadata_is_safe, CodexCredentialError};
#[cfg_attr(
    not(test),
    expect(
        unused_imports,
        reason = "Todo 8 consumes typed Codex credential resolution through the stable auth facade."
    )
)]
pub(crate) use credential::{
    read_external_credential, resolve_credential, CodexCredential, CodexCredentialSource,
};
#[cfg(test)]
pub(crate) use native::CodexRevocationFailure;
pub(crate) use native::{CodexNativeAdapter, CodexNativeError, CodexNativeLogout};
pub(crate) use status::{codex_status, CodexStatus};
