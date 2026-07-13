use std::fmt;

use chrono::{DateTime, Utc};

use super::credential::{CodexCredentialSource, CodexResolutionError, resolve_credential};
use crate::config::AppConfig;

pub(crate) enum CodexStatus {
    Unauthenticated,
    Authenticated {
        source: CodexCredentialSource,
        account_present: bool,
        expires_at: Option<DateTime<Utc>>,
    },
}

pub(crate) fn codex_status(config: &AppConfig) -> Result<CodexStatus, CodexResolutionError> {
    let Some(credential) = resolve_credential(config)? else {
        return Ok(CodexStatus::Unauthenticated);
    };
    Ok(CodexStatus::Authenticated {
        source: credential.source(),
        account_present: credential.account_id().is_some(),
        expires_at: credential.expires_at(),
    })
}

impl fmt::Display for CodexStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unauthenticated => formatter.write_str("Codex: unauthenticated"),
            Self::Authenticated {
                source,
                account_present,
                expires_at,
            } => {
                let source = match source {
                    CodexCredentialSource::ExternalCli => "external-cli",
                    CodexCredentialSource::TcuiNative => "tcui-native",
                };
                write!(
                    formatter,
                    "Codex: authenticated source={source} account_present={account_present} expires_at={expires_at:?}"
                )
            }
        }
    }
}
