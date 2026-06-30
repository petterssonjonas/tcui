use std::io::{self, IsTerminal};

use clap::{Parser, Subcommand};
use color_eyre::Result;
use crossterm::{
    event::DisableFocusChange,
    event::DisableMouseCapture,
    event::EnableFocusChange,
    event::EnableMouseCapture,
    execute,
    terminal::{
        disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen, SetTitle,
    },
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::sync::Arc;
use tokio::sync::RwLock;

mod app;
mod config;
mod diagnostics;
mod event;
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
mod ui;

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
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
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
    }

    Ok(terminal)
}

fn restore_terminal() {
    let is_tty = interactive_terminal_available();

    if is_tty {
        let _ = execute!(io::stdout(), DisableFocusChange);
        let _ = execute!(io::stdout(), DisableMouseCapture);
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        let _ = disable_raw_mode();
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    if let Some(command) = Cli::parse().command {
        match command {
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
        let v = Vault::new(std::path::PathBuf::from(p));
        if v.exists() {
            Some(Arc::new(v))
        } else {
            None
        }
    });

    let mut app = TuiApp::new(
        Arc::new(storage),
        Arc::new(RwLock::new(config)),
        Arc::new(LlmClient::new()),
        vault,
    );

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
