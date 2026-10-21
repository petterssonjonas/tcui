use std::cell::{Cell, RefCell};
use std::time::Duration;

use color_eyre::{eyre::eyre, Result};
use lol_html::{element, text, HtmlRewriter, MemorySettings, Settings};
use reqwest::{Client, Url};
use serde::Deserialize;

use super::SearchResult;

const DDG_ENDPOINT: &str = "https://lite.duckduckgo.com/lite/";
const SEARCH_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_QUERY_CHARS: usize = 2_000;
const MAX_RESULTS: usize = 10;
const MAX_RESPONSE_BYTES: usize = 1_048_576;
const MAX_URL_BYTES: usize = 8_192;
const MAX_CAPTURE_BYTES: usize = 16_384;
const MAX_CANDIDATES: usize = 100;
const MAX_TITLE_CHARS: usize = 512;
const MAX_SNIPPET_CHARS: usize = 2_048;

/// Search DuckDuckGo Lite, or only the explicitly configured SearXNG endpoint.
/// Dropping this future cancels its HTTP request; no subprocess or detached task is used.
pub(super) async fn search(
    query: &str,
    limit: usize,
    endpoint: Option<&str>,
) -> Result<Vec<SearchResult>> {
    if query.trim().is_empty() || query.chars().count() > MAX_QUERY_CHARS {
        return Err(eyre!(
            "Local search query must contain 1 to 2000 characters"
        ));
    }
    if !(1..=MAX_RESULTS).contains(&limit) {
        return Err(eyre!("Local search result limit must be between 1 and 10"));
    }

    let searxng = endpoint.is_some();
    let mut url = search_endpoint(endpoint.unwrap_or(DDG_ENDPOINT))?;
    // Replace these parameters rather than allowing a configured query to override them.
    let retained: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(key, _)| key != "q" && key != "format")
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    url.set_query(None);
    url.query_pairs_mut()
        .extend_pairs(retained)
        .append_pair("q", query);
    if searxng {
        url.query_pairs_mut().append_pair("format", "json");
    }

    let client = Client::builder()
        .user_agent("tcui/0.8 (terminal web search)")
        .connect_timeout(Duration::from_secs(5))
        .timeout(SEARCH_TIMEOUT)
        // A configured endpoint must not redirect queries to another service.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| eyre!("Could not initialize local search HTTP client"))?;
    let response = client
        .get(url)
        .header(
            reqwest::header::ACCEPT,
            if searxng {
                "application/json"
            } else {
                "text/html"
            },
        )
        .send()
        .await
        .map_err(http_error)?;
    let status = response.status();
    if status.is_redirection() {
        return Err(eyre!(
            "Local search endpoint redirected the request; configure its final search URL"
        ));
    }
    if !status.is_success() {
        return Err(eyre!(
            "Local search returned HTTP {}; check endpoint access, bot protection, and SearXNG JSON support",
            status.as_u16()
        ));
    }
    let body = read_response(response).await?;
    if searxng {
        parse_searxng(&body, limit)
    } else {
        parse_ddg(&body, limit)
    }
}

fn search_endpoint(endpoint: &str) -> Result<Url> {
    if endpoint.len() > MAX_URL_BYTES {
        return Err(eyre!("Local search endpoint URL is too long"));
    }
    let url = Url::parse(endpoint).map_err(|_| eyre!("Invalid local search endpoint URL"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || endpoint.chars().any(char::is_control)
    {
        return Err(eyre!(
            "Local search endpoint must be an HTTP(S) URL without credentials or a fragment"
        ));
    }
    Ok(url)
}

fn http_error(error: reqwest::Error) -> color_eyre::Report {
    // reqwest errors can contain the complete URL, including private search terms.
    if error.is_timeout() {
        eyre!("Local web search timed out after 15s")
    } else {
        eyre!(
            "Local web search HTTP request failed; check network access and endpoint configuration"
        )
    }
}

async fn read_response(mut response: reqwest::Response) -> Result<String> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(eyre!("Local search response exceeded the 1 MiB limit"));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(http_error)? {
        if chunk.len() > MAX_RESPONSE_BYTES - body.len() {
            return Err(eyre!("Local search response exceeded the 1 MiB limit"));
        }
        body.extend_from_slice(&chunk);
    }
    String::from_utf8(body).map_err(|_| eyre!("Local search returned invalid UTF-8"))
}

#[derive(Default)]
struct DdgResult {
    title: String,
    href: String,
    snippet: String,
}

fn parse_ddg(html: &str, limit: usize) -> Result<Vec<SearchResult>> {
    let candidates = RefCell::new(Vec::<DdgResult>::new());
    let current = Cell::new(None);
    let challenge = Cell::new(false);
    let no_results = Cell::new(false);
    {
        // Lite uses a title row followed by a separate snippet row. Retain even invalid
        // candidates until both rows are paired, so snippets never shift to another URL.
        let settings = Settings::new()
            .with_memory_settings(
                MemorySettings::new().with_max_allowed_memory_usage(MAX_RESPONSE_BYTES),
            )
            .append_element_content_handler(element!("a.result-link", |element| {
                let mut candidates = candidates.borrow_mut();
                if candidates.len() < MAX_CANDIDATES {
                    current.set(Some(candidates.len()));
                    candidates.push(DdgResult {
                        href: element.get_attribute("href").unwrap_or_default(),
                        ..DdgResult::default()
                    });
                } else {
                    current.set(None);
                }
                Ok(())
            }))
            .append_element_content_handler(text!("a.result-link", |chunk| {
                if chunk.text_type().allows_html_entities() {
                    if let Some(index) = current.get() {
                        append_capture(&mut candidates.borrow_mut()[index].title, chunk.as_str());
                    }
                }
                Ok(())
            }))
            .append_element_content_handler(text!("td.result-snippet", |chunk| {
                if chunk.text_type().allows_html_entities() {
                    if let Some(index) = current.get() {
                        append_capture(&mut candidates.borrow_mut()[index].snippet, chunk.as_str());
                    }
                }
                Ok(())
            }))
            .append_element_content_handler(element!(
                "form#challenge-form, .anomaly-modal, #anubis_challenge, #challenge-running",
                |_| {
                    challenge.set(true);
                    Ok(())
                }
            ))
            .append_element_content_handler(element!(".no-results", |_| {
                no_results.set(true);
                Ok(())
            }));
        let mut parser = HtmlRewriter::new(settings, |_: &[u8]| {});
        parser
            .write(html.as_bytes())
            .and_then(|()| parser.end())
            .map_err(|_| eyre!("DuckDuckGo Lite returned malformed or excessively complex HTML"))?;
    }
    if challenge.get() {
        return Err(eyre!(
            "DuckDuckGo Lite returned a bot challenge; configure a self-hosted SearXNG JSON endpoint or select an MCP search provider"
        ));
    }
    let mut results = Vec::new();
    for candidate in candidates.into_inner() {
        let Some(url) = ddg_result_url(&candidate.href) else {
            continue;
        };
        let title = normalize_text(&htmlize::unescape(&candidate.title), MAX_TITLE_CHARS);
        let snippet = normalize_text(&htmlize::unescape(&candidate.snippet), MAX_SNIPPET_CHARS);
        push_result(&mut results, title, url, snippet);
        if results.len() == limit {
            break;
        }
    }
    if results.is_empty() && !no_results.get() {
        return Err(eyre!(
            "DuckDuckGo Lite returned no recognizable results; its page format or bot protection may have changed"
        ));
    }
    Ok(results)
}

fn append_capture(output: &mut String, text: &str) {
    let mut end = text.len().min(MAX_CAPTURE_BYTES - output.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    output.push_str(&text[..end]);
}

fn ddg_result_url(href: &str) -> Option<String> {
    let decoded = htmlize::unescape_attribute(href);
    if decoded.len() > MAX_URL_BYTES || decoded.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return None;
    }
    let parsed = if decoded.starts_with("//") {
        Url::parse(&format!("https:{decoded}")).ok()?
    } else if decoded.starts_with('/') {
        Url::parse(DDG_ENDPOINT).ok()?.join(&decoded).ok()?
    } else {
        Url::parse(&decoded).ok()?
    };
    if !matches!(parsed.scheme(), "http" | "https")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return None;
    }
    if matches!(
        parsed.host_str(),
        Some(
            "duckduckgo.com" | "www.duckduckgo.com" | "lite.duckduckgo.com" | "html.duckduckgo.com"
        )
    ) {
        if parsed.path() != "/l/" && parsed.path() != "/l" {
            return None;
        }
        let (_, target) = parsed.query_pairs().find(|(key, _)| key == "uddg")?;
        // HTML decoding happens before percent decoding, exactly once per layer.
        // Decoding target entities again would corrupt literal `&amp;` in a URL.
        canonical_http_url(&target)
    } else {
        canonical_http_url(parsed.as_str())
    }
}

fn canonical_http_url(value: &str) -> Option<String> {
    if value.len() > MAX_URL_BYTES || value.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return None;
    }
    let url = Url::parse(value).ok()?;
    (matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none())
    .then(|| url.to_string())
}

fn normalize_text(value: &str, max_chars: usize) -> String {
    let mut output = String::new();
    let mut count = 0;
    let mut space = false;
    for character in value.chars() {
        if character.is_whitespace() {
            space = !output.is_empty();
            continue;
        }
        if character.is_control() {
            continue;
        }
        if space {
            if count + 1 >= max_chars {
                break;
            }
            output.push(' ');
            count += 1;
            space = false;
        }
        if count == max_chars {
            break;
        }
        output.push(character);
        count += 1;
    }
    output
}

fn push_result(results: &mut Vec<SearchResult>, title: String, url: String, snippet: String) {
    if !title.is_empty() && !results.iter().any(|result| result.url == url) {
        results.push(SearchResult {
            title,
            url,
            snippet,
        });
    }
}

fn parse_searxng(body: &str, limit: usize) -> Result<Vec<SearchResult>> {
    #[derive(Deserialize)]
    struct Response {
        results: Vec<Item>,
        #[serde(default)]
        unresponsive_engines: Vec<serde_json::Value>,
    }
    #[derive(Deserialize)]
    struct Item {
        title: String,
        url: String,
        #[serde(default)]
        content: Option<String>,
    }

    if body.trim_start().starts_with('<') {
        return Err(eyre!(
            "SearXNG returned HTML instead of JSON; enable json in search.formats and check bot protection"
        ));
    }
    let response: Response = serde_json::from_str(body)
        .map_err(|_| eyre!("SearXNG returned invalid search JSON (expected a results array)"))?;
    if response.results.is_empty() {
        if !response.unresponsive_engines.is_empty() {
            return Err(eyre!(
                "SearXNG returned no results and reported unavailable search engines"
            ));
        }
        return Ok(Vec::new());
    }
    let mut results = Vec::new();
    for item in response.results {
        let Some(url) = canonical_http_url(&item.url) else {
            continue;
        };
        // SearXNG JSON text is already decoded; unlike HTML, do not unescape it again.
        let title = normalize_text(&item.title, MAX_TITLE_CHARS);
        let snippet = normalize_text(
            item.content.as_deref().unwrap_or_default(),
            MAX_SNIPPET_CHARS,
        );
        push_result(&mut results, title, url, snippet);
        if results.len() == limit {
            break;
        }
    }
    if results.is_empty() {
        return Err(eyre!("SearXNG returned no usable HTTP(S) search results"));
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::task::JoinHandle;

    const DDG_FIXTURE: &str = r#"<!doctype html><html><body><table>
        <tr><td><a class='result-link' href='//duckduckgo.com/l/?uddg=https%3A%2F%2Fwww.kernel.org%2Fdoc%2Fhtml%2Flatest%2F%3Fa%3D1%26b%3D2&amp;rut=ignored'>
            The <b>Linux</b> Kernel &amp; docs &#x1f427;
        </a></td></tr>
        <tr><td class='result-snippet'>Kernel&#x27;s <b>documentation</b>&nbsp; &copy; 2026.</td></tr>
        <tr><td><a class='result-link' href='https://docs.rs/reqwest/latest/reqwest/'>reqwest</a></td></tr>
        <tr><td class='result-snippet'>HTTP client documentation.</td></tr>
        </table></body></html>"#;

    #[test]
    fn lite_results_decode_redirects_markup_and_entities() {
        let results = parse_ddg(DDG_FIXTURE, 5).expect("valid Lite page");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "The Linux Kernel & docs 🐧");
        assert_eq!(
            results[0].url,
            "https://www.kernel.org/doc/html/latest/?a=1&b=2"
        );
        assert_eq!(results[0].snippet, "Kernel's documentation © 2026.");
        assert_eq!(results[1].title, "reqwest");
        assert_eq!(results[1].snippet, "HTTP client documentation.");
    }

    #[test]
    fn lite_result_limit_is_enforced() {
        assert_eq!(parse_ddg(DDG_FIXTURE, 1).expect("Lite page").len(), 1);
    }

    #[test]
    fn lite_zero_results_are_not_a_parse_error() {
        assert!(
            parse_ddg("<div class='no-results'>No results found</div>", 5)
                .expect("real zero results")
                .is_empty()
        );
    }

    #[test]
    fn lite_bot_challenges_are_not_zero_results() {
        let page = "<form id='challenge-form'><div class='anomaly-modal'>Verify</div></form>";
        assert!(parse_ddg(page, 5)
            .unwrap_err()
            .to_string()
            .contains("bot challenge"));
    }

    #[test]
    fn lite_unknown_or_malformed_pages_fail_without_html_leakage() {
        for page in [
            "<html><body>private error details</body></html>",
            "<a href=\"",
        ] {
            let error = parse_ddg(page, 5).unwrap_err().to_string();
            assert!(
                error.contains("no recognizable results")
                    || error.contains("malformed or excessively complex HTML")
            );
            assert!(!error.contains("private error details"));
        }
    }

    #[test]
    fn invalid_lite_urls_do_not_shift_snippets_to_other_results() {
        let page = r#"<a class='result-link' href='javascript:alert(1)'>Bad</a>
            <table><tr><td class='result-snippet'>Wrong snippet</td></tr></table>
            <a class='result-link' href='https://kernel.org/'>Kernel</a>
            <table><tr><td class='result-snippet'>Correct snippet</td></tr></table>"#;
        let results = parse_ddg(page, 5).expect("one usable result");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].snippet, "Correct snippet");
    }

    #[test]
    fn redirect_targets_are_not_html_decoded_twice() {
        assert_eq!(
            ddg_result_url("/l/?uddg=https%3A%2F%2Fexample.com%2F%3Fx%3D%26amp%3B&amp;rut=ignored"),
            Some("https://example.com/?x=&amp;".to_string())
        );
    }

    #[test]
    fn direct_protocol_relative_result_urls_are_normalized() {
        assert_eq!(
            ddg_result_url("//docs.rs/reqwest/latest/?a=1&amp;b=2"),
            Some("https://docs.rs/reqwest/latest/?a=1&b=2".to_string())
        );
    }

    #[test]
    fn urls_reject_executable_schemes_credentials_and_controls() {
        for url in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "https://user:secret@example.com/",
            "https://example.com/\nsecret",
            "https://example.com/a b",
        ] {
            assert!(canonical_http_url(url).is_none(), "accepted {url:?}");
        }
    }

    #[test]
    fn searxng_parses_and_bounds_plain_json_text_without_double_decoding() {
        let json = serde_json::json!({"results": [
            {"title": " Linux &amp; docs ", "url": "https://kernel.org", "content": "one\n two\u{1b}"},
            {"title": "duplicate", "url": "https://kernel.org/"},
            {"title": "unsafe", "url": "file:///etc/passwd"},
            {"title": "x".repeat(1000), "url": "https://docs.rs/", "content": "🐧".repeat(3000)}
        ]});
        let results = parse_searxng(&json.to_string(), 5).expect("SearXNG JSON");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Linux &amp; docs");
        assert_eq!(results[0].snippet, "one two");
        assert_eq!(results[1].title.chars().count(), MAX_TITLE_CHARS);
        assert_eq!(results[1].snippet.chars().count(), MAX_SNIPPET_CHARS);
    }

    #[test]
    fn searxng_distinguishes_empty_results_from_unavailable_engines() {
        assert!(parse_searxng(r#"{"results":[]}"#, 5)
            .expect("empty JSON")
            .is_empty());
        let error = parse_searxng(
            r#"{"results":[],"unresponsive_engines":[["google","CAPTCHA"]]}"#,
            5,
        )
        .unwrap_err();
        assert!(error.to_string().contains("unavailable search engines"));
    }

    #[test]
    fn searxng_rejects_malformed_json_and_html_bot_pages() {
        for body in [
            "not JSON: secret details",
            r#"{"results":"wrong shape"}"#,
            "{}",
        ] {
            let error = parse_searxng(body, 5).unwrap_err().to_string();
            assert!(error.contains("invalid search JSON"));
            assert!(!error.contains("secret details"));
        }
        let error = parse_searxng("<html>Complete the bot challenge</html>", 5).unwrap_err();
        assert!(error.to_string().contains("HTML instead of JSON"));
    }

    async fn http_fixture(response: String) -> (String, JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fixture");
        let endpoint = format!(
            "http://{}/search",
            listener.local_addr().expect("fixture address")
        );
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept request");
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let count = stream.read(&mut buffer).await.expect("read request");
                assert!(count > 0 && request.len() + count <= 32_768);
                request.extend_from_slice(&buffer[..count]);
            }
            // Oversized-response tests may close the connection before the body is written.
            let _ = stream.write_all(response.as_bytes()).await;
            let _ = stream.shutdown().await;
            String::from_utf8(request).expect("ASCII HTTP request")
        });
        (endpoint, server)
    }

    fn json_response(body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    #[tokio::test]
    async fn configured_searxng_endpoint_receives_exact_query_as_data() {
        let body = r#"{"results":[{"title":"Linux","url":"https://kernel.org/","content":"Kernel docs"}]}"#;
        let (endpoint, server) = http_fixture(json_response(body)).await;
        let query = "--num 99 Linux &format=html \"docs\" 🐧";
        let configured = format!("{endpoint}?language=en&q=ignored&format=html");
        let results = search(query, 1, Some(&configured))
            .await
            .expect("fixture search");
        let request = server.await.expect("fixture task");
        let target = request
            .lines()
            .next()
            .expect("request line")
            .split_whitespace()
            .nth(1)
            .expect("target");
        let requested = Url::parse(&format!("http://fixture{target}")).expect("request URL");
        let pairs: Vec<_> = requested.query_pairs().collect();
        assert_eq!(pairs.iter().filter(|(key, _)| key == "q").count(), 1);
        assert!(pairs
            .iter()
            .any(|(key, value)| key == "q" && value == query));
        assert!(pairs
            .iter()
            .any(|(key, value)| key == "format" && value == "json"));
        assert!(pairs
            .iter()
            .any(|(key, value)| key == "language" && value == "en"));
        assert_eq!(results[0].snippet, "Kernel docs");
    }

    #[tokio::test]
    async fn configured_endpoint_http_error_does_not_fall_back() {
        let (endpoint, server) = http_fixture(
            "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string(),
        )
        .await;
        let error = search("Linux", 5, Some(&endpoint)).await.unwrap_err();
        server.await.expect("fixture task");
        assert!(error.to_string().contains("HTTP 403"));
    }

    #[tokio::test]
    async fn configured_endpoint_redirect_is_not_followed() {
        let (endpoint, server) = http_fixture(
            "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:9/other-provider\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string(),
        ).await;
        let error = search("Linux", 5, Some(&endpoint)).await.unwrap_err();
        server.await.expect("fixture task");
        assert!(error.to_string().contains("redirected"));
    }

    #[tokio::test]
    async fn oversized_content_length_fails_before_reading_body() {
        let (endpoint, server) = http_fixture(format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            MAX_RESPONSE_BYTES + 1
        ))
        .await;
        let error = search("Linux", 5, Some(&endpoint)).await.unwrap_err();
        server.await.expect("fixture task");
        assert!(error.to_string().contains("1 MiB"));
    }

    #[tokio::test]
    async fn oversized_chunked_response_is_bounded_without_content_length() {
        let body = "x".repeat(MAX_RESPONSE_BYTES + 1);
        let (endpoint, server) = http_fixture(format!(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n{body}\r\n0\r\n\r\n",
            body.len()
        )).await;
        let error = search("Linux", 5, Some(&endpoint)).await.unwrap_err();
        server.await.expect("fixture task");
        assert!(error.to_string().contains("1 MiB"));
    }

    #[tokio::test]
    async fn cancelling_search_closes_its_pending_http_request() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fixture");
        let endpoint = format!(
            "http://{}/search",
            listener.local_addr().expect("fixture address")
        );
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept request");
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let count = stream.read(&mut buffer).await.expect("read request");
                assert!(count > 0 && request.len() + count <= 32_768);
                request.extend_from_slice(&buffer[..count]);
            }
            started_tx.send(()).expect("search still running");
            stream.read(&mut buffer).await.expect("read cancellation")
        });
        let request = tokio::spawn(async move { search("Linux", 5, Some(&endpoint)).await });
        tokio::time::timeout(Duration::from_secs(2), started_rx)
            .await
            .expect("request started")
            .expect("fixture signaled");
        request.abort();
        assert!(request.await.expect_err("search cancelled").is_cancelled());
        let remaining = tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .expect("connection closed promptly")
            .expect("fixture task");
        assert_eq!(remaining, 0);
    }

    #[tokio::test]
    async fn invalid_queries_limits_and_endpoints_fail_before_network() {
        for (query, limit, endpoint) in [
            (" ", 5, "http://127.0.0.1:9/search"),
            ("Linux", 0, "http://127.0.0.1:9/search"),
            ("Linux", 11, "http://127.0.0.1:9/search"),
            ("Linux", 5, "file:///etc/passwd"),
            ("Linux", 5, "https://user:secret@example.com/search"),
        ] {
            assert!(search(query, limit, Some(endpoint)).await.is_err());
        }
        let error = search(&"🐧".repeat(MAX_QUERY_CHARS + 1), 5, None)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("2000 characters"));
    }
}
