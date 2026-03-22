use anyhow::{bail, Result};
use clap::Args;
use console::style;
use std::path::PathBuf;

use crate::commands::load_manifest;

#[derive(Args)]
pub struct BuildArgs {
    /// Package directory (defaults to current directory)
    #[arg(default_value = ".")]
    pub path: PathBuf,

    /// Build in release mode (default: debug)
    #[arg(long, short)]
    pub release: bool,
}

pub async fn run(args: BuildArgs) -> Result<()> {
    let manifest = load_manifest(&args.path)?;
    let language = manifest
        .build
        .as_ref()
        .map(|b| b.language.as_str())
        .unwrap_or("rust");

    println!(
        "{} Building {} ({}) ...",
        style("→").cyan().bold(),
        style(&manifest.display_name).bold(),
        language
    );

    match language {
        "rust" => build_rust(
            &args.path,
            &manifest.id,
            manifest.features.native_module,
            args.release,
        )?,
        "node" => build_node(&args.path)?,
        "python" => build_python(&args.path)?,
        "waffler_native" => {
            println!(
                "  {} Entities-only package — nothing to compile.",
                style("i").cyan()
            );
        }
        other => bail!(
            "Unsupported build language: '{}'. Check the `build.language` field in package.json.",
            other
        ),
    }

    println!("{} Build complete.", style("✓").green().bold());
    Ok(())
}

pub fn build_rust(
    pkg_dir: &std::path::Path,
    crate_name: &str,
    is_wasm: bool,
    release: bool,
) -> Result<()> {
    let mut cmd = std::process::Command::new("cargo");
    cmd.arg("build").arg("-p").arg(crate_name);

    if is_wasm {
        cmd.arg("--target").arg("wasm32-unknown-unknown");
    }
    if release {
        cmd.arg("--release");
    }
    cmd.current_dir(pkg_dir);

    let status = cmd.status()?;
    if !status.success() {
        bail!("cargo build failed (exit {})", status.code().unwrap_or(-1));
    }
    Ok(())
}

pub fn build_node(pkg_dir: &std::path::Path) -> Result<()> {
    // Look for app/, webapp/, or ui/ subdirectory
    let candidates = ["app", "webapp", "ui", "web"];
    let app_dir = candidates
        .iter()
        .map(|d| pkg_dir.join(d))
        .find(|p| p.exists())
        .unwrap_or_else(|| pkg_dir.to_path_buf());

    let npm_install = std::process::Command::new("npm")
        .args(["install"])
        .current_dir(&app_dir)
        .status()?;
    if !npm_install.success() {
        bail!("npm install failed");
    }

    let npm_build = std::process::Command::new("npm")
        .args(["run", "build"])
        .current_dir(&app_dir)
        .status()?;
    if !npm_build.success() {
        bail!("npm run build failed");
    }
    Ok(())
}

pub fn build_python(_pkg_dir: &std::path::Path) -> Result<()> {
    // Python packages are typically distributed as scripts; no compilation step.
    println!(
        "  {} Python packages: no compilation step (scripts are bundled as-is).",
        style("i").cyan()
    );
    Ok(())
}
