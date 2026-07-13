use super::TuiApp;
use crate::llm::auth::{Credential, CredentialError};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

#[tokio::test]
async fn cloud_probe_sends_a_bounded_authenticated_openrouter_models_request() {
    // Given
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind probe fixture");
    let endpoint = format!("http://{}", listener.local_addr().expect("fixture address"));
    let (request_tx, request_rx) = oneshot::channel();
    let fixture = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept probe request");
        let mut request = [0_u8; 2048];
        let bytes = stream.read(&mut request).await.expect("read probe request");
        request_tx
            .send(String::from_utf8_lossy(&request[..bytes]).to_string())
            .expect("capture probe request");
        stream
            .write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await
            .expect("write probe response");
    });
    let credential = Credential::api_key_for_test("OpenRouter", "probe-token");

    // When
    let result =
        TuiApp::probe_cloud_connection_for_test("OpenRouter", &endpoint, &credential).await;

    // Then
    assert!(result.is_ok());
    let request = request_rx.await.expect("receive probe request");
    assert!(request.starts_with("GET /models HTTP/1.1"));
    assert!(request.contains("authorization: Bearer probe-token"));
    assert!(request.contains("http-referer: https://github.com/jp/TermChatUI"));
    fixture.await.expect("finish probe fixture");
}

#[tokio::test]
async fn codex_connection_check_treats_a_resolved_credential_as_ready_without_probing()
-> color_eyre::Result<()> {
    // Given
    let credential = Credential::api_key_for_test("Codex", "external-cli-token");

    // When
    let result = TuiApp::check_cloud_connection(
        "Codex",
        "https://chatgpt.com/backend-api/codex",
        Some(&credential),
    )
    .await;

    // Then
    assert!(
        result.is_ok(),
        "resolved Codex credentials should not require a generic models probe: {result:?}"
    );
    Ok(())
}

#[test]
fn codex_connection_check_retries_a_transient_credential_error() {
    // Given
    let error = CredentialError::CodexCredentialUnavailable;

    // When
    let should_retry = TuiApp::should_retry_connection_credential("Codex", &error);

    // Then
    assert!(should_retry);
}
