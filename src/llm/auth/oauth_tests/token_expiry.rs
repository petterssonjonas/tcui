use chrono::{TimeZone, Utc};

use std::time::Duration;

use crate::llm::auth::oauth::{ExpirySkew, OAuthError, calculate_expiry};

#[test]
fn default_expiry_skew_reserves_thirty_seconds_before_expiry() -> Result<(), OAuthError> {
    assert_eq!(
        ExpirySkew::default(),
        ExpirySkew::new(Duration::from_secs(30))?
    );
    Ok(())
}

#[test]
fn expiry_calculation_handles_exact_clock_boundary() -> Result<(), OAuthError> {
    let now = Utc
        .with_ymd_and_hms(2026, 7, 12, 0, 0, 0)
        .single()
        .ok_or(OAuthError::InvalidValue)?;

    assert_eq!(
        calculate_expiry(now, Some(0))?,
        Some(
            Utc.with_ymd_and_hms(2026, 7, 12, 0, 0, 0)
                .single()
                .ok_or(OAuthError::InvalidValue)?
        )
    );
    Ok(())
}

#[test]
fn expiry_calculation_rejects_integer_and_datetime_overflow() {
    assert!(matches!(
        calculate_expiry(Utc::now(), Some(u64::MAX)),
        Err(OAuthError::ExpiryOverflow)
    ));
}
