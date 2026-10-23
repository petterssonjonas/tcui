use std::time::{Duration, Instant};

use super::submission::SubmissionId;
use super::{Action, TuiApp};

pub(super) type CancelEscape = Option<(SubmissionId, Instant)>;
const CANCEL_ESCAPE_WINDOW: Duration = Duration::from_millis(1000);

pub(super) fn cancel_escape_action(
    previous: CancelEscape,
    request_id: Option<SubmissionId>,
    now: Instant,
) -> (CancelEscape, Option<Action>) {
    let Some(request_id) = request_id else {
        return (None, None);
    };
    if previous.is_some_and(|(previous_id, pressed)| {
        previous_id == request_id && now.saturating_duration_since(pressed) <= CANCEL_ESCAPE_WINDOW
    }) {
        (None, Some(Action::CancelRequest))
    } else {
        (Some((request_id, now)), None)
    }
}

impl TuiApp {
    pub(crate) fn steer_active_request(&mut self, instruction: String) -> color_eyre::Result<()> {
        if instruction.trim().is_empty() {
            return Err(color_eyre::eyre::eyre!("Usage: /steer <instruction>"));
        }
        if self.active_request_id().is_none() {
            return Err(color_eyre::eyre::eyre!(
                "No active request in the displayed conversation. Your draft is preserved."
            ));
        }
        // HTTP SSE transports only receive events after submission. Claude --print
        // receives one prompt and closes stdin before streaming; neither is steerable.
        Err(color_eyre::eyre::eyre!(
            "Live steering is unsupported by the current transports. The request continues unchanged; your draft is preserved. Use /cancel, then resend your instruction."
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::submission::{InputSubmission, PendingDocuments, PreparedUserTurn};

    fn request_id() -> SubmissionId {
        PreparedUserTurn::prepare(&InputSubmission::new(
            "request",
            PendingDocuments::default(),
        ))
        .expect("prepare request")
        .submission_id()
    }

    #[test]
    fn escape_chord_includes_one_second_boundary_and_consumes_arm() {
        let id = request_id();
        let now = Instant::now();
        let (armed, action) = cancel_escape_action(None, Some(id), now);
        assert!(action.is_none());
        let (armed, action) =
            cancel_escape_action(armed, Some(id), now + Duration::from_millis(1000));
        assert!(armed.is_none());
        assert!(matches!(action, Some(Action::CancelRequest)));
    }

    #[test]
    fn escape_chord_rearms_after_timeout() {
        let id = request_id();
        let now = Instant::now();
        let later = now + Duration::from_millis(1001);
        let (armed, action) = cancel_escape_action(Some((id, now)), Some(id), later);
        assert_eq!(armed, Some((id, later)));
        assert!(action.is_none());
    }

    #[test]
    fn escape_chord_cannot_cancel_a_different_request() {
        let now = Instant::now();
        let id = request_id();
        let (armed, action) = cancel_escape_action(Some((request_id(), now)), Some(id), now);
        assert_eq!(armed, Some((id, now)));
        assert!(action.is_none());
    }

    #[test]
    fn escape_without_a_displayed_request_clears_chord() {
        let now = Instant::now();
        let (armed, action) = cancel_escape_action(Some((request_id(), now)), None, now);
        assert!(armed.is_none());
        assert!(action.is_none());
    }

    #[test]
    fn unsupported_steering_keeps_request_history_and_composer_unchanged() {
        crate::app::tests::with_test_app("unsupported-steering", |app| {
            let conversation_id = app.ui.tabs[0].active_conversation;
            let id = request_id();
            app.register_request(
                crate::app::requests::RequestRoute {
                    request_id: id,
                    origin_conversation_id: conversation_id,
                    destination_conversation_id: conversation_id,
                },
                0,
                Vec::new(),
            )
            .expect("register active request");
            app.append_request_content(id, "partial answer", false)
                .expect("stream partial answer");
            let history_before = serde_json::to_value(
                app.storage
                    .get_messages(conversation_id)
                    .expect("read history"),
            )
            .expect("serialize history");
            app.ui.tabs[0].input_content = "/steer change direction".into();
            app.ui.tabs[0].input_cursor = 8;
            let request_id = app.active_request_id();
            let error = app
                .steer_active_request("change direction".into())
                .unwrap_err();
            assert!(error.to_string().contains("unsupported"));
            assert_eq!(app.active_request_id(), request_id);
            assert!(app.ui.tabs[0].streaming);
            assert_eq!(app.ui.tabs[0].messages.len(), 1);
            assert_eq!(app.ui.tabs[0].messages[0].content, "partial answer");
            assert!(!app.requests.get(&id).expect("active request").cancelled);
            assert!(!app.requests.get(&id).expect("active request").finalized);
            assert_eq!(app.ui.tabs[0].input_content, "/steer change direction");
            assert_eq!(app.ui.tabs[0].input_cursor, 8);
            assert_eq!(
                serde_json::to_value(
                    app.storage
                        .get_messages(conversation_id)
                        .expect("read history")
                )
                .expect("serialize history"),
                history_before,
            );
        });
    }
}
