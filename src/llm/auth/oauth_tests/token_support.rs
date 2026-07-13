use std::error::Error;
use std::io;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::llm::auth::oauth::TokenEndpoint;

pub(super) async fn token_fixture(
    status: &str,
    body: String,
) -> Result<
    (
        TokenEndpoint,
        oneshot::Receiver<String>,
        JoinHandle<io::Result<()>>,
    ),
    Box<dyn Error>,
> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = TokenEndpoint::parse(&format!("http://{}", listener.local_addr()?))?;
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let (captured_request, request_receiver) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await?;
        let request = read_http_request(&mut stream).await?;
        let _ = captured_request.send(request);
        stream.write_all(response.as_bytes()).await?;
        stream.shutdown().await
    });
    Ok((endpoint, request_receiver, server))
}

pub(super) async fn token_sequence_fixture(
    responses: Vec<(&str, String)>,
) -> Result<(TokenEndpoint, JoinHandle<io::Result<()>>), Box<dyn Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = TokenEndpoint::parse(&format!("http://{}", listener.local_addr()?))?;
    let responses = responses
        .into_iter()
        .map(|(status, body)| {
            format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
        })
        .collect::<Vec<_>>();
    let server = tokio::spawn(async move {
        for response in responses {
            let (mut stream, _) = listener.accept().await?;
            let _ = read_http_request(&mut stream).await?;
            stream.write_all(response.as_bytes()).await?;
            stream.shutdown().await?;
        }
        Ok(())
    });
    Ok((endpoint, server))
}

async fn read_http_request(stream: &mut tokio::net::TcpStream) -> io::Result<String> {
    let mut bytes = Vec::with_capacity(1_024);
    let mut buffer = [0_u8; 1_024];
    let header_end = loop {
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "token request ended before headers",
            ));
        }
        bytes.extend_from_slice(&buffer[..read]);
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
    };
    let header = std::str::from_utf8(&bytes[..header_end])
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid HTTP header"))?;
    let content_length = header
        .lines()
        .find_map(|line| line.strip_prefix("content-length:"))
        .or_else(|| {
            header
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length:"))
        })
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing content length"))?
        .trim()
        .parse::<usize>()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid content length"))?;
    while bytes.len() < header_end + content_length {
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "token request ended before body",
            ));
        }
        bytes.extend_from_slice(&buffer[..read]);
    }
    String::from_utf8(bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid UTF-8"))
}
