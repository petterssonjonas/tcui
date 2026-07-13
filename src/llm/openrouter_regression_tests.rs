use std::io;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

use crate::app::message::Message;
use crate::llm::chat::{ChatRequest, stream_chat};
use crate::llm::model_fetcher::fetch_models;

#[tokio::test]
async fn openrouter_chat_keeps_existing_attribution_headers()
-> Result<(), Box<dyn std::error::Error>> {
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
        api_key: None,
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
async fn openrouter_model_fetch_keeps_existing_headers_and_parsing()
-> Result<(), Box<dyn std::error::Error>> {
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
    let models = fetch_models("OpenRouter", &endpoint, None, "openrouter").await;
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
