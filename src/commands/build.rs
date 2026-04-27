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
        "rust" => {
            let crate_name = manifest
                .build
                .as_ref()
                .and_then(|b| b.crate_name.as_deref())
                .unwrap_or(&manifest.id);

            let is_wasm = manifest
                .get_runtime_variant()?
                .map(|v| v.is_wasm_related())
                .unwrap_or(false);

            build_rust(&args.path, crate_name, is_wasm, args.release)?;
        }
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

    // ─── Build UI Plugins ─────────────────────────────────────────────────────
    let ui_root = args.path.join(".ui");
    if ui_root.exists() {
        println!("  {} Building UI plugins in .ui/ ...", style("→").cyan());
        let entries = std::fs::read_dir(&ui_root)?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && path.join("package.json").exists() {
                let name = entry.file_name().to_string_lossy().to_string();
                println!("    {} Building UI plugin: {} ...", style("→").cyan(), name);
                
                let npm_install = std::process::Command::new("npm")
                    .args(["install"])
                    .current_dir(&path)
                    .status()?;
                if !npm_install.success() {
                    bail!("npm install failed for UI plugin: {}", name);
                }

                let npm_build = std::process::Command::new("npm")
                    .args(["run", "build"])
                    .current_dir(&path)
                    .status()?;
                if !npm_build.success() {
                    bail!("npm build failed for UI plugin: {}", name);
                }
            }
        }
    }

    println!("{} Build complete.", style("✓").green().bold());
    Ok(())
}

/// Returns true if `name` is a valid Cargo package name.
/// Cargo requires: ASCII letters, digits, `-`, or `_`; must not start with a digit.
fn is_valid_cargo_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let mut chars = name.chars();
    let first = chars.next().unwrap();
    if first.is_ascii_digit() {
        return false;
    }
    std::iter::once(first)
        .chain(chars)
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

pub fn build_rust(
    pkg_dir: &std::path::Path,
    crate_name: &str,
    is_wasm: bool,
    release: bool,
) -> Result<()> {
    if !is_valid_cargo_name(crate_name) {
        let suggestion = crate_name.replace('.', "_");
        bail!(
            "Invalid Cargo package name: '{crate_name}'\n\
             \n\
             Cargo only allows ASCII letters, digits, '-', and '_' in package names.\n\
             Waffler package ids may contain dots, but the underlying Cargo crate name cannot.\n\
             \n\
             Fix: add a `crate_name` field to the `build` section of your package.json:\n\
             \n\
             \"build\": {{ \"language\": \"rust\", \"crate_name\": \"{suggestion}\" }}\n\
             \n\
             The value must match the `name` in your Cargo.toml [package] section."
        );
    }

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
