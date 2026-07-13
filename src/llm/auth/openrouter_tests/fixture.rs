use std::io;
use std::time::Duration;

use reqwest::Url;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::llm::auth::openrouter::{OpenRouterAdapter, OpenRouterTestEndpoints};

pub(super) struct Fixture {
    pub(super) adapter: OpenRouterAdapter,
    requests: oneshot::Receiver<Vec<String>>,
    server: JoinHandle<io::Result<()>>,
}

pub(super) async fn fixture(
    responses: Vec<String>,
    timeout: Duration,
) -> Result<Fixture, Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", listener.local_addr()?);
    let adapter = OpenRouterAdapter::for_test(
        OpenRouterTestEndpoints::new(
            Url::parse("https://authorization.example/auth")?,
            Url::parse(&format!("{base}/auth/keys/code"))?,
            Url::parse(&format!("{base}/auth/keys"))?,
        ),
        timeout,
    )?;
    let (sender, requests) = oneshot::channel();
    let server = tokio::spawn(async move {
        let mut captured = Vec::with_capacity(responses.len());
        for response in responses {
            let (mut stream, _) = listener.accept().await?;
            captured.push(read_http_request(&mut stream).await?);
            stream.write_all(response.as_bytes()).await?;
            stream.shutdown().await?;
        }
        let _ = sender.send(captured);
        Ok(())
    });
    Ok(Fixture {
        adapter,
        requests,
        server,
    })
}

impl Fixture {
    pub(super) async fn finish(self) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let requests = self.requests.await?;
        self.server.await??;
        Ok(requests)
    }
}

pub(super) fn response(status: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

pub(super) async fn delayed_fixture(
    timeout: Duration,
    delay: Duration,
) -> Result<(OpenRouterAdapter, JoinHandle<io::Result<()>>), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", listener.local_addr()?);
    let adapter = OpenRouterAdapter::for_test(
        OpenRouterTestEndpoints::new(
            Url::parse("https://authorization.example/auth")?,
            Url::parse(&format!("{base}/auth/keys/code"))?,
            Url::parse(&format!("{base}/auth/keys"))?,
        ),
        timeout,
    )?;
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await?;
        let _ = read_http_request(&mut stream).await?;
        tokio::time::sleep(delay).await;
        let _ = stream
            .write_all(response("200 OK", r#"{"key":"late-key"}"#).as_bytes())
            .await;
        Ok(())
    });
    Ok((adapter, server))
}

pub(super) fn body(request: &str) -> &str {
    request.split_once("\r\n\r\n").map_or("", |(_, body)| body)
}

async fn read_http_request(stream: &mut tokio::net::TcpStream) -> io::Result<String> {
    let mut bytes = Vec::with_capacity(1_024);
    let mut buffer = [0_u8; 1_024];
    let header_end = loop {
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "missing request header",
            ));
        }
        bytes.extend_from_slice(&buffer[..read]);
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
    };
    let header = std::str::from_utf8(&bytes[..header_end])
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid request header"))?;
    let content_length = header
        .lines()
        .find_map(|line| {
            line.split_once(':')
                .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        })
        .map(|(_, value)| value.trim())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing content length"))?
        .parse::<usize>()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid content length"))?;
    while bytes.len() < header_end + content_length {
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "missing request body",
            ));
        }
        bytes.extend_from_slice(&buffer[..read]);
    }
    String::from_utf8(bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid request encoding"))
}
