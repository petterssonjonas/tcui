mod cli;
mod credential;
mod native;
mod status;

pub(crate) use cli::{CodexCliError, login_with_cli, logout_external_cli};
#[cfg_attr(
    not(test),
    expect(
        unused_imports,
        reason = "Todo 8 consumes typed Codex credential resolution through the stable auth facade."
    )
)]
pub(crate) use credential::{
    CodexCredential, CodexCredentialSource, read_external_credential, resolve_credential,
};
#[cfg(test)]
pub(crate) use credential::{CodexCredentialError, external_metadata_is_safe};
#[cfg(test)]
pub(crate) use native::CodexRevocationFailure;
pub(crate) use native::{CodexNativeAdapter, CodexNativeError, CodexNativeLogout};
pub(crate) use status::{CodexStatus, codex_status};
