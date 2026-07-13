use tokio::net::TcpListener;
use tokio::time::Instant;

use super::callback::{AuthorizationCode, CallbackPath};
use super::callback_transport::{read_callback_request, write_response};
use super::deadline::OperationDeadline;
use super::{LoopbackCallbackConfig, OAuthCancellation, OAuthError, RedirectUri, State};

const MAX_INVALID_CALLBACK_ATTEMPTS: usize = 8;

pub(crate) struct LoopbackCallback {
    listener: TcpListener,
    redirect_uri: RedirectUri,
    expected_path: CallbackPath,
    expected_state: Option<State>,
    deadline: Instant,
}

impl LoopbackCallback {
    pub(crate) async fn bind(
        config: LoopbackCallbackConfig,
        state: State,
    ) -> Result<Self, OAuthError> {
        let listener = TcpListener::bind(&config.bind_address)
            .await
            .map_err(|_| OAuthError::CallbackIo)?;
        let port = listener
            .local_addr()
            .map_err(|_| OAuthError::CallbackIo)?
            .port();
        let redirect_uri = RedirectUri::parse(&format!(
            "http://{}:{port}{}",
            config.redirect_host,
            config.path.as_str()
        ))?;
        let deadline = Instant::now()
            .checked_add(config.timeout.duration())
            .ok_or(OAuthError::CallbackTimeout)?;
        Ok(Self {
            listener,
            redirect_uri,
            expected_path: config.path,
            expected_state: Some(state),
            deadline,
        })
    }

    pub(crate) async fn bind_without_state(
        config: LoopbackCallbackConfig,
    ) -> Result<Self, OAuthError> {
        let listener = TcpListener::bind(&config.bind_address)
            .await
            .map_err(|_| OAuthError::CallbackIo)?;
        let port = listener
            .local_addr()
            .map_err(|_| OAuthError::CallbackIo)?
            .port();
        let redirect_uri = RedirectUri::parse(&format!(
            "http://{}:{port}{}",
            config.redirect_host,
            config.path.as_str()
        ))?;
        let deadline = Instant::now()
            .checked_add(config.timeout.duration())
            .ok_or(OAuthError::CallbackTimeout)?;
        Ok(Self {
            listener,
            redirect_uri,
            expected_path: config.path,
            expected_state: None,
            deadline,
        })
    }

    pub(crate) fn redirect_uri(&self) -> &RedirectUri {
        &self.redirect_uri
    }

    pub(crate) async fn receive(
        mut self,
        cancellation: &OAuthCancellation,
    ) -> Result<AuthorizationCode, OAuthError> {
        let deadline = OperationDeadline::new(self.deadline, OAuthError::CallbackTimeout);
        let mut rejected = 0;
        loop {
            let result = self.accept_and_handle(cancellation, &deadline).await;
            match result {
                Ok(code) => return Ok(code),
                Err(error) if is_retryable_callback_error(&error) => {
                    rejected += 1;
                    if rejected >= MAX_INVALID_CALLBACK_ATTEMPTS {
                        return Err(OAuthError::CallbackAttemptsExceeded);
                    }
                }
                Err(error) => return Err(error),
            }
        }
    }

    async fn accept_and_handle(
        &mut self,
        cancellation: &OAuthCancellation,
        deadline: &OperationDeadline,
    ) -> Result<AuthorizationCode, OAuthError> {
        let (mut stream, _) = deadline
            .race(cancellation, self.listener.accept())
            .await?
            .map_err(|_| OAuthError::CallbackIo)?;
        let result = read_callback_request(&mut stream, cancellation, deadline)
            .await
            .and_then(|request| match &self.expected_state {
                Some(state) => super::response::parse_authorization_response(
                    &request.method,
                    &request.target,
                    &self.expected_path,
                    state,
                ),
                None => super::response::parse_authorization_response_without_state(
                    &request.method,
                    &request.target,
                    &self.expected_path,
                ),
            });
        write_response(&mut stream, &result, cancellation, deadline).await?;
        result
    }
}

fn is_retryable_callback_error(error: &OAuthError) -> bool {
    matches!(
        error,
        OAuthError::CallbackIo
            | OAuthError::CallbackMethod
            | OAuthError::CallbackPath
            | OAuthError::MalformedCallback
            | OAuthError::CallbackEncoding
            | OAuthError::CallbackBody
            | OAuthError::CallbackHeaderTooLarge
            | OAuthError::DuplicateCallbackParameter
            | OAuthError::StateMismatch
    )
}
