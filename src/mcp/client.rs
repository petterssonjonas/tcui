use crate::{
    config::AppConfig,
    mcp::{
        error::{McpError, McpResult},
        registry::{lookup_profile, McpProfile},
        transport::spawn_stdio,
    },
};
use rmcp::{
    model::{CallToolRequestParams, CallToolResult, Tool},
    service::{RoleClient, RunningService, ServiceExt},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::time::Duration;
use tokio::time::timeout;

const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(20);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_ERROR_DETAIL_CHARS: usize = 4_000;
const MAX_RESULT_CONTENT_CHARS: usize = 48_000;

pub struct McpClient;

pub struct McpSession {
    service: RunningService<RoleClient, ()>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpToolSummary {
    pub name: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub input_schema: Value,
    pub output_schema: Option<Value>,
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
        let profile = lookup_profile(&config.name).ok_or_else(|| McpError::UnknownProfile {
            query: config.name.clone(),
        })?;
        if !config.enabled {
            return Err(McpError::ProfileDisabled {
                name: config.name.clone(),
            });
        }

        let transport = spawn_stdio(profile, app_config)?;
        let service = timeout(INITIALIZE_TIMEOUT, ().serve(transport))
            .await
            .map_err(|_| McpError::Timeout {
                operation: "initialize",
                seconds: INITIALIZE_TIMEOUT.as_secs(),
            })??;
        Ok(McpSession { service })
    }
}

impl Default for McpClient {
    fn default() -> Self {
        Self::new()
    }
}

impl McpSession {
    pub async fn tool_summaries(&self) -> McpResult<Vec<McpToolSummary>> {
        let tools = timeout(REQUEST_TIMEOUT, self.service.peer().list_all_tools())
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
                .call_tool(CallToolRequestParams::new(name.clone()).with_arguments(arguments)),
        )
        .await
        .map_err(|_| McpError::Timeout {
            operation: "tool call",
            seconds: REQUEST_TIMEOUT.as_secs(),
        })??;
        McpToolCallResult::try_from((name.as_str(), result))
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
            Err(error) => Err(McpError::Shutdown {
                detail: error.to_string(),
            }),
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
        }
    }
}

impl TryFrom<(&str, CallToolResult)> for McpToolCallResult {
    type Error = McpError;

    fn try_from((tool, result): (&str, CallToolResult)) -> McpResult<Self> {
        let mut text = Vec::new();
        let mut fallback = Vec::new();
        let mut budget = MAX_RESULT_CONTENT_CHARS;

        for item in result.content {
            if let Some(text_content) = item.raw.as_text() {
                push_bounded(&mut text, &mut budget, &text_content.text);
            } else {
                match serde_json::to_string(&item) {
                    Ok(serialized) => push_bounded(&mut fallback, &mut budget, &serialized),
                    Err(_) => push_bounded(&mut fallback, &mut budget, &format!("{item:?}")),
                }
            }
            if budget == 0 {
                break;
            }
        }

        let converted = Self {
            text,
            fallback,
            structured: result.structured_content,
            is_error: result.is_error.unwrap_or(false),
        };
        if converted.is_error {
            return Err(McpError::ToolFailed {
                tool: tool.to_string(),
                detail: converted
                    .render()
                    .chars()
                    .take(MAX_ERROR_DETAIL_CHARS)
                    .collect(),
            });
        }
        Ok(converted)
    }
}

impl McpToolCallResult {
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
            sections.join("\n")
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

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::Content;

    #[test]
    fn tool_error_result_is_a_typed_error() {
        // Given
        let result = CallToolResult::error(vec![Content::text("upstream rejected request")]);

        // When
        let error = McpToolCallResult::try_from(("search", result)).expect_err("tool error");

        // Then
        assert!(matches!(
            error,
            McpError::ToolFailed { tool, detail }
                if tool == "search" && detail.contains("upstream rejected request")
        ));
    }
}
