use std::io;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio::time::timeout;

async fn local_http_fixture() -> io::Result<(String, JoinHandle<io::Result<()>>)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await?;
        let mut request = [0_u8; 1024];
        let received = stream.read(&mut request).await?;
        if received == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "local fixture received no request",
            ));
        }

        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
            .await?;
        stream.shutdown().await
    });
    Ok((endpoint, server))
}

#[tokio::test]
async fn local_reqwest_fixture_round_trips_with_explicit_timeout(
) -> Result<(), Box<dyn std::error::Error>> {
    let (endpoint, server) = local_http_fixture().await?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(500))
        .build()?;

    let body = client
        .get(endpoint)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    server.await??;
    assert_eq!(body, "ok");
    Ok(())
}

#[tokio::test]
async fn timeout_cancels_pending_async_wait() {
    let result = timeout(Duration::from_millis(10), std::future::pending::<()>()).await;

    assert!(result.is_err());
}
