use chrono::{TimeZone, Utc};

use crate::llm::auth::oauth::{
    oauth_cancellation, AuthorizationCode, AuthorizationCodeExchange, ClientId, OAuthError,
    PkceVerifier, RedirectUri, TokenErrorKind, TokenService,
};

use super::token_support::token_fixture;

fn exchange_request() -> Result<AuthorizationCodeExchange, OAuthError> {
    Ok(AuthorizationCodeExchange::new(
        ClientId::parse("client")?,
        RedirectUri::parse("http://127.0.0.1:34567/callback")?,
        AuthorizationCode::parse("authorization-code".to_owned())?,
        PkceVerifier::parse("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk")?,
    ))
}

#[tokio::test]
async fn token_response_accepts_rfc_extension_members_on_success_and_error(
) -> Result<(), Box<dyn std::error::Error>> {
    let (endpoint, _, server) = token_fixture(
        "200 OK",
        r#"{"access_token":"access","token_type":"Bearer","provider_extension":{"nested":true}}"#
            .to_owned(),
    )
    .await?;
    let client = reqwest::Client::new();
    let service = TokenService::new(&client, endpoint);
    let (cancellation, _) = oauth_cancellation();

    let success = service
        .exchange(&exchange_request()?, &cancellation, Utc::now())
        .await;
    server.await??;
    assert!(success.is_ok());

    let (endpoint, _, server) = token_fixture(
        "401 Unauthorized",
        r#"{"error":"invalid_grant","provider_extension":"ignored"}"#.to_owned(),
    )
    .await?;
    let service = TokenService::new(&client, endpoint);
    let error = service
        .exchange(&exchange_request()?, &cancellation, Utc::now())
        .await;
    server.await??;

    assert!(matches!(
        error,
        Err(OAuthError::TokenServer(TokenErrorKind::InvalidGrant))
    ));
    Ok(())
}

#[tokio::test]
async fn token_response_rejects_non_200_success_status() -> Result<(), Box<dyn std::error::Error>> {
    let (endpoint, _, server) = token_fixture(
        "201 Created",
        r#"{"access_token":"access","token_type":"Bearer"}"#.to_owned(),
    )
    .await?;
    let client = reqwest::Client::new();
    let service = TokenService::new(&client, endpoint);
    let (cancellation, _) = oauth_cancellation();

    let result = service
        .exchange(
            &exchange_request()?,
            &cancellation,
            Utc.with_ymd_and_hms(2026, 7, 12, 0, 0, 0)
                .single()
                .ok_or("fixed clock is invalid")?,
        )
        .await;
    server.await??;

    assert!(matches!(result, Err(OAuthError::UnexpectedTokenStatus)));
    Ok(())
}

#[tokio::test]
async fn token_response_rejects_wrong_types_for_required_success_members(
) -> Result<(), Box<dyn std::error::Error>> {
    let (endpoint, _, server) = token_fixture(
        "200 OK",
        r#"{"access_token":false,"token_type":"Bearer","provider_extension":"ignored"}"#.to_owned(),
    )
    .await?;
    let client = reqwest::Client::new();
    let service = TokenService::new(&client, endpoint);
    let (cancellation, _) = oauth_cancellation();

    let result = service
        .exchange(&exchange_request()?, &cancellation, Utc::now())
        .await;
    server.await??;

    assert!(matches!(result, Err(OAuthError::MalformedTokenResponse)));
    Ok(())
}

#[tokio::test]
async fn token_response_rejects_a_wrong_type_for_the_required_error_member(
) -> Result<(), Box<dyn std::error::Error>> {
    let (endpoint, _, server) = token_fixture(
        "400 Bad Request",
        r#"{"error":false,"provider_extension":"ignored"}"#.to_owned(),
    )
    .await?;
    let client = reqwest::Client::new();
    let service = TokenService::new(&client, endpoint);
    let (cancellation, _) = oauth_cancellation();

    let result = service
        .exchange(&exchange_request()?, &cancellation, Utc::now())
        .await;
    server.await??;

    assert!(matches!(result, Err(OAuthError::UnexpectedTokenStatus)));
    Ok(())
}
