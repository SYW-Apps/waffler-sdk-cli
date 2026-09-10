//! `sdk_cli::build-adapter` — running the toolchain a project declares.
//!
//! Spec: `sdk_cli::build-adapter` / `ibuild-adapter` / `build_adapter_impl`.
//!
//! THE INNER TOOL'S DIAGNOSTICS PASS THROUGH UNCHANGED. It names a file, a line and a type; any
//! sentence substituted for that is strictly less useful to whoever has to fix it. Exit status
//! decides success; stdout and stderr pass through.
//!
//! NO TECHNOLOGY SEAM IS DECLARED HERE, and that is a judgement rather than an omission. A
//! technology declaration fences a swappable vendor backend consumers must not know about. The Rust
//! toolchain is not that: a package project in this ecosystem IS a crate, its manifest is part of
//! what a developer edits and what a scaffold writes, and there is nothing to swap to.

use std::path::Path;

use anyhow::{Context, Result};

use super::types::BuildReport;

/// The only profile a publishable bundle may be built in.
///
/// RELEASE IS NOT A PREFERENCE. A debug module exceeds core's inline custody cap, so a debug
/// artifact does not merely run slower — it produces a bundle a node refuses with a message about
/// CUSTODY rather than about the profile, on someone else's machine, naming nothing that points
/// back here.
pub const PROFILE: &str = "release";

/// Build a Rust crate in release profile and return what the toolchain printed.
pub fn build_rust_crate(manifest_path: &Path) -> Result<BuildReport> {
    let output = std::process::Command::new("cargo")
        .args(["build", "--release", "--manifest-path"])
        .arg(manifest_path)
        .output()
        .with_context(|| {
            format!(
                "could not run the Rust toolchain to build {}. Is cargo on PATH?",
                manifest_path.display()
            )
        })?;

    // BOTH STREAMS, IN THE ORDER A TERMINAL WOULD HAVE SHOWN THEM. Diagnostics go to stderr and
    // progress to stdout, and a report carrying only one of them is a report missing either the
    // errors or the context they happened in.
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));

    Ok(BuildReport {
        ran: true,
        crate_manifest: Some(manifest_path.display().to_string()),
        profile: PROFILE.to_string(),
        // EXIT STATUS DECIDES, not output parsing. A build whose success is inferred from the
        // absence of the word "error" is one that reports success for a toolchain that changed its
        // wording.
        succeeded: output.status.success(),
        output: text,
    })
}

/// The report for a run that built nothing because it was told not to.
///
/// A DISTINCT CONSTRUCTOR RATHER THAN A DEFAULT, so `ran: false` is something a caller states
/// rather than something it forgets to set. "It packed the wrong binary" and "it packed a binary it
/// did not build" are the same incident seen a day apart, and the flag is what separates them.
pub fn skipped() -> BuildReport {
    BuildReport {
        ran: false,
        crate_manifest: None,
        profile: PROFILE.to_string(),
        succeeded: true,
        output: String::new(),
    }
}
