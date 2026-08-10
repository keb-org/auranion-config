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
    Update,
}

pub fn run() -> Result<()> {
    match Args::parse().command {
        Command::Config { apply: true } => config::apply_saved(),
        Command::Config { apply: false } => config::configure(),
        Command::Status => config::status(),
        Command::Update => update::run(),
    }
}
