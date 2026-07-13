use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use color_eyre::Result;
use crossterm::{
    cursor::SetCursorStyle,
    event::{
        DisableFocusChange, DisableMouseCapture, EnableFocusChange, EnableMouseCapture,
        KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{
        EnterAlternateScreen, LeaveAlternateScreen, SetTitle, disable_raw_mode, enable_raw_mode,
    },
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::sync::Arc;
use tokio::sync::RwLock;

mod app;
mod auth_command;
mod config;
mod diagnostics;
mod event;
mod export;
mod llm;
mod mcp;
#[cfg(feature = "memory")]
mod memory;
mod notifications;
mod obsidian;
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

use app::TuiApp;
use config::AppConfig;
use llm::LlmClient;
use obsidian::Vault;
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
    #[cfg(feature = "memory")]
    MemoryMcp {
        #[arg(long)]
        vault: Option<std::path::PathBuf>,
    },
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
        let _ = execute!(terminal.backend_mut(), SetCursorStyle::SteadyBlock);
        let _ = execute!(
            terminal.backend_mut(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::REPORT_EVENT_TYPES)
        );
    }

    Ok(terminal)
}

fn restore_terminal() {
    let is_tty = interactive_terminal_available();

    if is_tty {
        let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
        let _ = execute!(io::stdout(), DisableFocusChange);
        let _ = execute!(io::stdout(), DisableMouseCapture);
        let _ = execute!(io::stdout(), SetCursorStyle::DefaultUserShape);
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        let _ = disable_raw_mode();
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    if let Some(operation) = OfflineAction::from_cli(&cli)? {
        return run_offline_action(operation);
    }
    if cli.upgrade {
        println!("{}", updater::upgrade_to_latest().await?);
        return Ok(());
    }
    if let Some(command) = cli.command {
        match command {
            Command::Auth(auth) => return auth_command::run(auth).await,
            Command::Upgrade => {
                println!("{}", updater::upgrade_to_latest().await?);
                return Ok(());
            }
            Command::ReminderDispatch { id } => {
                return reminders::dispatch(&AppConfig::load()?, &id).await;
            }
            #[cfg(feature = "memory")]
            Command::MemoryMcp { vault } => {
                return memory::run_mcp(vault).await;
            }
        }
    }

    if !interactive_terminal_available() {
        println!("TermChatUI requires an interactive terminal.");
        return Ok(());
    }

    let mut terminal = setup_terminal()?;
    restore_on_drop(&mut terminal);
    ui::components::terminal_capabilities::initialize_terminal_profile();

    let storage = Storage::new()?;
    let config = AppConfig::load()?;

    let vault = config.vault_path.as_ref().and_then(|p| {
        let expanded = crate::app::generated_file::expand_user_path(
            std::path::Path::new(p),
            dirs::home_dir().as_deref(),
        );
        let v = Vault::new(expanded);
        if v.exists() { Some(Arc::new(v)) } else { None }
    });

    let mut app = TuiApp::new(
        storage,
        Arc::new(RwLock::new(config)),
        Arc::new(LlmClient::new()),
        vault,
    );
    app.queue_update_check();

    app.ui
        .show_toast("Welcome to TCUI.\nType /help for instructions.".to_string());

    app.run(&mut terminal).await?;

    drop(app);
    restore_terminal();
    Ok(())
}

fn restore_on_drop(_terminal: &mut TerminalType) {
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

fn run_offline_action(action: OfflineAction) -> Result<()> {
    match action {
        OfflineAction::Decrypt {
            path,
            key_path,
            format,
        } => {
            let key = load_cli_key(key_path.as_deref())?;
            let output = if path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value == "tcui-chat")
            {
                let document: crate::storage::chat_store::ChatDocument =
                    crate::storage::crypto::read_encrypted_document(&path, &key, "chat")?;
                export::render_chat_document(&document, format)?
            } else if path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value == "tcui-memory")
            {
                #[cfg(feature = "memory")]
                {
                    let document: crate::memory::MemoryDocument =
                        crate::storage::crypto::read_encrypted_document(&path, &key, "memory")?;
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
            let config = AppConfig::load()?;
            let vault = config
                .vault_path
                .as_deref()
                .map(Path::new)
                .ok_or_else(|| color_eyre::eyre::eyre!("Obsidian vault is not configured"))?;
            let store = open_memory_store(vault, key_path.as_deref())?;
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
                let config = AppConfig::load()?;
                let vault =
                    config.vault_path.as_deref().map(Path::new).ok_or_else(|| {
                        color_eyre::eyre::eyre!("Obsidian vault is not configured")
                    })?;
                export_all_memories(
                    vault,
                    key_path.as_deref(),
                    format,
                    &current_dir.join("tcui-memories"),
                )?;
            }
            Ok(())
        }
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
fn open_memory_store(vault: &Path, key_path: Option<&Path>) -> Result<crate::memory::MemoryStore> {
    match key_path {
        Some(path) => Ok(crate::memory::MemoryStore::open_with_key(
            vault,
            &crate::memory::MemoryStore::default_cache_path(),
            crate::storage::crypto::SharedKey::load_from_path(path)?,
        )?),
        None => Ok(crate::memory::MemoryStore::open(
            vault,
            &crate::memory::MemoryStore::default_cache_path(),
        )?),
    }
}

#[cfg(feature = "memory")]
fn export_all_memories(
    vault: &Path,
    key_path: Option<&Path>,
    format: export::OutputFormat,
    destination: &Path,
) -> Result<()> {
    std::fs::create_dir_all(destination)?;
    let store = open_memory_store(vault, key_path)?;
    for (_, document) in store.active_documents()? {
        let _ = export::export_memory_document_to_dir(&document, format, destination)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Cli, Command, OfflineAction};
    use clap::Parser;
    #[cfg(feature = "memory")]
    use std::path::PathBuf;

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

        assert!(
            OfflineAction::from_cli(&cli)
                .expect("upgrade does not conflict with offline actions")
                .is_none()
        );
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
        let cli = Cli::parse_from(["tcui", "memory-mcp", "--vault", "/tmp/vault"]);

        assert!(matches!(
            cli.command,
            Some(Command::MemoryMcp { vault: Some(path) }) if path == PathBuf::from("/tmp/vault")
        ));
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
