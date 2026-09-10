//! `sdk_cli::build-adapter` — running the toolchain a project declares, and asking it where its
//! output went.
//!
//! Spec: `sdk_cli::build-adapter` / `ibuild-adapter` / `build_adapter_impl`.
//!
//! THE INNER TOOL'S DIAGNOSTICS PASS THROUGH UNCHANGED. It names a file, a line and a type; any
//! sentence substituted for that is strictly less useful to whoever has to fix it. Exit status
//! decides success; stdout and stderr pass through.
//!
//! ## THE BUILD TOOL IS THE AUTHORITY ON ITS OWN OUTPUT
//!
//! [`built_artifact_path`] asks rather than assumes, and that is not a convenience. A manifest
//! declaring `target/release/libfoo.so` is asserting two things the build tool owns:
//!
//!   * where the output directory is — moved by `CARGO_TARGET_DIR` (our own container build sets it
//!     to a cache mount), by `build.target-dir`, and by a workspace's shared target directory;
//!   * what the file is called — `libfoo.so`, `foo.dll` or `libfoo.dylib` by platform.
//!
//! Both were wrong the first time this was run for real, and the failure was a pack refusing a path
//! nobody had typed. Asking is ONE authoritative answer where a fallback list would be a pile of
//! guesses, and it is what makes one manifest pack on three platforms.
//!
//! NO TECHNOLOGY SEAM IS DECLARED, and that is a judgement rather than an omission. A technology
//! declaration fences a swappable vendor backend consumers must not know about. The Rust toolchain is
//! not that: a package project in this ecosystem IS a crate, its manifest is part of what a developer
//! edits and what a scaffold writes, and there is nothing to swap to.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use super::types::BuildReport;

/// The only profile a publishable bundle may be built in.
///
/// RELEASE IS NOT A PREFERENCE. A debug module exceeds core's inline custody cap, so a debug artifact
/// does not merely run slower — it produces a bundle a node refuses with a message about CUSTODY
/// rather than about the profile, on someone else's machine, naming nothing that points back here.
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
    // progress to stdout, and a report carrying only one of them is missing either the errors or the
    // context they happened in.
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));

    Ok(BuildReport {
        ran: true,
        crate_manifest: Some(manifest_path.display().to_string()),
        profile: PROFILE.to_string(),
        // EXIT STATUS DECIDES, not output parsing. A build whose success is inferred from the absence
        // of the word "error" reports success for a toolchain that changed its wording.
        succeeded: output.status.success(),
        output: text,
    })
}

/// The report for a run that built nothing because it was told not to.
///
/// A DISTINCT CONSTRUCTOR RATHER THAN A DEFAULT, so `ran: false` is something a caller states rather
/// than something it forgets to set. "It packed the wrong binary" and "it packed a binary it did not
/// build" are the same incident seen a day apart, and the flag is what separates them.
pub fn skipped() -> BuildReport {
    BuildReport {
        ran: false,
        crate_manifest: None,
        profile: PROFILE.to_string(),
        succeeded: true,
        output: String::new(),
    }
}

/// What the crate's metadata says about its output: where the target directory is, and what the
/// loadable library target is called.
#[derive(Debug, Clone)]
pub struct CrateOutput {
    /// The target directory the toolchain actually uses, whatever moved it.
    pub target_directory: PathBuf,
    /// The library target's name, before platform decoration.
    pub library_name: String,
}

/// Ask the toolchain about a crate, without building it.
///
/// `cargo metadata` is a metadata read: it resolves the manifest, the workspace and the effective
/// target directory, and compiles nothing. That is what lets `pack --no-build` locate an artifact
/// something else already produced, without either rebuilding it or guessing where it went.
pub fn crate_output(manifest_path: &Path) -> Result<CrateOutput> {
    let output = std::process::Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1", "--manifest-path"])
        .arg(manifest_path)
        .output()
        .with_context(|| format!("could not ask the toolchain about {}", manifest_path.display()))?;

    if !output.status.success() {
        bail!(
            "the toolchain could not read {}:\n{}",
            manifest_path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("the toolchain's metadata did not decode")?;

    let target_directory = metadata
        .get("target_directory")
        .and_then(|v| v.as_str())
        .context("the toolchain's metadata names no target directory")?;

    // The library target, found by KIND rather than by position. A crate may declare binaries,
    // examples and tests alongside its library, and `targets[0]` is whichever the toolchain happened
    // to list first.
    let packages = metadata.get("packages").and_then(|v| v.as_array()).map(Vec::as_slice).unwrap_or(&[]);
    let mut library_names: Vec<String> = Vec::new();
    for package in packages {
        for target in package.get("targets").and_then(|v| v.as_array()).map(Vec::as_slice).unwrap_or(&[]) {
            let kinds = target.get("kind").and_then(|v| v.as_array()).map(Vec::as_slice).unwrap_or(&[]);
            let loadable = kinds.iter().any(|k| matches!(k.as_str(), Some("cdylib") | Some("dylib")));
            if loadable {
                if let Some(name) = target.get("name").and_then(|v| v.as_str()) {
                    library_names.push(name.to_string());
                }
            }
        }
    }

    // AMBIGUITY IS REFUSED RATHER THAN RESOLVED BY PREFERENCE, like every other ambiguity in this
    // tool. Two loadable libraries means the manifest has to say which, and picking one silently packs
    // an artifact the developer did not choose.
    if library_names.len() > 1 {
        bail!(
            "{} declares {} loadable library targets ({}). A `fromBuild` artifact cannot say which; \
             declare the artifact by `path` instead.",
            manifest_path.display(),
            library_names.len(),
            library_names.join(", ")
        );
    }
    let library_name = library_names.into_iter().next().with_context(|| {
        format!(
            "{} declares no loadable library target. A Waffler package's module is a `cdylib`; add \
             `crate-type = [\"cdylib\", \"rlib\"]` to its `[lib]` section.",
            manifest_path.display()
        )
    })?;

    Ok(CrateOutput { target_directory: PathBuf::from(target_directory), library_name })
}

/// Where the build put the loadable module for this crate.
///
/// THE PLATFORM DECORATION IS THE ONE DERIVED PART, and it is derived from the platform this tool is
/// running on — which is the platform the build ran on, because the build is a child process of this
/// one. Cross-compilation is not supported, and a `--target` build would put its output under a
/// triple-named subdirectory this does not look in; the resulting refusal names the path, which is
/// the right failure for something not yet supported.
pub fn built_artifact_path(manifest_path: &Path) -> Result<PathBuf> {
    let output = crate_output(manifest_path)?;
    let stem = output.library_name.replace('-', "_");
    let file_name = if cfg!(target_os = "windows") {
        format!("{stem}.dll")
    } else if cfg!(target_os = "macos") {
        format!("lib{stem}.dylib")
    } else {
        format!("lib{stem}.so")
    };
    Ok(output.target_directory.join(PROFILE).join(file_name))
}
