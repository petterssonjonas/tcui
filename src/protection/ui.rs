use color_eyre::{eyre::eyre, Result};
use crossterm::event::{Event, EventStream, KeyCode, KeyEventKind, KeyModifiers};
use futures::StreamExt;
use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use zeroize::{Zeroize, Zeroizing};

use super::ProtectionPolicy;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SecurityRestart {
    Configure,
    Unlock,
    InitializePassword,
    Lock,
    Transfer(super::Domain),
}

const MAX_PASSWORD_BYTES: usize = 4096;

fn password_required(policy: ProtectionPolicy, initialized: bool) -> bool {
    initialized || policy.any()
}

fn append_secret(destination: &mut String, text: &str) {
    for character in text.chars().filter(|character| !character.is_control()) {
        if destination.len() + character.len_utf8() > MAX_PASSWORD_BYTES {
            break;
        }
        destination.push(character);
    }
}

#[derive(Debug, PartialEq, Eq)]
enum SaveAction {
    Unlock,
    ConfigureUnlocked,
    ConfigureNew,
}

fn save_action(initialized: bool, unlocked: bool) -> SaveAction {
    match (initialized, unlocked) {
        (true, false) => SaveAction::Unlock,
        (true, true) => SaveAction::ConfigureUnlocked,
        (false, _) => SaveAction::ConfigureNew,
    }
}

fn can_continue(configured: bool, requires_unlock: bool, unlocked: bool, app_ready: bool) -> bool {
    configured && (!requires_unlock || unlocked) && app_ready
}

fn toggle(policy: &mut ProtectionPolicy, field: usize) {
    let value = match field {
        0 => &mut policy.app_password,
        1 => &mut policy.credentials,
        2 => &mut policy.chats,
        3 => &mut policy.memories,
        4 => &mut policy.vault,
        _ => return,
    };
    *value = !*value;
}

fn draw(terminal: &mut crate::TerminalType, title: &str, lines: Vec<Line<'_>>) -> Result<()> {
    terminal.draw(|frame| {
        frame.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .block(Block::default().borders(Borders::ALL).title(title)),
            frame.area(),
        );
    })?;
    Ok(())
}

async fn read_event(events: &mut EventStream) -> Result<Event> {
    match events.next().await {
        Some(event) => Ok(event?),
        None => Err(eyre!("Terminal input closed.")),
    }
}

/// Runs without constructing application stores or rendering cached application content.
pub(crate) async fn configure(
    terminal: &mut crate::TerminalType,
    events: &mut EventStream,
) -> Result<bool> {
    let mut status = crate::config::KeyStore::vault_status(&crate::config::AppConfig::default())?;
    let mut policy = super::policy()?.unwrap_or(ProtectionPolicy {
        credentials: status.initialized
            && status.storage == crate::config::key_store::CredentialStorage::Encrypted,
        ..ProtectionPolicy::default()
    });
    if !super::configured()? {
        for domain in [super::Domain::Vault, super::Domain::Memories] {
            #[cfg(not(feature = "memory"))]
            if domain == super::Domain::Memories {
                continue;
            }
            let inspection =
                crate::storage::native_transfer::native_root(domain).and_then(|root| {
                    crate::storage::native_transfer::NativeTransferService::prepare(
                        domain,
                        &root,
                        crate::storage::native_transfer::TransferMode::Add,
                    )
                });
            if let Ok(prepared) = inspection {
                let encrypted = prepared.files.iter().chain(&prepared.adoption_files).any(|file| file.encrypted)
                    || prepared.requirements.iter().any(|requirement| matches!(
                        requirement, crate::storage::native_transfer::TransferRequirement::LegacySourceWrapping { .. }
                    ));
                if encrypted {
                    match domain {
                        super::Domain::Vault => policy.vault = true,
                        super::Domain::Memories => policy.memories = true,
                        super::Domain::Chats => {}
                    }
                }
            }
        }
    }
    let mut password = Zeroizing::new(String::with_capacity(MAX_PASSWORD_BYTES));
    let mut confirmation = Zeroizing::new(String::with_capacity(MAX_PASSWORD_BYTES));
    let mut cursor = 0;
    let mut notice = String::new();
    loop {
        let action = save_action(status.initialized, status.unlocked);
        let requires_unlock = super::startup_requires_unlock()?;
        let mut lines = vec![
            Line::from("Choose protection independently for each app-owned data type."),
            Line::from(
                "Unchecked data remains plaintext. An entry password alone does NOT encrypt it.",
            ),
            Line::from(if !status.initialized {
                "Status: no shared password. Choose options and a new password."
            } else if status.unlocked {
                "Status: unlocked. Save selections without entering the password again."
            } else {
                "Status: locked. Unlock with the existing password to change protection."
            }),
            Line::from("Selections are not applied until you save."),
            Line::from(""),
        ];
        for (index, (label, checked)) in [
            ("Require password to enter the app", policy.app_password),
            (
                "Encrypt credentials (explicit plaintext connections remain possible)",
                policy.credentials,
            ),
            ("Encrypt chats (also requires startup unlock)", policy.chats),
            ("Encrypt memories", policy.memories),
            ("Encrypt internal vault", policy.vault),
        ]
        .into_iter()
        .enumerate()
        {
            lines.push(Line::from(format!(
                "{} [{}] {label}",
                if cursor == index { ">" } else { " " },
                if checked { "x" } else { " " }
            )));
        }
        lines.push(Line::from(""));
        if status.initialized {
            lines.push(Line::from(format!(
                "{} {}",
                if cursor == 5 { ">" } else { " " },
                if status.unlocked {
                    "Change shared password"
                } else {
                    "Unlock / recover with existing password"
                }
            )));
            lines.push(Line::from(format!(
                "{} {} (without saving staged selections)",
                if cursor == 6 { ">" } else { " " },
                if status.unlocked {
                    "Continue to app"
                } else if requires_unlock {
                    "Continue to app (unlock / recovery required)"
                } else {
                    "Continue to app with protected sections locked"
                }
            )));
        } else {
            lines.push(Line::from(format!(
                "{} New password: {}",
                if cursor == 5 { ">" } else { " " },
                "•".repeat(password.chars().count())
            )));
            lines.push(Line::from(format!(
                "{} Confirm:      {}",
                if cursor == 6 { ">" } else { " " },
                "•".repeat(confirmation.chars().count())
            )));
        }
        lines.push(Line::from(format!(
            "{} Save protection choices",
            if cursor == 7 { ">" } else { " " }
        )));
        lines.push(Line::from(
            "Tab / arrows: move   Space: toggle   Enter: select / save   Esc: cancel",
        ));
        lines.push(Line::from(
            "Ctrl+U: unlock / recover   Ctrl+P: change password   Ctrl+O: continue to app",
        ));
        lines.push(Line::from(Span::styled(
            notice.as_str(),
            Style::default().fg(Color::Yellow),
        )));
        draw(terminal, " Application protection ", lines)?;
        match read_event(events).await? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                let control = key.modifiers.contains(KeyModifiers::CONTROL);
                match key.code {
                    KeyCode::Esc => return Ok(false),
                    KeyCode::Char('c') if control => return Ok(false),
                    KeyCode::Char('o') if control => {
                        if can_continue(
                            super::configured()? || status.initialized,
                            super::startup_requires_unlock()?,
                            super::is_unlocked()?,
                            super::require_app().is_ok(),
                        ) {
                            return Ok(true);
                        }
                        notice = "Save first setup, or unlock / recover the saved setup before continuing.".into();
                    }
                    KeyCode::Char('u') if control => {
                        password.zeroize();
                        confirmation.zeroize();
                        if status.initialized || super::configured()? {
                            if unlock(terminal, events).await? {
                                notice = "Unlocked. Staged selections are unchanged and are not yet saved.".into();
                            }
                            status = crate::config::KeyStore::vault_status(
                                &crate::config::AppConfig::default(),
                            )?;
                        } else {
                            notice =
                                "Choose a new password and save the first protection setup.".into();
                        }
                    }
                    KeyCode::Char('p') if control => {
                        if status.unlocked && status.initialized {
                            if change_password(terminal, events).await? {
                                notice =
                                    "Password changed. Staged protection selections are not saved."
                                        .into();
                            }
                        } else {
                            notice = "Unlock the existing password before changing it.".into();
                        }
                        status = crate::config::KeyStore::vault_status(
                            &crate::config::AppConfig::default(),
                        )?;
                    }
                    KeyCode::Tab | KeyCode::Down => cursor = (cursor + 1) % 8,
                    KeyCode::BackTab | KeyCode::Up => cursor = (cursor + 7) % 8,
                    KeyCode::Char(' ') if cursor < 5 => toggle(&mut policy, cursor),
                    KeyCode::Enter if cursor < 5 => cursor += 1,
                    KeyCode::Enter if status.initialized && cursor == 5 => {
                        if status.unlocked {
                            if change_password(terminal, events).await? {
                                notice =
                                    "Password changed. Staged protection selections are not saved."
                                        .into();
                            }
                        } else if unlock(terminal, events).await? {
                            notice =
                                "Unlocked. Staged selections are unchanged and are not yet saved."
                                    .into();
                        }
                        status = crate::config::KeyStore::vault_status(
                            &crate::config::AppConfig::default(),
                        )?;
                    }
                    KeyCode::Enter if status.initialized && cursor == 6 => {
                        if can_continue(
                            super::configured()? || status.initialized,
                            super::startup_requires_unlock()?,
                            super::is_unlocked()?,
                            super::require_app().is_ok(),
                        ) {
                            return Ok(true);
                        }
                        notice = "Unlock / recover the saved setup before continuing.".into();
                    }
                    KeyCode::Enter if cursor < 7 => cursor += 1,
                    KeyCode::Enter => {
                        let setup = match action {
                            SaveAction::Unlock => {
                                if unlock(terminal, events).await? {
                                    notice =
                                        "Unlocked. Review the staged selections, then save.".into();
                                }
                                status = crate::config::KeyStore::vault_status(
                                    &crate::config::AppConfig::default(),
                                )?;
                                continue;
                            }
                            SaveAction::ConfigureUnlocked => {
                                match super::transfer_ui::prepare_configuration_sources(
                                    terminal, events, policy, None,
                                )
                                .await
                                {
                                    Ok(Some(_)) => super::configure_unlocked(policy),
                                    Ok(None) => {
                                        notice = "Source adoption cancelled. Protection selections were not saved.".into();
                                        continue;
                                    }
                                    Err(failure) => Err(failure),
                                }
                            }
                            SaveAction::ConfigureNew => {
                                if password_required(policy, false) && password.is_empty() {
                                    notice = "Enter a password before saving.".into();
                                    cursor = 5;
                                    continue;
                                }
                                if password.as_str() != confirmation.as_str() {
                                    notice = "Password confirmation does not match.".into();
                                    confirmation.zeroize();
                                    cursor = 6;
                                    continue;
                                }
                                match super::transfer_ui::prepare_configuration_sources(
                                    terminal,
                                    events,
                                    policy,
                                    Some(password.as_str()),
                                )
                                .await
                                {
                                    Ok(Some(_)) => super::configure(policy, password.as_str()),
                                    Ok(None) => {
                                        notice = "Source adoption cancelled. Protection selections were not saved.".into();
                                        continue;
                                    }
                                    Err(failure) => Err(failure),
                                }
                            }
                        };
                        password.zeroize();
                        confirmation.zeroize();
                        status = crate::config::KeyStore::vault_status(
                            &crate::config::AppConfig::default(),
                        )?;
                        match setup {
                            Ok(()) => {
                                notice =
                                    "Protection choices saved. Ctrl+O: continue to app.".into();
                                cursor = if status.initialized { 6 } else { 7 };
                            }
                            Err(failure) => {
                                if let Some(domain) = failure.chain().find_map(|cause| {
                                    cause
                                        .downcast_ref::<super::PortableUnlockNeeded>()
                                        .map(|needed| needed.domain)
                                }) {
                                    match super::transfer_ui::unlock_native_sources(
                                        terminal, events, domain,
                                    )
                                    .await
                                    {
                                        Ok(true) => {
                                            notice = "Source files unlocked. Save the staged selections to continue.".into();
                                            continue;
                                        }
                                        Ok(false) => {}
                                        Err(error) => {
                                            notice =
                                                format!("Source unlock did not complete: {error}");
                                            continue;
                                        }
                                    }
                                }
                                notice = format!("Protection changes were not completed; unlock to recover if needed: {failure}");
                            }
                        }
                    }
                    KeyCode::Backspace if !status.initialized && cursor == 5 => {
                        password.pop();
                    }
                    KeyCode::Backspace if !status.initialized && cursor == 6 => {
                        confirmation.pop();
                    }
                    KeyCode::Char(character)
                        if !status.initialized
                            && !key
                                .modifiers
                                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                    {
                        let mut bytes = [0; 4];
                        let text = character.encode_utf8(&mut bytes);
                        if cursor == 5 {
                            append_secret(&mut password, text);
                        } else if cursor == 6 {
                            append_secret(&mut confirmation, text);
                        }
                        bytes.zeroize();
                    }
                    _ => {}
                }
            }
            Event::Paste(mut text) => {
                if !status.initialized {
                    if cursor == 5 {
                        append_secret(&mut password, &text);
                    } else if cursor == 6 {
                        append_secret(&mut confirmation, &text);
                    }
                }
                text.zeroize();
            }
            _ => {}
        }
    }
}

/// Initializes the shared password without changing any protection or storage choices.
pub(crate) async fn initialize_password(
    terminal: &mut crate::TerminalType,
    events: &mut EventStream,
) -> Result<bool> {
    password_dialog(terminal, events, false).await
}

async fn change_password(
    terminal: &mut crate::TerminalType,
    events: &mut EventStream,
) -> Result<bool> {
    password_dialog(terminal, events, true).await
}

async fn password_dialog(
    terminal: &mut crate::TerminalType,
    events: &mut EventStream,
    change: bool,
) -> Result<bool> {
    let mut password = Zeroizing::new(String::with_capacity(MAX_PASSWORD_BYTES));
    let mut confirmation = Zeroizing::new(String::with_capacity(MAX_PASSWORD_BYTES));
    let mut cursor = 0;
    let mut error = String::new();
    loop {
        draw(
            terminal,
            if change {
                " Change shared password "
            } else {
                " Initialize application password "
            },
            vec![
                Line::from(if change {
                    "Choose a new shared password. The current session is already authenticated."
                } else {
                    "Create the shared application / credential-vault password."
                }),
                Line::from("Existing plaintext connections and data remain plaintext."),
                Line::from("Protection choices and the new-connection default stay unchanged."),
                Line::from(if change {
                    "Previously exported copies keep their old password."
                } else {
                    ""
                }),
                Line::from(if change {
                    "A password change cannot revoke a data key that was already copied or extracted."
                } else {
                    ""
                }),
                Line::from(""),
                Line::from(format!(
                    "{} Password: {}",
                    if cursor == 0 { ">" } else { " " },
                    "•".repeat(password.chars().count())
                )),
                Line::from(format!(
                    "{} Confirm:  {}",
                    if cursor == 1 { ">" } else { " " },
                    "•".repeat(confirmation.chars().count())
                )),
                Line::from(format!(
                    "{} {}",
                    if cursor == 2 { ">" } else { " " },
                    if change {
                        "Change password"
                    } else {
                        "Create password"
                    }
                )),
                Line::from("Tab / arrows: move   Enter: next / apply   Esc: cancel"),
                Line::from(Span::styled(
                    error.as_str(),
                    Style::default().fg(Color::Red),
                )),
            ],
        )?;
        match read_event(events).await? {
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Esc => return Ok(false),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(false)
                }
                KeyCode::Tab | KeyCode::Down => cursor = (cursor + 1) % 3,
                KeyCode::BackTab | KeyCode::Up => cursor = (cursor + 2) % 3,
                KeyCode::Enter if cursor < 2 => cursor += 1,
                KeyCode::Enter => {
                    if password.is_empty() {
                        error = "Enter a nonempty new password.".into();
                        cursor = 0;
                    } else if password.as_str() != confirmation.as_str() {
                        error = "Password confirmation does not match.".into();
                        confirmation.zeroize();
                        cursor = 1;
                    } else {
                        let result = if change {
                            super::change_password(password.as_str())
                        } else {
                            super::initialize_password(password.as_str())
                        };
                        match result {
                            Ok(()) => return Ok(true),
                            Err(failure) => {
                                error = format!("Password update failed: {failure}");
                                password.zeroize();
                                confirmation.zeroize();
                                cursor = 0;
                            }
                        }
                    }
                }
                KeyCode::Backspace => {
                    if cursor == 0 {
                        password.pop();
                    } else if cursor == 1 {
                        confirmation.pop();
                    }
                }
                KeyCode::Char(character)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    let mut bytes = [0; 4];
                    let text = character.encode_utf8(&mut bytes);
                    if cursor == 0 {
                        append_secret(&mut password, text);
                    } else if cursor == 1 {
                        append_secret(&mut confirmation, text);
                    }
                    bytes.zeroize();
                }
                _ => {}
            },
            Event::Paste(mut text) => {
                if cursor == 0 {
                    append_secret(&mut password, &text);
                } else if cursor == 1 {
                    append_secret(&mut confirmation, &text);
                }
                text.zeroize();
            }
            _ => {}
        }
    }
}

pub(crate) async fn unlock(
    terminal: &mut crate::TerminalType,
    events: &mut EventStream,
) -> Result<bool> {
    unlock_with_notice(terminal, events, None).await
}

pub(crate) async fn unlock_with_notice(
    terminal: &mut crate::TerminalType,
    events: &mut EventStream,
    notice: Option<&str>,
) -> Result<bool> {
    let mut password = Zeroizing::new(String::with_capacity(MAX_PASSWORD_BYTES));
    let mut error = notice.unwrap_or_default().to_owned();
    loop {
        draw(
            terminal,
            " Unlock application protection ",
            vec![
                Line::from("Enter the shared protection / credential-vault password."),
                Line::from("This unlocks every selected data type for this session."),
                Line::from(""),
                Line::from(format!(
                    "Password: {}",
                    "•".repeat(password.chars().count())
                )),
                Line::from("Enter: unlock   Esc: cancel and return"),
                Line::from(Span::styled(
                    error.as_str(),
                    Style::default().fg(Color::Red),
                )),
            ],
        )?;
        match read_event(events).await? {
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Esc => return Ok(false),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(false)
                }
                KeyCode::Enter => match super::unlock(password.as_str()) {
                    Ok(()) => return Ok(true),
                    Err(failure) => {
                        error = if failure
                            .chain()
                            .any(|cause| cause.is::<super::PasswordRotationUnlockNeeded>())
                        {
                            failure.to_string()
                        } else {
                            format!("Unlock failed: {failure}")
                        };
                        password.zeroize();
                    }
                },
                KeyCode::Backspace => {
                    password.pop();
                }
                KeyCode::Char(character)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    let mut bytes = [0; 4];
                    append_secret(&mut password, character.encode_utf8(&mut bytes));
                    bytes.zeroize();
                }
                _ => {}
            },
            Event::Paste(mut text) => {
                append_secret(&mut password, &text);
                text.zeroize();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independent_choices_do_not_enable_other_domains() {
        for field in 0..5 {
            let mut policy = ProtectionPolicy::default();
            toggle(&mut policy, field);
            assert_eq!(
                [
                    policy.app_password,
                    policy.credentials,
                    policy.chats,
                    policy.memories,
                    policy.vault
                ]
                .into_iter()
                .filter(|value| *value)
                .count(),
                1
            );
            assert!(password_required(policy, false));
            toggle(&mut policy, field);
            assert!(policy == ProtectionPolicy::default());
        }
    }

    #[test]
    fn plaintext_setup_only_skips_password_when_no_existing_credential_password() {
        assert!(!password_required(ProtectionPolicy::default(), false));
        assert!(password_required(ProtectionPolicy::default(), true));
    }

    #[test]
    fn secret_input_is_bounded_in_utf8_bytes_and_excludes_controls() {
        let mut value = Zeroizing::new("a".repeat(MAX_PASSWORD_BYTES - 3));
        append_secret(&mut value, "\n界x");
        assert_eq!(value.len(), MAX_PASSWORD_BYTES);
        assert!(value.ends_with('界'));
        append_secret(&mut value, "more");
        assert_eq!(value.len(), MAX_PASSWORD_BYTES);
    }

    #[test]
    fn continue_keeps_independent_domains_locked_but_enforces_entry_and_recovery() {
        assert!(can_continue(true, false, false, true));
        assert!(!can_continue(true, true, false, true));
        assert!(!can_continue(true, true, true, false));
        assert!(!can_continue(false, false, false, true));
        assert!(can_continue(true, true, true, true));
    }

    #[test]
    fn unlock_preserves_staged_choices_until_authenticated_save() {
        crate::app::tests::with_test_app("protection-manager-staged-save", |_app| {
            let saved = ProtectionPolicy {
                credentials: true,
                ..ProtectionPolicy::default()
            };
            super::super::configure(saved, "manager-password").unwrap();
            super::super::lock().unwrap();
            let mut staged = saved;
            toggle(&mut staged, 4);
            let config = crate::config::AppConfig::default();
            let status = crate::config::KeyStore::vault_status(&config).unwrap();
            assert_eq!(
                save_action(status.initialized, status.unlocked),
                SaveAction::Unlock
            );
            assert!(super::super::unlock("incorrect").is_err());
            assert!(super::super::policy().unwrap().unwrap() == saved);
            super::super::unlock("manager-password").unwrap();
            let status = crate::config::KeyStore::vault_status(&config).unwrap();
            assert_eq!(
                save_action(status.initialized, status.unlocked),
                SaveAction::ConfigureUnlocked
            );
            assert!(super::super::policy().unwrap().unwrap() == saved);
            super::super::configure_unlocked(staged).unwrap();
            assert!(super::super::policy().unwrap().unwrap() == staged);
        });
    }

    #[test]
    fn initialized_legacy_vault_uses_unlock_then_password_free_save() {
        crate::app::tests::with_test_app("protection-manager-legacy-save", |_app| {
            let config = crate::config::AppConfig::default();
            crate::config::KeyStore::initialize_vault(&config, "legacy-password").unwrap();
            crate::config::KeyStore::lock_vault(&config).unwrap();
            assert!(!super::super::configured().unwrap());
            let status = crate::config::KeyStore::vault_status(&config).unwrap();
            assert_eq!(
                save_action(status.initialized, status.unlocked),
                SaveAction::Unlock
            );
            super::super::unlock("legacy-password").unwrap();
            let status = crate::config::KeyStore::vault_status(&config).unwrap();
            assert_eq!(
                save_action(status.initialized, status.unlocked),
                SaveAction::ConfigureUnlocked
            );
            let staged = ProtectionPolicy {
                credentials: true,
                vault: true,
                ..ProtectionPolicy::default()
            };
            super::super::configure_unlocked(staged).unwrap();
            assert!(super::super::policy().unwrap().unwrap() == staged);
        });
    }
}
