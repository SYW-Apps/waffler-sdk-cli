use anyhow::Result;
use console::style;

use crate::auth;

pub async fn run() -> Result<()> {
    match auth::load_credentials() {
        None => {
            println!("Not currently logged in.");
        }
        Some(creds) => {
            auth::clear_credentials()?;
            println!(
                "{} Logged out {}",
                style("✓").green().bold(),
                style(&creds.developer.username).cyan()
            );
        }
    }
    Ok(())
}
