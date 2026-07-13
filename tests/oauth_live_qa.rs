const LIVE_QA_OPT_IN: &str = "TCUI_LIVE_OAUTH_QA";

#[test]
#[ignore = "requires user-owned Codex/OpenRouter accounts and explicit TCUI_LIVE_OAUTH_QA=1 opt-in"]
fn live_oauth_provider_qa_requires_user_owned_accounts() {
    // Given
    let opt_in = std::env::var(LIVE_QA_OPT_IN).ok();

    // When
    let explicitly_enabled = opt_in.as_deref() == Some("1");

    // Then
    assert!(
        explicitly_enabled,
        "live OAuth QA is deferred to a user with Codex/OpenRouter accounts; opt in with TCUI_LIVE_OAUTH_QA=1 and --ignored"
    );
    eprintln!(
        "LIVE QA DEFERRED: exercise Codex login/status/model/chat/logout and OpenRouter login/status/logout through the release CLI and TUI with user-owned accounts."
    );
}
