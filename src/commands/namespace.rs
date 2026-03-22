use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use console::style;

use crate::auth;

#[derive(Args)]
pub struct NamespaceArgs {
    #[command(subcommand)]
    pub command: NamespaceCommands,
}

#[derive(Subcommand)]
pub enum NamespaceCommands {
    /// List your claimed namespace tags
    List {
        /// Re-fetch from the registry instead of using cached credentials
        #[arg(long)]
        refresh: bool,
    },

    /// Check whether a tag is available and who owns it
    Check {
        /// The tag to check (e.g. "acme")
        tag: String,
    },

    /// Claim a new namespace tag
    Claim {
        /// The tag to claim (e.g. "acme") — lowercase letters, digits, underscores; max 32 chars
        tag: String,
    },

    /// Release (unclaim) a namespace tag you own
    Release {
        /// The tag to release
        tag: String,

        /// Skip the confirmation prompt
        #[arg(long, short = 'y')]
        yes: bool,
    },
}

pub async fn run(args: NamespaceArgs) -> Result<()> {
    match args.command {
        NamespaceCommands::List { refresh } => cmd_list(refresh).await,
        NamespaceCommands::Check { tag } => cmd_check(tag).await,
        NamespaceCommands::Claim { tag } => cmd_claim(tag).await,
        NamespaceCommands::Release { tag, yes } => cmd_release(tag, yes).await,
    }
}

// ─── list ─────────────────────────────────────────────────────────────────────

async fn cmd_list(refresh: bool) -> Result<()> {
    let mut creds = require_login()?;

    if refresh {
        let client = reqwest::Client::new();
        match auth::fetch_profile(&client, &creds.access_token).await {
            Some(profile) => {
                creds.developer.namespaces = profile.namespaces;
                auth::save_credentials(&creds)?;
                println!("  {} Refreshed from registry.", style("✓").green());
            }
            None => {
                println!(
                    "  {} Could not reach registry — showing cached tags.",
                    style("⚠").yellow()
                );
            }
        }
    }

    if creds.developer.namespaces.is_empty() {
        println!(
            "{} No namespace tags claimed yet.",
            style("⚠").yellow().bold()
        );
        println!(
            "  Run {} to claim one.",
            style("waffler namespace claim <tag>").cyan()
        );
        return Ok(());
    }

    println!(
        "{} Namespace tags for {}:",
        style("◆").cyan().bold(),
        style(&creds.developer.username).cyan()
    );
    for tag in &creds.developer.namespaces {
        println!("  {} {}.*", style("·").dim(), style(tag).yellow().bold());
    }
    println!();
    println!("  These tags allow publishing packages under the matching namespace prefix.");

    Ok(())
}

// ─── check ────────────────────────────────────────────────────────────────────

async fn cmd_check(tag: String) -> Result<()> {
    validate_tag_format(&tag)?;

    println!(
        "{} Checking availability of tag {}...",
        style("→").cyan().bold(),
        style(&tag).yellow()
    );

    let client = reqwest::Client::new();
    let info = auth::check_namespace(&client, &tag).await?;

    if info.available {
        println!(
            "{} Tag {} is {} — run {} to claim it.",
            style("✓").green().bold(),
            style(&tag).yellow().bold(),
            style("available").green(),
            style(format!("waffler namespace claim {tag}")).cyan()
        );
    } else {
        println!(
            "{} Tag {} is {}.",
            style("✗").red().bold(),
            style(&tag).yellow().bold(),
            style("already claimed").red()
        );
        if let Some(owner) = info.owner {
            println!("  Owner: {}", style(owner).cyan());
        }
    }

    Ok(())
}

// ─── claim ────────────────────────────────────────────────────────────────────

async fn cmd_claim(tag: String) -> Result<()> {
    validate_tag_format(&tag)?;

    let mut creds = require_login()?;
    if creds.is_expired() {
        bail!("Auth token expired. Run `waffler login` to refresh.");
    }

    if creds.developer.namespaces.contains(&tag) {
        println!(
            "{} You already own tag {}.",
            style("✓").green().bold(),
            style(&tag).yellow().bold()
        );
        return Ok(());
    }

    println!(
        "{} Claiming tag {}...",
        style("→").cyan().bold(),
        style(&tag).yellow()
    );

    let client = reqwest::Client::new();
    let namespaces = auth::claim_namespace(&client, &creds.access_token, &tag).await?;

    creds.developer.namespaces = namespaces;
    auth::save_credentials(&creds)?;

    println!(
        "{} Tag {} claimed successfully!",
        style("✓").green().bold(),
        style(&tag).yellow().bold()
    );
    println!(
        "  You can now publish packages under {} and its sub-namespaces.",
        style(format!("{tag}.*")).yellow()
    );

    Ok(())
}

// ─── release ──────────────────────────────────────────────────────────────────

async fn cmd_release(tag: String, yes: bool) -> Result<()> {
    validate_tag_format(&tag)?;

    let mut creds = require_login()?;
    if creds.is_expired() {
        bail!("Auth token expired. Run `waffler login` to refresh.");
    }

    if !creds.developer.namespaces.contains(&tag) {
        bail!("You don't own tag '{tag}'.");
    }

    if !yes {
        use dialoguer::Confirm;
        let confirmed = Confirm::new()
            .with_prompt(format!(
                "Release tag '{tag}'? Any packages published under {tag}.* will remain but you will lose the ability to publish new ones."
            ))
            .default(false)
            .interact()?;
        if !confirmed {
            println!("Aborted.");
            return Ok(());
        }
    }

    let client = reqwest::Client::new();
    let namespaces = auth::release_namespace(&client, &creds.access_token, &tag).await?;

    creds.developer.namespaces = namespaces;
    auth::save_credentials(&creds)?;

    println!(
        "{} Tag {} released.",
        style("✓").green().bold(),
        style(&tag).yellow().bold()
    );

    Ok(())
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn require_login() -> Result<auth::Credentials> {
    auth::load_credentials()
        .ok_or_else(|| anyhow::anyhow!("Not logged in. Run `waffler login` first."))
}

fn validate_tag_format(tag: &str) -> Result<()> {
    if tag.is_empty() {
        bail!("Tag cannot be empty.");
    }
    if tag.len() > 32 {
        bail!("Tag must be 32 characters or fewer.");
    }
    let valid = tag.chars().enumerate().all(|(i, c)| {
        if i == 0 {
            c.is_ascii_lowercase()
        } else {
            c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'
        }
    });
    if !valid {
        bail!(
            "Tag '{tag}' is invalid. Tags must start with a lowercase letter and contain only \
             lowercase letters, digits, and underscores (e.g. 'acme', 'my_org')."
        );
    }
    Ok(())
}
