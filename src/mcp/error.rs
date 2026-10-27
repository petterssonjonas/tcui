use rmcp::service::{ClientInitializeError, ServiceError};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum McpError {
    #[error("unknown MCP profile '{query}'")]
    UnknownProfile { query: String },
    #[error("MCP profile '{name}' is disabled")]
    ProfileDisabled { name: String },
    #[error("MCP profile '{profile}' needs {env_var}")]
    MissingVaultPath {
        profile: &'static str,
        env_var: &'static str,
    },
    #[error(
        "MCP profile '{profile}' needs {env_var}; store it in the '{key_store_name}' key store or set the environment variable"
    )]
    MissingApiKey {
        profile: &'static str,
        env_var: &'static str,
        key_store_name: &'static str,
    },
    #[error("command '{program}' is not available on PATH; {hint}")]
    MissingCommand {
        program: &'static str,
        hint: &'static str,
    },
    #[error("MCP {operation} timed out after {seconds}s; retry or check the server logs")]
    Timeout {
        operation: &'static str,
        seconds: u64,
    },
    #[error("MCP tool '{tool}' failed: {detail}")]
    ToolFailed { tool: String, detail: String },
    #[error("failed to shut down MCP session: {detail}")]
    Shutdown { detail: String },
    #[error("failed to spawn '{program}': {source}")]
    SpawnCommand {
        program: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Initialize(Box<ClientInitializeError>),
    #[error(transparent)]
    Service(Box<ServiceError>),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type McpResult<T> = Result<T, McpError>;

impl From<ClientInitializeError> for McpError {
    fn from(error: ClientInitializeError) -> Self {
        Self::Initialize(Box::new(error))
    }
}

impl From<ServiceError> for McpError {
    fn from(error: ServiceError) -> Self {
        Self::Service(Box::new(error))
    }
}
