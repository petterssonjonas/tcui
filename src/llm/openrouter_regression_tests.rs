use std::io;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

use crate::app::message::Message;
use crate::llm::chat::{stream_chat, ChatRequest};
use crate::llm::model_fetcher::fetch_models;

#[tokio::test]
async fn openrouter_chat_keeps_existing_attribution_headers(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}", listener.local_addr()?);
    let (sender, receiver) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await?;
        let request = read_http_request(&mut stream).await?;
        let _ = sender.send(request);
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\ndata: [DONE]\n\n";
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .await?;
        stream.shutdown().await
    });
    let request = ChatRequest {
        provider: "OpenRouter".to_owned(),
        endpoint,
        model: "openai/test".to_owned(),
        reasoning_effort: None,
        supported_reasoning_efforts: Vec::new(),
        backend_type: "openrouter".to_owned(),
        api_key: Some("saved-key".to_owned()),
        system_prompt: String::new(),
        messages: vec![Message::new(1, "user".to_owned(), "hello".to_owned())],
    };

    // When
    let output = stream_chat(request, |_| {}).await?;
    let captured = receiver.await?;
    server.await??;

    // Then
    assert_eq!(output.answer, "ok");
    let headers = captured.to_ascii_lowercase();
    assert!(headers.contains("http-referer: https://github.com/jp/termchatui"));
    assert!(headers.contains("x-title: termchatui"));
    Ok(())
}

#[tokio::test]
async fn openrouter_model_fetch_keeps_existing_headers_and_parsing(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}", listener.local_addr()?);
    let (sender, receiver) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await?;
        let request = read_http_request(&mut stream).await?;
        let _ = sender.send(request);
        let body = r#"{"data":[{"id":"openai/test","pricing":{"prompt":"0.1","completion":"0.2"},"context_length":128000}]}"#;
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .await?;
        stream.shutdown().await
    });

    // When
    let credential = crate::llm::auth::Credential::api_key_for_test("OpenRouter", "saved-key");
    let models = fetch_models("OpenRouter", &endpoint, Some(&credential), "openrouter").await;
    let captured = receiver.await?;
    server.await??;

    // Then
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "openai/test");
    assert_eq!(models[0].input_price, Some(100_000.0));
    assert_eq!(models[0].output_price, Some(200_000.0));
    assert_eq!(models[0].context_window, Some(128_000));
    let headers = captured.to_ascii_lowercase();
    assert!(headers.contains("http-referer: https://github.com/jp/termchatui"));
    assert!(headers.contains("x-title: termchatui"));
    Ok(())
}

async fn read_http_request(stream: &mut tokio::net::TcpStream) -> io::Result<String> {
    let mut bytes = Vec::with_capacity(1_024);
    let mut buffer = [0_u8; 1_024];
    loop {
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "missing request header",
            ));
        }
        bytes.extend_from_slice(&buffer[..read]);
        if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            return String::from_utf8(bytes).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "invalid request encoding")
            });
        }
    }
}

#[tokio::test]
async fn model_discovery_never_makes_an_unauthenticated_cloud_request(
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}", listener.local_addr()?);
    assert!(fetch_models("OpenRouter", &endpoint, None, "openrouter")
        .await
        .is_empty());
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(20), listener.accept())
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn anthropic_sdk_model_discovery_uses_console_key_headers(
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await?;
        let captured = read_http_request(&mut stream).await?;
        let body = r#"{"data":[{"id":"claude-sonnet-4-6"}]}"#;
        stream.write_all(format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len(),
        ).as_bytes()).await?;
        Ok::<_, std::io::Error>(captured)
    });
    let credential =
        crate::llm::auth::Credential::api_key_for_test("Anthropic", "saved-console-key");
    let models = fetch_models("Anthropic", &endpoint, Some(&credential), "anthropic_sdk").await;
    assert_eq!(models[0].id, "claude-sonnet-4-6");
    let headers = server.await??.to_ascii_lowercase();
    assert!(headers.contains("x-api-key: saved-console-key"));
    assert!(headers.contains("anthropic-version: 2023-06-01"));
    assert!(!headers.contains("authorization:"));
    Ok(())
}

#[test]
fn cloud_chat_rejects_missing_saved_credentials_before_transport() {
    let request = ChatRequest {
        provider: "OpenRouter".to_owned(),
        endpoint: "https://openrouter.ai/api/v1".to_owned(),
        model: "example/model".to_owned(),
        reasoning_effort: None,
        supported_reasoning_efforts: Vec::new(),
        backend_type: "openrouter".to_owned(),
        api_key: None,
        system_prompt: String::new(),
        messages: vec![Message::new(1, "user".to_owned(), "hello".to_owned())],
    };
    let error = crate::llm::chat::prepare_chat_request(request).unwrap_err();
    assert!(error.to_string().contains("/connect"));
}
