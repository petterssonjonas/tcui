use chrono::Utc;

use crate::llm::auth::oauth::{
    oauth_cancellation, AuthorizationCode, AuthorizationCodeExchange, ClientId, OAuthError,
    PkceVerifier, RedirectUri, TokenEndpoint, TokenService,
};

#[tokio::test]
async fn token_exchange_rejects_an_encoded_form_larger_than_the_request_limit(
) -> Result<(), OAuthError> {
    let request = AuthorizationCodeExchange::new(
        ClientId::parse(&"&".repeat(4_096))?,
        RedirectUri::parse("http://127.0.0.1:34567/callback")?,
        AuthorizationCode::parse("&".repeat(4_096))?,
        PkceVerifier::parse("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk")?,
    );
    let client = reqwest::Client::new();
    let endpoint = TokenEndpoint::parse("http://127.0.0.1:9")?;
    let service = TokenService::new(&client, endpoint);
    let (cancellation, _) = oauth_cancellation();

    let result = service.exchange(&request, &cancellation, Utc::now()).await;

    assert!(matches!(result, Err(OAuthError::InvalidValue)));
    Ok(())
}

#[tokio::test]
async fn token_exchange_rejects_a_field_larger_than_the_request_field_limit(
) -> Result<(), OAuthError> {
    let request = AuthorizationCodeExchange::new(
        ClientId::parse(&"x".repeat(4_097))?,
        RedirectUri::parse("http://127.0.0.1:34567/callback")?,
        AuthorizationCode::parse("authorization-code".to_owned())?,
        PkceVerifier::parse("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk")?,
    );
    let client = reqwest::Client::new();
    let endpoint = TokenEndpoint::parse("http://127.0.0.1:9")?;
    let service = TokenService::new(&client, endpoint);
    let (cancellation, _) = oauth_cancellation();

    let result = service.exchange(&request, &cancellation, Utc::now()).await;

    assert!(matches!(result, Err(OAuthError::InvalidValue)));
    Ok(())
}
