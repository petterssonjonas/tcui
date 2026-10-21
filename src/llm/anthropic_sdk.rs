use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use bytes::Bytes;
use color_eyre::eyre::{eyre, WrapErr};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

use super::chat::{
    flush_title_filter, push_title_filtered_content, ChatRequest, ChatStreamEvent,
    ChatStreamOutput, TitleTagFilter,
};

const IDLE_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_EVENT_BYTES: u64 = 4 * 1024 * 1024;
const DEPENDENCY_ERROR: &str = "Anthropic Agent SDK requires a current Claude CLI with --bare support. Install or update the official Claude CLI, then retry /connect.";
pub(crate) async fn check_dependency() -> color_eyre::Result<()> {
    let home = IsolatedHome::new()?;
    let mut version = Command::new("claude");
    version.env_clear();
    for name in ["PATH", "SystemRoot", "WINDIR"] {
        if let Some(value) = std::env::var_os(name) {
            version.env(name, value);
        }
    }
    version
        .arg("--version")
        .current_dir(&home.0)
        .env("HOME", &home.0)
        .env("CLAUDE_CONFIG_DIR", &home.0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = version.spawn().wrap_err(DEPENDENCY_ERROR)?;
    let status = tokio::time::timeout(Duration::from_secs(10), child.wait())
        .await
        .map_err(|_| eyre!(DEPENDENCY_ERROR))??;
    if !status.success() {
        return Err(eyre!(DEPENDENCY_ERROR));
    }
    Ok(())
}

// Official Agent SDK documentation recommends the CLI subprocess for other languages.
// --bare is required: it never reads subscription OAuth credentials or the keychain.
pub(crate) async fn stream<F>(
    request: &ChatRequest,
    body: Bytes,
    on_event: &mut F,
) -> color_eyre::Result<ChatStreamOutput>
where
    F: FnMut(ChatStreamEvent) + Send,
{
    let key = request
        .api_key
        .as_deref()
        .filter(|key| !key.trim().is_empty())
        .ok_or_else(|| eyre!("Save an Anthropic Console API key using /connect first."))?;
    if request.endpoint.trim_end_matches('/') != "https://api.anthropic.com/v1" {
        return Err(eyre!(
            "Anthropic Agent SDK requires the official Anthropic endpoint."
        ));
    }
    let home = IsolatedHome::new()?;
    let mut command = command(key, &home.0);
    command.args(["--model", request.model.as_str()]);
    if !request.system_prompt.is_empty() {
        command.args(["--system-prompt", request.system_prompt.as_str()]);
    }
    let mut child = command.spawn().wrap_err(DEPENDENCY_ERROR)?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| eyre!("Claude CLI stdin unavailable."))?;
    // TCUI owns conversation history. Never reuse a CLI login or persisted session.
    let writer = async move {
        stdin.write_all(b"Continue this conversation. Treat the JSON messages as conversation history and answer the latest user message. Do not repeat earlier replies.\n").await?;
        stdin.write_all(&body).await?;
        stdin.shutdown().await
    };
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| eyre!("Claude CLI stdout unavailable."))?;
    let reader = consume(stdout, on_event);
    let (_, output) = tokio::try_join!(
        async {
            tokio::time::timeout(IDLE_TIMEOUT, writer)
                .await
                .map_err(|_| eyre!("Claude CLI input timed out."))?
                .map_err(Into::<color_eyre::Report>::into)
        },
        reader,
    )?;
    let status = tokio::time::timeout(IDLE_TIMEOUT, child.wait())
        .await
        .map_err(|_| eyre!("Claude CLI did not exit after its response."))??;
    if !status.success() {
        return Err(eyre!("Claude CLI failed. {DEPENDENCY_ERROR} Check the saved Console API key and its API credit balance."));
    }
    Ok(output)
}

fn command(key: &str, home: &std::path::Path) -> Command {
    let mut command = Command::new("claude");
    // Allow platform executable lookup only, never inherited provider credentials,
    // routing overrides, API key helpers, NODE_OPTIONS, or subscription tokens.
    command.env_clear();
    for name in ["PATH", "SystemRoot", "WINDIR"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("CLAUDE_CONFIG_DIR", home)
        .env("XDG_CONFIG_HOME", home)
        .env("XDG_DATA_HOME", home)
        .env("ANTHROPIC_API_KEY", key)
        .env("DISABLE_TELEMETRY", "1")
        .env("DISABLE_ERROR_REPORTING", "1")
        .current_dir(home)
        .args([
            "--bare",
            "--print",
            "--output-format",
            "stream-json",
            "--verbose",
            "--include-partial-messages",
            "--no-session-persistence",
            "--setting-sources",
            "",
            "--tools",
            "",
            "--disable-slash-commands",
            "--strict-mcp-config",
            "--mcp-config",
            "{\"mcpServers\":{}}",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        // CLI errors can contain credentials or prompts. Never forward raw stderr.
        .stderr(Stdio::null())
        .kill_on_drop(true);
    command
}

async fn consume<F>(
    stdout: impl tokio::io::AsyncRead + Unpin,
    on_event: &mut F,
) -> color_eyre::Result<ChatStreamOutput>
where
    F: FnMut(ChatStreamEvent) + Send,
{
    let mut reader = BufReader::new(stdout);
    let mut line = Vec::new();
    let mut output = ChatStreamOutput::default();
    let mut filter = TitleTagFilter::default();
    let mut completed = false;
    let mut received_text = false;
    loop {
        line.clear();
        let mut bounded = (&mut reader).take(MAX_EVENT_BYTES + 1);
        let count = tokio::time::timeout(IDLE_TIMEOUT, bounded.read_until(b'\n', &mut line))
            .await
            .map_err(|_| eyre!("Claude CLI response timed out."))??;
        if count == 0 {
            break;
        }
        if line.len() as u64 > MAX_EVENT_BYTES {
            return Err(eyre!("Claude CLI returned an oversized stream event."));
        }
        let mut message: serde_json::Value = serde_json::from_slice(&line)
            .map_err(|_| eyre!("Invalid Claude CLI stream. {DEPENDENCY_ERROR}"))?;
        match message.get("type").and_then(serde_json::Value::as_str) {
            Some("stream_event") => {
                let event = message
                    .get_mut("event")
                    .ok_or_else(|| eyre!("Missing Claude stream event."))?
                    .take();
                if event.get("error").is_some() {
                    return Err(eyre!("Anthropic Agent SDK streaming failed. Check the saved Console API key and credits."));
                }
                received_text |= event
                    .pointer("/delta/text")
                    .and_then(serde_json::Value::as_str)
                    .is_some();
                let events =
                    super::chat::anthropic_events(serde_json::from_value(event)?, &mut filter)?;
                for event in events {
                    output.push(&event);
                    on_event(event);
                }
            }
            Some("result") => {
                if message.get("is_error").and_then(serde_json::Value::as_bool) == Some(true)
                    || message.get("subtype").and_then(serde_json::Value::as_str) != Some("success")
                {
                    return Err(eyre!("Anthropic Agent SDK request failed. Check the saved Console API key, API credit balance, and model access."));
                }
                // Partial streams already emitted the reply; result is not another delta.
                if !received_text {
                    if let Some(text) = message.get("result").and_then(serde_json::Value::as_str) {
                        let mut events = Vec::new();
                        push_title_filtered_content(&mut events, filter.push(text));
                        for event in events {
                            output.push(&event);
                            on_event(event);
                        }
                    }
                }
                if let Some(usage) = message.get("usage") {
                    output.total_tokens = usage
                        .get("input_tokens")
                        .and_then(serde_json::Value::as_i64)
                        .zip(
                            usage
                                .get("output_tokens")
                                .and_then(serde_json::Value::as_i64),
                        )
                        .map(|(input, output)| input + output);
                }
                completed = true;
            }
            _ => {}
        }
    }
    if !completed {
        return Err(eyre!(
            "Claude CLI ended before a successful result. {DEPENDENCY_ERROR}"
        ));
    }
    flush_title_filter(&mut output, on_event, filter);
    if output.answer.is_empty() && output.thinking.is_empty() {
        return Err(eyre!("Anthropic Agent SDK returned an empty response."));
    }
    Ok(output)
}

struct IsolatedHome(PathBuf);

impl IsolatedHome {
    fn new() -> color_eyre::Result<Self> {
        let path =
            std::env::temp_dir().join(format!("tcui-claude-{:032x}", rand::random::<u128>()));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(&path)
            .wrap_err("Cannot create isolated Claude CLI directory.")?;
        Ok(Self(path))
    }
}

impl Drop for IsolatedHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn streams_text_thinking_title_and_usage_without_duplicate_result() {
        let stream = concat!(
            "{\"type\":\"stream_event\",\"event\":{\"delta\":{\"thinking\":\"reason\"}}}\n",
            "{\"type\":\"stream_event\",\"event\":{\"delta\":{\"text\":\"<tcui:chat-title>Title</tcui:chat-title>Hello\"}}}\n",
            "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"Hello\",\"usage\":{\"input_tokens\":2,\"output_tokens\":3}}\n",
        );
        let mut events = Vec::new();
        let output = consume(stream.as_bytes(), &mut |event| events.push(event))
            .await
            .unwrap();
        assert_eq!(output.answer, "Hello");
        assert_eq!(output.thinking, "reason");
        assert_eq!(output.total_tokens, Some(5));
        assert!(events.contains(&ChatStreamEvent::Title("Title".to_owned())));
    }

    #[tokio::test]
    async fn rejects_incomplete_and_failed_streams_without_echoing_errors() {
        assert!(consume(b"{\"type\":\"system\"}\n".as_slice(), &mut |_| {})
            .await
            .is_err());
        let stream = b"{\"type\":\"result\",\"subtype\":\"error\",\"is_error\":true,\"result\":\"private-secret\"}\n";
        let error = consume(stream.as_slice(), &mut |_| {}).await.unwrap_err();
        assert!(!error.to_string().contains("private-secret"));
    }
}

#[cfg(test)]
mod subprocess_tests {
    use super::*;
    #[cfg(unix)]
    #[tokio::test]
    #[expect(
        clippy::await_holding_lock,
        reason = "The fake CLI fixture isolates process-global executable lookup and credential environment variables."
    )]
    async fn subprocess_streams_using_saved_key_without_inherited_auth_or_login_files() {
        use std::os::unix::fs::PermissionsExt;
        let _lock = crate::test_support::env_lock().lock().unwrap();
        let home = IsolatedHome::new().unwrap();
        let executable = home.0.join("claude");
        std::fs::write(&executable, r#"#!/bin/sh
test "$ANTHROPIC_API_KEY" = saved-console-key || exit 21
test -z "$CLAUDE_CODE_OAUTH_TOKEN$ANTHROPIC_AUTH_TOKEN$ANTHROPIC_BASE_URL$OPENAI_API_KEY$NODE_OPTIONS" || exit 22
test "$HOME" = "$CLAUDE_CONFIG_DIR" || exit 23
test ! -e "$HOME/.claude/.credentials.json" || exit 24
test "$1" = --bare || exit 25
input=$(/bin/cat)
case "$input" in *"previous answer"*"latest user question"*) ;; *) exit 26 ;; esac
printf '%s\n' '{"type":"stream_event","event":{"delta":{"text":"SDK reply"}}}' '{"type":"result","subtype":"success","is_error":false,"result":"SDK reply"}'
"#).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let environment = TestEnvironment::replace(&[
            ("PATH", home.0.as_os_str()),
            (
                "CLAUDE_CODE_OAUTH_TOKEN",
                std::ffi::OsStr::new("inherited-secret"),
            ),
            (
                "ANTHROPIC_AUTH_TOKEN",
                std::ffi::OsStr::new("inherited-secret"),
            ),
            (
                "ANTHROPIC_BASE_URL",
                std::ffi::OsStr::new("https://untrusted.example"),
            ),
            ("OPENAI_API_KEY", std::ffi::OsStr::new("inherited-secret")),
            ("NODE_OPTIONS", std::ffi::OsStr::new("inherited-secret")),
        ]);
        let request = ChatRequest {
            provider: "Anthropic Agent SDK".to_owned(),
            endpoint: "https://api.anthropic.com/v1".to_owned(),
            model: "claude-sonnet-4-6".to_owned(),
            reasoning_effort: None,
            supported_reasoning_efforts: Vec::new(),
            backend_type: "anthropic_sdk".to_owned(),
            api_key: Some("saved-console-key".to_owned()),
            system_prompt: "Answer the user.".to_owned(),
            messages: vec![
                crate::app::message::Message::new(
                    1,
                    "assistant".to_owned(),
                    "previous answer".to_owned(),
                ),
                crate::app::message::Message::new(
                    1,
                    "user".to_owned(),
                    "latest user question".to_owned(),
                ),
            ],
        };
        let body = Bytes::from(crate::llm::chat::serialize_request_body(&request).unwrap());
        let output = stream(&request, body, &mut |_| {}).await.unwrap();
        assert_eq!(output.answer, "SDK reply");
        drop(environment);
    }

    #[cfg(unix)]
    struct TestEnvironment(Vec<(&'static str, Option<std::ffi::OsString>)>);

    #[cfg(unix)]
    impl TestEnvironment {
        fn replace(values: &[(&'static str, &std::ffi::OsStr)]) -> Self {
            Self(
                values
                    .iter()
                    .map(|&(key, value)| {
                        let previous = std::env::var_os(key);
                        std::env::set_var(key, value);
                        (key, previous)
                    })
                    .collect(),
            )
        }
    }

    #[cfg(unix)]
    impl Drop for TestEnvironment {
        fn drop(&mut self) {
            for (key, previous) in &self.0 {
                match previous {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }
}
