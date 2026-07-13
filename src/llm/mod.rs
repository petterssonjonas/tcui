pub(crate) mod auth;
pub(crate) mod chat;
pub mod client;
pub(crate) mod codex_responses;
pub(crate) mod local;
pub mod model_fetcher;
pub mod provider;
pub mod tools;

pub use client::LlmClient;

#[cfg(test)]
#[path = "openrouter_regression_tests.rs"]
mod openrouter_regression_tests;
