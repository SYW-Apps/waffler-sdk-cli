use anyhow::Result;
use clap::Args;
use console::style;

const REPO_OWNER: &str = "SYW-Apps";
const REPO_NAME: &str = "waffler-sdk-cli";
const BIN_NAME: &str = "waffler";

#[derive(Args)]
pub struct UpdateArgs {
    /// Only check for a new version without downloading it
    #[arg(long)]
    pub check: bool,
}

pub async fn run(args: UpdateArgs) -> Result<()> {
    let current = self_update::cargo_crate_version!();

    println!(
        "{} Checking for updates (current: {})...",
        style("→").cyan().bold(),
        style(current).dim()
    );

    let release = tokio::task::spawn_blocking(|| {
        self_update::backends::github::Update::configure()
            .repo_owner(REPO_OWNER)
            .repo_name(REPO_NAME)
            .bin_name(BIN_NAME)
            .current_version(self_update::cargo_crate_version!())
            .build()
            .and_then(|u| u.get_latest_release())
    })
    .await??;

    let latest = &release.version;

    if self_update::version::bump_is_greater(current, latest)? {
        println!(
            "  {} New version available: {}",
            style("↑").yellow().bold(),
            style(latest).cyan().bold()
        );

        if args.check {
            println!(
                "  {} Run {} to install it.",
                style("hint:").dim(),
                style("waffler update").white().bold()
            );
            return Ok(());
        }

        println!("  {} Downloading and installing...", style("→").cyan());

        tokio::task::spawn_blocking(|| {
            self_update::backends::github::Update::configure()
                .repo_owner(REPO_OWNER)
                .repo_name(REPO_NAME)
                .bin_name(BIN_NAME)
                .show_download_progress(true)
                .current_version(self_update::cargo_crate_version!())
                .build()?
                .update()
        })
        .await??;

        println!(
            "\n{} Updated to {} — restart the CLI to use the new version.",
            style("✓").green().bold(),
            style(latest).cyan().bold()
        );
    } else {
        println!(
            "  {} Already up to date ({})",
            style("✓").green().bold(),
            style(current).cyan()
        );
    }

    Ok(())
}
