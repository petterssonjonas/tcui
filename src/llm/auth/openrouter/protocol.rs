use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub(super) struct CreateCodeRequest<'a> {
    pub(super) callback_url: &'a str,
    pub(super) code_challenge: &'a str,
    pub(super) code_challenge_method: &'static str,
}

#[derive(Deserialize)]
pub(super) struct CreateCodeResponse {
    pub(super) data: CreatedCode,
}

#[derive(Deserialize)]
pub(super) struct CreatedCode {
    pub(super) id: String,
}

#[derive(Serialize)]
pub(super) struct ExchangeCodeRequest<'a> {
    pub(super) code: &'a str,
    pub(super) code_verifier: &'a str,
    pub(super) code_challenge_method: &'static str,
}

#[derive(Deserialize)]
pub(super) struct ExchangeCodeResponse {
    pub(super) key: String,
}
