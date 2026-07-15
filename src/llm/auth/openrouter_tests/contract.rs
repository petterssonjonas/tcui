use crate::llm::auth::{
    oauth::{HeadlessAuthorizationInput, HeadlessInput, OAuthError, RedirectUri},
    openrouter::OpenRouterAdapter,
};

pub(super) struct PastedInput(Option<HeadlessAuthorizationInput>);

impl PastedInput {
    pub(super) fn code(value: &str) -> Self {
        Self(Some(HeadlessAuthorizationInput::Code(value.to_owned())))
    }
}

impl HeadlessInput for PastedInput {
    fn read_authorization_input(&mut self) -> Result<HeadlessAuthorizationInput, OAuthError> {
        self.0.take().ok_or(OAuthError::InvalidValue)
    }
}

#[test]
fn headless_authorization_url_uses_only_documented_openrouter_pkce_parameters(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let adapter = OpenRouterAdapter::production()?;
    let redirect_uri = RedirectUri::parse("https://tcui.invalid/oauth/openrouter")?;

    // When
    let authorization = adapter.begin_headless(redirect_uri)?;

    // Then
    let query = authorization
        .authorization_url()
        .query_pairs()
        .collect::<Vec<_>>();
    assert_eq!(
        query,
        [
            (
                "callback_url".into(),
                "https://tcui.invalid/oauth/openrouter".into()
            ),
            ("code_challenge".into(), query[1].1.clone()),
            ("code_challenge_method".into(), "S256".into()),
        ]
    );
    Ok(())
}

#[test]
fn headless_completion_accepts_documented_direct_code_without_undocumented_state(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let adapter = OpenRouterAdapter::production()?;
    let authorization =
        adapter.begin_headless(RedirectUri::parse("https://tcui.invalid/oauth/openrouter")?)?;
    let mut input = PastedInput(Some(HeadlessAuthorizationInput::Code(
        "authorization-code".to_owned(),
    )));

    // When
    let code = authorization.complete_headless(&mut input)?;

    // Then
    assert_eq!(code.code(), "authorization-code");
    Ok(())
}

#[test]
fn headless_completion_validates_documented_pasted_redirect_without_state(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let adapter = OpenRouterAdapter::production()?;
    let authorization =
        adapter.begin_headless(RedirectUri::parse("https://tcui.invalid/oauth/openrouter")?)?;
    let mut input = PastedInput(Some(HeadlessAuthorizationInput::RedirectUrl(
        "https://tcui.invalid/oauth/openrouter?code=authorization-code".to_owned(),
    )));

    // When
    let code = authorization.complete_headless(&mut input)?;

    // Then
    assert_eq!(code.code(), "authorization-code");
    Ok(())
}
