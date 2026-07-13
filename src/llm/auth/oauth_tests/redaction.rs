use crate::llm::auth::oauth::{
    AuthorizationCode, DeviceCode, OAuthError, PkceVerifier, RefreshToken,
};

#[test]
fn oauth_secret_newtypes_and_errors_do_not_expose_values() -> Result<(), OAuthError> {
    let code = AuthorizationCode::parse("authorization-code-secret".to_owned())?;
    let device = DeviceCode::parse("device-code-secret".to_owned())?;
    let refresh = RefreshToken::parse("refresh-token-secret".to_owned())?;
    let verifier = PkceVerifier::parse("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk")?;
    let rendered = format!("{code:?} {device:?} {refresh:?} {verifier:?}");

    assert!(
        !rendered.contains("authorization-code-secret")
            && !rendered.contains("device-code-secret")
            && !rendered.contains("refresh-token-secret")
            && !rendered.contains("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk")
    );
    Ok(())
}
