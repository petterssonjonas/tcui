mod adapters;
mod cli;
mod contracts;
#[cfg(debug_assertions)]
mod debug;
mod errors;
mod flow;
mod input;
mod runner;
#[cfg(test)]
mod tests;

pub(crate) use cli::AuthCommandArgs;
pub(crate) use contracts::{
    AuthCommandError, AuthCommandRequest, AuthCommandResult, AuthLoginRequest, AuthLogoutRequest,
    AuthProvider, AuthStatusRequest,
};
#[cfg(test)]
pub(crate) use contracts::{AuthExitCode, CODEX_NATIVE_EXPERIMENTAL_DISCLOSURE};
pub(crate) use runner::run;
