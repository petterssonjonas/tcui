use std::path::Path;

use color_eyre::{eyre::eyre, Result};
use crossterm::event::{Event, EventStream, KeyCode, KeyEventKind, KeyModifiers};
use futures::StreamExt;
use ratatui::{prelude::*, widgets::*};
use zeroize::{Zeroize, Zeroizing};

use super::Domain;
use crate::storage::native_transfer::{
    native_root, ExportProtection, NativeTransferService, PreparedTransfer, TransferError,
    TransferMode, TransferRequirement,
};
use crate::tui::components::SearchField;

fn name(domain: Domain) -> &'static str {
    match domain {
        Domain::Vault => "Vault",
        Domain::Memories => "Memories",
        Domain::Chats => "Chats",
    }
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

async fn event(events: &mut EventStream) -> Result<Event> {
    events
        .next()
        .await
        .ok_or_else(|| eyre!("Terminal input closed"))?
        .map_err(Into::into)
}

async fn confirm(
    terminal: &mut crate::TerminalType,
    events: &mut EventStream,
    title: &str,
    lines: Vec<Line<'_>>,
) -> Result<bool> {
    draw(terminal, title, lines)?;
    loop {
        if let Event::Key(key) = event(events).await? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Enter | KeyCode::Char('y' | 'Y') => return Ok(true),
                KeyCode::Esc | KeyCode::Char('n' | 'N') => return Ok(false),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(false)
                }
                _ => {}
            }
        }
    }
}

async fn password(
    terminal: &mut crate::TerminalType,
    events: &mut EventStream,
    title: &str,
    explanation: &str,
) -> Result<Option<Zeroizing<String>>> {
    let mut value = Zeroizing::new(String::new());
    loop {
        draw(
            terminal,
            title,
            vec![
                Line::from(explanation),
                Line::from(""),
                Line::from(format!("Password: {}", "•".repeat(value.chars().count()))),
                Line::from("Enter: unlock / continue   Esc: cancel"),
            ],
        )?;
        match event(events).await? {
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Esc => return Ok(None),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(None)
                }
                KeyCode::Enter => return Ok(Some(value)),
                KeyCode::Backspace => {
                    value.pop();
                }
                KeyCode::Char(ch)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                        && value.len() + ch.len_utf8() <= 4096 =>
                {
                    value.push(ch)
                }
                _ => {}
            },
            Event::Paste(mut text) => {
                for ch in text.chars().filter(|ch| !ch.is_control()) {
                    if value.len() + ch.len_utf8() > 4096 {
                        break;
                    }
                    value.push(ch);
                }
                text.zeroize();
            }
            _ => {}
        }
    }
}

async fn target_password(
    terminal: &mut crate::TerminalType,
    events: &mut EventStream,
) -> Result<Option<Zeroizing<String>>> {
    let status = crate::config::KeyStore::vault_status(&crate::config::AppConfig::default())?;
    let explanation = if status.initialized {
        "Enter this TCUI instance's existing password. The source password does not replace it."
    } else {
        "Choose a TCUI password. Imported encrypted data remains encrypted under this password."
    };
    let Some(value) = password(terminal, events, " Destination protection ", explanation).await?
    else {
        return Ok(None);
    };
    if !status.initialized {
        let Some(confirmation) = password(
            terminal,
            events,
            " Confirm new TCUI password ",
            "Enter the new TCUI password again.",
        )
        .await?
        else {
            return Ok(None);
        };
        if value.as_str() != confirmation.as_str() {
            return Err(eyre!(
                "Password confirmation does not match; no transfer was committed."
            ));
        }
    }
    Ok(Some(value))
}

fn preview(prepared: &PreparedTransfer) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(format!(
            "{}: {} active files, {} retained or recovery files, {} stored bytes",
            name(prepared.domain),
            prepared.files.len(),
            prepared.adoption_files.len(),
            prepared.total_bytes
        )),
        Line::from(format!(
            "Source: {}",
            prepared.source.to_string_lossy().escape_debug()
        )),
        Line::from(if prepared.same_native_root {
            "Adopt this copied native dataset in place. It will remain encrypted."
        } else if prepared.mode == TransferMode::Replace {
            "Replace the dataset. Previous data is retained in a private recovery backup."
        } else {
            "Add entries. Conflicting names or identities keep both versions."
        }),
        Line::from(""),
    ];
    for file in prepared
        .files
        .iter()
        .chain(&prepared.adoption_files)
        .take(12)
    {
        lines.push(Line::from(format!(
            "{}{}{}",
            file.relative_path.to_string_lossy().escape_debug(),
            if file.encrypted { " [encrypted]" } else { "" },
            if file.conflicts { " [conflict]" } else { "" }
        )));
    }
    let count = prepared.files.len() + prepared.adoption_files.len();
    if count > 12 {
        lines.push(Line::from(format!("... {} more files", count - 12)));
    }
    lines.push(Line::from(""));
    lines.push(Line::from("Enter/y: confirm   Esc/n: cancel"));
    lines
}

/// Only unlocks source wrappers. It does not change the destination policy or files.
pub(crate) async fn unlock_native_sources(
    terminal: &mut crate::TerminalType,
    events: &mut EventStream,
    domain: Domain,
) -> Result<bool> {
    let root = native_root(domain)?;
    loop {
        let prepared = NativeTransferService::prepare(domain, &root, TransferMode::Add)?;
        if let Some(TransferRequirement::LegacySourceWrapping { path }) =
            prepared.requirements.iter().find(|requirement| {
                matches!(
                    requirement,
                    TransferRequirement::LegacySourceWrapping { .. }
                )
            })
        {
            return Err(eyre!("{} needs the original instance's wrapping metadata or legacy chat.key. Unlock and upgrade it on the source instance before copying it.", path.to_string_lossy().escape_debug()));
        }
        let Some(TransferRequirement::SourcePassword { path }) = prepared
            .requirements
            .iter()
            .find(|requirement| matches!(requirement, TransferRequirement::SourcePassword { .. }))
        else {
            return Ok(true);
        };
        let explanation = format!(
            "TCUI encrypted file found: {}. Enter the source instance's password.",
            path.to_string_lossy().escape_debug()
        );
        let Some(secret) =
            password(terminal, events, " Unlock copied TCUI data ", &explanation).await?
        else {
            return Ok(false);
        };
        if let Err(error) = prepared.unlock_source(secret.as_str()) {
            if !confirm(
                terminal,
                events,
                " Source unlock failed ",
                vec![
                    Line::from(error.to_string()),
                    Line::from("Enter/y: try again   Esc/n: cancel"),
                ],
            )
            .await?
            {
                return Ok(false);
            }
        }
    }
}

async fn import(
    terminal: &mut crate::TerminalType,
    events: &mut EventStream,
    domain: Domain,
    path: &Path,
    mode: TransferMode,
) -> Result<String> {
    loop {
        let prepared = NativeTransferService::prepare(domain, path, mode)?;
        if let Some(requirement) = prepared.requirements.iter().find(|requirement| {
            matches!(
                requirement,
                TransferRequirement::LegacySourceWrapping { .. }
            )
        }) {
            return Err(eyre!("{requirement:?}: upgrade these legacy files on the source instance with its original wrapping metadata or chat.key first. Source data was not changed."));
        }
        if let Some(TransferRequirement::SourcePassword { path }) = prepared
            .requirements
            .iter()
            .find(|requirement| matches!(requirement, TransferRequirement::SourcePassword { .. }))
        {
            let explanation = format!(
                "Unlock {} with its source password. No provider credentials are imported.",
                path.to_string_lossy().escape_debug()
            );
            let Some(secret) =
                password(terminal, events, " Unlock source data ", &explanation).await?
            else {
                return Ok("Import cancelled.".into());
            };
            if let Err(error) = prepared.unlock_source(secret.as_str()) {
                if !confirm(
                    terminal,
                    events,
                    " Source unlock failed ",
                    vec![
                        Line::from(error.to_string()),
                        Line::from("Enter/y: try again   Esc/n: cancel"),
                    ],
                )
                .await?
                {
                    return Ok("Import cancelled.".into());
                }
            }
            continue;
        }
        if prepared.same_native_root {
            if !super::is_protected(domain)?
                && !prepared
                    .files
                    .iter()
                    .chain(&prepared.adoption_files)
                    .any(|file| file.encrypted)
            {
                return Err(eyre!("This is already the active plaintext dataset. Choose another source path to add or replace entries."));
            }
            if !confirm(
                terminal,
                events,
                " Adopt native TCUI data ",
                preview(&prepared),
            )
            .await?
            {
                return Ok("Adoption cancelled.".into());
            }
            let secret =
                if crate::config::KeyStore::vault_status(&crate::config::AppConfig::default())?
                    .unlocked
                {
                    None
                } else {
                    let Some(secret) = target_password(terminal, events).await? else {
                        return Ok("Adoption cancelled.".into());
                    };
                    Some(secret)
                };
            draw(
                terminal,
                " Adopting native data ",
                vec![Line::from(
                    "Applying authenticated protection and durable migration...",
                )],
            )?;
            let report = match secret {
                Some(secret) => NativeTransferService::adopt_native(prepared, secret.as_str())?,
                None => NativeTransferService::adopt_native_unlocked(prepared)?,
            };
            return Ok(format!(
                "{} adopted. {} entries retained.",
                name(domain),
                report.imported
            ));
        }
        if prepared
            .requirements
            .iter()
            .any(|requirement| matches!(requirement, TransferRequirement::EnableProtection { .. }))
        {
            let explanation = format!("The source contains encrypted {} data. Enable encryption for the destination {} before importing? Other protection choices remain unchanged.", name(domain), name(domain));
            if !confirm(
                terminal,
                events,
                " Keep imported data encrypted ",
                vec![
                    Line::from(explanation),
                    Line::from("Enter/y: enable encryption   Esc/n: cancel"),
                ],
            )
            .await?
            {
                return Ok(
                    "Import cancelled; encrypted data was not converted to plaintext.".into(),
                );
            }
            if crate::config::KeyStore::vault_status(&crate::config::AppConfig::default())?.unlocked
            {
                NativeTransferService::enable_protection_unlocked(domain)?;
            } else {
                let Some(secret) = target_password(terminal, events).await? else {
                    return Ok("Import cancelled.".into());
                };
                NativeTransferService::enable_protection(domain, secret.as_str())?;
            }
            continue;
        }
        if !confirm(
            terminal,
            events,
            " Confirm native import ",
            preview(&prepared),
        )
        .await?
        {
            return Ok("Import cancelled.".into());
        }
        draw(
            terminal,
            " Importing native data ",
            vec![Line::from(
                "Checking source hashes and writing the confirmed transfer...",
            )],
        )?;
        let report = prepared.commit()?;
        return Ok(format!(
            "Imported {} entries.{}",
            report.imported,
            report.backup.map_or(String::new(), |path| format!(
                " Previous data retained at {}.",
                path.to_string_lossy().escape_debug()
            ))
        ));
    }
}

/// Runs after the app, background tasks, and long-lived store guards have been dropped.
pub(crate) async fn run(
    terminal: &mut crate::TerminalType,
    events: &mut EventStream,
    domain: Domain,
) -> Result<()> {
    let root = native_root(domain)?;
    let root_text = root.to_string_lossy().into_owned();
    let mut input = SearchField::from_text(root_text.clone());
    let mut mode = 0;
    let mut notice = String::new();
    if super::require(domain).is_err() && !super::ui::unlock(terminal, events).await? {
        return Ok(());
    }
    if let Ok(prepared) = NativeTransferService::prepare(domain, &root, TransferMode::Add) {
        if prepared.requirements.iter().any(|requirement| {
            matches!(
                requirement,
                TransferRequirement::SourcePassword { .. }
                    | TransferRequirement::EnableProtection { .. }
            )
        }) {
            notice = match import(terminal, events, domain, &root, TransferMode::Add).await {
                Ok(message) => message,
                Err(error) => error.to_string(),
            };
        }
    }
    const MODES: [&str; 5] = [
        "Add / adopt",
        "Replace dataset",
        "Export portable",
        "Export plaintext",
        "Restore backup",
    ];
    loop {
        draw(
            terminal,
            &format!(" {} transfer ", name(domain)),
            vec![
                Line::from("Imports copy files into this instance's Vault or Memories."),
                Line::from(
                    "Encrypted imports remain encrypted. Provider credentials are not transferred.",
                ),
                Line::from(""),
                Line::from("Source / export destination / recovery backup:"),
                Line::from(input.query()),
                Line::from(""),
                Line::from(format!("Mode: {}", MODES[mode])),
                Line::from("Tab / Shift+Tab: change mode   Enter: preview   Esc: return to app"),
                Line::from(
                    "Use the native folder path above to adopt copied encrypted dotfiles in place.",
                ),
                Line::from(""),
                Line::from(notice.as_str()),
            ],
        )?;
        match event(events).await? {
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Esc => return Ok(()),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(())
                }
                KeyCode::Tab => mode = (mode + 1) % MODES.len(),
                KeyCode::BackTab => mode = (mode + MODES.len() - 1) % MODES.len(),
                KeyCode::Left => input.left(),
                KeyCode::Right => input.right(),
                KeyCode::Home => input.home(),
                KeyCode::End => input.end(),
                KeyCode::Backspace => input.backspace(),
                KeyCode::Delete => input.delete(),
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.clear()
                }
                KeyCode::Char(ch)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                        && input.len() < 4096 =>
                {
                    let mut encoded = [0; 4];
                    input.insert(ch.encode_utf8(&mut encoded));
                }
                KeyCode::Enter => {
                    if super::require(domain).is_err()
                        && !super::ui::unlock(terminal, events).await?
                    {
                        notice = "Destination unlock cancelled.".into();
                        continue;
                    }
                    let path = if input.query() == root_text {
                        root.clone()
                    } else {
                        crate::app::generated_file::expand_user_path(
                            Path::new(input.query().trim()),
                            dirs::home_dir().as_deref(),
                        )
                    };
                    let outcome = match mode {
                        0 | 1 => import(terminal, events, domain, &path, if mode == 0 { TransferMode::Add } else { TransferMode::Replace }).await,
                        2 | 3 => {
                            let plaintext = mode == 3;
                            let warning = if plaintext { "Export decrypted PLAINTEXT outside TCUI protection? This can expose all entry content and metadata." } else { "Export native files for another TCUI instance? Encrypted files keep their portable password wrapping." };
                            if confirm(terminal, events, " Confirm export ", vec![Line::from(warning), Line::from(format!("Destination: {}", path.to_string_lossy().escape_debug())), Line::from("Enter/y: export   Esc/n: cancel")]).await? {
                                NativeTransferService::export(domain, &path, if plaintext { ExportProtection::PlaintextConfirmed } else { ExportProtection::Preserve }).map(|report| format!("Exported {} entries to {}.", report.exported, path.to_string_lossy().escape_debug())).map_err(Into::into)
                            } else { Ok("Export cancelled.".into()) }
                        }
                        _ => {
                            if confirm(terminal, events, " Restore retained backup ", vec![Line::from("Restore this completed replacement backup? Current data is retained separately."), Line::from(path.to_string_lossy().escape_debug().to_string()), Line::from("Enter/y: restore   Esc/n: cancel")]).await? {
                                NativeTransferService::restore_backup(domain, &path).map(|displaced| format!("Backup restored. Displaced data retained at {}.", displaced.to_string_lossy().escape_debug())).map_err(Into::into)
                            } else { Ok("Restore cancelled.".into()) }
                        }
                    };
                    notice = match outcome {
                        Ok(message) => message,
                        Err(error) => {
                            let partial = error.chain().any(|cause| {
                                matches!(
                                    cause.downcast_ref::<TransferError>(),
                                    Some(TransferError::CommitIncomplete { .. })
                                )
                            });
                            format!(
                                "{error}{}",
                                if partial {
                                    " Some writes may already be committed. Check the retained backup and entries before retrying Add."
                                } else {
                                    ""
                                }
                            )
                        }
                    };
                }
                _ => {}
            },
            Event::Paste(text) => {
                let clean: String = text
                    .chars()
                    .filter(|ch| !ch.is_control())
                    .take(4096_usize.saturating_sub(input.len()))
                    .collect();
                input.insert(&clean);
            }
            _ => {}
        }
    }
}

/// Authenticate every copied store before changing either store's files.
pub(crate) async fn prepare_configuration_sources(
    terminal: &mut crate::TerminalType,
    events: &mut EventStream,
    desired: super::ProtectionPolicy,
    destination_password: Option<&str>,
) -> Result<Option<Vec<Domain>>> {
    let mut domains = Vec::new();
    for domain in [Domain::Vault, Domain::Memories] {
        #[cfg(not(feature = "memory"))]
        if domain == Domain::Memories {
            continue;
        }
        loop {
            match super::preflight_native_adoption(domain) {
                Ok(()) => break,
                Err(error) => {
                    let needs_source = error
                        .chain()
                        .any(|cause| cause.is::<super::PortableUnlockNeeded>());
                    let needs_adoption = error
                        .chain()
                        .any(|cause| cause.is::<super::NativeStoreAdoptionNeeded>());
                    if !needs_source && !needs_adoption {
                        return Err(error);
                    }
                    match destination_password {
                        Some("") => {
                            return Err(eyre!("Enter a destination TCUI password before adopting copied encrypted files. This is required even if the final choices decrypt those files."));
                        }
                        None if !crate::config::KeyStore::vault_status(
                            &crate::config::AppConfig::default(),
                        )?
                        .unlocked =>
                        {
                            return Err(eyre!(
                                "Unlock this TCUI instance before adopting copied encrypted files."
                            ));
                        }
                        _ => {}
                    }
                    if needs_source {
                        if !unlock_native_sources(terminal, events, domain).await? {
                            return Ok(None);
                        }
                    } else {
                        domains.push(domain);
                        break;
                    }
                }
            }
        }
    }
    if domains.is_empty() {
        return Ok(Some(domains));
    }
    let mut lines = vec![
        Line::from("Copied encrypted TCUI files were authenticated."),
        Line::from("Adopt them under this instance's password before applying the selected protection choices?"),
    ];
    for domain in &domains {
        lines.push(Line::from(format!(
            "{}: {}",
            name(*domain),
            if desired.protects(*domain) {
                "keep encrypted"
            } else {
                "the selected choices will DECRYPT this dataset to PLAINTEXT"
            }
        )));
    }
    lines.push(Line::from(
        "Enter/y: adopt and apply these choices   Esc/n: cancel",
    ));
    if !confirm(terminal, events, " Confirm copied data protection ", lines).await? {
        return Ok(None);
    }
    for domain in &domains {
        let prepared =
            NativeTransferService::prepare(*domain, &native_root(*domain)?, TransferMode::Add)?;
        match destination_password {
            Some(password) => {
                NativeTransferService::adopt_native(prepared, password)?;
            }
            None => {
                NativeTransferService::adopt_native_unlocked(prepared)?;
            }
        }
    }
    Ok(Some(domains))
}
