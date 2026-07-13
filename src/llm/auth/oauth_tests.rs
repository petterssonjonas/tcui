#[path = "oauth_tests/pkce_url.rs"]
mod pkce_url;

#[path = "oauth_tests/browser.rs"]
mod browser;
#[path = "oauth_tests/callback_rejection.rs"]
mod callback_rejection;
#[path = "oauth_tests/callback_success.rs"]
mod callback_success;
#[path = "oauth_tests/callback_support.rs"]
mod callback_support;
#[path = "oauth_tests/device.rs"]
mod device;
#[path = "oauth_tests/headless.rs"]
mod headless;
#[path = "oauth_tests/redaction.rs"]
mod redaction;
#[path = "oauth_tests/reviewer_callback.rs"]
mod reviewer_callback;
#[path = "oauth_tests/reviewer_callback_noise.rs"]
mod reviewer_callback_noise;
#[path = "oauth_tests/reviewer_deadline.rs"]
mod reviewer_deadline;
#[path = "oauth_tests/reviewer_deadline_drip.rs"]
mod reviewer_deadline_drip;
#[path = "oauth_tests/reviewer_headless.rs"]
mod reviewer_headless;
#[path = "oauth_tests/reviewer_token.rs"]
mod reviewer_token;
#[path = "oauth_tests/reviewer_token_bounds.rs"]
mod reviewer_token_bounds;
#[path = "oauth_tests/token_exchange.rs"]
mod token_exchange;
#[path = "oauth_tests/token_expiry.rs"]
mod token_expiry;
#[path = "oauth_tests/token_refresh.rs"]
mod token_refresh;
#[path = "oauth_tests/token_support.rs"]
mod token_support;
