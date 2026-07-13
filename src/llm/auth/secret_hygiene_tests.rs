use chrono::{Duration, Utc};

use super::codex::CodexCredential;
use super::oauth::{DeviceCode, RefreshToken};
use super::{Credential, CredentialSource};
use crate::config::key_store::{OAuthCredential, OAuthCredentialOwnership, OAuthCredentialSource};

const ACCESS_TOKEN_CANARY: &str = "eyJ.tcui-secret-canary";
const REFRESH_TOKEN_CANARY: &str = "tcui-refresh-secret-canary";
const DEVICE_CODE_CANARY: &str = "dc.tcui-device-canary";

#[test]
fn credential_and_oauth_formatting_redacts_all_token_canaries() {
    // Given
    let oauth = OAuthCredential {
        provider: "Codex".to_owned(),
        access_token: ACCESS_TOKEN_CANARY.to_owned(),
        refresh_token: Some(REFRESH_TOKEN_CANARY.to_owned()),
        expires_at: Utc::now() + Duration::hours(1),
        account_id: Some("account-123".to_owned()),
        ownership: OAuthCredentialOwnership::Tcui,
        source: OAuthCredentialSource::NativeOAuth,
    };
    let codex = CodexCredential::native(oauth.clone());
    let credential = Credential::codex(codex.clone());
    let refresh = RefreshToken::parse(REFRESH_TOKEN_CANARY.to_owned())
        .expect("refresh token fixture should parse");
    let device =
        DeviceCode::parse(DEVICE_CODE_CANARY.to_owned()).expect("device code fixture should parse");

    // When
    let rendered = [
        format!("{oauth:?}"),
        format!("{oauth}"),
        format!("{codex:?}"),
        format!("{codex}"),
        format!("{credential:?}"),
        format!("{credential}"),
        format!("{refresh:?}"),
        format!("{refresh}"),
        format!("{device:?}"),
        format!("{device}"),
    ]
    .join("\n");

    // Then
    assert_eq!(credential.source(), CredentialSource::TcuiNativeOAuth);
    for canary in [
        ACCESS_TOKEN_CANARY,
        REFRESH_TOKEN_CANARY,
        DEVICE_CODE_CANARY,
    ] {
        assert!(
            !rendered.contains(canary),
            "formatted credential leaked token canary {canary}"
        );
    }
}
