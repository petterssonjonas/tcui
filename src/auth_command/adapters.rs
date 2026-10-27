use super::AuthCommandError;
#[cfg(not(debug_assertions))]
use super::AuthProvider;
use crate::llm::auth::{codex::CodexNativeAdapter, openrouter::OpenRouterAdapter};

pub(super) fn codex_native_adapter() -> Result<CodexNativeAdapter, AuthCommandError> {
    #[cfg(debug_assertions)]
    {
        super::debug::codex_native_adapter()
    }
    #[cfg(not(debug_assertions))]
    {
        CodexNativeAdapter::production().map_err(|_| AuthCommandError::TransportFailure {
            provider: AuthProvider::Codex,
        })
    }
}

pub(super) fn openrouter_adapter() -> Result<OpenRouterAdapter, AuthCommandError> {
    #[cfg(debug_assertions)]
    {
        super::debug::openrouter_adapter()
    }
    #[cfg(not(debug_assertions))]
    {
        OpenRouterAdapter::production().map_err(|_| AuthCommandError::TransportFailure {
            provider: AuthProvider::OpenRouter,
        })
    }
}
