use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::{config, update};

#[derive(Debug, Parser)]
#[command(name = "auranion", version, about = "Configure Auranion integrations")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Config {
        /// Reapply saved integrations without interactive prompts.
        #[arg(long)]
        apply: bool,
    },
    Status,
    /// Update binary to latest release from GitHub.
    Update {
        /// Suppress progress output; used by the daily OS task.
        #[arg(long, hide = true)]
        background: bool,
    },
    /// Manage the daily OS update task.
    Schedule {
        #[command(subcommand)]
        command: ScheduleCommand,
    },
    #[command(hide = true)]
    ProviderToken,
}

#[derive(Debug, Subcommand)]
enum ScheduleCommand {
    Enable,
    Disable,
}

pub fn run() -> Result<()> {
    match Args::parse().command {
        Command::Config { apply } => {
            if apply {
                let _lock = update::config_lock()?;
                config::apply_saved()
            } else {
                {
                    let _lock = update::config_lock()?;
                    config::configure()?;
                }
                // Scheduling failure must not undo a successful integration setup.
                if let Err(error) = update::ensure_schedule(false) {
                    eprintln!(
                        "Auto-update setup failed: {error:#}. Retry `auranion schedule enable`."
                    );
                }
                Ok(())
            }
        }
        Command::Status => {
            // Status can recover an interrupted config transaction.
            let _lock = update::config_lock()?;
            config::status()
        }
        Command::Update { background } => update::run(background),
        Command::Schedule {
            command: ScheduleCommand::Enable,
        } => update::ensure_schedule(true),
        Command::Schedule {
            command: ScheduleCommand::Disable,
        } => update::disable_schedule(),
        Command::ProviderToken => config::print_provider_token(),
    }
}
