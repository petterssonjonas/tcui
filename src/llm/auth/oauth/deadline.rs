use std::future::Future;

use tokio::time::{sleep_until, Instant};

use super::{OAuthCancellation, OAuthError};

pub(super) struct OperationDeadline {
    instant: Instant,
    elapsed_error: OAuthError,
}

impl OperationDeadline {
    pub(super) fn new(instant: Instant, elapsed_error: OAuthError) -> Self {
        Self {
            instant,
            elapsed_error,
        }
    }

    pub(super) async fn race<T>(
        &self,
        cancellation: &OAuthCancellation,
        operation: impl Future<Output = T>,
    ) -> Result<T, OAuthError> {
        if Instant::now() >= self.instant {
            return Err(self.elapsed_error.clone());
        }
        let mut cancellation = cancellation.clone();
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => Err(OAuthError::Cancelled),
            _ = sleep_until(self.instant) => Err(self.elapsed_error.clone()),
            outcome = operation => Ok(outcome),
        }
    }
}
