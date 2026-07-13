use super::{AuthorizationCode, ClientId, PkceVerifier, RedirectUri};

pub(crate) struct AuthorizationCodeExchange {
    pub(super) client_id: ClientId,
    pub(super) redirect_uri: RedirectUri,
    pub(super) code: AuthorizationCode,
    pub(super) verifier: PkceVerifier,
}

impl AuthorizationCodeExchange {
    pub(crate) fn new(
        client_id: ClientId,
        redirect_uri: RedirectUri,
        code: AuthorizationCode,
        verifier: PkceVerifier,
    ) -> Self {
        Self {
            client_id,
            redirect_uri,
            code,
            verifier,
        }
    }
}
