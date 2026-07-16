use thiserror::Error;

#[cfg(test)]
use super::document::PendingDocuments;
use super::turn::{InputSubmission, PreparedUserTurn, SubmissionId};

#[derive(Debug, Clone)]
pub enum SubmissionState {
    Draft(InputSubmission),
    Preparing(InputSubmission),
    AwaitingConfirmation(PreparedUserTurn),
    Prepared(PreparedUserTurn),
    Committed(PreparedUserTurn),
    Launched { submission_id: SubmissionId },
    FailedAfterLaunch { submission_id: SubmissionId },
}

impl SubmissionState {
    pub const fn draft(submission: InputSubmission) -> Self {
        Self::Draft(submission)
    }

    pub fn begin_preparing(self) -> Result<Self, SubmissionTransitionError> {
        match self {
            Self::Draft(submission) => Ok(Self::Preparing(submission)),
            Self::Preparing(_)
            | Self::AwaitingConfirmation(_)
            | Self::Prepared(_)
            | Self::Committed(_)
            | Self::Launched { .. }
            | Self::FailedAfterLaunch { .. } => Err(SubmissionTransitionError),
        }
    }

    pub fn await_confirmation(self) -> Result<Self, SubmissionTransitionError> {
        match self {
            Self::Preparing(submission) => Ok(Self::AwaitingConfirmation(
                PreparedUserTurn::prepare(&submission),
            )),
            Self::Draft(_)
            | Self::AwaitingConfirmation(_)
            | Self::Prepared(_)
            | Self::Committed(_)
            | Self::Launched { .. }
            | Self::FailedAfterLaunch { .. } => Err(SubmissionTransitionError),
        }
    }

    pub fn confirm(self) -> Result<Self, SubmissionTransitionError> {
        match self {
            Self::AwaitingConfirmation(prepared) => Ok(Self::Prepared(prepared)),
            Self::Draft(_)
            | Self::Preparing(_)
            | Self::Prepared(_)
            | Self::Committed(_)
            | Self::Launched { .. }
            | Self::FailedAfterLaunch { .. } => Err(SubmissionTransitionError),
        }
    }

    pub fn finish_preparing(self) -> Result<Self, SubmissionTransitionError> {
        match self {
            Self::Preparing(submission) => {
                Ok(Self::Prepared(PreparedUserTurn::prepare(&submission)))
            }
            Self::Draft(_)
            | Self::AwaitingConfirmation(_)
            | Self::Prepared(_)
            | Self::Committed(_)
            | Self::Launched { .. }
            | Self::FailedAfterLaunch { .. } => Err(SubmissionTransitionError),
        }
    }

    pub fn commit(self) -> Result<Self, SubmissionTransitionError> {
        match self {
            Self::Prepared(prepared) => Ok(Self::Committed(prepared)),
            Self::Draft(_)
            | Self::Preparing(_)
            | Self::AwaitingConfirmation(_)
            | Self::Committed(_)
            | Self::Launched { .. }
            | Self::FailedAfterLaunch { .. } => Err(SubmissionTransitionError),
        }
    }

    pub fn launch(self) -> Result<Self, SubmissionTransitionError> {
        match self {
            Self::Committed(prepared) => Ok(Self::Launched {
                submission_id: prepared.submission_id,
            }),
            Self::Draft(_)
            | Self::Preparing(_)
            | Self::AwaitingConfirmation(_)
            | Self::Prepared(_)
            | Self::Launched { .. }
            | Self::FailedAfterLaunch { .. } => Err(SubmissionTransitionError),
        }
    }

    pub fn fail_after_launch(self) -> Result<Self, SubmissionTransitionError> {
        match self {
            Self::Launched { submission_id } => Ok(Self::FailedAfterLaunch { submission_id }),
            Self::Draft(_)
            | Self::Preparing(_)
            | Self::AwaitingConfirmation(_)
            | Self::Prepared(_)
            | Self::Committed(_)
            | Self::FailedAfterLaunch { .. } => Err(SubmissionTransitionError),
        }
    }

    pub const fn submission_id(&self) -> SubmissionId {
        match self {
            Self::Draft(submission) | Self::Preparing(submission) => submission.submission_id,
            Self::AwaitingConfirmation(prepared)
            | Self::Prepared(prepared)
            | Self::Committed(prepared) => prepared.submission_id,
            Self::Launched { submission_id } | Self::FailedAfterLaunch { submission_id } => {
                *submission_id
            }
        }
    }

    pub fn return_to_draft(self) -> Result<Self, SubmissionTransitionError> {
        match self {
            Self::Preparing(submission) => Ok(Self::Draft(submission)),
            Self::AwaitingConfirmation(prepared)
            | Self::Prepared(prepared)
            | Self::Committed(prepared) => Ok(Self::Draft(prepared.into_input_submission())),
            Self::Draft(_) | Self::Launched { .. } | Self::FailedAfterLaunch { .. } => {
                Err(SubmissionTransitionError)
            }
        }
    }

    #[cfg(test)]
    pub const fn documents(&self) -> Option<&PendingDocuments> {
        match self {
            Self::Draft(submission) | Self::Preparing(submission) => Some(&submission.documents),
            Self::AwaitingConfirmation(prepared)
            | Self::Prepared(prepared)
            | Self::Committed(prepared) => Some(&prepared.documents),
            Self::Launched { .. } | Self::FailedAfterLaunch { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
#[error("invalid submission state transition")]
pub struct SubmissionTransitionError;
