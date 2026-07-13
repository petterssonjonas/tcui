use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use super::deadline::OperationDeadline;
use super::{AuthorizationCode, OAuthCancellation, OAuthError};

const MAX_CALLBACK_HEADER_BYTES: usize = 8_192;

pub(super) struct CallbackRequest {
    pub(super) method: String,
    pub(super) target: String,
}

pub(super) async fn read_callback_request(
    stream: &mut TcpStream,
    cancellation: &OAuthCancellation,
    deadline: &OperationDeadline,
) -> Result<CallbackRequest, OAuthError> {
    let mut bytes = Vec::with_capacity(1_024);
    let mut buffer = [0_u8; 1_024];
    let header_end = loop {
        let read = deadline
            .race(cancellation, stream.read(&mut buffer))
            .await?
            .map_err(|_| OAuthError::CallbackIo)?;
        if read == 0 {
            return Err(OAuthError::MalformedCallback);
        }
        bytes.extend_from_slice(&buffer[..read]);
        if bytes.len() > MAX_CALLBACK_HEADER_BYTES {
            return Err(OAuthError::CallbackHeaderTooLarge);
        }
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
    };
    if bytes.len() != header_end {
        return Err(OAuthError::CallbackBody);
    }
    let header =
        std::str::from_utf8(&bytes[..header_end]).map_err(|_| OAuthError::MalformedCallback)?;
    let mut lines = header.split("\r\n");
    let request_line = lines.next().ok_or(OAuthError::MalformedCallback)?;
    let mut request_parts = request_line.split_ascii_whitespace();
    let method = request_parts.next().ok_or(OAuthError::MalformedCallback)?;
    let target = request_parts.next().ok_or(OAuthError::MalformedCallback)?;
    if request_parts.next() != Some("HTTP/1.1") || request_parts.next().is_some() {
        return Err(OAuthError::MalformedCallback);
    }
    if lines.any(|line| {
        line.split_once(':').is_some_and(|(name, value)| {
            name.trim().eq_ignore_ascii_case("content-length") && value.trim() != "0"
        })
    }) {
        return Err(OAuthError::CallbackBody);
    }
    Ok(CallbackRequest {
        method: method.to_owned(),
        target: target.to_owned(),
    })
}

pub(super) async fn write_response(
    stream: &mut TcpStream,
    result: &Result<AuthorizationCode, OAuthError>,
    cancellation: &OAuthCancellation,
    deadline: &OperationDeadline,
) -> Result<(), OAuthError> {
    let response = match result {
        Ok(_) => b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".as_slice(),
        Err(OAuthError::CallbackMethod) => {
            b"HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .as_slice()
        }
        Err(OAuthError::CallbackPath) => {
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".as_slice()
        }
        Err(_) => {
            b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".as_slice()
        }
    };
    deadline
        .race(cancellation, stream.write_all(response))
        .await?
        .map_err(|_| OAuthError::CallbackIo)?;
    deadline
        .race(cancellation, stream.shutdown())
        .await?
        .map_err(|_| OAuthError::CallbackIo)
}
