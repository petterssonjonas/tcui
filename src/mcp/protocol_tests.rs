use super::*;
use crate::mcp::{profile_by_skill, search::search_session};
use serde_json::json;
use std::sync::{Arc, Mutex};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
};

struct Fixture {
    url: mcp_reqwest::Url,
    requests: Arc<Mutex<Vec<(String, Value)>>>,
    task: JoinHandle<()>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Fixture {
    async fn new(tools: Value, result: Value, initialize_status: u16) -> Self {
        Self::with_redirect(tools, result, initialize_status, None).await
    }

    async fn with_redirect(
        tools: Value,
        result: Value,
        initialize_status: u16,
        redirect: Option<String>,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url =
            mcp_reqwest::Url::parse(&format!("http://{}/mcp", listener.local_addr().unwrap()))
                .unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        let task = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                let (header_end, length) = loop {
                    let mut chunk = [0; 4096];
                    let count = socket.read(&mut chunk).await.unwrap();
                    if count == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&chunk[..count]);
                    assert!(bytes.len() <= 128_000);
                    if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&bytes[..end]);
                        let length = headers
                            .lines()
                            .find_map(|line| {
                                let (name, value) = line.split_once(':')?;
                                name.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        break (end + 4, length);
                    }
                };
                while bytes.len() < header_end + length {
                    let mut chunk = [0; 4096];
                    let count = socket.read(&mut chunk).await.unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&chunk[..count]);
                }
                let headers = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
                let request: Value = if length == 0 {
                    Value::Null
                } else {
                    serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap()
                };
                recorded
                    .lock()
                    .unwrap()
                    .push((headers.clone(), request.clone()));
                let method = request["method"].as_str().unwrap_or_default();
                let (status, body) = if headers.starts_with("GET ") {
                    (405, String::new())
                } else if headers.starts_with("DELETE ") {
                    (200, String::new())
                } else if method == "initialize" && initialize_status != 200 {
                    (initialize_status, "Authorization: Bearer stored-protocol-secret https://invalid/?api_key=stored-protocol-secret".into())
                } else if method == "notifications/initialized" || request.get("id").is_none() {
                    (202, String::new())
                } else {
                    let payload = match method {
                        "initialize" => {
                            json!({"protocolVersion":request["params"]["protocolVersion"],"capabilities":{"tools":{}},"serverInfo":{"name":"protocol-fixture","version":"1"}})
                        }
                        "tools/list" => json!({"tools":tools}),
                        "tools/call" => result.clone(),
                        _ => panic!("unexpected MCP method: {method}"),
                    };
                    let response = if method == "tools/call" && result.get("$rpcError").is_some() {
                        json!({"jsonrpc":"2.0","id":request["id"],"error":result["$rpcError"]})
                    } else {
                        json!({"jsonrpc":"2.0","id":request["id"],"result":payload})
                    };
                    (200, response.to_string())
                };
                let location = redirect
                    .as_ref()
                    .map(|url| format!("Location: {url}\r\n"))
                    .unwrap_or_default();
                let response = format!("HTTP/1.1 {status} Fixture\r\n{location}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                socket.write_all(response.as_bytes()).await.unwrap();
                socket.shutdown().await.unwrap();
            }
        });
        Self {
            url,
            requests,
            task,
        }
    }

    async fn connect(&self, provider: &str) -> McpResult<McpSession> {
        let secret = Zeroizing::new("stored-protocol-secret".to_string());
        let transport = http_transport(
            self.url.clone(),
            profile_by_skill(provider),
            Some(secret.as_str()),
        )?;
        let service = timeout(Duration::from_secs(3), ().serve(transport))
            .await
            .map_err(|_| McpError::Timeout {
                operation: "fixture initialize",
                seconds: 3,
            })??;
        Ok(McpSession {
            service,
            secret: Some(secret),
        })
    }
}

fn search_tool(provider: &str) -> Value {
    let (name, count) = match provider {
        "exa" => ("web_search_exa", "numResults"),
        "tavily" => ("tavily_search", "max_results"),
        _ => ("firecrawl_search", "limit"),
    };
    let mut properties = json!({"query":{"type":"string"}});
    properties[count] =
        json!({"type":"number","minimum":if provider == "tavily" {5} else {1},"maximum":20});
    let mut required = vec![json!("query")];
    if provider == "firecrawl" {
        properties["sources"] = json!({"type":"array","items":{"anyOf":[{"type":"string","enum":["web","news"]},{"type":"object"}]}});
        properties["domainTools"] = json!({"type":"boolean"});
        properties["objective"] = json!({"type":"string"});
        required.push(json!("objective"));
    }
    json!({"name":name,"inputSchema":{"type":"object","properties":properties,"required":required},"annotations":{"readOnlyHint":true,"destructiveHint":false}})
}

#[tokio::test]
async fn all_providers_discover_only_search_authenticate_and_return_real_sources() {
    for provider in ["exa", "tavily", "firecrawl"] {
        let tool = search_tool(provider);
        let fixture = Fixture::new(json!([
            {"name":"execute_code","inputSchema":{"type":"object"}},
            {"name":"firecrawl_crawl","inputSchema":{"type":"object"}},
            {"name":"agent_run","inputSchema":{"type":"object"}}, tool,
        ]), json!({"content":[{"type":"text","text":"Title: Primary source\nURL: https://example.org/actual-source\nSnippet: stored-protocol-secret"}],"structuredContent":{"results":[{"url":"https://example.org/actual-source","title":"Primary source"}]}}), 200).await;
        let session = fixture.connect(provider).await.unwrap();
        let result = search_session(&session, provider, "rust security", 5)
            .await
            .unwrap();
        session.shutdown().await.unwrap();
        assert!(result.contains("https://example.org/actual-source"));
        assert!(!result.contains("stored-protocol-secret"));
        let requests = fixture.requests.lock().unwrap();
        let calls: Vec<_> = requests
            .iter()
            .filter(|(_, request)| request["method"] == "tools/call")
            .collect();
        assert_eq!(calls.len(), 1, "one search, no planner/crawl/code calls");
        assert_eq!(calls[0].1["params"]["name"], tool["name"]);
        assert_eq!(calls[0].1["params"]["arguments"]["query"], "rust security");
        let count = match provider {
            "exa" => "numResults",
            "tavily" => "max_results",
            _ => "limit",
        };
        assert_eq!(calls[0].1["params"]["arguments"][count], 5);
        if provider == "firecrawl" {
            assert_eq!(calls[0].1["params"]["arguments"]["sources"], json!(["web"]));
            assert_eq!(calls[0].1["params"]["arguments"]["domainTools"], false);
            assert!(calls[0].1["params"]["arguments"]["objective"].is_string());
            assert!(calls[0].1["params"]["arguments"]
                .get("scrapeOptions")
                .is_none());
        }
        for (headers, _) in requests.iter() {
            let expected = if provider == "exa" {
                "x-api-key: stored-protocol-secret"
            } else {
                "authorization: Bearer stored-protocol-secret"
            };
            assert!(headers
                .to_ascii_lowercase()
                .contains(&expected.to_ascii_lowercase()));
            assert!(!headers
                .lines()
                .next()
                .unwrap()
                .contains("stored-protocol-secret"));
        }
    }
}

#[tokio::test]
async fn initialization_auth_failures_do_not_expose_server_echoes() {
    let fixture = Fixture::new(json!([]), Value::Null, 401).await;
    let error = match fixture.connect("tavily").await {
        Ok(_) => panic!("unauthorized session"),
        Err(error) => error,
    };
    assert!(matches!(error, McpError::Initialize));
    assert!(!format!("{error} {error:?}").contains("stored-protocol-secret"));
}

#[tokio::test]
async fn tool_errors_empty_results_and_missing_search_are_honest() {
    for (tools, result, expected) in [
        (
            json!([search_tool("exa")]),
            json!({"content":[{"type":"text","text":"Authorization: Bearer stored-protocol-secret"}],"isError":true}),
            "tool",
        ),
        (
            json!([search_tool("exa")]),
            json!({"content":[{"type":"text","text":"No search results found"}]}),
            "empty",
        ),
        (
            json!([{"name":"agent_run","inputSchema":{"type":"object"}}]),
            json!({"content":[]}),
            "discovery",
        ),
    ] {
        let fixture = Fixture::new(tools, result, 200).await;
        let session = fixture.connect("exa").await.unwrap();
        let error = search_session(&session, "exa", "query", 5)
            .await
            .unwrap_err();
        session.shutdown().await.unwrap();
        match expected {
            "tool" => assert!(matches!(error, McpError::ToolFailed)),
            "empty" => assert!(matches!(error, McpError::EmptyResults)),
            _ => assert!(matches!(error, McpError::UnsupportedSearchTool)),
        }
        assert!(!format!("{error:?} {error}").contains("stored-protocol-secret"));
        let requests = fixture.requests.lock().unwrap();
        let calls = requests
            .iter()
            .filter(|(_, request)| request["method"] == "tools/call")
            .count();
        assert_eq!(calls, usize::from(expected != "discovery"));
    }
}

#[tokio::test]
async fn incompatible_or_destructive_search_schema_is_not_called() {
    for malformed in [false, true] {
        let mut tool = search_tool("firecrawl");
        if malformed {
            tool["inputSchema"]["required"] = json!(["query", "execute_code"]);
        } else {
            tool["annotations"]["destructiveHint"] = json!(true);
        }
        let fixture = Fixture::new(json!([tool]), json!({"content":[]}), 200).await;
        let session = fixture.connect("firecrawl").await.unwrap();
        assert!(matches!(
            search_session(&session, "firecrawl", "query", 5).await,
            Err(McpError::UnsupportedSearchTool)
        ));
        session.shutdown().await.unwrap();
        assert!(!fixture
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|(_, request)| request["method"] == "tools/call"));
    }
}

#[tokio::test]
async fn provider_minimum_never_increases_the_requested_result_limit() {
    let fixture = Fixture::new(json!([search_tool("tavily")]), json!({"content":[]}), 200).await;
    let session = fixture.connect("tavily").await.unwrap();
    assert!(matches!(
        search_session(&session, "tavily", "query", 3).await,
        Err(McpError::SearchLimitTooLow { minimum: 5 })
    ));
    session.shutdown().await.unwrap();
    assert!(!fixture
        .requests
        .lock()
        .unwrap()
        .iter()
        .any(|(_, request)| request["method"] == "tools/call"));
}

#[tokio::test]
async fn credential_bearing_http_redirects_are_not_followed() {
    let sink = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target = format!("http://{}/credential-sink", sink.local_addr().unwrap());
    let fixture = Fixture::with_redirect(json!([]), Value::Null, 307, Some(target)).await;
    assert!(fixture.connect("exa").await.is_err());
    assert!(timeout(Duration::from_millis(100), sink.accept())
        .await
        .is_err());
    assert_eq!(fixture.requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn protocol_rpc_errors_do_not_retain_secrets_in_error_chains() {
    let fixture = Fixture::new(json!([search_tool("exa")]), json!({"$rpcError":{"code":-32603,"message":"x-api-key: stored-protocol-secret https://invalid/?key=stored-protocol-secret"}}), 200).await;
    let session = fixture.connect("exa").await.unwrap();
    let error = search_session(&session, "exa", "query", 5)
        .await
        .unwrap_err();
    session.shutdown().await.unwrap();
    assert!(matches!(error, McpError::Service));
    assert!(!format!("{error:?} {error}").contains("stored-protocol-secret"));
    assert!(std::error::Error::source(&error).is_none());
}

#[tokio::test]
async fn structured_only_search_result_preserves_actual_source_url() {
    let fixture = Fixture::new(json!([search_tool("firecrawl")]), json!({"content":[],"structuredContent":{"success":true,"data":{"web":[{"url":"https://example.org/structured-source","title":"Structured source","description":"x".repeat(60_000)}]}}}), 200).await;
    let session = fixture.connect("firecrawl").await.unwrap();
    let result = search_session(&session, "firecrawl", "query", 5)
        .await
        .unwrap();
    session.shutdown().await.unwrap();
    assert!(result.contains("https://example.org/structured-source"));
    assert!(result.chars().count() <= 24_000);
}

#[tokio::test]
async fn provider_catalogue_links_are_not_mistaken_for_search_results() {
    let fixture = Fixture::new(json!([search_tool("firecrawl")]), json!({"content":[],"structuredContent":{"data":{"web":[],"tools":[{"url":"https://provider.example/tool","description":"Discovered tool"}]}}}), 200).await;
    let session = fixture.connect("firecrawl").await.unwrap();
    assert!(matches!(
        search_session(&session, "firecrawl", "query", 5).await,
        Err(McpError::EmptyResults)
    ));
    session.shutdown().await.unwrap();
}

#[tokio::test]
async fn truncated_search_output_cannot_expose_a_credential_prefix() {
    let public = "URL: https://example.org/public\n";
    let sensitive = "\nURL: https://example.org/?key=";
    let padding = "x".repeat(super::MAX_RESULT_CONTENT_CHARS - public.len() - sensitive.len() - 8);
    let text = format!("{public}{padding}{sensitive}stored-protocol-secret");
    let fixture = Fixture::new(
        json!([search_tool("exa")]),
        json!({"content":[{"type":"text","text":text}]}),
        200,
    )
    .await;
    let session = fixture.connect("exa").await.unwrap();
    let result = search_session(&session, "exa", "query", 5).await.unwrap();
    session.shutdown().await.unwrap();
    assert!(result.contains("https://example.org/public"));
    assert!(
        !result.contains("stored-p"),
        "truncation must not defeat credential redaction"
    );
}
