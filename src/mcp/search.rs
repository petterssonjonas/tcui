use super::{lookup_profile, McpClient, McpError, McpResult, McpSession, McpToolSummary};
use crate::config::AppConfig;
use serde_json::{json, Map, Value};

const MAX_SEARCH_CHARS: usize = 24_000;

/// Run one provider-owned, read-only web search. No model chooses a tool or command.
pub async fn search(
    config: &AppConfig,
    provider: &str,
    query: &str,
    limit: usize,
) -> McpResult<String> {
    validate_query(query, limit)?;
    let profile = lookup_profile(provider)
        .filter(|profile| matches!(profile.skill, "exa" | "tavily" | "firecrawl"))
        .ok_or(McpError::UnknownProfile)?;
    // Missing rows are disabled, just like new installations; never silently enable a service.
    let server = config
        .mcp_servers
        .iter()
        .find(|server| lookup_profile(&server.name) == Some(profile))
        .ok_or_else(|| McpError::ProfileDisabled {
            name: profile.name.to_string(),
        })?;
    let session = McpClient::new().connect(server, config).await?;
    let result = search_session(&session, profile.skill, query.trim(), limit).await;
    let cleanup = session.shutdown().await;
    match result {
        Err(error) => Err(error),
        Ok(text) => {
            cleanup?;
            Ok(text)
        }
    }
}

fn validate_query(query: &str, limit: usize) -> McpResult<()> {
    if query.trim().is_empty() || query.chars().count() > 4096 || !(1..=20).contains(&limit) {
        return Err(McpError::InvalidSearch);
    }
    Ok(())
}

pub(crate) async fn search_session(
    session: &McpSession,
    provider: &str,
    query: &str,
    limit: usize,
) -> McpResult<String> {
    validate_query(query, limit)?;
    let tools = session.tool_summaries().await?;
    let (tool, arguments) = search_arguments(provider, &tools, query, limit)?;
    let result = session.call_tool(tool, arguments).await?;
    if result.structured.as_ref().is_some_and(empty_result_set)
        || result
            .text
            .iter()
            .filter(|text| text.trim_start().starts_with('{'))
            .filter_map(|text| serde_json::from_str::<Value>(text).ok())
            .any(|value| empty_result_set(&value))
    {
        return Err(McpError::EmptyResults);
    }
    render_search(&result.render(), limit)
}

fn empty_result_set(value: &Value) -> bool {
    [
        value.get("results"),
        value.pointer("/data/web"),
        value.get("data"),
    ]
    .into_iter()
    .flatten()
    .filter_map(Value::as_array)
    .any(Vec::is_empty)
}

fn search_arguments<'a>(
    provider: &str,
    tools: &'a [McpToolSummary],
    query: &str,
    limit: usize,
) -> McpResult<(&'a str, Map<String, Value>)> {
    let (names, count): (&[&str], &str) = match provider {
        "exa" => (&["web_search_exa"], "numResults"),
        // Older official Tavily stdio releases used a hyphen; never accept other tools.
        "tavily" => (&["tavily_search", "tavily-search"], "max_results"),
        "firecrawl" => (&["firecrawl_search"], "limit"),
        _ => return Err(McpError::UnknownProfile),
    };
    let tool = names
        .iter()
        .find_map(|name| tools.iter().find(|tool| tool.name == *name))
        .ok_or(McpError::UnsupportedSearchTool)?;
    if tool.read_only_hint == Some(false) || tool.destructive_hint == Some(true) {
        return Err(McpError::UnsupportedSearchTool);
    }
    let properties = tool
        .input_schema
        .get("properties")
        .and_then(Value::as_object)
        .ok_or(McpError::UnsupportedSearchTool)?;
    if !properties.contains_key("query") || !properties.contains_key(count) {
        return Err(McpError::UnsupportedSearchTool);
    }
    let count_schema = &properties[count];
    let minimum = count_schema
        .get("minimum")
        .and_then(Value::as_f64)
        .unwrap_or(1.0)
        .ceil() as usize;
    let maximum = count_schema
        .get("maximum")
        .and_then(Value::as_f64)
        .unwrap_or(20.0)
        .floor() as usize;
    if minimum > maximum || maximum == 0 {
        return Err(McpError::UnsupportedSearchTool);
    }
    if minimum > limit {
        return Err(McpError::SearchLimitTooLow { minimum });
    }
    let mut arguments = Map::new();
    arguments.insert("query".to_string(), json!(query));
    arguments.insert(count.to_string(), json!(limit.min(maximum)));
    if provider == "firecrawl" {
        // No crawl, extraction, browser interaction, or discovered third-party execution.
        if properties.contains_key("sources") {
            arguments.insert("sources".to_string(), json!(["web"]));
        }
        if properties.contains_key("domainTools") {
            arguments.insert("domainTools".to_string(), json!(false));
        }
        if properties.contains_key("objective") {
            arguments.insert(
                "objective".to_string(),
                json!("Find web sources relevant to the user's search query."),
            );
        }
    }
    if tool
        .input_schema
        .get("required")
        .and_then(Value::as_array)
        .is_some_and(|required| {
            required.iter().any(|name| {
                name.as_str()
                    .is_none_or(|name| !arguments.contains_key(name))
            })
        })
    {
        return Err(McpError::UnsupportedSearchTool);
    }
    if arguments
        .iter()
        .any(|(name, value)| !schema_accepts(&properties[name], value))
    {
        return Err(McpError::UnsupportedSearchTool);
    }
    Ok((&tool.name, arguments))
}

fn schema_accepts(schema: &Value, value: &Value) -> bool {
    if let Some(options) = schema
        .get("anyOf")
        .or_else(|| schema.get("oneOf"))
        .and_then(Value::as_array)
    {
        return options.iter().any(|option| schema_accepts(option, value));
    }
    let valid_type = match schema.get("type").and_then(Value::as_str) {
        None => true,
        Some("string") => value.is_string(),
        Some("number") => value.is_number(),
        Some("integer") => value.is_i64() || value.is_u64(),
        Some("boolean") => value.is_boolean(),
        Some("array") => value.as_array().is_some_and(|values| {
            schema
                .get("items")
                .is_none_or(|item| values.iter().all(|value| schema_accepts(item, value)))
        }),
        Some("object") => value.is_object(),
        _ => false,
    };
    valid_type
        && schema
            .get("enum")
            .and_then(Value::as_array)
            .is_none_or(|options| options.contains(value))
}

fn render_search(text: &str, limit: usize) -> McpResult<String> {
    // Put citations before excerpts so truncation never drops all actual source URLs.
    let mut urls = Vec::new();
    for (index, _) in text.match_indices("http") {
        let prefix = text[..index]
            .rsplit('\n')
            .next()
            .unwrap_or_default()
            .trim_end()
            .trim_end_matches('"')
            .trim_end();
        if !prefix.ends_with("URL:") && !prefix.ends_with("\"url\":") {
            continue;
        }
        let candidate = text[index..]
            .split(|ch: char| ch.is_whitespace() || matches!(ch, '"' | '\'' | '<' | '>' | '\\'))
            .next()
            .unwrap_or_default();
        if candidate.len() > 4096 {
            continue;
        }
        let Ok(url) = reqwest::Url::parse(candidate) else {
            continue;
        };
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            continue;
        }
        if !urls.contains(&candidate) {
            urls.push(candidate);
        }
        if urls.len() == limit {
            break;
        }
    }
    if urls.is_empty() {
        return Err(McpError::EmptyResults);
    }
    let mut rendered = String::from("Sources:\n");
    for url in urls {
        if rendered.chars().count() + url.chars().count() + 1 > MAX_SEARCH_CHARS / 2 {
            break;
        }
        rendered.push_str(url);
        rendered.push('\n');
    }
    rendered.push_str("\nSearch evidence:\n");
    let remaining = MAX_SEARCH_CHARS.saturating_sub(rendered.chars().count());
    rendered.extend(text.chars().take(remaining));
    Ok(rendered)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn missing_and_disabled_profiles_never_connect() {
        let mut config = AppConfig::default();
        config.mcp_servers.clear();
        assert!(matches!(
            search(&config, "exa", "rust", 3).await,
            Err(McpError::ProfileDisabled { .. })
        ));
        config
            .mcp_servers
            .push(super::super::profile_by_skill("exa").unwrap().config(false));
        assert!(matches!(
            search(&config, "exa", "rust", 3).await,
            Err(McpError::ProfileDisabled { .. })
        ));
    }

    #[test]
    fn empty_result_text_is_not_success() {
        assert!(matches!(
            render_search("No search results found", 5),
            Err(McpError::EmptyResults)
        ));
        assert!(matches!(
            render_search("No results; sign up at https://provider.example/signup", 5),
            Err(McpError::EmptyResults)
        ));
    }

    #[test]
    fn result_is_bounded_and_retains_source_urls() {
        let text = format!("URL: https://example.org/source\n{}", "x".repeat(60_000));
        let result = render_search(&text, 5).unwrap();
        assert!(result.contains("https://example.org/source"));
        assert!(result.chars().count() <= MAX_SEARCH_CHARS);
    }

    #[test]
    fn source_url_punctuation_is_preserved() {
        let result = render_search("URL: https://example.org/wiki/Foo_(bar)", 1).unwrap();
        assert!(result
            .lines()
            .any(|line| line == "https://example.org/wiki/Foo_(bar)"));
    }
}
