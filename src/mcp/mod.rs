#![allow(unused_imports)]

pub mod client;
pub mod error;
pub mod registry;
pub mod transport;

pub use client::{McpClient, McpSession, McpToolCallResult, McpToolSummary};
pub use error::{McpError, McpResult};
pub use registry::{
    lookup_profile, merged_configs, profile_by_name, profile_by_skill, profiles, McpCapabilities,
    McpProfile,
};
