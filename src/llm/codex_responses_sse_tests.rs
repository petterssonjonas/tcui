use std::io;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

use super::*;

#[test]
fn responses_events_preserve_answer_reasoning_title_and_usage() -> color_eyre::Result<()> {
    let mut filter = TitleTagFilter::default();

    let first = codex_stream_event(
        r#"{"type":"response.output_text.delta","delta":"Visible <tcui:chat-title>Secret"}"#,
        &mut filter,
    )?;
    let reasoning = codex_stream_event(
        r#"{"type":"response.reasoning_summary_text.delta","delta":"Working"}"#,
        &mut filter,
    )?;
    let second = codex_stream_event(
        r#"{"type":"response.output_text.delta","delta":"</tcui:chat-title> answer"}"#,
        &mut filter,
    )?;
    let completed = codex_stream_event(
        r#"{"type":"response.completed","response":{"id":"resp-1","usage":{"total_tokens":42}}}"#,
        &mut filter,
    )?;

    assert_eq!(
        first.events,
        vec![ChatStreamEvent::Answer("Visible ".to_string())]
    );
    assert_eq!(
        reasoning.events,
        vec![ChatStreamEvent::Thinking("Working".to_string())]
    );
    assert_eq!(
        second.events,
        vec![
            ChatStreamEvent::Title("Secret".to_string()),
            ChatStreamEvent::Answer(" answer".to_string())
        ]
    );
    assert_eq!(completed.total_tokens, Some(42));
    assert!(completed.completed);
    Ok(())
}

#[test]
fn malformed_responses_sse_is_rejected() {
    let mut filter = TitleTagFilter::default();

    assert!(codex_stream_event("{not-json", &mut filter).is_err());
}

#[test]
fn response_error_message_extracts_only_the_provider_message() {
    let message = sse::response_error_message(
        r#"{"error":{"message":"access_token=eyJ.invalid"},"unexpected":"ignored"}"#,
    );

    assert_eq!(message.as_deref(), Some("access_token=eyJ.invalid"));
}

#[test]
fn responses_stream_errors_redact_oauth_tokens() {
    let mut filter = TitleTagFilter::default();
    let error = codex_stream_event(
        r#"{"type":"response.failed","response":{"error":{"message":"access_token=eyJ.secret-value"}}}"#,
        &mut filter,
    )
    .expect_err("failed response must surface an error");

    assert!(!error.to_string().contains("eyJ.secret-value"));
}

#[tokio::test]
async fn bounded_sse_rejects_total_and_per_event_limits() -> Result<(), Box<dyn std::error::Error>>
{
    let total_response = response_with("200 OK", b"data: a\n\ndata: b\n\n").await?;
    let total_error = transport::read_sse(
        total_response,
        limits(16, 64, 64, Duration::from_secs(1)),
        |_| Ok(true),
    )
    .await
    .expect_err("the aggregate stream limit must be enforced");
    assert!(matches!(
        total_error,
        transport::CodexTransportError::SseTotalTooLarge
    ));

    let event_response = response_with("200 OK", b"data: oversized\n\n").await?;
    let event_error = transport::read_sse(
        event_response,
        limits(128, 8, 64, Duration::from_secs(1)),
        |_| Ok(true),
    )
    .await
    .expect_err("an individual event limit must be enforced");
    assert!(matches!(
        event_error,
        transport::CodexTransportError::SseEventTooLarge
    ));
    Ok(())
}

#[tokio::test]
async fn bounded_sse_rejects_invalid_utf8_and_unterminated_events(
) -> Result<(), Box<dyn std::error::Error>> {
    let invalid_utf8_response = response_with("200 OK", b"data: \xff\n\n").await?;
    let invalid_utf8_error = transport::read_sse(
        invalid_utf8_response,
        limits(128, 128, 64, Duration::from_secs(1)),
        |_| Ok(true),
    )
    .await
    .expect_err("SSE data must be UTF-8");
    assert!(matches!(
        invalid_utf8_error,
        transport::CodexTransportError::InvalidSseUtf8
    ));

    let unterminated_response = response_with("200 OK", b"data: incomplete").await?;
    let unterminated_error = transport::read_sse(
        unterminated_response,
        limits(128, 128, 64, Duration::from_secs(1)),
        |_| Ok(true),
    )
    .await
    .expect_err("a trailing event without a blank line must be rejected");
    assert!(matches!(
        unterminated_error,
        transport::CodexTransportError::UnterminatedSse
    ));
    Ok(())
}

#[tokio::test]
async fn bounded_sse_enforces_its_absolute_stream_deadline(
) -> Result<(), Box<dyn std::error::Error>> {
    let (response, server) = stalled_response().await?;
    let error = transport::read_sse(
        response,
        limits(128, 128, 64, Duration::from_millis(20)),
        |_| Ok(true),
    )
    .await
    .expect_err("a stalled SSE stream must reach its deadline");
    server.abort();
    let _ = server.await;

    assert!(matches!(
        error,
        transport::CodexTransportError::StreamDeadline
    ));
    Ok(())
}

#[tokio::test]
async fn bounded_http_errors_are_typed_redacted_and_limited(
) -> Result<(), Box<dyn std::error::Error>> {
    let oversized_response = response_with("400 Bad Request", &[b'x'; 65]).await?;
    let oversized_error = transport::http_error(
        oversized_response,
        "Codex",
        limits(128, 128, 64, Duration::from_secs(1)),
    )
    .await;
    assert!(matches!(
        oversized_error,
        transport::CodexTransportError::ErrorBodyTooLarge
    ));

    const CANARY: &str = "eyJ.tcui-secret-canary";
    for (status, expected) in [
        ("400 Bad Request", transport::CodexHttpStatus::BadRequest),
        ("403 Forbidden", transport::CodexHttpStatus::Forbidden),
        (
            "429 Too Many Requests",
            transport::CodexHttpStatus::RateLimited,
        ),
        (
            "500 Internal Server Error",
            transport::CodexHttpStatus::InternalServerError,
        ),
    ] {
        let body = format!(r#"{{"error":{{"message":"access_token={CANARY}"}}}}"#);
        let response = response_with(status, body.as_bytes()).await?;
        let error = transport::http_error(
            response,
            "Codex",
            limits(256, 256, 256, Duration::from_secs(1)),
        )
        .await;
        match error {
            transport::CodexTransportError::Http { status, message } => {
                assert_eq!(status, expected);
                assert!(!message.contains(CANARY));
            }
            other => panic!("expected a typed HTTP error, received {other}"),
        }
    }
    Ok(())
}

fn limits(
    max_stream_bytes: usize,
    max_event_bytes: usize,
    max_error_body_bytes: usize,
    deadline: Duration,
) -> transport::TransportLimits {
    transport::TransportLimits {
        max_stream_bytes,
        max_event_bytes,
        max_error_body_bytes,
        deadline,
    }
}

async fn response_with(
    status: &str,
    body: &[u8],
) -> Result<reqwest::Response, Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}", listener.local_addr()?);
    let status = status.to_string();
    let body = body.to_vec();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await?;
        let headers = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        socket.write_all(headers.as_bytes()).await?;
        socket.write_all(&body).await?;
        socket.shutdown().await
    });
    let response = reqwest::Client::new().get(endpoint).send().await?;
    server.await??;
    Ok(response)
}

async fn stalled_response(
) -> Result<(reqwest::Response, tokio::task::JoinHandle<io::Result<()>>), Box<dyn std::error::Error>>
{
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await?;
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: keep-alive\r\n\r\n",
            )
            .await?;
        std::future::pending::<io::Result<()>>().await
    });
    let response = reqwest::Client::new().get(endpoint).send().await?;
    Ok((response, server))
}
