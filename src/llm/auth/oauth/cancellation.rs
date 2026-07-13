use tokio::sync::watch;

#[derive(Clone)]
pub(crate) struct OAuthCancellation {
    receiver: watch::Receiver<bool>,
}

pub(crate) struct OAuthCancellationHandle {
    sender: watch::Sender<bool>,
}

pub(crate) fn oauth_cancellation() -> (OAuthCancellation, OAuthCancellationHandle) {
    let (sender, receiver) = watch::channel(false);
    (
        OAuthCancellation { receiver },
        OAuthCancellationHandle { sender },
    )
}

impl OAuthCancellation {
    pub(crate) fn is_cancelled(&self) -> bool {
        *self.receiver.borrow()
    }

    pub(crate) async fn cancelled(&mut self) {
        loop {
            if *self.receiver.borrow_and_update() {
                return;
            }
            if self.receiver.changed().await.is_err() {
                std::future::pending::<()>().await;
            }
        }
    }
}

impl OAuthCancellationHandle {
    pub(crate) fn cancel(&self) {
        self.sender.send_replace(true);
    }
}
