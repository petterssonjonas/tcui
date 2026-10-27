use rmcp::service::{ClientInitializeError, ServiceError};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum McpError {
    #[error("unknown MCP search provider; choose exa, tavily, or firecrawl")]
    UnknownProfile,
    #[error("MCP profile '{name}' is disabled; enable it in Settings > Web search > Search API keys or MCP Servers")]
    ProfileDisabled { name: String },
    #[error("MCP profile '{profile}' needs a stored API key; add '{key_store_name}' in Settings > Web search > Search API keys")]
    MissingApiKey {
        profile: &'static str,
        key_store_name: &'static str,
    },
    #[error("cannot read '{key_store_name}' from TCUI KeyStore; unlock the vault and check Settings > Web search > Search API keys")]
    CredentialStore { key_store_name: &'static str },
    #[error("'{key_store_name}' also names a configured chat provider; resolve this credential-name collision in Settings before using MCP search")]
    CredentialCollision { key_store_name: &'static str },
    #[error("invalid MCP configuration: {detail}")]
    Configuration { detail: &'static str },
    #[error("configured MCP command is not available on PATH; install it or select the hosted streamable_http preset")]
    MissingCommand,
    #[error("could not start configured MCP command; check its executable and permissions")]
    SpawnCommand,
    #[error(
        "MCP {operation} timed out after {seconds}s; check connectivity and provider availability"
    )]
    Timeout {
        operation: &'static str,
        seconds: u64,
    },
    #[error("MCP tool failed; check the stored provider key, account credits, query, and provider availability")]
    ToolFailed,
    #[error("MCP server does not expose the supported read-only search tool or its expected argument schema")]
    UnsupportedSearchTool,
    #[error("MCP search requires at least {minimum} results; increase Search result limit in Settings > Web search")]
    SearchLimitTooLow { minimum: usize },
    #[error("MCP search returned no source URLs")]
    EmptyResults,
    #[error("MCP search needs a nonempty query of at most 4096 characters and a result limit from 1 to 20")]
    InvalidSearch,
    #[error("failed to shut down MCP session; the session was cancelled")]
    Shutdown,
    // Upstream errors may contain credential-bearing URLs, headers, or echoed server content.
    // Do not retain them as sources: Debug and chained error renderers must be safe too.
    #[error("MCP initialization failed; check transport, endpoint, stored API key, and provider availability")]
    Initialize,
    #[error(
        "MCP request failed; check the stored API key, account access, and provider availability"
    )]
    Service,
}

pub type McpResult<T> = Result<T, McpError>;

impl From<ClientInitializeError> for McpError {
    fn from(_: ClientInitializeError) -> Self {
        Self::Initialize
    }
}

impl From<ServiceError> for McpError {
    fn from(_: ServiceError) -> Self {
        Self::Service
    }
}
