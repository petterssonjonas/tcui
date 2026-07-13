use std::fmt;

use oauth2::{CsrfToken, PkceCodeChallenge, PkceCodeVerifier};

use super::OAuthError;

const PKCE_VERIFIER_MIN_LENGTH: usize = 43;
const PKCE_VERIFIER_MAX_LENGTH: usize = 128;
const STATE_RANDOM_BYTES: u32 = 32;

pub(crate) struct PkceVerifier(PkceCodeVerifier);

impl PkceVerifier {
    pub(crate) fn generate() -> Result<Self, OAuthError> {
        let (_, verifier) = PkceCodeChallenge::new_random_sha256();
        Ok(Self(verifier))
    }

    pub(crate) fn parse(value: &str) -> Result<Self, OAuthError> {
        if !(PKCE_VERIFIER_MIN_LENGTH..=PKCE_VERIFIER_MAX_LENGTH).contains(&value.len())
            || !value.bytes().all(is_unreserved)
        {
            return Err(OAuthError::InvalidValue);
        }
        Ok(Self(PkceCodeVerifier::new(value.to_owned())))
    }

    pub(crate) fn s256_challenge(&self) -> PkceCodeChallenge {
        PkceCodeChallenge::from_code_verifier_sha256(&self.0)
    }

    pub(super) fn as_oauth(&self) -> PkceCodeVerifier {
        PkceCodeVerifier::new(self.0.secret().to_owned())
    }

    pub(crate) fn as_str(&self) -> &str {
        self.0.secret()
    }
}

impl fmt::Debug for PkceVerifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PkceVerifier(<redacted>)")
    }
}

#[derive(Clone)]
pub(crate) struct State(CsrfToken);

impl State {
    pub(crate) fn generate() -> Result<Self, OAuthError> {
        Ok(Self(CsrfToken::new_random_len(STATE_RANDOM_BYTES)))
    }

    pub(super) fn as_oauth(&self) -> CsrfToken {
        self.0.clone()
    }

    pub(crate) fn as_str(&self) -> &str {
        self.0.secret()
    }

    pub(crate) fn matches(&self, candidate: &str) -> bool {
        self.0 == CsrfToken::new(candidate.to_owned())
    }
}

impl fmt::Debug for State {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("State(<redacted>)")
    }
}

const fn is_unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
}
