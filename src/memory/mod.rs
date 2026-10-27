mod activity;
mod config;
mod contextual;
mod embedding;
mod host;
mod index;
mod management;
mod markdown;
mod mcp_server;
mod metadata;
mod paths;
mod recall;
mod store;
mod sync;
mod write;

#[cfg(test)]
mod protection_tests;

pub(crate) use activity::{activities, set_activities, MemoryActivity};
pub(crate) use config::MemoryConfig;
pub(crate) use contextual::{invocation, run_skill_operation};
pub(crate) use host::{append_recall, is_memory_file_operation, recall};
pub(crate) use mcp_server::run as run_mcp;
pub(crate) use metadata::MemorySource;
pub(crate) use store::{MemoryDocument, MemoryError, MemoryStore};
pub(crate) use write::WriteOutcome;
