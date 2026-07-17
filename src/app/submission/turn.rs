use std::fmt;
use std::sync::Arc;

use thiserror::Error;

use super::document::PendingDocuments;
use crate::app::message::Message;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct SubmissionId(u64);

impl SubmissionId {
    fn new() -> Self {
        Self(rand::random())
    }
}

impl fmt::Debug for SubmissionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "SubmissionId({:016x})", self.0)
    }
}

#[derive(Clone)]
pub struct InputSubmission {
    pub(super) submission_id: SubmissionId,
    pub(super) instruction: Arc<str>,
    pub(super) documents: PendingDocuments,
}

impl InputSubmission {
    pub fn new(instruction: &str, documents: PendingDocuments) -> Self {
        Self {
            submission_id: SubmissionId::new(),
            instruction: Arc::from(instruction.trim()),
            documents,
        }
    }
}

impl fmt::Debug for InputSubmission {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InputSubmission")
            .field("submission_id", &self.submission_id)
            .field("instruction_bytes", &self.instruction.len())
            .field("documents", &self.documents)
            .finish()
    }
}

#[derive(Clone)]
pub struct PreparedUserTurn {
    pub(super) submission_id: SubmissionId,
    pub(super) instruction: Arc<str>,
    pub(super) documents: PendingDocuments,
    content: Arc<str>,
}

impl PreparedUserTurn {
    pub fn prepare(submission: &InputSubmission) -> Result<Self, PreparedUserTurnError> {
        let mut content = String::with_capacity(
            submission.instruction.len() + submission.documents.total_bytes(),
        );
        if !submission.instruction.is_empty() {
            content.push_str(&submission.instruction);
        }
        for document in submission.documents.iter() {
            if document.content().is_empty() {
                continue;
            }
            if !content.is_empty() {
                content.push_str("\n\n");
            }
            content.push_str(document.content());
        }
        if content.is_empty() {
            return Err(PreparedUserTurnError::EmptyContent);
        }
        Ok(Self {
            submission_id: submission.submission_id,
            instruction: Arc::clone(&submission.instruction),
            documents: submission.documents.clone(),
            content: Arc::from(content),
        })
    }

    pub const fn submission_id(&self) -> SubmissionId {
        self.submission_id
    }

    pub fn instruction(&self) -> &str {
        &self.instruction
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub(crate) const fn documents(&self) -> &PendingDocuments {
        &self.documents
    }

    pub fn expand_messages(&self, history: &[Message], conversation_id: i64) -> Vec<Message> {
        let mut messages = Vec::with_capacity(history.len() + 1);
        messages.extend_from_slice(history);
        messages.push(Message::new(
            conversation_id,
            "user".to_string(),
            self.content.to_string(),
        ));
        messages
    }

    pub(super) fn into_input_submission(self) -> InputSubmission {
        InputSubmission {
            submission_id: self.submission_id,
            instruction: self.instruction,
            documents: self.documents,
        }
    }
}

impl fmt::Debug for PreparedUserTurn {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedUserTurn")
            .field("submission_id", &self.submission_id)
            .field("instruction_bytes", &self.instruction.len())
            .field("content_bytes", &self.content.len())
            .field("documents", &self.documents)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
pub enum PreparedUserTurnError {
    #[error("message content must not be empty")]
    EmptyContent,
}
