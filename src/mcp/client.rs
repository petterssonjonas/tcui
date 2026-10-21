use crate::{
    config::AppConfig,
    mcp::{
        error::{McpError, McpResult},
        registry::lookup_profile,
        transport::{http_transport, resolve_secret, spawn_stdio, validate_http_endpoint},
    },
};
use rmcp::{
    model::{CallToolRequestParams, CallToolResult, PaginatedRequestParams, RawContent, Tool},
    service::{RoleClient, RunningService, ServiceExt},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::time::Duration;
use tokio::time::timeout;
use zeroize::Zeroizing;

const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(20);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_TOOL_PAGES: usize = 8;
const MAX_TOOLS: usize = 256;
const MAX_RESULT_CONTENT_CHARS: usize = 48_000;

pub struct McpClient;

pub struct McpSession {
    service: RunningService<RoleClient, ()>,
    secret: Option<Zeroizing<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpToolSummary {
    pub name: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub input_schema: Value,
    pub output_schema: Option<Value>,
    pub read_only_hint: Option<bool>,
    pub destructive_hint: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpToolCallResult {
    pub text: Vec<String>,
    pub fallback: Vec<String>,
    pub structured: Option<Value>,
    pub is_error: bool,
}

impl McpClient {
    pub fn new() -> Self {
        Self
    }

    pub async fn connect(
        &self,
        config: &crate::config::McpServerConfig,
        app_config: &AppConfig,
    ) -> McpResult<McpSession> {
        let profile = lookup_profile(&config.name);
        if !config.enabled {
            return Err(McpError::ProfileDisabled {
                name: profile
                    .map_or("custom server", |profile| profile.name)
                    .to_string(),
            });
        }
        // Validate the destination before loading any stored credential.
        let endpoint = match config.transport.as_str() {
            "streamable_http" => Some(validate_http_endpoint(config, profile)?),
            "stdio" => None,
            _ => {
                return Err(McpError::Configuration {
                    detail: "supported transports are streamable_http and stdio",
                })
            }
        };
        let secret = resolve_secret(profile, app_config)?;
        let initialize = async {
            match endpoint {
                Some(url) => ()
                    .serve(http_transport(
                        url,
                        profile,
                        secret.as_deref().map(String::as_str),
                    )?)
                    .await
                    .map_err(McpError::from),
                None => ()
                    .serve(spawn_stdio(
                        config,
                        profile,
                        app_config,
                        secret.as_deref().map(String::as_str),
                    )?)
                    .await
                    .map_err(McpError::from),
            }
        };
        let service = timeout(INITIALIZE_TIMEOUT, initialize)
            .await
            .map_err(|_| McpError::Timeout {
                operation: "initialize",
                seconds: INITIALIZE_TIMEOUT.as_secs(),
            })??;
        Ok(McpSession { service, secret })
    }
}

impl Default for McpClient {
    fn default() -> Self {
        Self::new()
    }
}

impl McpSession {
    pub async fn tool_summaries(&self) -> McpResult<Vec<McpToolSummary>> {
        let tools = timeout(REQUEST_TIMEOUT, async {
            let mut tools = Vec::new();
            let mut cursor = None;
            for _ in 0..MAX_TOOL_PAGES {
                let page = self
                    .service
                    .peer()
                    .list_tools(Some(
                        PaginatedRequestParams::default().with_cursor(cursor.clone()),
                    ))
                    .await?;
                if tools.len() + page.tools.len() > MAX_TOOLS {
                    return Err(McpError::UnsupportedSearchTool);
                }
                tools.extend(page.tools);
                match page.next_cursor {
                    None => return Ok(tools),
                    Some(next) if cursor.as_ref() != Some(&next) => cursor = Some(next),
                    Some(_) => return Err(McpError::UnsupportedSearchTool),
                }
            }
            Err(McpError::UnsupportedSearchTool)
        })
        .await
        .map_err(|_| McpError::Timeout {
            operation: "list tools",
            seconds: REQUEST_TIMEOUT.as_secs(),
        })??;
        Ok(tools.iter().map(McpToolSummary::from).collect())
    }

    pub async fn call_tool(
        &self,
        name: impl Into<String>,
        arguments: Map<String, Value>,
    ) -> McpResult<McpToolCallResult> {
        let name = name.into();
        let result = timeout(
            REQUEST_TIMEOUT,
            self.service
                .peer()
                .call_tool(CallToolRequestParams::new(name).with_arguments(arguments)),
        )
        .await
        .map_err(|_| McpError::Timeout {
            operation: "tool call",
            seconds: REQUEST_TIMEOUT.as_secs(),
        })??;
        McpToolCallResult::from_result(result, self.secret.as_deref().map(String::as_str))
    }

    pub fn render_result(result: &McpToolCallResult) -> String {
        result.render()
    }

    pub async fn shutdown(mut self) -> McpResult<()> {
        match self.service.close_with_timeout(SHUTDOWN_TIMEOUT).await {
            Ok(Some(_)) => Ok(()),
            Ok(None) => Err(McpError::Timeout {
                operation: "shutdown",
                seconds: SHUTDOWN_TIMEOUT.as_secs(),
            }),
            Err(_) => Err(McpError::Shutdown),
        }
    }
}

impl From<&Tool> for McpToolSummary {
    fn from(tool: &Tool) -> Self {
        Self {
            name: tool.name.to_string(),
            title: tool.title.clone(),
            description: tool.description.as_ref().map(ToString::to_string),
            input_schema: Value::Object(tool.input_schema.as_ref().clone()),
            output_schema: tool
                .output_schema
                .as_ref()
                .map(|schema| Value::Object(schema.as_ref().clone())),
            read_only_hint: tool
                .annotations
                .as_ref()
                .and_then(|annotations| annotations.read_only_hint),
            destructive_hint: tool
                .annotations
                .as_ref()
                .and_then(|annotations| annotations.destructive_hint),
        }
    }
}

impl McpToolCallResult {
    fn from_result(mut result: CallToolResult, secret: Option<&str>) -> McpResult<Self> {
        if result.is_error.unwrap_or(false) {
            return Err(McpError::ToolFailed);
        }
        // Redact complete values before bounding them: truncation must not reveal key prefixes.
        let redactions = secret.map(|secret| {
            let encoded =
                mcp_reqwest::Url::parse_with_params("https://redacted.invalid", &[("key", secret)])
                    .ok()
                    .and_then(|url| {
                        url.query()
                            .and_then(|query| query.strip_prefix("key="))
                            .map(ToString::to_string)
                    })
                    .unwrap_or_default();
            let mut escaped = serde_json::to_string(secret).unwrap_or_default();
            if escaped.len() >= 2 {
                escaped.remove(0);
                escaped.pop();
            }
            Zeroizing::new([secret.to_string(), encoded, escaped])
        });
        let variants = redactions
            .as_deref()
            .map_or(&[][..], |values| values.as_slice());
        if let Some(structured) = result.structured_content.as_mut() {
            redact_value(structured, variants);
        }
        let mut text = Vec::new();
        let mut fallback = Vec::new();
        let mut budget = MAX_RESULT_CONTENT_CHARS;
        if let Some(structured) = result.structured_content.as_ref() {
            let mut source_budget = 8_000;
            collect_source_urls(structured, &mut text, &mut source_budget, 0);
            budget -= 8_000 - source_budget;
        }

        for item in result.content {
            match item.raw {
                RawContent::Text(mut content) => {
                    redact_secret(&mut content.text, variants);
                    push_bounded(&mut text, &mut budget, &content.text);
                }
                _ => {
                    let mut serialized =
                        serde_json::to_string(&item).unwrap_or_else(|_| format!("{item:?}"));
                    redact_secret(&mut serialized, variants);
                    push_bounded(&mut fallback, &mut budget, &serialized);
                }
            }
            if budget == 0 {
                break;
            }
        }

        let structured = result.structured_content.filter(|value| {
            // Structured content shares the output budget with text content.
            serde_json::to_writer(&mut BudgetWriter(budget), value).is_ok()
        });
        Ok(Self {
            text,
            fallback,
            structured,
            is_error: false,
        })
    }

    pub fn render(&self) -> String {
        let mut sections = Vec::new();
        if !self.text.is_empty() {
            sections.push(self.text.join("\n"));
        }
        if !self.fallback.is_empty() {
            sections.push(self.fallback.join("\n"));
        }
        if let Some(structured) = &self.structured {
            sections.push(match serde_json::to_string_pretty(structured) {
                Ok(json) => json,
                Err(_) => structured.to_string(),
            });
        }
        if self.is_error && sections.is_empty() {
            "tool call returned an error".to_string()
        } else {
            sections
                .join("\n")
                .chars()
                .take(MAX_RESULT_CONTENT_CHARS)
                .collect()
        }
    }
}

fn push_bounded(parts: &mut Vec<String>, budget: &mut usize, value: &str) {
    if *budget == 0 {
        return;
    }
    let bounded: String = value.chars().take(*budget).collect();
    *budget = budget.saturating_sub(bounded.chars().count());
    parts.push(bounded);
}

struct BudgetWriter(usize);

impl std::io::Write for BudgetWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.0 {
            return Err(std::io::Error::other("MCP result exceeds output budget"));
        }
        self.0 -= bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn collect_source_urls(value: &Value, parts: &mut Vec<String>, budget: &mut usize, depth: usize) {
    if depth > 8 || *budget == 0 {
        return;
    }
    match value {
        Value::Array(values) => {
            for value in values.iter().take(20) {
                collect_source_urls(value, parts, budget, depth + 1);
                if *budget == 0 {
                    break;
                }
            }
        }
        Value::Object(values) => {
            if let Some(url) = values.get("url").and_then(Value::as_str) {
                if url.len() <= 4096 && url.len() + 5 <= *budget {
                    let line = format!("URL: {url}");
                    *budget -= line.len();
                    parts.push(line);
                }
            }
            for name in ["results", "data", "web"] {
                if let Some(value) = values.get(name) {
                    collect_source_urls(value, parts, budget, depth + 1);
                }
            }
        }
        _ => {}
    }
}

fn redact_secret(text: &mut String, variants: &[String]) {
    for secret in variants {
        if !secret.is_empty() && text.contains(secret.as_str()) {
            *text = text.replace(secret.as_str(), "[REDACTED]");
        }
    }
}

fn redact_value(value: &mut Value, variants: &[String]) {
    match value {
        Value::String(text) => redact_secret(text, variants),
        Value::Array(values) => values
            .iter_mut()
            .for_each(|value| redact_value(value, variants)),
        Value::Object(values) => {
            if values.keys().any(|key| {
                variants
                    .iter()
                    .any(|secret| !secret.is_empty() && key.contains(secret.as_str()))
            }) {
                let original = std::mem::take(values);
                *values = original
                    .into_iter()
                    .map(|(mut key, mut value)| {
                        redact_secret(&mut key, variants);
                        redact_value(&mut value, variants);
                        (key, value)
                    })
                    .collect();
            } else {
                values
                    .values_mut()
                    .for_each(|value| redact_value(value, variants));
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::Content;

    #[test]
    fn tool_error_result_is_a_typed_error() {
        // Given
        let result = CallToolResult::error(vec![Content::text("Authorization: Bearer secret")]);
        let error = McpToolCallResult::from_result(result, Some("secret")).expect_err("tool error");
        assert!(matches!(error, McpError::ToolFailed));
        assert!(!format!("{error:?} {error}").contains("secret"));
    }
}

#[cfg(test)]
#[path = "protocol_tests.rs"]
mod protocol_tests;
