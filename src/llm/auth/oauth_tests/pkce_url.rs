use std::collections::HashMap;

use crate::llm::auth::oauth::{
    AuthorizationEndpoint, AuthorizationRequest, ClientId, OAuthError, PkceVerifier, RedirectUri,
    Scope, State,
};

#[test]
fn pkce_s256_matches_rfc_7636_appendix_b_vector() -> Result<(), OAuthError> {
    let verifier = PkceVerifier::parse("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk")?;

    assert_eq!(
        verifier.s256_challenge().as_str(),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
    Ok(())
}

#[test]
fn generated_state_is_url_safe_and_has_256_bits_of_encoded_entropy() -> Result<(), OAuthError> {
    let state = State::generate()?;

    assert!(
        state.as_str().len() == 43
            && state
                .as_str()
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    );
    Ok(())
}

#[test]
fn state_comparison_rejects_short_and_long_candidates_without_length_shortcuts()
-> Result<(), OAuthError> {
    let state = State::generate()?;

    assert!(!state.matches(&state.as_str()[..42]));
    assert!(!state.matches(&format!("{}x", state.as_str())));
    Ok(())
}

#[test]
fn generated_pkce_verifier_is_unreserved_and_produces_s256_challenge() -> Result<(), OAuthError> {
    let verifier = PkceVerifier::generate()?;
    let challenge = verifier.s256_challenge();

    assert!(
        verifier.as_str().len() == 43
            && verifier
                .as_str()
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric()
                    || matches!(byte, b'-' | b'.' | b'_' | b'~'))
            && challenge.as_str().len() == 43
    );
    Ok(())
}

#[test]
fn authorization_url_preserves_existing_query_and_encodes_oauth_values() -> Result<(), OAuthError> {
    let endpoint = AuthorizationEndpoint::parse(
        "https://authorization.example/authorize?existing=one%20value&encoded=a%26b",
    )?;
    let request = AuthorizationRequest::new(
        ClientId::parse("client & identifier")?,
        RedirectUri::parse("http://127.0.0.1:34567/callback")?,
        State::generate()?,
        PkceVerifier::parse("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk")?.s256_challenge(),
        Some(Scope::parse("read write&encoded")?),
    );

    let values = request
        .build_url(&endpoint)?
        .query_pairs()
        .into_owned()
        .collect::<HashMap<_, _>>();

    assert_eq!(values.get("existing"), Some(&"one value".to_string()));
    assert_eq!(values.get("encoded"), Some(&"a&b".to_string()));
    assert_eq!(
        values.get("client_id"),
        Some(&"client & identifier".to_string())
    );
    assert_eq!(values.get("scope"), Some(&"read write&encoded".to_string()));
    assert_eq!(
        values.get("code_challenge_method"),
        Some(&"S256".to_string())
    );
    Ok(())
}

#[test]
fn authorization_url_rejects_endpoints_with_reserved_oauth_query_parameters()
-> Result<(), OAuthError> {
    let endpoint =
        AuthorizationEndpoint::parse("https://authorization.example/authorize?state=bad")?;
    let request = AuthorizationRequest::new(
        ClientId::parse("client")?,
        RedirectUri::parse("http://127.0.0.1:34567/callback")?,
        State::generate()?,
        PkceVerifier::parse("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk")?.s256_challenge(),
        None,
    );

    assert!(matches!(
        request.build_url(&endpoint),
        Err(OAuthError::ReservedAuthorizationParameter)
    ));
    Ok(())
}

#[test]
fn authorization_url_rejects_pre_supplied_scope() -> Result<(), OAuthError> {
    let endpoint =
        AuthorizationEndpoint::parse("https://authorization.example/authorize?scope=wrong")?;
    let request = AuthorizationRequest::new(
        ClientId::parse("client")?,
        RedirectUri::parse("http://127.0.0.1:34567/callback")?,
        State::generate()?,
        PkceVerifier::parse("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk")?.s256_challenge(),
        Some(Scope::parse("read")?),
    );

    assert!(matches!(
        request.build_url(&endpoint),
        Err(OAuthError::ReservedAuthorizationParameter)
    ));
    Ok(())
}
