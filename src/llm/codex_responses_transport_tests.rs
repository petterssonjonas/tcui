use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::*;
use crate::app::message::Message;

#[tokio::test]
async fn transport_retries_once_with_account_scoped_responses_headers(
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move {
        let mut requests = Vec::new();
        for (status, body) in [
            (
                "401 Unauthorized",
                r#"{"error":{"message":"expired"}}"#.to_string(),
            ),
            ("200 OK", completed_stream("answer")),
        ] {
            let (mut socket, _) = listener.accept().await?;
            requests.push(read_http_request(&mut socket).await?);
            write_response(&mut socket, status, &body).await?;
        }
        Ok::<_, io::Error>(requests)
    });
    let refreshes = Arc::new(AtomicUsize::new(0));
    let refreshed = session("new-token");
    let refresh_count = Arc::clone(&refreshes);
    let mut events = Vec::new();

    let output = stream_with_one_refresh(
        &reqwest::Client::new(),
        &request(endpoint),
        session("old-token"),
        move || {
            refresh_count.fetch_add(1, Ordering::SeqCst);
            async move { Ok(refreshed) }
        },
        &mut |event| events.push(event),
    )
    .await?;
    let requests = server.await??;

    assert_eq!(output.answer, "answer");
    assert_eq!(refreshes.load(Ordering::SeqCst), 1);
    assert_eq!(requests.len(), 2);
    let first = requests[0].to_ascii_lowercase();
    assert!(first.starts_with("post /responses http/1.1"));
    assert!(first.contains("chatgpt-account-id: account-123"));
    assert!(first.contains("originator: codex_cli_rs"));
    let user_agent = request::codex_user_agent()?.to_str()?.to_ascii_lowercase();
    assert!(first.contains(&format!("user-agent: {user_agent}")));
    assert!(first.contains("session-id: tcui-conversation-0000000000000001"));
    assert!(first.contains("thread-id: tcui-conversation-0000000000000001"));
    assert!(first.contains("x-client-request-id: tcui-conversation-0000000000000001"));
    assert!(first.contains("authorization: bearer old-token"));
    assert!(first.contains("content-type: application/json"));
    assert!(first.contains("accept: text/event-stream"));
    assert!(!first.contains("version:"));
    assert!(!first.contains("openai-beta:"));
    let body: serde_json::Value = serde_json::from_str(
        requests[0]
            .split_once("\r\n\r\n")
            .ok_or("missing request body")?
            .1,
    )?;
    assert_eq!(
        body["client_metadata"],
        serde_json::json!({
            "session_id": "tcui-conversation-0000000000000001",
            "thread_id": "tcui-conversation-0000000000000001"
        })
    );
    assert_eq!(events, vec![ChatStreamEvent::Answer("answer".to_string())]);
    Ok(())
}

#[tokio::test]
async fn transport_stops_after_a_second_unauthorized_response(
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move {
        for _ in 0..2 {
            let (mut socket, _) = listener.accept().await?;
            let _ = read_http_request(&mut socket).await?;
            write_response(
                &mut socket,
                "401 Unauthorized",
                r#"{"error":{"message":"expired"}}"#,
            )
            .await?;
        }
        Ok::<_, io::Error>(())
    });
    let refreshes = Arc::new(AtomicUsize::new(0));
    let refresh_count = Arc::clone(&refreshes);

    let error = stream_with_one_refresh(
        &reqwest::Client::new(),
        &request(endpoint),
        session("old-token"),
        move || {
            refresh_count.fetch_add(1, Ordering::SeqCst);
            async move { Ok(session("new-token")) }
        },
        &mut |_| {},
    )
    .await
    .expect_err("second 401 must stop the transport");
    server.await??;

    assert_eq!(refreshes.load(Ordering::SeqCst), 1);
    assert!(error.to_string().contains("expired"));
    Ok(())
}

#[tokio::test]
async fn dropping_the_transport_future_cancels_a_stalled_stream(
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await?;
        let _ = read_http_request(&mut socket).await?;
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: keep-alive\r\n\r\n")
            .await?;
        std::future::pending::<io::Result<()>>().await
    });

    let result = tokio::time::timeout(
        Duration::from_millis(50),
        stream_with_one_refresh(
            &reqwest::Client::new(),
            &request(endpoint),
            session("access-token"),
            || async { Ok(session("access-token")) },
            &mut |_| {},
        ),
    )
    .await;
    server.abort();
    let _ = server.await;

    assert!(result.is_err());
    Ok(())
}

#[tokio::test]
async fn unsupported_reasoning_effort_is_rejected_before_network_connection(
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}", listener.local_addr()?);
    let mut request = request(endpoint);
    request.reasoning_effort = Some("none".to_string());
    request.supported_reasoning_efforts =
        vec!["low".to_string(), "medium".to_string(), "high".to_string()];

    let result = send_request(&reqwest::Client::new(), &request, &session("access-token")).await;
    let connection = tokio::time::timeout(Duration::from_millis(50), listener.accept()).await;

    let Err(error) = result else {
        return Err("unsupported effort reached the network".into());
    };
    assert!(error
        .to_string()
        .contains("Supported efforts: low, medium, high"));
    assert!(
        connection.is_err(),
        "the request opened a network connection"
    );
    Ok(())
}

fn request(endpoint: String) -> ChatRequest {
    ChatRequest {
        provider: "Codex".to_string(),
        endpoint,
        model: "gpt-5.6".to_string(),
        reasoning_effort: None,
        supported_reasoning_efforts: Vec::new(),
        backend_type: "codex".to_string(),
        api_key: None,
        system_prompt: String::new(),
        messages: vec![Message::new(1, "user".to_string(), "hello".to_string())],
    }
}

fn session(access_token: &str) -> CodexSession {
    CodexSession {
        access_token: access_token.to_string(),
        account_id: "account-123".to_string(),
        source: CodexCredentialSource::ExternalCli,
    }
}

fn completed_stream(answer: &str) -> String {
    format!(
        "data: {{\"type\":\"response.output_text.delta\",\"delta\":\"{answer}\"}}\n\ndata: {{\"type\":\"response.completed\",\"response\":{{\"id\":\"resp-1\",\"usage\":{{\"total_tokens\":42}}}}}}\n\n"
    )
}

async fn read_http_request(socket: &mut tokio::net::TcpStream) -> io::Result<String> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 1024];
    loop {
        let read = socket.read(&mut buffer).await?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "missing headers",
            ));
        }
        bytes.extend_from_slice(&buffer[..read]);
        if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            return String::from_utf8(bytes)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid request"));
        }
    }
}

async fn write_response(
    socket: &mut tokio::net::TcpStream,
    status: &str,
    body: &str,
) -> io::Result<()> {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    socket.write_all(response.as_bytes()).await?;
    socket.shutdown().await
}
