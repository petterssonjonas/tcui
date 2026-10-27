use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use color_eyre::Result;
use crossterm::{
    cursor::SetCursorStyle,
    event::{
        DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
        EnableFocusChange, EnableMouseCapture, EventStream, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{
        disable_raw_mode, enable_raw_mode, Clear, ClearType, EnterAlternateScreen,
        LeaveAlternateScreen, SetTitle,
    },
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::sync::Arc;
use tokio::sync::RwLock;

mod app;
mod auth_command;
mod clipboard;
mod commands;
mod config;
mod diagnostics;
mod event;
mod export;
mod llm;
mod mcp;
#[cfg(feature = "memory")]
mod memory;
mod notifications;
mod protection;
mod reminders;
mod search;
mod skill_runtime;
mod skills;
mod storage;
#[cfg(test)]
mod test_support;
mod theme;
mod tui;
mod ui;
mod updater;
pub mod vault;

use app::TuiApp;
use config::AppConfig;
use llm::LlmClient;
use storage::Storage;

pub type Backend = CrosstermBackend<io::Stdout>;
pub type TerminalType = Terminal<Backend>;

#[derive(Debug, Parser)]
#[command(name = "tcui")]
struct Cli {
    #[arg(long, default_value_t = false)]
    upgrade: bool,
    #[arg(long)]
    decrypt: Option<PathBuf>,
    #[cfg(feature = "memory")]
    #[arg(long = "add-memory")]
    add_memory: Option<PathBuf>,
    #[arg(long = "export-all-chats", default_value_t = false)]
    export_all_chats: bool,
    #[cfg(feature = "memory")]
    #[arg(long = "export-all-memories", default_value_t = false)]
    export_all_memories: bool,
    #[arg(long)]
    key: Option<PathBuf>,
    #[arg(long, default_value_t = false)]
    json: bool,
    #[arg(long, default_value_t = false)]
    markdown: bool,
    /// Unlock protected content using one password line from redirected stdin.
    #[arg(long, global = true)]
    unlock_password_stdin: bool,
    /// Read one password line from a dedicated inherited descriptor (>=3), leaving MCP stdin untouched.
    #[arg(long, global = true, conflicts_with = "unlock_password_stdin")]
    unlock_password_fd: Option<u32>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    Auth(auth_command::AuthCommandArgs),
    Upgrade,
    #[command(hide = true)]
    ReminderDispatch {
        id: String,
    },
    #[command(hide = true)]
    ReminderPingDispatch {
        id: String,
    },
    #[command(hide = true)]
    ReminderCleanup {
        id: String,
    },
    #[cfg(feature = "memory")]
    MemoryMcp,
}

fn interactive_terminal_available() -> bool {
    io::stdin().is_terminal() && io::stdout().is_terminal() && io::stderr().is_terminal()
}

fn setup_terminal() -> Result<TerminalType> {
    let is_tty = interactive_terminal_available();

    if is_tty {
        enable_raw_mode()?;
    }

    let backend = Backend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    if is_tty {
        let _ = execute!(terminal.backend_mut(), EnterAlternateScreen);
        let _ = execute!(terminal.backend_mut(), SetTitle("TermChatUI"));
        let _ = execute!(terminal.backend_mut(), EnableMouseCapture);
        let _ = execute!(terminal.backend_mut(), EnableFocusChange);
        let _ = execute!(terminal.backend_mut(), EnableBracketedPaste);
        let _ = execute!(terminal.backend_mut(), SetCursorStyle::SteadyBar);
        let _ = execute!(
            terminal.backend_mut(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::REPORT_EVENT_TYPES)
        );
    }

    Ok(terminal)
}

fn clear_terminal_surface(terminal: &mut TerminalType) -> Result<()> {
    // Terminal::clear queries cursor position, but input belongs to the event stream.
    execute!(terminal.backend_mut(), Clear(ClearType::All))?;
    terminal.draw(|frame| frame.render_widget(ratatui::widgets::Clear, frame.area()))?;
    Ok(())
}

fn restore_terminal() {
    let is_tty = interactive_terminal_available();

    if is_tty {
        let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
        let _ = execute!(io::stdout(), DisableFocusChange);
        let _ = execute!(io::stdout(), DisableBracketedPaste);
        let _ = execute!(io::stdout(), DisableMouseCapture);
        let _ = execute!(io::stdout(), SetCursorStyle::DefaultUserShape);
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        let _ = disable_raw_mode();
    }
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_terminal();
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let offline = OfflineAction::from_cli(&cli)?;
    prepare_unlock_transport(&cli, offline.is_some())?;
    if let Some(operation) = offline {
        unlock_headless(&cli, false)?;
        let _session = protection::session_guard()?;
        return run_offline_action(operation);
    }
    if cli.upgrade {
        println!("{}", updater::upgrade_to_latest().await?);
        return Ok(());
    }
    if let Some(command) = cli.command {
        match command {
            Command::Auth(auth) => {
                prepare_auth_unlock(&auth, cli.unlock_password_stdin, cli.unlock_password_fd)?;
                return auth_command::run(auth).await;
            }
            Command::Upgrade => {
                println!("{}", updater::upgrade_to_latest().await?);
                return Ok(());
            }
            Command::ReminderDispatch { id } => {
                unlock_headless_options(cli.unlock_password_stdin, cli.unlock_password_fd, false)?;
                let _session = protection::session_guard()?;
                protection::require_app()?;
                return reminders::dispatch(&AppConfig::load()?, &id).await;
            }
            Command::ReminderPingDispatch { id } => {
                // This path consumes only an explicitly prepared notification job.
                // It cannot unlock or read protected application data.
                return reminders::dispatch_ping(&id).await;
            }
            Command::ReminderCleanup { id } => return reminders::cleanup(&id),
            #[cfg(feature = "memory")]
            Command::MemoryMcp => {
                unlock_headless_options(cli.unlock_password_stdin, cli.unlock_password_fd, true)?;
                let _session = protection::session_guard()?;
                protection::require_app()?;
                require_headless_domain(protection::Domain::Memories)?;
                return memory::run_mcp().await;
            }
        }
    }

    if !interactive_terminal_available() {
        println!("TermChatUI requires an interactive terminal.");
        return Ok(());
    }

    // The guard exists before setup so partially completed terminal setup is restored too.
    let _terminal_guard = TerminalGuard;
    restore_on_drop();
    let mut terminal = setup_terminal()?;
    ui::components::terminal_capabilities::initialize_terminal_profile();
    let mut events = EventStream::new();
    let vault_status = config::KeyStore::vault_status(&AppConfig::default())?;
    let show_protection = !protection::configured()?
        || vault_status.initialized
        || protection::policy()?.is_some_and(|policy| policy.any())
        || protection::startup_requires_unlock()?;
    if show_protection && !protection::ui::configure(&mut terminal, &mut events).await? {
        return Ok(());
    }
    let mut restart = None;
    let mut security_notice = None;
    let mut pushover_credential = None;
    loop {
        let mut unlock_completed = false;
        match restart.take() {
            Some(protection::ui::SecurityRestart::Configure) => {
                protection::ui::configure(&mut terminal, &mut events).await?;
            }
            Some(protection::ui::SecurityRestart::Unlock) => {
                unlock_completed = protection::ui::unlock(&mut terminal, &mut events).await?;
            }
            Some(protection::ui::SecurityRestart::InitializePassword) => {
                unlock_completed =
                    protection::ui::initialize_password(&mut terminal, &mut events).await?;
            }
            Some(protection::ui::SecurityRestart::Lock) => protection::lock()?,
            Some(protection::ui::SecurityRestart::Transfer(domain)) => {
                protection::transfer_ui::run(&mut terminal, &mut events, domain).await?;
            }
            None => {}
        }
        if protection::startup_requires_unlock()?
            && (!protection::is_unlocked()? || protection::require_app().is_err())
        {
            if !protection::ui::unlock_with_notice(
                &mut terminal,
                &mut events,
                security_notice.as_deref(),
            )
            .await?
            {
                return Ok(());
            }
            unlock_completed = true;
        }
        // Recovery runs without store/session guards. A locked pending transfer
        // must be unlocked before partially replaced native data can be reopened.
        protection::require_app()?;
        for domain in [protection::Domain::Vault, protection::Domain::Memories] {
            #[cfg(not(feature = "memory"))]
            if domain == protection::Domain::Memories {
                continue;
            }
            loop {
                match storage::native_transfer::NativeTransferService::recover_pending(domain) {
                    Ok(_) => break,
                    Err(error) if protection::require(domain).is_err() => {
                        if !protection::ui::unlock_with_notice(
                            &mut terminal,
                            &mut events,
                            Some(&error.to_string()),
                        )
                        .await?
                        {
                            return Ok(());
                        }
                    }
                    Err(error) => return Err(error.into()),
                }
            }
        }
        let session = protection::session_guard()?;
        // No application configuration initialization, stores, or background tasks precede the gate.
        let config = AppConfig::load()?;
        recover_vault_moves()?;
        let storage = Storage::new()?;
        let mut app = TuiApp::new(
            storage,
            Arc::new(RwLock::new(config)),
            Arc::new(LlmClient::new()),
        );
        app.queue_update_check();
        app.ui.show_toast(
            "Welcome to TCUI.\n/security: protection   /unlock: unlock   /help: help".to_string(),
        );
        if let Some(notice) = security_notice.take() {
            app.ui.show_toast(notice);
        }
        if let Some(target) = pushover_credential.take() {
            if unlock_completed {
                app.resume_pushover_credential(target);
            }
        }
        let result = loop {
            let result = app.run(&mut terminal, &mut events).await;
            if let Err(error) = &result {
                if app.recover_security_restart(error) {
                    continue;
                }
            }
            break result;
        };
        pushover_credential = app.take_pushover_credential_restart();
        restart = app.security_restart.take();
        security_notice = app.security_notice.take();
        let forced_lock =
            security_notice.is_some() && restart == Some(protection::ui::SecurityRestart::Lock);
        drop(app);
        drop(session);
        // Clear the old frame before any lock/configuration screen can be displayed.
        clear_terminal_surface(&mut terminal)?;
        if let Err(error) = result {
            if !forced_lock {
                return Err(error);
            }
            let notice = security_notice.get_or_insert_with(String::new);
            notice.push_str(&format!(
                "\nSome active work could not be saved after key revocation: {error}"
            ));
        }
        if restart.is_none() {
            return Ok(());
        }
    }
}

fn recover_vault_moves() -> Result<()> {
    // Protected journals cannot be recovered or pruned while locked. Leave them untouched
    // until the unlock restart; journal guards still refuse journal-backed mutations meanwhile.
    if protection::is_protected(protection::Domain::Vault)? && !protection::is_unlocked()? {
        return Ok(());
    }
    let data_root = crate::storage::paths::TcuiDataPaths::discover().root;
    let report = crate::vault::move_recovery::MoveRecoveryService::at_data_root(
        &data_root,
        crate::vault::move_recovery::MoveRecoveryConfig::default(),
    )
    .recover_startup()?;
    let blocked = report.blocked();
    for line in report.log_lines() {
        eprintln!("{line}");
    }
    if blocked {
        eprintln!("vault.move_recovery status=blocked");
    }
    Ok(())
}

fn restore_on_drop() {
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        original_hook(info);
    }));
}

enum OfflineAction {
    Decrypt {
        path: PathBuf,
        key_path: Option<PathBuf>,
        format: export::OutputFormat,
    },
    #[cfg(feature = "memory")]
    AddMemory {
        source: PathBuf,
        key_path: Option<PathBuf>,
    },
    ExportAll {
        key_path: Option<PathBuf>,
        format: export::OutputFormat,
        chats: bool,
        #[cfg(feature = "memory")]
        memories: bool,
    },
}

impl OfflineAction {
    fn from_cli(cli: &Cli) -> Result<Option<Self>> {
        let format = match (cli.json, cli.markdown) {
            (true, true) => {
                return Err(color_eyre::eyre::eyre!(
                    "--json and --markdown are mutually exclusive"
                ));
            }
            (true, false) => export::OutputFormat::Json,
            _ => export::OutputFormat::Markdown,
        };

        let export_any = {
            #[cfg(feature = "memory")]
            {
                cli.export_all_chats || cli.export_all_memories
            }
            #[cfg(not(feature = "memory"))]
            {
                cli.export_all_chats
            }
        };

        #[cfg(feature = "memory")]
        let direct_modes = usize::from(cli.decrypt.is_some())
            + usize::from(export_any)
            + usize::from(cli.add_memory.is_some());
        #[cfg(not(feature = "memory"))]
        let direct_modes = usize::from(cli.decrypt.is_some()) + usize::from(export_any);
        if direct_modes > 1 {
            return Err(color_eyre::eyre::eyre!(
                "choose only one of --decrypt, --add-memory, or export-all flags"
            ));
        }

        if let Some(path) = cli.decrypt.clone() {
            return Ok(Some(Self::Decrypt {
                path,
                key_path: cli.key.clone(),
                format,
            }));
        }

        #[cfg(feature = "memory")]
        if let Some(source) = cli.add_memory.clone() {
            return Ok(Some(Self::AddMemory {
                source,
                key_path: cli.key.clone(),
            }));
        }

        if export_any {
            return Ok(Some(Self::ExportAll {
                key_path: cli.key.clone(),
                format,
                chats: cli.export_all_chats,
                #[cfg(feature = "memory")]
                memories: cli.export_all_memories,
            }));
        }

        Ok(None)
    }
}

fn prepare_unlock_transport(cli: &Cli, offline: bool) -> Result<()> {
    if matches!(&cli.command, Some(Command::Auth(auth)) if auth.vault_password_stdin)
        && (cli.unlock_password_stdin || cli.unlock_password_fd.is_some())
    {
        return Err(color_eyre::eyre::eyre!(
            "Choose either --vault-password-stdin or one global --unlock-password transport, not both."
        ));
    }
    if let Some(fd) = cli.unlock_password_fd {
        if fd < 3 {
            return Err(color_eyre::eyre::eyre!(
                "--unlock-password-fd requires a dedicated inherited descriptor >=3"
            ));
        }
        #[cfg(unix)]
        {
            let descriptor = libc::c_int::try_from(fd)?;
            // SAFETY: fcntl accepts descriptor numbers, validates them in the kernel,
            // and neither takes ownership nor accesses Rust memory.
            let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFD) };
            if flags < 0 {
                return Err(io::Error::last_os_error().into());
            }
            // Preserve descriptor flags while preventing every later exec from inheriting
            // the original input, not merely the separately opened password reader.
            if unsafe { libc::fcntl(descriptor, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0 {
                return Err(io::Error::last_os_error().into());
            }
        }
        #[cfg(not(unix))]
        return Err(color_eyre::eyre::eyre!(
            "Inherited password descriptors are unsupported on this platform"
        ));
    }
    let auth_password_stdin = matches!(
        &cli.command,
        Some(Command::Auth(auth)) if auth.vault_password_stdin
    );
    if !cli.unlock_password_stdin && cli.unlock_password_fd.is_none() && !auth_password_stdin {
        return Ok(());
    }
    let consumes_password = if auth_password_stdin {
        !offline
            && !cli.upgrade
            && matches!(&cli.command, Some(Command::Auth(auth)) if auth.uses_tcui_store())
    } else {
        offline
            || (!cli.upgrade
                && match &cli.command {
                    Some(Command::Auth(auth)) => auth.uses_tcui_store(),
                    Some(Command::ReminderDispatch { .. }) => true,
                    #[cfg(feature = "memory")]
                    Some(Command::MemoryMcp) => true,
                    _ => false,
                })
    };
    if !consumes_password {
        return Err(color_eyre::eyre::eyre!(
            "Password transports require a headless data or TCUI authentication operation; they are not accepted for interactive startup, upgrades, or external logout."
        ));
    }
    Ok(())
}

fn prepare_auth_unlock(
    auth: &auth_command::AuthCommandArgs,
    stdin: bool,
    fd: Option<u32>,
) -> Result<()> {
    if auth.uses_tcui_store() && (stdin || fd.is_some()) {
        unlock_headless_options(stdin, fd, false)?;
    }
    Ok(())
}

fn unlock_headless(cli: &Cli, mcp: bool) -> Result<()> {
    unlock_headless_options(cli.unlock_password_stdin, cli.unlock_password_fd, mcp)
}

fn unlock_headless_options(stdin: bool, fd: Option<u32>, mcp: bool) -> Result<()> {
    use std::io::Read;
    if mcp && stdin {
        return Err(color_eyre::eyre::eyre!(
            "MCP stdin is reserved for JSON-RPC. Pass a password through --unlock-password-fd N (an inherited descriptor >=3), not stdin."
        ));
    }
    let input: Option<Box<dyn Read>> = if stdin {
        if io::stdin().is_terminal() {
            return Err(color_eyre::eyre::eyre!("Redirect a password line into --unlock-password-stdin; terminal input would expose it."));
        }
        Some(Box::new(io::stdin()))
    } else if let Some(fd) = fd {
        if fd < 3 {
            return Err(color_eyre::eyre::eyre!(
                "--unlock-password-fd requires a dedicated inherited descriptor >=3"
            ));
        }
        #[cfg(unix)]
        {
            Some(Box::new(std::fs::File::open(format!("/dev/fd/{fd}"))?))
        }
        #[cfg(not(unix))]
        {
            return Err(color_eyre::eyre::eyre!(
                "Inherited password descriptors are unsupported on this platform"
            ));
        }
    } else {
        None
    };
    if let Some(input) = input {
        unlock_password_line(input)?;
    }
    protection::require_app().map_err(|error| color_eyre::eyre::eyre!(
        "{error}. Unlock this process with --unlock-password-stdin, or --unlock-password-fd N for MCP; never put a password in arguments or environment variables."
    ))
}

fn unlock_password_line(mut input: impl io::Read) -> Result<()> {
    let mut bytes = zeroize::Zeroizing::new(Vec::new());
    let mut byte = zeroize::Zeroizing::new([0_u8; 1]);
    while input.read(byte.as_mut())? != 0 {
        if byte[0] == b'\n' {
            break;
        }
        if bytes.len() >= 4096 {
            return Err(color_eyre::eyre::eyre!("Password line exceeds 4096 bytes"));
        }
        bytes.push(byte[0]);
    }
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    protection::unlock(std::str::from_utf8(bytes.as_slice())?)
}

fn run_offline_action(action: OfflineAction) -> Result<()> {
    protection::require_app()?;
    match action {
        OfflineAction::Decrypt {
            path,
            key_path,
            format,
        } => {
            let output = if path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value == "tcui-chat")
            {
                let document: crate::storage::chat_store::ChatDocument = read_cli_document(
                    &path,
                    key_path.as_deref(),
                    "chat",
                    protection::Domain::Chats,
                )?;
                export::render_chat_document(&document, format)?
            } else if path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value == "tcui-memory")
            {
                #[cfg(feature = "memory")]
                {
                    let document: crate::memory::MemoryDocument = read_cli_document(
                        &path,
                        key_path.as_deref(),
                        "memory",
                        protection::Domain::Memories,
                    )?;
                    export::render_memory_document(&document, format)?
                }
                #[cfg(not(feature = "memory"))]
                {
                    return Err(color_eyre::eyre::eyre!(
                        "memory document support is unavailable without the memory feature"
                    ));
                }
            } else {
                return Err(color_eyre::eyre::eyre!(
                    "unsupported encrypted document type: {}",
                    path.display()
                ));
            };
            print!("{output}");
            Ok(())
        }
        #[cfg(feature = "memory")]
        OfflineAction::AddMemory { source, key_path } => {
            require_headless_domain(protection::Domain::Memories)?;
            let store = open_memory_store(key_path.as_deref())?;
            let outcome = store.add_from_plaintext(&source)?;
            match outcome {
                crate::memory::WriteOutcome::Saved { path, .. } => {
                    let destination = store
                        .physical_path_for_logical_path(Path::new(&path))?
                        .ok_or_else(|| {
                            color_eyre::eyre::eyre!(
                                "created memory document could not be located after import"
                            )
                        })?;
                    println!(
                        "Created memory from {} and copied to: {}",
                        source.display(),
                        destination.display()
                    );
                }
                crate::memory::WriteOutcome::AlreadyKnown { title } => {
                    println!("Memory already exists: {title}");
                }
            }
            Ok(())
        }
        OfflineAction::ExportAll {
            key_path,
            format,
            chats,
            #[cfg(feature = "memory")]
            memories,
        } => {
            let current_dir = std::env::current_dir()?;
            if chats {
                export_all_chats(key_path.as_deref(), format, &current_dir.join("tcui-chats"))?;
            }
            #[cfg(feature = "memory")]
            if memories {
                export_all_memories(
                    key_path.as_deref(),
                    format,
                    &current_dir.join("tcui-memories"),
                )?;
            }
            Ok(())
        }
    }
}

fn require_headless_domain(domain: protection::Domain) -> Result<()> {
    protection::require(domain).map_err(|error| color_eyre::eyre::eyre!(
        "{error}. Supply the shared password with --unlock-password-stdin, or use --unlock-password-fd N for MCP."
    ))
}

fn read_cli_document<T: serde::de::DeserializeOwned>(
    path: &Path,
    key_path: Option<&Path>,
    kind: &str,
    domain: protection::Domain,
) -> Result<T> {
    protection::require_app()?;
    require_headless_domain(domain)?;
    if protection::configured()? {
        if key_path.is_some() {
            return Err(color_eyre::eyre::eyre!("--key only applies to legacy documents; configured protection uses the shared password"));
        }
        Ok(serde_json::from_slice(
            &protection::read_for_explicit_decryption(domain, path)?,
        )?)
    } else {
        let key = load_cli_key(key_path)?;
        Ok(crate::storage::crypto::read_encrypted_document(
            path, &key, kind,
        )?)
    }
}

fn load_cli_key(path: Option<&Path>) -> Result<crate::storage::crypto::SharedKey> {
    match path {
        Some(path) => Ok(crate::storage::crypto::SharedKey::load_from_path(path)?),
        None => Ok(crate::storage::crypto::SharedKey::load_or_create_default(
            &crate::storage::paths::TcuiDataPaths::discover(),
        )?
        .key),
    }
}

fn export_all_chats(
    key_path: Option<&Path>,
    format: export::OutputFormat,
    destination: &Path,
) -> Result<()> {
    protection::require_app()?;
    require_headless_domain(protection::Domain::Chats)?;
    if key_path.is_some() && protection::configured()? {
        return Err(color_eyre::eyre::eyre!("--key only applies to legacy documents; configured protection uses the shared password"));
    }
    std::fs::create_dir_all(destination)?;
    let storage = match key_path {
        Some(path) => {
            Storage::new_with_key(crate::storage::crypto::SharedKey::load_from_path(path)?)?
        }
        None => Storage::new()?,
    };
    for document in storage.list_all_chat_documents()? {
        let _ = export::export_chat_document_to_dir(&document, format, destination)?;
    }
    Ok(())
}

#[cfg(feature = "memory")]
fn open_memory_store(key_path: Option<&Path>) -> Result<crate::memory::MemoryStore> {
    protection::require_app()?;
    require_headless_domain(protection::Domain::Memories)?;
    match key_path {
        Some(path) => Ok(crate::memory::MemoryStore::open_default_with_key(
            crate::storage::crypto::SharedKey::load_from_path(path)?,
        )?),
        None => Ok(crate::memory::MemoryStore::open_default()?),
    }
}

#[cfg(feature = "memory")]
fn export_all_memories(
    key_path: Option<&Path>,
    format: export::OutputFormat,
    destination: &Path,
) -> Result<()> {
    protection::require_app()?;
    require_headless_domain(protection::Domain::Memories)?;
    let store = open_memory_store(key_path)?;
    std::fs::create_dir_all(destination)?;
    for (_, document) in store.active_documents()? {
        let _ = export::export_memory_document_to_dir(&document, format, destination)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Cli, Command, OfflineAction};
    use clap::Parser;

    #[cfg(unix)]
    #[test]
    fn auth_global_password_descriptor_unlocks_before_the_owned_store_gate() {
        use std::os::fd::AsRawFd;
        crate::app::tests::with_test_app("auth-global-password-fd", |_app| {
            crate::protection::configure(
                crate::protection::ProtectionPolicy {
                    app_password: true,
                    ..Default::default()
                },
                "auth-global-password",
            )
            .expect("protect entry");
            crate::protection::lock().unwrap();
            let cli = Cli::parse_from(["tcui", "auth", "--plaintext", "status", "openrouter"]);
            let Some(Command::Auth(auth)) = cli.command else {
                panic!("auth command")
            };
            let root = crate::storage::paths::TcuiDataPaths::discover().root;
            let path = root.join("fixture-password");
            std::fs::write(&path, "wrong password\n").unwrap();
            let password = std::fs::File::open(&path).unwrap();
            assert!(
                super::prepare_auth_unlock(&auth, false, Some(password.as_raw_fd() as u32))
                    .is_err()
            );
            assert!(crate::protection::require_app().is_err());
            drop(password);

            std::fs::write(&path, "auth-global-password\n").unwrap();
            let password = std::fs::File::open(&path).unwrap();
            super::prepare_auth_unlock(&auth, false, Some(password.as_raw_fd() as u32))
                .expect("global descriptor authenticates application gate");
            assert!(crate::protection::require_app().is_ok());
            for fd in 0..3 {
                assert!(super::prepare_auth_unlock(&auth, false, Some(fd)).is_err());
            }
            assert!(super::prepare_auth_unlock(&auth, false, Some(u32::MAX)).is_err());
            let external = Cli::parse_from(["tcui", "auth", "logout", "codex", "--external"]);
            let Some(Command::Auth(external)) = external.command else {
                panic!("auth command")
            };
            crate::protection::lock().unwrap();
            super::prepare_auth_unlock(&external, false, Some(0))
                .expect("external logout never accesses or unlocks TCUI stores");
            assert!(crate::protection::require_app().is_err());
        });
    }

    #[test]
    fn headless_password_reader_consumes_only_one_line_and_rejects_oversized_input() {
        crate::app::tests::with_test_app("auth-global-password-line", |_app| {
            crate::protection::configure(
                crate::protection::ProtectionPolicy {
                    app_password: true,
                    ..Default::default()
                },
                "auth-global-password",
            )
            .expect("protect entry");
            crate::protection::lock().unwrap();
            let mut input = std::io::Cursor::new(b"auth-global-password\r\ncallback-code\n");
            super::unlock_password_line(&mut input).expect("shared stdin password reader");
            assert!(crate::protection::require_app().is_ok());
            assert_eq!(
                &input.get_ref()[input.position() as usize..],
                b"callback-code\n"
            );
            crate::protection::lock().unwrap();
            assert!(super::unlock_password_line(std::io::Cursor::new(vec![b'x'; 4097])).is_err());
            assert!(crate::protection::require_app().is_err());
            assert!(super::unlock_headless_options(true, None, true).is_err());

            let cli =
                Cli::parse_from(["tcui", "auth", "--vault-password-stdin", "status", "codex"]);
            let Some(Command::Auth(auth)) = cli.command else {
                panic!("auth command")
            };
            assert!(super::prepare_auth_unlock(&auth, true, None).is_err());
            assert!(super::prepare_auth_unlock(&auth, false, Some(3)).is_err());
            assert!(crate::protection::require_app().is_err());
        });
    }

    #[test]
    fn offline_action_parses_decrypt_and_export_flags() {
        let decrypt = Cli::parse_from(["tcui", "--decrypt", "chat.tcui-chat", "--json"]);
        let export = Cli::parse_from(["tcui", "--export-all-chats", "--markdown"]);

        match OfflineAction::from_cli(&decrypt).expect("decrypt action") {
            Some(OfflineAction::Decrypt { format, .. }) => {
                assert!(matches!(format, crate::export::OutputFormat::Json));
            }
            _ => panic!("expected decrypt action"),
        }
        match OfflineAction::from_cli(&export).expect("export action") {
            Some(OfflineAction::ExportAll { chats, .. }) => assert!(chats),
            _ => panic!("expected export action"),
        }
    }

    #[test]
    fn cli_parses_upgrade_as_a_pre_tui_command() {
        let cli = Cli::try_parse_from(["tcui", "upgrade"]).expect("upgrade command parses");

        assert!(matches!(cli.command, Some(Command::Upgrade)));
    }

    #[test]
    fn cli_leaves_offline_actions_empty_for_a_pre_tui_command() {
        let cli = Cli::try_parse_from(["tcui", "upgrade"]).expect("upgrade command parses");

        assert!(OfflineAction::from_cli(&cli)
            .expect("upgrade does not conflict with offline actions")
            .is_none());
    }

    #[cfg(feature = "memory")]
    #[test]
    fn offline_action_parses_add_memory_and_rejects_mixed_modes() {
        let add = Cli::parse_from(["tcui", "--add-memory", "note.md"]);
        let mixed = Cli::parse_from([
            "tcui",
            "--decrypt",
            "chat.tcui-chat",
            "--add-memory",
            "note.md",
        ]);

        match OfflineAction::from_cli(&add).expect("add-memory action") {
            Some(OfflineAction::AddMemory { .. }) => {}
            _ => panic!("expected add-memory action"),
        }
        assert!(OfflineAction::from_cli(&mixed).is_err());
    }

    #[cfg(feature = "memory")]
    #[test]
    fn cli_exposes_memory_mcp_subcommand_when_enabled() {
        let cli = Cli::parse_from(["tcui", "memory-mcp", "--unlock-password-fd", "3"]);
        assert!(matches!(cli.command, Some(Command::MemoryMcp)));
        assert_eq!(cli.unlock_password_fd, Some(3));
        assert!(Cli::try_parse_from(["tcui", "memory-mcp", "--vault", "/tmp/vault"]).is_err());
    }

    #[test]
    fn password_input_modes_are_explicit_and_mutually_exclusive() {
        let cli = Cli::try_parse_from(["tcui", "--export-all-chats", "--unlock-password-stdin"])
            .expect("stdin unlock");
        assert!(cli.unlock_password_stdin);
        assert!(Cli::try_parse_from([
            "tcui",
            "--unlock-password-stdin",
            "--unlock-password-fd",
            "3"
        ])
        .is_err());
    }

    #[test]
    fn filename_sanitizer_collapses_non_alphanumeric_runs() {
        assert_eq!(
            crate::export::sanitize_filename("Preferred editor / Helix"),
            "preferred-editor-helix"
        );
        assert_eq!(crate::export::sanitize_filename("###"), "memory");
    }
}
