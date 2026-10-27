use std::future::Future;
use std::io::Write;
use std::path::Path;

tokio::task_local! {
    static SENSITIVE_PROVIDER_ROUND: ();
}

/// Omit provider-controlled diagnostic text while polling a private model round.
///
/// This scope stores no credentials and leaves returned errors unchanged for caller
/// redaction. Spawned tasks do not inherit it; wrap their provider future separately.
pub(crate) async fn sensitive_provider_round<T>(future: impl Future<Output = T>) -> T {
    SENSITIVE_PROVIDER_ROUND.scope((), future).await
}

#[cfg(test)]
tokio::task_local! {
    static TEST_DATA_DIR: std::path::PathBuf;
}

pub(crate) fn provider_request(provider: &str, endpoint: &str, model: &str) {
    append(
        "provider.request",
        &format!("{provider} {endpoint} model={model}"),
    );
}

pub(crate) fn provider_response(provider: &str, status: reqwest::StatusCode) {
    append("provider.response", &format!("{provider} status={status}"));
}

pub(crate) fn provider_error(provider: &str, message: &str) {
    let message = if SENSITIVE_PROVIDER_ROUND.try_with(|_| ()).is_ok() {
        "provider-controlled details omitted for sensitive round"
    } else {
        message
    };
    append("provider.error", &format!("{provider} {message}"));
}

pub(crate) fn provider_migration(provider: &str) {
    append(
        "provider.migration",
        &format!("{provider} legacy endpoint and backend migrated"),
    );
}

fn append(kind: &str, message: &str) {
    #[cfg(test)]
    if TEST_DATA_DIR
        .try_with(|path| append_to_data_dir(path, kind, message))
        .is_ok()
    {
        return;
    }

    let Some(path) = dirs::data_dir() else {
        return;
    };
    append_to_data_dir(&path, kind, message);
}

fn append_to_data_dir(data_dir: &Path, kind: &str, message: &str) {
    let mut path = data_dir.to_path_buf();
    path.push("tcui");
    if std::fs::create_dir_all(&path).is_err() {
        return;
    }
    path.push("tcui.log");

    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return;
    };

    let timestamp = chrono::Utc::now().to_rfc3339();
    let safe = crate::llm::auth::redact_secrets(message);
    let _ = writeln!(file, "{timestamp} {kind} {safe}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

    #[tokio::test]
    async fn sensitive_http_error_omits_bare_key_without_suppressing_parallel_diagnostics() {
        const KEY: &str = "0123456789abcdefghijklmnopqrst";
        let root = std::env::temp_dir().join(format!(
            "tcui-sensitive-diagnostics-{:032x}",
            rand::random::<u128>()
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fixture");
        let endpoint = format!("http://{}", listener.local_addr().expect("fixture address"));
        let request = crate::llm::chat::ChatRequest {
            provider: crate::config::LOCAL_PROVIDER_NAME.to_owned(),
            endpoint,
            model: "fixture-model".to_owned(),
            reasoning_effort: None,
            supported_reasoning_efforts: Vec::new(),
            backend_type: "openai".to_owned(),
            api_key: None,
            system_prompt: format!("Private credential observation: user_key={KEY}"),
            messages: Vec::new(),
        };
        let barrier = tokio::sync::Barrier::new(2);
        let round = sensitive_provider_round(async {
            barrier.wait().await;
            crate::llm::chat::stream_chat(request, |_| {}).await
        });
        let normal = async {
            barrier.wait().await;
            provider_error("Normal", "ordinary parallel diagnostic");
        };
        let fixture = async {
            let (socket, _) = listener.accept().await.expect("accept fixture request");
            let mut socket = BufReader::new(socket);
            let mut content_length = 0;
            loop {
                let mut line = String::new();
                let count = socket.read_line(&mut line).await.expect("read header");
                assert_ne!(count, 0, "request ended before headers");
                if line == "\r\n" {
                    break;
                }
                if let Some((name, value)) = line.split_once(':') {
                    if name.eq_ignore_ascii_case("content-length") {
                        content_length = value.trim().parse::<usize>().expect("content length");
                    }
                }
            }
            let mut body = vec![0; content_length];
            socket
                .read_exact(&mut body)
                .await
                .expect("read request body");
            assert!(String::from_utf8(body)
                .expect("request UTF-8")
                .contains(KEY));
            let body = serde_json::json!({"error": {"message": format!("Rejected value {KEY}")}})
                .to_string();
            let response = format!(
                "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            socket
                .get_mut()
                .write_all(response.as_bytes())
                .await
                .expect("write fixture error");
        };
        let (result, (), ()) = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            TEST_DATA_DIR.scope(root.clone(), async { tokio::join!(round, normal, fixture) }),
        )
        .await
        .expect("fixture round should complete");
        // The registry still owns redaction of the returned error, not diagnostics.
        assert!(result
            .expect_err("fixture returns HTTP error")
            .to_string()
            .contains(KEY));
        TEST_DATA_DIR
            .scope(root.clone(), async {
                provider_error("Normal", "ordinary post-scope diagnostic");
            })
            .await;

        let log = std::fs::read_to_string(root.join("tcui").join("tcui.log"))
            .expect("diagnostics test log should exist");
        assert!(!log.contains(KEY), "private HTTP error leaked its bare key");
        assert!(log.contains("provider.response"));
        assert!(log.contains("status=400 Bad Request"));
        assert!(log.contains(crate::config::LOCAL_PROVIDER_NAME));
        assert!(log.contains("provider-controlled details omitted for sensitive round"));
        assert!(log.contains("Normal ordinary parallel diagnostic"));
        assert!(log.contains("Normal ordinary post-scope diagnostic"));
        std::fs::remove_dir_all(root).expect("remove diagnostics fixture");
    }

    #[test]
    fn tcui_log_redacts_oauth_token_canaries() {
        let root = std::env::temp_dir().join(format!("tcui-diagnostics-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);

        append_to_data_dir(
            &root,
            "provider.error",
            "Codex access_token=eyJ.tcui-secret-canary refresh_token=tcui-refresh-secret-canary device_code=dc.tcui-device-canary code=AUTHCODE.tcui-canary state=STATE.tcui-canary code_verifier=VERIFIER.tcui-canary authorization code and state remain prose",
        );

        let log = std::fs::read_to_string(root.join("tcui").join("tcui.log"))
            .expect("diagnostics test log should exist");
        assert!(log.contains("provider.error"));
        let leaked = [
            "eyJ.tcui-secret-canary",
            "tcui-refresh-secret-canary",
            "dc.tcui-device-canary",
            "AUTHCODE.tcui-canary",
            "STATE.tcui-canary",
            "VERIFIER.tcui-canary",
        ]
        .into_iter()
        .filter(|canary| log.contains(canary))
        .collect::<Vec<_>>();
        assert!(leaked.is_empty(), "tcui.log leaked {leaked:?}");
        assert!(log.contains("authorization code and state remain prose"));
        let _ = std::fs::remove_dir_all(root);
    }
}
