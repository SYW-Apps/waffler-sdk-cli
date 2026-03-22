mod auth;
mod commands;
mod segment;
mod zip_builder;

use clap::{Parser, Subcommand};
use console::style;

#[derive(Parser)]
#[command(
    name = "waffler",
    about = "Waffler Package Developer SDK",
    long_about = "Build, pack, validate, and scaffold Waffler packages.\nAuthenticate with your developer account to publish under your namespace.",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Scaffold a new Waffler package project with an interactive wizard
    Scaffold(commands::scaffold::ScaffoldArgs),

    /// Build the package in the current (or specified) directory
    Build(commands::build::BuildArgs),

    /// Build and package into a distributable .zip archive
    Pack(commands::pack::PackArgs),

    /// Validate the package manifest and namespace structure
    Validate(commands::validate::ValidateArgs),

    /// Authenticate with your Waffler developer account
    Login(commands::login::LoginArgs),

    /// Log out and clear stored credentials
    Logout,

    /// Show the currently authenticated developer
    Whoami,

    /// Manage your developer namespace tags
    Namespace(commands::namespace::NamespaceArgs),

    /// Build, pack, and publish the package to the registry
    Publish(commands::publish::PublishArgs),

    /// Check for and install CLI updates from GitHub
    Update(commands::update::UpdateArgs),
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Scaffold(args) => commands::scaffold::run(args).await,
        Commands::Build(args) => commands::build::run(args).await,
        Commands::Pack(args) => commands::pack::run(args).await,
        Commands::Validate(args) => commands::validate::run(args).await,
        Commands::Login(args) => commands::login::run(args).await,
        Commands::Logout => commands::logout::run().await,
        Commands::Whoami => commands::whoami::run().await,
        Commands::Namespace(args) => commands::namespace::run(args).await,
        Commands::Publish(args) => commands::publish::run(args).await,
        Commands::Update(args) => commands::update::run(args).await,
    };

    if let Err(e) = result {
        eprintln!("{} {:#}", style("error:").red().bold(), e);
        std::process::exit(1);
    }
}
