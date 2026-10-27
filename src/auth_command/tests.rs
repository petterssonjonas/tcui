use super::{
    AuthCommandError, AuthCommandRequest, AuthExitCode, AuthProvider,
    CODEX_NATIVE_EXPERIMENTAL_DISCLOSURE,
};
use crate::{Cli, Command};
use clap::{CommandFactory, Parser};

fn parse_auth_request(args: &[&str]) -> Result<AuthCommandRequest, AuthCommandError> {
    let cli = Cli::try_parse_from(args.iter().copied()).expect("auth arguments parse");
    let Some(Command::Auth(auth)) = cli.command else {
        panic!("expected auth command");
    };
    AuthCommandRequest::try_from(auth)
}

#[test]
fn login_request_parses_codex_headless() {
    let request = parse_auth_request(&["tcui", "auth", "login", "codex", "--headless"])
        .expect("canonical Codex request");

    assert!(matches!(
        request,
        AuthCommandRequest::Login(login)
            if login.provider == AuthProvider::Codex && login.headless && !login.native
    ));
}

#[test]
fn login_request_parses_explicit_codex_native_mode() {
    let request = parse_auth_request(&["tcui", "auth", "login", "codex", "--native"])
        .expect("native Codex request");

    assert!(matches!(
        request,
        AuthCommandRequest::Login(login)
            if login.provider == AuthProvider::Codex && !login.headless && login.native
    ));
}

#[test]
fn logout_request_parses_external_codex_mode() {
    let request = parse_auth_request(&["tcui", "auth", "logout", "codex", "--external"])
        .expect("external Codex logout request");

    assert!(matches!(
        request,
        AuthCommandRequest::Logout(logout)
            if logout.provider == AuthProvider::Codex && logout.external
    ));
}

#[test]
fn status_request_parses_without_a_provider() {
    let request =
        parse_auth_request(&["tcui", "auth", "status"]).expect("all-provider status request");

    assert!(matches!(
        request,
        AuthCommandRequest::Status(status) if status.provider.is_none()
    ));
}

#[test]
fn status_request_parses_openrouter_provider() {
    let request = parse_auth_request(&["tcui", "auth", "status", "openrouter"])
        .expect("OpenRouter status request");

    assert!(matches!(
        request,
        AuthCommandRequest::Status(status) if status.provider == Some(AuthProvider::OpenRouter)
    ));
}

#[test]
fn login_requires_a_provider() {
    let result = Cli::try_parse_from(["tcui", "auth", "login"]);

    assert!(result.is_err());
}

#[test]
fn unknown_provider_error_lists_supported_provider_ids() {
    let error = parse_auth_request(&["tcui", "auth", "login", "claude"])
        .expect_err("Claude OAuth is unsupported");

    assert_eq!(
        error.to_string(),
        "Claude OAuth is unsupported. Supported providers: codex, openrouter."
    );
}

#[test]
fn native_login_rejects_openrouter() {
    let error = parse_auth_request(&["tcui", "auth", "login", "openrouter", "--native"])
        .expect_err("native OpenRouter login is invalid");

    assert!(matches!(
        error,
        AuthCommandError::UnsupportedOption {
            provider: AuthProvider::OpenRouter,
            option: "--native"
        }
    ));
}

#[test]
fn external_logout_rejects_openrouter() {
    let error = parse_auth_request(&["tcui", "auth", "logout", "openrouter", "--external"])
        .expect_err("external OpenRouter logout is invalid");

    assert!(matches!(
        error,
        AuthCommandError::UnsupportedOption {
            provider: AuthProvider::OpenRouter,
            option: "--external"
        }
    ));
}

#[test]
fn login_help_describes_supported_provider_ids_and_flags() {
    let mut command = Cli::command();
    let error = command
        .try_get_matches_from_mut(["tcui", "auth", "login", "--help"])
        .expect_err("help exits without running a provider flow");

    let help = error.to_string();
    assert!(
        help.contains("codex, openrouter")
            && help.contains("--headless")
            && help.contains("--native")
    );
}

#[test]
fn auth_help_describes_the_stable_exit_code_taxonomy() {
    let mut command = Cli::command();
    let error = command
        .try_get_matches_from_mut(["tcui", "auth", "--help"])
        .expect_err("help exits without running a provider flow");

    assert!(
        error.to_string().contains(
            "Exit codes: 0 success; 10 unauthenticated; 11 denied or expired; 12 unsupported; 13 external CLI unavailable; 14 transport failure."
        )
    );
}

#[test]
fn native_disclosure_is_available_only_for_explicit_native_codex_login() {
    let native = parse_auth_request(&["tcui", "auth", "login", "codex", "--native"])
        .expect("native Codex request");
    let default =
        parse_auth_request(&["tcui", "auth", "login", "codex"]).expect("default Codex request");

    assert_eq!(
        native.disclosure(),
        Some(CODEX_NATIVE_EXPERIMENTAL_DISCLOSURE)
    );
    assert_eq!(default.disclosure(), None);
}

#[test]
fn auth_exit_codes_are_stable() {
    assert_eq!(AuthExitCode::Success.as_i32(), 0);
    assert_eq!(AuthExitCode::Unauthenticated.as_i32(), 10);
    assert_eq!(AuthExitCode::DeniedOrExpired.as_i32(), 11);
    assert_eq!(AuthExitCode::Unsupported.as_i32(), 12);
    assert_eq!(AuthExitCode::MissingExternalCli.as_i32(), 13);
    assert_eq!(AuthExitCode::TransportFailure.as_i32(), 14);
}
