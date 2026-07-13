use chrono::Utc;
use secrecy::ExposeSecret;

use crate::llm::auth::oauth::{
    ClientId, OAuthError, RefreshToken, RefreshTokenExchange, TokenService, oauth_cancellation,
};

use super::token_support::token_fixture;

fn refresh_request() -> Result<RefreshTokenExchange, OAuthError> {
    Ok(RefreshTokenExchange::new(
        ClientId::parse("client")?,
        RefreshToken::parse("prior-refresh-secret".to_owned())?,
    ))
}

#[tokio::test]
async fn refresh_rotates_refresh_token_when_server_returns_one()
-> Result<(), Box<dyn std::error::Error>> {
    let (endpoint, request_receiver, server) = token_fixture(
        "200 OK",
        r#"{"access_token":"access-secret","token_type":"Bearer","refresh_token":"rotated-secret"}"#.to_owned(),
    )
    .await?;
    let client = reqwest::Client::new();
    let service = TokenService::new(&client, endpoint);
    let (cancellation, _) = oauth_cancellation();

    let token_set = service
        .refresh(&refresh_request()?, &cancellation, Utc::now())
        .await?;
    let request = request_receiver.await?;
    server.await??;

    assert!(request.contains("grant_type=refresh_token"));
    assert_eq!(
        token_set
            .refresh_token()
            .map(|token| token.as_str().expose_secret()),
        Some("rotated-secret")
    );
    Ok(())
}

#[tokio::test]
async fn refresh_preserves_prior_token_when_server_omits_rotation()
-> Result<(), Box<dyn std::error::Error>> {
    let (endpoint, _, server) = token_fixture(
        "200 OK",
        r#"{"access_token":"access-secret","token_type":"Bearer"}"#.to_owned(),
    )
    .await?;
    let client = reqwest::Client::new();
    let service = TokenService::new(&client, endpoint);
    let (cancellation, _) = oauth_cancellation();

    let token_set = service
        .refresh(&refresh_request()?, &cancellation, Utc::now())
        .await?;
    server.await??;

    assert_eq!(
        token_set
            .refresh_token()
            .map(|token| token.as_str().expose_secret()),
        Some("prior-refresh-secret")
    );
    Ok(())
}
