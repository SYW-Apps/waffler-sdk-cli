pub mod init;

use anyhow::Result;
use clap::{Args, Subcommand};

/// Integration test scaffolding for Waffler packages
#[derive(Args)]
pub struct TestArgs {
    #[command(subcommand)]
    pub command: TestCommands,
}

#[derive(Subcommand)]
pub enum TestCommands {
    /// Scaffold integration tests for all capabilities in the current package
    Init(init::InitArgs),
}

pub async fn run(args: TestArgs) -> Result<()> {
    match args.command {
        TestCommands::Init(args) => init::run(args).await,
    }
}
