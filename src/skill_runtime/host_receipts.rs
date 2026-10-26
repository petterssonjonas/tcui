use std::{
    future::Future,
    sync::{Arc, Mutex},
};

use crate::app::{submission::SubmissionId, Action, Message};

#[derive(Clone, Default)]
pub(crate) struct Receipts(Arc<Mutex<Vec<Message>>>);

impl Receipts {
    pub(crate) fn snapshot(&self) -> Vec<Message> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
}

struct ReceiptContext {
    receipts: Receipts,
    request_id: SubmissionId,
    sender: tokio::sync::mpsc::UnboundedSender<Action>,
}

tokio::task_local! {
    static CURRENT: ReceiptContext;
}

pub(crate) async fn capture<T>(
    receipts: Receipts,
    request_id: SubmissionId,
    sender: tokio::sync::mpsc::UnboundedSender<Action>,
    future: impl Future<Output = T>,
) -> T {
    CURRENT
        .scope(
            ReceiptContext {
                receipts,
                request_id,
                sender,
            },
            future,
        )
        .await
}

/// Record only already-redacted, confirmed tool outcomes; never credential observations.
pub(crate) fn record(content: String) {
    let _ = CURRENT.try_with(|context| {
        let mut message = Message::new(0, "assistant".into(), content);
        message.tool_source = Some("Host action receipt".into());
        context
            .receipts
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(message);
        let _ = context
            .sender
            .send(Action::PersistHostActionReceipts(context.request_id));
    });
}
