use crate::llm::auth::oauth::{
    HeadlessAuthorizationInput, HeadlessInput, OAuthError, RedirectUri, State,
    complete_headless_authorization,
};

struct PastedInput(Option<HeadlessAuthorizationInput>);

impl HeadlessInput for PastedInput {
    fn read_authorization_input(&mut self) -> Result<HeadlessAuthorizationInput, OAuthError> {
        self.0.take().ok_or(OAuthError::InvalidValue)
    }
}

#[test]
fn headless_flow_uses_injected_redirect_url_input() -> Result<(), OAuthError> {
    let redirect_uri = RedirectUri::parse("https://client.example/callback")?;
    let state = State::generate()?;
    let input = HeadlessAuthorizationInput::RedirectUrl(format!(
        "https://client.example/callback?code=authorization-code&state={}",
        state.as_str()
    ));

    let code =
        complete_headless_authorization(&mut PastedInput(Some(input)), &redirect_uri, &state)?;

    assert_eq!(code.as_str(), "authorization-code");
    Ok(())
}

#[test]
fn headless_flow_uses_injected_direct_code_input() -> Result<(), OAuthError> {
    let redirect_uri = RedirectUri::parse("https://client.example/callback")?;
    let state = State::generate()?;
    let input = HeadlessAuthorizationInput::Code("authorization-code".to_owned());

    let code =
        complete_headless_authorization(&mut PastedInput(Some(input)), &redirect_uri, &state)?;

    assert_eq!(code.as_str(), "authorization-code");
    Ok(())
}
