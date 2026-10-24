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

fn password_required(policy: ProtectionPolicy, initialized: bool) -> bool {
    initialized
        || policy.app_password
        || policy.credentials
        || policy.chats
        || policy.memories
        || policy.vault
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
    let mut password = Zeroizing::new(String::new());
    let mut confirmation = Zeroizing::new(String::new());
    let mut cursor = 0;
    let mut error = String::new();
    loop {
        let mut lines = vec![
            Line::from("Choose protection independently for each app-owned data type."),
            Line::from(
                "Unchecked data remains plaintext. An entry password alone does NOT encrypt it.",
            ),
            Line::from(
                "Vault and Memories are separate, app-owned stores. There are no external vaults.",
            ),
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
        lines.push(Line::from(if status.initialized {
            "Use your EXISTING credential-vault password. It will not be replaced."
        } else {
            "Choose one password for entry and every encrypted data type."
        }));
        lines.push(Line::from(format!(
            "{} Password: {}",
            if cursor == 5 { ">" } else { " " },
            "•".repeat(password.chars().count())
        )));
        lines.push(Line::from(format!(
            "{} Confirm:  {}",
            if cursor == 6 { ">" } else { " " },
            "•".repeat(confirmation.chars().count())
        )));
        lines.push(Line::from(format!(
            "{} Save protection choices",
            if cursor == 7 { ">" } else { " " }
        )));
        lines.push(Line::from(
            "Tab / arrows: move   Space: toggle   Enter: next / save   Esc: cancel",
        ));
        lines.push(Line::from(
            "Ctrl+U: unlock / recover the saved protection setup",
        ));
        lines.push(Line::from(Span::styled(
            error.as_str(),
            Style::default().fg(Color::Red),
        )));
        draw(terminal, " Application protection ", lines)?;
        match read_event(events).await? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                match key.code {
                    KeyCode::Esc => return Ok(false),
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(false)
                    }
                    KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        password.zeroize();
                        confirmation.zeroize();
                        if super::configured()? {
                            if unlock(terminal, events).await? {
                                return Ok(true);
                            }
                        } else {
                            error = "No saved protection setup needs recovery; choose options and save.".into();
                        }
                    }
                    KeyCode::Tab | KeyCode::Down => cursor = (cursor + 1) % 8,
                    KeyCode::BackTab | KeyCode::Up => cursor = (cursor + 7) % 8,
                    KeyCode::Char(' ') if cursor < 5 => toggle(&mut policy, cursor),
                    KeyCode::Enter if cursor < 7 => cursor += 1,
                    KeyCode::Enter => {
                        if password_required(policy, status.initialized) && password.is_empty() {
                            error = "Enter the password before saving.".into();
                            cursor = 5;
                        } else if password.as_str() != confirmation.as_str() {
                            error = "Password confirmation does not match.".into();
                            confirmation.zeroize();
                            cursor = 6;
                        } else {
                            let setup = match super::transfer_ui::prepare_configuration_sources(
                                terminal,
                                events,
                                policy,
                                password.as_str(),
                            )
                            .await
                            {
                                Ok(Some(_)) => super::configure(policy, password.as_str()),
                                Ok(None) => {
                                    error = "Source adoption cancelled. No new protection choices were applied.".into();
                                    continue;
                                }
                                Err(failure) => Err(failure),
                            };
                            match setup {
                                Ok(()) => return Ok(true),
                                Err(failure) => {
                                    status = crate::config::KeyStore::vault_status(
                                        &crate::config::AppConfig::default(),
                                    )?;
                                    if let Some(domain) = failure.chain().find_map(|cause| {
                                        cause
                                            .downcast_ref::<super::PortableUnlockNeeded>()
                                            .map(|needed| needed.domain)
                                    }) {
                                        let local_ready = !super::configured()?
                                            || super::is_unlocked()?
                                            || super::unlock(password.as_str()).is_ok();
                                        if local_ready {
                                            match super::transfer_ui::unlock_native_sources(
                                                terminal, events, domain,
                                            )
                                            .await
                                            {
                                                Ok(true) => {
                                                    error = "Source files unlocked. Save the selected protection choices to continue.".into();
                                                    continue;
                                                }
                                                Ok(false) => {}
                                                Err(source_error) => {
                                                    error = format!("Source unlock did not complete: {source_error}");
                                                    password.zeroize();
                                                    confirmation.zeroize();
                                                    cursor = 5;
                                                    continue;
                                                }
                                            }
                                        }
                                    }
                                    error = format!("Protection setup did not complete; unlock to recover if prompted: {failure}");
                                    password.zeroize();
                                    confirmation.zeroize();
                                    cursor = 5;
                                }
                            }
                        }
                    }
                    KeyCode::Backspace if cursor == 5 => {
                        password.pop();
                    }
                    KeyCode::Backspace if cursor == 6 => {
                        confirmation.pop();
                    }
                    KeyCode::Char(character)
                        if !key
                            .modifiers
                            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                    {
                        if cursor == 5 {
                            password.push(character);
                        }
                        if cursor == 6 {
                            confirmation.push(character);
                        }
                    }
                    _ => {}
                }
            }
            Event::Paste(mut text) => {
                let destination = if cursor == 5 {
                    Some(&mut password)
                } else if cursor == 6 {
                    Some(&mut confirmation)
                } else {
                    None
                };
                if let Some(destination) = destination {
                    destination.extend(text.chars().filter(|character| !character.is_control()));
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
    let mut password = Zeroizing::new(String::new());
    let mut confirmation = Zeroizing::new(String::new());
    let mut cursor = 0;
    let mut error = String::new();
    loop {
        draw(
            terminal,
            " Initialize application password ",
            vec![
                Line::from("Create the shared application / credential-vault password."),
                Line::from("Existing plaintext connections and data remain plaintext."),
                Line::from("Protection choices and the new-connection default stay unchanged."),
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
                    "{} Create password",
                    if cursor == 2 { ">" } else { " " }
                )),
                Line::from("Tab / arrows: move   Enter: next / create   Esc: cancel"),
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
                        error = "Enter a password before creating it.".into();
                        cursor = 0;
                    } else if password.as_str() != confirmation.as_str() {
                        error = "Password confirmation does not match.".into();
                        confirmation.zeroize();
                        cursor = 1;
                    } else {
                        match super::initialize_password(password.as_str()) {
                            Ok(()) => return Ok(true),
                            Err(failure) => {
                                error = format!("Password initialization failed: {failure}");
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
                    if cursor == 0 {
                        password.push(character);
                    } else if cursor == 1 {
                        confirmation.push(character);
                    }
                }
                _ => {}
            },
            Event::Paste(mut text) => {
                if cursor == 0 {
                    password.extend(text.chars().filter(|character| !character.is_control()));
                } else if cursor == 1 {
                    confirmation.extend(text.chars().filter(|character| !character.is_control()));
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
    let mut password = Zeroizing::new(String::new());
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
                Line::from("Enter: unlock   Esc: cancel (mandatory startup unlock exits the app)"),
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
                        error = format!("Unlock failed: {failure}");
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
                    password.push(character)
                }
                _ => {}
            },
            Event::Paste(mut text) => {
                password.extend(text.chars().filter(|character| !character.is_control()));
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
}
