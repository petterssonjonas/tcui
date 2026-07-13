use std::error::Error;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::llm::auth::oauth::{
    CallbackPath, CallbackTimeout, LoopbackCallback, LoopbackCallbackConfig, OAuthError,
    RedirectUri, State,
};

pub(super) async fn callback_fixture() -> Result<(LoopbackCallback, String), OAuthError> {
    let state = State::generate()?;
    let state_value = state.as_str().to_owned();
    let config = LoopbackCallbackConfig::new(
        CallbackPath::parse("/callback")?,
        CallbackTimeout::new(Duration::from_millis(250))?,
    );
    let callback = LoopbackCallback::bind(config, state).await?;
    Ok((callback, state_value))
}

pub(super) async fn send_raw_callback(
    redirect_uri: &RedirectUri,
    request: &str,
) -> Result<String, Box<dyn Error>> {
    let uri = redirect_uri.as_url();
    let host = uri.host_str().ok_or("callback host is absent")?;
    let port = uri
        .port_or_known_default()
        .ok_or("callback port is absent")?;
    let mut stream = TcpStream::connect((host, port)).await?;
    stream.write_all(request.as_bytes()).await?;
    stream.shutdown().await?;

    let mut response = String::new();
    stream.read_to_string(&mut response).await?;
    Ok(response)
}

pub(super) fn request_for(callback: &LoopbackCallback, query: &str) -> String {
    format!(
        "GET {}?{query} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        callback.redirect_uri().as_url().path()
    )
}
