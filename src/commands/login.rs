use anyhow::Result;
use clap::Args;
use console::style;

use crate::auth;

#[derive(Args)]
pub struct LoginArgs {}

pub async fn run(_args: LoginArgs) -> Result<()> {
    // Already logged in?
    if let Some(creds) = auth::load_credentials() {
        if !creds.is_expired() {
            println!(
                "{} Already logged in as {} ({})",
                style("✓").green().bold(),
                style(&creds.developer.username).cyan(),
                creds.developer.email
            );
            if !creds.developer.namespaces.is_empty() {
                let tags = creds.developer.namespaces.join(", ");
                println!("  Tags: {}", style(tags).yellow());
            }
            println!();
            println!("  Run `waffler logout` first to switch accounts.");
            return Ok(());
        }
    }

    println!(
        "{} Authenticating with Waffler Developer Portal...",
        style("→").cyan().bold()
    );

    let client = reqwest::Client::new();
    let creds = auth::login(&client).await?;

    auth::save_credentials(&creds)?;

    println!(
        "\n{} Logged in as {} ({})",
        style("✓").green().bold(),
        style(&creds.developer.username).cyan().bold(),
        creds.developer.email
    );

    if creds.developer.namespaces.is_empty() {
        println!(
            "  {} No tags claimed. Run {} to get started.",
            style("⚠").yellow(),
            style("waffler namespace claim <tag>").cyan()
        );
    } else {
        let tags = creds
            .developer
            .namespaces
            .iter()
            .map(|t| format!("{}.*", t))
            .collect::<Vec<_>>()
            .join(", ");
        println!("  Tags: {}", style(tags).yellow().bold());
    }

    Ok(())
}
