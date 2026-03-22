use anyhow::Result;
use console::style;

use crate::auth;

pub async fn run() -> Result<()> {
    match auth::load_credentials() {
        None => {
            println!(
                "{} Not logged in. Run `waffler login` to authenticate.",
                style("✗").red()
            );
        }
        Some(creds) => {
            let expired_label = if creds.is_expired() {
                format!(" {}", style("(token expired — run waffler login)").yellow())
            } else {
                String::new()
            };
            println!(
                "{} {}{}",
                style("Developer:").bold(),
                style(&creds.developer.username).cyan().bold(),
                expired_label
            );
            println!("  Email:     {}", creds.developer.email);

            if creds.developer.namespaces.is_empty() {
                println!(
                    "  Tags:      {}",
                    style("none — run `waffler namespace claim <tag>` to claim one").dim()
                );
            } else {
                let tags = creds
                    .developer
                    .namespaces
                    .iter()
                    .map(|t| format!("{}.*", t))
                    .collect::<Vec<_>>()
                    .join(", ");
                println!("  Tags:      {}", style(tags).yellow());
            }
        }
    }
    Ok(())
}
