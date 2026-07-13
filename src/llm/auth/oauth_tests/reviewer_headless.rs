use crate::llm::auth::oauth::{
    HeadlessAuthorizationInput, HeadlessInput, OAuthError, RedirectUri, State,
    complete_headless_authorization,
};

struct FixedInput(Option<HeadlessAuthorizationInput>);

impl HeadlessInput for FixedInput {
    fn read_authorization_input(&mut self) -> Result<HeadlessAuthorizationInput, OAuthError> {
        self.0.take().ok_or(OAuthError::InvalidValue)
    }
}

#[test]
fn headless_authorization_rejects_oversized_code_and_redirect_url() -> Result<(), OAuthError> {
    let redirect_uri = RedirectUri::parse("https://client.example/callback")?;
    let state = State::generate()?;

    let code = complete_headless_authorization(
        &mut FixedInput(Some(HeadlessAuthorizationInput::Code("x".repeat(4_097)))),
        &redirect_uri,
        &state,
    );
    let url = complete_headless_authorization(
        &mut FixedInput(Some(HeadlessAuthorizationInput::RedirectUrl(format!(
            "https://client.example/callback?code={}",
            "x".repeat(8_192)
        )))),
        &redirect_uri,
        &state,
    );

    assert!(matches!(code, Err(OAuthError::HeadlessInputTooLarge)));
    assert!(matches!(url, Err(OAuthError::HeadlessInputTooLarge)));
    Ok(())
}
