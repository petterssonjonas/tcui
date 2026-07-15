use chrono::{TimeDelta, TimeZone, Utc};
use secrecy::ExposeSecret;

use crate::llm::auth::oauth::{
    oauth_cancellation, AuthorizationCode, AuthorizationCodeExchange, ClientId, ExpirySkew,
    OAuthError, PkceVerifier, RedirectUri, TokenErrorKind, TokenService,
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
async fn authorization_code_exchange_posts_pkce_form_and_parses_strict_success(
) -> Result<(), Box<dyn std::error::Error>> {
    let (endpoint, request_receiver, server) = token_fixture(
        "200 OK",
        r#"{"access_token":"access-secret","token_type":"Bearer","expires_in":60,"refresh_token":"refresh-secret","id_token":"header.account-claims.signature"}"#.to_owned(),
    )
    .await?;
    let client = reqwest::Client::new();
    let service = TokenService::new(&client, endpoint);
    let now = Utc
        .with_ymd_and_hms(2026, 7, 12, 0, 0, 0)
        .single()
        .ok_or("fixed clock is invalid")?;
    let (cancellation, _) = oauth_cancellation();

    let token_set = service
        .exchange(&exchange_request()?, &cancellation, now)
        .await?;
    let request = request_receiver.await?;
    server.await??;

    assert!(request.contains("grant_type=authorization_code"));
    assert!(request.contains("code_verifier=dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"));
    assert_eq!(
        token_set.access_token().as_str().expose_secret(),
        "access-secret"
    );
    assert_eq!(
        token_set.expires_at(),
        Some(
            Utc.with_ymd_and_hms(2026, 7, 12, 0, 1, 0)
                .single()
                .ok_or("fixed expiry is invalid")?
        )
    );
    let skew = ExpirySkew::default();
    assert!(token_set.is_usable_at(now + TimeDelta::seconds(29), skew));
    assert!(!token_set.is_usable_at(now + TimeDelta::seconds(30), skew));
    assert!(!format!("{token_set:?}").contains("access-secret"));
    assert_eq!(
        token_set
            .id_token()
            .map(|id_token| id_token.as_str().expose_secret()),
        Some("header.account-claims.signature")
    );
    assert!(!format!("{token_set:?}").contains("account-claims"));
    Ok(())
}

#[tokio::test]
async fn token_exchange_rejects_malformed_and_error_json_without_secret_output(
) -> Result<(), Box<dyn std::error::Error>> {
    let (endpoint, _, server) =
        token_fixture("200 OK", "{\"access_token\":\"only-token\"}".to_owned()).await?;
    let client = reqwest::Client::new();
    let service = TokenService::new(&client, endpoint);
    let (cancellation, _) = oauth_cancellation();

    let malformed = service
        .exchange(&exchange_request()?, &cancellation, Utc::now())
        .await;
    server.await??;

    assert!(matches!(malformed, Err(OAuthError::MalformedTokenResponse)));

    let (endpoint, _, server) = token_fixture(
        "400 Bad Request",
        r#"{"error":"invalid_grant","error_description":"authorization-code-secret"}"#.to_owned(),
    )
    .await?;
    let service = TokenService::new(&client, endpoint);
    let rejected = service
        .exchange(&exchange_request()?, &cancellation, Utc::now())
        .await;
    server.await??;

    assert!(matches!(
        &rejected,
        Err(OAuthError::TokenServer(TokenErrorKind::InvalidGrant))
    ));
    assert!(!rejected
        .unwrap_err()
        .to_string()
        .contains("authorization-code-secret"));
    Ok(())
}

#[tokio::test]
async fn token_exchange_honors_pre_cancelled_operation() -> Result<(), OAuthError> {
    let endpoint = crate::llm::auth::oauth::TokenEndpoint::parse("http://127.0.0.1:9")?;
    let client = reqwest::Client::new();
    let service = TokenService::new(&client, endpoint);
    let (cancellation, handle) = oauth_cancellation();
    handle.cancel();

    assert!(matches!(
        service
            .exchange(&exchange_request()?, &cancellation, Utc::now())
            .await,
        Err(OAuthError::Cancelled)
    ));
    Ok(())
}

#[tokio::test]
async fn token_exchange_rejects_oversized_json_response() -> Result<(), Box<dyn std::error::Error>>
{
    let (endpoint, _, server) = token_fixture(
        "200 OK",
        format!(
            "{{\"access_token\":\"token\",\"token_type\":\"Bearer\",\"scope\":\"{}\"}}",
            "x".repeat(16_384)
        ),
    )
    .await?;
    let client = reqwest::Client::new();
    let service = TokenService::new(&client, endpoint);
    let (cancellation, _) = oauth_cancellation();

    let result = service
        .exchange(&exchange_request()?, &cancellation, Utc::now())
        .await;
    server.await??;

    assert!(matches!(result, Err(OAuthError::TokenResponseTooLarge)));
    Ok(())
}
