pub mod dev;

use anyhow::Result;
use clap::{Args, Subcommand};

/// UI plugin development tooling
#[derive(Args)]
pub struct UiArgs {
    #[command(subcommand)]
    pub command: UiCommands,
}

#[derive(Subcommand)]
pub enum UiCommands {
    /// Start a local dev server for a UI plugin with live reload
    Dev(dev::DevArgs),
}

pub async fn run(args: UiArgs) -> Result<()> {
    match args.command {
        UiCommands::Dev(args) => dev::run(args).await,
    }
}
